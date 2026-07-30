use crate::operational::validation::sanitize_model_id;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use warp::{Filter, Reply};
use wasmtime::{Config, Engine, ExternType, Module, ValType};

const DEFAULT_MAX_SOURCE_BYTES: usize = 50 * 1024;
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_OPENROUTER_APP_TITLE: &str = "massive_game_server";
const DEFAULT_ARENA_WASM_DIR: &str = "data/arena_bots";
const DEFAULT_ARENA_SOURCE_DIR: &str = "data/arena_sources";
const DEFAULT_OPENROUTER_MAX_TOKENS: u32 = 4_096;
const MIN_OPENROUTER_MAX_TOKENS: u32 = 2_049;
const MAX_OPENROUTER_MAX_TOKENS: u32 = 16_384;
const DEFAULT_OPENROUTER_TIMEOUT_SECS: u64 = 120;
const MIN_OPENROUTER_TIMEOUT_SECS: u64 = 30;
const MAX_OPENROUTER_TIMEOUT_SECS: u64 = 900;
const MAX_OPENROUTER_GENERATION_ID_BYTES: usize = 128;
const MAX_OPENROUTER_METADATA_BYTES: usize = 256;
// Streaming providers may repeat hundreds of bytes of JSON/SSE envelope for
// every small token delta. Keep the decoded source at 50 KiB while allowing a
// bounded amount of legitimate framing overhead on the wire.
const MAX_OPENROUTER_STREAM_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_RUSTC_TIMEOUT_SECS: u64 = 12;
const MAX_RUSTC_TIMEOUT_SECS: u64 = 120;
const DEFAULT_RUSTC_CPU_LIMIT_SECS: u64 = 10;
const MAX_RUSTC_CPU_LIMIT_SECS: u64 = 120;
const DEFAULT_RUSTC_MEMORY_LIMIT_MB: u64 = 1024;
const MAX_RUSTC_MEMORY_LIMIT_MB: u64 = 4096;

pub const ARENA_COMPETITION_PROMPT_VERSION: &str = "arena-rust-v3.1.0";
pub const ARENA_COLLABORATION_ABI_VERSION: &str = "bot_tick_v2/1";
pub const ARENA_PROVIDER_SORT_POLICY: &str = "throughput";
pub const ARENA_PROVIDER_REQUIRE_PARAMETERS: bool = true;
pub const ARENA_TEMPERATURE_POLICY: &str = "provider_default";
pub const ARENA_REASONING_POLICY_VERSION: &str = "capability_minimum_v1";
pub const ARENA_REASONING_EXCLUDE: bool = true;
pub const ARENA_RESPONSE_TRANSPORT_POLICY: &str = "sse_v1";
pub const ARENA_UNIFORM_SYSTEM_PROMPT: &str = "You are a contestant in a deterministic Rust/WASM fighter competition. Begin the Rust source immediately; do not analyze exhaustively. Return exactly one complete Rust 2021 source file as raw text and stop immediately after its final closing brace. Prefer a simple, complete file below 8 KiB and roughly 2,000 visible tokens. An incomplete file is a failed submission. Never return markdown fences, explanations, or anything before or after the source file.";
pub const ARENA_UNIFORM_COMPETITION_PROMPT: &str = r#"Implement one fighter strategy for the uniform OpenRouter Model Arena season.

OFFICIAL OBJECTIVE
Your program is ranked on four separately reported dimensions:
1. TEAM: win rounds and advance or deny the mode objective.
2. PERSONAL: deal effective damage and secure eliminations efficiently.
3. COLLABORATION: create real teammate benefit through damage-preventing support and assists.
4. WORLD: make your whole faction survive and score when all ten model factions fight simultaneously.
The duel rating combines PERSONAL 40%, TEAM 35%, and COLLABORATION 25%. The epoch strategy score combines that duel rating 75% with WORLD 25%, and weekly tour points are awarded by epoch strategy rank. Empty support, stalling, invalid actions, traps, and fallback execution earn no benefit. Optimize robustly across hidden opponents, seeds, sides, and modes; do not target one known schedule.

FILE AND ABI CONTRACT
- The UTF-8 source file must be at most 51,200 bytes (50 KiB). Prefer a concise, complete file below 8 KiB; complexity itself earns no points.
- It must compile with:
  rustc --edition=2021 --crate-type=cdylib --target=wasm32-unknown-unknown -O
- Export exactly this season entry point (helper functions and constants are allowed):
  #[no_mangle]
  pub extern "C" fn bot_tick_v2(
      self_health: i32,
      target_health: i32,
      personal_score: i32,
      team_score_delta: i32,
      objective_delta: i32,
      allies_alive: i32,
      enemies_alive: i32,
      lowest_ally_health: i32,
      slot: i32,
      mode: i32,
      tick: i32,
  ) -> i32

OBSERVATION SEMANTICS
- self_health and target_health are current hit points. Fighters spawn with 100.
- personal_score is this fighter's combat score.
- team_score_delta is own team score minus enemy team score.
- objective_delta is own objective value minus enemy objective value.
- allies_alive includes this fighter. enemies_alive is the living enemy count.
- lowest_ally_health is the health of the lowest-health living ally other than self, or 0 when none exists. Ties select the lowest slot.
- slot is this fighter's stable zero-based team slot. Every teammate runs the same submitted program, so slot and tick can coordinate deterministic roles.
- mode is 0=arena, 1=team deathmatch, 2=capture the flag, 3=king of the hill.
- tick starts at 0. All fighters decide from the same pre-tick snapshot; actions resolve simultaneously.
- In two-faction duels, ATTACK and CHARGE target the living enemy at the same slot, wrapping forward to the next living enemy.

RETURN EXACTLY ONE ACTION CODE
- 0 IDLE: no damage, defense, support, or objective presence.
- 1 ATTACK: base 10 damage with deterministic jitter from -1 through +2; CTF push 2; KOTH presence 2.
- 2 DEFEND: no outgoing damage; self incoming damage becomes 40%; CTF push 1 and block 3; KOTH presence 3.
- 3 CHARGE: base 16 damage with deterministic jitter from -1 through +2 and 4 self-damage; CTF push 4; KOTH presence 2.
- 4 SUPPORT: no damage and no self protection; shield the selected lowest-health ally so its post-defend incoming damage becomes 50%; CTF block 1; KOTH presence 1. Multiple supporters do not stack the shield and split credit.
Any other return value becomes IDLE and records a strategy fault.

SCORING AND MATCH MECHANICS
- Personal score gains 2 + floor(effective_damage / 3) for each damaging action and 40 for an elimination, then loses 4 on death. Self-damage is not credited.
- Team score gains 40 for each enemy elimination. A CTF capture adds 70. KOTH control gains 1 + floor(presence_advantage / 2) per advancing tick.
- Collaboration score gains one point per hit point of actual ally damage prevented by SUPPORT. If multiple teammates damage an enemy on its elimination tick, the highest effective-damage contributor gets the elimination and every other effective contributor gets a 20-point assist.
- DEFEND protects only self and never earns collaboration points. SUPPORT protects only a teammate and earns points only when damage is actually prevented.
- Non-arena two-faction modes grant three respawns per fighter; arena and the world event grant none.
- CTF progress per tick is max(0, total_push - enemy_total_block), capture threshold is 14 * team_size, and three captures end the round. On ticks divisible by 24, each side also loses 1 uncaptured progress after capture checks.
- KOTH control advances by the positive team presence difference; target is 160 * team_size.
- Team victory is decided by objective value, then team score. Personal and collaboration scores remain distinct leaderboard dimensions and do not rewrite the round winner.

ALL-MODEL WORLD EVENT
- All ten submitted models enter one shared free-for-all. Each model controls one faction of identical-program squadmates; all factions act from the same pre-tick snapshot.
- mode is 1. allies_alive is your living faction size, enemies_alive is every living fighter outside your faction, and lowest_ally_health keeps its normal teammate meaning.
- team_score_delta compares your faction team score with the strongest opponent. objective_delta compares your eliminations with the leading opponent.
- Each living ATTACK or CHARGE receives one deterministic seeded target selected from every living enemy fighter. Request ordering cannot change the canonical faction order.
- There are no respawns. A round ranks factions by fighters alive, eliminations, remaining health, team score, personal score, collaboration score, then fewer deaths. World placement points for ranks 1-10 are 1000, 600, 360, 220, 140, 90, 60, 40, 30, and 22.

SAFETY AND FAIRNESS REQUIREMENTS
- Use only safe, deterministic, stable Rust and the standard prelude. No external crates.
- Do not emit the token `unsafe` anywhere, including comments or identifiers.
- No file, network, process, environment, clock, thread, assembly, include, additional extern declaration, host import, or custom macro access; #[no_mangle] is the only attribute/macro needed.
- No mutable global state, deliberate panic, infinite loop, runtime trap, host import, or attempt to trigger fallback behavior.
- Avoid overflow with comparisons, clamps, saturating arithmetic, or small bounded values.

Return only the complete raw Rust source file."#;
pub const ARENA_REVISION_PROMPT_VERSION: &str = "arena-rust-revision-v1.0.0";
pub const ARENA_REVISION_SYSTEM_PROMPT: &str = "You are a contestant in a deterministic Rust/WASM fighter competition revising your previous fighter. Begin the Rust source immediately; do not analyze exhaustively. Return exactly one complete Rust 2021 source file as raw text and stop immediately after its final closing brace. Prefer a simple, complete file below 8 KiB and roughly 2,000 visible tokens. An incomplete file is a failed submission. Never return markdown fences, explanations, or anything before or after the source file.";
pub const ARENA_REVISION_USER_PROMPT_PREFIX: &str = "You submitted the fighter below earlier this season. Its mid-season performance digest follows. Return one improved complete Rust source file that keeps the exact same required exports and ABI, and addresses the weaknesses the digest shows. Do not change the function signature.\n\nPREVIOUS SOURCE\n";
pub const ARENA_REVISION_STATS_SEPARATOR: &str = "\n\nPERFORMANCE DIGEST\n";

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
    openrouter_max_tokens: u32,
    openrouter_timeout_secs: u64,
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
    reasoning_mode: Option<String>,
    reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenerateAndCompileBotCodeBody {
    model_id: String,
    model: String,
    objective: Option<String>,
    prompt_style: Option<String>,
    reasoning_mode: Option<String>,
    reasoning_effort: Option<String>,
    overwrite: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct CompileBotCodeBody {
    model_id: String,
    source_code: String,
    overwrite: Option<bool>,
    verify_existing: Option<bool>,
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
    prompt_version: String,
    prompt_sha256: String,
    prompt_text: String,
    max_completion_tokens: u32,
    provider_sort_policy: String,
    provider_require_parameters: bool,
    temperature_policy: String,
    reasoning_policy_version: String,
    reasoning_mode: String,
    reasoning_effort: Option<String>,
    reasoning_exclude: bool,
    response_transport_policy: String,
    finish_reason: Option<String>,
    resolved_model: Option<String>,
    provider_name: Option<String>,
    provider_response_id: Option<String>,
    usage: Option<OpenRouterUsage>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CodeGenerationStatusResponse {
    provider_configured: bool,
    prompt_version: String,
    prompt_sha256: String,
    revision_prompt_version: String,
    revision_prompt_sha256: String,
    source_limit_bytes: usize,
    max_tokens: u32,
    provider_sort_policy: String,
    provider_require_parameters: bool,
    temperature_policy: String,
    reasoning_policy_version: String,
    reasoning_exclude: bool,
    response_transport_policy: String,
    provider_timeout_secs: u64,
    collaboration_abi_version: String,
    simulator_rules_version: String,
}

#[derive(Debug, Clone, Serialize)]
struct CompileBotCodeResponse {
    model_id: String,
    source_path: String,
    wasm_path: Option<String>,
    compiled: bool,
    bytes_written: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    wasm_sha256: Option<String>,
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
    max_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<OpenRouterReasoningRequest>,
    provider: OpenRouterProviderRoutingRequest,
}

#[derive(Debug, Serialize)]
struct OpenRouterReasoningRequest {
    effort: String,
    exclude: bool,
}

#[derive(Debug, Serialize)]
struct OpenRouterProviderRoutingRequest {
    sort: &'static str,
    require_parameters: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArenaReasoningPolicy {
    mode: String,
    effort: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenRouterMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct OpenRouterStreamChunk {
    id: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    #[serde(default)]
    choices: Vec<OpenRouterChoice>,
    usage: Option<OpenRouterUsage>,
    error: Option<OpenRouterProviderError>,
}

#[derive(Deserialize)]
struct OpenRouterChoice {
    index: Option<u32>,
    #[serde(default)]
    delta: OpenRouterDelta,
    finish_reason: Option<String>,
    error: Option<OpenRouterProviderError>,
}

#[derive(Default, Deserialize)]
struct OpenRouterDelta {
    content: Option<serde_json::Value>,
    tool_calls: Option<serde_json::Value>,
    function_call: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct OpenRouterProviderError {
    metadata: Option<OpenRouterProviderErrorMetadata>,
}

#[derive(Deserialize)]
struct OpenRouterProviderErrorMetadata {
    error_type: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OpenRouterUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
    cost: Option<f64>,
    #[serde(default)]
    completion_tokens_details: Option<OpenRouterCompletionTokenDetails>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OpenRouterCompletionTokenDetails {
    reasoning_tokens: Option<u64>,
}

struct OpenRouterGeneration {
    source_code: String,
    finish_reason: Option<String>,
    resolved_model: Option<String>,
    provider_name: Option<String>,
    provider_response_id: Option<String>,
    usage: Option<OpenRouterUsage>,
}

#[derive(Debug, Clone)]
struct OpenRouterResponseAudit {
    status: reqwest::StatusCode,
    generation_id: Option<String>,
    declared_content_length: Option<u64>,
    content_type: String,
}

#[derive(Debug, Clone, Copy)]
enum OpenRouterStreamReadFailure {
    Timeout,
    Transport,
}

#[derive(Debug, Clone, Copy)]
struct OpenRouterSseLimits {
    source_bytes: usize,
    stream_bytes: usize,
    line_bytes: usize,
    event_bytes: usize,
}

impl OpenRouterSseLimits {
    fn production(source_bytes: usize) -> Self {
        Self {
            source_bytes,
            stream_bytes: MAX_OPENROUTER_STREAM_BYTES,
            // A legal 50 KiB source can occupy more than 300 KiB when every
            // byte is represented as a JSON escape in one provider chunk.
            line_bytes: 384 * 1024,
            event_bytes: 448 * 1024,
        }
    }
}

#[derive(Debug)]
struct OpenRouterSseFailure {
    category: &'static str,
    event_index: Option<usize>,
    byte_length: Option<usize>,
    sha256: Option<String>,
    observed_bytes: Option<usize>,
    limit_bytes: Option<usize>,
    provider_error_type: Option<String>,
    usage: Option<OpenRouterUsage>,
}

impl OpenRouterSseFailure {
    fn protocol(category: &'static str, usage: Option<&OpenRouterUsage>) -> Box<Self> {
        Box::new(Self {
            category,
            event_index: None,
            byte_length: None,
            sha256: None,
            observed_bytes: None,
            limit_bytes: None,
            provider_error_type: None,
            usage: usage.cloned(),
        })
    }

    fn bounded(category: &'static str, observed_bytes: usize, limit_bytes: usize) -> Box<Self> {
        Box::new(Self {
            category,
            event_index: None,
            byte_length: None,
            sha256: None,
            observed_bytes: Some(observed_bytes),
            limit_bytes: Some(limit_bytes),
            provider_error_type: None,
            usage: None,
        })
    }

    fn event(
        category: &'static str,
        event_index: usize,
        data: &[u8],
        usage: Option<&OpenRouterUsage>,
    ) -> Box<Self> {
        Box::new(Self {
            category,
            event_index: Some(event_index),
            byte_length: Some(data.len()),
            sha256: Some(sha256_hex(data)),
            observed_bytes: None,
            limit_bytes: None,
            provider_error_type: None,
            usage: usage.cloned(),
        })
    }
}

struct OpenRouterSseParser {
    limits: OpenRouterSseLimits,
    total_stream_bytes: usize,
    line_buffer: Vec<u8>,
    event_data: Vec<u8>,
    event_has_data: bool,
    bom_probe: Vec<u8>,
    bom_checked: bool,
    pending_cr: bool,
    event_index: usize,
    source_code: String,
    finish_reason: Option<String>,
    resolved_model: Option<String>,
    provider_name: Option<String>,
    provider_response_id: Option<String>,
    usage: Option<OpenRouterUsage>,
    usage_seen: bool,
    done_seen: bool,
}

impl OpenRouterSseParser {
    fn new(limits: OpenRouterSseLimits) -> Self {
        Self {
            limits,
            total_stream_bytes: 0,
            line_buffer: Vec::new(),
            event_data: Vec::new(),
            event_has_data: false,
            bom_probe: Vec::new(),
            bom_checked: false,
            pending_cr: false,
            event_index: 0,
            source_code: String::new(),
            finish_reason: None,
            resolved_model: None,
            provider_name: None,
            provider_response_id: None,
            usage: None,
            usage_seen: false,
            done_seen: false,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), Box<OpenRouterSseFailure>> {
        let observed_bytes = self.total_stream_bytes.saturating_add(chunk.len());
        if observed_bytes > self.limits.stream_bytes {
            return Err(OpenRouterSseFailure::bounded(
                "stream_too_large",
                observed_bytes,
                self.limits.stream_bytes,
            ));
        }
        self.total_stream_bytes = observed_bytes;

        for &byte in chunk {
            self.push_stream_byte(byte)?;
        }
        Ok(())
    }

    fn push_stream_byte(&mut self, byte: u8) -> Result<(), Box<OpenRouterSseFailure>> {
        if !self.bom_checked {
            const UTF8_BOM: &[u8; 3] = b"\xef\xbb\xbf";
            self.bom_probe.push(byte);
            if UTF8_BOM.starts_with(&self.bom_probe) {
                if self.bom_probe.len() == UTF8_BOM.len() {
                    self.bom_probe.clear();
                    self.bom_checked = true;
                }
                return Ok(());
            }
            self.bom_checked = true;
            let probed = std::mem::take(&mut self.bom_probe);
            for probed_byte in probed {
                self.push_framed_byte(probed_byte)?;
            }
            return Ok(());
        }
        self.push_framed_byte(byte)
    }

    fn push_framed_byte(&mut self, byte: u8) -> Result<(), Box<OpenRouterSseFailure>> {
        if self.pending_cr {
            self.pending_cr = false;
            if byte == b'\n' {
                return Ok(());
            }
        }

        if byte == b'\r' {
            self.finish_line()?;
            self.pending_cr = true;
            return Ok(());
        }
        if byte == b'\n' {
            return self.finish_line();
        }

        self.line_buffer.push(byte);
        if self.line_buffer.len() > self.limits.line_bytes {
            let mut failure = OpenRouterSseFailure::bounded(
                "line_too_large",
                self.line_buffer.len(),
                self.limits.line_bytes,
            );
            failure.byte_length = Some(self.line_buffer.len());
            failure.sha256 = Some(sha256_hex(&self.line_buffer));
            return Err(failure);
        }
        Ok(())
    }

    fn finish_line(&mut self) -> Result<(), Box<OpenRouterSseFailure>> {
        let line = std::mem::take(&mut self.line_buffer);
        if line.is_empty() {
            return self.dispatch_event();
        }
        if line.starts_with(b":") {
            return Ok(());
        }

        let (field, mut value) = match line.iter().position(|byte| *byte == b':') {
            Some(index) => (&line[..index], &line[index + 1..]),
            None => (line.as_slice(), &[][..]),
        };
        if value.first() == Some(&b' ') {
            value = &value[1..];
        }
        if field != b"data" {
            return Ok(());
        }

        let separator_bytes = usize::from(self.event_has_data);
        let observed_bytes = self
            .event_data
            .len()
            .saturating_add(separator_bytes)
            .saturating_add(value.len());
        if observed_bytes > self.limits.event_bytes {
            return Err(OpenRouterSseFailure::bounded(
                "event_too_large",
                observed_bytes,
                self.limits.event_bytes,
            ));
        }
        if separator_bytes == 1 {
            self.event_data.push(b'\n');
        }
        self.event_data.extend_from_slice(value);
        self.event_has_data = true;
        Ok(())
    }

    fn dispatch_event(&mut self) -> Result<(), Box<OpenRouterSseFailure>> {
        if !self.event_has_data {
            return Ok(());
        }
        self.event_has_data = false;
        let data = std::mem::take(&mut self.event_data);
        if data.is_empty() {
            return Ok(());
        }
        self.event_index = self.event_index.saturating_add(1);
        let event_index = self.event_index;

        if data == b"[DONE]" {
            if self.done_seen {
                return Err(OpenRouterSseFailure::event(
                    "duplicate_done",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }
            if self.finish_reason.as_deref() != Some("stop") {
                return Err(OpenRouterSseFailure::event(
                    "done_without_stop",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }
            if !self.usage_seen {
                return Err(OpenRouterSseFailure::event(
                    "done_without_usage",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }
            self.done_seen = true;
            return Ok(());
        }
        if self.done_seen {
            return Err(OpenRouterSseFailure::event(
                "event_after_done",
                event_index,
                &data,
                self.usage.as_ref(),
            ));
        }

        let chunk: OpenRouterStreamChunk = serde_json::from_slice(&data).map_err(|error| {
            OpenRouterSseFailure::event(
                openrouter_sse_json_category(&error),
                event_index,
                &data,
                self.usage.as_ref(),
            )
        })?;

        if let Some(id) = chunk.id.as_deref() {
            self.provider_response_id = Some(sanitize_openrouter_metadata(id));
        }
        if let Some(model) = chunk.model.as_deref() {
            self.resolved_model = Some(sanitize_openrouter_metadata(model));
        }
        if let Some(provider) = chunk.provider.as_deref() {
            self.provider_name = Some(sanitize_openrouter_metadata(provider));
        }

        if let Some(error) = chunk.error.as_ref() {
            let mut failure = OpenRouterSseFailure::event(
                "provider_error",
                event_index,
                &data,
                self.usage.as_ref(),
            );
            failure.provider_error_type = sanitize_openrouter_error_type(error);
            return Err(failure);
        }
        if self.usage_seen {
            return Err(OpenRouterSseFailure::event(
                "event_after_usage",
                event_index,
                &data,
                self.usage.as_ref(),
            ));
        }
        if chunk.choices.len() > 1 {
            return Err(OpenRouterSseFailure::event(
                "multiple_choices",
                event_index,
                &data,
                self.usage.as_ref(),
            ));
        }

        let usage_present = chunk.usage.is_some();
        let choice = chunk.choices.first();
        let inert_post_finish_usage_choice = usage_present
            && match (choice, self.finish_reason.as_deref()) {
                (Some(choice), Some(existing_finish)) => {
                    let content_is_empty = match choice.delta.content.as_ref() {
                        None => true,
                        Some(content) => extract_content_string(content)
                            .is_some_and(|fragment| fragment.is_empty()),
                    };
                    choice.index.is_none_or(|index| index == 0)
                        && choice.error.is_none()
                        && choice.delta.tool_calls.is_none()
                        && choice.delta.function_call.is_none()
                        && content_is_empty
                        && choice
                            .finish_reason
                            .as_deref()
                            .is_none_or(|finish| finish == existing_finish)
                }
                _ => false,
            };
        if usage_present {
            let valid_terminal_layout = match choice {
                // OpenRouter's canonical final usage chunk has no choices and
                // follows a separate terminal choice chunk.
                None => self.finish_reason.is_some(),
                // DeepInfra includes the terminal choice and usage together.
                // Accept only that exact transition, never a non-terminal or
                // duplicate choice carrying usage.
                Some(choice) => {
                    (self.finish_reason.is_none() && choice.finish_reason.is_some())
                        || inert_post_finish_usage_choice
                }
            };
            if !valid_terminal_layout {
                return Err(OpenRouterSseFailure::event(
                    "invalid_usage_chunk",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }
        }

        if inert_post_finish_usage_choice {
            // Some routed providers retain an inert choice in the final usage
            // event after already sending the terminal finish. Treat that
            // exact empty, idempotent layout like choices:[]; all non-empty or
            // conflicting post-finish choices remain invalid above.
        } else if let Some(choice) = choice {
            if choice.index.is_some_and(|index| index != 0) {
                return Err(OpenRouterSseFailure::event(
                    "unexpected_choice_index",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }
            if let Some(error) = choice.error.as_ref() {
                let mut failure = OpenRouterSseFailure::event(
                    "provider_choice_error",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                );
                failure.provider_error_type = sanitize_openrouter_error_type(error);
                return Err(failure);
            }
            if choice.delta.tool_calls.is_some() || choice.delta.function_call.is_some() {
                return Err(OpenRouterSseFailure::event(
                    "tool_call_delta",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }

            if let Some(content) = choice.delta.content.as_ref() {
                if self.finish_reason.is_some() {
                    return Err(OpenRouterSseFailure::event(
                        "content_after_finish",
                        event_index,
                        &data,
                        self.usage.as_ref(),
                    ));
                }
                let Some(fragment) = extract_content_string(content) else {
                    return Err(OpenRouterSseFailure::event(
                        "unsupported_content_shape",
                        event_index,
                        &data,
                        self.usage.as_ref(),
                    ));
                };
                let observed_bytes = self.source_code.len().saturating_add(fragment.len());
                if observed_bytes > self.limits.source_bytes {
                    let mut failure = OpenRouterSseFailure::event(
                        "source_too_large",
                        event_index,
                        &data,
                        self.usage.as_ref(),
                    );
                    failure.observed_bytes = Some(observed_bytes);
                    failure.limit_bytes = Some(self.limits.source_bytes);
                    return Err(failure);
                }
                self.source_code.push_str(&fragment);
            }

            if let Some(finish_reason) = choice.finish_reason.as_deref() {
                if self.finish_reason.is_some() {
                    return Err(OpenRouterSseFailure::event(
                        "duplicate_finish",
                        event_index,
                        &data,
                        self.usage.as_ref(),
                    ));
                }
                match finish_reason {
                    "stop" | "length" => self.finish_reason = Some(finish_reason.to_owned()),
                    "error" => {
                        return Err(OpenRouterSseFailure::event(
                            "finish_error",
                            event_index,
                            &data,
                            self.usage.as_ref(),
                        ));
                    }
                    "content_filter" => {
                        return Err(OpenRouterSseFailure::event(
                            "finish_content_filter",
                            event_index,
                            &data,
                            self.usage.as_ref(),
                        ));
                    }
                    "tool_calls" => {
                        return Err(OpenRouterSseFailure::event(
                            "finish_tool_calls",
                            event_index,
                            &data,
                            self.usage.as_ref(),
                        ));
                    }
                    _ => {
                        return Err(OpenRouterSseFailure::event(
                            "finish_unknown",
                            event_index,
                            &data,
                            self.usage.as_ref(),
                        ));
                    }
                }
            }
        }

        if let Some(usage) = chunk.usage {
            self.usage = Some(usage);
            if self.usage.as_ref().is_some_and(|usage| {
                usage.prompt_tokens.is_none()
                    || usage.completion_tokens.is_none()
                    || usage.total_tokens.is_none()
                    || usage
                        .cost
                        .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
            }) {
                return Err(OpenRouterSseFailure::event(
                    "invalid_usage_fields",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }
            self.usage_seen = true;
            if self.finish_reason.as_deref() == Some("length") {
                return Err(OpenRouterSseFailure::event(
                    "finish_length",
                    event_index,
                    &data,
                    self.usage.as_ref(),
                ));
            }
        }
        Ok(())
    }

    fn finish(
        self,
        header_generation_id: Option<String>,
    ) -> Result<OpenRouterGeneration, Box<OpenRouterSseFailure>> {
        if !self.bom_checked && !self.bom_probe.is_empty() {
            return Err(OpenRouterSseFailure::protocol(
                "incomplete_bom",
                self.usage.as_ref(),
            ));
        }
        if !self.line_buffer.is_empty() || self.event_has_data || !self.event_data.is_empty() {
            return Err(OpenRouterSseFailure::protocol(
                "incomplete_event",
                self.usage.as_ref(),
            ));
        }
        if !self.done_seen {
            return Err(OpenRouterSseFailure::protocol(
                "missing_done",
                self.usage.as_ref(),
            ));
        }
        if self.finish_reason.as_deref() != Some("stop") {
            return Err(OpenRouterSseFailure::protocol(
                "missing_stop",
                self.usage.as_ref(),
            ));
        }
        if !self.usage_seen || self.usage.is_none() {
            return Err(OpenRouterSseFailure::protocol(
                "missing_usage",
                self.usage.as_ref(),
            ));
        }

        let source = extract_rust_source(&self.source_code);
        if source.trim().is_empty() {
            return Err(OpenRouterSseFailure::protocol(
                "empty_source",
                self.usage.as_ref(),
            ));
        }
        Ok(OpenRouterGeneration {
            source_code: source,
            finish_reason: self.finish_reason,
            resolved_model: self.resolved_model,
            provider_name: self.provider_name,
            provider_response_id: select_openrouter_response_id(
                self.provider_response_id,
                header_generation_id,
            ),
            usage: self.usage,
        })
    }
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
        let openrouter_max_tokens = std::env::var("MGS_OPENROUTER_MAX_TOKENS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            .unwrap_or(DEFAULT_OPENROUTER_MAX_TOKENS)
            .clamp(MIN_OPENROUTER_MAX_TOKENS, MAX_OPENROUTER_MAX_TOKENS);
        let openrouter_timeout_secs = std::env::var("MGS_OPENROUTER_TIMEOUT_SECS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_OPENROUTER_TIMEOUT_SECS)
            .clamp(MIN_OPENROUTER_TIMEOUT_SECS, MAX_OPENROUTER_TIMEOUT_SECS);
        let max_source_bytes = std::env::var("MGS_BOT_SOURCE_MAX_BYTES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|value| *value > 1024)
            .map(|value| value.min(DEFAULT_MAX_SOURCE_BYTES))
            .unwrap_or(DEFAULT_MAX_SOURCE_BYTES);
        let source_dir = std::env::var("MGS_ARENA_SOURCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ARENA_SOURCE_DIR));
        let wasm_dir = std::env::var("MGS_ARENA_WASM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ARENA_WASM_DIR));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(openrouter_timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            inner: Arc::new(CodeGenerationInner {
                client,
                openrouter_api_key,
                openrouter_base_url,
                openrouter_app_title,
                openrouter_http_referrer,
                openrouter_max_tokens,
                openrouter_timeout_secs,
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

    fn status(&self) -> CodeGenerationStatusResponse {
        CodeGenerationStatusResponse {
            provider_configured: self.inner.openrouter_api_key.is_some(),
            prompt_version: ARENA_COMPETITION_PROMPT_VERSION.to_owned(),
            prompt_sha256: competition_prompt_sha256(),
            revision_prompt_version: ARENA_REVISION_PROMPT_VERSION.to_owned(),
            revision_prompt_sha256: revision_prompt_sha256(),
            source_limit_bytes: self.inner.max_source_bytes,
            max_tokens: self.inner.openrouter_max_tokens,
            provider_sort_policy: ARENA_PROVIDER_SORT_POLICY.to_owned(),
            provider_require_parameters: ARENA_PROVIDER_REQUIRE_PARAMETERS,
            temperature_policy: ARENA_TEMPERATURE_POLICY.to_owned(),
            reasoning_policy_version: ARENA_REASONING_POLICY_VERSION.to_owned(),
            reasoning_exclude: ARENA_REASONING_EXCLUDE,
            response_transport_policy: ARENA_RESPONSE_TRANSPORT_POLICY.to_owned(),
            provider_timeout_secs: self.inner.openrouter_timeout_secs,
            collaboration_abi_version: ARENA_COLLABORATION_ABI_VERSION.to_owned(),
            simulator_rules_version: crate::operational::bot_sandbox::ARENA_SIMULATOR_RULES_VERSION
                .to_owned(),
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
        let reasoning_policy = normalize_arena_reasoning_policy(
            body.reasoning_mode.as_deref(),
            body.reasoning_effort.as_deref(),
        )
        .map_err(|message| ApiErrorBody {
            code: "invalid_reasoning_policy",
            message,
        })?;

        let objective =
            "maximize team wins, personal combat efficiency, and causal collaboration".to_owned();
        let prompt_style = ARENA_COMPETITION_PROMPT_VERSION.to_owned();

        let mut simulated = false;
        let mut warnings = Vec::new();
        if body
            .objective
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
            || body
                .prompt_style
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
        {
            warnings.push(
                "request objective/prompt_style ignored; the uniform season prompt is immutable"
                    .to_owned(),
            );
        }

        let mut finish_reason = None;
        let mut resolved_model = None;
        let mut provider_name = None;
        let mut provider_response_id = None;
        let mut usage = None;
        let source_code = if let Some(api_key) = self.inner.openrouter_api_key.as_deref() {
            match self
                .generate_via_openrouter(api_key, model, &reasoning_policy)
                .await
            {
                Ok(generation) => {
                    finish_reason = generation.finish_reason;
                    resolved_model = generation.resolved_model;
                    provider_name = generation.provider_name;
                    provider_response_id = generation.provider_response_id;
                    usage = generation.usage;
                    generation.source_code
                }
                Err(err) => {
                    return Err(ApiErrorBody {
                        code: "openrouter_generation_failed",
                        message: err,
                    });
                }
            }
        } else {
            simulated = true;
            warnings.push(
                "OPENROUTER_API_KEY is not configured; deterministic local template used"
                    .to_owned(),
            );
            self.build_local_template()
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
            prompt_version: ARENA_COMPETITION_PROMPT_VERSION.to_owned(),
            prompt_sha256: competition_prompt_sha256(),
            prompt_text: canonical_competition_prompt(),
            max_completion_tokens: self.inner.openrouter_max_tokens,
            provider_sort_policy: ARENA_PROVIDER_SORT_POLICY.to_owned(),
            provider_require_parameters: ARENA_PROVIDER_REQUIRE_PARAMETERS,
            temperature_policy: ARENA_TEMPERATURE_POLICY.to_owned(),
            reasoning_policy_version: ARENA_REASONING_POLICY_VERSION.to_owned(),
            reasoning_mode: reasoning_policy.mode,
            reasoning_effort: reasoning_policy.effort,
            reasoning_exclude: ARENA_REASONING_EXCLUDE,
            response_transport_policy: ARENA_RESPONSE_TRANSPORT_POLICY.to_owned(),
            finish_reason,
            resolved_model,
            provider_name,
            provider_response_id,
            usage,
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
                reasoning_mode: body.reasoning_mode,
                reasoning_effort: body.reasoning_effort,
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

    async fn compile_bot_code(&self, body: CompileBotCodeBody) -> CompileBotCodeResponse {
        if body.verify_existing.unwrap_or(false) {
            let overwrite_requested = body.overwrite.unwrap_or(false);
            let mut response = self
                .verify_existing_compiled_code_best_effort(body.model_id, body.source_code)
                .await;
            if overwrite_requested {
                response
                    .warnings
                    .push("overwrite=true ignored because verify_existing=true".to_owned());
            }
            response
        } else {
            self.compile_generated_code_best_effort(
                body.model_id,
                body.source_code,
                body.overwrite.unwrap_or(false),
            )
            .await
        }
    }

    async fn verify_existing_compiled_code_best_effort(
        &self,
        model_id: String,
        source_code: String,
    ) -> CompileBotCodeResponse {
        let max_source_bytes = self.inner.max_source_bytes;
        let source_dir = self.inner.source_dir.clone();
        let wasm_dir = self.inner.wasm_dir.clone();
        let response_model_id = model_id.clone();
        tokio::task::spawn_blocking(move || {
            verify_existing_compiled_code_impl(
                model_id,
                source_code,
                max_source_bytes,
                source_dir,
                wasm_dir,
            )
        })
        .await
        .unwrap_or_else(|err| CompileBotCodeResponse {
            model_id: response_model_id,
            source_path: String::new(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("compile verification task join error: {}", err),
            warnings: vec!["failed to run non-publishing compile verification".to_owned()],
        })
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
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("compile task join error: {}", err),
            warnings: vec!["failed to run compile task".to_owned()],
        })
    }

    async fn generate_via_openrouter(
        &self,
        api_key: &str,
        model: &str,
        reasoning_policy: &ArenaReasoningPolicy,
    ) -> Result<OpenRouterGeneration, String> {
        let request =
            build_openrouter_request(model, self.inner.openrouter_max_tokens, reasoning_policy);

        let endpoint = format!("{}/chat/completions", self.inner.openrouter_base_url);
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {}", api_key);
        let auth = HeaderValue::from_str(&bearer)
            .map_err(|_| "OpenRouter authorization header was invalid".to_owned())?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );

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
            .map_err(format_openrouter_send_error)?;

        let audit = OpenRouterResponseAudit {
            status: response.status(),
            generation_id: sanitize_openrouter_generation_id(
                response.headers().get("x-generation-id"),
            ),
            declared_content_length: response.content_length(),
            content_type: sanitize_openrouter_content_type(response.headers().get(CONTENT_TYPE)),
        };
        if !audit.status.is_success() {
            return Err(format_openrouter_http_status_error(&audit));
        }
        if !matches!(audit.content_type.as_str(), "text/event-stream" | "absent") {
            return Err(format!(
                "OpenRouter returned a non-SSE success response ({}, content_type={})",
                openrouter_response_context(&audit),
                audit.content_type,
            ));
        }

        let stream = response.bytes_stream().map(|chunk| {
            chunk.map_err(|error| {
                if error.is_timeout() {
                    OpenRouterStreamReadFailure::Timeout
                } else {
                    OpenRouterStreamReadFailure::Transport
                }
            })
        });
        consume_openrouter_sse_stream(
            stream,
            audit,
            self.inner.max_source_bytes,
            self.inner.openrouter_max_tokens,
        )
        .await
    }

    fn build_local_template(&self) -> String {
        r#"#[no_mangle]
pub extern "C" fn bot_tick_v2(
    self_health: i32,
    target_health: i32,
    personal_score: i32,
    team_score_delta: i32,
    objective_delta: i32,
    allies_alive: i32,
    _enemies_alive: i32,
    lowest_ally_health: i32,
    slot: i32,
    mode: i32,
    tick: i32,
) -> i32 {
    if allies_alive > 1
        && lowest_ally_health > 0
        && lowest_ally_health < 35
        && (slot + tick).rem_euclid(3) == 0
    {
        return 4;
    }
    if self_health < 25 {
        return 2;
    }
    if target_health < 20 {
        return 1;
    }
    if mode == 2 && objective_delta < 0 && self_health > 45 {
        return 3;
    }
    if mode == 3 && team_score_delta >= 0 {
        return 2;
    }
    if team_score_delta < 0 && self_health > 55 && (tick + personal_score) % 7 == 0 {
        return 3;
    }
    1
}"#
        .to_owned()
    }
}

fn normalize_arena_reasoning_policy(
    mode: Option<&str>,
    effort: Option<&str>,
) -> Result<ArenaReasoningPolicy, String> {
    let mode = mode.unwrap_or("disabled").trim();
    let effort = effort.map(str::trim).filter(|value| !value.is_empty());
    match mode {
        "unsupported" => {
            if effort.is_some() {
                return Err(
                    "reasoning_effort must be absent when reasoning_mode is unsupported".to_owned(),
                );
            }
            Ok(ArenaReasoningPolicy {
                mode: mode.to_owned(),
                effort: None,
            })
        }
        "disabled" => {
            if effort.is_some() {
                return Err(
                    "reasoning_effort must be absent when reasoning_mode is disabled".to_owned(),
                );
            }
            Ok(ArenaReasoningPolicy {
                mode: mode.to_owned(),
                effort: None,
            })
        }
        "minimum" => {
            let Some(effort) = effort else {
                return Err(
                    "reasoning_effort is required when reasoning_mode is minimum".to_owned(),
                );
            };
            if !matches!(
                effort,
                "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ) {
                return Err("reasoning_effort is not an allowed OpenRouter effort".to_owned());
            }
            Ok(ArenaReasoningPolicy {
                mode: mode.to_owned(),
                effort: Some(effort.to_owned()),
            })
        }
        _ => Err("reasoning_mode must be unsupported, disabled, or minimum".to_owned()),
    }
}

fn build_openrouter_request(
    model: &str,
    max_completion_tokens: u32,
    reasoning_policy: &ArenaReasoningPolicy,
) -> OpenRouterChatRequest {
    let reasoning = match reasoning_policy.mode.as_str() {
        "unsupported" => None,
        "disabled" => Some(OpenRouterReasoningRequest {
            effort: "none".to_owned(),
            exclude: ARENA_REASONING_EXCLUDE,
        }),
        "minimum" => Some(OpenRouterReasoningRequest {
            effort: reasoning_policy
                .effort
                .clone()
                .expect("validated minimum reasoning policy has an effort"),
            exclude: ARENA_REASONING_EXCLUDE,
        }),
        _ => unreachable!("reasoning policy is normalized before request construction"),
    };
    OpenRouterChatRequest {
        model: model.to_owned(),
        messages: vec![
            OpenRouterMessage {
                role: "system",
                content: ARENA_UNIFORM_SYSTEM_PROMPT.to_owned(),
            },
            OpenRouterMessage {
                role: "user",
                content: ARENA_UNIFORM_COMPETITION_PROMPT.to_owned(),
            },
        ],
        max_tokens: max_completion_tokens,
        stream: true,
        reasoning,
        provider: OpenRouterProviderRoutingRequest {
            sort: ARENA_PROVIDER_SORT_POLICY,
            require_parameters: ARENA_PROVIDER_REQUIRE_PARAMETERS,
        },
    }
}

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sanitize_openrouter_generation_id(value: Option<&HeaderValue>) -> Option<String> {
    let value = value?;
    let raw = value.to_str().ok().map(str::trim).unwrap_or_default();
    if !raw.is_empty()
        && raw.len() <= MAX_OPENROUTER_GENERATION_ID_BYTES
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Some(raw.to_owned());
    }

    // Preserve a stable correlation value without reflecting an untrusted
    // header into logs or API error messages.
    Some(format!("sha256:{}", sha256_hex(value.as_bytes())))
}

fn sanitize_openrouter_content_type(value: Option<&HeaderValue>) -> String {
    let Some(raw) = value.and_then(|header| header.to_str().ok()) else {
        return "absent".to_owned();
    };
    let media_type = raw.split(';').next().unwrap_or_default().trim();
    if media_type.is_empty()
        || media_type.len() > 128
        || !media_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'+' | b'.' | b'-'))
    {
        return "invalid".to_owned();
    }
    media_type.to_ascii_lowercase()
}

fn sanitize_openrouter_metadata(value: &str) -> String {
    let trimmed = value.trim();
    if !trimmed.is_empty()
        && trimmed.len() <= MAX_OPENROUTER_METADATA_BYTES
        && trimmed.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b' ' | b'/' | b'_' | b'-' | b'.' | b':' | b'+' | b'(' | b')'
                )
        })
    {
        return trimmed.to_owned();
    }

    format!("sha256:{}", sha256_hex(value.as_bytes()))
}

fn sanitize_openrouter_error_type(error: &OpenRouterProviderError) -> Option<String> {
    let value = error.metadata.as_ref()?.error_type.as_ref()?;
    let Some(raw) = value.as_str() else {
        return Some("non_string".to_owned());
    };
    let trimmed = raw.trim();
    if !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Some(trimmed.to_owned());
    }
    Some(format!("sha256:{}", sha256_hex(raw.as_bytes())))
}

fn openrouter_response_context(audit: &OpenRouterResponseAudit) -> String {
    let generation_id = audit.generation_id.as_deref().unwrap_or("absent");
    let declared_length = audit
        .declared_content_length
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    format!(
        "status={}, generation_id={generation_id}, declared_length={declared_length}",
        audit.status.as_u16()
    )
}

fn append_openrouter_usage(context: &mut String, usage: Option<&OpenRouterUsage>) {
    let Some(usage) = usage else { return };
    if let Some(tokens) = usage.prompt_tokens {
        context.push_str(&format!(", prompt_tokens={tokens}"));
    }
    if let Some(tokens) = usage.completion_tokens {
        context.push_str(&format!(", completion_tokens={tokens}"));
    }
    if let Some(tokens) = usage.total_tokens {
        context.push_str(&format!(", total_tokens={tokens}"));
    }
    if let Some(tokens) = usage
        .completion_tokens_details
        .as_ref()
        .and_then(|details| details.reasoning_tokens)
    {
        context.push_str(&format!(", reasoning_tokens={tokens}"));
    }
    if let Some(cost) = usage.cost.filter(|cost| cost.is_finite() && *cost >= 0.0) {
        context.push_str(&format!(", cost_usd={cost:.8}"));
    }
}

fn format_openrouter_send_error(error: reqwest::Error) -> String {
    let category = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_request() {
        "request"
    } else {
        "transport"
    };
    format!(
        "OpenRouter request failed (status=unavailable, generation_id=absent, declared_length=unknown, category={category})"
    )
}

fn format_openrouter_http_status_error(audit: &OpenRouterResponseAudit) -> String {
    format!(
        "OpenRouter returned an unsuccessful response ({})",
        openrouter_response_context(audit)
    )
}

fn openrouter_sse_json_category(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        serde_json::error::Category::Io => "event_json_io",
        serde_json::error::Category::Syntax => "event_json_syntax",
        serde_json::error::Category::Data => "event_json_data",
        serde_json::error::Category::Eof => "event_json_eof",
    }
}

fn format_openrouter_sse_failure(
    failure: &OpenRouterSseFailure,
    audit: &OpenRouterResponseAudit,
    max_tokens: u32,
) -> String {
    let mut details = openrouter_response_context(audit);
    details.push_str(&format!(
        ", content_type={}, category={}",
        audit.content_type, failure.category
    ));
    if let Some(event_index) = failure.event_index {
        details.push_str(&format!(", event_index={event_index}"));
    }
    if let Some(byte_length) = failure.byte_length {
        details.push_str(&format!(", byte_length={byte_length}"));
    }
    if let Some(hash) = failure.sha256.as_deref() {
        details.push_str(&format!(", event_sha256={hash}"));
    }
    if let Some(observed_bytes) = failure.observed_bytes {
        details.push_str(&format!(", observed_bytes={observed_bytes}"));
    }
    if let Some(limit_bytes) = failure.limit_bytes {
        details.push_str(&format!(", limit_bytes={limit_bytes}"));
    }
    if let Some(error_type) = failure.provider_error_type.as_deref() {
        details.push_str(&format!(", error_type={error_type}"));
    }
    append_openrouter_usage(&mut details, failure.usage.as_ref());

    if failure.category == "finish_length" {
        format!(
            "provider output reached max_tokens={max_tokens} and was rejected as truncated ({details})"
        )
    } else {
        format!("OpenRouter SSE response rejected ({details})")
    }
}

async fn consume_openrouter_sse_stream<S>(
    stream: S,
    audit: OpenRouterResponseAudit,
    max_source_bytes: usize,
    max_tokens: u32,
) -> Result<OpenRouterGeneration, String>
where
    S: Stream<Item = Result<Bytes, OpenRouterStreamReadFailure>>,
{
    futures_util::pin_mut!(stream);
    let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(max_source_bytes));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|failure| {
            let category = match failure {
                OpenRouterStreamReadFailure::Timeout => "timeout",
                OpenRouterStreamReadFailure::Transport => "transport",
            };
            format!(
                "OpenRouter SSE body read failed ({}, content_type={}, category={category}, received_bytes={})",
                openrouter_response_context(&audit),
                audit.content_type,
                parser.total_stream_bytes,
            )
        })?;
        parser
            .push_chunk(&chunk)
            .map_err(|failure| format_openrouter_sse_failure(&failure, &audit, max_tokens))?;
    }

    parser
        .finish(audit.generation_id.clone())
        .map_err(|failure| format_openrouter_sse_failure(&failure, &audit, max_tokens))
}

fn select_openrouter_response_id(
    body_id: Option<String>,
    header_id: Option<String>,
) -> Option<String> {
    body_id.or(header_id)
}

fn canonical_competition_prompt() -> String {
    format!(
        "SYSTEM\n{}\n\nUSER\n{}",
        ARENA_UNIFORM_SYSTEM_PROMPT, ARENA_UNIFORM_COMPETITION_PROMPT
    )
}

fn competition_prompt_sha256() -> String {
    sha256_hex(canonical_competition_prompt().as_bytes())
}

fn canonical_revision_prompt() -> String {
    format!(
        "SYSTEM\n{}\n\nUSER\n{}",
        ARENA_REVISION_SYSTEM_PROMPT, ARENA_REVISION_USER_PROMPT_PREFIX
    )
}

fn revision_prompt_sha256() -> String {
    sha256_hex(canonical_revision_prompt().as_bytes())
}

fn rust_crate_name(safe_model_id: &str) -> String {
    // The crate name is local to one non-incremental rustc invocation; source
    // and WASM artifact identity continues to use the exact safe model ID.
    // Therefore punctuation collapsing here cannot merge model artifacts.
    let mut crate_name = String::with_capacity("arena_".len() + safe_model_id.len());
    crate_name.push_str("arena_");
    for byte in safe_model_id.bytes() {
        crate_name.push(if byte.is_ascii_alphanumeric() || byte == b'_' {
            char::from(byte)
        } else {
            '_'
        });
    }
    crate_name
}

static ACTIVE_COMPILE_PATHS: OnceLock<(Mutex<HashSet<PathBuf>>, Condvar)> = OnceLock::new();

struct CompilePathGuard {
    path: PathBuf,
}

impl CompilePathGuard {
    fn acquire(path: &Path) -> Self {
        let (active_paths, changed) =
            ACTIVE_COMPILE_PATHS.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()));
        let path = path.to_owned();
        let mut active = active_paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while active.contains(&path) {
            active = changed
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        active.insert(path.clone());
        Self { path }
    }
}

impl Drop for CompilePathGuard {
    fn drop(&mut self) {
        let Some((active_paths, changed)) = ACTIVE_COMPILE_PATHS.get() else {
            return;
        };
        let mut active = active_paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        active.remove(&self.path);
        changed.notify_all();
    }
}

struct TemporaryCompileRoot {
    path: PathBuf,
}

impl TemporaryCompileRoot {
    fn create() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "mgs-arena-compile-verification-{}",
            Uuid::new_v4().simple()
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TemporaryCompileRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn append_compile_diagnostic(stderr: String, diagnostic: &str) -> String {
    if stderr.trim().is_empty() {
        diagnostic.to_owned()
    } else {
        format!("{stderr}\n{diagnostic}")
    }
}

const VERIFICATION_FINAL_BASENAME_DIAGNOSTIC: &str =
    "verification rustc output uses the final wasm basename because rustc embeds the output filename in the wasm name section";

// Verification deliberately compiles straight to the final-looking basename
// inside a private temporary directory. Unlike the publishing compiler, this
// path needs no atomic staging rename, and using a staging basename would alter
// rustc's embedded name section and make a byte-identical legacy rebuild look
// different from an artifact originally compiled directly to `<model>.wasm`.
fn compile_verification_artifact_impl(
    model_id: String,
    source_code: String,
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
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: "invalid model_id".to_owned(),
            warnings: vec!["model_id contains unsupported characters".to_owned()],
        };
    };

    let source_path = source_dir.join(format!("{}.rs", safe_model_id));
    let wasm_path = wasm_dir.join(format!("{}.wasm", safe_model_id));
    let mut warnings = Vec::new();

    if source_code.len() > max_source_bytes {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!(
                "source exceeds configured max ({} > {} bytes)",
                source_code.len(),
                max_source_bytes
            ),
            warnings,
        };
    }

    let validation = validate_source_impl(max_source_bytes, &source_code, Some("rust"));
    warnings.extend(validation.warnings);
    if !validation.valid {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("source failed validation: {}", validation.errors.join("; ")),
            warnings,
        };
    }

    if let Err(err) = fs::create_dir_all(&source_dir) {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("failed creating isolated source dir: {err}"),
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
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("failed creating isolated wasm dir: {err}"),
            warnings,
        };
    }
    if let Err(err) = atomic_replace_bytes(&source_path, source_code.as_bytes()) {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("failed writing isolated source: {err}"),
            warnings,
        };
    }

    let crate_name = rust_crate_name(&safe_model_id);
    let rustc_run = run_rustc_with_limits(&source_path, &wasm_path, &crate_name);
    let (output, timed_out) = match rustc_run {
        Ok(value) => value,
        Err(err) => {
            let _ = fs::remove_file(&wasm_path);
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path.display().to_string(),
                wasm_path: None,
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: String::new(),
                compiler_stderr: format!("failed to execute rustc. Ensure rustc is installed, wasm target is available, and compile sandbox limits are valid. {err}"),
                warnings,
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if timed_out {
        let _ = fs::remove_file(&wasm_path);
        warnings.push(format!(
            "rustc timed out after {}s and was terminated",
            rustc_timeout_secs()
        ));
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }
    if !output.status.success() {
        let _ = fs::remove_file(&wasm_path);
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }

    if let Err(err) = validate_compiled_wasm_export(&wasm_path) {
        let _ = fs::remove_file(&wasm_path);
        warnings.push(format!("compiled wasm failed export validation: {err}"));
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: Some(wasm_path.display().to_string()),
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }

    let wasm_bytes = match fs::read(&wasm_path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => {
            let _ = fs::remove_file(&wasm_path);
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path.display().to_string(),
                wasm_path: None,
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: truncate_for_api(&stdout, 4000),
                compiler_stderr: "compiler produced an empty wasm artifact".to_owned(),
                warnings,
            };
        }
        Err(err) => {
            let _ = fs::remove_file(&wasm_path);
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path.display().to_string(),
                wasm_path: None,
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: truncate_for_api(&stdout, 4000),
                compiler_stderr: format!("failed reading compiled wasm: {err}"),
                warnings,
            };
        }
    };

    CompileBotCodeResponse {
        model_id,
        source_path: source_path.display().to_string(),
        wasm_path: Some(wasm_path.display().to_string()),
        compiled: true,
        bytes_written: wasm_bytes.len(),
        wasm_sha256: Some(sha256_hex(&wasm_bytes)),
        compiler_stdout: truncate_for_api(&stdout, 4000),
        compiler_stderr: truncate_for_api(&stderr, 4000),
        warnings,
    }
}

fn verify_existing_compiled_code_impl(
    model_id: String,
    source_code: String,
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
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: "invalid model_id".to_owned(),
            warnings: vec!["model_id contains unsupported characters".to_owned()],
        };
    };

    let source_path = source_dir.join(format!("{}.rs", safe_model_id));
    let wasm_path = wasm_dir.join(format!("{}.wasm", safe_model_id));
    let source_path_display = source_path.display().to_string();
    let wasm_path_display = wasm_path.display().to_string();

    // The same path guard used by publishing compiles covers the complete
    // verification. A concurrent normal compile therefore cannot rewrite the
    // live source or replace the live WASM between compilation and comparison.
    let _live_path_guard = CompilePathGuard::acquire(&wasm_path);
    if !wasm_path.is_file() {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path_display,
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: "existing wasm artifact is required for verification".to_owned(),
            warnings: vec!["verification mode never publishes source or wasm".to_owned()],
        };
    }

    let temporary_root = match TemporaryCompileRoot::create() {
        Ok(root) => root,
        Err(err) => {
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path_display,
                wasm_path: Some(wasm_path_display),
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: String::new(),
                compiler_stderr: format!(
                    "failed creating isolated compile verification directory: {err}"
                ),
                warnings: vec!["verification mode never publishes source or wasm".to_owned()],
            };
        }
    };

    let staged_source_dir = temporary_root.path.join("source");
    let staged_wasm_dir = temporary_root.path.join("wasm");
    let staged = compile_verification_artifact_impl(
        model_id.clone(),
        source_code,
        max_source_bytes,
        staged_source_dir,
        staged_wasm_dir,
    );
    let mut warnings = staged.warnings;
    warnings.push("verification mode never publishes source or wasm".to_owned());

    if !staged.compiled {
        warnings.push(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC.to_owned());
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path_display,
            wasm_path: Some(wasm_path_display),
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: staged.compiler_stdout,
            compiler_stderr: staged.compiler_stderr,
            warnings,
        };
    }

    let Some(staged_wasm_path) = staged.wasm_path.as_deref() else {
        warnings.push(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC.to_owned());
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path_display,
            wasm_path: Some(wasm_path_display),
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: staged.compiler_stdout,
            compiler_stderr: append_compile_diagnostic(
                staged.compiler_stderr,
                "isolated compile reported success without an artifact path",
            ),
            warnings,
        };
    };
    let staged_wasm = match fs::read(staged_wasm_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            warnings.push(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC.to_owned());
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path_display,
                wasm_path: Some(wasm_path_display),
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: staged.compiler_stdout,
                compiler_stderr: append_compile_diagnostic(
                    staged.compiler_stderr,
                    &format!("failed reading isolated compiled wasm: {err}"),
                ),
                warnings,
            };
        }
    };
    let existing_wasm = match fs::read(&wasm_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            warnings.push(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC.to_owned());
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path_display,
                wasm_path: Some(wasm_path_display),
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: staged.compiler_stdout,
                compiler_stderr: append_compile_diagnostic(
                    staged.compiler_stderr,
                    &format!("failed reading existing wasm artifact: {err}"),
                ),
                warnings,
            };
        }
    };

    if staged_wasm != existing_wasm {
        warnings.push(
            "isolated compiled wasm did not byte-match the existing artifact; existing artifacts were unchanged"
                .to_owned(),
        );
        warnings.push(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC.to_owned());
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path_display,
            wasm_path: Some(wasm_path_display),
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: staged.compiler_stdout,
            compiler_stderr: append_compile_diagnostic(
                staged.compiler_stderr,
                "isolated compiled wasm does not match existing artifact",
            ),
            warnings,
        };
    }

    let wasm_sha256 = sha256_hex(&existing_wasm);
    warnings.push(
        "supplied source compiled byte-identically to the existing wasm; existing artifacts were unchanged"
            .to_owned(),
    );
    warnings.push(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC.to_owned());
    CompileBotCodeResponse {
        model_id,
        source_path: source_path_display,
        wasm_path: Some(wasm_path_display),
        compiled: true,
        bytes_written: existing_wasm.len(),
        wasm_sha256: Some(wasm_sha256),
        compiler_stdout: staged.compiler_stdout,
        compiler_stderr: staged.compiler_stderr,
        warnings,
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
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: "invalid model_id".to_owned(),
            warnings: vec!["model_id contains unsupported characters".to_owned()],
        };
    };

    let source_path = source_dir.join(format!("{}.rs", safe_model_id));
    let wasm_path = wasm_dir.join(format!("{}.wasm", safe_model_id));
    let mut warnings = Vec::new();

    if source_code.len() > max_source_bytes {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!(
                "source exceeds configured max ({} > {} bytes)",
                source_code.len(),
                max_source_bytes
            ),
            warnings,
        };
    }

    // Every compilation path, including archived-source rehydration, must pass
    // the same validation boundary. Keeping this here prevents a caller from
    // bypassing validation by using the compile-only endpoint directly.
    let validation = validate_source_impl(max_source_bytes, &source_code, Some("rust"));
    warnings.extend(validation.warnings);
    if !validation.valid {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("source failed validation: {}", validation.errors.join("; ")),
            warnings,
        };
    }

    if let Err(err) = fs::create_dir_all(&source_dir) {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
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
            wasm_sha256: None,
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
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: "wasm already exists".to_owned(),
            warnings,
        };
    }

    let _compile_path_guard = CompilePathGuard::acquire(&wasm_path);
    // A same-model request may have completed while this request waited.
    if wasm_path.exists() && !overwrite {
        warnings.push("existing wasm kept because overwrite=false".to_owned());
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: Some(wasm_path.display().to_string()),
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: "wasm already exists".to_owned(),
            warnings,
        };
    }

    if let Err(err) = atomic_replace_bytes(&source_path, source_code.as_bytes()) {
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: String::new(),
            compiler_stderr: format!("failed writing source: {}", err),
            warnings,
        };
    }

    let crate_name = rust_crate_name(&safe_model_id);
    let staged_wasm_path = stable_wasm_staging_path(&wasm_path);
    let _ = fs::remove_file(&staged_wasm_path);
    let rustc_run = run_rustc_with_limits(&source_path, &staged_wasm_path, &crate_name);

    let (output, timed_out) = match rustc_run {
        Ok(value) => value,
        Err(err) => {
            let _ = fs::remove_file(&staged_wasm_path);
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path.display().to_string(),
                wasm_path: None,
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: String::new(),
                compiler_stderr: format!(
                    "failed to execute rustc. Ensure rustc is installed, wasm target is \
available, and compile sandbox limits are valid. {}",
                    err
                ),
                warnings,
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if timed_out {
        let _ = fs::remove_file(&staged_wasm_path);
        warnings.push(format!(
            "rustc timed out after {}s and was terminated",
            rustc_timeout_secs()
        ));
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }
    if !output.status.success() {
        let _ = fs::remove_file(&staged_wasm_path);
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }

    if let Err(err) = validate_compiled_wasm_export(&staged_wasm_path) {
        let _ = fs::remove_file(&staged_wasm_path);
        warnings.push(format!("compiled wasm failed export validation: {}", err));
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: Some(wasm_path.display().to_string()),
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: truncate_for_api(&stderr, 4000),
            warnings,
        };
    }

    let wasm_bytes = match fs::read(&staged_wasm_path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => {
            let _ = fs::remove_file(&staged_wasm_path);
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path.display().to_string(),
                wasm_path: None,
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: truncate_for_api(&stdout, 4000),
                compiler_stderr: "compiler produced an empty wasm artifact".to_owned(),
                warnings,
            };
        }
        Err(err) => {
            let _ = fs::remove_file(&staged_wasm_path);
            return CompileBotCodeResponse {
                model_id,
                source_path: source_path.display().to_string(),
                wasm_path: None,
                compiled: false,
                bytes_written: 0,
                wasm_sha256: None,
                compiler_stdout: truncate_for_api(&stdout, 4000),
                compiler_stderr: format!("failed reading compiled wasm: {err}"),
                warnings,
            };
        }
    };
    let bytes_written = wasm_bytes.len();
    let wasm_sha256 = sha256_hex(&wasm_bytes);
    if !overwrite && wasm_path.exists() {
        let _ = fs::remove_file(&staged_wasm_path);
        warnings.push("existing wasm kept because overwrite=false".to_owned());
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: Some(wasm_path.display().to_string()),
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: "wasm already exists".to_owned(),
            warnings,
        };
    }
    if let Err(err) = fs::rename(&staged_wasm_path, &wasm_path) {
        let _ = fs::remove_file(&staged_wasm_path);
        return CompileBotCodeResponse {
            model_id,
            source_path: source_path.display().to_string(),
            wasm_path: None,
            compiled: false,
            bytes_written: 0,
            wasm_sha256: None,
            compiler_stdout: truncate_for_api(&stdout, 4000),
            compiler_stderr: format!("failed publishing compiled wasm: {err}"),
            warnings,
        };
    }
    CompileBotCodeResponse {
        model_id,
        source_path: source_path.display().to_string(),
        wasm_path: Some(wasm_path.display().to_string()),
        compiled: true,
        bytes_written,
        wasm_sha256: Some(wasm_sha256),
        compiler_stdout: truncate_for_api(&stdout, 4000),
        compiler_stderr: truncate_for_api(&stderr, 4000),
        warnings,
    }
}

fn temporary_sibling_path(path: &Path, extension: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("arena-artifact");
    path.with_file_name(format!(
        ".{file_name}.{}.tmp.{extension}",
        Uuid::new_v4().simple()
    ))
}

fn stable_wasm_staging_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("arena-fighter.wasm");
    path.with_file_name(format!(".{file_name}.compile.wasm"))
}

fn atomic_replace_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let tmp_path = temporary_sibling_path(path, "data");
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn validate_compiled_wasm_export(path: &Path) -> Result<(), String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config).map_err(|err| err.to_string())?;
    let module = Module::from_file(&engine, path).map_err(|err| err.to_string())?;
    if let Some(export) = module.get_export("bot_tick_v2") {
        return validate_compiled_tick_type(export, "bot_tick_v2", 11);
    }
    let Some(export) = module.get_export("bot_tick") else {
        return Err("missing 'bot_tick_v2' or legacy 'bot_tick' export".to_owned());
    };
    validate_compiled_tick_type(export, "bot_tick", 4)
}

fn validate_compiled_tick_type(
    export: ExternType,
    export_name: &str,
    expected_params: usize,
) -> Result<(), String> {
    let ExternType::Func(func) = export else {
        return Err(format!("'{}' export is not a function", export_name));
    };
    if func.params().len() != expected_params || func.results().len() != 1 {
        return Err(format!(
            "{} signature must have {} i32 parameters and one i32 result",
            export_name, expected_params
        ));
    }
    if !func.params().all(|param| matches!(param, ValType::I32)) {
        return Err(format!("{} parameters must be i32", export_name));
    }
    if !matches!(func.results().next(), Some(ValType::I32)) {
        return Err(format!("{} return type must be i32", export_name));
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
        ("proc_macro", "proc_macro", "procmacro"),
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

    // These identifiers provide compile-time access to the parent process or
    // filesystem. Match lexical identifiers everywhere (including comments
    // and strings) so whitespace/comments cannot disguise a macro invocation.
    // `mod` is forbidden because an out-of-line module can read sibling or
    // absolute-path source files during compilation.
    for identifier in [
        "env",
        "option_env",
        "include",
        "include_str",
        "include_bytes",
        "macro_rules",
        "mod",
    ] {
        if source_contains_identifier(source, identifier) {
            errors.push(format!(
                "forbidden compile-time identifier detected: '{}'",
                identifier
            ));
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

fn source_contains_identifier(source: &str, expected: &str) -> bool {
    source
        .split(|ch: char| !(ch == '_' || ch.is_alphanumeric()))
        .any(|identifier| identifier == expected)
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

fn inline_admin_expected_token() -> Option<&'static String> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            std::env::var("MGS_ADMIN_BEARER_TOKEN")
                .or_else(|_| std::env::var("MGS_ADMIN_TOKEN"))
                .ok()
                .map(|raw| raw.trim().to_owned())
                .filter(|raw| !raw.is_empty())
        })
        .as_ref()
}

fn parse_bearer_token(authorization_header: Option<&str>) -> Option<&str> {
    let raw = authorization_header?.trim();
    if raw.is_empty() {
        return None;
    }
    raw.strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    left_bytes.len() == right_bytes.len() && left_bytes.ct_eq(right_bytes).into()
}

fn inline_admin_authorized(authorization_header: Option<&str>) -> bool {
    let Some(expected) = inline_admin_expected_token() else {
        return false;
    };
    let Some(provided) = parse_bearer_token(authorization_header) else {
        return false;
    };
    constant_time_eq(expected.as_str(), provided)
}

fn rustc_timeout_secs() -> u64 {
    static VALUE: OnceLock<u64> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("MGS_CODEGEN_RUSTC_TIMEOUT_SECS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_RUSTC_TIMEOUT_SECS)
            .clamp(1, MAX_RUSTC_TIMEOUT_SECS)
    })
}

fn rustc_cpu_limit_secs() -> Option<u64> {
    static VALUE: OnceLock<Option<u64>> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("MGS_CODEGEN_RUSTC_CPU_LIMIT_SECS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map(|value| value.clamp(1, MAX_RUSTC_CPU_LIMIT_SECS))
            .or(Some(DEFAULT_RUSTC_CPU_LIMIT_SECS))
    })
}

fn rustc_memory_limit_bytes() -> Option<u64> {
    static VALUE: OnceLock<Option<u64>> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("MGS_CODEGEN_RUSTC_MEMORY_LIMIT_MB")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map(|value| value.clamp(64, MAX_RUSTC_MEMORY_LIMIT_MB))
            .or(Some(DEFAULT_RUSTC_MEMORY_LIMIT_MB))
            .map(|mb| mb.saturating_mul(1024 * 1024))
    })
}

fn apply_rustc_child_limits(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let cpu_limit_secs = rustc_cpu_limit_secs();
        let memory_limit_bytes = rustc_memory_limit_bytes();
        // SAFETY: pre_exec runs in the child process after fork and before exec.
        // We only invoke async-signal-safe libc::setrlimit and return I/O errors.
        unsafe {
            command.pre_exec(move || {
                if let Some(cpu_secs) = cpu_limit_secs {
                    let limit = libc::rlimit {
                        rlim_cur: cpu_secs as libc::rlim_t,
                        rlim_max: cpu_secs as libc::rlim_t,
                    };
                    if libc::setrlimit(libc::RLIMIT_CPU, &limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                if let Some(memory_bytes) = memory_limit_bytes {
                    let limit = libc::rlimit {
                        rlim_cur: memory_bytes as libc::rlim_t,
                        rlim_max: memory_bytes as libc::rlim_t,
                    };
                    // RLIMIT_AS also counts rustc/LLVM's large file-backed
                    // library mappings and can make even a tiny safe source
                    // fail before parsing. RLIMIT_DATA constrains writable
                    // compiler allocations while allowing those code maps.
                    if libc::setrlimit(libc::RLIMIT_DATA, &limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
}

fn run_rustc_with_limits(
    source_path: &Path,
    wasm_path: &Path,
    crate_name: &str,
) -> Result<(Output, bool), String> {
    let rustc_path = rustc_executable_path()?;
    let mut command = Command::new(rustc_path);
    command
        // Generated source is untrusted. In particular, env!/option_env! must
        // never be able to observe the game server's credentials even if a
        // future validator regression lets one through.
        .env_clear()
        .arg("--edition=2021")
        .arg("--crate-name")
        .arg(crate_name)
        .arg("--crate-type=cdylib")
        .arg("--target=wasm32-unknown-unknown")
        .arg("-O")
        .arg(source_path)
        .arg("-o")
        .arg(wasm_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_rustc_child_limits(&mut command);

    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to execute rustc: {}", err))?;
    let timeout = Duration::from_secs(rustc_timeout_secs());
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|err| format!("failed collecting rustc output: {}", err))?;
                return Ok((output, false));
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let output = child.wait_with_output().map_err(|err| {
                        format!("failed collecting timed-out rustc output: {}", err)
                    })?;
                    return Ok((output, true));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => {
                return Err(format!("failed while waiting for rustc: {}", err));
            }
        }
    }
}

fn rustc_executable_path() -> Result<PathBuf, String> {
    static RUSTC_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    RUSTC_PATH
        .get_or_init(resolve_rustc_executable_path)
        .clone()
}

fn resolve_rustc_executable_path() -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os("MGS_CODEGEN_RUSTC_PATH") {
        let path = PathBuf::from(configured);
        if !path.is_absolute() {
            return Err("MGS_CODEGEN_RUSTC_PATH must be absolute".to_owned());
        }
        if !path.is_file() {
            return Err(format!(
                "MGS_CODEGEN_RUSTC_PATH is not a file: {}",
                path.display()
            ));
        }
        return Ok(path);
    }

    let search_path = std::env::var_os("PATH")
        .ok_or_else(|| "PATH is not configured and MGS_CODEGEN_RUSTC_PATH is unset".to_owned())?;
    for directory in std::env::split_paths(&search_path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join("rustc");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("could not resolve an absolute rustc executable path".to_owned())
}

pub fn build_code_generation_routes(
    service: CodeGenerationService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    // Source code validation bodies can be up to 128 KB (max source size + JSON overhead).
    // Generation requests are small JSON bodies so 64 KB suffices.
    let json_body_limit = 1024 * 64;
    let source_body_limit = 256 * 1024; // 256 KB for source code payloads

    let status = warp::path!("api" / "arena" / "code" / "status")
        .and(warp::get())
        .and(warp::header::optional::<String>("authorization"))
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, service: CodeGenerationService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                ok_response(service.status())
            },
        );

    let validate = warp::path!("api" / "arena" / "code" / "validate")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(source_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>,
             body: ValidateBotCodeBody,
             service: CodeGenerationService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                ok_response(service.validate_source(body))
            },
        );

    let generate = warp::path!("api" / "arena" / "code" / "generate")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .and_then(
            |authorization: Option<String>,
             body: GenerateBotCodeBody,
             service: CodeGenerationService| async move {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return Ok::<_, warp::Rejection>(error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    ));
                }
                let reply = match service.generate_bot_code(body).await {
                    Ok(response) => ok_response(response),
                    Err(err) => error_response(err.code, err.message),
                };
                Ok::<_, warp::Rejection>(reply)
            },
        );

    let generate_and_compile = warp::path!("api" / "arena" / "code" / "generate_and_compile")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .and_then(
            |authorization: Option<String>,
             body: GenerateAndCompileBotCodeBody,
             service: CodeGenerationService| async move {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return Ok::<_, warp::Rejection>(error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    ));
                }
                let reply = match service.generate_and_compile(body).await {
                    Ok(response) => ok_response(response),
                    Err(err) => error_response(err.code, err.message),
                };
                Ok::<_, warp::Rejection>(reply)
            },
        );

    // Rehydrates an archived, already-generated season source into the active
    // WASM directory. This never contacts a model provider and lets a resumed
    // evaluator prove that its live fighter matches the immutable source
    // checkpoint before any cached battle result can be reused.
    let compile = warp::path!("api" / "arena" / "code" / "compile")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(source_body_limit))
        .and(warp::body::json())
        .and(with_service(service))
        .and_then(
            |authorization: Option<String>,
             body: CompileBotCodeBody,
             service: CodeGenerationService| async move {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return Ok::<_, warp::Rejection>(error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    ));
                }
                Ok::<_, warp::Rejection>(ok_response(service.compile_bot_code(body).await))
            },
        );

    status
        .or(validate)
        .or(generate)
        .or(generate_and_compile)
        .or(compile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sse_test_audit() -> OpenRouterResponseAudit {
        OpenRouterResponseAudit {
            status: reqwest::StatusCode::OK,
            generation_id: Some("gen-safe-123".to_owned()),
            declared_content_length: Some(12_345),
            content_type: "text/event-stream".to_owned(),
        }
    }

    fn valid_sse_fixture() -> Vec<u8> {
        let first = serde_json::json!({
            "id": "body-generation-id",
            "model": "provider/model",
            "provider": "Provider Name",
            "choices": [{
                "index": 0,
                "delta": {"content": "fn bot_tick_v2() -> &'static str { \"caf"},
                "finish_reason": null
            }]
        })
        .to_string();
        let split_at = first
            .find(',')
            .expect("fixture must have a safe JSON whitespace split")
            + 1;
        let second = serde_json::json!({
            "id": "body-generation-id",
            "model": "provider/model",
            "provider": "Provider Name",
            "choices": [{
                "index": 0,
                "delta": {"content": "é\" }\n"},
                "finish_reason": "stop"
            }]
        })
        .to_string();
        let usage = serde_json::json!({
            "id": "body-generation-id",
            "model": "provider/model",
            "provider": "Provider Name",
            "choices": [],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 25,
                "total_tokens": 125,
                "cost": 0.0125,
                "completion_tokens_details": {"reasoning_tokens": 7}
            }
        })
        .to_string();

        let mut fixture = Vec::new();
        fixture.extend_from_slice(b"\xef\xbb\xbf: OPENROUTER PROCESSING\r\n\r\n");
        fixture.extend_from_slice(b"event: completion\r");
        fixture.extend_from_slice(format!("data: {}\r", &first[..split_at]).as_bytes());
        fixture.extend_from_slice(format!("data: {}\r\r", &first[split_at..]).as_bytes());
        fixture.extend_from_slice(b": keepalive during generation\n\n");
        fixture.extend_from_slice(format!("data: {second}\n\n").as_bytes());
        fixture.extend_from_slice(format!("retry: 1000\r\ndata: {usage}\r\n\r\n").as_bytes());
        fixture.extend_from_slice(b"data: [DONE]\n\n");
        fixture
    }

    fn parse_sse_fixture(
        fixture: &[u8],
        fragment_bytes: usize,
        limits: OpenRouterSseLimits,
    ) -> Result<OpenRouterGeneration, Box<OpenRouterSseFailure>> {
        let mut parser = OpenRouterSseParser::new(limits);
        for chunk in fixture.chunks(fragment_bytes.max(1)) {
            parser.push_chunk(chunk)?;
        }
        parser.finish(Some("header-generation-id".to_owned()))
    }

    fn expect_sse_failure(
        result: Result<OpenRouterGeneration, Box<OpenRouterSseFailure>>,
        message: &str,
    ) -> Box<OpenRouterSseFailure> {
        match result {
            Ok(_) => panic!("{message}"),
            Err(failure) => failure,
        }
    }

    #[test]
    fn default_source_limit_is_50_kib() {
        assert_eq!(DEFAULT_MAX_SOURCE_BYTES, 51_200);
    }

    #[test]
    fn uniform_prompt_has_stable_provenance_and_collaboration_contract() {
        let canonical = canonical_competition_prompt();
        let hash = competition_prompt_sha256();
        assert!(canonical.starts_with("SYSTEM\n"));
        assert_eq!(ARENA_COMPETITION_PROMPT_VERSION, "arena-rust-v3.1.0");
        assert!(
            canonical.contains("Begin the Rust source immediately; do not analyze exhaustively.")
        );
        assert!(canonical.contains("An incomplete file is a failed submission."));
        assert!(canonical.contains("pub extern \"C\" fn bot_tick_v2("));
        assert!(canonical.contains("COLLABORATION"));
        assert!(canonical.contains("51,200 bytes"));
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(hash, competition_prompt_sha256());
    }

    #[test]
    fn status_exposes_revision_contract() {
        let service = CodeGenerationService::new_from_env();
        let status = service.status();
        assert_eq!(
            status.revision_prompt_version,
            ARENA_REVISION_PROMPT_VERSION
        );
        assert_eq!(status.revision_prompt_sha256, revision_prompt_sha256());
        assert_eq!(revision_prompt_sha256().len(), 64);
        // template is deterministic
        assert_eq!(revision_prompt_sha256(), revision_prompt_sha256());
        // revision contract differs from the generation contract
        assert_ne!(status.revision_prompt_sha256, status.prompt_sha256);
    }

    #[test]
    fn source_limit_is_an_exact_utf8_byte_boundary() {
        let prefix = "fn bot_tick_v2() {}";
        let exact = format!(
            "{}{}",
            prefix,
            " ".repeat(DEFAULT_MAX_SOURCE_BYTES - prefix.len())
        );
        assert_eq!(exact.len(), DEFAULT_MAX_SOURCE_BYTES);
        assert!(validate_source_impl(DEFAULT_MAX_SOURCE_BYTES, &exact, Some("rust")).valid);

        let oversized = format!("{}x", exact);
        let validation =
            validate_source_impl(DEFAULT_MAX_SOURCE_BYTES, oversized.as_str(), Some("rust"));
        assert!(!validation.valid);
        assert!(validation
            .errors
            .iter()
            .any(|error| error.contains("exceeds max size")));
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
        temp_env::async_with_vars(
            [
                ("OPENROUTER_API_KEY", None::<&str>),
                ("OPENROUTER_API_KEY_FILE", None::<&str>),
            ],
            async {
                let service = CodeGenerationService::new_from_env();
                let status = service.status();
                assert_eq!(status.provider_sort_policy, ARENA_PROVIDER_SORT_POLICY);
                assert_eq!(
                    status.response_transport_policy,
                    ARENA_RESPONSE_TRANSPORT_POLICY
                );
                let response = service
                    .generate_bot_code(GenerateBotCodeBody {
                        model: "openai/gpt-4o".to_owned(),
                        objective: Some("request-specific objective is ignored".to_owned()),
                        prompt_style: Some("request-specific style is ignored".to_owned()),
                        reasoning_mode: Some("disabled".to_owned()),
                        reasoning_effort: None,
                    })
                    .await
                    .expect("generation should work");
                assert!(response.source_code.contains("bot_tick_v2"));
                assert!(response.simulated);
                assert_eq!(response.prompt_version, ARENA_COMPETITION_PROMPT_VERSION);
                assert_eq!(response.prompt_sha256, competition_prompt_sha256());
                assert_eq!(response.prompt_text, canonical_competition_prompt());
                assert_eq!(
                    response.max_completion_tokens,
                    DEFAULT_OPENROUTER_MAX_TOKENS
                );
                assert_eq!(response.provider_sort_policy, ARENA_PROVIDER_SORT_POLICY);
                assert_eq!(
                    response.provider_require_parameters,
                    ARENA_PROVIDER_REQUIRE_PARAMETERS
                );
                assert_eq!(response.temperature_policy, ARENA_TEMPERATURE_POLICY);
                assert_eq!(
                    response.reasoning_policy_version,
                    ARENA_REASONING_POLICY_VERSION
                );
                assert_eq!(response.reasoning_mode, "disabled");
                assert!(response.reasoning_effort.is_none());
                assert_eq!(response.reasoning_exclude, ARENA_REASONING_EXCLUDE);
                assert_eq!(
                    response.response_transport_policy,
                    ARENA_RESPONSE_TRANSPORT_POLICY
                );
                assert!(response.usage.is_none());
            },
        )
        .await;
    }

    #[test]
    fn openrouter_request_serializes_disabled_reasoning_and_throughput_routing() {
        let policy = normalize_arena_reasoning_policy(Some("disabled"), None)
            .expect("disabled policy should normalize");
        let request = build_openrouter_request("provider/model", 16_384, &policy);
        let serialized = serde_json::to_value(request).expect("request should serialize");

        assert_eq!(serialized["model"], "provider/model");
        assert_eq!(serialized["max_tokens"], 16_384);
        assert_eq!(serialized["stream"], true);
        assert!(serialized.get("stream_options").is_none());
        assert!(serialized.get("max_completion_tokens").is_none());
        assert_eq!(serialized["reasoning"]["effort"], "none");
        assert_eq!(serialized["reasoning"]["exclude"], true);
        assert_eq!(serialized["provider"]["sort"], "throughput");
        assert_eq!(serialized["provider"]["require_parameters"], true);
        assert_eq!(ARENA_PROVIDER_SORT_POLICY, "throughput");
        assert!(
            serialized.get("temperature").is_none(),
            "temperature must remain provider-default for cross-model compatibility"
        );
        assert_eq!(ARENA_TEMPERATURE_POLICY, "provider_default");
    }

    #[test]
    fn openrouter_request_serializes_mandatory_minimum_reasoning() {
        let policy = normalize_arena_reasoning_policy(Some("minimum"), Some("low"))
            .expect("minimum policy should normalize");
        let request = build_openrouter_request("provider/model", 16_384, &policy);
        let serialized = serde_json::to_value(request).expect("request should serialize");

        assert_eq!(serialized["reasoning"]["effort"], "low");
        assert_eq!(serialized["reasoning"]["exclude"], true);
    }

    #[test]
    fn openrouter_request_omits_reasoning_for_unsupported_models() {
        let policy = normalize_arena_reasoning_policy(Some("unsupported"), None)
            .expect("unsupported policy should normalize");
        let request = build_openrouter_request("provider/model", 16_384, &policy);
        let serialized = serde_json::to_value(request).expect("request should serialize");

        assert!(serialized.get("reasoning").is_none());
        assert_eq!(serialized["provider"]["require_parameters"], true);
    }

    #[test]
    fn arena_reasoning_policy_rejects_invalid_combinations() {
        assert!(normalize_arena_reasoning_policy(Some("minimum"), None).is_err());
        assert!(normalize_arena_reasoning_policy(Some("disabled"), Some("low")).is_err());
        assert!(normalize_arena_reasoning_policy(Some("minimum"), Some("none")).is_err());
        assert!(normalize_arena_reasoning_policy(Some("unknown"), None).is_err());
    }

    #[test]
    fn openrouter_response_metadata_is_sanitized_for_diagnostics() {
        let safe_id = HeaderValue::from_static("gen-1234_safe.id:attempt-2");
        assert_eq!(
            sanitize_openrouter_generation_id(Some(&safe_id)).as_deref(),
            Some("gen-1234_safe.id:attempt-2")
        );

        let unsafe_id = HeaderValue::from_static("generation id with spaces");
        let sanitized = sanitize_openrouter_generation_id(Some(&unsafe_id))
            .expect("a present header should retain a safe fingerprint");
        assert!(sanitized.starts_with("sha256:"));
        assert!(!sanitized.contains("generation id with spaces"));

        let content_type = HeaderValue::from_static("Application/JSON; charset=utf-8");
        assert_eq!(
            sanitize_openrouter_content_type(Some(&content_type)),
            "application/json"
        );
    }

    #[test]
    fn openrouter_sse_parser_handles_one_byte_fragments_utf8_and_all_line_endings() {
        let generation = parse_sse_fixture(
            &valid_sse_fixture(),
            1,
            OpenRouterSseLimits::production(DEFAULT_MAX_SOURCE_BYTES),
        )
        .expect("fragmented fixture should parse");

        assert_eq!(
            generation.source_code,
            "fn bot_tick_v2() -> &'static str { \"café\" }"
        );
        assert_eq!(generation.finish_reason.as_deref(), Some("stop"));
        assert_eq!(generation.resolved_model.as_deref(), Some("provider/model"));
        assert_eq!(generation.provider_name.as_deref(), Some("Provider Name"));
        assert_eq!(
            generation.provider_response_id.as_deref(),
            Some("body-generation-id")
        );
        let usage = generation.usage.expect("final usage is required");
        assert_eq!(usage.prompt_tokens, Some(100));
        assert_eq!(usage.completion_tokens, Some(25));
        assert_eq!(usage.total_tokens, Some(125));
        assert_eq!(usage.cost, Some(0.0125));
        assert_eq!(
            usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens),
            Some(7)
        );
    }

    #[test]
    fn openrouter_sse_accepts_deepinfra_combined_terminal_usage_chunk() {
        let fixture = concat!(
            "data: {\"id\":\"deepinfra-generation-id\",\"model\":\"provider/model\",\"provider\":\"DeepInfra\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fn bot_tick_v2() {}\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"deepinfra-generation-id\",\"model\":\"provider/model\",\"provider\":\"DeepInfra\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":25,\"total_tokens\":125,\"cost\":0.0125,\"completion_tokens_details\":{\"reasoning_tokens\":7}}}\n\n",
            "data: [DONE]\n\n"
        );
        let generation = parse_sse_fixture(
            fixture.as_bytes(),
            1,
            OpenRouterSseLimits::production(DEFAULT_MAX_SOURCE_BYTES),
        )
        .expect("DeepInfra's combined terminal choice and usage must parse");

        assert_eq!(generation.source_code, "fn bot_tick_v2() {}");
        assert_eq!(generation.finish_reason.as_deref(), Some("stop"));
        assert_eq!(generation.provider_name.as_deref(), Some("DeepInfra"));
        let usage = generation.usage.expect("combined final usage is required");
        assert_eq!(usage.prompt_tokens, Some(100));
        assert_eq!(usage.completion_tokens, Some(25));
        assert_eq!(usage.total_tokens, Some(125));
        assert_eq!(usage.cost, Some(0.0125));
    }

    #[test]
    fn openrouter_sse_accepts_inert_choice_in_post_finish_usage_chunk() {
        let placeholder = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fn bot_tick_v2() {}\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":25,\"total_tokens\":125,\"cost\":0.0125}}\n\n",
            "data: [DONE]\n\n"
        );
        let repeated_finish = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fn bot_tick_v2() {}\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":25,\"total_tokens\":125,\"cost\":0.0125}}\n\n",
            "data: [DONE]\n\n"
        );

        for fixture in [placeholder, repeated_finish] {
            let generation = parse_sse_fixture(
                fixture.as_bytes(),
                1,
                OpenRouterSseLimits::production(DEFAULT_MAX_SOURCE_BYTES),
            )
            .expect("an inert post-finish choice in the final usage event must parse");
            assert_eq!(generation.source_code, "fn bot_tick_v2() {}");
            assert_eq!(generation.finish_reason.as_deref(), Some("stop"));
            assert_eq!(
                generation.usage.and_then(|usage| usage.completion_tokens),
                Some(25)
            );
        }
    }

    #[test]
    fn openrouter_sse_total_bound_allows_legal_source_with_many_small_events() {
        let prefix = "fn bot_tick_v2() {}";
        let target_source_bytes = 51_000;
        let mut remaining = target_source_bytes - prefix.len();
        let mut fixture = String::new();
        fixture.push_str(&format!(
            "data: {}\n\n",
            serde_json::json!({
                "id": "generation-id-repeated-for-realistic-framing-overhead",
                "model": "provider/model-with-repeated-envelope-metadata",
                "provider": "Provider Name With Repeated Metadata",
                "choices": [{"index": 0, "delta": {"content": prefix}, "finish_reason": null}]
            })
        ));
        while remaining > 0 {
            let fragment_len = remaining.min(10);
            remaining -= fragment_len;
            let finish_reason = if remaining == 0 {
                serde_json::Value::String("stop".to_owned())
            } else {
                serde_json::Value::Null
            };
            fixture.push_str(&format!(
                "data: {}\n\n",
                serde_json::json!({
                    "id": "generation-id-repeated-for-realistic-framing-overhead",
                    "model": "provider/model-with-repeated-envelope-metadata",
                    "provider": "Provider Name With Repeated Metadata",
                    "choices": [{
                        "index": 0,
                        "delta": {"content": "x".repeat(fragment_len)},
                        "finish_reason": finish_reason
                    }]
                })
            ));
        }
        fixture.push_str(concat!(
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":51000,\"total_tokens\":51001}}\n\n",
            "data: [DONE]\n\n"
        ));
        assert!(
            fixture.len() > 512 * 1024,
            "fixture must exercise framing overhead"
        );

        let generation = parse_sse_fixture(
            fixture.as_bytes(),
            4_096,
            OpenRouterSseLimits::production(DEFAULT_MAX_SOURCE_BYTES),
        )
        .expect("a legal source must fit despite repeated SSE envelopes");
        assert_eq!(generation.source_code.len(), target_source_bytes);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn openrouter_sse_async_fixture_consumes_split_frames() {
        let fixture = valid_sse_fixture();
        let chunks = fixture
            .chunks(7)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<Result<Bytes, OpenRouterStreamReadFailure>>>();
        let generation = consume_openrouter_sse_stream(
            futures_util::stream::iter(chunks),
            sse_test_audit(),
            DEFAULT_MAX_SOURCE_BYTES,
            16_384,
        )
        .await
        .expect("async SSE fixture should parse");
        assert!(generation.source_code.contains("café"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn openrouter_sse_read_error_is_sanitized_and_keeps_response_context() {
        let private_partial =
            b"data: {\"choices\":[{\"delta\":{\"content\":\"PRIVATE SOURCE\"}}]}\n\n";
        let stream = futures_util::stream::iter(vec![
            Ok(Bytes::copy_from_slice(private_partial)),
            Err(OpenRouterStreamReadFailure::Timeout),
        ]);
        let result = consume_openrouter_sse_stream(
            stream,
            sse_test_audit(),
            DEFAULT_MAX_SOURCE_BYTES,
            16_384,
        )
        .await;
        let error = match result {
            Ok(_) => panic!("timeout after partial content must be fatal"),
            Err(error) => error,
        };

        assert!(error.contains("status=200"));
        assert!(error.contains("generation_id=gen-safe-123"));
        assert!(error.contains("declared_length=12345"));
        assert!(error.contains("category=timeout"));
        assert!(error.contains(&format!("received_bytes={}", private_partial.len())));
        assert!(!error.contains("PRIVATE SOURCE"));
    }

    #[test]
    fn openrouter_sse_malformed_event_is_hashed_without_raw_content() {
        let private_event = b"{\"content\":\"PRIVATE FIGHTER SOURCE\"";
        let fixture = [b"data: ".as_slice(), private_event, b"\n\n"].concat();
        let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(51_200));
        let failure = parser
            .push_chunk(&fixture)
            .expect_err("malformed event must fail");
        let diagnostic = format_openrouter_sse_failure(&failure, &sse_test_audit(), 16_384);

        assert_eq!(failure.category, "event_json_eof");
        assert!(diagnostic.contains(&format!("byte_length={}", private_event.len())));
        assert!(diagnostic.contains(&format!("event_sha256={}", sha256_hex(private_event))));
        assert!(diagnostic.contains("status=200"));
        assert!(diagnostic.contains("generation_id=gen-safe-123"));
        assert!(diagnostic.contains("declared_length=12345"));
        assert!(!diagnostic.contains("PRIVATE FIGHTER SOURCE"));
    }

    #[test]
    fn openrouter_sse_rejects_top_level_and_choice_level_errors_safely() {
        for (fixture, expected_category, expected_type) in [
            (
                r#"data: {"error":{"code":503,"message":"PRIVATE PROVIDER MESSAGE","metadata":{"error_type":"provider_unavailable"}},"choices":[{"index":0,"delta":{},"finish_reason":"error"}]}

"#,
                "provider_error",
                "provider_unavailable",
            ),
            (
                r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"error","error":{"code":"server_error","message":"PRIVATE CHOICE MESSAGE","metadata":{"error_type":"server"}}}]}

"#,
                "provider_choice_error",
                "server",
            ),
        ] {
            let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(51_200));
            let failure = parser
                .push_chunk(fixture.as_bytes())
                .expect_err("provider error event must fail");
            assert_eq!(failure.category, expected_category);
            assert_eq!(failure.provider_error_type.as_deref(), Some(expected_type));
            let diagnostic = format_openrouter_sse_failure(&failure, &sse_test_audit(), 16_384);
            assert!(!diagnostic.contains("PRIVATE PROVIDER MESSAGE"));
            assert!(!diagnostic.contains("PRIVATE CHOICE MESSAGE"));
        }
    }

    #[test]
    fn openrouter_sse_length_error_keeps_numeric_usage() {
        let fixture = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"PRIVATE SOURCE\"},\"finish_reason\":\"length\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":16384,\"total_tokens\":16484,\"cost\":0.0123456789,\"completion_tokens_details\":{\"reasoning_tokens\":2048}}}\n\n"
        );
        let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(51_200));
        let failure = parser
            .push_chunk(fixture.as_bytes())
            .expect_err("length finish must fail after final usage");
        let diagnostic = format_openrouter_sse_failure(&failure, &sse_test_audit(), 16_384);

        assert_eq!(failure.category, "finish_length");
        assert!(diagnostic.contains("max_tokens=16384"));
        assert!(diagnostic.contains("prompt_tokens=100"));
        assert!(diagnostic.contains("completion_tokens=16384"));
        assert!(diagnostic.contains("reasoning_tokens=2048"));
        assert!(diagnostic.contains("cost_usd=0.01234568"));
        assert!(!diagnostic.contains("PRIVATE SOURCE"));
    }

    #[test]
    fn openrouter_sse_combined_length_and_usage_keeps_numeric_audit() {
        let fixture = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"PRIVATE SOURCE\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":16384,\"total_tokens\":16484,\"cost\":0.0123456789,\"completion_tokens_details\":{\"reasoning_tokens\":15902}}}\n\n"
        );
        let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(51_200));
        let failure = parser
            .push_chunk(fixture.as_bytes())
            .expect_err("combined length finish must retain final usage and fail");
        let diagnostic = format_openrouter_sse_failure(&failure, &sse_test_audit(), 16_384);

        assert_eq!(failure.category, "finish_length");
        assert!(diagnostic.contains("max_tokens=16384"));
        assert!(diagnostic.contains("prompt_tokens=100"));
        assert!(diagnostic.contains("completion_tokens=16384"));
        assert!(diagnostic.contains("reasoning_tokens=15902"));
        assert!(diagnostic.contains("cost_usd=0.01234568"));
        assert!(!diagnostic.contains("PRIVATE SOURCE"));
    }

    #[test]
    fn openrouter_sse_rejects_nonterminal_nonempty_or_conflicting_usage_choice() {
        let nonterminal = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"PRIVATE SOURCE\"},\"finish_reason\":null}],",
            "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n"
        );
        let nonempty_after_finish = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"PRIVATE SOURCE\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"MORE PRIVATE SOURCE\"},\"finish_reason\":null}],",
            "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n"
        );
        let conflicting_finish = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"PRIVATE SOURCE\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}],",
            "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n"
        );

        for fixture in [nonterminal, nonempty_after_finish, conflicting_finish] {
            let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(51_200));
            let failure = parser.push_chunk(fixture.as_bytes()).expect_err(
                "usage must not accompany nonterminal, nonempty, or conflicting choices",
            );
            assert_eq!(failure.category, "invalid_usage_chunk");
            let diagnostic = format_openrouter_sse_failure(&failure, &sse_test_audit(), 16_384);
            assert!(!diagnostic.contains("PRIVATE SOURCE"));
            assert!(!diagnostic.contains("MORE PRIVATE SOURCE"));
        }
    }

    #[test]
    fn openrouter_sse_rejects_filtered_tool_and_unknown_finishes() {
        for (finish_reason, expected_category) in [
            ("content_filter", "finish_content_filter"),
            ("tool_calls", "finish_tool_calls"),
            ("unexpected_provider_value", "finish_unknown"),
        ] {
            let event = format!(
                "data: {}\n\n",
                serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {"content": "fn bot_tick_v2() {}"},
                        "finish_reason": finish_reason
                    }]
                })
            );
            let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(51_200));
            let failure = parser
                .push_chunk(event.as_bytes())
                .expect_err("unsupported finish must fail immediately");
            assert_eq!(failure.category, expected_category);
        }

        let mut parser = OpenRouterSseParser::new(OpenRouterSseLimits::production(51_200));
        let failure = parser
            .push_chunk(
                b"data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[]},\"finish_reason\":null}]}\n\n",
            )
            .expect_err("tool-call deltas must fail even before finish");
        assert_eq!(failure.category, "tool_call_delta");
    }

    #[test]
    fn openrouter_non_success_diagnostic_ignores_plain_json_body() {
        let mut audit = sse_test_audit();
        audit.status = reqwest::StatusCode::TOO_MANY_REQUESTS;
        audit.content_type = "application/json".to_owned();
        let private_body = r#"{"error":{"message":"PRIVATE PROVIDER MESSAGE"}}"#;
        let diagnostic = format_openrouter_http_status_error(&audit);

        assert!(diagnostic.contains("status=429"));
        assert!(diagnostic.contains("generation_id=gen-safe-123"));
        assert!(diagnostic.contains("declared_length=12345"));
        assert!(!diagnostic.contains(private_body));
        assert!(!diagnostic.contains("PRIVATE PROVIDER MESSAGE"));
    }

    #[test]
    fn openrouter_sse_rejects_incomplete_or_conflicting_terminal_protocol() {
        let limits = OpenRouterSseLimits::production(51_200);
        let incomplete_event = b"data: {\"choices\":[]}";
        let failure = expect_sse_failure(
            parse_sse_fixture(incomplete_event, 2, limits),
            "pending event at EOF must fail",
        );
        assert_eq!(failure.category, "incomplete_event");

        let missing_done = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fn bot_tick_v2() {}\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n"
        );
        let failure = expect_sse_failure(
            parse_sse_fixture(missing_done.as_bytes(), 3, limits),
            "clean EOF without DONE must fail",
        );
        assert_eq!(failure.category, "missing_done");

        let missing_finish = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fn bot_tick_v2() {}\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n"
        );
        let failure = expect_sse_failure(
            parse_sse_fixture(missing_finish.as_bytes(), 4, limits),
            "DONE without finish must fail",
        );
        assert_eq!(failure.category, "done_without_stop");

        let missing_usage = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fn bot_tick_v2() {}\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let failure = expect_sse_failure(
            parse_sse_fixture(missing_usage.as_bytes(), 5, limits),
            "DONE without usage must fail",
        );
        assert_eq!(failure.category, "done_without_usage");

        let duplicate_finish = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"
        );
        let failure = expect_sse_failure(
            parse_sse_fixture(duplicate_finish.as_bytes(), 6, limits),
            "duplicate finish must fail",
        );
        assert_eq!(failure.category, "duplicate_finish");

        let mut duplicate_done = valid_sse_fixture();
        duplicate_done.extend_from_slice(b"data: [DONE]\n\n");
        let failure = expect_sse_failure(
            parse_sse_fixture(&duplicate_done, 7, limits),
            "duplicate DONE must fail",
        );
        assert_eq!(failure.category, "duplicate_done");
    }

    #[test]
    fn openrouter_sse_bounds_line_event_source_and_total_stream() {
        let mut line_parser = OpenRouterSseParser::new(OpenRouterSseLimits {
            source_bytes: 64,
            stream_bytes: 256,
            line_bytes: 8,
            event_bytes: 128,
        });
        assert_eq!(
            line_parser
                .push_chunk(b"data: 1234")
                .expect_err("line limit")
                .category,
            "line_too_large"
        );

        let mut event_parser = OpenRouterSseParser::new(OpenRouterSseLimits {
            source_bytes: 64,
            stream_bytes: 256,
            line_bytes: 128,
            event_bytes: 8,
        });
        assert_eq!(
            event_parser
                .push_chunk(b"data: 123456789\n")
                .expect_err("event limit")
                .category,
            "event_too_large"
        );

        let mut source_parser = OpenRouterSseParser::new(OpenRouterSseLimits {
            source_bytes: 4,
            stream_bytes: 512,
            line_bytes: 256,
            event_bytes: 256,
        });
        assert_eq!(
            source_parser
                .push_chunk(
                    b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"12345\"},\"finish_reason\":null}]}\n\n",
                )
                .expect_err("source limit")
                .category,
            "source_too_large"
        );

        let mut stream_parser = OpenRouterSseParser::new(OpenRouterSseLimits {
            source_bytes: 64,
            stream_bytes: 4,
            line_bytes: 4,
            event_bytes: 4,
        });
        assert_eq!(
            stream_parser
                .push_chunk(b"12345")
                .expect_err("stream limit")
                .category,
            "stream_too_large"
        );
    }

    #[test]
    fn openrouter_response_body_id_precedes_header_fallback() {
        assert_eq!(
            select_openrouter_response_id(
                Some("body-generation-id".to_owned()),
                Some("header-generation-id".to_owned()),
            )
            .as_deref(),
            Some("body-generation-id")
        );
        assert_eq!(
            select_openrouter_response_id(None, Some("header-generation-id".to_owned())).as_deref(),
            Some("header-generation-id")
        );
    }

    #[test]
    fn openrouter_timeout_ceiling_allows_slow_reasoning_models() {
        temp_env::with_var("MGS_OPENROUTER_TIMEOUT_SECS", Some("1200"), || {
            let service = CodeGenerationService::new_from_env();
            assert_eq!(service.inner.openrouter_timeout_secs, 900);
        });
    }

    #[test]
    fn compiler_refuses_oversized_source_before_writing_it() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mgs_codegen_limit_{stamp}"));
        let source_dir = root.join("source");
        let wasm_dir = root.join("wasm");
        let response = compile_generated_code_impl(
            "oversized".to_owned(),
            "x".repeat(DEFAULT_MAX_SOURCE_BYTES + 1),
            true,
            DEFAULT_MAX_SOURCE_BYTES,
            source_dir,
            wasm_dir,
        );
        assert!(!response.compiled);
        assert!(response
            .compiler_stderr
            .contains("source exceeds configured max"));
        assert!(!Path::new(&response.source_path).exists());
    }

    #[test]
    fn sanitize_model_id_rejects_path_traversal() {
        assert!(sanitize_model_id("../bot").is_none());
        assert!(sanitize_model_id("..").is_none());
        assert!(sanitize_model_id(".hidden").is_none());
        assert!(sanitize_model_id("bot..v2").is_none());
        assert!(sanitize_model_id("bot_alpha-1").is_some());
        assert!(sanitize_model_id(
            &"a".repeat(crate::operational::validation::MAX_MODEL_ID_LEN + 1)
        )
        .is_none());
    }

    #[test]
    fn rust_crate_name_normalizes_filename_only_model_id_characters() {
        assert_eq!(
            rust_crate_name("orw-20260724-xiaomi-mimo-v2.5"),
            "arena_orw_20260724_xiaomi_mimo_v2_5"
        );
        assert_eq!(rust_crate_name("9-model"), "arena_9_model");
        assert!(rust_crate_name("bot-alpha_1.2")
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'));
    }

    #[test]
    fn compiler_accepts_model_id_with_dots_and_hyphens() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mgs_codegen_crate_name_{stamp}"));
        let source_dir = root.join("source");
        let wasm_dir = root.join("wasm");
        let model_id = "orw-20260724-xiaomi-mimo-v2.5";
        let source = r#"
#[no_mangle]
pub extern "C" fn bot_tick_v2(
    _: i32, _: i32, _: i32, _: i32, _: i32, _: i32,
    _: i32, _: i32, _: i32, _: i32, _: i32,
) -> i32 { 1 }
"#;

        let response = compile_generated_code_impl(
            model_id.to_owned(),
            source.to_owned(),
            true,
            DEFAULT_MAX_SOURCE_BYTES,
            source_dir,
            wasm_dir,
        );
        let source_file_name = Path::new(&response.source_path)
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_owned);
        let wasm_file_name = response
            .wasm_path
            .as_deref()
            .and_then(|value| Path::new(value).file_name())
            .and_then(|value| value.to_str())
            .map(str::to_owned);
        let stderr = response.compiler_stderr.clone();
        let compiled = response.compiled;
        let bytes_written = response.bytes_written;
        let wasm_sha256 = response.wasm_sha256.clone();
        let published_wasm = response
            .wasm_path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok());
        let repeated = compile_generated_code_impl(
            model_id.to_owned(),
            source.to_owned(),
            true,
            DEFAULT_MAX_SOURCE_BYTES,
            root.join("source"),
            root.join("wasm"),
        );
        let published_file_count = std::fs::read_dir(root.join("wasm"))
            .expect("wasm output directory")
            .filter_map(Result::ok)
            .count();
        let _ = std::fs::remove_dir_all(root);

        assert!(
            compiled,
            "rustc failed for dotted/hyphenated model ID: {stderr}"
        );
        assert!(bytes_written > 0);
        assert_eq!(published_wasm.as_deref().map(sha256_hex), wasm_sha256);
        assert!(repeated.compiled, "repeat compilation should succeed");
        assert_eq!(repeated.wasm_sha256, wasm_sha256);
        assert_eq!(published_file_count, 1, "no staged artifact should remain");
        assert_eq!(
            source_file_name.as_deref(),
            Some("orw-20260724-xiaomi-mimo-v2.5.rs")
        );
        assert_eq!(
            wasm_file_name.as_deref(),
            Some("orw-20260724-xiaomi-mimo-v2.5.wasm")
        );
    }

    fn compile_direct_final_basename_fixture(
        root: &Path,
        model_id: &str,
        source: &str,
    ) -> (PathBuf, PathBuf) {
        let source_dir = root.join("source");
        let wasm_dir = root.join("wasm");
        std::fs::create_dir_all(&source_dir).expect("fixture source directory");
        std::fs::create_dir_all(&wasm_dir).expect("fixture wasm directory");
        let source_path = source_dir.join(format!("{model_id}.rs"));
        let wasm_path = wasm_dir.join(format!("{model_id}.wasm"));
        std::fs::write(&source_path, source).expect("fixture source");

        let (output, timed_out) =
            run_rustc_with_limits(&source_path, &wasm_path, &rust_crate_name(model_id))
                .expect("fixture rustc execution");
        assert!(!timed_out, "fixture rustc timed out");
        assert!(
            output.status.success(),
            "fixture rustc failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        validate_compiled_wasm_export(&wasm_path).expect("fixture wasm export");
        (source_path, wasm_path)
    }

    #[test]
    fn compile_verification_accepts_identical_wasm_without_mutating_live_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "mgs-codegen-verify-match-{}",
            Uuid::new_v4().simple()
        ));
        let source_dir = root.join("source");
        let wasm_dir = root.join("wasm");
        let model_id = "verification-match";
        let source = r#"
#[no_mangle]
pub extern "C" fn bot_tick_v2(
    _: i32, _: i32, _: i32, _: i32, _: i32, _: i32,
    _: i32, _: i32, _: i32, _: i32, _: i32,
        ) -> i32 { 7 }
"#;

        let (live_source_path, live_wasm_path) =
            compile_direct_final_basename_fixture(&root, model_id, source);
        let source_before = std::fs::read(&live_source_path).expect("published source");
        let wasm_before = std::fs::read(&live_wasm_path).expect("published wasm");
        let final_basename = format!("{model_id}.wasm");
        assert!(
            wasm_before
                .windows(final_basename.len())
                .any(|window| window == final_basename.as_bytes()),
            "fixture must embed the direct final output basename"
        );

        let verified = verify_existing_compiled_code_impl(
            model_id.to_owned(),
            source.to_owned(),
            DEFAULT_MAX_SOURCE_BYTES,
            source_dir,
            wasm_dir,
        );
        let source_after = std::fs::read(&live_source_path).expect("source after verification");
        let wasm_after = std::fs::read(&live_wasm_path).expect("wasm after verification");
        let _ = std::fs::remove_dir_all(root);

        assert!(
            verified.compiled,
            "verification failed: {}",
            verified.compiler_stderr
        );
        assert_eq!(verified.bytes_written, wasm_before.len());
        assert_eq!(
            verified.wasm_sha256.as_deref(),
            Some(sha256_hex(&wasm_before).as_str())
        );
        assert_eq!(
            verified.warnings.last().map(String::as_str),
            Some(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC)
        );
        assert_eq!(source_after, source_before, "live source must not change");
        assert_eq!(wasm_after, wasm_before, "live wasm must not change");
    }

    #[test]
    fn compile_verification_rejects_different_wasm_without_mutating_live_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "mgs-codegen-verify-mismatch-{}",
            Uuid::new_v4().simple()
        ));
        let source_dir = root.join("source");
        let wasm_dir = root.join("wasm");
        let model_id = "verification-mismatch";
        let published_source = r#"
#[no_mangle]
pub extern "C" fn bot_tick_v2(
    _: i32, _: i32, _: i32, _: i32, _: i32, _: i32,
    _: i32, _: i32, _: i32, _: i32, _: i32,
) -> i32 { 11 }
"#;
        let different_source = r#"
#[no_mangle]
pub extern "C" fn bot_tick_v2(
    _: i32, _: i32, _: i32, _: i32, _: i32, _: i32,
    _: i32, _: i32, _: i32, _: i32, _: i32,
        ) -> i32 { 29 }
"#;

        let (live_source_path, live_wasm_path) =
            compile_direct_final_basename_fixture(&root, model_id, published_source);
        let source_before = std::fs::read(&live_source_path).expect("published source");
        let wasm_before = std::fs::read(&live_wasm_path).expect("published wasm");

        let verified = verify_existing_compiled_code_impl(
            model_id.to_owned(),
            different_source.to_owned(),
            DEFAULT_MAX_SOURCE_BYTES,
            source_dir,
            wasm_dir,
        );
        let source_after = std::fs::read(&live_source_path).expect("source after verification");
        let wasm_after = std::fs::read(&live_wasm_path).expect("wasm after verification");
        let _ = std::fs::remove_dir_all(root);

        assert!(!verified.compiled);
        assert_eq!(verified.bytes_written, 0);
        assert_eq!(verified.wasm_sha256, None);
        assert!(verified
            .compiler_stderr
            .contains("does not match existing artifact"));
        assert_eq!(
            verified.warnings.last().map(String::as_str),
            Some(VERIFICATION_FINAL_BASENAME_DIAGNOSTIC)
        );
        assert_eq!(source_after, source_before, "live source must not change");
        assert_eq!(wasm_after, wasm_before, "live wasm must not change");
    }

    #[cfg(unix)]
    #[test]
    fn compile_verification_temp_root_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temporary_root = TemporaryCompileRoot::create().expect("verification temp root");
        let mode = std::fs::metadata(&temporary_root.path)
            .expect("verification temp root metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn atomic_replace_never_leaves_a_partial_or_temporary_artifact() {
        let root =
            std::env::temp_dir().join(format!("mgs-codegen-atomic-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).expect("temporary artifact directory");
        let path = root.join("fighter.wasm");
        std::fs::write(&path, b"previous").expect("write prior artifact");

        atomic_replace_bytes(&path, b"complete replacement").expect("atomic replacement");

        assert_eq!(
            std::fs::read(&path).expect("published artifact"),
            b"complete replacement"
        );
        assert_eq!(
            std::fs::read_dir(&root)
                .expect("artifact directory")
                .filter_map(Result::ok)
                .count(),
            1,
            "temporary sibling must be removed after publication"
        );
        let _ = std::fs::remove_dir_all(root);
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
    fn validator_rejects_compile_time_environment_and_file_access() {
        for forbidden in [
            r#"const _: &str = env /* hidden */ ! ("ARENA_SECRET");"#,
            r#"const _: Option<&str> = option_env!("ARENA_SECRET");"#,
            r#"const _: &str = include_str /* hidden */ ! ("/etc/passwd");"#,
            r#"const _: &[u8] = include_bytes!("/etc/passwd");"#,
            r#"mod stolen_strategy;"#,
        ] {
            let source = format!(
                r#"
#[no_mangle]
pub extern "C" fn bot_tick_v2(
    _: i32, _: i32, _: i32, _: i32, _: i32, _: i32,
    _: i32, _: i32, _: i32, _: i32, _: i32,
) -> i32 {{ match 0 {{ _ => 0 }} }}
{forbidden}
"#
            );
            let validation = validate_source_impl(DEFAULT_MAX_SOURCE_BYTES, &source, Some("rust"));
            assert!(
                !validation.valid,
                "compile-time access unexpectedly passed validation: {forbidden}"
            );
            assert!(
                validation
                    .errors
                    .iter()
                    .any(|error| error.contains("forbidden compile-time identifier")),
                "unexpected errors for {forbidden}: {:?}",
                validation.errors
            );
        }
    }

    #[test]
    fn compile_only_path_cannot_bypass_source_validation() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mgs_codegen_validation_{stamp}"));
        let source_dir = root.join("source");
        let wasm_dir = root.join("wasm");
        let source = r#"
#[no_mangle]
pub extern "C" fn bot_tick_v2(
    _: i32, _: i32, _: i32, _: i32, _: i32, _: i32,
    _: i32, _: i32, _: i32, _: i32, _: i32,
) -> i32 { match 0 { _ => 0 } }
const _: Option<&str> = option_env!("ARENA_SECRET");
"#;
        let response = compile_generated_code_impl(
            "validation-bypass".to_owned(),
            source.to_owned(),
            true,
            DEFAULT_MAX_SOURCE_BYTES,
            source_dir,
            wasm_dir,
        );
        assert!(!response.compiled);
        assert!(response
            .compiler_stderr
            .contains("source failed validation"));
        assert!(!Path::new(&response.source_path).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rustc_child_environment_does_not_inherit_server_secrets() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mgs_codegen_env_{stamp}"));
        std::fs::create_dir_all(&root).expect("temporary compiler directory");
        let source_path = root.join("leak.rs");
        let wasm_path = root.join("leak.wasm");
        std::fs::write(
            &source_path,
            r#"compile_error!(env!("MGS_CODEGEN_ENV_LEAK_TEST"));"#,
        )
        .expect("write compiler probe");

        let secret = "compiler-boundary-secret-must-not-appear";
        let (output, timed_out) =
            temp_env::with_var("MGS_CODEGEN_ENV_LEAK_TEST", Some(secret), || {
                run_rustc_with_limits(&source_path, &wasm_path, "arena_env_probe")
                    .expect("rustc should start")
            });
        assert!(!timed_out);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("MGS_CODEGEN_ENV_LEAK_TEST"),
            "unexpected compiler stderr: {stderr}"
        );
        assert!(
            !stderr.contains(secret),
            "rustc inherited and disclosed the server environment"
        );
        let _ = std::fs::remove_dir_all(root);
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
        let key = "MGS_TEST_OPENROUTER_API_KEY";
        let file_key = "MGS_TEST_OPENROUTER_API_KEY_FILE";
        temp_env::with_vars(
            [(key, Some("inline-secret")), (file_key, None::<&str>)],
            || {
                let value = read_env_secret(key);
                assert_eq!(value.as_deref(), Some("inline-secret"));
            },
        );
    }

    #[test]
    fn read_env_secret_uses_file_fallback() {
        let key = "MGS_TEST_OPENROUTER_FILE_ONLY";
        let file_key = "MGS_TEST_OPENROUTER_FILE_ONLY_FILE";

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mgs_openrouter_secret_{stamp}.txt"));
        std::fs::write(&path, "file-secret\n").expect("secret file should be written");
        let path_raw = path.to_string_lossy().into_owned();

        temp_env::with_vars(
            [(key, None::<&str>), (file_key, Some(path_raw.as_str()))],
            || {
                let value = read_env_secret(key);
                assert_eq!(value.as_deref(), Some("file-secret"));
            },
        );

        let _ = std::fs::remove_file(path);
    }
}
