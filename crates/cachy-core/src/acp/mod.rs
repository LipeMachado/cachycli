//! Agent Client Protocol (ACP) support: a client that can drive any ACP agent
//! (e.g. `opencode acp`, `cachycli agent stdio`) and a server that exposes
//! cachycli as an ACP agent (`cachycli agent stdio`).
//!
//! Protocol version 1, JSON-RPC 2.0 over stdio, line-delimited.

pub mod client;
pub mod server;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

/// A content block in prompts and messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ResourceLink {
        uri: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
    },
    Resource {
        resource: Value,
    },
    Image {
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        data: Option<String>,
        #[serde(default)]
        uri: Option<String>,
    },
    Audio {
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        data: Option<String>,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// A session/update payload variant (subset used by cachycli).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub enum SessionUpdatePayload {
    UserMessageChunk {
        #[serde(default)]
        content: Value,
    },
    AgentMessageChunk {
        #[serde(default)]
        message_id: Option<String>,
        content: Value,
    },
    AgentThoughtChunk {
        #[serde(default)]
        message_id: Option<String>,
        content: Value,
    },
    ToolCall {
        tool_call_id: String,
        title: String,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        raw_input: Option<Value>,
    },
    ToolCallUpdate {
        tool_call_id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        content: Option<Value>,
        #[serde(default)]
        raw_output: Option<Value>,
    },
    Plan {
        entries: Vec<PlanEntryPayload>,
    },
    AvailableCommandsUpdate {
        #[serde(default)]
        available_commands: Vec<Value>,
    },
    CurrentModeUpdate {
        #[serde(default)]
        current_mode_id: Option<String>,
    },
    UsageUpdate {
        #[serde(default)]
        used: u64,
        #[serde(default)]
        size: u64,
        #[serde(default)]
        cost: Option<Value>,
    },
    SessionInfoUpdate {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        updated_at: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEntryPayload {
    pub content: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_priority() -> String {
    "medium".into()
}
fn default_status() -> String {
    "pending".into()
}

/// Extract text from an ACP content value (used for chunk payloads).
pub fn content_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
