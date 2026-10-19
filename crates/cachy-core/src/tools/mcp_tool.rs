//! MCP bridge tools: `search_tool` discovers integration tools, `use_tool`
//! invokes them.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{Tool, ToolContext, ToolKind, ToolResult};

pub struct SearchTool;

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search_tool"
    }
    fn description(&self) -> &str {
        "Search the available MCP integration tools by name, description or \
         server. Returns matching tool names to call with use_tool."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search terms (empty lists everything)" }
            }
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Mcp
    }
    fn title(&self, args: &Value) -> String {
        format!("Search tools \"{}\"", super::short_target(args, &["query"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let tools = ctx.mcp.list_tools().await;
        if tools.is_empty() {
            return ToolResult::ok(
                "No MCP tools are available. Configure servers under [mcp_servers.<name>] in config.toml.",
            );
        }
        let terms: Vec<&str> = query.split_whitespace().collect();
        let mut matched: Vec<(i64, String)> = Vec::new();
        for t in &tools {
            let haystack = format!("{} {} {}", t.server, t.name, t.description).to_lowercase();
            let score = if terms.is_empty() {
                1
            } else {
                terms.iter().filter(|term| haystack.contains(*term)).count() as i64
            };
            if score > 0 {
                matched.push((
                    score,
                    format!(
                        "{}__{}\n  {}",
                        t.server,
                        t.name,
                        t.description.replace('\n', " ")
                    ),
                ));
            }
        }
        if matched.is_empty() {
            let listing = tools
                .iter()
                .take(40)
                .map(|t| format!("{}__{}", t.server, t.name))
                .collect::<Vec<_>>()
                .join("\n");
            return ToolResult::ok(format!(
                "No tools matched \"{query}\". Available tools:\n{listing}"
            ));
        }
        matched.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        ToolResult::ok(
            matched
                .into_iter()
                .take(20)
                .map(|(_, s)| s)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

pub struct UseTool;

#[async_trait]
impl Tool for UseTool {
    fn name(&self) -> &str {
        "use_tool"
    }
    fn description(&self) -> &str {
        "Call an MCP integration tool discovered with search_tool. Names are \
         namespaced as <server>__<tool>."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Namespaced tool name, e.g. filesystem__read_file" },
                "arguments": { "type": "object", "description": "Tool arguments" }
            },
            "required": ["name"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Mcp
    }
    fn title(&self, args: &Value) -> String {
        format!("MCP {}", super::short_target(args, &["name"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: name");
        };
        let arguments = args.get("arguments").cloned().unwrap_or(json!({}));
        let (server, tool) = match name.split_once("__") {
            Some((s, t)) => (s.to_string(), t.to_string()),
            None => {
                // Bare name: find the unique server providing it.
                let matches: Vec<_> = ctx
                    .mcp
                    .list_tools()
                    .await
                    .into_iter()
                    .filter(|t| t.name == name)
                    .collect();
                match matches.len() {
                    0 => return ToolResult::err(format!("unknown tool {name}")),
                    1 => (matches[0].server.clone(), matches[0].name.clone()),
                    _ => {
                        return ToolResult::err(format!(
                            "tool {name} exists on multiple servers; use <server>__{name}"
                        ))
                    }
                }
            }
        };
        match ctx.mcp.call_tool(&server, &tool, arguments).await {
            Ok(result) => ToolResult::ok(result),
            Err(e) => ToolResult::err(format!("MCP call failed: {e}")),
        }
    }
}
