//! Message and content types shared by providers, tools and persistence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// A provider-neutral chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    /// Reasoning/thinking text associated with this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Tool calls requested by the assistant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Set for `Role::Tool` messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Optional name (tool name for tool results, or agent name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Timestamp (RFC 3339) when the message was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let mut m = Self::new(Role::Tool, content);
        m.tool_call_id = Some(tool_call_id.into());
        m.name = Some(name.into());
        m
    }
    fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
            ts: Some(chrono::Utc::now().to_rfc3339()),
        }
    }
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON string of arguments as produced by the model.
    pub arguments: String,
}

impl ToolCall {
    /// Parse arguments into JSON, returning an object (empty when invalid).
    pub fn parsed_arguments(&self) -> serde_json::Value {
        serde_json::from_str(&self.arguments)
            .unwrap_or(serde_json::Value::Object(Default::default()))
    }
}

/// Why a model turn stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    MaxTurns,
    Cancelled,
    Refusal,
    Error,
}

impl StopReason {
    pub fn as_acp(&self) -> &'static str {
        match self {
            StopReason::EndTurn => "end_turn",
            StopReason::ToolUse => "end_turn",
            StopReason::MaxTokens => "max_tokens",
            StopReason::MaxTurns => "max_turn_requests",
            StopReason::Cancelled => "cancelled",
            StopReason::Refusal => "refusal",
            StopReason::Error => "refusal",
        }
    }
}

/// Streaming events emitted by a provider for one model request.
#[derive(Debug, Clone)]
pub enum ProviderEvent {
    TextDelta(String),
    ThoughtDelta(String),
    ToolCall {
        id: String,
        name: String,
    },
    ToolCallArgsDelta {
        id: String,
        args_delta: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        cached_input_tokens: u64,
    },
    Done {
        stop_reason: StopReason,
    },
}

/// A completed assistant turn returned by providers that do not stream
/// incrementally into our event channel.
#[derive(Debug, Clone, Default)]
pub struct AssistantTurn {
    pub text: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: Option<StopReason>,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}
