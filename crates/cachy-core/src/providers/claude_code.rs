//! Claude Code backend: drives the `claude` CLI in headless streaming mode.
//!
//! Transport: `claude -p <prompt> --output-format stream-json --verbose`.
//! Every line of stdout is a JSON event; we map them onto the same
//! `AgentEvent` stream the rest of the interface already understands.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::agent::{AgentEvent, AgentSession};
use crate::config::Config;
use crate::message::StopReason;

pub struct ClaudeCodeBackend {
    config: Arc<Config>,
    cwd: PathBuf,
    tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: Mutex<Option<String>>,
    cancel: Mutex<Option<CancellationToken>>,
    label: RwLock<String>,
    context: RwLock<(u64, u64)>,
    interactive: bool,
    /// Weak self handle, installed by `new`, used by the `AgentSession` impl.
    self_ref: std::sync::OnceLock<std::sync::Weak<Self>>,
}

impl ClaudeCodeBackend {
    pub fn new(
        config: Arc<Config>,
        cwd: PathBuf,
        tx: mpsc::UnboundedSender<AgentEvent>,
        interactive: bool,
    ) -> Arc<Self> {
        let arc = Arc::new(Self {
            config,
            cwd,
            tx,
            session_id: Mutex::new(None),
            cancel: Mutex::new(None),
            label: RwLock::new("Claude Code".into()),
            context: RwLock::new((0, 0)),
            interactive,
            self_ref: std::sync::OnceLock::new(),
        });
        let _ = arc.self_ref.set(Arc::downgrade(&arc));
        arc
    }

    fn shared(&self) -> Option<Arc<Self>> {
        self.self_ref.get().and_then(|weak| weak.upgrade())
    }

    /// The command to run, honouring `[agent.claude]` overrides.
    fn binary(&self) -> String {
        std::env::var("CACHY_CLAUDE_BIN").unwrap_or_else(|_| "claude".into())
    }

    pub async fn prompt(self: &Arc<Self>, text: String) -> Result<StopReason> {
        let token = CancellationToken::new();
        *self.cancel.lock().await = Some(token.clone());
        let _ = self.tx.send(AgentEvent::UserMessage { text: text.clone() });

        let mut cmd = tokio::process::Command::new(self.binary());
        cmd.arg("-p").arg(&text);
        cmd.args(["--output-format", "stream-json"]);
        cmd.args(["--verbose", "--include-partial-messages"]);
        if let Some(model) = &self.config.agent.claude.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(session) = self.session_id.lock().await.clone() {
            cmd.arg("--resume").arg(session);
        }
        if self.interactive && !self.config.agent.claude.always_approve {
            // let Claude Code decide; the CLI applies its own permission rules
        }
        if self.config.agent.claude.always_approve {
            cmd.args(["--permission-mode", "bypassPermissions"]);
        }
        cmd.current_dir(&self.cwd);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let message = format!(
                    "could not start `{}`: {e}. Install Claude Code (https://claude.com/code) or run /provider to connect something else.",
                    self.binary()
                );
                let _ = self.tx.send(AgentEvent::Error(message));
                return Ok(StopReason::Error);
            }
        };

        if let Some(stderr) = child.stderr.take() {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let lower = line.to_lowercase();
                    if lower.contains("error") || lower.contains("not logged") {
                        let _ = tx.send(AgentEvent::Info(format!("claude: {line}")));
                    }
                }
            });
        }

        let Some(stdout) = child.stdout.take() else {
            return Ok(StopReason::Error);
        };
        let mut lines = BufReader::new(stdout).lines();
        let mut stop = StopReason::EndTurn;
        let mut streamed_text = false;
        let mut saw_error = false;

        loop {
            let next = tokio::select! {
                _ = token.cancelled() => {
                    let _ = child.kill().await;
                    let _ = self.tx.send(AgentEvent::Info("stopped".into()));
                    return Ok(StopReason::Cancelled);
                }
                line = lines.next_line() => line,
            };
            let Ok(Some(line)) = next else { break };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<Value>(trimmed) else { continue };

            if let Some(session) = event.get("session_id").and_then(|v| v.as_str()) {
                let mut guard = self.session_id.lock().await;
                if guard.as_deref() != Some(session) {
                    *guard = Some(session.to_string());
                }
            }

            match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                "system" => {
                    if let Some(model) = event.get("model").and_then(|v| v.as_str()) {
                        *self.label.write().await = format!("Claude Code · {model}");
                    }
                }
                "stream_event" => {
                    let inner = event.get("event").cloned().unwrap_or(json!({}));
                    match inner.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                        "content_block_delta" => {
                            let delta = inner.get("delta").cloned().unwrap_or(json!({}));
                            match delta.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                                "text_delta" => {
                                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                        streamed_text = true;
                                        let _ = self.tx.send(AgentEvent::TextDelta(text.to_string()));
                                    }
                                }
                                "thinking_delta" => {
                                    if let Some(text) = delta.get("thinking").and_then(|v| v.as_str()) {
                                        let _ = self.tx.send(AgentEvent::ReasoningDelta(text.to_string()));
                                    }
                                }
                                _ => {}
                            }
                        }
                        "content_block_start" => {
                            let block = inner.get("content_block").cloned().unwrap_or(json!({}));
                            if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("call").to_string();
                                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("tool").to_string();
                                let _ = self.tx.send(AgentEvent::ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    title: name,
                                    kind: "tool".into(),
                                    arguments: json!({}),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                "assistant" => {
                    let message = event.get("message").cloned().unwrap_or(json!({}));
                    if let Some(blocks) = message.get("content").and_then(|v| v.as_array()) {
                        let mut text = String::new();
                        for block in blocks {
                            match block.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                                "text" => {
                                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                        text.push_str(t);
                                    }
                                }
                                "tool_use" => {
                                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("call").to_string();
                                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("tool").to_string();
                                    let input = block.get("input").cloned().unwrap_or(json!({}));
                                    let title = tool_title(&name, &input);
                                    let _ = self.tx.send(AgentEvent::ToolCall {
                                        id,
                                        name,
                                        title,
                                        kind: "tool".into(),
                                        arguments: input,
                                    });
                                }
                                _ => {}
                            }
                        }
                        if !text.is_empty() && !streamed_text {
                            let _ = self.tx.send(AgentEvent::TextDelta(text));
                        }
                    }
                    if let Some(usage) = message.get("usage") {
                        let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let cache = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let mut ctx = self.context.write().await;
                        ctx.0 = ctx.0.max(input + cache);
                        ctx.1 = ctx.1.max(200_000);
                        let _ = self.tx.send(AgentEvent::Usage {
                            input_tokens: input,
                            output_tokens: output,
                            context_used: ctx.0,
                            context_size: ctx.1,
                        });
                    }
                }
                "user" => {
                    let message = event.get("message").cloned().unwrap_or(json!({}));
                    if let Some(blocks) = message.get("content").and_then(|v| v.as_array()) {
                        for block in blocks {
                            if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                                let id = block
                                    .get("tool_use_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("call")
                                    .to_string();
                                let failed = block.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                                let summary = summarize_tool_result(block.get("content"));
                                let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                                    id,
                                    status: if failed { "failed".into() } else { "completed".into() },
                                    title: None,
                                    summary: Some(summary),
                                    meta: None,
                                });
                            }
                        }
                    }
                }
                "result" => {
                    let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or("success");
                    if subtype != "success" {
                        saw_error = true;
                        stop = StopReason::Error;
                    }
                    if let Some(result) = event.get("result").and_then(|v| v.as_str()) {
                        if !streamed_text && !result.is_empty() {
                            let _ = self.tx.send(AgentEvent::TextDelta(result.to_string()));
                        }
                    }
                    if let Some(cost) = event.get("total_cost_usd").and_then(|v| v.as_f64()) {
                        let _ = self.tx.send(AgentEvent::Info(format!("claude-code · ${cost:.4}")));
                    }
                    if event.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false) {
                        saw_error = true;
                        stop = StopReason::Error;
                    }
                }
                _ => {}
            }
        }

        let status = child.wait().await;
        let _ = self.cancel.lock().await.take();
        if let Ok(status) = status {
            if !status.success() && !saw_error && stop == StopReason::EndTurn {
                // Claude exits non-zero on cancellation and some refusals.
                stop = StopReason::EndTurn;
            }
        }
        Ok(stop)
    }

    pub fn cancel_current(&self) {
        if let Ok(guard) = self.cancel.try_lock() {
            if let Some(token) = guard.as_ref() {
                token.cancel();
            }
        }
    }
}

fn tool_title(name: &str, input: &Value) -> String {
    for key in ["command", "file_path", "path", "pattern", "url", "query", "description"] {
        if let Some(value) = input.get(key).and_then(|v| v.as_str()) {
            let short: String = value.lines().next().unwrap_or("").chars().take(60).collect();
            if !short.trim().is_empty() {
                return format!("{name} {short}");
            }
        }
    }
    name.to_string()
}

fn summarize_tool_result(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => first_line(s),
        Some(Value::Array(items)) => {
            let mut out = String::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    out.push_str(text);
                    out.push('\n');
                }
            }
            first_line(&out)
        }
        Some(other) => first_line(&other.to_string()),
        None => String::new(),
    }
}

fn first_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    line.chars().take(160).collect()
}

#[async_trait]
impl AgentSession for ClaudeCodeBackend {
    async fn prompt(&self, text: String) -> Result<StopReason> {
        let started = std::time::Instant::now();
        let result = match self.shared() {
            Some(arc) => arc.prompt(text).await,
            None => Err(anyhow!("claude-code backend is not shared")),
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
        self.cancel_current();
    }
    fn session_id(&self) -> String {
        self.session_id.try_lock().ok().and_then(|g| g.clone()).unwrap_or_default()
    }
    async fn backend_session(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }
    async fn reset_backend_session(&self) {
        *self.session_id.lock().await = None;
    }
    async fn resume_backend_session(&self, id: &str) -> bool {
        // `claude --resume <id>` takes it from here; if the id is unknown to
        // claude the turn fails with its own message, which is the honest
        // answer.
        *self.session_id.lock().await = Some(id.to_string());
        true
    }
    fn model_label(&self) -> String {
        self.label.try_read().map(|l| l.clone()).unwrap_or_else(|_| "Claude Code".into())
    }
    fn context_usage(&self) -> (u64, u64) {
        self.context.try_read().map(|c| *c).unwrap_or((0, 0))
    }
    async fn shutdown(&self) {
        self.cancel_current();
    }
}
