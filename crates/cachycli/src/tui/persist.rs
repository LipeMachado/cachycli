//! Keeping the stored session in step with a provider that runs its own.
//!
//! The native loop writes the transcript itself. When opencode, ACP or Claude
//! Code run the turn, the interface is the only place that sees the messages,
//! so it writes them: `/export`, `/share`, the dashboard, `/timeline all` and
//! a later `--resume` all read from there. The provider's own session id is
//! remembered too, so the next run can continue *there* instead of starting a
//! new conversation.

use std::path::PathBuf;

use cachy_core::agent::{AgentEvent, AgentSession};
use cachy_core::message::ChatMessage;
use cachy_core::session::{self, SessionStore, SessionUpdate};

/// One tool call seen during a turn.
struct ToolLine {
    id: String,
    name: String,
    title: String,
    kind: String,
    arguments: serde_json::Value,
    ts: String,
    status: String,
    summary: Option<String>,
}

/// Watches a provider's events and writes them to the session directory.
pub struct BackendTranscript {
    cwd: PathBuf,
    session_id: String,
    provider: String,
    store: Option<SessionStore>,
    user: Option<String>,
    assistant: String,
    thought: String,
    tools: Vec<ToolLine>,
    /// The provider's session id, as last seen.
    backend_session: Option<String>,
}

impl BackendTranscript {
    pub fn new(cwd: PathBuf, session_id: String, provider: String) -> Self {
        Self {
            cwd,
            session_id,
            provider,
            store: None,
            user: None,
            assistant: String::new(),
            thought: String::new(),
            tools: Vec::new(),
            backend_session: None,
        }
    }

    /// Point at another session (`/new`, `/fork`, `/resume` inside the UI).
    pub fn set_session(&mut self, id: &str) {
        if self.session_id != id {
            self.session_id = id.to_string();
            self.store = None;
        }
    }

    /// Watch one event. `backend` is asked for its session id when a turn ends.
    pub async fn on_event(&mut self, event: &AgentEvent, backend: &dyn AgentSession) {
        match event {
            AgentEvent::UserMessage { text } => {
                self.user = Some(text.clone());
                self.assistant.clear();
                self.thought.clear();
                self.tools.clear();
            }
            AgentEvent::TextDelta(delta) => self.assistant.push_str(delta),
            AgentEvent::ReasoningDelta(delta) => self.thought.push_str(delta),
            AgentEvent::ToolCall {
                id,
                name,
                title,
                kind,
                arguments,
            } => {
                self.tools.push(ToolLine {
                    id: id.clone(),
                    name: name.clone(),
                    title: title.clone(),
                    kind: kind.clone(),
                    arguments: arguments.clone(),
                    ts: now(),
                    status: "running".into(),
                    summary: None,
                });
            }
            AgentEvent::ToolCallUpdate {
                id,
                status,
                summary,
                ..
            } => {
                if let Some(tool) = self.tools.iter_mut().find(|tool| tool.id == *id) {
                    tool.status = status.clone();
                    if summary.is_some() {
                        tool.summary = summary.clone();
                    }
                }
            }
            AgentEvent::TurnEnd { .. } => self.finish_turn(backend).await,
            _ => {}
        }
    }

    /// Write the finished turn to `chat_history.jsonl` and `updates.jsonl`, and
    /// remember the provider's session id.
    async fn finish_turn(&mut self, backend: &dyn AgentSession) {
        let user = self.user.take();
        let assistant = std::mem::take(&mut self.assistant);
        let thought = std::mem::take(&mut self.thought);
        let tools = std::mem::take(&mut self.tools);

        let Some(mut store) = self.ensure_store() else {
            return;
        };

        if let Some(id) = backend.backend_session().await {
            if self.backend_session.as_deref() != Some(id.as_str()) {
                self.backend_session = Some(id.clone());
                store.summary.backend_session = Some(id);
                store.summary.backend_provider = Some(self.provider.clone());
                let _ = store.save_summary();
            }
        }

        if let Some(text) = &user {
            let _ = store.append_chat(&ChatMessage::user(text.clone()));
        }
        if !assistant.trim().is_empty() {
            let _ = store.append_chat(&ChatMessage::assistant(assistant.clone()));
        }

        let at = now();
        if let Some(text) = user {
            let _ = store.append_update(&SessionUpdate::UserMessage {
                text,
                ts: at.clone(),
            });
        }
        for tool in &tools {
            let _ = store.append_update(&SessionUpdate::ToolCall {
                id: tool.id.clone(),
                name: tool.name.clone(),
                title: tool.title.clone(),
                kind: tool.kind.clone(),
                arguments: tool.arguments.clone(),
                ts: tool.ts.clone(),
            });
            let _ = store.append_update(&SessionUpdate::ToolCallUpdate {
                id: tool.id.clone(),
                status: tool.status.clone(),
                content: tool.summary.clone(),
                ts: now(),
            });
        }
        if !thought.trim().is_empty() {
            let _ = store.append_update(&SessionUpdate::AgentThought {
                text: thought,
                ts: at.clone(),
            });
        }
        if !assistant.trim().is_empty() {
            let _ = store.append_update(&SessionUpdate::AgentMessage {
                text: assistant,
                ts: at,
            });
        }
        let _ = store.append_update(&SessionUpdate::TurnEnd {
            stop_reason: "end_turn".into(),
            ts: now(),
        });
    }

    /// The session this transcript belongs to, opened once and kept.
    fn ensure_store(&mut self) -> Option<SessionStore> {
        if self.store.is_none() {
            self.store = SessionStore::open(&self.cwd, &self.session_id)
                .ok()
                .or_else(|| {
                    let summary = session::most_recent(&self.cwd).ok().flatten()?;
                    SessionStore::open(&self.cwd, &summary.id).ok()
                });
        }
        self.store.clone()
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
