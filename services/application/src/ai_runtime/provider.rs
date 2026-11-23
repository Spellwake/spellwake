//! Pluggable AI decision providers for production OpenAI Responses API, local
//! Codex CLI dev verification, and deterministic tests.
use super::quota::AiTier;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Reasoning effort forwarded to the LLM provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    #[default]
    Medium,
    High,
    XHigh,
    Max,
    Ultra,
}

impl ReasoningEffort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" | "extra_high" | "extra-high" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }
}

/// Model and effort selection for one AI tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiTierModelConfig {
    pub model: String,
    pub effort: ReasoningEffort,
    pub timeout: Duration,
}

/// Paired model configuration for Normal AI and Advanced AI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiTierCatalog {
    pub normal: AiTierModelConfig,
    pub advanced: AiTierModelConfig,
}

impl Default for AiTierCatalog {
    fn default() -> Self {
        Self {
            normal: AiTierModelConfig {
                model: "gpt-6.1-terra".to_owned(),
                effort: ReasoningEffort::Medium,
                timeout: Duration::from_secs(45),
            },
            advanced: AiTierModelConfig {
                model: "gpt-6.2-astra".to_owned(),
                effort: ReasoningEffort::XHigh,
                timeout: Duration::from_secs(60),
            },
        }
    }
}

impl AiTierCatalog {
    /// Load tier model/effort overrides from environment variables, falling back to
    /// the verified two-tier defaults (`gpt-6.1-terra` medium / `gpt-6.2-astra` xhigh).
    pub fn from_env() -> Self {
        let mut catalog = Self::default();
        if let Ok(model) = std::env::var("SPELLWAKE_AI_NORMAL_MODEL")
            && !model.trim().is_empty()
        {
            catalog.normal.model = model.trim().to_owned();
        }
        if let Ok(effort) = std::env::var("SPELLWAKE_AI_NORMAL_EFFORT")
            && let Some(parsed) = ReasoningEffort::parse(&effort)
        {
            catalog.normal.effort = parsed;
        }
        if let Ok(model) = std::env::var("SPELLWAKE_AI_ADVANCED_MODEL")
            && !model.trim().is_empty()
        {
            catalog.advanced.model = model.trim().to_owned();
        }
        if let Ok(effort) = std::env::var("SPELLWAKE_AI_ADVANCED_EFFORT")
            && let Some(parsed) = ReasoningEffort::parse(&effort)
        {
            catalog.advanced.effort = parsed;
        }
        catalog
    }

    /// Select the model configuration for a seat's AI tier.
    pub fn for_tier(&self, tier: AiTier) -> &AiTierModelConfig {
        match tier {
            AiTier::Normal => &self.normal,
            AiTier::Advanced => &self.advanced,
        }
    }
}

/// Token and cache telemetry reported by a single AI decision call.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AiUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

/// Input to one AI model decision call.
#[derive(Debug, Clone)]
pub struct AiModelRequest {
    pub tier: AiTier,
    pub game_id: Uuid,
    pub seat_id: Uuid,
    pub prompt: String,
    pub schema: Value,
    pub cache_key: String,
}

/// Output from one AI model decision call.
#[derive(Debug, Clone)]
pub struct AiModelResponse {
    pub raw_json: String,
    pub model: String,
    pub effort: ReasoningEffort,
    pub usage: AiUsage,
    pub latency_ms: u64,
}

/// Safe provider error (never carries raw API keys or credentials).
#[derive(Debug)]
pub enum AiProviderError {
    Configuration(&'static str),
    Timeout,
    RetryableHttp(u16),
    HttpStatus(u16),
    Transport(String),
    MalformedResponse(&'static str),
}

impl AiProviderError {
    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::RetryableHttp(_))
    }
}

impl fmt::Display for AiProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(msg) => write!(f, "AI provider configuration error: {msg}"),
            Self::Timeout => f.write_str("AI provider request timed out"),
            Self::RetryableHttp(code) => write!(f, "AI provider transient HTTP {code}"),
            Self::HttpStatus(code) => write!(f, "AI provider rejected HTTP {code}"),
            Self::Transport(msg) => write!(f, "AI provider transport failed: {msg}"),
            Self::MalformedResponse(msg) => write!(f, "AI provider malformed response: {msg}"),
        }
    }
}

impl Error for AiProviderError {}

/// Pluggable backend for invoking an LLM on a compact game prompt.
pub trait AiDecisionProvider: Send + Sync {
    /// Request one structured JSON decision for a live AI boundary.
    /// # Errors
    /// Returns a safe `AiProviderError` if transport, auth, timeout, or parsing fails.
    fn decide(&self, request: &AiModelRequest) -> Result<AiModelResponse, AiProviderError>;
}

/// Production direct OpenAI Responses API provider (zero CLI overhead, native prompt caching).
pub struct OpenAiResponsesProvider {
    client: reqwest::blocking::Client,
    endpoint: String,
    api_key: String,
    catalog: AiTierCatalog,
}

impl OpenAiResponsesProvider {
    /// Construct a direct OpenAI Responses API provider.
    /// # Errors
    /// Rejects empty API keys, invalid URLs, or HTTP client setup errors.
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        catalog: AiTierCatalog,
    ) -> Result<Self, AiProviderError> {
        let key = api_key.trim().to_owned();
        if key.is_empty() {
            return Err(AiProviderError::Configuration("missing OpenAI API key"));
        }
        let base = base_url
            .unwrap_or_else(|| "https://api.openai.com/v1".to_owned())
            .trim_end_matches('/')
            .to_owned();
        let endpoint = format!("{base}/responses");
        reqwest::Url::parse(&endpoint)
            .map_err(|_| AiProviderError::Configuration("invalid OpenAI base URL"))?;
        let client = reqwest::blocking::Client::builder()
            .user_agent("spellwake-ai-runtime/1")
            .build()
            .map_err(|_| AiProviderError::Configuration("HTTP client init failed"))?;
        Ok(Self {
            client,
            endpoint,
            api_key: key,
            catalog,
        })
    }
}

impl AiDecisionProvider for OpenAiResponsesProvider {
    fn decide(&self, request: &AiModelRequest) -> Result<AiModelResponse, AiProviderError> {
        let tier_cfg = self.catalog.for_tier(request.tier);
        let started = Instant::now();
        let body = json!({
            "model": tier_cfg.model,
            "reasoning": { "effort": tier_cfg.effort.as_str() },
            "prompt_cache_key": request.cache_key,
            "input": [
                {
                    "role": "user",
                    "content": [{ "type": "input_text", "text": request.prompt }]
                }
            ],
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "spellwake_compact_reply",
                    "strict": true,
                    "schema": request.schema
                }
            }
        });

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .timeout(tier_cfg.timeout)
            .json(&body)
            .send()
            .map_err(|err| {
                if err.is_timeout() {
                    AiProviderError::Timeout
                } else {
                    AiProviderError::Transport("request send failed".to_owned())
                }
            })?;

        let status = response.status().as_u16();
        if status == 408 || status == 429 || status >= 500 {
            return Err(AiProviderError::RetryableHttp(status));
        }
        if !(200..300).contains(&status) {
            return Err(AiProviderError::HttpStatus(status));
        }

        let payload: Value = response
            .json()
            .map_err(|_| AiProviderError::MalformedResponse("response body was not JSON"))?;
        let raw_json = extract_responses_output_text(&payload)
            .ok_or(AiProviderError::MalformedResponse("missing output text"))?;
        let usage = extract_responses_usage(&payload);
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        Ok(AiModelResponse {
            raw_json,
            model: tier_cfg.model.clone(),
            effort: tier_cfg.effort,
            usage,
            latency_ms,
        })
    }
}

fn extract_responses_output_text(payload: &Value) -> Option<String> {
    if let Some(text) = payload.get("output_text").and_then(Value::as_str)
        && !text.trim().is_empty()
    {
        return Some(text.to_owned());
    }
    let output = payload.get("output")?.as_array()?;
    for item in output {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content {
            if let Some(text) = part.get("text").and_then(Value::as_str)
                && !text.trim().is_empty()
            {
                return Some(text.to_owned());
            }
        }
    }
    None
}

fn extract_responses_usage(payload: &Value) -> AiUsage {
    let Some(u) = payload.get("usage").and_then(Value::as_object) else {
        return AiUsage::default();
    };
    let input_tokens = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let cached_input_tokens = u
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
    let reasoning_tokens = u
        .get("output_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    AiUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_tokens,
    }
}

/// Local development adapter invoking a pinned Codex CLI binary with a fixed
/// empty working directory so provider prompt caching hits reliably.
pub struct CodexCliProvider {
    cli_path: PathBuf,
    fixed_cwd: PathBuf,
    scratch_root: PathBuf,
    catalog: AiTierCatalog,
}

impl CodexCliProvider {
    /// Construct a local Codex CLI dev adapter.
    /// # Errors
    /// Fails if the CLI binary does not exist or the fixed empty cwd cannot be prepared.
    pub fn new(cli_path: PathBuf, catalog: AiTierCatalog) -> Result<Self, AiProviderError> {
        if !cli_path.is_file() {
            return Err(AiProviderError::Configuration("Codex CLI binary not found"));
        }
        let fixed_cwd = std::env::temp_dir().join("spellwake-codex-empty-cwd");
        fs::create_dir_all(&fixed_cwd)
            .map_err(|_| AiProviderError::Configuration("failed to create fixed Codex cwd"))?;
        let scratch_root = std::env::temp_dir().join("spellwake-ai-codex-calls");
        fs::create_dir_all(&scratch_root)
            .map_err(|_| AiProviderError::Configuration("failed to create Codex scratch dir"))?;
        Ok(Self {
            cli_path,
            fixed_cwd,
            scratch_root,
            catalog,
        })
    }
}

impl AiDecisionProvider for CodexCliProvider {
    fn decide(&self, request: &AiModelRequest) -> Result<AiModelResponse, AiProviderError> {
        let tier_cfg = self.catalog.for_tier(request.tier);
        let started = Instant::now();
        let call_dir = self.scratch_root.join(Uuid::new_v4().to_string());
        fs::create_dir_all(&call_dir)
            .map_err(|_| AiProviderError::Transport("failed to create call dir".to_owned()))?;
        let schema_path = call_dir.join("schema.json");
        let out_path = call_dir.join("reply.json");
        let cleanup = TempDirGuard(&call_dir);

        let schema_bytes = serde_json::to_vec(&request.schema)
            .map_err(|_| AiProviderError::MalformedResponse("failed to encode schema"))?;
        fs::write(&schema_path, schema_bytes)
            .map_err(|_| AiProviderError::Transport("failed to write schema".to_owned()))?;

        let mut child = Command::new(&self.cli_path)
            .current_dir(&self.fixed_cwd)
            .arg("exec")
            .arg("--skip-git-repo-check")
            .arg("--json")
            .arg("--model")
            .arg(&tier_cfg.model)
            .arg("-c")
            .arg(format!(
                "model_reasoning_effort=\"{}\"",
                tier_cfg.effort.as_str()
            ))
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("-o")
            .arg(&out_path)
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AiProviderError::Transport(format!("codex spawn failed: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin
                .write_all(request.prompt.as_bytes())
                .map_err(|_| AiProviderError::Transport("codex stdin write failed".to_owned()))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|_| AiProviderError::Transport("codex wait failed".to_owned()))?;

        if !output.status.success() {
            let stderr_tail = String::from_utf8_lossy(&output.stderr);
            let snippet: String = stderr_tail
                .chars()
                .rev()
                .take(300)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            return Err(AiProviderError::Transport(format!(
                "codex exited with status {:?}: {}",
                output.status.code(),
                snippet.trim()
            )));
        }

        let raw_json = fs::read_to_string(&out_path)
            .map_err(|_| AiProviderError::MalformedResponse("codex output file missing"))?;
        let usage = parse_codex_jsonl_usage(&output.stdout);
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        drop(cleanup);

        Ok(AiModelResponse {
            raw_json,
            model: tier_cfg.model.clone(),
            effort: tier_cfg.effort,
            usage,
            latency_ms,
        })
    }
}

struct TempDirGuard<'a>(&'a Path);
impl Drop for TempDirGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0);
    }
}

fn parse_codex_jsonl_usage(stdout: &[u8]) -> AiUsage {
    let text = String::from_utf8_lossy(stdout);
    let mut usage = AiUsage::default();
    for line in text.lines() {
        if !line.contains("turn.completed") {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(line)
            && val.get("type").and_then(Value::as_str) == Some("turn.completed")
            && let Some(u) = val.get("usage").and_then(Value::as_object)
        {
            usage.input_tokens = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
            usage.cached_input_tokens = u
                .get("cached_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            usage.output_tokens = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
            usage.reasoning_tokens = u
                .get("reasoning_output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
        }
    }
    usage
}

/// Deterministic scripted provider for offline unit and integration tests.
pub struct StaticScriptedProvider {
    replies: Mutex<std::collections::VecDeque<String>>,
    fallback: String,
}

impl StaticScriptedProvider {
    /// Return each scripted JSON reply in order, falling back to `fallback` once drained.
    pub fn new(replies: Vec<String>, fallback: String) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            fallback,
        }
    }
}

impl AiDecisionProvider for StaticScriptedProvider {
    fn decide(&self, request: &AiModelRequest) -> Result<AiModelResponse, AiProviderError> {
        let reply = self
            .replies
            .lock()
            .ok()
            .and_then(|mut q| q.pop_front())
            .unwrap_or_else(|| self.fallback.clone());
        Ok(AiModelResponse {
            raw_json: reply,
            model: format!("scripted-{}", request.tier.as_str()),
            effort: ReasoningEffort::Low,
            usage: AiUsage::default(),
            latency_ms: 1,
        })
    }
}

/// Construct the configured runtime provider from environment variables.
///
/// Supported `SPELLWAKE_AI_PROVIDER` values:
/// - `"chatgpt"` / `"codex-app-server"` (default): persistent ChatGPT-login
///   `codex app-server --listen stdio://` with per-seat thread prompt caching
/// - `"codex"`: one-shot `codex exec` CLI adapter
/// - `"openai"`: direct OpenAI Responses API using `OPENAI_API_KEY`
///
/// Returns `Ok(None)` when `SPELLWAKE_AI_ENABLED` is explicitly false or when
/// neither `SPELLWAKE_AI_ENABLED` nor `SPELLWAKE_AI_PROVIDER` activates the runtime.
/// # Errors
/// Returns `AiProviderError::Configuration` when enabled with invalid settings.
pub fn provider_from_env() -> Result<Option<std::sync::Arc<dyn AiDecisionProvider>>, AiProviderError>
{
    let enabled_var = std::env::var("SPELLWAKE_AI_ENABLED")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase());
    if matches!(enabled_var.as_deref(), Some("0" | "false" | "no" | "off")) {
        return Ok(None);
    }
    let explicit = std::env::var("SPELLWAKE_AI_PROVIDER").ok();
    let enabled = matches!(enabled_var.as_deref(), Some("1" | "true" | "yes" | "on"));
    if explicit.is_none() && !enabled {
        return Ok(None);
    }
    let catalog = AiTierCatalog::from_env();
    let mode = explicit
        .unwrap_or_else(|| "chatgpt".to_owned())
        .trim()
        .to_ascii_lowercase();
    match mode.as_str() {
        "chatgpt" | "codex-app-server" => {
            let cfg = super::codex_app_server::CodexAppServerConfig::from_env(catalog)?;
            Ok(Some(std::sync::Arc::new(
                super::codex_app_server::CodexAppServerProvider::new(cfg)?,
            )))
        }
        "openai" => {
            let api_key = std::env::var("SPELLWAKE_AI_OPENAI_API_KEY")
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
                .map_err(|_| {
                    AiProviderError::Configuration(
                        "SPELLWAKE_AI_OPENAI_API_KEY or OPENAI_API_KEY required for openai provider",
                    )
                })?;
            let base_url = std::env::var("SPELLWAKE_AI_OPENAI_BASE_URL").ok();
            Ok(Some(std::sync::Arc::new(OpenAiResponsesProvider::new(
                api_key, base_url, catalog,
            )?)))
        }
        "codex" => {
            let cli_path = std::env::var("SPELLWAKE_AI_CODEX_CLI").map_err(|_| {
                AiProviderError::Configuration("SPELLWAKE_AI_CODEX_CLI required for codex provider")
            })?;
            Ok(Some(std::sync::Arc::new(CodexCliProvider::new(
                PathBuf::from(cli_path),
                catalog,
            )?)))
        }
        _ => Err(AiProviderError::Configuration(
            "SPELLWAKE_AI_PROVIDER must be 'chatgpt', 'codex', or 'openai'",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_responses_api_text_and_usage() {
        let payload = json!({
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "{\"action\":{\"kind\":\"pass\"},\"delegate_pass\":true,\"wake\":[],\"max_skips\":4}"
                }]
            }],
            "usage": {
                "input_tokens": 3500,
                "input_tokens_details": { "cached_tokens": 2800 },
                "output_tokens": 42,
                "output_tokens_details": { "reasoning_tokens": 12 }
            }
        });
        assert!(extract_responses_output_text(&payload).is_some());
        let usage = extract_responses_usage(&payload);
        assert_eq!(usage.input_tokens, 3500);
        assert_eq!(usage.cached_input_tokens, 2800);
        assert_eq!(usage.output_tokens, 42);
        assert_eq!(usage.reasoning_tokens, 12);
    }

    #[test]
    fn parses_all_reasoning_efforts_and_verified_tier_defaults() {
        for (raw, expected, canonical) in [
            ("low", ReasoningEffort::Low, "low"),
            ("medium", ReasoningEffort::Medium, "medium"),
            ("high", ReasoningEffort::High, "high"),
            ("xhigh", ReasoningEffort::XHigh, "xhigh"),
            ("extra-high", ReasoningEffort::XHigh, "xhigh"),
            ("extra_high", ReasoningEffort::XHigh, "xhigh"),
            ("max", ReasoningEffort::Max, "max"),
            ("ultra", ReasoningEffort::Ultra, "ultra"),
        ] {
            let parsed = ReasoningEffort::parse(raw).expect("valid effort");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), canonical);
        }
        assert!(ReasoningEffort::parse("unknown").is_none());

        let defaults = AiTierCatalog::default();
        assert_eq!(defaults.normal.model, "gpt-6.1-terra");
        assert_eq!(defaults.normal.effort, ReasoningEffort::Medium);
        assert_eq!(defaults.advanced.model, "gpt-6.2-astra");
        assert_eq!(defaults.advanced.effort, ReasoningEffort::XHigh);
    }
}
