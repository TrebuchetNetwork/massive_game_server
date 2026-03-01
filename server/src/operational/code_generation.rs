use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use warp::{Filter, Reply};
use wasmtime::{Config, Engine, ExternType, Module, ValType};

const DEFAULT_MAX_SOURCE_BYTES: usize = 64 * 1024;
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_OPENROUTER_APP_TITLE: &str = "massive_game_server";
const DEFAULT_ARENA_WASM_DIR: &str = "data/arena_bots";
const DEFAULT_ARENA_SOURCE_DIR: &str = "data/arena_sources";
const DEFAULT_OPENROUTER_MAX_TOKENS: u32 = 700;
const MAX_MODEL_ID_LEN: usize = 128;

fn read_env_secret(env_key: &str) -> Option<String> {
    if let Ok(raw) = std::env::var(env_key) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    let file_key = format!("{env_key}_FILE");
    let secret_file = std::env::var(file_key)
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())?;

    let file_contents = fs::read_to_string(secret_file).ok()?;
    let trimmed = file_contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

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
    source_dir: PathBuf,
    wasm_dir: PathBuf,
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

#[derive(Debug, Clone, Deserialize)]
struct GenerateAndCompileBotCodeBody {
    model_id: String,
    model: String,
    objective: Option<String>,
    prompt_style: Option<String>,
    overwrite: Option<bool>,
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
struct CompileBotCodeResponse {
    model_id: String,
    source_path: String,
    wasm_path: Option<String>,
    compiled: bool,
    bytes_written: usize,
    compiler_stdout: String,
    compiler_stderr: String,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GenerateAndCompileBotCodeResponse {
    generated: GenerateBotCodeResponse,
    compile: CompileBotCodeResponse,
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
        let openrouter_api_key = read_env_secret("OPENROUTER_API_KEY");
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
        let source_dir = std::env::var("MGS_ARENA_SOURCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ARENA_SOURCE_DIR));
        let wasm_dir = std::env::var("MGS_ARENA_WASM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ARENA_WASM_DIR));
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
                source_dir,
                wasm_dir,
            }),
        }
    }

    fn validate_source(&self, body: ValidateBotCodeBody) -> CodeValidationResponse {
        validate_source_impl(
            self.inner.max_source_bytes,
            &body.source_code,
            body.language.as_deref(),
        )
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

        let validation =
            validate_source_impl(self.inner.max_source_bytes, &source_code, Some("rust"));
        if !validation.valid {
            return Err(ApiErrorBody {
                code: "generated_code_invalid",
                message: format!(
                    "generated source failed validation: {}",
                    validation.errors.join("; ")
                ),
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

    async fn generate_and_compile(
        &self,
        body: GenerateAndCompileBotCodeBody,
    ) -> Result<GenerateAndCompileBotCodeResponse, ApiErrorBody> {
        let model_id = body.model_id.trim();
        if model_id.is_empty() {
            return Err(ApiErrorBody {
                code: "invalid_model_id",
                message: "model_id is required".to_owned(),
            });
        }

        let generated = self
            .generate_bot_code(GenerateBotCodeBody {
                model: body.model,
                objective: body.objective,
                prompt_style: body.prompt_style,
            })
            .await?;

        let compile = self
            .compile_generated_code_best_effort(
                model_id.to_owned(),
                generated.source_code.clone(),
                body.overwrite.unwrap_or(false),
            )
            .await;

        Ok(GenerateAndCompileBotCodeResponse { generated, compile })
    }

    async fn compile_generated_code_best_effort(
        &self,
        model_id: String,
        source_code: String,
        overwrite: bool,
    ) -> CompileBotCodeResponse {
        let max_source_bytes = self.inner.max_source_bytes;
        let source_dir = self.inner.source_dir.clone();
        let wasm_dir = self.inner.wasm_dir.clone();
        tokio::task::spawn_blocking(move || {
            compile_generated_code_impl(
                model_id,
                source_code,
                overwrite,
                max_source_bytes,
                source_dir,
                wasm_dir,
            )
        })
        .await
        .unwrap_or_else(|err| CompileBotCodeResponse {
            model_id: "unknown".to_owned(),
            source_path: String::new(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            compiler_stdout: String::new(),
            compiler_stderr: format!("compile task join error: {}", err),
            warnings: vec!["failed to run compile task".to_owned()],
        })
    }

    async fn generate_via_openrouter(
        &self,
        api_key: &str,
        model: &str,
        objective: &str,
        prompt_style: &str,
    ) -> Result<String, String> {
        let system_prompt =
            "You generate deterministic Rust bot code for a wasm arena. Return code only.";
        let user_prompt = format!(
            "Generate Rust with exactly one exported function:\n\
#[no_mangle]\npub extern \"C\" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32\n\
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
            max_tokens: DEFAULT_OPENROUTER_MAX_TOKENS,
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
            "#[no_mangle]\npub extern \"C\" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {{
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

fn compile_generated_code_impl(
    model_id: String,
    source_code: String,
    overwrite: bool,
    max_source_bytes: usize,
    source_dir: PathBuf,
    wasm_dir: PathBuf,
) -> CompileBotCodeResponse {
    let Some(safe_model_id) = sanitize_model_id(&model_id) else {
        return CompileBotCodeResponse {
            model_id,
            source_path: String::new(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            compiler_stdout: String::new(),
            compiler_stderr: "invalid model_id".to_owned(),
            warnings: vec!["model_id contains unsupported characters".to_owned()],
        };
    };

    let source_path = source_dir.join(format!("{}.rs", safe_model_id));
    let wasm_path = wasm_dir.join(format!("{}.wasm", safe_model_id));
    let mut warnings = Vec::new();

    if source_code.len() > max_source_bytes {
        warnings.push(format!(
            "source exceeds configured max ({} > {})",
            source_code.len(),
            max_source_bytes
        ));
    }

    if let Err(err) = fs::create_dir_all(&source_dir) {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            compiler_stdout: String::new(),
            compiler_stderr: format!("failed creating source dir: {}", err),
            warnings,
        };
    }
    if let Err(err) = fs::create_dir_all(&wasm_dir) {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            compiler_stdout: String::new(),
            compiler_stderr: format!("failed creating wasm dir: {}", err),
            warnings,
        };
    }
    if wasm_path.exists() && !overwrite {
        warnings.push("existing wasm kept because overwrite=false".to_owned());
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: Some(wasm_path.display().to_string()),
            compiled: false,
            bytes_written: 0,
            compiler_stdout: String::new(),
            compiler_stderr: "wasm already exists".to_owned(),
            warnings,
        };
    }

    if let Err(err) = fs::write(&source_path, source_code.as_bytes()) {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            compiler_stdout: String::new(),
            compiler_stderr: format!("failed writing source: {}", err),
            warnings,
        };
    }

    let output = Command::new("rustc")
        .arg("--edition=2021")
        .arg("--crate-type=cdylib")
        .arg("--target=wasm32-unknown-unknown")
        .arg("-O")
        .arg(&source_path)
        .arg("-o")
        .arg(&wasm_path)
        .output();

    let Ok(output) = output else {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            compiler_stdout: String::new(),
            compiler_stderr:
                "failed to execute rustc. Ensure rustc is installed and wasm target is available."
                    .to_owned(),
            warnings,
        };
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }

    if let Err(err) = validate_compiled_wasm_export(&wasm_path) {
        warnings.push(format!("compiled wasm failed export validation: {}", err));
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: Some(wasm_path.display().to_string()),
            compiled: false,
            bytes_written: 0,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }

    let bytes_written = fs::metadata(&wasm_path)
        .ok()
        .map(|meta| meta.len() as usize)
        .unwrap_or(0);
    CompileBotCodeResponse {
        model_id,
        source_path: source_path.display().to_string(),
        wasm_path: Some(wasm_path.display().to_string()),
        compiled: true,
        bytes_written,
        compiler_stdout: truncate_for_api(&stdout, 4000),
        compiler_stderr: truncate_for_api(&stderr, 4000),
        warnings,
    }
}

fn validate_compiled_wasm_export(path: &Path) -> Result<(), String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config).map_err(|err| err.to_string())?;
    let module = Module::from_file(&engine, path).map_err(|err| err.to_string())?;
    let Some(export) = module.get_export("bot_tick") else {
        return Err("missing 'bot_tick' export".to_owned());
    };
    let ExternType::Func(func) = export else {
        return Err("'bot_tick' export is not a function".to_owned());
    };
    if func.params().len() != 4 || func.results().len() != 1 {
        return Err("bot_tick signature must be (i32, i32, i32, i32) -> i32".to_owned());
    }
    if !func.params().all(|param| matches!(param, ValType::I32)) {
        return Err("bot_tick parameters must be i32".to_owned());
    }
    if !matches!(func.results().next(), Some(ValType::I32)) {
        return Err("bot_tick return type must be i32".to_owned());
    }
    Ok(())
}

fn validate_source_impl(
    max_source_bytes: usize,
    source: &str,
    language: Option<&str>,
) -> CodeValidationResponse {
    let language = language.unwrap_or("rust").trim().to_ascii_lowercase();

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let source_len = source.len();
    if source_len == 0 {
        errors.push("source_code cannot be empty".to_owned());
        return CodeValidationResponse {
            valid: false,
            errors,
            warnings,
        };
    }
    if source_len > max_source_bytes {
        errors.push(format!(
            "source_code exceeds max size ({} > {} bytes)",
            source_len, max_source_bytes
        ));
    }

    let lowered = source.to_ascii_lowercase();
    let lowered_no_whitespace: String = lowered
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let lowered_compact_alnum: String = lowered
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect();
    let forbidden_patterns = [
        ("unsafe", "unsafe", "unsafe"),
        ("std::fs", "std::fs", "stdfs"),
        ("std::process", "std::process", "stdprocess"),
        ("std::net", "std::net", "stdnet"),
        ("libc::", "libc::", "libc"),
        ("asm!(", "asm!(", "asm"),
        ("thread::spawn", "thread::spawn", "threadspawn"),
        ("tokio::spawn", "tokio::spawn", "tokiospawn"),
        ("extern \"c\" fn main", "extern\"c\"fnmain", "externcfnmain"),
    ];
    for (display_pattern, normalized_pattern, compact_pattern) in forbidden_patterns {
        if lowered.contains(display_pattern)
            || lowered_no_whitespace.contains(normalized_pattern)
            || lowered_compact_alnum.contains(compact_pattern)
        {
            errors.push(format!("forbidden pattern detected: '{}'", display_pattern));
        }
    }

    if language == "rust" && !lowered.contains("fn bot_tick") {
        errors.push("required function `bot_tick` is missing".to_owned());
    }

    if !lowered.contains("#[no_mangle]") {
        warnings.push("export marker #[no_mangle] not detected".to_owned());
    }
    if !lowered.contains("extern \"c\"") {
        warnings.push("extern \"C\" not detected; wasm export may be mangled".to_owned());
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

fn sanitize_model_id(model_id: &str) -> Option<String> {
    let trimmed = model_id.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_MODEL_ID_LEN {
        return None;
    }
    if trimmed
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-' || ch == b'.')
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

fn truncate_for_api(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut cutoff = max_bytes.min(value.len());
    while cutoff > 0 && !value.is_char_boundary(cutoff) {
        cutoff -= 1;
    }
    let mut out = value[..cutoff].to_owned();
    out.push_str("...<truncated>");
    out
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
    // Source code validation bodies can be up to 128 KB (max source size + JSON overhead).
    // Generation requests are small JSON bodies so 64 KB suffices.
    let json_body_limit = 1024 * 64;
    let source_body_limit = 256 * 1024; // 256 KB for source code payloads

    let validate = warp::path!("api" / "arena" / "code" / "validate")
        .and(warp::post())
        .and(warp::body::content_length_limit(source_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |body: ValidateBotCodeBody, service: CodeGenerationService| {
                ok_response(service.validate_source(body))
            },
        );

    let generate = warp::path!("api" / "arena" / "code" / "generate")
        .and(warp::post())
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .and_then(
            |body: GenerateBotCodeBody, service: CodeGenerationService| async move {
                let reply = match service.generate_bot_code(body).await {
                    Ok(response) => ok_response(response),
                    Err(err) => error_response(err.code, err.message),
                };
                Ok::<_, warp::Rejection>(reply)
            },
        );

    let generate_and_compile = warp::path!("api" / "arena" / "code" / "generate_and_compile")
        .and(warp::post())
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service))
        .and_then(
            |body: GenerateAndCompileBotCodeBody, service: CodeGenerationService| async move {
                let reply = match service.generate_and_compile(body).await {
                    Ok(response) => ok_response(response),
                    Err(err) => error_response(err.code, err.message),
                };
                Ok::<_, warp::Rejection>(reply)
            },
        );

    validate.or(generate).or(generate_and_compile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn with_env_lock<T>(f: impl FnOnce() -> T) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned");
        f()
    }

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
        assert!(response.source_code.contains("bot_tick"));
    }

    #[test]
    fn sanitize_model_id_rejects_path_traversal() {
        assert!(sanitize_model_id("../bot").is_none());
        assert!(sanitize_model_id("bot_alpha-1").is_some());
        assert!(sanitize_model_id(&"a".repeat(MAX_MODEL_ID_LEN + 1)).is_none());
    }

    #[test]
    fn validator_rejects_std_fs_with_whitespace_bypass() {
        let service = CodeGenerationService::new_from_env();
        let response = service.validate_source(ValidateBotCodeBody {
            source_code: r#"
#[no_mangle]
pub extern "C" fn bot_tick() -> i32 {
    let _ = std :: fs :: read_to_string("/tmp/secret");
    0
}
"#
            .to_owned(),
            language: Some("rust".to_owned()),
        });
        assert!(!response.valid);
        assert!(
            response.errors.iter().any(|line| line.contains("std::fs")),
            "expected std::fs rejection, got {:?}",
            response.errors
        );
    }

    #[test]
    fn truncate_for_api_preserves_utf8_boundaries() {
        let value = "abc🙂def";
        let truncated = truncate_for_api(value, 5);
        assert!(truncated.ends_with("...<truncated>"));
        let prefix = truncated.trim_end_matches("...<truncated>");
        assert_eq!(prefix, "abc");
    }

    #[test]
    fn read_env_secret_prefers_direct_env_value() {
        with_env_lock(|| {
            let key = "MGS_TEST_OPENROUTER_API_KEY";
            let file_key = "MGS_TEST_OPENROUTER_API_KEY_FILE";

            let prev_key = std::env::var(key).ok();
            let prev_file_key = std::env::var(file_key).ok();
            // SAFETY: Tests intentionally mutate process env under a global test lock.
            unsafe { std::env::remove_var(file_key) };
            // SAFETY: Tests intentionally mutate process env under a global test lock.
            unsafe { std::env::set_var(key, "inline-secret") };

            let value = read_env_secret(key);
            assert_eq!(value.as_deref(), Some("inline-secret"));

            match prev_key {
                Some(raw) => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::set_var(key, raw) }
                }
                None => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::remove_var(key) }
                }
            }
            match prev_file_key {
                Some(raw) => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::set_var(file_key, raw) }
                }
                None => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::remove_var(file_key) }
                }
            }
        });
    }

    #[test]
    fn read_env_secret_uses_file_fallback() {
        with_env_lock(|| {
            let key = "MGS_TEST_OPENROUTER_FILE_ONLY";
            let file_key = "MGS_TEST_OPENROUTER_FILE_ONLY_FILE";
            let prev_key = std::env::var(key).ok();
            let prev_file_key = std::env::var(file_key).ok();
            // SAFETY: Tests intentionally mutate process env under a global test lock.
            unsafe { std::env::remove_var(key) };

            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be monotonic")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("mgs_openrouter_secret_{stamp}.txt"));
            std::fs::write(&path, "file-secret\n").expect("secret file should be written");

            // SAFETY: Tests intentionally mutate process env under a global test lock.
            unsafe { std::env::set_var(file_key, &path) };
            let value = read_env_secret(key);
            assert_eq!(value.as_deref(), Some("file-secret"));

            match prev_key {
                Some(raw) => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::set_var(key, raw) }
                }
                None => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::remove_var(key) }
                }
            }
            match prev_file_key {
                Some(raw) => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::set_var(file_key, raw) }
                }
                None => {
                    // SAFETY: Tests intentionally mutate process env under a global test lock.
                    unsafe { std::env::remove_var(file_key) }
                }
            }
            let _ = std::fs::remove_file(path);
        });
    }
}
