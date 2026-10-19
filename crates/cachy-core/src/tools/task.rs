//! Subagent tools: `task`, `get_task_output`, `wait_tasks`, `kill_task`.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{BackgroundTask, TaskKind, TaskStatus, Tool, ToolContext, ToolKind, ToolResult};
use crate::message::TokenUsage;

/// Request to spawn a subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentRequest {
    /// `general-purpose`, `explore`, `plan`, or a custom profile name.
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
    /// Isolate the subagent from the parent conversation.
    pub isolated: bool,
    /// Optional model override.
    pub model: Option<String>,
}

/// Outcome of a subagent run.
#[derive(Debug, Clone)]
pub struct SubagentOutcome {
    pub text: String,
    pub session_id: String,
    pub usage: TokenUsage,
    pub is_error: bool,
}

/// Implemented by the agent engine to run nested sessions.
#[async_trait]
pub trait SubagentRunner: Send + Sync {
    async fn run_subagent(
        &self,
        request: SubagentRequest,
        parent: &ToolContext,
        cancel: tokio_util::sync::CancellationToken,
    ) -> SubagentOutcome;
}

/// Run every step of a workflow as a subagent, at most `max_concurrent` at a
/// time, registering each one so `/tasks` shows progress. Returns one report
/// line per step, in order.
pub async fn run_workflow_steps(
    runner: std::sync::Arc<dyn SubagentRunner>,
    ctx: &ToolContext,
    workflow: &crate::workflows::Workflow,
) -> Vec<String> {
    let mut handles = Vec::new();
    for (index, step) in workflow.steps.iter().enumerate() {
        let request = SubagentRequest {
            subagent_type: "general-purpose".into(),
            description: format!("{} · {}", workflow.name, step.title),
            prompt: step.prompt.clone(),
            isolated: true,
            model: None,
        };
        let ctx = ctx.clone();
        let runner = runner.clone();
        let cancel = tokio_util::sync::CancellationToken::new();
        let task = std::sync::Arc::new(crate::tools::BackgroundTask {
            id: ctx.tasks.next_id("wf"),
            kind: crate::tools::TaskKind::Subagent,
            description: format!("{} · {}", workflow.name, step.title),
            started_at: std::time::Instant::now(),
            finished_at: std::sync::Mutex::new(None),
            output: std::sync::Arc::new(tokio::sync::RwLock::new(String::new())),
            status: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::tools::TaskStatus::Running,
            )),
            cancel: cancel.clone(),
        });
        let _ = ctx.tasks.insert(task.clone()).await;
        handles.push((
            index,
            step.title.clone(),
            tokio::spawn(async move {
                let outcome = runner.run_subagent(request, &ctx, cancel).await;
                *task.output.write().await = outcome.text.clone();
                task.finish(if outcome.is_error {
                    crate::tools::TaskStatus::Failed(outcome.text.chars().take(200).collect())
                } else {
                    crate::tools::TaskStatus::Completed
                })
                .await;
                outcome
            }),
        ));
    }
    let mut reports: Vec<(usize, String, String)> = Vec::new();
    for (index, title, handle) in handles {
        match handle.await {
            Ok(outcome) if !outcome.is_error => reports.push((index, title, outcome.text)),
            Ok(outcome) => reports.push((index, title, format!("failed: {}", outcome.text))),
            Err(e) => reports.push((index, title, format!("failed: {e}"))),
        }
    }
    reports.sort_by_key(|(index, _, _)| *index);
    reports
        .into_iter()
        .map(|(_, title, text)| {
            let first: String = text.lines().take(3).collect::<Vec<_>>().join(" ");
            format!("{title}: {}", first.chars().take(200).collect::<String>())
        })
        .collect()
}

/// Run `count` verifier subagents against a goal. Shared by `/goal verify` and
/// the `set_goal` tool. `digest` is what the verifiers are told happened so far
/// (the repository itself is the source of truth).
pub async fn verify_goal_with(
    runner: std::sync::Arc<dyn SubagentRunner>,
    ctx: &ToolContext,
    goal: &str,
    digest: &str,
    count: usize,
) -> Vec<String> {
    let count = count.max(1);
    let mut handles = Vec::new();
    for index in 0..count {
        let request = SubagentRequest {
            subagent_type: "explore".into(),
            description: format!("verify goal ({}/{})", index + 1, count),
            prompt: format!(
                "Goal:\n{goal}\n\nWhat happened so far:\n{digest}\n\n\
                 Check the goal against the actual state of the repository (read files, \
                 run read-only commands). Answer with: VERDICT (met / not met / unclear), \
                 what you verified, and exactly what is still missing."
            ),
            isolated: true,
            model: None,
        };
        let ctx = ctx.clone();
        let runner = runner.clone();
        let cancel = tokio_util::sync::CancellationToken::new();
        let task = std::sync::Arc::new(crate::tools::BackgroundTask {
            id: ctx.tasks.next_id("goal"),
            kind: crate::tools::TaskKind::Subagent,
            description: format!("verify goal ({}/{})", index + 1, count),
            started_at: std::time::Instant::now(),
            finished_at: std::sync::Mutex::new(None),
            output: std::sync::Arc::new(tokio::sync::RwLock::new(String::new())),
            status: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::tools::TaskStatus::Running,
            )),
            cancel: cancel.clone(),
        });
        let _ = ctx.tasks.insert(task.clone()).await;
        handles.push(tokio::spawn(async move {
            let outcome = runner.run_subagent(request, &ctx, cancel).await;
            *task.output.write().await = outcome.text.clone();
            task.finish(if outcome.is_error {
                crate::tools::TaskStatus::Failed(outcome.text.chars().take(200).collect())
            } else {
                crate::tools::TaskStatus::Completed
            })
            .await;
            outcome.text
        }));
    }
    let mut reports = Vec::new();
    for (index, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(text) => {
                let first: String = text.lines().take(4).collect::<Vec<_>>().join(" ");
                reports.push(format!(
                    "verifier {}: {}",
                    index + 1,
                    first.chars().take(220).collect::<String>()
                ));
            }
            Err(e) => reports.push(format!("verifier {} failed: {e}", index + 1)),
        }
    }
    reports
}

/// One step of a plan the model wrote itself.
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub title: String,
    pub prompt: String,
    /// 1-based indices of the steps this one needs first.
    pub depends_on: Vec<usize>,
}

/// Run a plan in waves: steps whose dependencies are done go together (the
/// runner's semaphore caps how many actually run at once), and each step sees
/// the reports of the steps it depends on.
pub async fn run_plan(
    runner: std::sync::Arc<dyn SubagentRunner>,
    ctx: &ToolContext,
    plan_name: &str,
    steps: &[PlanStep],
) -> Vec<String> {
    let mut done: std::collections::BTreeMap<usize, String> = Default::default();
    let mut reports: Vec<(usize, String)> = Vec::new();
    let mut remaining: Vec<usize> = (0..steps.len()).collect();

    while !remaining.is_empty() {
        // Steps whose dependencies are all finished.
        let ready: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|index| {
                steps[*index]
                    .depends_on
                    .iter()
                    .all(|dep| done.contains_key(&(dep.saturating_sub(1))))
            })
            .collect();
        let ready = if ready.is_empty() {
            // A cycle (or a bad index): run the rest in order rather than hang.
            vec![remaining[0]]
        } else {
            ready
        };

        let mut handles = Vec::new();
        for index in ready.iter().copied() {
            let step = &steps[index];
            let mut prompt = step.prompt.clone();
            for dep in &step.depends_on {
                if let Some(text) = done.get(&(dep.saturating_sub(1))) {
                    prompt.push_str(&format!(
                        "\n\nReport from step {dep} ({}):\n{}",
                        steps[dep.saturating_sub(1)].title,
                        text.chars().take(2_000).collect::<String>()
                    ));
                }
            }
            let request = SubagentRequest {
                subagent_type: "general-purpose".into(),
                description: format!("{plan_name} · {}", step.title),
                prompt,
                isolated: true,
                model: None,
            };
            let ctx = ctx.clone();
            let runner = runner.clone();
            let cancel = tokio_util::sync::CancellationToken::new();
            let task = std::sync::Arc::new(crate::tools::BackgroundTask {
                id: ctx.tasks.next_id("plan"),
                kind: crate::tools::TaskKind::Subagent,
                description: format!("{plan_name} · {}", step.title),
                started_at: std::time::Instant::now(),
                finished_at: std::sync::Mutex::new(None),
                output: std::sync::Arc::new(tokio::sync::RwLock::new(String::new())),
                status: std::sync::Arc::new(tokio::sync::RwLock::new(
                    crate::tools::TaskStatus::Running,
                )),
                cancel: cancel.clone(),
            });
            let _ = ctx.tasks.insert(task.clone()).await;
            handles.push((
                index,
                tokio::spawn(async move {
                    let outcome = runner.run_subagent(request, &ctx, cancel).await;
                    *task.output.write().await = outcome.text.clone();
                    task.finish(if outcome.is_error {
                        crate::tools::TaskStatus::Failed(outcome.text.chars().take(200).collect())
                    } else {
                        crate::tools::TaskStatus::Completed
                    })
                    .await;
                    outcome.text
                }),
            ));
        }

        for (index, handle) in handles {
            let text = match handle.await {
                Ok(text) => text,
                Err(e) => format!("failed: {e}"),
            };
            done.insert(index, text.clone());
            let first: String = text.lines().take(3).collect::<Vec<_>>().join(" ");
            reports.push((
                index,
                format!(
                    "{}: {}",
                    steps[index].title,
                    first.chars().take(200).collect::<String>()
                ),
            ));
            remaining.retain(|value| *value != index);
        }
    }

    reports.sort_by_key(|(index, _)| *index);
    reports.into_iter().map(|(_, line)| line).collect()
}

/// `plan_workflow`: the model writes the plan and it runs as subagents.
pub struct PlanWorkflowTool;

#[async_trait::async_trait]
impl crate::tools::Tool for PlanWorkflowTool {
    fn name(&self) -> &'static str {
        "plan_workflow"
    }

    fn description(&self) -> &'static str {
        "Break a job into steps and run them as subagents. Each step gets its own \
         prompt; steps that list `depends_on` wait for those steps and receive \
         their reports, so later steps build on earlier findings. Use it for \
         multi-phase work (investigate → change → verify). Set `save` to also \
         store the plan as a reusable workflow under .cachy/workflows."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Short name for this plan" },
                "steps": {
                    "type": "array",
                    "description": "1 to 8 steps, in the order you want them reported",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "prompt": { "type": "string" },
                            "depends_on": {
                                "type": "array",
                                "items": { "type": "integer" },
                                "description": "1-based step numbers this step needs first"
                            }
                        },
                        "required": ["title", "prompt"]
                    }
                },
                "save": {
                    "type": "boolean",
                    "description": "Also save the plan as .cachy/workflows/<name>.json"
                }
            },
            "required": ["name", "steps"]
        })
    }

    fn kind(&self) -> crate::tools::ToolKind {
        crate::tools::ToolKind::Task
    }

    fn title(&self, args: &Value) -> String {
        let count = args
            .get("steps")
            .and_then(|v| v.as_array())
            .map(|steps| steps.len())
            .unwrap_or(0);
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("plan");
        format!("plan_workflow {name} ({count} steps)")
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("plan")
            .trim()
            .to_string();
        let Some(raw_steps) = args.get("steps").and_then(|v| v.as_array()) else {
            return ToolResult::err("missing required argument: steps");
        };
        if raw_steps.is_empty() {
            return ToolResult::err("the plan needs at least one step");
        }
        if raw_steps.len() > 8 {
            return ToolResult::err("at most 8 steps per plan");
        }
        let mut steps: Vec<PlanStep> = Vec::new();
        for (index, item) in raw_steps.iter().enumerate() {
            let Some(prompt) = item.get("prompt").and_then(|v| v.as_str()) else {
                return ToolResult::err(format!("step {} has no prompt", index + 1));
            };
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("step")
                .to_string();
            let mut depends_on: Vec<usize> = item
                .get("depends_on")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|value| value.as_u64())
                        .map(|value| value as usize)
                        .filter(|value| *value >= 1 && *value <= raw_steps.len())
                        .collect()
                })
                .unwrap_or_default();
            depends_on.sort_unstable();
            depends_on.dedup();
            steps.push(PlanStep {
                title,
                prompt: prompt.to_string(),
                depends_on,
            });
        }

        if args.get("save").and_then(|v| v.as_bool()).unwrap_or(false) {
            let dir = ctx.cwd.join(".cachy/workflows");
            let _ = std::fs::create_dir_all(&dir);
            let body = serde_json::json!({
                "name": name,
                "description": format!("Plan written by the agent ({})", steps.len()),
                "steps": steps.iter().map(|step| serde_json::json!({
                    "title": step.title,
                    "prompt": step.prompt,
                })).collect::<Vec<_>>(),
            });
            if let Ok(text) = serde_json::to_string_pretty(&body) {
                let _ = std::fs::write(dir.join(format!("{name}.json")), text);
            }
        }

        let Some(runner) = ctx.subagents.get().cloned() else {
            return ToolResult::err("subagent runner unavailable");
        };
        let reports = run_plan(runner, ctx, &name, &steps).await;
        ToolResult::ok(format!(
            "plan \"{name}\" finished ({} step(s)):\n{}",
            reports.len(),
            reports.join("\n")
        ))
    }
}

/// `set_goal`: lets the model keep the session goal up to date by itself.
pub struct SetGoalTool;

#[async_trait::async_trait]
impl crate::tools::Tool for SetGoalTool {
    fn name(&self) -> &'static str {
        "set_goal"
    }

    fn description(&self) -> &'static str {
        "Record what the session is trying to achieve. Use it when the user asks \
         for something multi-step (so the goal survives long turns), update it \
         when the goal shifts, and pass an empty string to clear it when it is \
         done. The goal is injected into your system prompt on every turn."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "goal": { "type": "string", "description": "The goal, or an empty string to clear it" },
                "verify": { "type": "boolean", "description": "Also run the verifier subagents now" }
            },
            "required": ["goal"]
        })
    }

    fn kind(&self) -> crate::tools::ToolKind {
        crate::tools::ToolKind::Task
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let raw = args.get("goal").and_then(|v| v.as_str()).unwrap_or("");
        let goal = raw.trim().to_string();
        let cleared = goal.is_empty();
        *ctx.goal.write().await = if cleared { None } else { Some(goal.clone()) };
        let mut out = if cleared {
            "goal cleared".to_string()
        } else {
            format!("goal set: {goal}")
        };
        if args
            .get("verify")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            if cleared {
                return ToolResult::err("cannot verify an empty goal");
            }
            let Some(runner) = ctx.subagents.get().cloned() else {
                return ToolResult::err("subagent runner unavailable");
            };
            let count = ctx.config.agent.goal_verifier_count.max(1);
            let reports = verify_goal_with(runner, ctx, &goal, "", count).await;
            out.push_str(&format!(
                "\nverification ({} subagent(s)):\n{}",
                reports.len(),
                reports.join("\n")
            ));
        }
        ToolResult::ok(out)
    }
}

/// `run_workflow`: lets the model run a named workflow by itself.
pub struct RunWorkflowTool;

#[async_trait::async_trait]
impl crate::tools::Tool for RunWorkflowTool {
    fn name(&self) -> &'static str {
        "run_workflow"
    }

    fn description(&self) -> &'static str {
        "Run a multi-step workflow (from .cachy/workflows) as parallel subagents. \
         Use it when a task has clearly separable phases; the steps run on their \
         own and you get one report line per step."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Workflow name" }
            },
            "required": ["name"]
        })
    }

    fn kind(&self) -> crate::tools::ToolKind {
        crate::tools::ToolKind::Task
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: name");
        };
        let workflows = crate::workflows::discover(&ctx.cwd);
        let Some(workflow) = workflows
            .iter()
            .find(|workflow| workflow.name.eq_ignore_ascii_case(name.trim()))
            .cloned()
        else {
            let available: Vec<String> = workflows.iter().map(|w| w.name.clone()).collect();
            return ToolResult::err(format!(
                "unknown workflow \"{name}\"; available: {}",
                if available.is_empty() {
                    "(none — add .cachy/workflows/<name>.md)".to_string()
                } else {
                    available.join(", ")
                }
            ));
        };
        let Some(runner) = ctx.subagents.get().cloned() else {
            return ToolResult::err("subagent runner unavailable");
        };
        let reports = run_workflow_steps(runner, ctx, &workflow).await;
        ToolResult::ok(format!(
            "workflow \"{}\" finished ({} step(s)):\n{}",
            workflow.name,
            reports.len(),
            reports.join("\n")
        ))
    }
}

/// Built-in subagent types and their guidance (shown to the model).
pub const SUBAGENT_TYPES: &[(&str, &str)] = &[
    (
        "general-purpose",
        "General multi-step work: research, refactors, running commands, editing files.",
    ),
    (
        "explore",
        "Fast read-only reconnaissance: locate code, answer questions about the repo. Cannot edit.",
    ),
    (
        "plan",
        "Produce an implementation plan without making changes.",
    ),
];

pub struct TaskTool;

#[async_trait]
impl Tool for TaskTool {
    fn name(&self) -> &str {
        "task"
    }
    fn display_name(&self) -> String {
        "spawn_subagent".into()
    }
    fn description(&self) -> &str {
        "Spawn a subagent session for a well-scoped task. Subagents have their \
         own context window and can work in parallel. Use subagent_type to pick \
         a profile (general-purpose, explore, plan). Set background=true for \
         long tasks and poll with get_task_output."
    }
    fn parameters(&self) -> Value {
        let types: Vec<Value> = SUBAGENT_TYPES
            .iter()
            .map(|(name, desc)| json!({ "type": "string", "const": name, "description": desc }))
            .collect();
        json!({
            "type": "object",
            "properties": {
                "description": { "type": "string", "description": "Short (3-5 word) task description" },
                "prompt": { "type": "string", "description": "Complete instructions for the subagent" },
                "subagent_type": { "oneOf": types },
                "background": { "type": "boolean", "description": "Run in background and return immediately" },
                "isolated": { "type": "boolean", "description": "Do not inherit the parent conversation context" },
                "model": { "type": "string", "description": "Model override for this subagent" }
            },
            "required": ["description", "prompt"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Task
    }
    fn title(&self, args: &Value) -> String {
        format!("Subagent: {}", super::short_target(args, &["description"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        // Nested subagents stop at `[subagents] max_depth`, so a runaway agent
        // cannot fan out forever.
        let max_depth = ctx.config.subagents.max_depth.max(1);
        if ctx.depth + 1 > max_depth {
            return ToolResult::err(format!(
                "subagent depth limit reached ({max_depth}); finish this task yourself instead of delegating again"
            ));
        }
        let Some(prompt) = args.get("prompt").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: prompt");
        };
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("subagent task")
            .to_string();
        let subagent_type = args
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general-purpose")
            .to_string();
        if !SUBAGENT_TYPES.iter().any(|(n, _)| n == &subagent_type) {
            // Allow custom profiles discovered from .cachy/agents; unknown
            // Unknown names fail closed: a tool that cannot be resolved is a
            // tool the model may not run.
            if !ctx.config.subagents.toggle.contains_key(&subagent_type)
                && !std::path::Path::new(".cachy/agents")
                    .join(format!("{subagent_type}.md"))
                    .is_file()
            {
                return ToolResult::err(format!(
                    "unknown subagent type \"{subagent_type}\"; known types: {}",
                    SUBAGENT_TYPES
                        .iter()
                        .map(|(n, _)| *n)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        let Some(runner) = ctx.subagents.get().cloned() else {
            return ToolResult::err("subagents are not available in this session");
        };
        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let request = SubagentRequest {
            subagent_type,
            description: description.clone(),
            prompt: prompt.to_string(),
            isolated: args
                .get("isolated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            model: args
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        };

        if background {
            let id = ctx.tasks.next_id("agent");
            let task = Arc::new(BackgroundTask {
                id: id.clone(),
                kind: TaskKind::Subagent,
                description: description.clone(),
                started_at: std::time::Instant::now(),
                finished_at: std::sync::Mutex::new(None),
                output: Arc::new(RwLock::new(String::new())),
                status: Arc::new(RwLock::new(TaskStatus::Running)),
                cancel: tokio_util::sync::CancellationToken::new(),
            });
            ctx.tasks.insert(task.clone()).await;
            let output = task.output.clone();
            let status = task.status.clone();
            let cancel = task.cancel.clone();
            let parent_ctx = clone_ctx(ctx);
            let events = ctx.events.clone();
            tokio::spawn(async move {
                let outcome = runner
                    .run_subagent(request, &parent_ctx, cancel.clone())
                    .await;
                *output.write().await = outcome.text.clone();
                *status.write().await = if outcome.is_error {
                    TaskStatus::Failed("subagent failed".into())
                } else {
                    TaskStatus::Completed
                };
                if let Some(tx) = events {
                    let _ = tx.send(crate::agent::AgentEvent::BackgroundFinished {
                        task_id: task.id.clone(),
                        description: task.description.clone(),
                        status: status.read().await.as_str().to_string(),
                    });
                }
            });
            return ToolResult::with_meta(
                format!("Subagent \"{description}\" started in the background with task id {id}."),
                json!({ "task_id": id, "background": true }),
            );
        }

        let outcome = runner
            .run_subagent(request, ctx, tokio_util::sync::CancellationToken::new())
            .await;
        ToolResult {
            content: outcome.text,
            is_error: outcome.is_error,
            meta: Some(json!({ "subagent_session": outcome.session_id, "usage": {
                "input_tokens": outcome.usage.input_tokens,
                "output_tokens": outcome.usage.output_tokens,
            }})),
        }
    }
}

fn clone_ctx(ctx: &ToolContext) -> ToolContext {
    ctx.clone_shared()
}

pub struct GetTaskOutputTool;

#[async_trait]
impl Tool for GetTaskOutputTool {
    fn name(&self) -> &str {
        "get_task_output"
    }
    fn display_name(&self) -> String {
        "get_command_or_subagent_output".into()
    }
    fn description(&self) -> &str {
        "Get the current status and output of a background command or subagent."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task id returned when the task started" },
                "tail": { "type": "integer", "description": "Only the last N lines of output" }
            },
            "required": ["task_id"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Task
    }
    fn title(&self, args: &Value) -> String {
        format!("Check task {}", super::short_target(args, &["task_id"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(id) = args.get("task_id").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: task_id");
        };
        let Some(task) = ctx.tasks.get(id).await else {
            let ids = ctx
                .tasks
                .list()
                .await
                .iter()
                .map(|t| format!("{} ({})", t.id, t.description))
                .collect::<Vec<_>>()
                .join(", ");
            return ToolResult::err(format!("unknown task id {id}. Known tasks: {ids}"));
        };
        let status = task.status.read().await.clone();
        let output = task.output.read().await.clone();
        let tail = args.get("tail").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let output = if tail > 0 {
            let lines: Vec<&str> = output.lines().collect();
            lines[lines.len().saturating_sub(tail)..].join("\n")
        } else {
            output
        };
        let elapsed = task.started_at.elapsed().as_secs_f64();
        let (output, truncated) = super::bash::truncate_text(&output, 32_768);
        let mut text = format!(
            "task {id} ({}) — {}\nelapsed: {elapsed:.1}s\n",
            task.description,
            status.as_str()
        );
        if let TaskStatus::Failed(err) = &status {
            text.push_str(&format!("error: {err}\n"));
        }
        if truncated {
            text.push_str("note: showing latest output\n");
        }
        text.push_str(&output);
        ToolResult::with_meta(
            text,
            json!({ "status": status.as_str(), "elapsed_secs": elapsed }),
        )
    }
}

pub struct WaitTasksTool;

#[async_trait]
impl Tool for WaitTasksTool {
    fn name(&self) -> &str {
        "wait_tasks"
    }
    fn display_name(&self) -> String {
        "wait_commands_or_subagents".into()
    }
    fn description(&self) -> &str {
        "Wait for one or more background tasks to finish (or a timeout). \
         Pass task_ids to wait for specific tasks, or omit to wait for all."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_ids": { "type": "array", "items": { "type": "string" } },
                "timeout_secs": { "type": "number", "description": "Give up after this many seconds (default 300)" }
            }
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Task
    }
    fn title(&self, _args: &Value) -> String {
        "Waiting for background tasks".into()
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let ids: Vec<String> = args
            .get("task_ids")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_f64())
            .unwrap_or(300.0);
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout.max(1.0));
        loop {
            let tasks = if ids.is_empty() {
                ctx.tasks.list().await
            } else {
                let mut out = Vec::new();
                for id in &ids {
                    if let Some(t) = ctx.tasks.get(id).await {
                        out.push(t);
                    }
                }
                out
            };
            let mut all_done = true;
            let mut summary = String::new();
            for task in &tasks {
                let status = task.status.read().await.clone();
                if status == TaskStatus::Running {
                    all_done = false;
                }
                summary.push_str(&format!(
                    "- {} ({}): {}\n",
                    task.id,
                    task.description,
                    status.as_str()
                ));
            }
            if all_done {
                let mut text = format!("All tasks finished:\n{summary}");
                for task in &tasks {
                    let out = task.output.read().await.clone();
                    if !out.is_empty() {
                        let (out, _) = super::bash::truncate_text(&out, 8192);
                        text.push_str(&format!("\n== {} output ==\n{out}\n", task.id));
                    }
                }
                return ToolResult::ok(text);
            }
            if std::time::Instant::now() >= deadline {
                return ToolResult::err(format!(
                    "timed out after {timeout}s waiting for tasks:\n{summary}"
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}

pub struct KillTaskTool;

#[async_trait]
impl Tool for KillTaskTool {
    fn name(&self) -> &str {
        "kill_task"
    }
    fn display_name(&self) -> String {
        "kill_command_or_subagent".into()
    }
    fn description(&self) -> &str {
        "Stop a running background command or subagent."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" }
            },
            "required": ["task_id"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Task
    }
    fn title(&self, args: &Value) -> String {
        format!("Kill task {}", super::short_target(args, &["task_id"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(id) = args.get("task_id").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: task_id");
        };
        let Some(task) = ctx.tasks.get(id).await else {
            return ToolResult::err(format!("unknown task id {id}"));
        };
        task.cancel.cancel();
        *task.status.write().await = TaskStatus::Killed;
        ToolResult::ok(format!("Task {id} stopped."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolContext;

    fn context(depth: usize, max_depth: usize) -> ToolContext {
        let mut config = crate::config::Config::default();
        config.subagents.max_depth = max_depth;
        let mut ctx = ToolContext::new(
            std::env::temp_dir(),
            std::sync::Arc::new(config),
            std::sync::Arc::new(crate::mcp::McpManager::new(Default::default(), "test")),
        );
        ctx.depth = depth;
        ctx
    }

    #[tokio::test]
    async fn depth_limit_stops_nested_delegation() {
        let tool = TaskTool;
        let args = serde_json::json!({ "prompt": "do something", "description": "x" });
        // depth 1 with max_depth 2 -> gets past the guard (it may fail later
        // for lack of a runner, which is not what this test is about)
        let allowed = tool.run(args.clone(), &context(1, 2)).await;
        assert!(
            !allowed.content.contains("depth limit"),
            "the guard fired too early: {}",
            allowed.content
        );
        // depth 2 with max_depth 2 -> refused by the guard
        let refused = tool.run(args, &context(2, 2)).await;
        assert!(refused.is_error);
        assert!(
            refused.content.contains("depth limit"),
            "{}",
            refused.content
        );
    }
}

#[cfg(test)]
mod goal_tool_tests {
    use super::*;
    use crate::tools::{Tool, ToolContext};

    struct StubRunner;

    #[async_trait::async_trait]
    impl SubagentRunner for StubRunner {
        async fn run_subagent(
            &self,
            _request: SubagentRequest,
            _parent: &ToolContext,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> SubagentOutcome {
            SubagentOutcome {
                text: "VERDICT: met\nchecked the repo".into(),
                session_id: "stub".into(),
                usage: Default::default(),
                is_error: false,
            }
        }
    }

    fn context() -> ToolContext {
        let ctx = ToolContext::new(
            std::env::temp_dir(),
            std::sync::Arc::new(crate::config::Config::default()),
            std::sync::Arc::new(crate::mcp::McpManager::new(Default::default(), "test")),
        );
        let _ = ctx.subagents.set(std::sync::Arc::new(StubRunner));
        ctx
    }

    #[tokio::test]
    async fn set_goal_tool_updates_the_shared_cell() {
        let ctx = context();
        let tool = SetGoalTool;
        let out = tool
            .run(serde_json::json!({ "goal": "ship the reviewer" }), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(
            ctx.goal.read().await.clone().as_deref(),
            Some("ship the reviewer")
        );
        // An empty goal clears it.
        let _ = tool.run(serde_json::json!({ "goal": "" }), &ctx).await;
        assert!(ctx.goal.read().await.is_none());
    }

    #[tokio::test]
    async fn set_goal_with_verify_runs_the_verifiers() {
        let ctx = context();
        let tool = SetGoalTool;
        let out = tool
            .run(
                serde_json::json!({ "goal": "make the tests pass", "verify": true }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("verification"), "{}", out.content);
        assert!(out.content.contains("VERDICT"), "{}", out.content);
        // The verifier shows up in the task registry too.
        let tasks = ctx.tasks.list().await;
        assert!(tasks.iter().any(|task| task.id.starts_with("goal-")));
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    use crate::tools::Tool;

    /// Echoes the prompt back, so tests can see what each step received.
    struct EchoRunner;

    #[async_trait::async_trait]
    impl SubagentRunner for EchoRunner {
        async fn run_subagent(
            &self,
            request: SubagentRequest,
            _parent: &ToolContext,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> SubagentOutcome {
            SubagentOutcome {
                text: format!("saw: {}", request.prompt),
                session_id: "echo".into(),
                usage: Default::default(),
                is_error: false,
            }
        }
    }

    fn context() -> ToolContext {
        let ctx = ToolContext::new(
            std::env::temp_dir(),
            std::sync::Arc::new(crate::config::Config::default()),
            std::sync::Arc::new(crate::mcp::McpManager::new(Default::default(), "test")),
        );
        let _ = ctx.subagents.set(std::sync::Arc::new(EchoRunner));
        ctx
    }

    #[tokio::test]
    async fn plan_steps_receive_their_dependencies_reports() {
        let ctx = context();
        let tool = PlanWorkflowTool;
        let out = tool
            .run(
                serde_json::json!({
                    "name": "investigate-then-fix",
                    "steps": [
                        { "title": "find", "prompt": "Find the bug" },
                        { "title": "fix", "prompt": "Fix it", "depends_on": [1] }
                    ]
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("find: saw: Find the bug"),
            "{}",
            out.content
        );
        // The dependent step saw the first step's report.
        assert!(
            out.content.contains("Report from step 1"),
            "{}",
            out.content
        );
        // Both steps show up in the task registry.
        let tasks = ctx.tasks.list().await;
        assert_eq!(
            tasks.iter().filter(|t| t.id.starts_with("plan-")).count(),
            2
        );
    }

    #[tokio::test]
    async fn plan_rejects_too_many_steps() {
        let ctx = context();
        let tool = PlanWorkflowTool;
        let steps: Vec<serde_json::Value> = (0..9)
            .map(|i| serde_json::json!({ "title": format!("s{i}"), "prompt": "do" }))
            .collect();
        let out = tool
            .run(serde_json::json!({ "name": "big", "steps": steps }), &ctx)
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("at most 8"), "{}", out.content);
    }
}
