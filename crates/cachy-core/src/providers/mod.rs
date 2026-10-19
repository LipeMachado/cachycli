//! LLM providers: OpenAI-compatible chat completions and Anthropic messages,
//! plus the opencode / ACP agent backends.

pub mod acp_client;
pub mod anthropic;
pub mod claude_code;
pub mod cli_agent;
pub mod openai;
pub mod opencode;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::message::{ChatMessage, ProviderEvent};
use crate::models::Model;
use crate::tools::ToolSpec;

/// A streaming chat provider.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream one model request. Events are pushed to `tx`.
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolSpec>,
        cancel: CancellationToken,
        tx: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Result<()>;
}

/// Build the provider for a model.
pub fn for_model(model: &Model) -> Box<dyn LlmProvider> {
    match model.api_backend {
        crate::models::ApiBackend::Anthropic => {
            Box::new(anthropic::AnthropicProvider::new(model.clone()))
        }
        crate::models::ApiBackend::Responses => {
            Box::new(openai::OpenAiProvider::new(model.clone()))
        }
        crate::models::ApiBackend::Chat => Box::new(openai::OpenAiProvider::new(model.clone())),
    }
}

/// Shared HTTP client builder.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_default()
}

/// Join base URL and path without doubling slashes.
pub fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Best-effort context window estimate from a message list.
pub fn estimate_tokens(messages: &[ChatMessage]) -> u64 {
    let chars: usize = messages
        .iter()
        .map(|m| {
            m.content.len() + m.reasoning.as_deref().map(str::len).unwrap_or(0) + m.arguments_len()
        })
        .sum();
    (chars as u64 / 4).max(1)
}

trait ArgumentsLen {
    fn arguments_len(&self) -> usize;
}

impl ArgumentsLen for ChatMessage {
    fn arguments_len(&self) -> usize {
        self.tool_calls
            .iter()
            .map(|c| c.name.len() + c.arguments.len())
            .sum()
    }
}
