//! Session export to Markdown and introspection output.

use anyhow::Result;

use crate::session::{SessionStore, SessionUpdate};

/// Render a session transcript as Markdown.
pub fn session_markdown(store: &SessionStore) -> Result<String> {
    let mut out = String::new();
    let s = &store.summary;
    out.push_str(&format!(
        "# {}\n\n",
        if s.title.is_empty() {
            "Session"
        } else {
            &s.title
        }
    ));
    out.push_str(&format!("- Session ID: `{}`\n", s.id));
    out.push_str(&format!("- Working directory: `{}`\n", s.cwd));
    out.push_str(&format!("- Model: `{}`\n", s.current_model_id));
    out.push_str(&format!("- Created: {}\n", s.created_at));
    out.push_str(&format!("- Updated: {}\n", s.updated_at));
    out.push_str(&format!("- Messages: {}\n\n---\n\n", s.num_messages));

    for update in store.load_updates()? {
        match update {
            SessionUpdate::UserMessage { text, .. } => {
                out.push_str(&format!("## User\n\n{text}\n\n"));
            }
            SessionUpdate::AgentMessage { text, .. } => {
                out.push_str(&format!("## Assistant\n\n{text}\n\n"));
            }
            SessionUpdate::AgentThought { text, .. } => {
                out.push_str(&format!(
                    "<details><summary>Thinking</summary>\n\n{text}\n\n</details>\n\n"
                ));
            }
            SessionUpdate::ToolCall {
                name,
                title,
                arguments,
                ..
            } => {
                out.push_str(&format!(
                    "### Tool call: {name}\n\n```json\n{}\n```\n\n",
                    serde_json::to_string_pretty(&arguments).unwrap_or_default()
                ));
                let _ = title;
            }
            SessionUpdate::ToolCallUpdate {
                status, content, ..
            } => {
                out.push_str(&format!("*Status: {status}*\n\n"));
                if let Some(content) = content {
                    out.push_str(&format!("```\n{content}\n```\n\n"));
                }
            }
            SessionUpdate::Plan { entries, .. } => {
                out.push_str("### Plan\n\n");
                for entry in entries {
                    out.push_str(&format!("- [{}] {}\n", entry.status, entry.content));
                }
                out.push('\n');
            }
            SessionUpdate::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                out.push_str(&format!(
                    "*Usage: {input_tokens} in / {output_tokens} out*\n\n"
                ));
            }
            SessionUpdate::TurnEnd { stop_reason, .. } => {
                out.push_str(&format!("---\n\n*Turn ended: {stop_reason}*\n\n"));
            }
            SessionUpdate::Notice {
                title, description, ..
            } => {
                out.push_str(&format!(
                    "> **{title}**{}\n\n",
                    description.map(|d| format!(" — {d}")).unwrap_or_default()
                ));
            }
            _ => {}
        }
    }
    Ok(out)
}
