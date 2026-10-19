//! CLI-agent backends: drive another agent's own command line in its headless
//! mode and map what it prints onto the events the interface already knows.
//!
//! A *dialect* knows two things: how to build the command line, and how to read
//! the stream. Everything else — spawning, cancellation, session ids, usage —
//! is shared, so adding the next CLI is a match arm rather than a new backend.
//!
//! - `codex exec --json` prints one JSON object per line
//!   (`thread.started`, `item.completed`, `turn.completed`).
//! - `gemini -p --output-format stream-json` prints the same idea in its own
//!   vocabulary (`init`, `message`, `result`), which the reader accepts
//!   loosely: a field that is missing costs a detail, not the turn.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::agent::{AgentEvent, AgentSession};
use crate::config::Config;
use crate::message::StopReason;

/// Which command line we are driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Codex,
    Gemini,
}

impl Dialect {
    pub fn provider_name(&self) -> &'static str {
        match self {
            Dialect::Codex => "codex",
            Dialect::Gemini => "gemini",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Dialect::Codex => "Codex",
            Dialect::Gemini => "Gemini",
        }
    }

    /// The binary, unless `[agent.<name>] command` overrides it.
    fn binary(&self, config: &Config) -> String {
        let configured = match self {
            Dialect::Codex => &config.agent.codex.command,
            Dialect::Gemini => &config.agent.gemini.command,
        };
        configured.clone()
    }

    fn model(&self, config: &Config) -> Option<String> {
        match self {
            Dialect::Codex => config.agent.codex.model.clone(),
            Dialect::Gemini => config.agent.gemini.model.clone(),
        }
    }

    fn extra_args(&self, config: &Config) -> Vec<String> {
        match self {
            Dialect::Codex => config.agent.codex.args.clone(),
            Dialect::Gemini => config.agent.gemini.args.clone(),
        }
    }

    fn always_approve(&self, config: &Config) -> bool {
        match self {
            Dialect::Codex => config.agent.codex.always_approve,
            Dialect::Gemini => config.agent.gemini.always_approve,
        }
    }
}

pub struct CliAgentBackend {
    dialect: Dialect,
    config: Arc<Config>,
    cwd: PathBuf,
    tx: mpsc::UnboundedSender<AgentEvent>,
    /// The CLI's own session id, so a resumed conversation continues there.
    session_id: Mutex<Option<String>>,
    cancel: Mutex<Option<CancellationToken>>,
    label: RwLock<String>,
    context: RwLock<(u64, u64)>,
    interactive: bool,
    self_ref: std::sync::OnceLock<std::sync::Weak<Self>>,
}

impl CliAgentBackend {
    pub fn new(
        dialect: Dialect,
        config: Arc<Config>,
        cwd: PathBuf,
        tx: mpsc::UnboundedSender<AgentEvent>,
        interactive: bool,
    ) -> Arc<Self> {
        let arc = Arc::new(Self {
            dialect,
            config,
            cwd,
            tx,
            session_id: Mutex::new(None),
            cancel: Mutex::new(None),
            label: RwLock::new(dialect.label().to_string()),
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

    /// The command line for one turn.
    fn build_command(&self, text: &str, session: Option<&str>) -> tokio::process::Command {
        let config = &self.config;
        let binary = self.dialect.binary(config);
        let mut cmd = tokio::process::Command::new(&binary);
        match self.dialect {
            Dialect::Codex => {
                cmd.arg("exec");
                cmd.arg("--json");
                cmd.arg("--skip-git-repo-check");
                // The child cannot answer its own prompts here (stdin is
                // closed), so its gate is bypassed; its own rules can still be
                // turned on through [agent.codex] args.
                cmd.arg("--dangerously-bypass-approvals-and-sandbox");
                if let Some(model) = self.dialect.model(config) {
                    cmd.arg("--model").arg(model);
                }
                cmd.args(self.dialect.extra_args(config));
                if let Some(session) = session {
                    cmd.arg("resume").arg(session);
                }
                cmd.arg(text);
            }
            Dialect::Gemini => {
                if let Some(model) = self.dialect.model(config) {
                    cmd.arg("--model").arg(model);
                }
                cmd.args(["--output-format", "stream-json"]);
                let _ = self.dialect.always_approve(config);
                cmd.arg("--approval-mode").arg("yolo");
                cmd.args(self.dialect.extra_args(config));
                if let Some(session) = session {
                    cmd.arg("--resume").arg(session);
                }
                cmd.arg("-p").arg(text);
            }
        }
        cmd
    }

    /// Map one line of the CLI's stream onto our events. Returns true when the
    /// turn is over.
    async fn handle_line(&self, line: &str) -> bool {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        match self.dialect {
            Dialect::Codex => self.handle_codex(&event).await,
            Dialect::Gemini => self.handle_gemini(&event).await,
        }
    }

    async fn handle_codex(&self, event: &Value) -> bool {
        match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "thread.started" => {
                if let Some(id) = event.get("thread_id").and_then(|v| v.as_str()) {
                    *self.session_id.lock().await = Some(id.to_string());
                }
                false
            }
            "item.completed" | "item.started" => {
                let item = event.get("item").cloned().unwrap_or(json!({}));
                let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let text = item
                    .get("text")
                    .or_else(|| item.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match kind {
                    "agent_message" | "assistant_message" => {
                        if !text.is_empty() {
                            let _ = self.tx.send(AgentEvent::TextDelta(text.to_string()));
                        }
                    }
                    "reasoning" | "thought" => {
                        if !text.is_empty() {
                            let _ = self.tx.send(AgentEvent::ReasoningDelta(text.to_string()));
                        }
                    }
                    "command_execution" | "tool_call" | "local_shell_call" => {
                        let command = item
                            .get("command")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("name").and_then(|v| v.as_str()))
                            .unwrap_or("command");
                        let id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("call")
                            .to_string();
                        let title = format!("bash {}", first_line(command));
                        if event.get("type").and_then(|v| v.as_str()) == Some("item.completed") {
                            let failed = item
                                .get("exit_code")
                                .and_then(|v| v.as_i64())
                                .is_some_and(|code| code != 0);
                            let summary = item
                                .get("aggregated_output")
                                .and_then(|v| v.as_str())
                                .map(|out| first_line(out).to_string());
                            let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                                id,
                                status: if failed {
                                    "failed".into()
                                } else {
                                    "completed".into()
                                },
                                title: None,
                                summary,
                                meta: None,
                            });
                        } else {
                            let _ = self.tx.send(AgentEvent::ToolCall {
                                id,
                                name: "bash".into(),
                                title,
                                kind: "execute".into(),
                                arguments: json!({ "command": command }),
                            });
                        }
                    }
                    _ => {}
                }
                false
            }
            "turn.completed" => {
                if let Some(usage) = event.get("usage") {
                    let input = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let output = usage
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let cached = usage
                        .get("cached_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let mut ctx = self.context.write().await;
                    ctx.0 = ctx.0.max(input + cached);
                    let _ = self.tx.send(AgentEvent::Usage {
                        input_tokens: input,
                        output_tokens: output,
                        context_used: ctx.0,
                        context_size: ctx.1,
                    });
                }
                true
            }
            "error" | "turn.failed" => {
                let message = event
                    .get("message")
                    .or_else(|| event.get("error").and_then(|e| e.get("message")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("the agent reported an error");
                let _ = self.tx.send(AgentEvent::Error(message.to_string()));
                true
            }
            _ => false,
        }
    }

    async fn handle_gemini(&self, event: &Value) -> bool {
        match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "init" => {
                if let Some(id) = event
                    .get("session_id")
                    .or_else(|| event.get("sessionId"))
                    .and_then(|v| v.as_str())
                {
                    *self.session_id.lock().await = Some(id.to_string());
                }
                if let Some(model) = event.get("model").and_then(|v| v.as_str()) {
                    *self.label.write().await = format!("Gemini · {model}");
                }
                false
            }
            "message" => {
                let role = event
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("assistant");
                if role != "assistant" {
                    return false;
                }
                let text = text_of(event.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    let _ = self.tx.send(AgentEvent::TextDelta(text));
                }
                false
            }
            "tool_call" | "tool_use" => {
                let id = event
                    .get("id")
                    .or_else(|| event.get("tool_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("call")
                    .to_string();
                let name = event
                    .get("name")
                    .or_else(|| event.get("tool_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let arguments = event
                    .get("arguments")
                    .or_else(|| event.get("parameters"))
                    .or_else(|| event.get("input"))
                    .cloned()
                    .unwrap_or(json!({}));
                let title = format!("{} {}", name, first_line(&arguments.to_string()));
                let _ = self.tx.send(AgentEvent::ToolCall {
                    id,
                    name,
                    title,
                    kind: "tool".into(),
                    arguments,
                });
                false
            }
            "tool_result" => {
                let id = event
                    .get("id")
                    .or_else(|| event.get("tool_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("call")
                    .to_string();
                let failed = event
                    .get("status")
                    .and_then(|v| v.as_str())
                    .is_some_and(|status| status != "success");
                let summary = text_of(event.get("output").unwrap_or(&Value::Null));
                let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                    id,
                    status: if failed {
                        "failed".into()
                    } else {
                        "completed".into()
                    },
                    title: None,
                    summary: (!summary.is_empty()).then(|| first_line(&summary).to_string()),
                    meta: None,
                });
                false
            }
            "result" => {
                let failed = event
                    .get("status")
                    .and_then(|v| v.as_str())
                    .is_some_and(|status| status != "success");
                if let Some(stats) = event.get("stats") {
                    let input = stats
                        .get("input_tokens")
                        .or_else(|| stats.get("inputTokenCount"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let output = stats
                        .get("output_tokens")
                        .or_else(|| stats.get("outputTokenCount"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let mut ctx = self.context.write().await;
                    ctx.0 = ctx.0.max(input);
                    let _ = self.tx.send(AgentEvent::Usage {
                        input_tokens: input,
                        output_tokens: output,
                        context_used: ctx.0,
                        context_size: ctx.1,
                    });
                }
                if failed {
                    let message = event
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("the agent reported an error");
                    let _ = self.tx.send(AgentEvent::Error(message.to_string()));
                }
                true
            }
            "error" => {
                let message = event
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("the agent reported an error");
                let _ = self.tx.send(AgentEvent::Error(message.to_string()));
                true
            }
            _ => false,
        }
    }

    pub async fn prompt(self: &Arc<Self>, text: String) -> Result<StopReason> {
        let token = CancellationToken::new();
        *self.cancel.lock().await = Some(token.clone());
        let _ = self.tx.send(AgentEvent::UserMessage { text: text.clone() });

        let session = self.session_id.lock().await.clone();
        let mut cmd = self.build_command(&text, session.as_deref());
        cmd.current_dir(&self.cwd);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let package = match self.dialect {
                    Dialect::Codex => "@openai/codex",
                    Dialect::Gemini => "@google/gemini-cli",
                };
                let hint = if self.interactive {
                    format!("install it (npm i -g {package}) or run /provider to connect something else")
                } else {
                    format!(
                        "install it (npm i -g {package}) or set [agent.{}] command",
                        self.dialect.provider_name()
                    )
                };
                let message = format!(
                    "could not start `{}`: {e} — {hint}",
                    self.dialect.binary(&self.config)
                );
                let _ = self.tx.send(AgentEvent::Error(message));
                return Ok(StopReason::Error);
            }
        };

        // The CLIs report their trouble on stderr; anything that smells like a
        // failure is worth showing, the rest is noise.
        if let Some(stderr) = child.stderr.take() {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let lower = line.to_lowercase();
                    if lower.contains("error")
                        || lower.contains("not logged")
                        || lower.contains("unauthorized")
                        || lower.contains("api key")
                    {
                        let _ = tx.send(AgentEvent::Info(line.trim().to_string()));
                    }
                }
            });
        }

        let Some(stdout) = child.stdout.take() else {
            return Ok(StopReason::Error);
        };
        let mut lines = BufReader::new(stdout).lines();
        let mut stop = StopReason::EndTurn;
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
            let over = self.handle_line(trimmed).await;
            if over {
                if trimmed.contains("\"error\"") || trimmed.contains("\"failed\"") {
                    saw_error = true;
                }
                break;
            }
        }

        let status = child.wait().await;
        if let Ok(status) = status {
            if !status.success() {
                saw_error = true;
            }
        }
        if saw_error {
            stop = StopReason::Error;
        }
        let _ = self.tx.send(AgentEvent::TurnEnd {
            stop_reason: stop,
            duration_secs: 0.0,
        });
        Ok(stop)
    }
}

/// Text out of a string, or out of a list of `{text}` parts.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|part| match part {
                Value::String(text) => text.clone(),
                other => other
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let clipped: String = line.chars().take(80).collect();
    clipped
}

#[async_trait]
impl AgentSession for CliAgentBackend {
    async fn prompt(&self, text: String) -> Result<StopReason> {
        match self.shared() {
            Some(arc) => arc.prompt(text).await,
            None => Ok(StopReason::Error),
        }
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
            .and_then(|id| id.clone())
            .unwrap_or_default()
    }

    fn model_label(&self) -> String {
        self.label
            .try_read()
            .map(|label| label.clone())
            .unwrap_or_else(|_| self.dialect.label().to_string())
    }

    fn context_usage(&self) -> (u64, u64) {
        self.context.try_read().map(|c| *c).unwrap_or((0, 0))
    }

    async fn shutdown(&self) {
        self.cancel();
    }

    async fn backend_session(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    async fn resume_backend_session(&self, id: &str) -> bool {
        // Both CLIs take the id back on their own command line.
        *self.session_id.lock().await = Some(id.to_string());
        true
    }

    async fn reset_backend_session(&self) {
        *self.session_id.lock().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_text_from_the_shapes_the_clis_use() {
        assert_eq!(text_of(&json!("hello")), "hello");
        assert_eq!(text_of(&json!([{ "text": "a" }, { "text": "b" }])), "ab");
        assert_eq!(text_of(&json!({ "text": "c" })), "c");
        assert_eq!(text_of(&json!(null)), "");
    }

    #[test]
    fn first_line_is_short_and_single() {
        assert_eq!(first_line("one\ntwo"), "one");
        assert_eq!(first_line(&"x".repeat(200)).chars().count(), 80);
    }
}
