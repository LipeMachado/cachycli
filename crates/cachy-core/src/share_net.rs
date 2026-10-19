//! Handing sessions to a running `agent serve`, over its socket.
//!
//! `cachycli share --to ws://host:8765` sends the current session; `--id <id>`
//! pulls one back; `--list` shows what the server holds. The server keeps the
//! bundles under `~/.cachy/shares`, so a session can be handed over without
//! either side having a server of its own.

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::share::ShareEntry;

/// How long to wait for the server to answer one request.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// One request, one answer.
async fn call(url: &str, request: serde_json::Value) -> Result<serde_json::Value> {
    let (mut socket, _) = tokio_tungstenite::connect_async(url)
        .await
        .with_context(|| format!("connecting to {url}"))?;
    socket
        .send(Message::Text(request.to_string()))
        .await
        .with_context(|| format!("sending to {url}"))?;
    loop {
        let message = tokio::time::timeout(TIMEOUT, socket.next())
            .await
            .map_err(|_| anyhow!("{url} did not answer"))?;
        let message = message
            .with_context(|| format!("{url} closed the connection"))?
            .with_context(|| format!("reading from {url}"))?;
        let Message::Text(text) = message else { continue };
        let reply: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("{url} answered with something else: {text}"))?;
        if reply.get("type").and_then(|v| v.as_str()) == Some("error") {
            let message = reply
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("the server refused");
            anyhow::bail!("{url}: {message}");
        }
        return Ok(reply);
    }
}

/// Send a bundle to a server; returns what it stored.
pub async fn push(url: &str, bundle: &str) -> Result<ShareEntry> {
    let reply = call(url, serde_json::json!({ "share": bundle })).await?;
    serde_json::from_value(reply).context("the server answered with something other than a share")
}

/// Fetch a shared session; an empty `id` asks for the most recent one.
pub async fn fetch(url: &str, id: &str) -> Result<String> {
    let reply = call(url, serde_json::json!({ "fetch": id })).await?;
    reply
        .get("data")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("{url} sent no bundle"))
}

/// Ask the server to forget a shared session.
pub async fn remove(url: &str, id: &str) -> Result<String> {
    let reply = call(url, serde_json::json!({ "unshare": id })).await?;
    reply
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("{url} did not say what it removed"))
}

/// What the server is holding.
pub async fn list_remote(url: &str) -> Result<Vec<ShareEntry>> {
    let reply = call(url, serde_json::json!({ "shares": true })).await?;
    let items = reply
        .get("items")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    serde_json::from_value(items).context("the server answered with something other than a list")
}
