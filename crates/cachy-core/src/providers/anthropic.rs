//! Anthropic Messages API provider with streaming and tool use.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{http_client, join_url, LlmProvider};
use crate::message::{ChatMessage, ProviderEvent, Role, StopReason};
use crate::models::{auth_headers, Model};
use crate::tools::ToolSpec;

pub struct AnthropicProvider {
    model: Model,
}

impl AnthropicProvider {
    pub fn new(model: Model) -> Self {
        Self { model }
    }

    fn endpoint(&self) -> String {
        let base = self.model.base_url.trim_end_matches('/');
        if base.ends_with("/messages") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            join_url(base, "v1/messages")
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolSpec>,
        cancel: CancellationToken,
        tx: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Result<()> {
        // Split system messages; Anthropic takes them separately.
        let mut system = String::new();
        let mut wire = Vec::new();
        for m in &messages {
            match m.role {
                Role::System => {
                    if !system.is_empty() {
                        system.push_str("\n\n");
                    }
                    system.push_str(&m.content);
                }
                Role::User => {
                    wire.push(json!({ "role": "user", "content": [{ "type": "text", "text": m.content }] }));
                }
                Role::Assistant => {
                    let mut blocks = Vec::new();
                    if !m.content.is_empty() {
                        blocks.push(json!({ "type": "text", "text": m.content }));
                    }
                    for call in &m.tool_calls {
                        let input: Value =
                            serde_json::from_str(&call.arguments).unwrap_or(json!({}));
                        blocks.push(json!({ "type": "tool_use", "id": call.id, "name": call.name, "input": input }));
                    }
                    if !blocks.is_empty() {
                        wire.push(json!({ "role": "assistant", "content": blocks }));
                    }
                }
                Role::Tool => {
                    let block = json!({
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": [{ "type": "text", "text": m.content }],
                    });
                    // Merge consecutive tool results into one user message.
                    if let Some(last) = wire.last_mut() {
                        if last.get("role").and_then(|v| v.as_str()) == Some("user")
                            && last
                                .get("content")
                                .and_then(|c| c.as_array())
                                .map(|a| {
                                    a.iter().all(|b| {
                                        b.get("type").and_then(|t| t.as_str())
                                            == Some("tool_result")
                                    })
                                })
                                .unwrap_or(false)
                        {
                            if let Some(arr) =
                                last.get_mut("content").and_then(|c| c.as_array_mut())
                            {
                                arr.push(block);
                            }
                            continue;
                        }
                    }
                    wire.push(json!({ "role": "user", "content": [block] }));
                }
            }
        }

        let mut body = json!({
            "model": self.model.model_id,
            "max_tokens": self.model.max_completion_tokens.unwrap_or(8192),
            "messages": wire,
            "stream": true,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if let Some(t) = self.model.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(p) = self.model.top_p {
            body["top_p"] = json!(p);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools
                .iter()
                .map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                }))
                .collect::<Vec<_>>());
        }

        let mut req = http_client()
            .post(self.endpoint())
            .json(&body)
            .header("accept", "text/event-stream");
        for (k, v) in auth_headers(&self.model) {
            req = req.header(k, v);
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = tx.send(ProviderEvent::Done { stop_reason: StopReason::Cancelled });
                return Ok(());
            }
            r = req.send() => r?,
        };
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let detail: String = text.chars().take(600).collect();
            return Err(anyhow!(
                "model request failed (HTTP {status}) at {}: {detail}",
                self.endpoint()
            ));
        }

        // Track the current content block by index for tool_use accumulation.
        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        let mut buffer = String::new();
        let mut current_tool: Option<(String, String)> = None; // (id, name)
        let mut stop_reason = StopReason::EndTurn;

        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = tx.send(ProviderEvent::Done { stop_reason: StopReason::Cancelled });
                    return Ok(());
                }
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            let bytes = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(pos) = buffer.find('\n') {
                let line: String = buffer.drain(..=pos).collect();
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "content_block_start" => {
                        let block = value.get("content_block").cloned().unwrap_or(json!({}));
                        if let Some("tool_use") = block.get("type").and_then(|v| v.as_str()) {
                            let id = block
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let name = block
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let _ = tx.send(ProviderEvent::ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                            });
                            current_tool = Some((id, name));
                        }
                    }
                    "content_block_delta" => {
                        let delta = value.get("delta").cloned().unwrap_or(json!({}));
                        match delta.get("type").and_then(|v| v.as_str()) {
                            Some("text_delta") => {
                                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                    let _ = tx.send(ProviderEvent::TextDelta(text.to_string()));
                                }
                            }
                            Some("thinking_delta") => {
                                if let Some(text) = delta.get("thinking").and_then(|v| v.as_str()) {
                                    let _ = tx.send(ProviderEvent::ThoughtDelta(text.to_string()));
                                }
                            }
                            Some("input_json_delta") => {
                                if let (Some(json_part), Some((id, _))) = (
                                    delta.get("partial_json").and_then(|v| v.as_str()),
                                    current_tool.clone(),
                                ) {
                                    let _ = tx.send(ProviderEvent::ToolCallArgsDelta {
                                        id,
                                        args_delta: json_part.to_string(),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        current_tool = None;
                    }
                    "message_delta" => {
                        if let Some(reason) = value
                            .get("delta")
                            .and_then(|d| d.get("stop_reason"))
                            .and_then(|v| v.as_str())
                        {
                            stop_reason = match reason {
                                "tool_use" => StopReason::ToolUse,
                                "max_tokens" => StopReason::MaxTokens,
                                "refusal" => StopReason::Refusal,
                                _ => StopReason::EndTurn,
                            };
                        }
                        if let Some(usage) = value.get("usage") {
                            let _ = tx.send(ProviderEvent::Usage {
                                input_tokens: usage
                                    .get("input_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                output_tokens: usage
                                    .get("output_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                cached_input_tokens: usage
                                    .get("cache_read_input_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                            });
                        }
                    }
                    "message_start" => {
                        if let Some(usage) = value.get("message").and_then(|m| m.get("usage")) {
                            let _ = tx.send(ProviderEvent::Usage {
                                input_tokens: usage
                                    .get("input_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                output_tokens: 0,
                                cached_input_tokens: usage
                                    .get("cache_read_input_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                            });
                        }
                    }
                    "error" => {
                        let err = value.get("error").cloned().unwrap_or(json!({}));
                        return Err(anyhow!("model error: {err}"));
                    }
                    _ => {}
                }
            }
        }

        let _ = tx.send(ProviderEvent::Done { stop_reason });
        Ok(())
    }
}
