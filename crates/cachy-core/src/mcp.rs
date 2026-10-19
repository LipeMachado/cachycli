//! Minimal MCP client: stdio and streamable-HTTP transports, tool listing and
//! invocation, resource reads.
//!
//! Implements JSON-RPC 2.0 with newline-delimited framing (stdio) and HTTP
//! POST (Streamable HTTP). SSE responses are parsed when the server answers
//! with `text/event-stream`.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use crate::config::McpServerConfig;

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
pub const CLIENT_NAME: &str = "cachycli";

/// A tool exposed by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// stdio transport, guarded by a mutex for request/response sequencing.
struct StdioTransport {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    #[allow(dead_code)]
    child: Child,
}

/// HTTP transport with optional session id.
struct HttpTransport {
    url: String,
    headers: BTreeMap<String, String>,
    session_id: Mutex<Option<String>>,
    client: reqwest::Client,
}

/// One connected MCP server.
struct Connection {
    stdio: Option<Mutex<StdioTransport>>,
    http: Option<HttpTransport>,
    next_id: AtomicI64,
}

/// Manages all configured MCP servers.
pub struct McpManager {
    configs: BTreeMap<String, McpServerConfig>,
    connections: Mutex<BTreeMap<String, std::sync::Arc<Connection>>>,
    tools: Mutex<Vec<McpToolInfo>>,
    errors: Mutex<BTreeMap<String, String>>,
    session_id: String,
}

impl McpManager {
    pub fn new(configs: BTreeMap<String, McpServerConfig>, session_id: impl Into<String>) -> Self {
        Self {
            configs,
            connections: Mutex::new(BTreeMap::new()),
            tools: Mutex::new(Vec::new()),
            errors: Mutex::new(BTreeMap::new()),
            session_id: session_id.into(),
        }
    }

    pub fn empty() -> Self {
        Self::new(BTreeMap::new(), String::new())
    }

    pub fn server_configs(&self) -> &BTreeMap<String, McpServerConfig> {
        &self.configs
    }

    /// Connect to every enabled server and cache their tools.
    pub async fn initialize(&self) {
        for (name, cfg) in &self.configs {
            if !cfg.enabled {
                continue;
            }
            match self.connect(name, cfg).await {
                Ok(tools) => self.tools.lock().await.extend(tools),
                Err(e) => {
                    self.errors.lock().await.insert(name.clone(), e.to_string());
                }
            }
        }
    }

    async fn connect(&self, name: &str, cfg: &McpServerConfig) -> Result<Vec<McpToolInfo>> {
        let conn = if let Some(url) = &cfg.url {
            let url = url.replace("{{session_id}}", &self.session_id);
            let headers: BTreeMap<String, String> = cfg
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.replace("{{session_id}}", &self.session_id)))
                .collect();
            std::sync::Arc::new(Connection {
                stdio: None,
                http: Some(HttpTransport {
                    url,
                    headers,
                    session_id: Mutex::new(None),
                    client: reqwest::Client::builder()
                        .user_agent(crate::user_agent())
                        .build()?,
                }),
                next_id: AtomicI64::new(1),
            })
        } else {
            let command = cfg
                .command
                .clone()
                .ok_or_else(|| anyhow!("server {name} needs command or url"))?;
            let mut cmd = tokio::process::Command::new(&command);
            cmd.args(&cfg.args);
            for (k, v) in &cfg.env {
                cmd.env(k, v);
            }
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = cmd
                .spawn()
                .with_context(|| format!("starting MCP server {name} ({command})"))?;
            let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
            let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
            std::sync::Arc::new(Connection {
                stdio: Some(Mutex::new(StdioTransport {
                    stdin,
                    stdout: BufReader::new(stdout),
                    child,
                })),
                http: None,
                next_id: AtomicI64::new(1),
            })
        };

        let timeout = std::time::Duration::from_secs(cfg.startup_timeout_sec.max(1));
        // initialize
        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "roots": { "listChanged": false } },
            "clientInfo": { "name": CLIENT_NAME, "title": crate::APP_TITLE, "version": crate::VERSION }
        });
        tokio::time::timeout(timeout, request(&conn, "initialize", params))
            .await
            .map_err(|_| anyhow!("initialize timed out for {name}"))?
            .with_context(|| format!("initialize failed for {name}"))?;
        let _ = notify(&conn, "notifications/initialized", json!({})).await;

        let tools_result = tokio::time::timeout(timeout, request(&conn, "tools/list", json!({})))
            .await
            .map_err(|_| anyhow!("tools/list timed out for {name}"))?
            .with_context(|| format!("tools/list failed for {name}"))?;
        let mut tools = Vec::new();
        if let Some(list) = tools_result.get("tools").and_then(|v| v.as_array()) {
            for tool in list {
                tools.push(McpToolInfo {
                    server: name.to_string(),
                    name: tool
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: tool
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    input_schema: tool
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object" })),
                });
            }
        }
        self.connections.lock().await.insert(name.to_string(), conn);
        Ok(tools)
    }

    pub async fn list_tools(&self) -> Vec<McpToolInfo> {
        self.tools.lock().await.clone()
    }

    pub async fn errors(&self) -> BTreeMap<String, String> {
        self.errors.lock().await.clone()
    }

    async fn connection(&self, server: &str) -> Result<std::sync::Arc<Connection>> {
        if let Some(conn) = self.connections.lock().await.get(server) {
            return Ok(conn.clone());
        }
        let cfg = self
            .configs
            .get(server)
            .ok_or_else(|| anyhow!("unknown MCP server \"{server}\""))?
            .clone();
        let tools = self.connect(server, &cfg).await?;
        {
            let mut guard = self.tools.lock().await;
            guard.retain(|t| t.server != server);
            guard.extend(tools);
        }
        self.connections
            .lock()
            .await
            .get(server)
            .cloned()
            .ok_or_else(|| anyhow!("connection lost"))
    }

    pub async fn call_tool(&self, server: &str, tool: &str, arguments: Value) -> Result<String> {
        let conn = self.connection(server).await?;
        let cfg = self.configs.get(server).cloned().unwrap_or_default();
        let timeout = cfg
            .tool_timeouts
            .get(tool)
            .copied()
            .unwrap_or(cfg.tool_timeout_sec.max(1));
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            request(
                &conn,
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
            ),
        )
        .await
        .map_err(|_| anyhow!("tool call timed out after {timeout}s"))??;

        if result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Ok(format!("[error] {}", render_content(&result)));
        }
        Ok(render_content(&result))
    }

    pub async fn read_resource(&self, server: &str, uri: &str) -> Result<String> {
        let conn = self.connection(server).await?;
        let result = request(&conn, "resources/read", json!({ "uri": uri })).await?;
        Ok(render_content(&result))
    }

    /// Drop all connections (child processes are terminated on drop).
    pub async fn shutdown(&self) {
        self.connections.lock().await.clear();
    }
}

fn render_content(result: &Value) -> String {
    let mut out = String::new();
    if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
        for item in content {
            let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            match kind {
                "text" => {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        out.push_str(text);
                        out.push('\n');
                    }
                }
                "resource" => {
                    if let Some(text) = item
                        .get("resource")
                        .and_then(|r| r.get("text"))
                        .and_then(|v| v.as_str())
                    {
                        out.push_str(text);
                        out.push('\n');
                    } else if let Some(uri) = item.get("resource").and_then(|r| r.get("uri")) {
                        out.push_str(&format!("[resource: {uri}]\n"));
                    }
                }
                other => out.push_str(&format!("[{other} content]\n")),
            }
        }
    }
    if out.trim().is_empty() {
        out = serde_json::to_string_pretty(result).unwrap_or_default();
    }
    out.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// JSON-RPC plumbing
// ---------------------------------------------------------------------------

async fn request(conn: &Connection, method: &str, params: Value) -> Result<Value> {
    let id = conn.next_id.fetch_add(1, Ordering::SeqCst);
    let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let response = exchange(conn, message, Some(id)).await?;
    if let Some(error) = response.get("error") {
        return Err(anyhow!("MCP error: {error}"));
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

async fn notify(conn: &Connection, method: &str, params: Value) -> Result<()> {
    let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
    exchange(conn, message, None).await?;
    Ok(())
}

async fn exchange(conn: &Connection, message: Value, want_id: Option<i64>) -> Result<Value> {
    if let Some(http) = &conn.http {
        return http_exchange(http, message, want_id).await;
    }
    let transport = conn
        .stdio
        .as_ref()
        .ok_or_else(|| anyhow!("connection has no transport"))?;
    let mut guard = transport.lock().await;
    let mut line = serde_json::to_string(&message)?;
    line.push('\n');
    guard.stdin.write_all(line.as_bytes()).await?;
    guard.stdin.flush().await?;
    let Some(id) = want_id else {
        return Ok(Value::Null);
    };
    loop {
        let mut buf = String::new();
        let n = guard.stdout.read_line(&mut buf).await?;
        if n == 0 {
            return Err(anyhow!("MCP server closed the connection"));
        }
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if value.get("id").and_then(|v| v.as_i64()) == Some(id) {
            return Ok(value);
        }
    }
}

async fn http_exchange(
    http: &HttpTransport,
    message: Value,
    want_id: Option<i64>,
) -> Result<Value> {
    let mut req = http.client.post(&http.url).json(&message);
    for (k, v) in &http.headers {
        req = req.header(k, v);
    }
    if let Some(session) = http.session_id.lock().await.clone() {
        req = req.header("mcp-session-id", session);
    }
    req = req.header("accept", "application/json, text/event-stream");
    let resp = req.send().await?;
    if let Some(session) = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
    {
        *http.session_id.lock().await = Some(session.to_string());
    }
    if want_id.is_none() {
        return Ok(Value::Null);
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = resp.text().await?;
    if content_type.contains("text/event-stream") {
        let mut last = Value::Null;
        for line in body.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    last = value;
                }
            }
        }
        return Ok(last);
    }
    Ok(serde_json::from_str(&body).unwrap_or(Value::Null))
}

/// Expand `${VAR}` templates in config strings.
pub fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find('}') {
            let var = &rest[start + 2..start + end];
            out.push_str(&std::env::var(var).unwrap_or_default());
            rest = &rest[start + end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_env() {
        std::env::set_var("CACHY_TEST_VAR", "value");
        assert_eq!(expand_env("a-${CACHY_TEST_VAR}-b"), "a-value-b");
    }
}
