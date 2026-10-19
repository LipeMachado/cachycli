//! The native agent engine: turn loop, tool execution, approvals and events.
//!
//! `NativeAgent` drives one conversation against an LLM provider. The TUI,
//! headless mode and the ACP server all consume the same `AgentEvent` stream.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::message::{ChatMessage, ProviderEvent, StopReason, TokenUsage, ToolCall};
use crate::models::Model;
use crate::permissions::{ApprovalChoice, Decision, PermissionEngine, RuleSet};
use crate::providers;
use crate::session::{PlanEntry, SessionStore, SessionUpdate};
use crate::tools::task::{SubagentOutcome, SubagentRequest, SubagentRunner};
use crate::tools::{ToolContext, ToolRegistry};

/// Events emitted while an agent turn runs.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Started {
        session_id: String,
        model: String,
    },
    UserMessage {
        text: String,
    },
    TextDelta(String),
    ReasoningDelta(String),
    MessageComplete {
        text: String,
    },
    ReasoningComplete {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        title: String,
        kind: String,
        arguments: Value,
    },
    ToolCallUpdate {
        id: String,
        status: String,
        title: Option<String>,
        summary: Option<String>,
        meta: Option<Value>,
    },
    PermissionRequest {
        id: String,
        tool: String,
        title: String,
        detail: String,
        options: Vec<String>,
    },
    Todos(Vec<crate::tools::TodoItem>),
    Plan(Vec<PlanEntry>),
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        context_used: u64,
        context_size: u64,
    },
    TurnEnd {
        stop_reason: StopReason,
        duration_secs: f64,
    },
    BackgroundFinished {
        task_id: String,
        description: String,
        status: String,
    },
    Info(String),
    Error(String),
}

/// Decision returned from an approval prompt.
#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    Choice(ApprovalChoice),
    RejectWithFeedback(String),
    Cancelled,
}

/// How a prompt run should behave.
#[derive(Debug, Clone, Default)]
pub struct TurnOptions {
    pub max_turns: Option<u32>,
    /// Interactive sessions ask the user; headless sessions error instead.
    pub interactive: bool,
}

/// Shared trait implemented by all session backends (native, opencode, ACP).
#[async_trait]
pub trait AgentSession: Send + Sync {
    async fn prompt(&self, text: String) -> Result<StopReason>;
    fn cancel(&self);
    fn session_id(&self) -> String;
    fn model_label(&self) -> String;
    fn context_usage(&self) -> (u64, u64);
    async fn shutdown(&self);

    /// The provider's own session id, when the provider keeps sessions itself
    /// (opencode, ACP, Claude Code). `None` for the native loop, whose sessions
    /// are cachycli's own.
    async fn backend_session(&self) -> Option<String> {
        None
    }

    /// Start a new conversation on the provider's side (`/new`), so the model
    /// does not keep answering from the old one.
    async fn reset_backend_session(&self) {}

    /// Drop the last `turns` turns from the provider's own conversation
    /// (`/rewind`). Returns `false` when the provider cannot.
    async fn rewind_backend_session(&self, _turns: usize) -> bool {
        false
    }

    /// Branch the provider's own conversation (`/fork`). Returns `false` when
    /// the provider cannot fork, in which case the fork keeps sharing the
    /// original conversation.
    async fn fork_backend_session(&self) -> bool {
        false
    }

    /// Continue an earlier provider session instead of starting a new one.
    /// Returns `false` when the provider does not have that session any more,
    /// in which case the caller keeps the fresh one.
    async fn resume_backend_session(&self, _id: &str) -> bool {
        false
    }
}

/// The native agent.
pub struct NativeAgent {
    pub cwd: PathBuf,
    pub config: Arc<Config>,
    pub model: Model,
    pub model_swap: Mutex<Option<Model>>,
    pub session: Mutex<SessionStore>,
    pub history: Mutex<Vec<ChatMessage>>,
    pub engine: RwLock<PermissionEngine>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>,
    pub cancel: Mutex<Option<CancellationToken>>,
    pub tx: mpsc::UnboundedSender<AgentEvent>,
    pub ctx: ToolContext,
    pub tools: Arc<ToolRegistry>,
    pub max_turns: u32,
    pub usage: RwLock<TokenUsage>,
    pub injected_skills: Mutex<Vec<crate::skills::Skill>>,
    /// Agent profile text injected into the system prompt (`/personas`).
    persona: tokio::sync::RwLock<Option<String>>,
    /// What this session is working towards (`/goal`); lives in the context so
    /// the `set_goal` tool can change it too.
    goal: Arc<tokio::sync::RwLock<Option<String>>>,
    /// Hooks discovered for this directory, so a turn can run them.
    hooks: Vec<crate::hooks::DiscoveredHook>,
    /// True while a turn is running.
    pub busy: RwLock<bool>,
    /// True for interactive sessions (TUI); headless denies prompts.
    pub interactive: bool,
    /// Nesting level of this agent (0 = main, 1 = subagent, …).
    pub depth: usize,
    /// Limits how many subagents run at once.
    subagent_slots: Arc<tokio::sync::Semaphore>,
}

impl NativeAgent {
    /// Background tasks and subagents this agent is running (`/tasks`).
    pub fn tasks(&self) -> Arc<crate::tools::TaskRegistry> {
        self.ctx.tasks.clone()
    }

    #[allow(clippy::too_many_arguments)] // a constructor: every field has to come in
    pub async fn new(
        cwd: PathBuf,
        config: Arc<Config>,
        model: Model,
        session: SessionStore,
        mcp: Arc<crate::mcp::McpManager>,
        tools: ToolRegistry,
        permissions: PermissionEngine,
        tx: mpsc::UnboundedSender<AgentEvent>,
        max_turns: u32,
        interactive: bool,
        depth: usize,
    ) -> Result<Arc<Self>> {
        let tools = Arc::new(tools);
        let cwd_for_hooks = cwd.clone();
        let config_for_hooks = config.clone();
        let mut ctx = ToolContext::new(cwd.clone(), config.clone(), mcp);
        ctx.events = Some(tx.clone());
        ctx.depth = depth;
        let ctx_goal = ctx.goal.clone();
        let slots = config.subagents.max_concurrent.max(1);

        let agent = Arc::new(Self {
            cwd,
            config,
            model,
            model_swap: Mutex::new(None),
            session: Mutex::new(session),
            history: Mutex::new(Vec::new()),
            engine: RwLock::new(permissions),
            approvals: Mutex::new(HashMap::new()),
            cancel: Mutex::new(None),
            tx: tx.clone(),
            ctx,
            tools,
            max_turns,
            depth,
            subagent_slots: Arc::new(tokio::sync::Semaphore::new(slots)),
            usage: RwLock::new(TokenUsage::default()),
            injected_skills: Mutex::new(Vec::new()),
            hooks: crate::hooks::discover(&cwd_for_hooks, &config_for_hooks),
            persona: tokio::sync::RwLock::new(None),
            goal: ctx_goal.clone(),
            busy: RwLock::new(false),
            interactive,
        });

        // Register the subagent runner now that the Arc exists.
        let _ = agent
            .ctx
            .subagents
            .set(agent.clone() as Arc<dyn SubagentRunner>);

        let _ = tx.send(AgentEvent::Started {
            session_id: agent.session_id().await,
            model: agent.model.display().to_string(),
        });
        Ok(agent)
    }

    pub fn ctx_clone(&self) -> ToolContext {
        self.ctx.clone()
    }

    pub async fn session_id(&self) -> String {
        self.session.lock().await.summary.id.clone()
    }

    pub async fn load_history(&self, messages: Vec<ChatMessage>) {
        *self.history.lock().await = messages;
    }

    /// Continue in another stored session (`/fork`): the transcript and the
    /// conversation move with it, so the loop stops writing to the old one.
    pub async fn adopt_session(&self, store: SessionStore, history: Vec<ChatMessage>) {
        *self.session.lock().await = store;
        *self.history.lock().await = history;
    }

    pub async fn resolve_approval(&self, id: &str, decision: ApprovalDecision) {
        if let Some(tx) = self.approvals.lock().await.remove(id) {
            let _ = tx.send(decision);
        }
    }

    pub fn cancel_current(&self) {
        if let Ok(guard) = self.cancel.try_lock() {
            if let Some(token) = guard.as_ref() {
                token.cancel();
            }
        }
    }

    pub async fn inject_skill(&self, skill: crate::skills::Skill) {
        self.injected_skills.lock().await.push(skill);
    }

    pub async fn set_model(&self, model: Model) {
        *self.model_swap.lock().await = Some(model);
    }

    pub async fn current_model(&self) -> Model {
        let guard = self.model_swap.lock().await;
        guard.clone().unwrap_or_else(|| self.model.clone())
    }

    pub async fn label(&self) -> String {
        self.current_model().await.display().to_string()
    }

    pub async fn usage_snapshot(&self) -> (TokenUsage, u64) {
        let usage = *self.usage.read().await;
        let ctx = self.current_model().await.context_window;
        (usage, ctx)
    }

    pub async fn set_permission_mode(&self, mode: crate::permissions::PermissionMode) {
        self.engine.write().await.mode = mode;
    }

    pub async fn set_always_approve(&self, on: bool) {
        self.engine.write().await.always_approve = on;
    }

    pub async fn always_approve(&self) -> bool {
        let e = self.engine.read().await;
        e.always_approve || e.mode.is_auto()
    }

    /// Compact the conversation by summarizing older messages.
    /// One model call that turns a slice of the conversation into a summary.
    async fn summarize(&self, transcript: &str) -> Result<String> {
        let prompt = format!(
            "Summarize this conversation so it can be continued by another assistant. \
             Keep decisions, file paths, code changes and open questions. Be concise.\n\n{transcript}"
        );
        let model = self.current_model().await;
        let provider = providers::for_model(&model);
        let (ptx, mut prx) = mpsc::unbounded_channel();
        let request = vec![
            ChatMessage::system("You summarize conversations compactly and accurately."),
            ChatMessage::user(prompt),
        ];
        let handle = tokio::spawn(async move {
            let _ = provider
                .chat(request, Vec::new(), CancellationToken::new(), ptx)
                .await;
        });
        let mut summary = String::new();
        while let Some(event) = prx.recv().await {
            match event {
                ProviderEvent::TextDelta(t) => summary.push_str(&t),
                ProviderEvent::Done { .. } => break,
                _ => {}
            }
        }
        let _ = handle.await;
        if summary.trim().is_empty() {
            anyhow::bail!("compaction produced no summary");
        }
        Ok(summary.trim().to_string())
    }

    /// Run a workflow: every step becomes a subagent, at most
    /// `[subagents] max_concurrent` at a time (the runner queues the rest).
    /// Returns one report line per step.
    pub async fn run_workflow(&self, workflow: crate::workflows::Workflow) -> Vec<String> {
        let _ = self.tx.send(AgentEvent::Info(format!(
            "workflow \"{}\": {} step(s)",
            workflow.name,
            workflow.steps.len()
        )));
        let ctx = self.ctx_clone();
        // The runner is registered on the context (the same path the `task`
        // tool uses), so nested agents share the concurrency limits.
        let Some(runner) = ctx.subagents.get().cloned() else {
            let _ = self
                .tx
                .send(AgentEvent::Info("subagent runner unavailable".into()));
            return Vec::new();
        };
        let lines = crate::tools::task::run_workflow_steps(runner, &ctx, &workflow).await;
        for line in &lines {
            let _ = self.tx.send(AgentEvent::Info(format!("workflow: {line}")));
        }
        let _ = self.tx.send(AgentEvent::Info(format!(
            "workflow \"{}\" finished ({} step(s))",
            workflow.name,
            lines.len()
        )));
        lines
    }

    /// Compaction in segments: the oldest messages are summarized chunk by
    /// chunk (so nothing is dropped), the most recent ones stay verbatim.
    pub async fn compact_segments(&self, keep_messages: usize) -> Result<usize> {
        let history = self.history.lock().await.clone();
        let keep = keep_messages.max(2);
        if history.len() <= keep + 2 {
            return Ok(0);
        }
        let split = history.len() - keep;
        let (old, recent) = history.split_at(split);

        let mut summaries: Vec<String> = Vec::new();
        const SEGMENT: usize = 8;
        for chunk in old.chunks(SEGMENT) {
            let mut transcript = String::new();
            for message in chunk {
                transcript.push_str(&format!("{}: {}\n", message.role.as_str(), message.content));
                for call in &message.tool_calls {
                    transcript
                        .push_str(&format!("  [tool_call {} {}]\n", call.name, call.arguments));
                }
            }
            if transcript.len() > 24_000 {
                transcript.truncate(24_000);
                transcript.push_str("\n[...truncated...]\n");
            }
            match self.summarize(&transcript).await {
                Ok(summary) => summaries.push(summary),
                Err(e) => {
                    let _ = self.tx.send(AgentEvent::Info(format!(
                        "segment summary failed ({e}); keeping the messages verbatim"
                    )));
                    summaries.push(transcript.chars().take(2_000).collect());
                }
            }
        }

        let mut new_history = vec![ChatMessage::assistant(format!(
            "[Conversation summary — {} earlier segment(s)]\n{}",
            summaries.len(),
            summaries.join("\n\n---\n\n")
        ))];
        new_history.extend(recent.to_vec());
        *self.history.lock().await = new_history.clone();
        // Keep the session file in step: a resume should see the summary too.
        if let Ok(mut store) = self.session.try_lock() {
            let _ = store.rewrite_chat(&new_history);
        }
        let _ = self.tx.send(AgentEvent::Info(format!(
            "Compacted {} earlier segment(s); the last {} messages stay verbatim.",
            summaries.len(),
            recent.len()
        )));
        Ok(summaries.len())
    }

    pub async fn compact(&self) -> Result<()> {
        let history = self.history.lock().await.clone();
        if history.len() < 6 {
            return Ok(());
        }
        let keep = 4usize;
        let split = history.len().saturating_sub(keep);
        let (old, recent) = history.split_at(split);
        let mut transcript = String::new();
        for m in old {
            transcript.push_str(&format!("{}: {}\n", m.role.as_str(), m.content));
            for call in &m.tool_calls {
                transcript.push_str(&format!("  [tool_call {} {}]\n", call.name, call.arguments));
            }
            if transcript.len() > 24_000 {
                transcript.push_str("[...older messages omitted...]\n");
                break;
            }
        }
        let summary = self.summarize(&transcript).await?;
        let mut new_history = vec![ChatMessage::assistant(format!(
            "[Conversation summary]\n{summary}"
        ))];
        new_history.extend(recent.to_vec());
        *self.history.lock().await = new_history.clone();
        if let Ok(mut store) = self.session.try_lock() {
            let _ = store.rewrite_chat(&new_history);
        }
        let _ = self
            .tx
            .send(AgentEvent::Info("Conversation compacted.".into()));
        Ok(())
    }

    /// Run one full turn (model + tools until it stops).
    pub async fn run_turn(&self, user_text: String, opts: TurnOptions) -> Result<StopReason> {
        let token = CancellationToken::new();
        {
            let mut guard = self.cancel.lock().await;
            *guard = Some(token.clone());
        }
        *self.busy.write().await = true;
        let started = std::time::Instant::now();
        let result = self.run_turn_inner(&user_text, &opts, token.clone()).await;
        *self.busy.write().await = false;
        {
            let mut guard = self.cancel.lock().await;
            *guard = None;
        }
        let stop_reason = match &result {
            Ok(r) => *r,
            Err(_) => StopReason::Error,
        };
        // The model may have set the goal with the `set_goal` tool: keep the
        // session file in step so a resume shows it.
        if let Some(goal) = self.goal().await {
            if let Ok(mut store) = self.session.try_lock() {
                if store.summary.goal.as_deref() != Some(goal.as_str()) {
                    store.summary.goal = Some(goal);
                    let _ = store.save_summary();
                }
            }
        }
        // Auto-compaction: when the context passes the configured threshold the
        // older segments are summarized so the session can keep going.
        let threshold = self.config.session.auto_compact_threshold_percent;
        if threshold > 0 && stop_reason != StopReason::Error {
            let (used, size) = self.context_usage();
            if size > 0 && used * 100 / size >= threshold as u64 {
                let _ = self.tx.send(AgentEvent::Info(format!(
                    "context at {}% — compacting older segments automatically",
                    used * 100 / size
                )));
                let _ = self.compact_segments(6).await;
            }
        }
        // Stop hooks run as the turn finishes; a failure is reported, not fatal.
        if !self.hooks.is_empty() {
            let payload = serde_json::json!({
                "stop_reason": format!("{stop_reason:?}"),
                "duration_secs": started.elapsed().as_secs_f64(),
            });
            let results = crate::hooks::run_event(
                &self.hooks,
                crate::hooks::HookEvent::Stop,
                &self.cwd,
                None,
                &payload,
            )
            .await;
            for (command, output, ok) in results {
                if !ok {
                    let _ = self.tx.send(AgentEvent::Info(format!(
                        "Stop hook failed: {command}: {output}"
                    )));
                }
            }
        }
        let _ = self.tx.send(AgentEvent::TurnEnd {
            stop_reason,
            duration_secs: started.elapsed().as_secs_f64(),
        });
        result
    }

    async fn run_turn_inner(
        &self,
        user_text: &str,
        opts: &TurnOptions,
        token: CancellationToken,
    ) -> Result<StopReason> {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = self.tx.send(AgentEvent::UserMessage {
            text: user_text.to_string(),
        });

        // UserPromptSubmit hooks see the prompt before the model does.
        if !self.hooks.is_empty() {
            let payload = serde_json::json!({ "prompt": user_text });
            let results = crate::hooks::run_event(
                &self.hooks,
                crate::hooks::HookEvent::UserPromptSubmit,
                &self.cwd,
                None,
                &payload,
            )
            .await;
            for (command, output, ok) in results {
                if !ok {
                    let _ = self.tx.send(AgentEvent::Info(format!(
                        "UserPromptSubmit hook failed: {command}: {output}"
                    )));
                }
            }
        }

        // Persist user turn.
        {
            let mut store = self.session.lock().await;
            store.set_title_if_empty(&crate::prompt::derive_title(user_text))?;
            store.append_update(&SessionUpdate::UserMessage {
                text: user_text.to_string(),
                ts: now.clone(),
            })?;
            store.append_chat(&ChatMessage::user(user_text))?;
        }
        {
            let mut history = self.history.lock().await;
            history.push(ChatMessage::user(user_text));
        }

        let max_turns = opts.max_turns.unwrap_or(self.max_turns).max(1);
        let mut turns = 0u32;
        let mut last_stop = StopReason::EndTurn;

        loop {
            turns += 1;
            if turns > max_turns {
                last_stop = StopReason::MaxTurns;
                break;
            }
            // Build the request: system prompt + history (+ injected skills).
            let system = self.build_system_prompt().await;
            // Keep the last one on disk, so what the model was told is not a
            // mystery after the fact (`system_prompt.txt`).
            {
                let store = self.session.lock().await;
                let _ = store.write_system_prompt(&system);
            }
            let history = self.history.lock().await.clone();
            let mut messages = Vec::with_capacity(history.len() + 1);
            messages.push(ChatMessage::system(system));
            messages.extend(history);

            let specs = self.tool_specs();
            let provider = providers::for_model(&self.current_model().await);
            let (ptx, mut prx) = mpsc::unbounded_channel::<ProviderEvent>();
            let cancel_for_provider = token.clone();
            let provider_task = tokio::spawn(async move {
                provider
                    .chat(messages, specs, cancel_for_provider, ptx)
                    .await
            });

            let mut assistant = ChatMessage::assistant("");
            let mut calls: Vec<ToolCall> = Vec::new();
            let mut call_order: Vec<String> = Vec::new();
            let mut provider_error: Option<anyhow::Error> = None;

            while let Some(event) = prx.recv().await {
                match event {
                    ProviderEvent::TextDelta(text) => {
                        assistant.content.push_str(&text);
                        let _ = self.tx.send(AgentEvent::TextDelta(text));
                    }
                    ProviderEvent::ThoughtDelta(text) => {
                        let reasoning = assistant.reasoning.get_or_insert_with(String::new);
                        reasoning.push_str(&text);
                        let _ = self.tx.send(AgentEvent::ReasoningDelta(text));
                    }
                    ProviderEvent::ToolCall { id, name } => {
                        if !call_order.contains(&id) {
                            call_order.push(id.clone());
                        }
                        if let Some(existing) = calls.iter_mut().find(|c| c.id == id) {
                            if existing.name.is_empty() {
                                existing.name = name;
                            }
                        } else {
                            calls.push(ToolCall {
                                id,
                                name,
                                arguments: String::new(),
                            });
                        }
                    }
                    ProviderEvent::ToolCallArgsDelta { id, args_delta } => {
                        let idx = if id.is_empty() {
                            calls.len().saturating_sub(1)
                        } else {
                            calls
                                .iter()
                                .position(|c| c.id == id)
                                .unwrap_or_else(|| calls.len().saturating_sub(1))
                        };
                        if let Some(call) = calls.get_mut(idx) {
                            if call.id.is_empty() {
                                call.id = id;
                            }
                            call.arguments.push_str(&args_delta);
                        } else {
                            calls.push(ToolCall {
                                id: if id.is_empty() { "call_0".into() } else { id },
                                name: String::new(),
                                arguments: args_delta,
                            });
                        }
                    }
                    ProviderEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cached_input_tokens,
                    } => {
                        let mut usage = self.usage.write().await;
                        usage.input_tokens = input_tokens.max(usage.input_tokens);
                        usage.output_tokens += output_tokens;
                        usage.cached_input_tokens = cached_input_tokens;
                        let (used, size) = self.usage_snapshot().await;
                        let _ = self.tx.send(AgentEvent::Usage {
                            input_tokens: used.input_tokens,
                            output_tokens: used.output_tokens,
                            context_used: used.total(),
                            context_size: size,
                        });
                    }
                    ProviderEvent::Done { stop_reason } => {
                        last_stop = stop_reason;
                        break;
                    }
                }
            }
            if let Ok(Err(e)) = provider_task.await {
                provider_error = Some(e);
            }
            if let Some(e) = provider_error {
                let _ = self.tx.send(AgentEvent::Error(e.to_string()));
                return Err(e);
            }

            // Record the assistant message.
            if !assistant.content.is_empty() {
                let _ = self.tx.send(AgentEvent::MessageComplete {
                    text: assistant.content.clone(),
                });
            }
            if let Some(reasoning) = &assistant.reasoning {
                if !reasoning.is_empty() {
                    let _ = self.tx.send(AgentEvent::ReasoningComplete {
                        text: reasoning.clone(),
                    });
                }
            }
            assistant.tool_calls = calls.clone();
            {
                let mut history = self.history.lock().await;
                history.push(assistant.clone());
            }
            {
                let mut store = self.session.lock().await;
                if !assistant.content.is_empty() {
                    let _ = store.append_update(&SessionUpdate::AgentMessage {
                        text: assistant.content.clone(),
                        ts: chrono::Utc::now().to_rfc3339(),
                    });
                }
                if let Some(reasoning) = &assistant.reasoning {
                    if !reasoning.is_empty() {
                        let _ = store.append_update(&SessionUpdate::AgentThought {
                            text: reasoning.clone(),
                            ts: chrono::Utc::now().to_rfc3339(),
                        });
                    }
                }
                let _ = store.append_chat(&assistant);
            }

            if calls.is_empty() {
                break;
            }

            // Execute tool calls sequentially.
            for call in &calls {
                if token.is_cancelled() {
                    last_stop = StopReason::Cancelled;
                    break;
                }
                let result = self.execute_tool_call(call, opts, &token).await;
                let (content, is_error) = match result {
                    Ok(r) => (r.content, r.is_error),
                    Err(e) => (format!("tool failed: {e}"), true),
                };
                let mut tool_msg =
                    ChatMessage::tool_result(call.id.clone(), call.name.clone(), content.clone());
                tool_msg.ts = Some(chrono::Utc::now().to_rfc3339());
                {
                    let mut history = self.history.lock().await;
                    history.push(tool_msg.clone());
                }
                {
                    let mut store = self.session.lock().await;
                    let _ = store.append_chat(&tool_msg);
                }
                let _ = is_error;
            }
            if token.is_cancelled() {
                break;
            }
        }

        {
            let mut store = self.session.lock().await;
            let _ = store.append_update(&SessionUpdate::TurnEnd {
                stop_reason: last_stop.as_acp().to_string(),
                ts: chrono::Utc::now().to_rfc3339(),
            });
            let _ = store.save_summary();
        }
        Ok(last_stop)
    }

    /// Set (or clear) the agent profile injected into the system prompt.
    pub async fn set_persona(&self, persona: Option<String>) {
        *self.persona.write().await = persona;
    }

    /// The profile in use, if any.
    pub async fn persona(&self) -> Option<String> {
        self.persona.read().await.clone()
    }

    /// Set (or clear) the session goal; it is persisted in the session summary.
    pub async fn set_goal(&self, goal: Option<String>) {
        *self.goal.write().await = goal.clone();
        if let Ok(mut store) = self.session.try_lock() {
            store.summary.goal = goal;
            let _ = store.save_summary();
        }
    }

    /// The current goal, if any.
    pub async fn goal(&self) -> Option<String> {
        self.goal.read().await.clone()
    }

    /// Run N verifier subagents against the goal: each one gets the goal plus a
    /// digest of what happened and answers whether the goal is met.
    pub async fn verify_goal(&self, count: usize) -> Vec<String> {
        let Some(goal) = self.goal().await else {
            let _ = self
                .tx
                .send(AgentEvent::Info("no goal set — use /goal <text>".into()));
            return Vec::new();
        };
        let history = self.history.lock().await.clone();
        let mut digest = String::new();
        for message in history
            .iter()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let text: String = message.content.chars().take(600).collect();
            digest.push_str(&format!("{}: {text}\n", message.role.as_str()));
        }
        let Some(runner) = self.ctx.subagents.get().cloned() else {
            let _ = self
                .tx
                .send(AgentEvent::Info("subagent runner unavailable".into()));
            return Vec::new();
        };
        let ctx = self.ctx_clone();
        let reports =
            crate::tools::task::verify_goal_with(runner, &ctx, &goal, &digest, count).await;
        for line in &reports {
            let _ = self.tx.send(AgentEvent::Info(format!("goal: {line}")));
        }
        reports
    }

    async fn build_system_prompt(&self) -> String {
        let skills = crate::skills::discover(&self.cwd, &self.config);
        let injected = self.injected_skills.lock().await.clone();
        let model = self.current_model().await;
        crate::prompt::build_system_prompt(&crate::prompt::PromptInputs {
            cwd: &self.cwd,
            config: &self.config,
            skills: &skills,
            extra_rules: &[],
            model_name: &model.model_id,
            agent_name: &self
                .config
                .agent
                .name
                .clone()
                .unwrap_or_else(|| "cachy".into()),
            agent_persona: self.persona.read().await.clone(),
            goal: self.goal.read().await.clone(),
            injected_skills: &injected,
        })
    }

    fn tool_specs(&self) -> Vec<crate::tools::ToolSpec> {
        let mut specs = self.tools.specs();
        // Surface MCP tools dynamically through search_tool/use_tool only.
        specs.retain(|s| s.name != "use_tool" || true);
        specs
    }

    async fn execute_tool_call(
        &self,
        call: &ToolCall,
        opts: &TurnOptions,
        token: &CancellationToken,
    ) -> Result<crate::tools::ToolResult> {
        let args = call.parsed_arguments();
        let tool = self
            .tools
            .get(&call.name)
            .ok_or_else(|| anyhow!("unknown tool \"{}\"", call.name))?;
        let kind = format!("{:?}", tool.kind()).to_lowercase();
        let title = tool.title(&args);

        let _ = self.tx.send(AgentEvent::ToolCall {
            id: call.id.clone(),
            name: tool.display_name(),
            title: title.clone(),
            kind,
            arguments: args.clone(),
        });

        // Permission check.
        let decision = self.engine.read().await.decide(tool.name(), &args);
        let allow = match decision {
            Decision::Allow => true,
            Decision::Deny => {
                let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                    id: call.id.clone(),
                    status: "failed".into(),
                    title: Some(title.clone()),
                    summary: Some("denied by permission rules".into()),
                    meta: None,
                });
                return Ok(crate::tools::ToolResult::err(
                    "permission denied by rule (--deny / permission mode)",
                ));
            }
            Decision::Ask => {
                if opts.interactive && self.interactive {
                    let (tx, rx) = oneshot::channel();
                    self.approvals.lock().await.insert(call.id.clone(), tx);
                    let options: Vec<String> = ApprovalChoice::ALL
                        .iter()
                        .map(|c| c.label().to_string())
                        .collect();
                    let detail = build_approval_detail(tool.name(), &args);
                    let _ = self.tx.send(AgentEvent::PermissionRequest {
                        id: call.id.clone(),
                        tool: tool.display_name(),
                        title: title.clone(),
                        detail,
                        options,
                    });
                    let decision = tokio::select! {
                        d = rx => d.unwrap_or(ApprovalDecision::Cancelled),
                        _ = token.cancelled() => ApprovalDecision::Cancelled,
                    };
                    match decision {
                        ApprovalDecision::Choice(ApprovalChoice::AllowAlways) => {
                            self.engine.write().await.always_approve = true;
                            let _ = self.tx.send(AgentEvent::Info(
                                "Always-approve mode is on for this session.".into(),
                            ));
                            true
                        }
                        ApprovalDecision::Choice(ApprovalChoice::AllowSession) => {
                            let mut e = self.engine.write().await;
                            e.rules.allow.push(crate::permissions::Rule {
                                kind: crate::permissions::rule_kind_for_tool(tool.name())
                                    .unwrap_or(crate::permissions::RuleKind::Any),
                                matcher: None,
                                domain: None,
                                raw: format!("{} (session)", tool.name()),
                            });
                            true
                        }
                        ApprovalDecision::Choice(ApprovalChoice::Allow) => true,
                        ApprovalDecision::Choice(ApprovalChoice::Reject)
                        | ApprovalDecision::RejectWithFeedback(_) => {
                            let feedback = match decision {
                                ApprovalDecision::RejectWithFeedback(f) => f,
                                _ => String::new(),
                            };
                            let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                                id: call.id.clone(),
                                status: "failed".into(),
                                title: Some(title.clone()),
                                summary: Some("rejected by user".into()),
                                meta: None,
                            });
                            return Ok(crate::tools::ToolResult::err(format!(
                                "The user rejected this tool call{}. Do not retry it as-is; ask how to proceed.",
                                if feedback.is_empty() {
                                    String::new()
                                } else {
                                    format!(" and said: \"{feedback}\"")
                                }
                            )));
                        }
                        ApprovalDecision::Cancelled => {
                            return Ok(crate::tools::ToolResult::err("tool call cancelled"));
                        }
                    }
                } else {
                    // Non-interactive: materialize the same decision headless
                    // sessions expect: deny with a clear hint.
                    let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                        id: call.id.clone(),
                        status: "failed".into(),
                        title: Some(title.clone()),
                        summary: Some("approval required".into()),
                        meta: None,
                    });
                    return Ok(crate::tools::ToolResult::err(
                        "This tool needs approval but the session is not interactive. \
                         Re-run with --always-approve, or allow it with --allow \"Tool(pattern)\".",
                    ));
                }
            }
        };

        if !allow {
            return Ok(crate::tools::ToolResult::err("tool call not permitted"));
        }

        let _ = self.tx.send(AgentEvent::ToolCallUpdate {
            id: call.id.clone(),
            status: "in_progress".into(),
            title: Some(title.clone()),
            summary: None,
            meta: None,
        });

        // A PreToolUse hook that fails refuses the call: the reason goes back
        // to the model instead of the tool running.
        if !self.hooks.is_empty() {
            let results = crate::hooks::run_event(
                &self.hooks,
                crate::hooks::HookEvent::PreToolUse,
                &self.cwd,
                Some(&call.name),
                &args,
            )
            .await;
            let refused: Vec<String> = results
                .iter()
                .filter(|(_, _, ok)| !ok)
                .map(|(command, output, _)| format!("{command}: {output}"))
                .collect();
            if !refused.is_empty() {
                let _ = self.tx.send(AgentEvent::ToolCallUpdate {
                    id: call.id.clone(),
                    status: "failed".into(),
                    title: Some(title.clone()),
                    summary: Some("refused by a hook".into()),
                    meta: None,
                });
                return Ok(crate::tools::ToolResult::err(format!(
                    "A PreToolUse hook refused this tool call — {}",
                    refused.join("; ")
                )));
            }
        }

        let ctx = self.ctx_clone();
        let started = std::time::Instant::now();
        let result = tokio::select! {
            r = tool.run(args.clone(), &ctx) => r,
            _ = token.cancelled() => crate::tools::ToolResult::err("cancelled"),
        };

        // PostToolUse hooks see the outcome; a failure there is reported but
        // does not change the result.
        if !self.hooks.is_empty() {
            let payload = serde_json::json!({
                "result": result.content.chars().take(2000).collect::<String>(),
                "is_error": result.is_error,
            });
            let results = crate::hooks::run_event(
                &self.hooks,
                crate::hooks::HookEvent::PostToolUse,
                &self.cwd,
                Some(&call.name),
                &payload,
            )
            .await;
            for (command, output, ok) in results {
                if !ok {
                    let _ = self.tx.send(AgentEvent::Info(format!(
                        "PostToolUse hook failed: {command}: {output}"
                    )));
                }
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        let status = crate::tools::status_of(&result);
        let summary = summarize_result(&result, elapsed);
        let _ = self.tx.send(AgentEvent::ToolCallUpdate {
            id: call.id.clone(),
            status: status.into(),
            title: Some(title),
            summary: Some(summary),
            meta: result.meta.clone(),
        });
        {
            let store = self.session.lock().await;
            let _ = store.append_update(&SessionUpdate::ToolCallUpdate {
                id: call.id.clone(),
                status: status.into(),
                content: Some(truncate(&result.content, 2000)),
                ts: chrono::Utc::now().to_rfc3339(),
            });
        }
        Ok(result)
    }

    async fn run_subagent_inner(
        &self,
        request: SubagentRequest,
        parent: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<SubagentOutcome> {
        // Queue behind the concurrency limit instead of stampeding.
        let _slot = self
            .subagent_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("subagent slots closed"))?;
        // Build a fresh session for the child.
        let model = match &request.model {
            Some(name) => crate::models::resolve(&self.config, Some(name))
                .ok_or_else(|| anyhow!("unknown subagent model {name}"))?,
            None => self.current_model().await,
        };
        let session = SessionStore::create(
            &self.cwd,
            &model.model_id,
            &request.subagent_type,
            Some(&self.session_id().await),
        )?;
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();

        // Restricted toolset for read-only subagent types.
        let mut tools = ToolRegistry::default_registry(&self.config);
        match request.subagent_type.as_str() {
            "explore" => {
                tools.filter(
                    Some(&[
                        "read_file".into(),
                        "grep".into(),
                        "glob".into(),
                        "list_dir".into(),
                    ]),
                    &[],
                );
            }
            "plan" => {
                tools.filter(
                    Some(&[
                        "read_file".into(),
                        "grep".into(),
                        "glob".into(),
                        "list_dir".into(),
                        "todo_write".into(),
                    ]),
                    &[],
                );
            }
            _ => {}
        }
        if let Some(enabled) = self.config.subagents.toggle.get(&request.subagent_type) {
            if !enabled {
                anyhow::bail!(
                    "subagent type \"{}\" is disabled in config",
                    request.subagent_type
                );
            }
        }

        let child = NativeAgent::new(
            self.cwd.clone(),
            self.config.clone(),
            model,
            session,
            parent.mcp.clone(),
            tools,
            PermissionEngine::new(
                RuleSet::default(),
                // Subagents inherit always-approve so they never deadlock on prompts.
                crate::permissions::PermissionMode::AcceptEdits,
                true,
            ),
            tx,
            self.max_turns,
            false,
            self.depth + 1,
        )
        .await?;

        let prompt = format!(
            "You are the \"{}\" subagent. Complete this task and return a final report.\n\nTask: {}\n\n{}",
            request.subagent_type, request.description, request.prompt
        );
        let child_id = child.session_id().await;
        let run = child.run_turn(
            prompt,
            TurnOptions {
                max_turns: Some(self.max_turns),
                interactive: false,
            },
        );
        let outcome = tokio::select! {
            r = run => r,
            _ = cancel.cancelled() => {
                child.cancel_current();
                Err(anyhow!("subagent cancelled"))
            }
        };
        // Collect text from events.
        let mut text = String::new();
        let mut usage = TokenUsage::default();
        while let Ok(event) = rx.try_recv() {
            match event {
                AgentEvent::MessageComplete { text: t } => {
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(&t);
                }
                AgentEvent::Usage {
                    input_tokens,
                    output_tokens,
                    ..
                } => {
                    usage.input_tokens = input_tokens;
                    usage.output_tokens = output_tokens;
                }
                _ => {}
            }
        }
        let is_error = outcome.is_err();
        if text.trim().is_empty() {
            text = match &outcome {
                Ok(stop) => format!("subagent finished ({})", stop.as_acp()),
                Err(e) => format!("subagent failed: {e}"),
            };
        }
        Ok(SubagentOutcome {
            text,
            session_id: child_id,
            usage,
            is_error,
        })
    }
}

fn summarize_result(result: &crate::tools::ToolResult, elapsed: f64) -> String {
    let first = result
        .content
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let summary: String = first.chars().take(120).collect();
    format!("{summary} ({elapsed:.1}s)")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

fn build_approval_detail(tool: &str, args: &Value) -> String {
    match tool {
        "run_terminal_cmd" | "bash" => args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "write" => {
            let path = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            format!("Write {path}")
        }
        "search_replace" => {
            let path = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            format!("Edit {path}")
        }
        "web_fetch" => args
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => args.to_string(),
    }
}

#[async_trait]
impl AgentSession for NativeAgent {
    async fn prompt(&self, text: String) -> Result<StopReason> {
        self.run_turn(
            text,
            TurnOptions {
                max_turns: None,
                interactive: self.interactive,
            },
        )
        .await
    }
    fn cancel(&self) {
        self.cancel_current();
    }
    fn session_id(&self) -> String {
        self.session
            .try_lock()
            .map(|s| s.summary.id.clone())
            .unwrap_or_default()
    }
    fn model_label(&self) -> String {
        self.model_swap
            .try_lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| self.model.clone())
            .display()
            .to_string()
    }
    fn context_usage(&self) -> (u64, u64) {
        let usage = self.usage.try_read().map(|u| *u).unwrap_or_default();
        let ctx = self
            .model_swap
            .try_lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| self.model.clone())
            .context_window;
        (usage.total(), ctx)
    }
    async fn shutdown(&self) {
        self.cancel_current();
        self.ctx.mcp.shutdown().await;
    }
}

#[async_trait]
impl SubagentRunner for NativeAgent {
    async fn run_subagent(
        &self,
        request: SubagentRequest,
        parent: &ToolContext,
        cancel: CancellationToken,
    ) -> SubagentOutcome {
        match self.run_subagent_inner(request, parent, cancel).await {
            Ok(outcome) => outcome,
            Err(e) => SubagentOutcome {
                text: format!("subagent failed: {e}"),
                session_id: String::new(),
                usage: TokenUsage::default(),
                is_error: true,
            },
        }
    }
}
