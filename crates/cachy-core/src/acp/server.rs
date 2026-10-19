//! ACP agent server: `cachycli agent stdio`.
//!
//! Exposes the native cachycli agent over the Agent Client Protocol so IDEs
//! (Zed, Neovim, Emacs) and other ACP clients can connect to it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

use super::{content_text, PROTOCOL_VERSION};
use crate::agent::{AgentEvent, ApprovalDecision, NativeAgent, TurnOptions};
use crate::config::Config;
use crate::mcp::McpManager;
use crate::models;
use crate::permissions::{ApprovalChoice, PermissionEngine, RuleSet};
use crate::session::{SessionStore, SessionUpdate};
use crate::tools::ToolRegistry;

/// Run the ACP server loop on stdin/stdout.
/// The ACP server, independent of the transport that carries its messages.
///
/// `run_stdio` drives it with stdin/stdout; `agent serve` drives the same
/// server over a WebSocket, so editor clients can connect either way.
pub struct AcpServer {
    state: Arc<ServerState>,
    out_rx: mpsc::UnboundedReceiver<Value>,
}

impl AcpServer {
    pub fn new(
        config: Arc<Config>,
        model_override: Option<String>,
        always_approve: bool,
        cwd: PathBuf,
    ) -> Self {
        let (out_tx, out_rx) = mpsc::unbounded_channel::<Value>();
        Self {
            state: Arc::new(ServerState {
                config,
                cwd,
                model_override,
                always_approve,
                out: out_tx,
                sessions: Mutex::new(HashMap::new()),
                next_id: AtomicI64::new(1),
                pending_outbound: Mutex::new(HashMap::new()),
                client_caps: Mutex::new(json!({})),
            }),
            out_rx,
        }
    }

    /// Feed one incoming JSON-RPC line (already trimmed).
    pub async fn handle_line(&self, line: &str) {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(result) = self.state.handle_incoming(message).await {
            let _ = self.state.out.send(result);
        }
    }

    /// Take the outgoing receiver, so a transport (a socket, a pipe) can own
    /// the writing side while the reader keeps feeding `handle_line`.
    pub fn take_out(&mut self) -> mpsc::UnboundedReceiver<Value> {
        std::mem::replace(&mut self.out_rx, mpsc::unbounded_channel().1)
    }
}

pub async fn run_stdio(
    config: Arc<Config>,
    model_override: Option<String>,
    always_approve: bool,
    cwd: PathBuf,
) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut server = AcpServer::new(config, model_override, always_approve, cwd);

    // Writer task.
    let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = line_rx.recv().await {
            if stdout.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    // Drain the server's outgoing messages into the writer.
    let drain = {
        let mut out = std::mem::replace(&mut server.out_rx, mpsc::unbounded_channel().1);
        let line_tx = line_tx.clone();
        tokio::spawn(async move {
            while let Some(value) = out.recv().await {
                let mut line = serde_json::to_string(&value).unwrap_or_default();
                line.push('\n');
                if line_tx.send(line).is_err() {
                    break;
                }
            }
        })
    };

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        server.handle_line(trimmed).await;
    }
    drop(line_tx);
    let _ = drain.await;
    let _ = writer.await;
    Ok(())
}

struct ServerState {
    config: Arc<Config>,
    cwd: PathBuf,
    model_override: Option<String>,
    always_approve: bool,
    out: mpsc::UnboundedSender<Value>,
    sessions: Mutex<HashMap<String, Arc<AcpSession>>>,
    next_id: AtomicI64,
    /// Pending client responses for requests we sent (permissions).
    pending_outbound: Mutex<HashMap<i64, oneshot::Sender<Value>>>,
    client_caps: Mutex<Value>,
}

struct AcpSession {
    agent: Arc<NativeAgent>,
    /// Held so the forwarding task keeps running for this session.
    _events_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ServerState {
    async fn handle_incoming(self: &Arc<Self>, message: Value) -> Option<Value> {
        let method = message.get("method").and_then(|v| v.as_str());
        // Response to one of our outbound requests.
        if method.is_none() {
            if let Some(id) = message.get("id").and_then(|v| v.as_i64()) {
                if let Some(tx) = self.pending_outbound.lock().await.remove(&id) {
                    let _ = tx.send(message);
                }
            }
            return None;
        }
        let method = method.unwrap_or("");
        let id = message.get("id").cloned();
        if id.is_none() {
            // Notification from client (session/cancel).
            if method == "session/cancel" {
                if let Some(session_id) = message
                    .get("params")
                    .and_then(|p| p.get("sessionId"))
                    .and_then(|v| v.as_str())
                {
                    if let Some(session) = self.sessions.lock().await.get(session_id).cloned() {
                        session.agent.cancel_current();
                    }
                }
            }
            return None;
        }
        let id = id.unwrap();
        let params = message.get("params").cloned().unwrap_or(json!({}));
        let result = self.dispatch(method, params).await;
        Some(match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
            Err(e) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32603, "message": e.to_string() }
            }),
        })
    }

    async fn dispatch(self: &Arc<Self>, method: &str, params: Value) -> Result<Value> {
        match method {
            "initialize" => {
                *self.client_caps.lock().await = params
                    .get("clientCapabilities")
                    .cloned()
                    .unwrap_or(json!({}));
                Ok(json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "agentCapabilities": {
                        "loadSession": true,
                        "promptCapabilities": { "image": false, "audio": false, "embeddedContext": true },
                        "mcpCapabilities": { "http": true, "sse": false }
                    },
                    "agentInfo": {
                        "name": crate::APP_NAME,
                        "title": crate::APP_TITLE,
                        "version": crate::VERSION
                    },
                    "authMethods": []
                }))
            }
            "authenticate" => Ok(json!({})),
            "session/new" => {
                let cwd = params
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.cwd.clone());
                let (session_id, session) = self.create_session(&cwd, None).await?;
                self.sessions
                    .lock()
                    .await
                    .insert(session_id.clone(), session);
                Ok(json!({ "sessionId": session_id }))
            }
            "session/load" => {
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing sessionId"))?
                    .to_string();
                let cwd = params
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.cwd.clone());
                let (_, session) = self.create_session(&cwd, Some(&session_id)).await?;
                // Replay history as updates.
                for update in session.agent.history.lock().await.clone() {
                    self.send_update(
                        &session_id,
                        json!({
                            "sessionUpdate": match update.role {
                                crate::message::Role::User => "user_message_chunk",
                                _ => "agent_message_chunk",
                            },
                            "content": { "type": "text", "text": update.content }
                        }),
                    );
                }
                self.sessions
                    .lock()
                    .await
                    .insert(session_id.clone(), session);
                Ok(json!({}))
            }
            "session/prompt" => {
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing sessionId"))?
                    .to_string();
                let prompt_blocks = params.get("prompt").cloned().unwrap_or(json!([]));
                let text = prompt_to_text(&prompt_blocks);
                let session = self
                    .sessions
                    .lock()
                    .await
                    .get(&session_id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("unknown session {session_id}"))?;
                let stop = session
                    .agent
                    .run_turn(
                        text,
                        TurnOptions {
                            max_turns: None,
                            interactive: true,
                        },
                    )
                    .await;
                let stop = match stop {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = self.out.send(json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": { "sessionId": session_id, "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": format!("error: {e}") }
                            }}
                        }));
                        crate::message::StopReason::Error
                    }
                };
                Ok(json!({ "stopReason": stop.as_acp() }))
            }
            "session/delete" => Ok(json!({})),
            other => Err(anyhow::anyhow!("method not found: {other}")),
        }
    }

    async fn create_session(
        &self,
        cwd: &Path,
        resume: Option<&str>,
    ) -> Result<(String, Arc<AcpSession>)> {
        let model =
            models::resolve(&self.config, self.model_override.as_deref()).ok_or_else(|| {
                anyhow::anyhow!("no model configured; add [model.<name>] to config.toml")
            })?;
        let store = match resume {
            Some(id) => SessionStore::open(cwd, id).unwrap_or(SessionStore::create(
                cwd,
                &model.model_id,
                "cachy",
                None,
            )?),
            None => SessionStore::create(cwd, &model.model_id, "cachy", None)?,
        };
        let session_id = store.summary.id.clone();
        // Restore history when resuming.
        let history = if resume.is_some() {
            store.load_chat().unwrap_or_default()
        } else {
            Vec::new()
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let mcp = Arc::new(McpManager::new(
            self.config.mcp_servers.clone(),
            session_id.clone(),
        ));
        mcp.initialize().await;
        let tools = ToolRegistry::default_registry(&self.config);
        let engine = PermissionEngine::new(
            RuleSet::default(),
            crate::permissions::PermissionMode::Default,
            self.always_approve,
        );
        let agent = NativeAgent::new(
            cwd.to_path_buf(),
            self.config.clone(),
            model,
            store,
            mcp,
            tools,
            engine,
            tx,
            // ACP turns are driven by the client, so the loop limit is a
            // generous constant rather than the interactive default.
            50,
            true,
            0,
        )
        .await?;
        if !history.is_empty() {
            agent.load_history(history).await;
        }

        // Forward AgentEvents as ACP session updates.
        let this = Arc::new(self.clone_state());
        let session_id_for_task = session_id.clone();
        let agent_for_task = agent.clone();
        let events_task = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                this.forward_event(&session_id_for_task, event, &agent_for_task)
                    .await;
            }
        });

        Ok((
            session_id,
            Arc::new(AcpSession {
                agent,
                _events_task: Mutex::new(Some(events_task)),
            }),
        ))
    }

    fn clone_state(&self) -> Self {
        Self {
            config: self.config.clone(),
            cwd: self.cwd.clone(),
            model_override: self.model_override.clone(),
            always_approve: self.always_approve,
            out: self.out.clone(),
            sessions: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(self.next_id.load(Ordering::SeqCst)),
            pending_outbound: Mutex::new(HashMap::new()),
            client_caps: Mutex::new(json!({})),
        }
    }

    fn send_update(&self, session_id: &str, update: Value) {
        let _ = self.out.send(json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "sessionId": session_id, "update": update }
        }));
    }

    async fn forward_event(
        self: &Arc<Self>,
        session_id: &str,
        event: AgentEvent,
        agent: &Arc<NativeAgent>,
    ) {
        match event {
            AgentEvent::TextDelta(text) => {
                self.send_update(
                    session_id,
                    json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } }),
                );
            }
            AgentEvent::ReasoningDelta(text) => {
                self.send_update(
                    session_id,
                    json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": text } }),
                );
            }
            AgentEvent::ToolCall {
                id,
                name,
                title,
                kind,
                arguments,
            } => {
                self.send_update(
                    session_id,
                    json!({
                        "sessionUpdate": "tool_call",
                        "toolCallId": id,
                        "title": title,
                        "kind": acp_tool_kind(&kind),
                        "status": "pending",
                        "rawInput": arguments,
                        "name": name,
                    }),
                );
            }
            AgentEvent::ToolCallUpdate {
                id,
                status,
                title,
                summary,
                meta,
            } => {
                let mut content = Vec::new();
                if let Some(summary) = summary {
                    content.push(json!({ "type": "content", "content": { "type": "text", "text": summary } }));
                }
                self.send_update(
                    session_id,
                    json!({
                        "sessionUpdate": "tool_call_update",
                        "toolCallId": id,
                        "status": status,
                        "title": title,
                        "content": if content.is_empty() { Value::Null } else { json!(content) },
                        "rawOutput": meta,
                    }),
                );
            }
            AgentEvent::Plan(entries) => {
                self.send_update(
                    session_id,
                    json!({
                        "sessionUpdate": "plan",
                        "entries": entries.iter().map(|e| json!({
                            "content": e.content, "status": e.status, "priority": e.priority
                        })).collect::<Vec<_>>()
                    }),
                );
            }
            AgentEvent::Todos(todos) => {
                // Express todos as a plan update too (clients render plans).
                self.send_update(
                    session_id,
                    json!({
                        "sessionUpdate": "plan",
                        "entries": todos.iter().map(|t| json!({
                            "content": t.content, "status": t.status, "priority": t.priority
                        })).collect::<Vec<_>>()
                    }),
                );
            }
            AgentEvent::Usage {
                input_tokens,
                output_tokens,
                context_used,
                context_size,
            } => {
                self.send_update(
                    session_id,
                    json!({
                        "sessionUpdate": "usage_update",
                        "used": context_used,
                        "size": context_size,
                        "cost": Value::Null,
                    }),
                );
                let _ = (input_tokens, output_tokens);
            }
            AgentEvent::PermissionRequest {
                id,
                tool,
                title,
                detail,
                options,
            } => {
                // Ask the ACP client for permission.
                let request_id = self.next_id.fetch_add(1, Ordering::SeqCst);
                let (tx, rx) = oneshot::channel();
                self.pending_outbound.lock().await.insert(request_id, tx);
                let option_values: Vec<Value> = options
                    .iter()
                    .enumerate()
                    .map(|(i, label)| {
                        json!({
                            "optionId": match i {
                                0 => "allow_always",
                                1 => "allow_always",
                                2 => "allow_once",
                                _ => "reject_once",
                            },
                            "name": label,
                            "kind": match i {
                                0 | 1 => "allow_always",
                                2 => "allow_once",
                                _ => "reject_once",
                            }
                        })
                    })
                    .collect();
                let _ = self.out.send(json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "method": "session/request_permission",
                    "params": {
                        "sessionId": session_id,
                        "toolCall": {
                            "toolCallId": id,
                            "title": title,
                            "kind": acp_tool_kind(&tool),
                            "rawInput": serde_json::from_str::<Value>(&detail).ok(),
                        },
                        "options": option_values,
                    }
                }));
                // Await the client's answer in the background; the turn keeps
                // waiting on the agent's approval channel.
                let agent = agent.clone();
                let call_id = id.clone();
                tokio::spawn(async move {
                    let outcome =
                        tokio::time::timeout(std::time::Duration::from_secs(600), rx).await;
                    let decision = match outcome {
                        Ok(Ok(response)) => {
                            let option = response
                                .get("result")
                                .and_then(|r| r.get("outcome"))
                                .and_then(|o| o.get("optionId"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("reject_once")
                                .to_string();
                            match option.as_str() {
                                "allow_always" => {
                                    ApprovalDecision::Choice(ApprovalChoice::AllowAlways)
                                }
                                "allow_once" => ApprovalDecision::Choice(ApprovalChoice::Allow),
                                _ => ApprovalDecision::Choice(ApprovalChoice::Reject),
                            }
                        }
                        _ => ApprovalDecision::Cancelled,
                    };
                    agent.resolve_approval(&call_id, decision).await;
                });
            }
            AgentEvent::Error(message) => {
                self.send_update(
                    session_id,
                    json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": format!("error: {message}") } }),
                );
            }
            AgentEvent::Started { .. }
            | AgentEvent::UserMessage { .. }
            | AgentEvent::MessageComplete { .. }
            | AgentEvent::ReasoningComplete { .. }
            | AgentEvent::TurnEnd { .. }
            | AgentEvent::BackgroundFinished { .. }
            | AgentEvent::Info(_) => {}
        }
    }
}

fn acp_tool_kind(kind: &str) -> &'static str {
    match kind {
        "bash" => "execute",
        "read" => "read",
        "edit" => "edit",
        "write" => "edit",
        "search" => "search",
        "web" => "fetch",
        _ => "other",
    }
}

fn prompt_to_text(blocks: &Value) -> String {
    let mut out = String::new();
    if let Some(list) = blocks.as_array() {
        for block in list {
            match block.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(text);
                    }
                }
                Some("resource_link") => {
                    if let Some(uri) = block.get("uri").and_then(|v| v.as_str()) {
                        out.push_str(&format!("\n[file: {uri}]"));
                    }
                }
                Some("resource") => {
                    let text = block
                        .get("resource")
                        .and_then(|r| r.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let uri = block
                        .get("resource")
                        .and_then(|r| r.get("uri"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !text.is_empty() {
                        out.push_str(&format!("\n<file uri=\"{uri}\">\n{text}\n</file>"));
                    }
                }
                _ => {
                    let text = content_text(block);
                    if !text.is_empty() {
                        out.push_str(&text);
                    }
                }
            }
        }
    } else if let Some(text) = blocks.as_str() {
        out = text.to_string();
    }
    out
}

/// Store an ACP session update into our persistence layer.
pub fn record_update(store: &mut SessionStore, update: &Value) {
    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ts = chrono::Utc::now().to_rfc3339();
    let _ = match kind {
        "agent_message_chunk" => store.append_update(&SessionUpdate::AgentMessage {
            text: content_text(update.get("content").unwrap_or(&Value::Null)),
            ts,
        }),
        "agent_thought_chunk" => store.append_update(&SessionUpdate::AgentThought {
            text: content_text(update.get("content").unwrap_or(&Value::Null)),
            ts,
        }),
        _ => Ok(()),
    };
}
