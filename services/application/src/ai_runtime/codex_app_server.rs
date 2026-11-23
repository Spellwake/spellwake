//! Persistent ChatGPT-login Codex `app-server` provider in pure Rust.
//!
//! Mirrors Prebreak's production ChatGPT-login architecture:
//! - Spawns one persistent `codex app-server --listen stdio://` child process
//!   with zero coding tools, zero MCP servers, and an isolated empty working
//!   directory.
//! - Authenticates using the ChatGPT login session (`auth.json` in a private
//!   `0700` `CODEX_HOME`, seeded optionally from `SPELLWAKE_AI_CHATGPT_AUTH_JSON`
//!   or inherited from the local developer login) and fails closed if
//!   `account/read` does not confirm `account.type == "chatgpt"`.
//! - Places `COMPACT_ACTION_GUIDE` once in `thread/start` `baseInstructions` and
//!   reuses one ephemeral thread per `(game_id, seat_id, tier)` for up to
//!   `MAX_TURNS_PER_THREAD` turns so OpenAI prompt caching achieves ~95-98%
//!   cache hit rates and ~2-3.5s turn latency without per-turn process startup.
use super::{
    compact::COMPACT_ACTION_GUIDE,
    provider::{
        AiDecisionProvider, AiModelRequest, AiModelResponse, AiProviderError, AiTierCatalog,
        AiUsage,
    },
    quota::AiTier,
};
use base64::Engine as _;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Mutex,
    thread,
    time::Instant,
};
use uuid::Uuid;

/// Pinned static Linux x86_64 musl Codex CLI installed by `services/application/Dockerfile`.
pub const HOSTED_CODEX_EXECUTABLE: &str =
    "/opt/spellwake-codex/package/vendor/x86_64-unknown-linux-musl/bin/codex";

/// Rotate a seat's ephemeral app-server thread after this many turns so context
/// stays compact while maximizing prompt-cache reuse within a game.
const MAX_TURNS_PER_THREAD: u32 = 18;

/// Bound retained seat thread handles in long-lived server processes.
const MAX_TRACKED_SEAT_THREADS: usize = 256;

/// Configuration for the persistent ChatGPT-login `codex app-server` provider.
#[derive(Clone)]
pub struct CodexAppServerConfig {
    pub cli_path: PathBuf,
    pub codex_home: Option<PathBuf>,
    pub seed_auth_json: Option<String>,
    pub catalog: AiTierCatalog,
    /// Number of lazily-spawned sharded `codex app-server` worker slots (`1..=4`).
    /// Each `(game_id, seat_id, tier)` hashes deterministically to one slot so
    /// per-seat prompt caches stay warm while concurrent matches run in parallel.
    pub concurrency: usize,
}

impl CodexAppServerConfig {
    /// Resolve executable path, optional private `CODEX_HOME`, optional
    /// `SPELLWAKE_AI_CHATGPT_AUTH_JSON` seed, tier model catalog, and concurrency from env.
    /// # Errors
    /// Returns `AiProviderError::Configuration` if no Codex executable is found.
    pub fn from_env(catalog: AiTierCatalog) -> Result<Self, AiProviderError> {
        let cli_path = if let Ok(explicit) = std::env::var("SPELLWAKE_AI_CODEX_CLI")
            && !explicit.trim().is_empty()
        {
            PathBuf::from(explicit.trim())
        } else if Path::new(HOSTED_CODEX_EXECUTABLE).is_file() {
            PathBuf::from(HOSTED_CODEX_EXECUTABLE)
        } else {
            return Err(AiProviderError::Configuration(
                "SPELLWAKE_AI_CODEX_CLI or hosted codex executable required for chatgpt provider",
            ));
        };
        let codex_home = std::env::var("SPELLWAKE_AI_CODEX_HOME")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let seed_auth_json = std::env::var("SPELLWAKE_AI_CHATGPT_AUTH_JSON")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let concurrency = std::env::var("SPELLWAKE_AI_CONCURRENCY")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(2)
            .clamp(1, 4);
        Ok(Self {
            cli_path,
            codex_home,
            seed_auth_json,
            catalog,
            concurrency,
        })
    }
}

/// Persistent Rust client over a sharded pool of `codex app-server --listen stdio://` children.
pub struct CodexAppServerProvider {
    cli_path: PathBuf,
    fixed_cwd: PathBuf,
    codex_home: Option<PathBuf>,
    catalog: AiTierCatalog,
    sessions: Vec<Mutex<Option<AppServerSession>>>,
}

impl CodexAppServerProvider {
    /// Prepare the fixed empty working directory and optional private
    /// `CODEX_HOME/auth.json` seed, then construct the provider.
    /// # Errors
    /// Fails closed if the CLI binary is missing, the seed auth payload is invalid,
    /// or private directory setup fails.
    pub fn new(config: CodexAppServerConfig) -> Result<Self, AiProviderError> {
        if !config.cli_path.is_file() {
            return Err(AiProviderError::Configuration("Codex CLI binary not found"));
        }
        let fixed_cwd = std::env::temp_dir().join("spellwake-codex-appserver-cwd");
        fs::create_dir_all(&fixed_cwd)
            .map_err(|_| AiProviderError::Configuration("failed to create Codex working dir"))?;

        let codex_home = prepare_codex_home(config.codex_home, config.seed_auth_json.as_deref())?;
        let slot_count = config.concurrency.clamp(1, 4);
        let sessions = (0..slot_count).map(|_| Mutex::new(None)).collect();

        Ok(Self {
            cli_path: config.cli_path,
            fixed_cwd,
            codex_home,
            catalog: config.catalog,
            sessions,
        })
    }

    fn slot_index(&self, game_id: Uuid, seat_id: Uuid, tier: AiTier) -> usize {
        if self.sessions.len() <= 1 {
            return 0;
        }
        let mixed = game_id.as_u128() ^ seat_id.as_u128().rotate_left(17) ^ (tier as u128);
        usize::try_from(mixed % (self.sessions.len() as u128)).unwrap_or(0)
    }

    /// Verify live ChatGPT login readiness eagerly by booting slot 0 and
    /// confirming `account.type == "chatgpt"`.
    /// # Errors
    /// Returns an error if the app-server fails to start or is not logged in via ChatGPT.
    pub fn verify_chatgpt_login(&self) -> Result<(), AiProviderError> {
        let mut guard = self.sessions[0]
            .lock()
            .map_err(|_| AiProviderError::Transport("app-server mutex poisoned".into()))?;
        if guard.is_none() {
            *guard = Some(AppServerSession::spawn(
                &self.cli_path,
                &self.fixed_cwd,
                self.codex_home.as_deref(),
            )?);
        }
        Ok(())
    }
}

impl AiDecisionProvider for CodexAppServerProvider {
    fn decide(&self, request: &AiModelRequest) -> Result<AiModelResponse, AiProviderError> {
        let tier_cfg = self.catalog.for_tier(request.tier);
        let started = Instant::now();
        let slot = self.slot_index(request.game_id, request.seat_id, request.tier);
        let mut guard = self.sessions[slot]
            .lock()
            .map_err(|_| AiProviderError::Transport("app-server mutex poisoned".into()))?;

        if guard.is_none() {
            *guard = Some(AppServerSession::spawn(
                &self.cli_path,
                &self.fixed_cwd,
                self.codex_home.as_deref(),
            )?);
        }

        let turn_input = strip_static_guide(&request.prompt);
        let seat_key = (request.game_id, request.seat_id, request.tier);

        let result = {
            let Some(session) = guard.as_mut() else {
                return Err(AiProviderError::Transport("app-server unavailable".into()));
            };
            session.run_decision(
                seat_key,
                &tier_cfg.model,
                tier_cfg.effort.as_str(),
                turn_input,
                &request.schema,
                &self.fixed_cwd,
            )
        };

        match result {
            Ok((raw_json, usage)) => Ok(AiModelResponse {
                raw_json,
                model: tier_cfg.model.clone(),
                effort: tier_cfg.effort,
                usage,
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            }),
            Err(err) => {
                // Drop any broken session so the next attempt respawns cleanly.
                *guard = None;
                Err(err)
            }
        }
    }
}

#[derive(Clone)]
struct SeatThread {
    thread_id: String,
    turns_used: u32,
}

struct AppServerSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    threads: HashMap<(Uuid, Uuid, AiTier), SeatThread>,
}

impl AppServerSession {
    fn spawn(
        cli_path: &Path,
        fixed_cwd: &Path,
        codex_home: Option<&Path>,
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
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(home) = codex_home {
            cmd.env("CODEX_HOME", home);
        }

        let mut child = cmd.spawn().map_err(|e| {
            AiProviderError::Transport(format!("codex app-server spawn failed: {e}"))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AiProviderError::Transport("codex app-server stdin missing".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AiProviderError::Transport("codex app-server stdout missing".into()))?;
        if let Some(mut stderr) = child.stderr.take() {
            // Drain stderr continuously so the child never blocks on a full pipe;
            // never log raw stderr because it could contain auth/provider details.
            let _ = thread::Builder::new()
                .name("spellwake-codex-stderr".into())
                .spawn(move || {
                    let mut buf = [0_u8; 4096];
                    while let Ok(n) = stderr.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                    }
                });
        }

        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            threads: HashMap::new(),
        };

        session.request(
            "initialize",
            json!({
                "clientInfo": { "name": "spellwake-ai-runtime", "version": "1.0.0" },
                "capabilities": { "experimentalApi": true },
            }),
        )?;
        session.notify("initialized", None)?;

        let account = session.request("account/read", json!({ "refreshToken": false }))?;
        let account_type = account
            .get("account")
            .and_then(|a| a.get("type"))
            .and_then(Value::as_str);
        if account_type != Some("chatgpt") {
            return Err(AiProviderError::Configuration(
                "Codex app-server is not logged in with a ChatGPT account",
            ));
        }
        Ok(session)
    }

    fn send_json(&mut self, value: &Value) -> Result<(), AiProviderError> {
        let mut line = serde_json::to_string(value)
            .map_err(|_| AiProviderError::MalformedResponse("failed to encode JSON-RPC frame"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| AiProviderError::Transport(format!("app-server write failed: {e}")))
    }

    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), AiProviderError> {
        let frame = match params {
            Some(p) => json!({ "method": method, "params": p }),
            None => json!({ "method": method }),
        };
        self.send_json(&frame)
    }

    fn read_frame(&mut self) -> Result<Value, AiProviderError> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .stdout
                .read_line(&mut line)
                .map_err(|e| AiProviderError::Transport(format!("app-server read failed: {e}")))?;
            if n == 0 {
                return Err(AiProviderError::Transport(
                    "app-server closed stdout".into(),
                ));
            }
            if line.trim().is_empty() {
                continue;
            }
            return serde_json::from_str(&line)
                .map_err(|_| AiProviderError::MalformedResponse("invalid JSON-RPC frame"));
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, AiProviderError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send_json(&json!({ "id": id, "method": method, "params": params }))?;
        loop {
            let frame = self.read_frame()?;
            let is_response = frame.get("id").and_then(Value::as_u64) == Some(id)
                && frame.get("method").is_none();
            if is_response {
                if let Some(err) = frame.get("error") {
                    let msg = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown RPC error");
                    return Err(AiProviderError::Transport(format!(
                        "app-server {method} failed: {msg}"
                    )));
                }
                return Ok(frame.get("result").cloned().unwrap_or(Value::Null));
            }
            // Deny any unexpected interactive server-to-client request cleanly.
            if let Some(req_id) = frame.get("id")
                && frame.get("method").is_some()
            {
                let _ = self.send_json(&json!({
                    "id": req_id,
                    "error": { "code": -32601, "message": "unsupported interactive request" }
                }));
            }
        }
    }

    fn ensure_seat_thread(
        &mut self,
        seat_key: (Uuid, Uuid, AiTier),
        model: &str,
        cwd: &Path,
    ) -> Result<String, AiProviderError> {
        if let Some(existing) = self.threads.get(&seat_key)
            && existing.turns_used < MAX_TURNS_PER_THREAD
        {
            return Ok(existing.thread_id.clone());
        }
        if self.threads.len() >= MAX_TRACKED_SEAT_THREADS {
            self.threads.clear();
        }
        let result = self.request(
            "thread/start",
            json!({
                "cwd": cwd.to_string_lossy(),
                "model": model,
                "ephemeral": true,
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "baseInstructions": COMPACT_ACTION_GUIDE,
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
        )?;
        let thread_id = result
            .get("thread")
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(AiProviderError::MalformedResponse(
                "thread/start missing thread.id",
            ))?
            .to_owned();
        self.threads.insert(
            seat_key,
            SeatThread {
                thread_id: thread_id.clone(),
                turns_used: 0,
            },
        );
        Ok(thread_id)
    }

    fn run_decision(
        &mut self,
        seat_key: (Uuid, Uuid, AiTier),
        model: &str,
        effort: &str,
        turn_input: &str,
        schema: &Value,
        cwd: &Path,
    ) -> Result<(String, AiUsage), AiProviderError> {
        let thread_id = self.ensure_seat_thread(seat_key, model, cwd)?;
        let turn_res = self.request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "model": model,
                "effort": effort,
                "approvalPolicy": "never",
                "outputSchema": schema,
                "input": [{ "type": "text", "text": turn_input }],
            }),
        )?;
        let turn_id = turn_res
            .get("turn")
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(AiProviderError::MalformedResponse(
                "turn/start missing turn.id",
            ))?
            .to_owned();

        let mut reply: Option<String> = None;
        let mut usage = AiUsage::default();

        loop {
            let frame = self.read_frame()?;
            // Reject any server-initiated tool/approval requests immediately.
            if let Some(req_id) = frame.get("id")
                && frame.get("method").is_some()
            {
                let _ = self.send_json(&json!({
                    "id": req_id,
                    "error": { "code": -32601, "message": "tools disabled in gameplay runtime" }
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
                "thread/tokenUsage/updated" => {
                    if let Some(last) = params.get("tokenUsage").and_then(|u| u.get("last")) {
                        usage = parse_app_server_usage(last);
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
                        return Err(AiProviderError::Transport(format!(
                            "app-server turn ended with status {status}"
                        )));
                    }
                    break;
                }
                _ => {}
            }
        }

        if let Some(entry) = self.threads.get_mut(&seat_key) {
            entry.turns_used = entry.turns_used.saturating_add(1);
        }
        let raw_json = reply.ok_or(AiProviderError::MalformedResponse(
            "app-server turn completed without agentMessage text",
        ))?;
        Ok((raw_json, usage))
    }
}

impl Drop for AppServerSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse_app_server_usage(last: &Value) -> AiUsage {
    AiUsage {
        input_tokens: last.get("inputTokens").and_then(Value::as_u64).unwrap_or(0),
        cached_input_tokens: last
            .get("cachedInputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: last
            .get("outputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reasoning_tokens: last
            .get("reasoningOutputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

/// Remove the leading static `COMPACT_ACTION_GUIDE` when it is already installed
/// as `baseInstructions` on the seat's `codex app-server` thread.
fn strip_static_guide(prompt: &str) -> &str {
    prompt
        .strip_prefix(COMPACT_ACTION_GUIDE)
        .map(str::trim_start)
        .filter(|s| !s.is_empty())
        .unwrap_or(prompt)
}

/// Prepare a private `0700` `CODEX_HOME` directory and seed `0600` `auth.json`
/// when `SPELLWAKE_AI_CHATGPT_AUTH_JSON` is supplied (raw JSON or base64).
fn prepare_codex_home(
    configured_home: Option<PathBuf>,
    seed_auth_json: Option<&str>,
) -> Result<Option<PathBuf>, AiProviderError> {
    let Some(raw_seed) = seed_auth_json else {
        return Ok(configured_home);
    };
    let decoded_bytes = decode_and_validate_auth_json(raw_seed)?;
    let home = configured_home
        .unwrap_or_else(|| std::env::temp_dir().join("spellwake-chatgpt-codex-home"));
    fs::create_dir_all(&home)
        .map_err(|_| AiProviderError::Configuration("failed to create private CODEX_HOME"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&home, fs::Permissions::from_mode(0o700));
    }
    let auth_path = home.join("auth.json");
    // Preserve an already-refreshed auth.json on a persistent volume if present.
    if !auth_path.exists() {
        write_private_file(&auth_path, &decoded_bytes)?;
    }
    Ok(Some(home))
}

fn decode_and_validate_auth_json(raw: &str) -> Result<Vec<u8>, AiProviderError> {
    let trimmed = raw.trim();
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
            "SPELLWAKE_AI_CHATGPT_AUTH_JSON exceeds 64 KiB limit",
        ));
    }
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|_| {
        AiProviderError::Configuration("SPELLWAKE_AI_CHATGPT_AUTH_JSON is not valid JSON")
    })?;
    let tokens =
        parsed
            .get("tokens")
            .and_then(Value::as_object)
            .ok_or(AiProviderError::Configuration(
                "SPELLWAKE_AI_CHATGPT_AUTH_JSON missing tokens object",
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
            "SPELLWAKE_AI_CHATGPT_AUTH_JSON missing ChatGPT token fields",
        ));
    }
    Ok(bytes)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), AiProviderError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| AiProviderError::Configuration("failed to create private auth.json"))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| AiProviderError::Configuration("failed to write private auth.json"))
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
            .map_err(|_| AiProviderError::Configuration("failed to write auth.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_raw_and_base64_chatgpt_auth_seed() {
        let sample = json!({
            "tokens": {
                "access_token": "tok-a",
                "refresh_token": "tok-r",
                "account_id": "acct-1"
            }
        })
        .to_string();
        assert!(decode_and_validate_auth_json(&sample).is_ok());
        let b64 = base64::engine::general_purpose::STANDARD.encode(sample.as_bytes());
        assert!(decode_and_validate_auth_json(&b64).is_ok());
        assert!(decode_and_validate_auth_json("{}").is_err());
    }

    #[test]
    fn strips_static_guide_for_app_server_turns() {
        let full = format!("{COMPACT_ACTION_GUIDE}\n\nSTATE:\n{{\"boundary\":\"priority\"}}");
        assert_eq!(
            strip_static_guide(&full),
            "STATE:\n{\"boundary\":\"priority\"}"
        );
    }
}
