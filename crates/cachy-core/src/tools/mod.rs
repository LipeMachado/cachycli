//! Built-in tool system: definitions, registry, execution context.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::Config;
use crate::mcp::McpManager;

pub mod bash;
pub mod fs;
pub mod mcp_tool;
pub mod task;
pub mod todo;
pub mod web;

/// Result of a tool invocation.
#[derive(Debug, Clone, Default)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    /// Structured metadata for the TUI (e.g. diff previews, todo lists).
    pub meta: Option<Value>,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            meta: None,
        }
    }
    pub fn with_meta(content: impl Into<String>, meta: Value) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            meta: Some(meta),
        }
    }
    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            meta: None,
        }
    }
}

/// How a tool is classified for permissions and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Bash,
    Read,
    Edit,
    Write,
    Search,
    Web,
    Todo,
    Task,
    Mcp,
}

/// Context handed to every tool execution.
#[derive(Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub config: Arc<Config>,
    pub tasks: Arc<TaskRegistry>,
    pub todos: Arc<RwLock<Vec<TodoItem>>>,
    pub mcp: Arc<McpManager>,
    /// Channel used to surface nested events (todos, subagent progress).
    pub events: Option<mpsc::UnboundedSender<crate::agent::AgentEvent>>,
    /// Runner used by the `task` tool to spawn nested agent sessions.
    pub subagents: Arc<std::sync::OnceLock<Arc<dyn task::SubagentRunner>>>,
    /// Nesting level: 0 for the main agent, +1 per subagent.
    pub depth: usize,
    /// The session goal (`/goal`, `set_goal`): shared with the agent so tools
    /// can set it too.
    pub goal: Arc<RwLock<Option<String>>>,
}

impl ToolContext {
    pub fn new(cwd: PathBuf, config: Arc<Config>, mcp: Arc<McpManager>) -> Self {
        Self {
            cwd,
            config,
            tasks: Arc::new(TaskRegistry::default()),
            todos: Arc::new(RwLock::new(Vec::new())),
            mcp,
            events: None,
            subagents: Arc::new(std::sync::OnceLock::new()),
            depth: 0,
            goal: Arc::new(RwLock::new(None)),
        }
    }

    /// Clone with shared registries, as nested agents need.
    pub fn clone_shared(&self) -> Self {
        Self {
            cwd: self.cwd.clone(),
            config: self.config.clone(),
            tasks: self.tasks.clone(),
            todos: self.todos.clone(),
            mcp: self.mcp.clone(),
            events: self.events.clone(),
            subagents: self.subagents.clone(),
            depth: self.depth,
            goal: self.goal.clone(),
        }
    }

    pub fn with_tasks(mut self, tasks: Arc<TaskRegistry>) -> Self {
        self.tasks = tasks;
        self
    }
}

/// A tool the model can call.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    /// Name shown to the model (some tools are displayed under a different name).
    fn display_name(&self) -> String {
        self.name().to_string()
    }
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn kind(&self) -> ToolKind;
    /// One-line title for the TUI, e.g. `Edit src/main.rs`.
    fn title(&self, args: &Value) -> String {
        format!(
            "{} {}",
            self.display_name(),
            short_target(args, &["file_path", "path", "command", "query", "url"])
        )
    }
    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult;
}

/// Extract a short target string for display titles.
pub fn short_target(args: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = args.get(*key).and_then(|v| v.as_str()) {
            let first_line = v.lines().next().unwrap_or("");
            let truncated: String = first_line.chars().take(60).collect();
            if !truncated.trim().is_empty() {
                return truncated;
            }
        }
    }
    String::new()
}

/// How the interface labels a finished call: a command that ran but exited
/// non-zero is a failure even though the tool itself answered.
pub fn status_of(result: &ToolResult) -> &'static str {
    if result.is_error {
        return "failed";
    }
    let exit_code = result
        .meta
        .as_ref()
        .and_then(|meta| meta.get("exit_code"))
        .and_then(|code| code.as_i64());
    match exit_code {
        Some(0) | None => "completed",
        Some(_) => "failed",
    }
}

/// OpenAI-style function spec for a tool.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .iter()
            .find(|t| t.name() == name || t.display_name() == name)
            .cloned()
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            })
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name().to_string()).collect()
    }

    /// Build the default registry honoring config feature flags.
    pub fn default_registry(config: &Config) -> Self {
        let mut r = Self::new();
        r.register(Arc::new(bash::BashTool));
        r.register(Arc::new(fs::ReadFileTool));
        r.register(Arc::new(fs::WriteTool));
        r.register(Arc::new(fs::SearchReplaceTool));
        r.register(Arc::new(fs::ListDirTool));
        r.register(Arc::new(fs::GrepTool));
        r.register(Arc::new(fs::GlobTool));
        r.register(Arc::new(todo::TodoWriteTool));
        r.register(Arc::new(mcp_tool::SearchTool));
        r.register(Arc::new(mcp_tool::UseTool));
        if config.features.web_fetch {
            r.register(Arc::new(web::WebFetchTool));
        }
        if config.features.web_search {
            r.register(Arc::new(web::WebSearchTool));
        }
        if config.subagents.enabled {
            r.register(Arc::new(task::SetGoalTool));
            r.register(Arc::new(task::PlanWorkflowTool));
            r.register(Arc::new(task::RunWorkflowTool));
            r.register(Arc::new(task::TaskTool));
            r.register(Arc::new(task::GetTaskOutputTool));
            r.register(Arc::new(task::WaitTasksTool));
            r.register(Arc::new(task::KillTaskTool));
        }
        r
    }

    /// Apply `--tools` (allowlist) then `--disallowed-tools` (denylist).
    /// Returns warnings for entries that matched nothing.
    pub fn filter(&mut self, allow: Option<&[String]>, deny: &[String]) -> Vec<String> {
        let mut warnings = Vec::new();
        if let Some(allow) = allow {
            let wanted: Vec<String> = allow.iter().map(|s| normalize_tool_name(s)).collect();
            let mut kept = Vec::new();
            for tool in self.tools.drain(..) {
                if wanted.iter().any(|w| w == tool.name()) {
                    kept.push(tool);
                }
            }
            for w in &wanted {
                if !kept.iter().any(|t| t.name() == w) {
                    warnings.push(format!("--tools entry \"{w}\" does not name a tool"));
                }
            }
            self.tools = kept;
        }
        for entry in deny {
            let normalized = normalize_tool_name(entry);
            if normalized.starts_with("agent(") {
                // Subagent type restrictions are enforced by the task tool.
                continue;
            }
            let before = self.tools.len();
            self.tools
                .retain(|t| t.name() != normalized && t.display_name() != normalized);
            if self.tools.len() == before {
                warnings.push(format!(
                    "--disallowed-tools entry \"{entry}\" does not name a tool"
                ));
            }
        }
        warnings
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Map the model-facing names to internal tool ids.
pub fn normalize_tool_name(name: &str) -> String {
    match name.trim() {
        "bash" | "run_terminal_command" | "Shell" => "run_terminal_cmd",
        "read" | "Read" => "read_file",
        "write" | "Write" => "write",
        "edit" | "StrReplace" | "EditNotebook" => "search_replace",
        "grep_search" | "Grep" => "grep",
        "Glob" => "glob",
        "list_dir" => "list_dir",
        "TodoWrite" => "todo_write",
        "WebSearch" => "web_search",
        "WebFetch" => "web_fetch",
        "spawn_subagent" => "task",
        "get_command_or_subagent_output" => "get_task_output",
        "wait_commands_or_subagents" => "wait_tasks",
        "kill_command_or_subagent" => "kill_task",
        other => other,
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Task registry: background processes and subagents.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// A background task tracked by the registry.
pub struct BackgroundTask {
    pub id: String,
    pub kind: TaskKind,
    pub description: String,
    pub started_at: std::time::Instant,
    /// When it reached a terminal state, so panels show the duration instead of
    /// a clock that keeps ticking.
    pub finished_at: std::sync::Mutex<Option<std::time::Instant>>,
    pub output: Arc<RwLock<String>>,
    pub status: Arc<RwLock<TaskStatus>>,
    /// Cancel handle for the running task.
    pub cancel: tokio_util::sync::CancellationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    Command,
    Subagent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
    Killed,
}

impl BackgroundTask {
    /// Seconds this task ran (or has been running).
    pub fn elapsed_secs(&self) -> f64 {
        match self.finished_at.lock().ok().and_then(|guard| *guard) {
            Some(end) => end.duration_since(self.started_at).as_secs_f64(),
            None => self.started_at.elapsed().as_secs_f64(),
        }
    }

    /// Move to a terminal state and freeze the clock.
    pub async fn finish(&self, status: TaskStatus) {
        *self.status.write().await = status;
        if let Ok(mut guard) = self.finished_at.lock() {
            *guard = Some(std::time::Instant::now());
        }
    }
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed(_) => "failed",
            TaskStatus::Killed => "killed",
        }
    }
}

#[derive(Default)]
pub struct TaskRegistry {
    tasks: Mutex<HashMap<String, Arc<BackgroundTask>>>,
    counter: std::sync::atomic::AtomicU64,
}

impl TaskRegistry {
    pub fn next_id(&self, prefix: &str) -> String {
        let n = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        format!("{prefix}-{n}")
    }

    pub async fn insert(&self, task: Arc<BackgroundTask>) -> String {
        let id = task.id.clone();
        self.tasks.lock().await.insert(id.clone(), task);
        id
    }

    pub async fn get(&self, id: &str) -> Option<Arc<BackgroundTask>> {
        self.tasks.lock().await.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<Arc<BackgroundTask>> {
        let mut out: Vec<_> = self.tasks.lock().await.values().cloned().collect();
        out.sort_by_key(|t| t.started_at);
        out
    }

    pub async fn running(&self) -> Vec<Arc<BackgroundTask>> {
        let tasks = self.tasks.lock().await;
        let mut out = Vec::new();
        for task in tasks.values() {
            let running = match task.status.try_read() {
                Ok(status) => *status == TaskStatus::Running,
                Err(_) => true,
            };
            if running {
                out.push(task.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod registry_tests {

    #[tokio::test]
    async fn elapsed_is_frozen_once_finished() {
        let registry = TaskRegistry::default();
        let task = std::sync::Arc::new(BackgroundTask {
            id: registry.next_id("cmd"),
            kind: TaskKind::Command,
            description: "work".into(),
            started_at: std::time::Instant::now(),
            finished_at: std::sync::Mutex::new(None),
            output: std::sync::Arc::new(tokio::sync::RwLock::new(String::new())),
            status: std::sync::Arc::new(tokio::sync::RwLock::new(TaskStatus::Running)),
            cancel: tokio_util::sync::CancellationToken::new(),
        });
        let id = registry.insert(task.clone()).await;
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(task.elapsed_secs() >= 0.1, "a running task counts up");
        task.finish(TaskStatus::Completed).await;
        let frozen = task.elapsed_secs();
        assert!(
            frozen >= 0.1,
            "the finished task keeps its duration: {frozen}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        assert!(
            (task.elapsed_secs() - frozen).abs() < 0.02,
            "the clock stops when the task ends"
        );
        assert!(registry.get(&id).await.is_some());
    }

    #[tokio::test]
    async fn killing_a_task_cancels_its_token() {
        let registry = TaskRegistry::default();
        let cancel = tokio_util::sync::CancellationToken::new();
        let task = std::sync::Arc::new(BackgroundTask {
            id: registry.next_id("cmd"),
            kind: TaskKind::Command,
            description: "sleep 30".into(),
            started_at: std::time::Instant::now(),
            finished_at: std::sync::Mutex::new(None),
            output: std::sync::Arc::new(tokio::sync::RwLock::new(String::new())),
            status: std::sync::Arc::new(tokio::sync::RwLock::new(TaskStatus::Running)),
            cancel: cancel.clone(),
        });
        let id = registry.insert(task).await;
        // This is exactly what the `/tasks` panel does on `x`.
        let fetched = registry.get(&id).await.expect("task");
        fetched.cancel.cancel();
        assert!(cancel.is_cancelled(), "the panel's kill cancels the token");
        // The task itself notices the cancellation and marks itself done.
        *fetched.status.write().await = TaskStatus::Killed;
        assert_eq!(
            registry.running().await.len(),
            0,
            "a killed task is not running"
        );
    }

    use super::*;

    #[test]
    fn workflow_tool_is_offered_to_the_model() {
        let config = crate::config::Config::default();
        let registry = ToolRegistry::default_registry(&config);
        assert!(
            registry.get("run_workflow").is_some(),
            "run_workflow must be registered"
        );
        assert!(
            registry.get("plan_workflow").is_some(),
            "plan_workflow must be registered"
        );
        assert!(
            registry.get("set_goal").is_some(),
            "set_goal must be registered"
        );
        // And it disappears with subagents disabled, like the other task tools.
        let mut off = crate::config::Config::default();
        off.subagents.enabled = false;
        let registry = ToolRegistry::default_registry(&off);
        assert!(registry.get("run_workflow").is_none());
    }
}
