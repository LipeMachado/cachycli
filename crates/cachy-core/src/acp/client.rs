//! ACP client: spawn any ACP agent over stdio and drive sessions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use super::{content_text, ContentBlock, PROTOCOL_VERSION};
use crate::agent::{AgentEvent, AgentSession};
use crate::message::StopReason;
use crate::session::PlanEntry;
use crate::tools::TodoItem;

/// Where the ACP messages go: a child process or a WebSocket.
type AcpSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

enum AcpWriter {
    Stdio(Mutex<ChildStdin>),
    Socket(
        Mutex<futures_util::stream::SplitSink<AcpSocket, tokio_tungstenite::tungstenite::Message>>,
    ),
}

/// A live ACP agent connection.
pub struct AcpClient {
    child: Mutex<Option<Child>>,
    writer: AcpWriter,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>,
    /// Notification receiver wired to the UI event channel.
    tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: Mutex<Option<String>>,
    label: Mutex<String>,
    /// Request id counter for agent→client requests (permissions).
    permission_replies: Mutex<HashMap<String, mpsc::UnboundedSender<Value>>>,
    pub agent_name: Mutex<String>,
}

impl AcpClient {
    /// Spawn an ACP agent and complete the initialize handshake.
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &std::collections::BTreeMap<String, String>,
        cwd: &std::path::Path,
        tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<Arc<Self>> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.current_dir(cwd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning ACP agent: {command}"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

        let client = Arc::new(Self {
            child: Mutex::new(Some(child)),
            writer: AcpWriter::Stdio(Mutex::new(stdin)),
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            tx,
            session_id: Mutex::new(None),
            label: Mutex::new(format!("acp:{command}")),
            permission_replies: Mutex::new(HashMap::new()),
            agent_name: Mutex::new(String::new()),
        });

        // Reader task.
        {
            let this = client.clone();
            let reader = BufReader::new(stdout);
            tokio::spawn(async move { this.read_loop(reader).await });
        }

        // initialize
        let result = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": { "readTextFile": true, "writeTextFile": true },
                        "terminal": false
                    },
                    "clientInfo": {
                        "name": crate::APP_NAME,
                        "title": crate::APP_TITLE,
                        "version": crate::VERSION
                    }
                }),
            )
            .await?;
        if let Some(info) = result.get("agentInfo") {
            let name = info
                .get("title")
                .or_else(|| info.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !name.is_empty() {
                *client.agent_name.lock().await = name.to_string();
                *client.label.lock().await = name.to_string();
            }
        }
        if let Ok(url) = std::env::var("CACHY_ACP_DEBUG") {
            let _ = std::fs::write(url, format!("initialize: {result}"));
        }
        Ok(client)
    }

    async fn read_loop(self: Arc<Self>, mut reader: BufReader<ChildStdout>) {
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
            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };
            self.dispatch(message).await;
        }
        // Fail any pending requests.
        let mut pending = self.pending.lock().await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err("agent closed the connection".into()));
        }
    }

    async fn dispatch(self: &Arc<Self>, message: Value) {
        let method = message.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = message.get("id").cloned();
        if !method.is_empty() {
            if id.is_some() {
                // agent → client request
                let result = self
                    .handle_agent_request(
                        method,
                        message.get("params").cloned().unwrap_or(json!({})),
                    )
                    .await;
                let response = match result {
                    Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
                    Err(e) => {
                        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": e.to_string() } })
                    }
                };
                let _ = self.write_message(response).await;
            } else {
                // notification
                self.handle_notification(
                    method,
                    message.get("params").cloned().unwrap_or(json!({})),
                )
                .await;
            }
            return;
        }
        // response to one of our requests
        if let Some(id) = message.get("id").and_then(|v| v.as_i64()) {
            if let Some(tx) = self.pending.lock().await.remove(&id) {
                if let Some(error) = message.get("error") {
                    let _ = tx.send(Err(error.to_string()));
                } else {
                    let _ = tx.send(Ok(message.get("result").cloned().unwrap_or(Value::Null)));
                }
            }
        }
    }

    async fn handle_notification(&self, method: &str, params: Value) {
        if method != "session/update" {
            return;
        }
        let update = params.get("update").cloned().unwrap_or(json!({}));
        let kind = update
            .get("sessionUpdate")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match kind {
            "agent_message_chunk" => {
                let text = content_text(update.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    let _ = self.tx.send(AgentEvent::TextDelta(text));
                }
            }
            "agent_thought_chunk" => {
                let text = content_text(update.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    let _ = self.tx.send(AgentEvent::ReasoningDelta(text));
                }
            }
            "tool_call" => {
                let id = update
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = update
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let kind = update
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("other")
                    .to_string();
                let status = update
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending")
                    .to_string();
                let _ = self.tx.send(AgentEvent::ToolCall {
                    id: id.clone(),
                    name: title.clone(),
                    title,
                    kind,
                    arguments: update.get("rawInput").cloned().unwrap_or(json!({})),
                });
                let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                    id,
                    status,
                    title: None,
                    summary: None,
                    meta: None,
                });
            }
            "tool_call_update" => {
                let id = update
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let status = update
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("in_progress")
                    .to_string();
                let mut summary = String::new();
                if let Some(contents) = update.get("content").and_then(|v| v.as_array()) {
                    for item in contents {
                        let inner = item.get("content").unwrap_or(item);
                        summary.push_str(&content_text(inner));
                        summary.push('\n');
                    }
                }
                let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                    id,
                    status,
                    title: None,
                    summary: if summary.trim().is_empty() {
                        None
                    } else {
                        Some(summary.trim().to_string())
                    },
                    meta: None,
                });
            }
            "plan" => {
                let entries = update
                    .get("entries")
                    .and_then(|v| v.as_array())
                    .map(|list| {
                        list.iter()
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
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let _ = self.tx.send(AgentEvent::Plan(entries.clone()));
                let todos: Vec<TodoItem> = entries
                    .into_iter()
                    .map(|e| TodoItem {
                        content: e.content,
                        status: e.status,
                        priority: e.priority,
                    })
                    .collect();
                let _ = self.tx.send(AgentEvent::Todos(todos));
            }
            "usage_update" => {
                let used = update.get("used").and_then(|v| v.as_u64()).unwrap_or(0);
                let size = update.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                let _ = self.tx.send(AgentEvent::Usage {
                    input_tokens: used,
                    output_tokens: 0,
                    context_used: used,
                    context_size: size,
                });
            }
            "session_info_update" => {
                if let Some(title) = update.get("title").and_then(|v| v.as_str()) {
                    let _ = self.tx.send(AgentEvent::Info(format!("session: {title}")));
                }
            }
            _ => {}
        }
    }

    async fn handle_agent_request(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "session/request_permission" => {
                let tool_call = params.get("toolCall").cloned().unwrap_or(json!({}));
                let title = tool_call
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("a tool call")
                    .to_string();
                let id = tool_call
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let options = params
                    .get("options")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let option_labels: Vec<String> = options
                    .iter()
                    .map(|o| {
                        o.get("name")
                            .or_else(|| o.get("optionId"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("option")
                            .to_string()
                    })
                    .collect();
                let (reply_tx, mut reply_rx) = mpsc::unbounded_channel();
                self.permission_replies
                    .lock()
                    .await
                    .insert(id.clone(), reply_tx);
                let _ = self.tx.send(AgentEvent::PermissionRequest {
                    id: id.clone(),
                    tool: "acp".into(),
                    title,
                    detail: params
                        .get("toolCall")
                        .map(|t| t.to_string())
                        .unwrap_or_default(),
                    options: option_labels,
                });
                // Wait for the UI to select an option.
                let selected = reply_rx.recv().await;
                let option_id = selected
                    .and_then(|v| v.get("option_id").cloned())
                    .and_then(|v| v.as_str().map(str::to_string))
                    .or_else(|| {
                        options
                            .iter()
                            .find(|o| o.get("kind").and_then(|k| k.as_str()) == Some("allow_once"))
                            .and_then(|o| o.get("optionId"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "reject_once".into());
                Ok(json!({
                    "outcome": { "outcome": "selected", "optionId": option_id }
                }))
            }
            "fs/read_text_file" => {
                let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = tokio::fs::read_to_string(path).await?;
                Ok(json!({ "content": content }))
            }
            "fs/write_text_file" => {
                let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
                tokio::fs::write(path, content).await?;
                Ok(json!({}))
            }
            other => Err(anyhow!("method not supported: {other}")),
        }
    }

    /// Connect to a remote ACP agent over a WebSocket and initialize.
    pub async fn connect(url: &str, tx: mpsc::UnboundedSender<AgentEvent>) -> Result<Arc<Self>> {
        use futures_util::StreamExt;
        let (socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .with_context(|| format!("connecting to the ACP agent at {url}"))?;
        let (sink, stream) = socket.split();
        let client = Arc::new(Self {
            child: Mutex::new(None),
            writer: AcpWriter::Socket(Mutex::new(sink)),
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            tx,
            session_id: Mutex::new(None),
            label: Mutex::new(format!("acp:{url}")),
            permission_replies: Mutex::new(HashMap::new()),
            agent_name: Mutex::new(String::new()),
        });
        {
            let this = client.clone();
            let mut incoming = stream;
            tokio::spawn(async move {
                use futures_util::StreamExt;
                while let Some(Ok(message)) = incoming.next().await {
                    let text = match message {
                        tokio_tungstenite::tungstenite::Message::Text(text) => text.to_string(),
                        tokio_tungstenite::tungstenite::Message::Close(_) => break,
                        _ => continue,
                    };
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if std::env::var("CACHY_ACP_DEBUG").is_ok() {
                        eprintln!("[acp-client] <- {}", &trimmed[..trimmed.len().min(160)]);
                    }
                    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
                        continue;
                    };
                    this.dispatch(value).await;
                }
                let mut pending = this.pending.lock().await;
                for (_, tx) in pending.drain() {
                    let _ = tx.send(Err("the ACP agent closed the connection".into()));
                }
            });
        }
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&self) -> Result<()> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": { "readTextFile": true, "writeTextFile": true },
                        "terminal": false
                    },
                    "clientInfo": {
                        "name": crate::APP_NAME,
                        "title": crate::APP_TITLE,
                        "version": crate::VERSION
                    }
                }),
            )
            .await?;
        if let Some(info) = result.get("agentInfo") {
            let name = info
                .get("title")
                .or_else(|| info.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !name.is_empty() {
                *self.agent_name.lock().await = name.to_string();
                *self.label.lock().await = format!("acp:{name}");
            }
        }
        Ok(())
    }

    async fn write_message(&self, message: Value) -> Result<()> {
        match &self.writer {
            AcpWriter::Stdio(stdin) => {
                let mut line = serde_json::to_string(&message)?;
                line.push('\n');
                let mut stdin = stdin.lock().await;
                stdin.write_all(line.as_bytes()).await?;
                stdin.flush().await?;
                Ok(())
            }
            AcpWriter::Socket(socket) => {
                use futures_util::SinkExt;
                let text = serde_json::to_string(&message)?;
                if std::env::var("CACHY_ACP_DEBUG").is_ok() {
                    eprintln!("[acp-client] -> {}", &text[..text.len().min(160)]);
                }
                let mut socket = socket.lock().await;
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(text))
                    .await
                    .map_err(|e| anyhow!("sending to the ACP agent: {e}"))
            }
        }
    }

    /// Send a request and await the response.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.write_message(
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
        .await?;
        match rx.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(anyhow!("{e}")),
            Err(_) => Err(anyhow!("agent dropped the request")),
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write_message(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    /// Resolve a pending permission prompt with a selected option.
    pub async fn reply_permission(&self, request_id: &str, option_id: &str) {
        if let Some(tx) = self.permission_replies.lock().await.remove(request_id) {
            let _ = tx.send(json!({ "option_id": option_id }));
        }
    }

    /// Create a session for a working directory.
    pub async fn new_session(&self, cwd: &std::path::Path) -> Result<String> {
        let result = self
            .request(
                "session/new",
                json!({ "cwd": cwd.to_string_lossy(), "mcpServers": [] }),
            )
            .await?;
        let id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("agent returned no sessionId: {result}"))?
            .to_string();
        *self.session_id.lock().await = Some(id.clone());
        Ok(id)
    }

    /// Branch the current session (`session/fork`), returning the new id.
    pub async fn fork_session(&self, cwd: &std::path::Path) -> Result<String> {
        let session_id = self
            .session_id
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("no ACP session"))?;
        let result = self
            .request(
                "session/fork",
                json!({
                    "sessionId": session_id,
                    "cwd": cwd.to_string_lossy()
                }),
            )
            .await?;
        let id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("agent returned no sessionId: {result}"))?
            .to_string();
        *self.session_id.lock().await = Some(id.clone());
        Ok(id)
    }

    /// Load an existing session.
    pub async fn load_session(&self, session_id: &str, cwd: &std::path::Path) -> Result<()> {
        self.request(
            "session/load",
            json!({ "sessionId": session_id, "cwd": cwd.to_string_lossy(), "mcpServers": [] }),
        )
        .await?;
        *self.session_id.lock().await = Some(session_id.to_string());
        Ok(())
    }

    /// Send a prompt turn; returns the stop reason.
    pub async fn prompt(
        &self,
        blocks: Vec<ContentBlock>,
        cancel: CancellationToken,
    ) -> Result<StopReason> {
        let session_id = self
            .session_id
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("no ACP session"))?;
        let params = json!({ "sessionId": session_id, "prompt": blocks });
        let request = self.request("session/prompt", params);
        tokio::pin!(request);
        let result = tokio::select! {
            r = &mut request => r?,
            _ = cancel.cancelled() => {
                let _ = self.notify("session/cancel", json!({ "sessionId": session_id })).await;
                // Wait for the agent's cancelled response.
                let r = request.await?;
                r
            }
        };
        let stop = result
            .get("stopReason")
            .and_then(|v| v.as_str())
            .unwrap_or("end_turn");
        Ok(match stop {
            "max_tokens" => StopReason::MaxTokens,
            "max_turn_requests" => StopReason::MaxTurns,
            "cancelled" => StopReason::Cancelled,
            "refusal" => StopReason::Refusal,
            _ => StopReason::EndTurn,
        })
    }

    pub async fn cancel(&self) {
        if let Some(session_id) = self.session_id.lock().await.clone() {
            let _ = self
                .notify("session/cancel", json!({ "sessionId": session_id }))
                .await;
        }
    }

    pub async fn shutdown(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
    }
}

/// `AgentSession` adapter so the TUI can treat an ACP agent as a backend.
pub struct AcpBackend {
    pub client: Arc<AcpClient>,
    pub cwd: PathBuf,
    pub cancel: Mutex<Option<CancellationToken>>,
    pub context: Mutex<(u64, u64)>,
    /// Channel used to deliver the synthetic turn-end event.
    pub usage_tx: mpsc::UnboundedSender<AgentEvent>,
}

impl AcpBackend {
    /// Reach a remote ACP agent over a WebSocket (`cachycli agent serve`).
    pub async fn connect_url(
        url: &str,
        cwd: &std::path::Path,
        tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<Arc<Self>> {
        let client = AcpClient::connect(url, tx.clone()).await?;
        client.new_session(cwd).await?;
        Ok(Arc::new(Self {
            client,
            cwd: cwd.to_path_buf(),
            cancel: Mutex::new(None),
            context: Mutex::new((0, 0)),
            usage_tx: tx,
        }))
    }

    pub async fn connect(
        command: &str,
        args: &[String],
        env: &std::collections::BTreeMap<String, String>,
        cwd: &std::path::Path,
        tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<Arc<Self>> {
        let client = AcpClient::spawn(command, args, env, cwd, tx.clone()).await?;
        client.new_session(cwd).await?;
        Ok(Arc::new(Self {
            client,
            cwd: cwd.to_path_buf(),
            cancel: Mutex::new(None),
            context: Mutex::new((0, 0)),
            usage_tx: tx,
        }))
    }
}

#[async_trait]
impl AgentSession for AcpBackend {
    async fn prompt(&self, text: String) -> Result<StopReason> {
        let token = CancellationToken::new();
        *self.cancel.lock().await = Some(token.clone());
        let started = std::time::Instant::now();
        // ACP agents do not echo the user message; surface it locally.
        let _ = self
            .usage_tx
            .send(AgentEvent::UserMessage { text: text.clone() });
        let result = self
            .client
            .prompt(vec![ContentBlock::text(text)], token)
            .await;
        let _ = self.cancel.lock().await.take();
        let stop_reason = match &result {
            Ok(reason) => *reason,
            Err(_) => StopReason::Error,
        };
        // The ACP agent reports usage through session updates; forward a turn
        // end so the UI leaves the busy state.
        let _ = self.usage_tx.send(AgentEvent::TurnEnd {
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
        self.client
            .session_id
            .try_lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default()
    }
    async fn backend_session(&self) -> Option<String> {
        self.client.session_id.lock().await.clone()
    }
    async fn fork_backend_session(&self) -> bool {
        self.client.fork_session(&self.cwd).await.is_ok()
    }
    async fn reset_backend_session(&self) {
        // A fresh `session/new` on the agent's side.
        let _ = self.client.new_session(&self.cwd).await;
    }
    async fn resume_backend_session(&self, id: &str) -> bool {
        // `session/load` asks the agent to continue that session.
        self.client.load_session(id, &self.cwd).await.is_ok()
    }
    fn model_label(&self) -> String {
        self.client
            .agent_name
            .try_lock()
            .ok()
            .map(|g| g.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "acp agent".into())
    }
    fn context_usage(&self) -> (u64, u64) {
        self.context.try_lock().map(|c| *c).unwrap_or((0, 0))
    }
    async fn shutdown(&self) {
        self.cancel();
        self.client.shutdown().await;
    }
}

/// Permission reply helper used by the TUI for ACP-backed sessions.
pub async fn reply_acp_permission(
    client: &AcpClient,
    request_id: &str,
    choice: crate::permissions::ApprovalChoice,
) {
    let option = match choice {
        crate::permissions::ApprovalChoice::Allow => "allow_once",
        crate::permissions::ApprovalChoice::AllowAlways => "allow_always",
        crate::permissions::ApprovalChoice::AllowSession => "allow_always",
        crate::permissions::ApprovalChoice::Reject => "reject_once",
    };
    client.reply_permission(request_id, option).await;
}
