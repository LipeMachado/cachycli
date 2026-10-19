//! `todo_write` — task list state for the current session.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{TodoItem, Tool, ToolContext, ToolKind, ToolResult};

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }
    fn description(&self) -> &str {
        "Create and manage the session task list. Pass the complete list every time; \
         statuses are pending, in_progress, completed or cancelled."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The complete todo list",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"] },
                            "priority": { "type": "string", "enum": ["high", "medium", "low"] }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Todo
    }
    fn title(&self, _args: &Value) -> String {
        "Updated todo list".into()
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(items) = args.get("todos").and_then(|v| v.as_array()) else {
            return ToolResult::err("missing required argument: todos");
        };
        let mut todos = Vec::new();
        for item in items {
            let content = item
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if content.is_empty() {
                continue;
            }
            todos.push(TodoItem {
                content,
                status: item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending")
                    .to_string(),
                priority: item
                    .get("priority")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium")
                    .to_string(),
            });
        }
        let snapshot = todos.clone();
        *ctx.todos.write().await = todos;

        if let Some(tx) = &ctx.events {
            let _ = tx.send(crate::agent::AgentEvent::Todos(snapshot.clone()));
        }

        let mut summary = String::from("Todo list updated:\n");
        for t in &snapshot {
            let mark = match t.status.as_str() {
                "completed" => "[x]",
                "in_progress" => "[>]",
                "cancelled" => "[-]",
                _ => "[ ]",
            };
            summary.push_str(&format!("{mark} {}\n", t.content));
        }
        ToolResult::with_meta(summary, json!({ "todos": snapshot }))
    }
}
