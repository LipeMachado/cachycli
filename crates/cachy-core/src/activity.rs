//! Activity across sessions: every turn, when it ran and how long it took.
//!
//! The interface shows the current session's turns (`/timeline`); this is the
//! same view with every session in it (`/timeline all`), read back from the
//! persisted update streams, so it works after a restart and across projects.

use std::path::Path;

use crate::paths;
use crate::session::{SessionSummary, SessionUpdate};

/// One finished turn, as the activity view shows it.
#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    /// When the turn started (RFC 3339), taken from the user message.
    pub started: String,
    pub seconds: f64,
    pub tools: usize,
    pub stop: String,
}

impl TurnRecord {
    /// The session's name, or a short id when it never got one.
    pub fn label(&self) -> String {
        if self.title.trim().is_empty() {
            short_id(&self.session_id)
        } else {
            self.title.clone()
        }
    }

    /// When the turn started, as `MM-DD HH:MM`.
    pub fn when(&self) -> String {
        when_text(&self.started)
    }

    /// The project it ran in, shortened for a terminal row.
    pub fn project(&self) -> String {
        shorten(&self.cwd)
    }
}

/// Turns from the most recent sessions, newest first.
pub fn recent_turns(limit: usize) -> Vec<TurnRecord> {
    recent_turns_in(&paths::sessions_dir(), limit)
}

/// The same, from an explicit sessions root (tests use a temp directory).
pub fn recent_turns_in(root: &Path, limit: usize) -> Vec<TurnRecord> {
    let mut summaries: Vec<(SessionSummary, std::path::PathBuf)> = Vec::new();
    let Ok(cwds) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    for cwd_dir in cwds.flatten() {
        let Ok(sessions) = std::fs::read_dir(cwd_dir.path()) else {
            continue;
        };
        for session in sessions.flatten() {
            let dir = session.path();
            let Ok(raw) = std::fs::read_to_string(dir.join("summary.json")) else {
                continue;
            };
            if let Ok(parsed) = serde_json::from_str::<SessionSummary>(&raw) {
                summaries.push((parsed, dir));
            }
        }
    }
    summaries.sort_by(|a, b| b.0.last_active_at.cmp(&a.0.last_active_at));

    let mut out: Vec<TurnRecord> = Vec::new();
    for (summary, dir) in &summaries {
        out.extend(turns_of(summary, dir));
        if out.len() >= limit {
            break;
        }
    }
    out.sort_by(|a, b| b.started.cmp(&a.started));
    out.truncate(limit);
    out
}

/// Pair each user message with the turn end that follows it.
fn turns_of(summary: &SessionSummary, dir: &Path) -> Vec<TurnRecord> {
    let Ok(raw) = std::fs::read_to_string(dir.join("updates.jsonl")) else {
        return Vec::new();
    };
    let mut turns = Vec::new();
    let mut open: Option<(String, usize)> = None;
    for line in raw.lines() {
        let Ok(update) = serde_json::from_str::<SessionUpdate>(line) else {
            continue;
        };
        match update {
            SessionUpdate::UserMessage { ts, .. } => open = Some((ts, 0)),
            SessionUpdate::ToolCall { .. } => {
                if let Some((_, tools)) = open.as_mut() {
                    *tools += 1;
                }
            }
            SessionUpdate::TurnEnd { stop_reason, ts } => {
                if let Some((started, tools)) = open.take() {
                    turns.push(TurnRecord {
                        session_id: summary.id.clone(),
                        title: summary.title.clone(),
                        cwd: summary.cwd.clone(),
                        seconds: seconds_between(&started, &ts),
                        started,
                        tools,
                        stop: stop_reason,
                    });
                }
            }
            _ => {}
        }
    }
    turns
}

fn seconds_between(start: &str, end: &str) -> f64 {
    let (Ok(start), Ok(end)) = (
        chrono::DateTime::parse_from_rfc3339(start),
        chrono::DateTime::parse_from_rfc3339(end),
    ) else {
        return 0.0;
    };
    let millis = (end - start).num_milliseconds();
    if millis < 0 {
        0.0
    } else {
        millis as f64 / 1000.0
    }
}

/// A stored timestamp as `MM-DD HH:MM` in local time, falling back to the raw
/// text when it is not RFC 3339.
pub fn when_text(ts: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(at) => at
            .with_timezone(&chrono::Local)
            .format("%m-%d %H:%M")
            .to_string(),
        Err(_) => ts.chars().take(16).collect(),
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// `~/P/l/cachycli`-style shortening, so a row can show where it ran.
fn shorten(cwd: &str) -> String {
    if cwd.is_empty() {
        return String::new();
    }
    let path = Path::new(cwd);
    let mut parts: Vec<String> = Vec::new();
    for component in path.components().rev().take(2) {
        parts.push(component.as_os_str().to_string_lossy().to_string());
    }
    parts.reverse();
    let tail = parts.join("/");
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy().to_string();
        if cwd == home {
            return "~".into();
        }
        if let Some(rest) = cwd.strip_prefix(&format!("{home}/")) {
            // One letter per directory, like the header does.
            let mut out = String::from("~/");
            let components: Vec<&str> = rest.split('/').collect();
            for (index, part) in components.iter().enumerate() {
                if index + 1 == components.len() {
                    out.push_str(part);
                } else {
                    out.push_str(&part.chars().take(1).collect::<String>());
                    out.push('/');
                }
            }
            return out;
        }
    }
    format!("/{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_turns_from_a_sessions_root() {
        let root = std::env::temp_dir().join("cachy-activity-test");
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("%2Ftmp%2Fproj").join("01test-session");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("summary.json"),
            serde_json::json!({
                "id": "01test-session",
                "cwd": "/tmp/proj",
                "title": "conversa",
                "last_active_at": "2026-10-18T10:00:00+00:00"
            })
            .to_string(),
        )
        .unwrap();
        let updates = [
            serde_json::json!({"type":"user_message","text":"oi","ts":"2026-10-18T10:00:00+00:00"}),
            serde_json::json!({"type":"tool_call","id":"c1","name":"bash","title":"bash ls",
                               "kind":"execute","arguments":{},"ts":"2026-10-18T10:00:05+00:00"}),
            serde_json::json!({"type":"turn_end","stop_reason":"end_turn","ts":"2026-10-18T10:00:12+00:00"}),
        ]
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        std::fs::write(dir.join("updates.jsonl"), updates).unwrap();

        let turns = recent_turns_in(&root, 10);
        assert_eq!(turns.len(), 1, "one finished turn");
        assert_eq!(turns[0].seconds, 12.0);
        assert_eq!(turns[0].tools, 1);
        assert_eq!(turns[0].label(), "conversa");
        assert_eq!(turns[0].project(), "/tmp/proj");
        assert!(!turns[0].when().is_empty());

        // A missing root is not an error, just nothing to show.
        assert!(recent_turns_in(&root.join("nope"), 5).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
