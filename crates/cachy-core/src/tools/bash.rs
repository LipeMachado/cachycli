//! `run_terminal_cmd` — shell command execution with timeouts, output caps and
//! background support.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;

use super::{BackgroundTask, TaskKind, TaskStatus, Tool, ToolContext, ToolKind, ToolResult};

pub const MAX_OUTPUT_BYTES: usize = 65_536;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "run_terminal_cmd"
    }
    fn display_name(&self) -> String {
        "bash".into()
    }
    fn description(&self) -> &str {
        "Execute a shell command in the working directory and return its output. \
         Use is_background=true for long-running processes (servers, watchers). \
         Output is capped; commands have a timeout (default 120s)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to run" },
                "description": { "type": "string", "description": "Short description of what the command does" },
                "is_background": { "type": "boolean", "description": "Run in the background and return a task id" },
                "timeout_secs": { "type": "number", "description": "Timeout in seconds (foreground only)" }
            },
            "required": ["command"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Bash
    }
    fn title(&self, args: &Value) -> String {
        let command = crate::tools::short_target(args, &["command"]);
        format!("bash {command}")
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if command.is_empty() {
            return ToolResult::err("missing required argument: command");
        }
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let background = args
            .get("is_background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_f64())
            .unwrap_or(ctx.config.toolset.bash.timeout_secs);
        let limit = ctx.config.toolset.bash.output_byte_limit.max(1024);

        if background {
            return self.spawn_background(command, description, ctx).await;
        }

        let mut cmd = shell_command(&command);
        cmd.current_dir(&ctx.cwd);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("failed to spawn command: {e}")),
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_secs_f64(timeout.max(1.0)),
            async {
                let output = child.wait_with_output().await;
                output
            },
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let mut text = String::new();
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                if !stdout.is_empty() {
                    text.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str("[stderr]\n");
                    text.push_str(&stderr);
                }
                let (text, truncated) = truncate_text(&text, limit);
                let code = output.status.code().unwrap_or(-1);
                let mut header = format!("exit code: {code}");
                if truncated {
                    header.push_str(" (output truncated)");
                }
                ToolResult::with_meta(
                    format!("{header}\n{text}"),
                    json!({ "exit_code": code, "truncated": truncated }),
                )
            }
            Ok(Err(e)) => ToolResult::err(format!("command failed: {e}")),
            Err(_) => ToolResult::err(format!("command timed out after {timeout}s")),
        }
    }
}

impl BashTool {
    async fn spawn_background(
        &self,
        command: String,
        description: String,
        ctx: &ToolContext,
    ) -> ToolResult {
        let id = ctx.tasks.next_id("cmd");
        let task = Arc::new(BackgroundTask {
            id: id.clone(),
            kind: TaskKind::Command,
            description: if description.is_empty() {
                command.clone()
            } else {
                description
            },
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
        let cwd = ctx.cwd.clone();
        tokio::spawn(async move {
            let mut cmd = shell_command(&command);
            cmd.current_dir(&cwd);
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            cmd.stdin(std::process::Stdio::null());
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    *status.write().await = TaskStatus::Failed(e.to_string());
                    return;
                }
            };
            let mut stdout = child.stdout.take();
            let mut stderr = child.stderr.take();
            let out_ref = output.clone();
            let err_ref = output.clone();
            let read_out = async {
                if let Some(mut s) = stdout.take() {
                    let mut buf = [0u8; 4096];
                    loop {
                        match s.read(&mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                let mut guard = out_ref.write().await;
                                if guard.len() < MAX_OUTPUT_BYTES * 4 {
                                    guard.push_str(&String::from_utf8_lossy(&buf[..n]));
                                }
                            }
                        }
                    }
                }
            };
            let read_err = async {
                if let Some(mut s) = stderr.take() {
                    let mut buf = [0u8; 4096];
                    loop {
                        match s.read(&mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                let mut guard = err_ref.write().await;
                                guard.push_str(&String::from_utf8_lossy(&buf[..n]));
                            }
                        }
                    }
                }
            };
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    task.finish(TaskStatus::Killed).await;
                }
                _ = async { tokio::join!(read_out, read_err); } => {
                    match child.wait().await {
                        Ok(status_code) if status_code.success() => {
                            *status.write().await = TaskStatus::Completed;
                        }
                        Ok(status_code) => {
                            *status.write().await = TaskStatus::Failed(format!("exit code {}", status_code.code().unwrap_or(-1)));
                        }
                        Err(e) => *status.write().await = TaskStatus::Failed(e.to_string()),
                    }
                }
            }
        });

        ToolResult::with_meta(
            format!(
                "Command started in the background with task id {id}. Use get_task_output(\"{id}\") to check it and kill_task(\"{id}\") to stop it."
            ),
            json!({ "task_id": id, "background": true }),
        )
    }
}

/// Build the platform shell command.
pub fn shell_command(command: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", command]);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

pub fn truncate_text(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_string(), false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    (
        format!(
            "{}\n[... truncated {} bytes ...]",
            &text[..end],
            text.len() - end
        ),
        true,
    )
}

/// Helper used by other tools that reuse the registry shape.
pub trait TaskContextExt {
    fn task_registry(&self) -> &Arc<super::TaskRegistry>;
}

impl TaskContextExt for ToolContext {
    fn task_registry(&self) -> &Arc<super::TaskRegistry> {
        &self.tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolContext};

    fn context() -> ToolContext {
        ToolContext::new(
            std::env::temp_dir(),
            std::sync::Arc::new(crate::config::Config::default()),
            std::sync::Arc::new(crate::mcp::McpManager::new(Default::default(), "test")),
        )
    }

    #[tokio::test]
    async fn hostile_commands_are_answered() {
        let ctx = context();
        let tool = BashTool;

        // No command at all.
        let result = tool.run(serde_json::json!({}), &ctx).await;
        assert!(result.is_error, "an empty command is an error");

        // A command that does not exist: the output comes back with the exit
        // code, and the interface counts it as a failure.
        let result = tool
            .run(
                serde_json::json!({ "command": "cachy-nao-existe-xyz" }),
                &ctx,
            )
            .await;
        assert!(
            result.content.contains("127"),
            "the exit code is reported: {}",
            result.content
        );
        assert_eq!(crate::tools::status_of(&result), "failed");

        // A command that fails.
        let result = tool
            .run(serde_json::json!({ "command": "exit 3" }), &ctx)
            .await;
        assert_eq!(
            crate::tools::status_of(&result),
            "failed",
            "exit 3 is a failure"
        );

        // A command that succeeds is not.
        let result = tool
            .run(serde_json::json!({ "command": "true" }), &ctx)
            .await;
        assert_eq!(crate::tools::status_of(&result), "completed");

        // Output that is not valid UTF-8.
        let result = tool
            .run(
                serde_json::json!({ "command": "printf '\\xff\\xfe\\x00fim'" }),
                &ctx,
            )
            .await;
        assert!(
            !result.is_error,
            "lossy decoding, not a crash: {}",
            result.content
        );

        // Output far past the limit gets truncated, not swallowed.
        let result = tool
            .run(
                serde_json::json!({ "command": "yes cachy | head -c 400000" }),
                &ctx,
            )
            .await;
        assert!(result.content.len() < 400_000, "output should be capped");
        assert!(result.content.contains("truncated"), "and say so");

        // A timeout ends the command instead of hanging the turn.
        let result = tool
            .run(
                serde_json::json!({ "command": "sleep 5", "timeout_secs": 1 }),
                &ctx,
            )
            .await;
        assert!(result.is_error, "a timeout is reported: {}", result.content);
    }
}
