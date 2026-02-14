use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use warp::{Filter, Reply};

const DEFAULT_MAX_SOURCE_BYTES: usize = 64 * 1024;
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_OPENROUTER_APP_TITLE: &str = "massive_game_server";

#[derive(Clone)]
pub struct CodeGenerationService {
    inner: Arc<CodeGenerationInner>,
}

struct CodeGenerationInner {
    client: reqwest::Client,
    openrouter_api_key: Option<String>,
    openrouter_base_url: String,
    openrouter_app_title: String,
    openrouter_http_referrer: Option<String>,
    max_source_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct ValidateBotCodeBody {
    source_code: String,
    language: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenerateBotCodeBody {
    model: String,
    objective: Option<String>,
    prompt_style: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CodeValidationResponse {
    valid: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GenerateBotCodeResponse {
    model: String,
    provider: String,
    prompt_style: String,
    objective: String,
    source_code: String,
    simulated: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ApiResponse<T>
where
    T: Serialize,
{
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiErrorBody>,
}

#[derive(Debug, Serialize)]
struct OpenRouterChatRequest {
    model: String,
    messages: Vec<OpenRouterMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct OpenRouterMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChatResponse {
    choices: Vec<OpenRouterChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponseMessage {
    content: serde_json::Value,
}

impl CodeGenerationService {
    pub fn new_from_env() -> Self {
        let openrouter_api_key = std::env::var("OPENROUTER_API_KEY")
            .ok()
            .map(|raw| raw.trim().to_owned())
            .filter(|raw| !raw.is_empty());
        let openrouter_base_url = std::env::var("OPENROUTER_BASE_URL")
            .ok()
            .map(|raw| raw.trim().trim_end_matches('/').to_owned())
            .filter(|raw| !raw.is_empty())
            .unwrap_or_else(|| DEFAULT_OPENROUTER_BASE_URL.to_owned());
        let openrouter_app_title = std::env::var("OPENROUTER_APP_TITLE")
            .ok()
            .map(|raw| raw.trim().to_owned())
            .filter(|raw| !raw.is_empty())
            .unwrap_or_else(|| DEFAULT_OPENROUTER_APP_TITLE.to_owned());
        let openrouter_http_referrer = std::env::var("OPENROUTER_HTTP_REFERER")
            .ok()
            .map(|raw| raw.trim().to_owned())
            .filter(|raw| !raw.is_empty());
        let max_source_bytes = std::env::var("MGS_BOT_SOURCE_MAX_BYTES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|value| *value > 1024)
            .unwrap_or(DEFAULT_MAX_SOURCE_BYTES);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            inner: Arc::new(CodeGenerationInner {
                client,
                openrouter_api_key,
                openrouter_base_url,
                openrouter_app_title,
                openrouter_http_referrer,
                max_source_bytes,
            }),
        }
    }

    fn validate_source(&self, body: ValidateBotCodeBody) -> CodeValidationResponse {
        let language = body
            .language
            .as_deref()
            .unwrap_or("rust")
            .trim()
            .to_ascii_lowercase();

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let source = body.source_code;
        let source_len = source.len();
        if source_len == 0 {
            errors.push("source_code cannot be empty".to_owned());
            return CodeValidationResponse {
                valid: false,
                errors,
                warnings,
            };
        }
        if source_len > self.inner.max_source_bytes {
            errors.push(format!(
                "source_code exceeds max size ({} > {} bytes)",
                source_len, self.inner.max_source_bytes
            ));
        }

        let lowered = source.to_ascii_lowercase();
        let forbidden_patterns = [
            "unsafe",
            "std::fs",
            "std::process",
            "std::net",
            "libc::",
            "asm!(",
            "thread::spawn",
            "tokio::spawn",
            "extern \"c\"",
        ];
        for pattern in forbidden_patterns {
            if lowered.contains(pattern) {
                errors.push(format!("forbidden pattern detected: '{}'", pattern));
            }
        }

        if language == "rust" && !lowered.contains("fn bot_tick") {
            errors.push("required function `fn bot_tick` is missing".to_owned());
        }

        if !lowered.contains("const") {
            warnings.push("bot code has no constants; consider bounded config values".to_owned());
        }
        if !lowered.contains("match ") {
            warnings.push("bot code has no match-based decision branch".to_owned());
        }

        CodeValidationResponse {
            valid: errors.is_empty(),
            errors,
            warnings,
        }
    }

    async fn generate_bot_code(
        &self,
        body: GenerateBotCodeBody,
    ) -> Result<GenerateBotCodeResponse, ApiErrorBody> {
        let model = body.model.trim();
        if model.is_empty() {
            return Err(ApiErrorBody {
                code: "invalid_model",
                message: "model is required".to_owned(),
            });
        }

        let objective = body
            .objective
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("win 1v1 duel by balancing attack/defend")
            .to_owned();
        let prompt_style = body
            .prompt_style
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("balanced")
            .to_owned();

        let mut simulated = false;
        let mut warnings = Vec::new();
        let source_code = if let Some(api_key) = self.inner.openrouter_api_key.as_deref() {
            match self
                .generate_via_openrouter(api_key, model, &objective, &prompt_style)
                .await
            {
                Ok(source) => source,
                Err(err) => {
                    simulated = true;
                    warnings.push(format!(
                        "openrouter generation failed ({}); deterministic local template used",
                        err
                    ));
                    self.build_local_template(&objective, &prompt_style)
                }
            }
        } else {
            simulated = true;
            warnings.push(
                "OPENROUTER_API_KEY is not configured; deterministic local template used"
                    .to_owned(),
            );
            self.build_local_template(&objective, &prompt_style)
        };

        let validation = self.validate_source(ValidateBotCodeBody {
            source_code: source_code.clone(),
            language: Some("rust".to_owned()),
        });
        if !validation.valid {
            return Err(ApiErrorBody {
                code: "generated_code_invalid",
                message: format!("generated source failed validation: {}", validation.errors.join("; ")),
            });
        }
        warnings.extend(validation.warnings);

        Ok(GenerateBotCodeResponse {
            model: model.to_owned(),
            provider: "openrouter".to_owned(),
            prompt_style,
            objective,
            source_code,
            simulated,
            warnings,
        })
    }

    async fn generate_via_openrouter(
        &self,
        api_key: &str,
        model: &str,
        objective: &str,
        prompt_style: &str,
    ) -> Result<String, String> {
        let system_prompt = "You generate safe deterministic Rust bot code for a wasm arena. Return Rust code only.";
        let user_prompt = format!(
            "Generate Rust code with exactly one public function:\n\
pub fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32\n\
Action encoding: 0=idle, 1=attack, 2=defend, 3=charge.\n\
Constraints: no unsafe, no IO, no networking, no threads, no external crates.\n\
Style: {}.\nObjective: {}.\nReturn only code.",
            prompt_style, objective
        );

        let request = OpenRouterChatRequest {
            model: model.to_owned(),
            messages: vec![
                OpenRouterMessage {
                    role: "system",
                    content: system_prompt.to_owned(),
                },
                OpenRouterMessage {
                    role: "user",
                    content: user_prompt,
                },
            ],
            temperature: 0.2,
            max_tokens: 700,
        };

        let endpoint = format!("{}/chat/completions", self.inner.openrouter_base_url);
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {}", api_key);
        let auth = HeaderValue::from_str(&bearer).map_err(|err| err.to_string())?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(referrer) = self.inner.openrouter_http_referrer.as_deref() {
            if let Ok(value) = HeaderValue::from_str(referrer) {
                headers.insert("HTTP-Referer", value);
            }
        }
        if let Ok(value) = HeaderValue::from_str(&self.inner.openrouter_app_title) {
            headers.insert("X-Title", value);
        }

        let response = self
            .inner
            .client
            .post(endpoint)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|err| err.to_string())?;

        let status = response.status();
        let body = response.text().await.map_err(|err| err.to_string())?;
        if !status.is_success() {
            return Err(format!("http {}", status));
        }

        let parsed: OpenRouterChatResponse =
            serde_json::from_str(&body).map_err(|err| err.to_string())?;
        let content = parsed
            .choices
            .first()
            .and_then(|choice| extract_content_string(&choice.message.content))
            .ok_or_else(|| "empty content from provider".to_owned())?;
        let source = extract_rust_source(&content);
        if source.trim().is_empty() {
            return Err("provider returned empty source".to_owned());
        }
        Ok(source)
    }

    fn build_local_template(&self, objective: &str, prompt_style: &str) -> String {
        format!(
            "pub fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {{
    // objective: {}
    // style: {}
    if self_health < 25 {{
        return 2; // defend
    }}
    if enemy_health < 20 {{
        return 1; // finish with attack
    }}
    if (tick + self_score) % 11 == 0 {{
        return 3; // occasional charge
    }}
    1 // default attack pressure
}}",
            objective, prompt_style
        )
    }
}

fn extract_content_string(content: &serde_json::Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    if let Some(parts) = content.as_array() {
        let mut merged = String::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                merged.push_str(text);
            } else if let Some(text) = part.as_str() {
                merged.push_str(text);
            }
        }
        if !merged.is_empty() {
            return Some(merged);
        }
    }
    None
}

fn extract_rust_source(raw: &str) -> String {
    if let Some(start) = raw.find("```rust") {
        let rest = &raw[start + "```rust".len()..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim().to_owned();
        }
    }
    if let Some(start) = raw.find("```") {
        let rest = &raw[start + 3..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim().to_owned();
        }
    }
    raw.trim().to_owned()
}

fn ok_response<T>(data: T) -> warp::reply::Json
where
    T: Serialize,
{
    warp::reply::json(&ApiResponse {
        ok: true,
        data: Some(data),
        error: None::<ApiErrorBody>,
    })
}

fn error_response(code: &'static str, message: String) -> warp::reply::Json {
    warp::reply::json(&ApiResponse::<serde_json::Value> {
        ok: false,
        data: None,
        error: Some(ApiErrorBody { code, message }),
    })
}

fn with_service(
    service: CodeGenerationService,
) -> impl Filter<Extract = (CodeGenerationService,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || service.clone())
}

pub fn build_code_generation_routes(
    service: CodeGenerationService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    let validate = warp::path!("api" / "arena" / "code" / "validate")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(|body: ValidateBotCodeBody, service: CodeGenerationService| {
            ok_response(service.validate_source(body))
        });

    let generate = warp::path!("api" / "arena" / "code" / "generate")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service))
        .and_then(
            |body: GenerateBotCodeBody, service: CodeGenerationService| async move {
                let reply = match service.generate_bot_code(body).await {
                    Ok(response) => ok_response(response),
                    Err(err) => error_response(err.code, err.message),
                };
                Ok::<_, warp::Rejection>(reply)
            },
        );

    validate.or(generate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_rejects_unsafe_source() {
        let service = CodeGenerationService::new_from_env();
        let response = service.validate_source(ValidateBotCodeBody {
            source_code: "unsafe fn bot_tick() {}".to_owned(),
            language: Some("rust".to_owned()),
        });
        assert!(!response.valid);
        assert!(response.errors.iter().any(|line| line.contains("unsafe")));
    }

    #[test]
    fn extract_rust_source_from_fence() {
        let raw = "```rust\nfn bot_tick() -> i32 { 1 }\n```";
        assert!(extract_rust_source(raw).contains("fn bot_tick"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generator_returns_template_when_key_missing() {
        let service = CodeGenerationService::new_from_env();
        let response = service
            .generate_bot_code(GenerateBotCodeBody {
                model: "openai/gpt-4o".to_owned(),
                objective: Some("survive and trade efficiently".to_owned()),
                prompt_style: Some("aggressive".to_owned()),
            })
            .await
            .expect("generation should work");
        assert!(response.source_code.contains("fn bot_tick"));
    }
}
