//! OpenAI-compatible provider: `/chat/completions` with streaming and tools.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{http_client, join_url, LlmProvider};
use crate::message::{ChatMessage, ProviderEvent, StopReason};
use crate::models::{auth_headers, Model};
use crate::tools::ToolSpec;

pub struct OpenAiProvider {
    model: Model,
}

impl OpenAiProvider {
    pub fn new(model: Model) -> Self {
        Self { model }
    }

    fn endpoint(&self) -> String {
        let base = self.model.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") || base.ends_with("/responses") {
            base.to_string()
        } else {
            join_url(base, "chat/completions")
        }
    }

    fn build_messages(&self, messages: &[ChatMessage]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| {
                let mut obj = json!({ "role": m.role.as_str() });
                match m.role {
                    crate::message::Role::Tool => {
                        obj["tool_call_id"] = json!(m.tool_call_id.clone().unwrap_or_default());
                        obj["content"] = json!(m.content);
                        if let Some(name) = &m.name {
                            obj["name"] = json!(name);
                        }
                    }
                    crate::message::Role::Assistant if !m.tool_calls.is_empty() => {
                        obj["content"] = if m.content.is_empty() { Value::Null } else { json!(m.content) };
                        obj["tool_calls"] = json!(m
                            .tool_calls
                            .iter()
                            .map(|c| json!({
                                "id": c.id,
                                "type": "function",
                                "function": { "name": c.name, "arguments": c.arguments }
                            }))
                            .collect::<Vec<_>>());
                    }
                    _ => {
                        obj["content"] = json!(m.content);
                    }
                }
                obj
            })
            .collect()
    }

    fn build_body(&self, messages: &[ChatMessage], tools: &[ToolSpec]) -> Value {
        let mut body = json!({
            "model": self.model.model_id,
            "messages": self.build_messages(messages),
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(t) = self.model.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(p) = self.model.top_p {
            body["top_p"] = json!(p);
        }
        if let Some(m) = self.model.max_completion_tokens {
            body["max_completion_tokens"] = json!(m);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                }))
                .collect::<Vec<_>>());
        }
        body
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolSpec>,
        cancel: CancellationToken,
        tx: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Result<()> {
        let body = self.build_body(&messages, &tools);
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

        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        let mut buffer = String::new();
        let mut saw_usage = false;
        let mut finish: Option<String> = None;

        'outer: loop {
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
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    // Some servers keep the connection open after the terminator.
                    break 'outer;
                }
                if data.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(data) else { continue };
                if let Some(err) = value.get("error") {
                    return Err(anyhow!("model error: {err}"));
                }
                if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
                    saw_usage = true;
                    let _ = tx.send(ProviderEvent::Usage {
                        input_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        output_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        cached_input_tokens: usage
                            .get("prompt_tokens_details")
                            .and_then(|d| d.get("cached_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    });
                }
                let Some(choices) = value.get("choices").and_then(|c| c.as_array()) else { continue };
                for choice in choices {
                    if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                        finish = Some(reason.to_string());
                    }
                    let Some(delta) = choice.get("delta") else { continue };
                    if let Some(reasoning) = delta
                        .get("reasoning_content")
                        .or_else(|| delta.get("reasoning"))
                        .and_then(|v| v.as_str())
                    {
                        if !reasoning.is_empty() {
                            let _ = tx.send(ProviderEvent::ThoughtDelta(reasoning.to_string()));
                        }
                    }
                    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                        if !content.is_empty() {
                            let _ = tx.send(ProviderEvent::TextDelta(content.to_string()));
                        }
                    }
                    if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                        for call in calls {
                            let id = call
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let name = call
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if !name.is_empty() {
                                let _ = tx.send(ProviderEvent::ToolCall { id: id.clone(), name });
                            }
                            if let Some(args) = call
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                            {
                                if !args.is_empty() {
                                    let _ = tx.send(ProviderEvent::ToolCallArgsDelta {
                                        id: id.clone(),
                                        args_delta: args.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        let _ = saw_usage;
        let stop_reason = match finish.as_deref() {
            Some("tool_calls") | Some("function_call") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            Some("content_filter") => StopReason::Refusal,
            _ => StopReason::EndTurn,
        };
        let _ = tx.send(ProviderEvent::Done { stop_reason });
        Ok(())
    }
}
