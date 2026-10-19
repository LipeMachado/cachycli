//! `cachycli agent serve`: a WebSocket server other clients can drive.
//!
//! One agent per connection, in the server's working directory:
//!
//! ```text
//! client → {"prompt": "run the tests"}      (or a plain text frame)
//!        → {"cancel": true}                 stops the running turn
//! server → {"type":"text","data":"…"}       streamed answer
//!        → {"type":"thought","data":"…"}    reasoning
//!        → {"type":"tool","name":"bash","title":"bash git status"}
//!        → {"type":"done","stop":"end_turn"}
//!        → {"type":"error","message":"…"}
//! ```
//!
//! Nothing listens on the network by default: you run it, you choose the host
//! and port (the default binds localhost).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use cachy_core::acp::server::AcpServer;
use cachy_core::agent::{AgentEvent, AgentSession, NativeAgent, TurnOptions};
use cachy_core::config::Config;
use cachy_core::message::StopReason;
use cachy_core::permissions::{PermissionEngine, PermissionMode, RuleSet};
use cachy_core::session::SessionStore;
use cachy_core::share::ShareStore;
use cachy_core::tools::ToolRegistry;
use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

/// Bind `host:port` and serve until the process ends.
pub async fn serve(host: &str, port: u16, cwd: PathBuf, config: Arc<Config>) -> Result<()> {
    let listener = TcpListener::bind((host, port))
        .await
        .with_context(|| format!("binding {host}:{port}"))?;
    let local = listener.local_addr()?;
    println!(
        "cachycli agent listening on ws://{local}  (directory: {})",
        cwd.display()
    );
    println!(
        "send {{\"prompt\": \"…\"}} and read the events back; {{\"cancel\": true}} stops a turn"
    );
    serve_listener(listener, cwd, config).await
}

/// Serve an already-bound listener (the tests use this with port 0).
pub async fn serve_listener(
    listener: TcpListener,
    cwd: PathBuf,
    config: Arc<Config>,
) -> Result<()> {
    serve_listener_at(listener, cwd, config, ShareStore::default_dir()).await
}

/// The same server with the share directory spelled out, so tests (and anyone
/// keeping shares somewhere else) can point it away from `~/.cachy/shares`.
pub async fn serve_listener_at(
    listener: TcpListener,
    cwd: PathBuf,
    config: Arc<Config>,
    shares: PathBuf,
) -> Result<()> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("accept failed: {e}");
                continue;
            }
        };
        let cwd = cwd.clone();
        let config = config.clone();
        let shares = shares.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, cwd, config, shares).await {
                eprintln!("connection from {peer} ended: {e}");
            }
        });
    }
}

async fn handle(
    stream: TcpStream,
    cwd: PathBuf,
    config: Arc<Config>,
    shares: PathBuf,
) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .context("websocket handshake")?;
    let (mut sink, mut incoming) = ws.split();

    // The first frame decides the dialect: a JSON-RPC `method` means the client
    // speaks ACP (editors), anything else is the simple prompt/event protocol.
    let mut first: Option<String> = None;
    while let Some(message) = incoming.next().await {
        match message? {
            Message::Text(text) => {
                first = Some(text.to_string());
                break;
            }
            Message::Ping(payload) => {
                let _ = sink.send(Message::Pong(payload)).await;
            }
            Message::Close(_) => return Ok(()),
            _ => {}
        }
    }
    let Some(first) = first else { return Ok(()) };
    let speaks_acp = serde_json::from_str::<serde_json::Value>(&first)
        .ok()
        .map(|value| value.get("method").is_some())
        .unwrap_or(false);
    if speaks_acp {
        return serve_acp(sink, incoming, first, cwd, config).await;
    }

    let model = cachy_core::models::resolve(&config, None)
        .ok_or_else(|| anyhow::anyhow!("no model configured for the server"))?;
    let session = SessionStore::create(&cwd, &model.model_id, "ws", None)?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel::<StopReason>();
    let agent = NativeAgent::new(
        cwd.clone(),
        config.clone(),
        model,
        session,
        Arc::new(cachy_core::mcp::McpManager::new(Default::default(), "ws")),
        ToolRegistry::default_registry(&config),
        PermissionEngine::new(RuleSet::default(), PermissionMode::AcceptEdits, true),
        tx.clone(),
        100,
        false,
        0,
    )
    .await?;

    let mut busy = false;
    // The frame that told us this is not ACP is still a prompt.
    if !handle_simple(&first, &mut sink, &agent, &mut busy, &done_tx, &shares).await {
        agent.shutdown().await;
        return Ok(());
    }
    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else { break };
                if let Some(payload) = event_to_json(&event) {
                    if sink.send(Message::Text(payload.to_string())).await.is_err() {
                        break;
                    }
                }
            }
            stop = done_rx.recv() => {
                busy = false;
                let stop = stop.unwrap_or(StopReason::EndTurn);
                let _ = sink
                    .send(Message::Text(
                        serde_json::json!({"type":"done","stop": stop.as_acp()}).to_string(),
                    ))
                    .await;
            }
            message = incoming.next() => {
                let Some(message) = message else { break };
                match message? {
                    Message::Text(text) => {
                        if !handle_simple(
                            &text,
                            &mut sink,
                            &agent,
                            &mut busy,
                            &done_tx,
                            &shares,
                        )
                        .await
                        {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(payload) => {
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    _ => {}
                }
            }
        }
    }
    agent.shutdown().await;
    Ok(())
}

/// One client frame of the simple protocol. Returns false when the socket
/// should close.
async fn handle_simple(
    text: &str,
    sink: &mut SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
    agent: &Arc<NativeAgent>,
    busy: &mut bool,
    done_tx: &tokio::sync::mpsc::UnboundedSender<StopReason>,
    shares: &Path,
) -> bool {
    let value: serde_json::Value =
        serde_json::from_str(text).unwrap_or(serde_json::Value::String(text.to_string()));

    // Sessions can be handed over the socket, not only prompted: push a bundle,
    // fetch one back, or ask what the server is holding.
    let store = ShareStore::open(shares);
    if let Some(bundle) = value.get("share").and_then(|v| v.as_str()) {
        let reply = match store.put(bundle) {
            Ok(entry) => {
                let mut reply = serde_json::to_value(&entry).unwrap_or_default();
                if let Some(map) = reply.as_object_mut() {
                    map.insert("type".into(), serde_json::json!("shared"));
                }
                reply
            }
            Err(e) => serde_json::json!({ "type": "error", "message": e.to_string() }),
        };
        let _ = sink.send(Message::Text(reply.to_string())).await;
        return true;
    }
    if let Some(id) = value.get("fetch").and_then(|v| v.as_str()) {
        // An empty id means "the most recent one".
        let wanted = if id.trim().is_empty() {
            store.list().first().map(|entry| entry.id.clone())
        } else {
            Some(id.to_string())
        };
        let reply = match wanted {
            Some(id) => match store.get(&id) {
                Ok(data) => serde_json::json!({ "type": "bundle", "id": id, "data": data }),
                Err(e) => serde_json::json!({ "type": "error", "message": e.to_string() }),
            },
            None => serde_json::json!({
                "type": "error",
                "message": "this server has no shared sessions yet"
            }),
        };
        let _ = sink.send(Message::Text(reply.to_string())).await;
        return true;
    }
    if let Some(id) = value.get("unshare").and_then(|v| v.as_str()) {
        let reply = match store.remove(id) {
            Ok(()) => serde_json::json!({ "type": "unshared", "id": id }),
            Err(e) => serde_json::json!({ "type": "error", "message": e.to_string() }),
        };
        let _ = sink.send(Message::Text(reply.to_string())).await;
        return true;
    }
    if value.get("shares").is_some() {
        let reply = serde_json::json!({ "type": "shares", "items": store.list() });
        let _ = sink.send(Message::Text(reply.to_string())).await;
        return true;
    }
    if value
        .get("cancel")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        agent.cancel_current();
        return true;
    }
    let prompt = value
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| value.as_str().map(|s| s.to_string()));
    let Some(prompt) = prompt else {
        let _ = sink
            .send(Message::Text(
                serde_json::json!({"type":"error","message":"send {\"prompt\": \"…\"}"})
                    .to_string(),
            ))
            .await;
        return true;
    };
    if *busy {
        let _ = sink
            .send(Message::Text(
                serde_json::json!({
                    "type":"error",
                    "message":"a turn is already running; send {\"cancel\": true} first"
                })
                .to_string(),
            ))
            .await;
        return true;
    }
    *busy = true;
    let agent = agent.clone();
    let done_tx = done_tx.clone();
    tokio::spawn(async move {
        let result = agent
            .run_turn(
                prompt,
                TurnOptions {
                    max_turns: Some(100),
                    interactive: false,
                },
            )
            .await;
        let stop = match result {
            Ok(stop) => stop,
            Err(_) => StopReason::Error,
        };
        let _ = done_tx.send(stop);
    });
    true
}

/// ACP over the socket: the same server `agent stdio` runs, with the socket as
/// the transport, so editor clients can connect over WebSocket instead of pipes.
async fn serve_acp(
    mut sink: SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
    mut incoming: SplitStream<tokio_tungstenite::WebSocketStream<TcpStream>>,
    first: String,
    cwd: PathBuf,
    config: Arc<Config>,
) -> Result<()> {
    let mut server = AcpServer::new(config, None, true, cwd);
    let mut out_rx = server.take_out();
    let writer = tokio::spawn(async move {
        while let Some(value) = out_rx.recv().await {
            if sink.send(Message::Text(value.to_string())).await.is_err() {
                break;
            }
        }
    });

    server.handle_line(&first).await;
    while let Some(message) = incoming.next().await {
        match message {
            Ok(Message::Text(text)) => {
                if std::env::var("CACHY_ACP_DEBUG").is_ok() {
                    eprintln!("[acp-server] <- {}", &text[..text.len().min(160)]);
                }
                server.handle_line(&text).await
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = writer.await;
    Ok(())
}

/// JSON for the events a client cares about (`None` for the internal ones).
fn event_to_json(event: &AgentEvent) -> Option<serde_json::Value> {
    Some(match event {
        AgentEvent::TextDelta(text) => serde_json::json!({"type":"text","data":text}),
        AgentEvent::ReasoningDelta(text) => serde_json::json!({"type":"thought","data":text}),
        AgentEvent::ToolCall { name, title, .. } => {
            serde_json::json!({"type":"tool","name":name,"title":title})
        }
        AgentEvent::ToolCallUpdate {
            id,
            status,
            summary,
            ..
        } => {
            serde_json::json!({"type":"tool_update","id":id,"status":status,"summary":summary})
        }
        AgentEvent::Error(message) => serde_json::json!({"type":"error","message":message}),
        AgentEvent::Info(message) => serde_json::json!({"type":"info","message":message}),
        AgentEvent::Usage {
            context_used,
            context_size,
            output_tokens,
            ..
        } => json_usage(*context_used, *context_size, *output_tokens),
        _ => return None,
    })
}

fn json_usage(used: u64, size: u64, output: u64) -> serde_json::Value {
    serde_json::json!({
        "type": "usage",
        "context_used": used,
        "context_size": size,
        "output_tokens": output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same socket speaks ACP when the client starts with JSON-RPC.
    #[tokio::test]
    async fn serves_acp_over_websocket() {
        let config = Arc::new(cachy_core::config::Config::default());
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cwd = std::env::temp_dir();
        let before = sessions_before(&cwd);
        tokio::spawn(serve_listener(listener, cwd.clone(), config));

        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .expect("connect");
        ws.send(Message::Text(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}"#
                .into(),
        ))
        .await
        .unwrap();

        let mut saw_agent = false;
        let mut saw_session = false;
        for _ in 0..10 {
            let Ok(Some(Ok(message))) =
                tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await
            else {
                break;
            };
            let Message::Text(text) = message else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            if value.get("id").and_then(|v| v.as_i64()) == Some(1) {
                assert!(
                    value["result"]["agentInfo"]["name"] == "cachycli",
                    "initialize should answer with the agent info: {text}"
                );
                saw_agent = true;
                ws.send(Message::Text(
                    r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}"#
                        .into(),
                ))
                .await
                .unwrap();
            } else if value.get("id").and_then(|v| v.as_i64()) == Some(2) {
                assert!(
                    value["result"]["sessionId"].is_string(),
                    "session/new should answer with an id: {text}"
                );
                saw_session = true;
                break;
            }
        }
        assert!(saw_agent, "ACP initialize over the socket");
        assert!(saw_session, "ACP session/new over the socket");
        forget_new_sessions(&cwd, &before);
    }

    /// Sessions a server creates land under the cachy home. Remember what was
    /// there so the test can leave the directory exactly as it found it.
    fn sessions_before(cwd: &Path) -> Vec<PathBuf> {
        let mut before: Vec<PathBuf> = std::fs::read_dir(cachy_core::paths::sessions_dir_for(cwd))
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default();
        before.sort();
        before
    }

    fn forget_new_sessions(cwd: &Path, before: &[PathBuf]) {
        let dir = cachy_core::paths::sessions_dir_for(cwd);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !before.contains(&path) {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
        // Succeeds only when nothing else is using the directory.
        let _ = std::fs::remove_dir(&dir);
    }

    /// A full round trip against the mock provider: prompt in, events out.
    #[tokio::test]
    async fn serves_a_prompt_over_websocket() {
        // Needs the harness mock provider (http://127.0.0.1:41999/v1); without it
        // there is nothing to talk to, so the test says so and returns.
        if reqwest::Client::new()
            .get("http://127.0.0.1:41999/v1/models")
            .send()
            .await
            .is_err()
        {
            eprintln!("skipping: the mock provider is not running (see docs/HANDOFF.md)");
            return;
        }
        let mut config = cachy_core::config::Config::default();
        config.models.default = Some("mock-1".into());
        config.custom_models.insert(
            "mock-1".into(),
            cachy_core::config::ModelConfig {
                model: Some("mock-1".into()),
                base_url: Some("http://127.0.0.1:41999/v1".into()),
                ..Default::default()
            },
        );
        let config = Arc::new(config);
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cwd = std::env::temp_dir();
        let before = sessions_before(&cwd);
        tokio::spawn(serve_listener(listener, cwd.clone(), config));

        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .expect("connect");
        ws.send(Message::Text(r#"{"prompt":"diga ok"}"#.into()))
            .await
            .unwrap();
        let mut saw_text = false;
        let mut saw_done = false;
        for _ in 0..40 {
            let next = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;
            let Ok(Some(Ok(message))) = next else { break };
            let Message::Text(text) = message else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            match value.get("type").and_then(|v| v.as_str()) {
                Some("text") => saw_text = true,
                Some("done") => {
                    saw_done = true;
                    break;
                }
                Some("error") => panic!("server error: {text}"),
                _ => {}
            }
        }
        assert!(saw_text, "the answer should stream back");
        assert!(saw_done, "the turn should finish with done");
        forget_new_sessions(&cwd, &before);
    }

    /// Sessions travel over the same socket: push, list, fetch.
    #[tokio::test]
    async fn shares_a_session_over_the_socket() {
        let shares = std::env::temp_dir().join("cachy-share-net-test");
        let _ = std::fs::remove_dir_all(&shares);
        let config = Arc::new(cachy_core::config::Config::default());
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_listener_at(
            listener,
            std::env::temp_dir(),
            config,
            shares.clone(),
        ));
        let url = format!("ws://{addr}");

        // Built by hand, so the test never touches the real home directory.
        let bundle = serde_json::json!({
            "kind": "cachycli.session",
            "version": 1,
            "id": "01test-session",
            "title": "conversa de teste",
            "cwd": "/tmp",
            "model": "mock-1",
            "agent": "cachy",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "messages": [{"role": "user", "content": "oi"}]
        })
        .to_string();

        let entry = cachy_core::share_net::push(&url, &bundle)
            .await
            .expect("push");
        assert_eq!(entry.id, "01test-session");
        assert_eq!(entry.title, "conversa de teste");
        assert_eq!(entry.messages, 1);

        let list = cachy_core::share_net::list_remote(&url)
            .await
            .expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "01test-session");

        // An empty id asks for the most recent one.
        let fetched = cachy_core::share_net::fetch(&url, "").await.expect("fetch");
        assert!(
            fetched.contains("conversa de teste"),
            "the bundle should come back whole: {fetched}"
        );

        // The server can forget one again.
        let removed = cachy_core::share_net::remove(&url, &entry.id)
            .await
            .expect("remove");
        assert_eq!(removed, "01test-session");
        assert!(cachy_core::share_net::list_remote(&url)
            .await
            .unwrap()
            .is_empty());

        // Foreign json is refused with something readable.
        let error = cachy_core::share_net::push(&url, r#"{"kind":"other"}"#)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not a"), "got: {error}");

        let _ = std::fs::remove_dir_all(&shares);
    }

    /// The real ACP client (what `[agent.acp] url` uses) against this server.
    #[tokio::test]
    async fn acp_client_talks_to_the_websocket_server() {
        // Needs the harness mock provider (http://127.0.0.1:41999/v1).
        if reqwest::Client::new()
            .get("http://127.0.0.1:41999/v1/models")
            .send()
            .await
            .is_err()
        {
            eprintln!("skipping: the mock provider is not running (see docs/HANDOFF.md)");
            return;
        }
        let mut config = cachy_core::config::Config::default();
        config.models.default = Some("mock-1".into());
        config.custom_models.insert(
            "mock-1".into(),
            cachy_core::config::ModelConfig {
                model: Some("mock-1".into()),
                base_url: Some("http://127.0.0.1:41999/v1".into()),
                ..Default::default()
            },
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cwd = std::env::temp_dir();
        let before = sessions_before(&cwd);
        tokio::spawn(serve_listener(listener, cwd.clone(), Arc::new(config)));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let client = cachy_core::acp::client::AcpClient::connect(&format!("ws://{addr}"), tx)
            .await
            .expect("the client should connect and initialize");
        let session = client.new_session(&cwd).await.expect("session/new");
        assert!(!session.is_empty(), "session/new should return an id");

        let cancel = tokio_util::sync::CancellationToken::new();
        let prompt = client.prompt(
            vec![cachy_core::acp::ContentBlock::Text {
                text: "diga ok".into(),
            }],
            cancel,
        );
        tokio::pin!(prompt);
        let mut saw_text = false;
        loop {
            tokio::select! {
                result = &mut prompt => {
                    result.expect("the prompt should finish");
                    break;
                }
                event = rx.recv() => match event {
                    Some(AgentEvent::TextDelta(text)) | Some(AgentEvent::MessageComplete { text })
                        if !text.is_empty() => saw_text = true,
                    Some(_) => {}
                    None => break,
                },
            }
        }
        assert!(
            saw_text,
            "the remote agent's answer should stream back over the socket"
        );
        forget_new_sessions(&cwd, &before);
    }
}
