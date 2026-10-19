//! opencode backend: drives an `opencode` server through its HTTP/SSE API.
//!
//! This lets cachycli use opencode's sessions, models and tools as its agent
//! while keeping the cachycli interface. The server is either discovered at a
//! configured URL, or started automatically (`opencode serve`).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::agent::{AgentEvent, AgentSession};
use crate::config::{Config, OpencodeTransport};
use crate::message::StopReason;
use crate::session::PlanEntry;
use crate::tools::TodoItem;

pub struct OpencodeBackend {
    config: Arc<Config>,
    cwd: PathBuf,
    /// Resolved once the server is up.
    base_url: RwLock<Option<String>>,
    client: reqwest::Client,
    session_id: Mutex<Option<String>>,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: Mutex<Option<CancellationToken>>,
    server_child: Mutex<Option<Child>>,
    label: RwLock<String>,
    context: RwLock<(u64, u64)>,
    /// Model chosen at runtime with `/model`, overriding the config.
    model_override: RwLock<Option<String>>,
    /// Set when the last turn failed because the credential expired, so the
    /// next attempt can restart the server (a reused server may hold an old
    /// credential in memory) and try once more.
    auth_failed: std::sync::atomic::AtomicBool,
    /// True after the automatic restart, so we only retry once.
    restarted_for_auth: std::sync::atomic::AtomicBool,
    /// True after a restart triggered by a server error, so we retry once.
    restarted_for_errors: std::sync::atomic::AtomicBool,
    /// Port offset used after a restart (so a stale server is not reused).
    port_bump: std::sync::atomic::AtomicUsize,
    /// Weak self-reference installed by `new`, used by the `AgentSession` impl.
    self_ref: std::sync::OnceLock<std::sync::Weak<Self>>,
}

impl OpencodeBackend {
    pub fn new(
        config: Arc<Config>,
        cwd: PathBuf,
        tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Arc<Self> {
        let arc = Arc::new(Self {
            config,
            cwd,
            base_url: RwLock::new(None),
            client: reqwest::Client::builder()
                .user_agent(crate::user_agent())
                .build()
                .unwrap_or_default(),
            session_id: Mutex::new(None),
            tx,
            cancel: Mutex::new(None),
            server_child: Mutex::new(None),
            label: RwLock::new("opencode".into()),
            context: RwLock::new((0, 0)),
            model_override: RwLock::new(None),
            auth_failed: std::sync::atomic::AtomicBool::new(false),
            restarted_for_auth: std::sync::atomic::AtomicBool::new(false),
            restarted_for_errors: std::sync::atomic::AtomicBool::new(false),
            port_bump: std::sync::atomic::AtomicUsize::new(0),
            self_ref: std::sync::OnceLock::new(),
        });
        let _ = arc.self_ref.set(Arc::downgrade(&arc));
        arc
    }

    fn self_arc(&self) -> Option<Arc<Self>> {
        self.self_ref.get().and_then(|w| w.upgrade())
    }

    /// Resolve a live server, starting one when needed.
    pub async fn ensure_server(&self) -> Result<String> {
        if let Some(url) = self.base_url.read().await.clone() {
            return Ok(url);
        }
        let oc = &self.config.agent.opencode;
        let candidates: Vec<String> = std::env::var("OPENCODE_SERVER_URL")
            .ok()
            .into_iter()
            .chain(oc.url.clone())
            .collect();
        for url in candidates {
            let url = url.trim_end_matches('/').to_string();
            if self.health(&url).await {
                *self.base_url.write().await = Some(url.clone());
                return Ok(url);
            }
        }
        let bump = self.port_bump.load(std::sync::atomic::Ordering::SeqCst) as u16;
        let port = if oc.port != 0 { oc.port } else { 4191 };
        let port = port.wrapping_add(bump);
        let url = format!("http://{}:{}", oc.hostname, port);
        if self.health(&url).await {
            *self.base_url.write().await = Some(url.clone());
            return Ok(url);
        }
        if !oc.autostart {
            anyhow::bail!(
                "no opencode server reachable at {url}. Start one with `opencode serve` or set [agent.opencode] url."
            );
        }
        // Start our own server.
        let mut cmd = tokio::process::Command::new(&oc.command);
        let mut args = oc.args.clone();
        if args.is_empty() {
            args.push("serve".into());
        }
        // Ensure hostname/port flags are present.
        if !args.iter().any(|a| a == "--port") {
            args.push("--port".into());
            args.push(port.to_string());
        }
        if !args.iter().any(|a| a == "--hostname") {
            args.push("--hostname".into());
            args.push(oc.hostname.clone());
        }
        cmd.args(&args);
        cmd.current_dir(&self.cwd);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        let child = cmd
            .spawn()
            .with_context(|| format!("starting opencode server ({})", oc.command))?;
        *self.server_child.lock().await = Some(child);

        // Wait for health.
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if self.health(&url).await {
                *self.base_url.write().await = Some(url.clone());
                return Ok(url);
            }
        }
        anyhow::bail!("opencode server did not become healthy at {url}")
    }

    async fn health(&self, url: &str) -> bool {
        let resp = self
            .client
            .get(format!("{url}/api/health"))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await;
        match resp {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }

    async fn ensure_session(&self, base: &str) -> Result<String> {
        if let Some(id) = self.session_id.lock().await.clone() {
            return Ok(id);
        }
        // The API the opencode CLI itself uses (verified against 1.18.31 with a
        // logging proxy): POST /session?directory=… with the permission denies
        // the CLI also sends, so questions never block the turn.
        let body = json!({
            "permission": [
                { "permission": "question", "action": "deny", "pattern": "*" },
                { "permission": "plan_enter", "action": "deny", "pattern": "*" },
                { "permission": "plan_exit", "action": "deny", "pattern": "*" }
            ]
        });
        let resp = self
            .client
            .post(format!(
                "{base}/session?directory={}",
                urlencode(&self.cwd.to_string_lossy())
            ))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let value: Value = resp.json().await.unwrap_or(json!({}));
        if !status.is_success() {
            // A stale server can answer 5xx for everything (an opencode that
            // migrated its database under it, for one) while a fresh one is
            // fine. Restart once and try again before giving up.
            if status.is_server_error()
                && !self
                    .restarted_for_errors
                    .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                let _ = self.tx.send(AgentEvent::Info(
                    "the opencode server answered an error — restarting it and retrying".into(),
                ));
                self.restart_server().await;
                if let Ok(base) = self.ensure_server().await {
                    return Box::pin(self.ensure_session(&base)).await;
                }
            }
            // Otherwise, a server that cannot create sessions can still run
            // them: continue the newest one it has for this directory.
            if let Some(id) = self.existing_session(base).await {
                let _ = self.tx.send(AgentEvent::Info(format!(
                    "could not create an opencode session (HTTP {status}) — continuing {id}"
                )));
                *self.session_id.lock().await = Some(id.clone());
                return Ok(id);
            }
            anyhow::bail!(
                "opencode session/create failed (HTTP {status}): {value} — run `{} serve` in this \
                 directory to see its error (a failing migration is the usual cause); updating \
                 opencode or clearing its storage at ~/.local/share/opencode fixes it",
                self.config.agent.opencode.command
            );
        }
        let session = value
            .get("data")
            .or_else(|| value.get("session"))
            .cloned()
            .unwrap_or(value);
        let id = session
            .get("id")
            .or_else(|| session.get("sessionID"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("opencode returned no session id: {session}"))?
            .to_string();
        if let Some(model) = session.get("model") {
            let provider = model
                .get("providerID")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let id = model.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if !id.is_empty() {
                *self.label.write().await = format!("{provider}/{id}");
            }
        }
        *self.session_id.lock().await = Some(id.clone());
        Ok(id)
    }

    /// The newest session the server already has for this directory.
    async fn existing_session(&self, base: &str) -> Option<String> {
        let resp = self
            .client
            .get(format!("{base}/session"))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let value: Value = resp.json().await.ok()?;
        let directory = self.cwd.to_string_lossy().to_string();
        value
            .as_array()?
            .iter()
            .filter(|session| {
                session.get("directory").and_then(|d| d.as_str()) == Some(directory.as_str())
            })
            .max_by_key(|session| {
                session
                    .get("time")
                    .and_then(|t| t.get("updated"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
            })
            .and_then(|session| session.get("id").and_then(|v| v.as_str()))
            .map(|id| id.to_string())
    }

    /// Every model the server offers for the providers you are signed in to,
    /// as `provider/model`.
    ///
    /// `opencode models` is the CLI's job, but it can fail (an unmigrated
    /// database, for one) while the server answers, and the server is the same
    /// source the turns use.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let base = self.ensure_server().await?;
        let resp = self
            .client
            .get(format!("{base}/provider"))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("the opencode server answered HTTP {}", resp.status());
        }
        let value: Value = resp.json().await?;
        let connected: Vec<String> = value
            .get("connected")
            .and_then(|c| c.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|id| id.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let mut out: Vec<String> = Vec::new();
        for provider in value
            .get("all")
            .and_then(|all| all.as_array())
            .cloned()
            .unwrap_or_default()
        {
            let Some(id) = provider.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            // Signed-in providers only: the rest cannot run a turn anyway.
            if !connected.is_empty() && !connected.iter().any(|known| known == id) {
                continue;
            }
            if let Some(models) = provider.get("models").and_then(|m| m.as_object()) {
                for model in models.keys() {
                    out.push(format!("{id}/{model}"));
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// The model to use: the runtime choice wins over the config.
    pub async fn effective_model(&self) -> Option<String> {
        if let Some(model) = self.model_override.read().await.clone() {
            return Some(model);
        }
        self.config.agent.opencode.model.clone()
    }

    /// Drop a server that may hold a stale credential: kill the one we started
    /// and forget the URL so the next attempt starts (or finds) a fresh one on
    /// a new port.
    pub async fn restart_server(&self) {
        if let Some(mut child) = self.server_child.lock().await.take() {
            let _ = child.kill().await;
        }
        *self.base_url.write().await = None;
        let used = self
            .port_bump
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if used > 8 {
            self.port_bump.store(0, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// `/model`: switch models without restarting the provider. The next prompt
    /// opens a fresh opencode session so the choice takes effect.
    pub async fn set_model_override(&self, model: Option<String>) {
        *self.model_override.write().await = model.clone();
        // The model travels with each message, so the session (and its cached
        // context) survives a switch — switching is instant and cheap.
        if let Some(model) = model {
            *self.label.write().await = model;
        }
    }

    /// Send one prompt and stream the resulting events.
    pub async fn prompt(self: &Arc<Self>, text: String) -> Result<StopReason> {
        let transport = self.transport().await;
        match transport {
            OpencodeTransport::Cli => self.prompt_cli(text).await,
            OpencodeTransport::Server => self.prompt_server(text).await,
            OpencodeTransport::Auto => match self.ensure_server().await {
                Ok(_) => {
                    let first = self.prompt_server(text.clone()).await;
                    // A reused server can hold a credential that has since been
                    // renewed: restart it and try once more before giving up.
                    let auth_failed = self
                        .auth_failed
                        .swap(false, std::sync::atomic::Ordering::SeqCst);
                    if auth_failed
                        && !self
                            .restarted_for_auth
                            .swap(true, std::sync::atomic::Ordering::SeqCst)
                    {
                        let _ = self.tx.send(AgentEvent::Info(
                            "opencode reported an expired credential — restarting the server and retrying"
                                .into(),
                        ));
                        self.restart_server().await;
                        if self.ensure_server().await.is_ok() {
                            return self.prompt_server(text).await;
                        }
                    }
                    first
                }
                Err(e) => {
                    let _ = self.tx.send(AgentEvent::Info(format!(
                        "opencode server unavailable ({e}); falling back to the CLI transport"
                    )));
                    self.prompt_cli(text).await
                }
            },
        }
    }

    /// Decide which transport to use for this session.
    pub async fn transport(&self) -> OpencodeTransport {
        let oc = &self.config.agent.opencode;
        if oc.transport != OpencodeTransport::Auto {
            return oc.transport;
        }
        // Explicit URL or env var -> server. Otherwise prefer the CLI, which
        // uses the user's own authenticated opencode installation.
        if oc.url.is_some() || std::env::var("OPENCODE_SERVER_URL").is_ok() {
            return OpencodeTransport::Server;
        }
        // Auto: the server streams tokens and asks for permissions the way the
        // reference interface does; the CLI is the fallback.
        OpencodeTransport::Auto
    }

    /// Drive `opencode run --format json` as a subprocess.
    async fn prompt_cli(self: &Arc<Self>, text: String) -> Result<StopReason> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let oc = &self.config.agent.opencode;
        let token = CancellationToken::new();
        *self.cancel.lock().await = Some(token.clone());
        let _ = self.tx.send(AgentEvent::UserMessage { text: text.clone() });

        let mut cmd = tokio::process::Command::new(&oc.command);
        cmd.arg("run").arg("--format").arg("json");
        if let Some(model) = self.effective_model().await {
            cmd.arg("-m").arg(model);
        }
        if let Some(agent) = &oc.agent {
            cmd.arg("--agent").arg(agent);
        }
        if let Some(session) = self.session_id.lock().await.clone() {
            cmd.arg("--session").arg(session);
        }
        // Permissions in a non-interactive child: auto-approve to avoid a
        // deadlocked prompt (the UI still shows what ran).
        cmd.arg("--auto");
        cmd.arg(&text);
        cmd.current_dir(&self.cwd);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let message = format!("failed to spawn `{}`: {e}", oc.command);
                let _ = self.tx.send(AgentEvent::Error(message.clone()));
                return Ok(StopReason::Error);
            }
        };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.to_lowercase().contains("error") {
                        let _ = tx.send(AgentEvent::Info(format!("opencode: {line}")));
                    }
                }
            });
        }
        let Some(stdout) = stdout else {
            return Ok(StopReason::Error);
        };
        let mut lines = BufReader::new(stdout).lines();
        let mut stop = StopReason::EndTurn;
        let mut tool_calls_seen: std::collections::HashMap<String, String> = Default::default();
        loop {
            let next = tokio::select! {
                _ = token.cancelled() => {
                    let _ = child.kill().await;
                    let _ = self.tx.send(AgentEvent::Info("opencode run cancelled".into()));
                    return Ok(StopReason::Cancelled);
                }
                line = lines.next_line() => line,
            };
            match next {
                Ok(Some(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
                        continue;
                    };
                    let kind = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(session_id) = event.get("sessionID").and_then(|v| v.as_str()) {
                        let mut guard = self.session_id.lock().await;
                        if guard.as_deref() != Some(session_id) {
                            *guard = Some(session_id.to_string());
                        }
                    }
                    match kind {
                        "text" => {
                            let text = event
                                .get("text")
                                .or_else(|| event.get("part").and_then(|p| p.get("text")))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !text.is_empty() {
                                let _ = self.tx.send(AgentEvent::TextDelta(text.to_string()));
                            }
                        }
                        "reasoning" => {
                            let text = event
                                .get("text")
                                .or_else(|| event.get("part").and_then(|p| p.get("text")))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !text.is_empty() {
                                let _ = self.tx.send(AgentEvent::ReasoningDelta(text.to_string()));
                            }
                        }
                        "tool" => {
                            let part = event.get("part").unwrap_or(&event);
                            let call_id = part
                                .get("callID")
                                .or_else(|| event.get("callID"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("call")
                                .to_string();
                            let tool = part
                                .get("tool")
                                .or_else(|| event.get("tool"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            let state = part.get("state").cloned().unwrap_or(json!({}));
                            let status = state
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("running");
                            let input = state
                                .get("input")
                                .cloned()
                                .or_else(|| {
                                    state.get("raw").and_then(|r| {
                                        serde_json::from_str::<Value>(r.as_str().unwrap_or("")).ok()
                                    })
                                })
                                .unwrap_or(json!({}));
                            if !tool_calls_seen.contains_key(&call_id) {
                                tool_calls_seen.insert(call_id.clone(), tool.clone());
                                let _ = self.tx.send(AgentEvent::ToolCall {
                                    id: call_id.clone(),
                                    name: tool.clone(),
                                    title: format!("{tool} {}", summarize_tool_input(&input)),
                                    kind: "tool".into(),
                                    arguments: input.clone(),
                                });
                            }
                            let summary = state
                                .get("output")
                                .or_else(|| state.get("metadata").and_then(|m| m.get("output")))
                                .and_then(|v| v.as_str())
                                .map(|s| s.lines().take(6).collect::<Vec<_>>().join("\n"));
                            let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                                id: call_id,
                                status: status.to_string(),
                                title: None,
                                summary,
                                meta: None,
                            });
                        }
                        "step_finish" | "step-start" | "step_start" => {
                            let part = event.get("part").unwrap_or(&event);
                            if let Some(tokens) = part.get("tokens") {
                                let input =
                                    tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
                                let output =
                                    tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
                                let reasoning = tokens
                                    .get("reasoning")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                // `input` already includes cached reads; the
                                // context used is input + generated output.
                                let used = input + output + reasoning;
                                let mut ctx = self.context.write().await;
                                if used > ctx.0 {
                                    ctx.0 = used;
                                }
                                if ctx.1 == 0 {
                                    ctx.1 = 200_000;
                                }
                                if ctx.0 > ctx.1 {
                                    // Unknown larger window: round up to 32K.
                                    ctx.1 = ((ctx.0 / 32_000) + 1) * 32_000;
                                }
                                let size = ctx.1;
                                let _ = self.tx.send(AgentEvent::Usage {
                                    input_tokens: input,
                                    output_tokens: output,
                                    context_used: ctx.0,
                                    context_size: size,
                                });
                            }
                            if let Some(model) = part.get("model") {
                                let provider = model
                                    .get("providerID")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let id = model.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                if !id.is_empty() {
                                    *self.label.write().await = format!("{provider}/{id}")
                                        .trim_start_matches('/')
                                        .to_string();
                                }
                            }
                        }
                        "error" => {
                            let message = event
                                .get("error")
                                .map(|e| {
                                    e.get("data")
                                        .and_then(|d| d.get("message"))
                                        .and_then(|m| m.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| e.to_string())
                                })
                                .unwrap_or_else(|| "unknown error".into());
                            let _ = self.tx.send(AgentEvent::Error(with_auth_hint(&message)));
                            stop = StopReason::Error;
                        }
                        _ => {}
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = self
                        .tx
                        .send(AgentEvent::Error(format!("opencode output error: {e}")));
                    stop = StopReason::Error;
                    break;
                }
            }
        }
        let status = child.wait().await;
        let _ = self.cancel.lock().await.take();
        if let Ok(status) = status {
            if !status.success() && stop != StopReason::Error {
                stop = StopReason::Error;
            }
        }
        Ok(stop)
    }

    /// Drive an opencode server over HTTP + SSE.
    async fn prompt_server(self: &Arc<Self>, text: String) -> Result<StopReason> {
        let base = self.ensure_server().await?;
        let session = self.ensure_session(&base).await?;
        let token = CancellationToken::new();
        *self.cancel.lock().await = Some(token.clone());

        let _ = self.tx.send(AgentEvent::UserMessage { text: text.clone() });

        // Subscribe to the event stream first so no updates are lost. The CLI
        // uses the global `/event?directory=` stream and filters by session.
        let (stop_tx, mut stop_rx) = mpsc::unbounded_channel::<StopReason>();
        let events_url = format!(
            "{base}/event?directory={}",
            urlencode(&self.cwd.to_string_lossy())
        );
        let this = self.clone();
        let listener_token = token.clone();
        let listener = tokio::spawn(async move {
            this.listen_events(&events_url, listener_token, stop_tx)
                .await
        });

        // Send the message: POST /session/{id}/message with the model and parts.
        let mut prompt_body = json!({
            "parts": [{ "type": "text", "text": text }]
        });
        if let Some(model) = self.effective_model().await {
            if let Some(reference) = parse_model_ref(&model) {
                prompt_body["model"] = reference;
            }
        }
        if let Some(agent) = &self.config.agent.opencode.agent {
            prompt_body["agent"] = json!(agent);
        }
        let resp = self
            .client
            .post(format!(
                "{base}/session/{session}/message?directory={}",
                urlencode(&self.cwd.to_string_lossy())
            ))
            .json(&prompt_body)
            .send()
            .await;
        match resp {
            Ok(r) if !r.status().is_success() => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                let _ = self.tx.send(AgentEvent::Error(with_auth_hint(&format!(
                    "opencode prompt failed (HTTP {status}): {body}"
                ))));
                let _ = self.cancel.lock().await.take();
                listener.abort();
                return Ok(StopReason::Error);
            }
            Err(e) => {
                let _ = self
                    .tx
                    .send(AgentEvent::Error(format!("opencode prompt failed: {e}")));
                let _ = self.cancel.lock().await.take();
                listener.abort();
                return Ok(StopReason::Error);
            }
            Ok(r) => {
                // The response body carries the admitted input; discard it.
                let _ = r.bytes().await;
            }
        }

        // Wait for the turn to end (session.idle) or cancellation.
        let stop = tokio::select! {
            reason = stop_rx.recv() => reason.unwrap_or(StopReason::EndTurn),
            _ = token.cancelled() => StopReason::Cancelled,
            _ = tokio::time::timeout(std::time::Duration::from_secs(60 * 60), listener) => {
                StopReason::EndTurn
            }
        };
        let _ = self.cancel.lock().await.take();
        Ok(stop)
    }

    /// Read the SSE stream and translate events into AgentEvents.
    async fn listen_events(
        self: Arc<Self>,
        url: &str,
        token: CancellationToken,
        stop_tx: mpsc::UnboundedSender<StopReason>,
    ) {
        use futures_util::StreamExt;
        let resp = match self
            .client
            .get(url)
            .header("accept", "text/event-stream")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let _ = self.tx.send(AgentEvent::Error(format!(
                    "opencode event stream failed: {e}"
                )));
                let _ = stop_tx.send(StopReason::Error);
                return;
            }
        };
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut stop = StopReason::EndTurn;
        // Text streamed as deltas, flushed when the part ends or a tool starts.
        let mut pending_text = String::new();
        let mut tool_parts: std::collections::HashMap<String, String> = Default::default();
        let mut reasoning_parts: std::collections::HashSet<String> = Default::default();
        loop {
            let chunk = tokio::select! {
                _ = token.cancelled() => {
                    let _ = stop_tx.send(StopReason::Cancelled);
                    return;
                }
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            let Ok(bytes) = chunk else { break };
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(pos) = buffer.find('\n') {
                let line: String = buffer.drain(..=pos).collect();
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let Ok(event) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                let kind = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                // V1 payloads use `properties`, V2 uses `data`.
                let payload = event
                    .get("data")
                    .or_else(|| event.get("properties"))
                    .cloned()
                    .unwrap_or(json!({}));
                let session_matches = payload
                    .get("sessionID")
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        let want = self.session_id.try_lock().ok().and_then(|g| g.clone());
                        match want {
                            Some(id) => s == id,
                            None => true,
                        }
                    })
                    .unwrap_or(true);
                if !session_matches {
                    continue;
                }
                match kind {
                    "session.next.text.delta" | "message.part.delta" => {
                        // v1 deltas carry `field` ("text" or "reasoning"); v2 text
                        // deltas have no field.
                        let field = payload
                            .get("field")
                            .and_then(|v| v.as_str())
                            .unwrap_or("text");
                        if let Some(delta) = payload.get("delta").and_then(|v| v.as_str()) {
                            match field {
                                "reasoning" => {
                                    let _ =
                                        self.tx.send(AgentEvent::ReasoningDelta(delta.to_string()));
                                }
                                "text" => {
                                    pending_text.push_str(delta);
                                    let _ = self.tx.send(AgentEvent::TextDelta(delta.to_string()));
                                }
                                _ => {}
                            }
                        } else if let Some(part) = payload.get("part") {
                            if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                    let _ = self.tx.send(AgentEvent::TextDelta(t.to_string()));
                                }
                            }
                        }
                    }
                    "session.next.reasoning.delta" => {
                        if let Some(delta) = payload.get("delta").and_then(|v| v.as_str()) {
                            let _ = self.tx.send(AgentEvent::ReasoningDelta(delta.to_string()));
                        }
                    }
                    "session.next.text.ended" => {
                        if let Some(text) = payload.get("text").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                let _ = self.tx.send(AgentEvent::MessageComplete {
                                    text: text.to_string(),
                                });
                            }
                        }
                    }
                    "session.next.tool.called" => {
                        let id = payload
                            .get("callID")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tool = payload
                            .get("tool")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        let input = payload.get("input").cloned().unwrap_or(json!({}));
                        let _ = self.tx.send(AgentEvent::ToolCall {
                            id,
                            name: tool.clone(),
                            title: tool,
                            kind: "tool".into(),
                            arguments: input,
                        });
                    }
                    "session.next.tool.success" => {
                        let id = payload
                            .get("callID")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let summary = payload
                            .get("result")
                            .map(render_tool_result)
                            .or_else(|| payload.get("content").map(render_tool_result))
                            .unwrap_or_default();
                        let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                            id,
                            status: "completed".into(),
                            title: None,
                            summary: Some(summary),
                            meta: None,
                        });
                    }
                    "session.next.tool.failed" => {
                        let id = payload
                            .get("callID")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let summary = payload
                            .get("error")
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| "tool failed".into());
                        let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                            id,
                            status: "failed".into(),
                            title: None,
                            summary: Some(summary),
                            meta: None,
                        });
                    }
                    "session.next.step.started" => {
                        if let Some(model) = payload.get("model") {
                            let provider = model
                                .get("providerID")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let id = model.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            if !id.is_empty() {
                                *self.label.write().await = format!("{provider}/{id}")
                                    .trim_start_matches('/')
                                    .to_string();
                            }
                        }
                    }
                    "session.next.step.ended" => {
                        if let Some(finish) = payload.get("finish").and_then(|v| v.as_str()) {
                            stop = match finish {
                                "tool-calls" | "tool_calls" => StopReason::EndTurn,
                                "length" => StopReason::MaxTokens,
                                "cancelled" | "aborted" => StopReason::Cancelled,
                                _ => StopReason::EndTurn,
                            };
                        }
                        if let Some(tokens) = payload.get("tokens") {
                            let input = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
                            let output = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
                            let cache_read = tokens
                                .get("cache")
                                .and_then(|c| c.get("read"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let mut ctx = self.context.write().await;
                            *ctx = (input + output + cache_read, ctx.1);
                            let _ = self.tx.send(AgentEvent::Usage {
                                input_tokens: input,
                                output_tokens: output,
                                context_used: input + output + cache_read,
                                context_size: ctx.1,
                            });
                        }
                    }
                    // --- API used by the opencode CLI (v1 events) -----------
                    "message.part.updated" => {
                        let part = payload.get("part").cloned().unwrap_or(json!({}));
                        match part.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                            "text" => {
                                // The part is complete once it carries an end time.
                                let ended = part
                                    .get("time")
                                    .and_then(|t| t.get("end"))
                                    .map(|v| !v.is_null())
                                    .unwrap_or(false);
                                if ended {
                                    let full = part
                                        .get("text")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let text = if full.is_empty() {
                                        std::mem::take(&mut pending_text)
                                    } else {
                                        pending_text.clear();
                                        full
                                    };
                                    if !text.trim().is_empty() {
                                        let _ = self.tx.send(AgentEvent::MessageComplete { text });
                                    }
                                }
                            }
                            "reasoning" => {
                                let id = part
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if !id.is_empty() && reasoning_parts.insert(id) {
                                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                        if !text.trim().is_empty() {
                                            let _ = self.tx.send(AgentEvent::ReasoningComplete {
                                                text: text.to_string(),
                                            });
                                        }
                                    }
                                }
                            }
                            "tool" => {
                                let call_id = part
                                    .get("callID")
                                    .or_else(|| part.get("id"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("call")
                                    .to_string();
                                let tool = part
                                    .get("tool")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("tool")
                                    .to_string();
                                let state = part.get("state").cloned().unwrap_or(json!({}));
                                let status = state
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("running");
                                let input = state.get("input").cloned().unwrap_or(json!({}));
                                if !tool_parts.contains_key(&call_id) {
                                    // Flush the text written before this tool call.
                                    if !pending_text.trim().is_empty() {
                                        let _ = self.tx.send(AgentEvent::MessageComplete {
                                            text: std::mem::take(&mut pending_text),
                                        });
                                    }
                                    tool_parts.insert(call_id.clone(), tool.clone());
                                    let _ = self.tx.send(AgentEvent::ToolCall {
                                        id: call_id.clone(),
                                        name: tool.clone(),
                                        title: format!("{tool} {}", summarize_tool_input(&input)),
                                        kind: "tool".into(),
                                        arguments: input.clone(),
                                    });
                                }
                                if status != "running" {
                                    let summary = state
                                        .get("output")
                                        .or_else(|| state.get("error"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.lines().take(8).collect::<Vec<_>>().join("\n"));
                                    let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                                        id: call_id,
                                        status: status.to_string(),
                                        title: None,
                                        summary,
                                        meta: None,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    "permission.updated" => {
                        let id = payload
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let kind = payload
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("permission")
                            .to_string();
                        let title = payload
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&kind)
                            .to_string();
                        let pattern = payload
                            .get("pattern")
                            .map(|p| p.to_string())
                            .unwrap_or_default();
                        let metadata = payload.get("metadata").cloned().unwrap_or(json!({}));
                        let detail = metadata
                            .get("command")
                            .or_else(|| metadata.get("filePath"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| pattern.trim_matches('"').to_string());
                        let _ = self.tx.send(AgentEvent::PermissionRequest {
                            id,
                            tool: kind,
                            title,
                            detail,
                            options: vec!["Yes".into(), "Yes, always".into(), "No, reject".into()],
                        });
                    }
                    "session.status" => {
                        if let Some(status) = payload.get("status") {
                            match status.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                                "retry" => {
                                    let attempt =
                                        status.get("attempt").and_then(|v| v.as_u64()).unwrap_or(0);
                                    let _ = self.tx.send(AgentEvent::Info(format!(
                                        "opencode is retrying (attempt {attempt})"
                                    )));
                                }
                                "idle" => {
                                    let _ = stop_tx.send(stop);
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }
                    "todo.updated" => {
                        if let Some(todos) = payload.get("todos").and_then(|v| v.as_array()) {
                            let list: Vec<TodoItem> = todos
                                .iter()
                                .map(|t| TodoItem {
                                    content: t
                                        .get("content")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    status: t
                                        .get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("pending")
                                        .to_string(),
                                    priority: t
                                        .get("priority")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("medium")
                                        .to_string(),
                                })
                                .collect();
                            let _ = self.tx.send(AgentEvent::Todos(list));
                        }
                    }
                    "plan.updated" => {
                        if let Some(entries) = payload.get("entries").and_then(|v| v.as_array()) {
                            let plan: Vec<PlanEntry> = entries
                                .iter()
                                .map(|e| PlanEntry {
                                    content: e
                                        .get("content")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    status: e
                                        .get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("pending")
                                        .to_string(),
                                    priority: e
                                        .get("priority")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("medium")
                                        .to_string(),
                                })
                                .collect();
                            let _ = self.tx.send(AgentEvent::Plan(plan));
                        }
                    }
                    "permission.asked" | "permission.v2.asked" => {
                        let id = payload
                            .get("id")
                            .or_else(|| payload.get("requestID"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let permission = payload
                            .get("permission")
                            .and_then(|v| v.as_str())
                            .unwrap_or("permission")
                            .to_string();
                        let patterns = payload.get("patterns").cloned().unwrap_or(json!([]));
                        let _ = self.tx.send(AgentEvent::PermissionRequest {
                            id,
                            tool: payload
                                .get("tool")
                                .and_then(|t| t.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or(&permission)
                                .to_string(),
                            title: permission,
                            detail: patterns.to_string(),
                            options: vec!["Yes".into(), "Yes, always".into(), "No, reject".into()],
                        });
                    }
                    "session.error" => {
                        // Remember auth failures so the retry can restart the
                        // server instead of trusting its cached credential.
                        if payload
                            .get("error")
                            .map(|e| e.to_string().to_lowercase())
                            .map(|text| {
                                text.contains("api_key_expired")
                                    || text.contains("api key has expired")
                                    || text.contains("authentication_error")
                            })
                            .unwrap_or(false)
                        {
                            self.auth_failed
                                .store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        // Prefer the human message inside the error object.
                        let message = payload
                            .get("error")
                            .map(|error| {
                                error
                                    .get("data")
                                    .and_then(|data| data.get("message"))
                                    .and_then(|m| m.as_str())
                                    .map(|m| m.to_string())
                                    .or_else(|| {
                                        error
                                            .get("message")
                                            .and_then(|m| m.as_str())
                                            .map(|m| m.to_string())
                                    })
                                    .unwrap_or_else(|| error.to_string())
                            })
                            .unwrap_or_else(|| payload.to_string());
                        let _ = self.tx.send(AgentEvent::Error(with_auth_hint(&message)));
                    }
                    "session.idle" => {
                        let _ = stop_tx.send(stop);
                        return;
                    }
                    _ => {}
                }
            }
        }
        let _ = stop_tx.send(stop);
    }

    /// Reply to a pending permission request.
    pub async fn reply_permission(&self, request_id: &str, reply: &str) -> Result<()> {
        let base = self
            .base_url
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("opencode server not connected"))?;
        let session = self
            .session_id
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("no opencode session"))?;
        let reply = match reply {
            "AllowAlways" | "AllowSession" => "always",
            "Allow" => "once",
            _ => "reject",
        };
        let resp = self
            .client
            .post(format!("{base}/session/{session}/permissions/{request_id}"))
            .json(&json!({ "response": reply }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("permission reply failed (HTTP {status}): {body}");
        }
        Ok(())
    }

    /// Interrupt the running turn.
    pub async fn interrupt(&self) {
        let base = self.base_url.read().await.clone();
        let session = self.session_id.lock().await.clone();
        if let (Some(base), Some(session)) = (base, session) {
            let _ = self
                .client
                .post(format!("{base}/session/{session}/abort"))
                .send()
                .await;
        }
    }
}

/// opencode reports credential problems verbatim; add the fix to the message.
pub fn with_auth_hint(message: &str) -> String {
    let lower = message.to_lowercase();
    let expired = lower.contains("api_key_expired")
        || lower.contains("api key has expired")
        || lower.contains("authentication_error")
        || lower.contains("statuscode\":401");
    if !expired {
        return message.to_string();
    }
    format!(
        "{message}\n\nopencode: this credential expired — run `opencode auth login` to renew it, \
         or pick another model with /model (for example opencode/big-pickle)."
    )
}

/// Percent-encode a path for a query string.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `provider/model` -> the shape the opencode API expects.
fn parse_model_ref(model: &str) -> Option<Value> {
    let (provider, id) = model.split_once('/')?;
    Some(json!({ "providerID": provider, "modelID": id }))
}

fn render_tool_result(result: &Value) -> String {
    match result {
        Value::String(s) => truncate_line(s),
        Value::Null => String::new(),
        other => {
            if let Some(text) = other.get("text").and_then(|v| v.as_str()) {
                truncate_line(text)
            } else {
                truncate_line(&other.to_string())
            }
        }
    }
}

fn summarize_tool_input(input: &Value) -> String {
    for key in [
        "command",
        "description",
        "filePath",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
    ] {
        if let Some(value) = input.get(key).and_then(|v| v.as_str()) {
            let value: String = value.chars().take(60).collect();
            if !value.trim().is_empty() {
                return value;
            }
        }
    }
    String::new()
}

fn truncate_line(s: &str) -> String {
    let first = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let truncated: String = first.chars().take(140).collect();
    truncated
}

#[async_trait]
impl AgentSession for OpencodeBackend {
    async fn prompt(&self, text: String) -> Result<StopReason> {
        let started = std::time::Instant::now();
        let result = match self.self_arc() {
            Some(arc) => arc.prompt(text).await,
            None => Err(anyhow!("opencode backend is not shared")),
        };
        let stop_reason = match &result {
            Ok(reason) => *reason,
            Err(_) => StopReason::Error,
        };
        let _ = self.tx.send(AgentEvent::TurnEnd {
            stop_reason,
            duration_secs: started.elapsed().as_secs_f64(),
        });
        result
    }
    fn cancel(&self) {
        if let Ok(guard) = self.cancel.try_lock() {
            if let Some(token) = guard.as_ref() {
                token.cancel();
            }
        }
    }
    fn session_id(&self) -> String {
        self.session_id
            .try_lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default()
    }
    async fn backend_session(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }
    async fn rewind_backend_session(&self, turns: usize) -> bool {
        // opencode keeps the message ids: revert to the first turn to drop.
        if turns == 0 {
            return false;
        }
        let Some(id) = self.session_id.lock().await.clone() else {
            return false;
        };
        let Ok(base) = self.ensure_server().await else {
            return false;
        };
        let Ok(resp) = self
            .client
            .get(format!("{base}/session/{id}/message"))
            .send()
            .await
        else {
            return false;
        };
        if !resp.status().is_success() {
            return false;
        }
        let value: Value = resp.json().await.unwrap_or(json!([]));
        let items = value.as_array().cloned().unwrap_or_default();
        let users: Vec<String> = items
            .iter()
            .filter_map(|item| {
                let info = item.get("info").unwrap_or(item);
                if info.get("role").and_then(|v| v.as_str()) != Some("user") {
                    return None;
                }
                info.get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        if users.is_empty() {
            return false;
        }
        let keep = users.len().saturating_sub(turns).min(users.len() - 1);
        let Some(target) = users.get(keep).cloned() else {
            return false;
        };
        self.client
            .post(format!("{base}/session/{id}/revert"))
            .json(&json!({ "messageID": target }))
            .send()
            .await
            .map(|resp| resp.status().is_success())
            .unwrap_or(false)
    }

    async fn fork_backend_session(&self) -> bool {
        // opencode can branch a session: POST /session/{id}/fork.
        let Some(id) = self.session_id.lock().await.clone() else {
            return false;
        };
        let Ok(base) = self.ensure_server().await else {
            return false;
        };
        let Ok(resp) = self
            .client
            .post(format!("{base}/session/{id}/fork"))
            .send()
            .await
        else {
            return false;
        };
        if !resp.status().is_success() {
            return false;
        }
        let value: Value = resp.json().await.unwrap_or(json!({}));
        let session = value
            .get("data")
            .or_else(|| value.get("session"))
            .cloned()
            .unwrap_or(value);
        let new_id = session
            .get("id")
            .or_else(|| session.get("sessionID"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        match new_id {
            Some(new_id) => {
                *self.session_id.lock().await = Some(new_id);
                true
            }
            None => false,
        }
    }
    async fn reset_backend_session(&self) {
        // Drop the id: the next turn creates a fresh opencode session.
        *self.session_id.lock().await = None;
    }
    async fn resume_backend_session(&self, id: &str) -> bool {
        // Only adopt a session the server still knows: a stale id would fail
        // every turn instead of starting a new conversation.
        let Ok(base) = self.ensure_server().await else {
            return false;
        };
        let known = self
            .client
            .get(format!("{base}/session/{id}"))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map(|resp| resp.status().is_success())
            .unwrap_or(false);
        if !known {
            return false;
        }
        *self.session_id.lock().await = Some(id.to_string());
        true
    }
    fn model_label(&self) -> String {
        self.label
            .try_read()
            .map(|l| l.clone())
            .unwrap_or_else(|_| "opencode".into())
    }
    fn context_usage(&self) -> (u64, u64) {
        self.context.try_read().map(|c| *c).unwrap_or((0, 0))
    }
    async fn shutdown(&self) {
        self.cancel();
        self.interrupt().await;
        if let Some(mut child) = self.server_child.lock().await.take() {
            let _ = child.kill().await;
        }
    }
}
