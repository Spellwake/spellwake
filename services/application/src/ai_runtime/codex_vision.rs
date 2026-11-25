//! Dedicated Vision OCR client over Spellwake's ChatGPT-login Codex `app-server`.
//!
//! Operates strictly isolated from the gameplay `CodexAppServerProvider`:
//! - Separate `CODEX_HOME` subdirectory so refreshed `auth.json` never races gameplay workers.
//! - Separate `Mutex<Option<VisionSession>>` so slow OCR calls never stall live AI turns.
//! - Fresh ephemeral `thread/start` per call with dedicated Vision OCR instructions, zero tools,
//!   read-only sandbox, and disabled web/plugin features.
//! - RAII `TempImageGuard` (`0700` directory / `0600` file on Unix) guaranteeing temp images
//!   are unlinked immediately on return or panic.
//! - Dedicated per-account sliding-window rate limiter and max concurrency semaphore.

use super::{codex_app_server::HOSTED_CODEX_EXECUTABLE, provider::AiProviderError};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Maximum decoded image bytes accepted for Vision OCR fallback (512 KiB).
pub const MAX_VISION_IMAGE_BYTES: usize = 524_288;

/// Maximum base64 character length accepted for Vision OCR fallback.
pub const MAX_VISION_IMAGE_BASE64_LEN: usize = 720_000;

/// Startup timeout for initializing the dedicated Vision `codex app-server` worker.
const VISION_STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// Static system prompt installed via `baseInstructions` on every ephemeral Vision OCR thread.
const VISION_OCR_BASE_INSTRUCTIONS: &str = "\
You are a high-precision Magic: The Gathering card OCR and layout extractor for Spellwake Custom Card Studio.\n\
Inspect the attached card image carefully and transcribe its exact printed fields into the requested JSON schema.\n\
Rules:\n\
- Preserve exact card name, mana cost using curly-brace symbols like {2}{U}{R}, {W/U}, {X}, {T}, exact type line using ' — ' (em dash) between types and subtypes, exact rules text with line breaks between abilities, optional flavor text, power/toughness (for creatures/vehicles), loyalty (for planeswalkers), defense (for battles), and artist credit.\n\
- Choose the closest layout from: normal, transform, modal_dfc, adventure, split, planeswalker, saga.\n\
- Choose rarity from: common, uncommon, rare, mythic.\n\
- Provide artCropBox with normalized [0.0, 1.0] coordinates (x, y, width, height) tightly enclosing the illustration frame inside the card image, or null if no artwork box is visible.\n\
- If any text is blurry, cut off, or ambiguous, still give your best transcription and add a short note in warnings.\n\
- Return ONLY JSON matching the output schema.";

/// Normalized art bounding box returned by Vision OCR (`0.0..=1.0` fractions of the source image).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionArtCropBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One transcribed face returned by Vision OCR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionExtractedFace {
    pub name: String,
    pub mana_cost: Option<String>,
    pub type_line: String,
    pub oracle_text: String,
    pub flavor_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub defense: Option<String>,
    pub color_indicator: Option<Vec<String>>,
    pub artist: Option<String>,
}

/// Structured card extraction result returned by `CodexVisionClient::extract_card`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionExtractedCard {
    pub layout: String,
    pub rarity: String,
    pub faces: Vec<VisionExtractedFace>,
    pub art_crop_box: Option<VisionArtCropBox>,
    pub warnings: Vec<String>,
}

/// Safe typed failures for Vision OCR imports.
#[derive(Debug)]
pub enum VisionImportError {
    InvalidImage(&'static str),
    RateLimited,
    Busy,
    Unavailable(&'static str),
    Timeout,
    MalformedOutput(&'static str),
    Transport(String),
}

impl fmt::Display for VisionImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImage(msg) => write!(f, "invalid vision import image: {msg}"),
            Self::RateLimited => f.write_str("vision import rate limit exceeded"),
            Self::Busy => f.write_str("vision import service is busy"),
            Self::Unavailable(msg) => write!(f, "vision import unavailable: {msg}"),
            Self::Timeout => f.write_str("vision import timed out"),
            Self::MalformedOutput(msg) => write!(f, "vision import returned malformed card: {msg}"),
            Self::Transport(msg) => write!(f, "vision import transport error: {msg}"),
        }
    }
}

impl Error for VisionImportError {}

/// Startup configuration for the dedicated Vision `CodexVisionClient`.
#[derive(Clone)]
pub struct CodexVisionConfig {
    pub cli_path: PathBuf,
    pub codex_home: PathBuf,
    pub seed_auth_json: Option<String>,
    pub model: String,
    pub effort: String,
    pub timeout: Duration,
    pub per_account_per_min: u32,
    pub max_concurrent: usize,
}

impl CodexVisionConfig {
    /// Load optional Vision OCR configuration from environment variables.
    ///
    /// Returns `Ok(None)` when no Codex executable is configured or installed, allowing the API
    /// to boot cleanly and return `503 unavailable` only on `/v1/custom-cards/import-vision`.
    ///
    /// # Errors
    /// Fails closed if `SPELLWAKE_VISION_CODEX_CLI` is explicitly set to a missing executable.
    pub fn from_env() -> Result<Option<Self>, AiProviderError> {
        let explicit_vision = std::env::var("SPELLWAKE_VISION_CODEX_CLI")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let explicit_ai = std::env::var("SPELLWAKE_AI_CODEX_CLI")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());

        let cli_path = if let Some(path) = explicit_vision {
            let pb = PathBuf::from(path);
            if !pb.is_file() {
                return Err(AiProviderError::Configuration(
                    "SPELLWAKE_VISION_CODEX_CLI points to a missing binary",
                ));
            }
            pb
        } else if let Some(path) = explicit_ai {
            let pb = PathBuf::from(path);
            if !pb.is_file() {
                return Ok(None);
            }
            pb
        } else if Path::new(HOSTED_CODEX_EXECUTABLE).is_file() {
            PathBuf::from(HOSTED_CODEX_EXECUTABLE)
        } else {
            return Ok(None);
        };

        let base_home = std::env::var("SPELLWAKE_VISION_CODEX_HOME")
            .ok()
            .or_else(|| std::env::var("SPELLWAKE_AI_CODEX_HOME").ok())
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("spellwake-codex-home"));
        // Dedicated child directory isolates auth.json refreshes from gameplay workers.
        let codex_home = base_home.join("vision");

        let seed_auth_json = std::env::var("SPELLWAKE_VISION_CHATGPT_AUTH_JSON")
            .ok()
            .or_else(|| std::env::var("SPELLWAKE_AI_CHATGPT_AUTH_JSON").ok())
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());

        let model = std::env::var("SPELLWAKE_VISION_MODEL")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "gpt-5.4".to_owned());

        let effort = std::env::var("SPELLWAKE_VISION_EFFORT")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| matches!(v.as_str(), "none" | "minimal" | "low" | "medium" | "high"))
            .unwrap_or_else(|| "low".to_owned());

        let timeout_secs = std::env::var("SPELLWAKE_VISION_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(45)
            .clamp(10, 90);

        let per_account_per_min = std::env::var("SPELLWAKE_VISION_RATE_LIMIT_PER_MIN")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(6)
            .clamp(1, 60);

        let max_concurrent = std::env::var("SPELLWAKE_VISION_MAX_CONCURRENT")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(2)
            .clamp(1, 4);

        Ok(Some(Self {
            cli_path,
            codex_home,
            seed_auth_json,
            model,
            effort,
            timeout: Duration::from_secs(timeout_secs),
            per_account_per_min,
            max_concurrent,
        }))
    }
}

/// Dedicated rate-limited, concurrency-capped Vision OCR client over `codex app-server`.
pub struct CodexVisionClient {
    cli_path: PathBuf,
    fixed_cwd: PathBuf,
    temp_dir: PathBuf,
    codex_home: PathBuf,
    model: String,
    effort: String,
    timeout: Duration,
    sessions: Vec<Mutex<Option<VisionSession>>>,
    next_slot: Mutex<usize>,
    rate_limit: AccountRateLimiter,
    active_permits: Arc<Mutex<usize>>,
    max_concurrent: usize,
}

impl CodexVisionClient {
    /// Prepare isolated working/temp directories and optional `auth.json` seed, then construct
    /// the Vision OCR client.
    ///
    /// # Errors
    /// Fails closed if directories cannot be created or `seed_auth_json` is malformed.
    pub fn new(config: CodexVisionConfig) -> Result<Self, AiProviderError> {
        if !config.cli_path.is_file() {
            return Err(AiProviderError::Configuration(
                "Vision Codex CLI binary not found",
            ));
        }
        let fixed_cwd = std::env::temp_dir().join("spellwake-codex-vision-cwd");
        let temp_dir = std::env::temp_dir().join("spellwake-codex-vision-images");
        create_private_dir(&fixed_cwd)?;
        create_private_dir(&temp_dir)?;
        prepare_vision_codex_home(&config.codex_home, config.seed_auth_json.as_deref())?;

        let slots = config.max_concurrent.clamp(1, 4);
        let sessions = (0..slots).map(|_| Mutex::new(None)).collect();

        Ok(Self {
            cli_path: config.cli_path,
            fixed_cwd,
            temp_dir,
            codex_home: config.codex_home,
            model: config.model,
            effort: config.effort,
            timeout: config.timeout,
            sessions,
            next_slot: Mutex::new(0),
            rate_limit: AccountRateLimiter::new(config.per_account_per_min),
            active_permits: Arc::new(Mutex::new(0)),
            max_concurrent: slots,
        })
    }

    /// Decode and OCR an uploaded PNG/JPEG/WebP card image for `account_id`.
    ///
    /// # Errors
    /// Rejects malformed/oversized images, rate-limited callers, overloaded concurrency caps,
    /// timeouts, and malformed model outputs.
    pub fn extract_card(
        &self,
        account_id: Uuid,
        mime_type: &str,
        image_base64: &str,
    ) -> Result<VisionExtractedCard, VisionImportError> {
        let (bytes, ext) = decode_vision_image(mime_type, image_base64)?;
        if !self.rate_limit.allow(account_id) {
            return Err(VisionImportError::RateLimited);
        }
        let _permit = ConcurrencyPermit::try_acquire(&self.active_permits, self.max_concurrent)
            .ok_or(VisionImportError::Busy)?;

        let temp_image = TempImageGuard::create(&self.temp_dir, ext, &bytes)?;
        let slot_idx = {
            let mut cursor = self
                .next_slot
                .lock()
                .map_err(|_| VisionImportError::Unavailable("vision slot mutex poisoned"))?;
            let idx = *cursor % self.sessions.len();
            *cursor = cursor.wrapping_add(1);
            idx
        };

        let mut guard = self.sessions[slot_idx]
            .lock()
            .map_err(|_| VisionImportError::Unavailable("vision session mutex poisoned"))?;
        if guard.is_none() {
            *guard = Some(
                VisionSession::spawn(&self.cli_path, &self.fixed_cwd, &self.codex_home).map_err(
                    |err| match err {
                        AiProviderError::Configuration(_) => {
                            VisionImportError::Unavailable("vision session authentication missing")
                        }
                        AiProviderError::Timeout => VisionImportError::Timeout,
                        other => VisionImportError::Transport(other.to_string()),
                    },
                )?,
            );
        }

        let deadline = Instant::now() + self.timeout;
        let run_result = {
            let Some(session) = guard.as_mut() else {
                return Err(VisionImportError::Unavailable("vision session missing"));
            };
            session.run_vision_ocr(
                &self.model,
                &self.effort,
                &self.fixed_cwd,
                temp_image.path(),
                deadline,
            )
        };

        // Temp image drops automatically at end of scope; explicitly drop here before parsing.
        drop(temp_image);

        match run_result {
            Ok(raw_json) => parse_vision_extracted_card(&raw_json),
            Err(err) => {
                *guard = None;
                Err(err)
            }
        }
    }
}

/// Validate and decode a PNG, JPEG, or WebP base64 image payload for Vision OCR.
///
/// # Errors
/// Rejects unsupported MIME types, invalid base64, SVG/HTML, magic-byte mismatches, and > 512 KiB.
pub fn decode_vision_image(
    mime_type: &str,
    image_base64: &str,
) -> Result<(Vec<u8>, &'static str), VisionImportError> {
    let ext = match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => {
            return Err(VisionImportError::InvalidImage(
                "supported formats are image/png, image/jpeg, and image/webp",
            ));
        }
    };
    if image_base64.is_empty() || image_base64.len() > MAX_VISION_IMAGE_BASE64_LEN {
        return Err(VisionImportError::InvalidImage(
            "image exceeds 512 KiB limit",
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image_base64.trim())
        .map_err(|_| VisionImportError::InvalidImage("invalid base64 encoding"))?;
    if bytes.is_empty() || bytes.len() > MAX_VISION_IMAGE_BYTES {
        return Err(VisionImportError::InvalidImage(
            "decoded image exceeds 512 KiB limit",
        ));
    }
    let valid_magic = match mime_type {
        "image/png" => {
            bytes.len() >= 8 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        }
        "image/jpeg" => bytes.len() >= 3 && bytes[0..3] == [0xFF, 0xD8, 0xFF],
        "image/webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    };
    if !valid_magic {
        return Err(VisionImportError::InvalidImage(
            "image header does not match declared MIME type",
        ));
    }
    Ok((bytes, ext))
}

/// RAII temporary image file guard ensuring strict permissions and unconditional cleanup.
pub(crate) struct TempImageGuard {
    path: PathBuf,
}

impl TempImageGuard {
    pub(crate) fn create(dir: &Path, ext: &str, bytes: &[u8]) -> Result<Self, VisionImportError> {
        let filename = format!("vision-{}.{ext}", Uuid::new_v4());
        let path = dir.join(filename);
        write_private_temp_file(&path, bytes)?;
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempImageGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Per-account sliding-window rate limiter (`N` requests per 60 seconds).
pub(crate) struct AccountRateLimiter {
    limit_per_min: usize,
    windows: Mutex<HashMap<Uuid, VecDeque<Instant>>>,
}

impl AccountRateLimiter {
    pub(crate) fn new(limit_per_min: u32) -> Self {
        Self {
            limit_per_min: usize::try_from(limit_per_min).unwrap_or(6).max(1),
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn allow(&self, account_id: Uuid) -> bool {
        let now = Instant::now();
        let cutoff = Duration::from_secs(60);
        let Ok(mut guard) = self.windows.lock() else {
            return false;
        };
        if guard.len() > 4096 {
            guard.retain(|_, q| {
                q.back()
                    .is_some_and(|ts| now.saturating_duration_since(*ts) < cutoff)
            });
        }
        let queue = guard.entry(account_id).or_default();
        while queue
            .front()
            .is_some_and(|ts| now.saturating_duration_since(*ts) >= cutoff)
        {
            queue.pop_front();
        }
        if queue.len() >= self.limit_per_min {
            return false;
        }
        queue.push_back(now);
        true
    }
}

struct ConcurrencyPermit {
    counter: Arc<Mutex<usize>>,
}

impl ConcurrencyPermit {
    fn try_acquire(counter: &Arc<Mutex<usize>>, max: usize) -> Option<Self> {
        let mut guard = counter.lock().ok()?;
        if *guard >= max {
            return None;
        }
        *guard += 1;
        Some(Self {
            counter: Arc::clone(counter),
        })
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.counter.lock() {
            *guard = guard.saturating_sub(1);
        }
    }
}

enum VisionStdoutEvent {
    Frame(Value),
    Eof,
    Error(VisionImportError),
}

struct VisionSession {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: mpsc::Receiver<VisionStdoutEvent>,
    next_id: u64,
}

impl VisionSession {
    fn spawn(
        cli_path: &Path,
        fixed_cwd: &Path,
        codex_home: &Path,
    ) -> Result<Self, AiProviderError> {
        let mut cmd = Command::new(cli_path);
        cmd.current_dir(fixed_cwd)
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .arg("-c")
            .arg("mcp_servers={}")
            .arg("-c")
            .arg("shell_snapshot=false")
            .env("CODEX_HOME", codex_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AiProviderError::Transport(format!("vision codex app-server spawn failed: {e}"))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            AiProviderError::Transport("vision codex app-server stdin missing".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AiProviderError::Transport("vision codex app-server stdout missing".into())
        })?;
        if let Some(mut stderr) = child.stderr.take() {
            let _ = thread::Builder::new()
                .name("spellwake-codex-vision-stderr".into())
                .spawn(move || {
                    let mut buf = [0_u8; 4096];
                    while let Ok(n) = stderr.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                    }
                });
        }

        let (tx, rx) = mpsc::channel();
        let _ = thread::Builder::new()
            .name("spellwake-codex-vision-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            let _ = tx.send(VisionStdoutEvent::Eof);
                            break;
                        }
                        Ok(_) => {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let event = match serde_json::from_str::<Value>(&line) {
                                Ok(frame) => VisionStdoutEvent::Frame(frame),
                                Err(_) => VisionStdoutEvent::Error(
                                    VisionImportError::MalformedOutput("invalid JSON-RPC frame"),
                                ),
                            };
                            if tx.send(event).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(VisionStdoutEvent::Error(
                                VisionImportError::Transport(format!("vision stdout read: {e}")),
                            ));
                            break;
                        }
                    }
                }
            });

        let mut session = Self {
            child,
            stdin,
            stdout_rx: rx,
            next_id: 0,
        };

        let startup_deadline = Instant::now() + VISION_STARTUP_TIMEOUT;
        session
            .request_until(
                "initialize",
                json!({
                    "clientInfo": { "name": "spellwake-vision-ocr", "version": "1.0.0" },
                    "capabilities": { "experimentalApi": true },
                }),
                startup_deadline,
            )
            .map_err(|e| AiProviderError::Transport(e.to_string()))?;
        session
            .notify("initialized", None)
            .map_err(|e| AiProviderError::Transport(e.to_string()))?;

        let account = session
            .request_until(
                "account/read",
                json!({ "refreshToken": false }),
                startup_deadline,
            )
            .map_err(|e| AiProviderError::Transport(e.to_string()))?;
        let account_type = account
            .get("account")
            .and_then(|a| a.get("type"))
            .and_then(Value::as_str);
        if account_type != Some("chatgpt") {
            return Err(AiProviderError::Configuration(
                "Vision Codex app-server is not logged in with a ChatGPT account",
            ));
        }
        Ok(session)
    }

    fn send_json(&mut self, value: &Value) -> Result<(), VisionImportError> {
        let mut line = serde_json::to_string(value)
            .map_err(|_| VisionImportError::MalformedOutput("failed to encode JSON-RPC frame"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| VisionImportError::Transport(format!("vision stdin write: {e}")))
    }

    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), VisionImportError> {
        let frame = match params {
            Some(p) => json!({ "method": method, "params": p }),
            None => json!({ "method": method }),
        };
        self.send_json(&frame)
    }

    fn read_frame_until(&mut self, deadline: Instant) -> Result<Value, VisionImportError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(VisionImportError::Timeout);
        }
        match self.stdout_rx.recv_timeout(remaining) {
            Ok(VisionStdoutEvent::Frame(frame)) => Ok(frame),
            Ok(VisionStdoutEvent::Eof) => Err(VisionImportError::Transport(
                "vision app-server closed stdout".into(),
            )),
            Ok(VisionStdoutEvent::Error(err)) => Err(err),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(VisionImportError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(VisionImportError::Transport(
                "vision stdout reader disconnected".into(),
            )),
        }
    }

    fn request_until(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, VisionImportError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send_json(&json!({ "id": id, "method": method, "params": params }))?;
        loop {
            let frame = self.read_frame_until(deadline)?;
            let is_response = frame.get("id").and_then(Value::as_u64) == Some(id)
                && frame.get("method").is_none();
            if is_response {
                if let Some(err) = frame.get("error") {
                    let msg = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown RPC error");
                    return Err(VisionImportError::Transport(format!(
                        "vision {method} failed: {msg}"
                    )));
                }
                return Ok(frame.get("result").cloned().unwrap_or(Value::Null));
            }
            if let Some(req_id) = frame.get("id")
                && frame.get("method").is_some()
            {
                let _ = self.send_json(&json!({
                    "id": req_id,
                    "error": { "code": -32601, "message": "interactive requests disabled in vision OCR" }
                }));
            }
        }
    }

    fn run_vision_ocr(
        &mut self,
        model: &str,
        effort: &str,
        cwd: &Path,
        image_path: &Path,
        deadline: Instant,
    ) -> Result<String, VisionImportError> {
        let thread_res = self.request_until(
            "thread/start",
            json!({
                "cwd": cwd.to_string_lossy(),
                "model": model,
                "ephemeral": true,
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "baseInstructions": VISION_OCR_BASE_INSTRUCTIONS,
                "config": {
                    "web_search": "disabled",
                    "features": {
                        "apps": false,
                        "plugins": false,
                        "remote_plugin": false,
                        "multi_agent": false,
                        "computer_use": false,
                        "browser_use": false
                    }
                }
            }),
            deadline,
        )?;
        let thread_id = thread_res
            .get("thread")
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(VisionImportError::MalformedOutput(
                "thread/start missing thread.id",
            ))?
            .to_owned();

        let turn_res = self.request_until(
            "turn/start",
            json!({
                "threadId": thread_id,
                "model": model,
                "effort": effort,
                "approvalPolicy": "never",
                "outputSchema": vision_output_schema(),
                "input": [
                    {
                        "type": "text",
                        "text": "Extract this Magic card image into the exact Spellwake custom card JSON schema."
                    },
                    {
                        "type": "localImage",
                        "path": image_path.to_string_lossy()
                    }
                ],
            }),
            deadline,
        )?;
        let turn_id = turn_res
            .get("turn")
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(VisionImportError::MalformedOutput(
                "turn/start missing turn.id",
            ))?
            .to_owned();

        let mut reply: Option<String> = None;
        loop {
            let frame = self.read_frame_until(deadline)?;
            if let Some(req_id) = frame.get("id")
                && frame.get("method").is_some()
            {
                let _ = self.send_json(&json!({
                    "id": req_id,
                    "error": { "code": -32601, "message": "tools disabled in vision OCR" }
                }));
                continue;
            }
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                continue;
            };
            let params = frame.get("params").unwrap_or(&Value::Null);
            if params.get("threadId").and_then(Value::as_str) != Some(thread_id.as_str()) {
                continue;
            }
            match method {
                "item/completed" => {
                    if params.get("turnId").and_then(Value::as_str) == Some(turn_id.as_str())
                        && let Some(item) = params.get("item")
                        && item.get("type").and_then(Value::as_str) == Some("agentMessage")
                        && let Some(text) = item.get("text").and_then(Value::as_str)
                        && !text.trim().is_empty()
                    {
                        reply = Some(text.to_owned());
                    }
                }
                "turn/completed" => {
                    let completed_id = params
                        .get("turn")
                        .and_then(|t| t.get("id"))
                        .and_then(Value::as_str);
                    if completed_id != Some(turn_id.as_str()) {
                        continue;
                    }
                    let status = params
                        .get("turn")
                        .and_then(|t| t.get("status"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    if status != "completed" {
                        return Err(VisionImportError::Transport(format!(
                            "vision turn ended with status {status}"
                        )));
                    }
                    break;
                }
                _ => {}
            }
        }

        reply.ok_or(VisionImportError::MalformedOutput(
            "vision turn completed without agentMessage",
        ))
    }
}

impl Drop for VisionSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Strict JSON Schema supplied to Codex `turn/start` `outputSchema`.
pub(crate) fn vision_output_schema() -> Value {
    let nullable_string = json!({ "type": ["string", "null"] });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["layout", "rarity", "faces", "artCropBox", "warnings"],
        "properties": {
            "layout": {
                "type": "string",
                "enum": ["normal", "transform", "modal_dfc", "adventure", "split", "planeswalker", "saga"]
            },
            "rarity": {
                "type": "string",
                "enum": ["common", "uncommon", "rare", "mythic"]
            },
            "faces": {
                "type": "array",
                "minItems": 1,
                "maxItems": 2,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "name", "manaCost", "typeLine", "oracleText", "flavorText",
                        "power", "toughness", "loyalty", "defense", "colorIndicator", "artist"
                    ],
                    "properties": {
                        "name": { "type": "string" },
                        "manaCost": nullable_string,
                        "typeLine": { "type": "string" },
                        "oracleText": { "type": "string" },
                        "flavorText": nullable_string,
                        "power": nullable_string,
                        "toughness": nullable_string,
                        "loyalty": nullable_string,
                        "defense": nullable_string,
                        "colorIndicator": {
                            "type": ["array", "null"],
                            "items": { "type": "string", "enum": ["W", "U", "B", "R", "G"] }
                        },
                        "artist": nullable_string
                    }
                }
            },
            "artCropBox": {
                "type": ["object", "null"],
                "additionalProperties": false,
                "required": ["x", "y", "width", "height"],
                "properties": {
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "width": { "type": "number" },
                    "height": { "type": "number" }
                }
            },
            "warnings": {
                "type": "array",
                "items": { "type": "string" }
            }
        }
    })
}

/// Parse and sanitize structured JSON output from the Vision OCR turn.
///
/// # Errors
/// Rejects invalid JSON, empty face lists, or missing card names.
pub fn parse_vision_extracted_card(
    raw_json: &str,
) -> Result<VisionExtractedCard, VisionImportError> {
    let mut card: VisionExtractedCard = serde_json::from_str(raw_json.trim())
        .map_err(|_| VisionImportError::MalformedOutput("invalid vision OCR JSON payload"))?;

    if !matches!(
        card.layout.as_str(),
        "normal" | "transform" | "modal_dfc" | "adventure" | "split" | "planeswalker" | "saga"
    ) {
        card.layout = "normal".to_owned();
    }
    if !matches!(
        card.rarity.as_str(),
        "common" | "uncommon" | "rare" | "mythic"
    ) {
        card.rarity = "rare".to_owned();
    }
    if card.faces.is_empty() || card.faces.len() > 2 {
        return Err(VisionImportError::MalformedOutput(
            "vision OCR returned invalid face count",
        ));
    }

    // Align layout to actual extracted face count so compilation never fails on a layout mismatch.
    if card.faces.len() == 1
        && matches!(
            card.layout.as_str(),
            "transform" | "modal_dfc" | "adventure" | "split"
        )
    {
        card.layout = "normal".to_owned();
    } else if card.faces.len() == 2
        && matches!(card.layout.as_str(), "normal" | "planeswalker" | "saga")
    {
        card.layout = "transform".to_owned();
    }

    for face in &mut card.faces {
        face.name = face.name.trim().chars().take(120).collect();
        if face.name.is_empty() {
            face.name = "Untitled Card".to_owned();
        }
        face.type_line = face.type_line.trim().chars().take(200).collect();
        face.type_line = {
            let cleaned: String = face.type_line.trim().chars().take(160).collect();
            if cleaned.is_empty() {
                "Creature".to_owned()
            } else {
                cleaned
            }
        };
        face.oracle_text = face.oracle_text.trim().chars().take(2000).collect();
        face.mana_cost = face
            .mana_cost
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(64).collect());
        for stat in [
            &mut face.power,
            &mut face.toughness,
            &mut face.loyalty,
            &mut face.defense,
        ] {
            *stat = stat
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(16).collect());
        }
        face.flavor_text = face
            .flavor_text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(512).collect());
        face.artist = face
            .artist
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(80).collect());
        if let Some(ind) = face.color_indicator.as_mut() {
            let mut seen = std::collections::BTreeSet::new();
            ind.retain(|c| {
                matches!(c.as_str(), "W" | "U" | "B" | "R" | "G") && seen.insert(c.clone())
            });
            if ind.is_empty() {
                face.color_indicator = None;
            }
        }
    }

    if let Some(crop) = card.art_crop_box.as_mut() {
        let valid_box = crop.x.is_finite()
            && crop.y.is_finite()
            && crop.width.is_finite()
            && crop.height.is_finite()
            && (0.0..=0.99).contains(&crop.x)
            && (0.0..=0.99).contains(&crop.y)
            && crop.width >= 0.01
            && crop.height >= 0.01
            && crop.x + crop.width <= 1.05
            && crop.y + crop.height <= 1.05;
        if valid_box {
            crop.width = crop.width.clamp(0.01, (1.0 - crop.x).max(0.01));
            crop.height = crop.height.clamp(0.01, (1.0 - crop.y).max(0.01));
        } else {
            card.art_crop_box = None;
        }
    }
    card.warnings.truncate(8);
    for w in &mut card.warnings {
        *w = w.trim().chars().take(512).collect();
    }
    card.warnings.retain(|w| !w.is_empty());

    Ok(card)
}

fn create_private_dir(path: &Path) -> Result<(), AiProviderError> {
    fs::create_dir_all(path)
        .map_err(|_| AiProviderError::Configuration("failed to create vision private directory"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn write_private_temp_file(path: &Path, bytes: &[u8]) -> Result<(), VisionImportError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| {
                VisionImportError::Transport(format!("failed to create temp image: {e}"))
            })?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| VisionImportError::Transport(format!("failed to write temp image: {e}")))
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
            .map_err(|e| VisionImportError::Transport(format!("failed to write temp image: {e}")))
    }
}

fn prepare_vision_codex_home(
    codex_home: &Path,
    seed_auth_json: Option<&str>,
) -> Result<(), AiProviderError> {
    create_private_dir(codex_home)?;
    let Some(raw_seed) = seed_auth_json else {
        return Ok(());
    };
    let trimmed = raw_seed.trim();
    let bytes = if trimmed.starts_with('{') {
        trimmed.as_bytes().to_vec()
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(trimmed))
            .map_err(|_| {
                AiProviderError::Configuration(
                    "SPELLWAKE_AI_CHATGPT_AUTH_JSON must be valid JSON or base64",
                )
            })?
    };
    if bytes.len() > 65_536 {
        return Err(AiProviderError::Configuration(
            "ChatGPT auth JSON exceeds 64 KiB limit",
        ));
    }
    let parsed: Value = serde_json::from_slice(&bytes)
        .map_err(|_| AiProviderError::Configuration("ChatGPT auth JSON is not valid JSON"))?;
    let tokens =
        parsed
            .get("tokens")
            .and_then(Value::as_object)
            .ok_or(AiProviderError::Configuration(
                "ChatGPT auth JSON missing tokens object",
            ))?;
    let has_required = ["access_token", "refresh_token", "account_id"]
        .iter()
        .all(|k| {
            tokens
                .get(*k)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
        });
    if !has_required {
        return Err(AiProviderError::Configuration(
            "ChatGPT auth JSON missing required token fields",
        ));
    }
    let auth_path = codex_home.join("auth.json");
    if !auth_path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&auth_path)
                .map_err(|_| AiProviderError::Configuration("failed to create vision auth.json"))?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| AiProviderError::Configuration("failed to write vision auth.json"))?;
        }
        #[cfg(not(unix))]
        {
            fs::write(&auth_path, &bytes)
                .map_err(|_| AiProviderError::Configuration("failed to write vision auth.json"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_jpeg_webp_and_rejects_svg_or_mismatched_headers() {
        let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[1, 2, 3, 4]);
        let b64_png = base64::engine::general_purpose::STANDARD.encode(&png);
        let (decoded, ext) = decode_vision_image("image/png", &b64_png).expect("valid png");
        assert_eq!(ext, "png");
        assert_eq!(decoded, png);

        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let b64_jpg = base64::engine::general_purpose::STANDARD.encode(&jpeg);
        assert_eq!(
            decode_vision_image("image/jpeg", &b64_jpg).unwrap().1,
            "jpg"
        );

        let svg = base64::engine::general_purpose::STANDARD.encode(b"<svg/>");
        assert!(decode_vision_image("image/svg+xml", &svg).is_err());
        assert!(decode_vision_image("image/png", &svg).is_err());
    }

    #[test]
    fn temp_image_guard_unlinks_file_on_drop() {
        let dir = std::env::temp_dir().join(format!("spellwake-vision-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let created_path = {
            let guard = TempImageGuard::create(&dir, "png", b"test-bytes").unwrap();
            let p = guard.path().to_path_buf();
            assert!(p.exists());
            p
        };
        assert!(!created_path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn account_rate_limiter_enforces_per_account_cap() {
        let limiter = AccountRateLimiter::new(2);
        let acct_a = Uuid::new_v4();
        let acct_b = Uuid::new_v4();
        assert!(limiter.allow(acct_a));
        assert!(limiter.allow(acct_a));
        assert!(!limiter.allow(acct_a));
        assert!(limiter.allow(acct_b));
    }

    #[test]
    fn parses_and_sanitizes_vision_ocr_card_json() {
        let raw = json!({
            "layout": "normal",
            "rarity": "rare",
            "faces": [{
                "name": "Aetherblade Vanguard",
                "manaCost": "{2}{U}{R}",
                "typeLine": "Creature \u{2014} Human Artificer",
                "oracleText": "Flying, haste",
                "flavorText": "Speed is precision.",
                "power": "3",
                "toughness": "2",
                "loyalty": null,
                "defense": null,
                "colorIndicator": null,
                "artist": "Elena Vance"
            }],
            "artCropBox": { "x": 0.08, "y": 0.11, "width": 0.84, "height": 0.43 },
            "warnings": []
        })
        .to_string();
        let parsed = parse_vision_extracted_card(&raw).expect("parses valid OCR output");
        assert_eq!(parsed.faces[0].name, "Aetherblade Vanguard");
        assert!(parsed.art_crop_box.is_some());
    }
}
