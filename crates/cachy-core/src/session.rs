//! Session persistence.
//!
//! Layout:
//!
//! ```text
//! ~/.cachy/sessions/<encoded-cwd>/<session-id>/
//!   summary.json          # metadata: title, timestamps, model, counts
//!   chat_history.jsonl    # raw chat messages sent to the model
//!   updates.jsonl         # UI-level event stream (messages + tool calls)
//!   plan.json             # TODO/task list state
//!   system_prompt.txt     # last assembled system prompt
//!   rewind_points.jsonl   # file snapshots for /rewind
//!   signals.json          # turn counters and usage
//! ```

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::message::ChatMessage;
use crate::paths;

/// Metadata stored in `summary.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub cwd: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub last_active_at: String,
    #[serde(default)]
    pub num_messages: u64,
    #[serde(default)]
    pub num_chat_messages: u64,
    #[serde(default)]
    pub current_model_id: String,
    #[serde(default)]
    pub agent_name: String,
    /// The goal this session is working towards (`/goal`).
    #[serde(default)]
    pub goal: Option<String>,
    /// The provider's own session id, when the provider keeps sessions itself
    /// (opencode, ACP, Claude Code) — so `--resume` can continue there too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_session: Option<String>,
    /// Which provider that id belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_provider: Option<String>,
    #[serde(default)]
    pub sandbox_profile: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// Parent session id when this is a subagent session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// True when the title was explicitly set via /rename.
    #[serde(default)]
    pub renamed: bool,
}

impl SessionSummary {
    pub fn new(id: &str, cwd: &Path, model: &str, agent: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: id.to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            title: String::new(),
            created_at: now.clone(),
            updated_at: now.clone(),
            last_active_at: now,
            num_messages: 0,
            num_chat_messages: 0,
            current_model_id: model.to_string(),
            agent_name: agent.to_string(),
            goal: None,
            backend_session: None,
            backend_provider: None,
            sandbox_profile: "off".into(),
            reasoning_effort: None,
            parent_session_id: None,
            renamed: false,
        }
    }
}

/// Signals written as the session progresses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSignals {
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// An append-only entry in `updates.jsonl`, describing conversation events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionUpdate {
    UserMessage {
        text: String,
        ts: String,
    },
    AgentMessage {
        text: String,
        ts: String,
    },
    AgentThought {
        text: String,
        ts: String,
    },
    ToolCall {
        id: String,
        name: String,
        title: String,
        kind: String,
        arguments: serde_json::Value,
        ts: String,
    },
    ToolCallUpdate {
        id: String,
        status: String,
        content: Option<String>,
        ts: String,
    },
    Plan {
        entries: Vec<PlanEntry>,
        ts: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        context_used: u64,
        context_size: u64,
        ts: String,
    },
    Notice {
        severity: String,
        title: String,
        description: Option<String>,
        ts: String,
    },
    TurnEnd {
        stop_reason: String,
        ts: String,
    },
    ModeChange {
        mode: String,
        ts: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEntry {
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// Handle to one session directory.
#[derive(Debug, Clone)]
pub struct SessionStore {
    pub dir: PathBuf,
    pub summary: SessionSummary,
}

impl SessionStore {
    /// Create a fresh session directory.
    pub fn create(cwd: &Path, model: &str, agent: &str, parent: Option<&str>) -> Result<Self> {
        let store = Self::create_with_id(cwd, &uuid::Uuid::now_v7().to_string(), model, agent)?;
        if let Some(parent) = parent {
            let mut store = store;
            store.summary.parent_session_id = Some(parent.to_string());
            store.write_summary()?;
            return Ok(store);
        }
        Ok(store)
    }

    /// Create a session with a caller-chosen id (named sessions).
    pub fn create_with_id(cwd: &Path, id: &str, model: &str, agent: &str) -> Result<Self> {
        let dir = paths::sessions_dir_for(cwd).join(id);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let summary = SessionSummary::new(id, cwd, model, agent);
        let store = Self { dir, summary };
        store.write_summary()?;
        Ok(store)
    }

    /// Open an existing session by id under a specific working directory,
    /// falling back to a global search.
    pub fn open(cwd: &Path, id: &str) -> Result<Self> {
        let dir = paths::sessions_dir_for(cwd).join(id);
        if dir.is_dir() {
            return Self::from_dir(dir);
        }
        if let Some(found) = find_session_dir(id) {
            return Self::from_dir(found);
        }
        anyhow::bail!("session not found: {id}")
    }

    pub fn from_dir(dir: PathBuf) -> Result<Self> {
        let summary_path = dir.join("summary.json");
        let summary: SessionSummary = serde_json::from_str(
            &std::fs::read_to_string(&summary_path)
                .with_context(|| format!("reading {}", summary_path.display()))?,
        )?;
        Ok(Self { dir, summary })
    }

    pub fn updates_path(&self) -> PathBuf {
        self.dir.join("updates.jsonl")
    }

    pub fn chat_history_path(&self) -> PathBuf {
        self.dir.join("chat_history.jsonl")
    }

    fn write_summary(&self) -> Result<()> {
        let path = self.dir.join("summary.json");
        let json = serde_json::to_string_pretty(&self.summary)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn save_summary(&mut self) -> Result<()> {
        self.summary.updated_at = chrono::Utc::now().to_rfc3339();
        self.summary.last_active_at = self.summary.updated_at.clone();
        self.write_summary()
    }

    /// Append a UI event.
    pub fn append_update(&self, update: &SessionUpdate) -> Result<()> {
        append_jsonl(&self.updates_path(), update)
    }

    /// Append a raw chat message.
    pub fn append_chat(&mut self, message: &ChatMessage) -> Result<()> {
        append_jsonl(&self.chat_history_path(), message)?;
        self.summary.num_chat_messages += 1;
        if matches!(
            message.role,
            crate::message::Role::User | crate::message::Role::Assistant
        ) {
            self.summary.num_messages += 1;
        }
        self.save_summary()
    }

    /// Replace the stored chat history (used by `/rewind`).
    pub fn rewrite_chat(&mut self, messages: &[ChatMessage]) -> Result<()> {
        let path = self.chat_history_path();
        let mut body = String::new();
        for message in messages {
            body.push_str(&serde_json::to_string(message)?);
            body.push('\n');
        }
        std::fs::write(&path, body)?;
        self.summary.num_chat_messages = messages.len() as u64;
        self.summary.num_messages = messages
            .iter()
            .filter(|m| {
                matches!(
                    m.role,
                    crate::message::Role::User | crate::message::Role::Assistant
                )
            })
            .count() as u64;
        self.save_summary()
    }

    /// Load all chat messages.
    pub fn load_chat(&self) -> Result<Vec<ChatMessage>> {
        read_jsonl(&self.chat_history_path())
    }

    /// Load all UI events.
    pub fn load_updates(&self) -> Result<Vec<SessionUpdate>> {
        read_jsonl(&self.updates_path())
    }

    pub fn write_system_prompt(&self, prompt: &str) -> Result<()> {
        std::fs::write(self.dir.join("system_prompt.txt"), prompt)?;
        Ok(())
    }

    pub fn write_plan(&self, entries: &[PlanEntry]) -> Result<()> {
        let json = serde_json::to_string_pretty(&serde_json::json!({ "entries": entries }))?;
        std::fs::write(self.dir.join("plan.json"), json)?;
        Ok(())
    }

    pub fn read_signals(&self) -> SessionSignals {
        std::fs::read_to_string(self.dir.join("signals.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn write_signals(&self, signals: &SessionSignals) -> Result<()> {
        std::fs::write(
            self.dir.join("signals.json"),
            serde_json::to_string_pretty(signals)?,
        )?;
        Ok(())
    }

    /// Attach a title if the session has none yet.
    pub fn set_title_if_empty(&mut self, title: &str) -> Result<()> {
        if self.summary.title.is_empty() {
            let title = title.trim();
            let t: String = title.chars().take(60).collect();
            self.summary.title = t;
            self.save_summary()?;
        }
        Ok(())
    }

    pub fn rename(&mut self, title: &str) -> Result<()> {
        self.summary.title = title.trim().chars().take(80).collect();
        self.summary.renamed = true;
        self.save_summary()
    }
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(value)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<T>(&line) {
            Ok(v) => out.push(v),
            Err(_) => continue, // tolerate torn writes
        }
    }
    Ok(out)
}

/// Locate a session directory anywhere under the sessions root.
pub fn find_session_dir(id: &str) -> Option<PathBuf> {
    let root = paths::sessions_dir();
    let entries = std::fs::read_dir(&root).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(id);
        if candidate.join("summary.json").is_file() {
            return Some(candidate);
        }
    }
    None
}

/// List session summaries for a working directory (or all when `cwd` is None).
pub fn list_sessions(cwd: Option<&Path>) -> Result<Vec<SessionSummary>> {
    let mut out = Vec::new();
    let roots: Vec<PathBuf> = match cwd {
        Some(cwd) => vec![paths::sessions_dir_for(cwd)],
        None => std::fs::read_dir(paths::sessions_dir())
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default(),
    };
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let summary = entry.path().join("summary.json");
            if summary.is_file() {
                if let Ok(raw) = std::fs::read_to_string(&summary) {
                    if let Ok(s) = serde_json::from_str::<SessionSummary>(&raw) {
                        out.push(s);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
    Ok(out)
}

/// Most recent session for a directory.
pub fn most_recent(cwd: &Path) -> Result<Option<SessionSummary>> {
    Ok(list_sessions(Some(cwd))?.into_iter().next())
}

/// Match a session by id or title (case-insensitive, word-joining lenient).
pub fn find_by_id_or_title(cwd: &Path, needle: &str) -> Result<Option<SessionSummary>> {
    let sessions = list_sessions(Some(cwd))?;
    if let Some(exact) = sessions.iter().find(|s| s.id == needle) {
        return Ok(Some(exact.clone()));
    }
    // The listings show a short id, so a prefix is what people copy and paste.
    let prefix = needle.to_lowercase();
    if prefix.len() >= 6 && prefix.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        let by_prefix: Vec<&SessionSummary> = sessions
            .iter()
            .filter(|s| s.id.to_lowercase().starts_with(&prefix))
            .collect();
        if let Some(newest) = by_prefix.first() {
            // `list_sessions` is newest first, so the most recent wins.
            return Ok(Some((*newest).clone()));
        }
    }
    let lower = needle.to_lowercase();
    let norm = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect()
    };
    let needle_words = norm(&lower);
    let mut matches: Vec<&SessionSummary> = sessions
        .iter()
        .filter(|s| {
            let title = s.title.to_lowercase();
            title.contains(&lower)
                || (needle_words.len() > 1
                    && needle_words.iter().all(|w| title.contains(w.as_str())))
        })
        .collect();
    if matches.len() == 1 {
        return Ok(Some(matches.remove(0).clone()));
    }
    if matches.len() > 1 {
        // A sole explicitly renamed title wins.
        let renamed: Vec<&&SessionSummary> = matches.iter().filter(|s| s.renamed).collect();
        if renamed.len() == 1 {
            return Ok(Some((*renamed[0]).clone()));
        }
        let ids = matches
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("multiple sessions match \"{needle}\": {ids}");
    }
    // Substring per-word match: every needle word appears as a title word.
    let mut word_matches: Vec<&SessionSummary> = sessions
        .iter()
        .filter(|s| {
            let words = norm(&s.title);
            !needle_words.is_empty() && needle_words.iter().all(|w| words.contains(w))
        })
        .collect();
    match word_matches.len() {
        0 => Ok(None),
        1 => Ok(Some(word_matches.remove(0).clone())),
        _ => {
            let ids = word_matches
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("multiple sessions match \"{needle}\": {ids}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_session_by_id_prefix() {
        let cwd = std::env::temp_dir().join("cachy-session-lookup-test");
        let _ = std::fs::create_dir_all(&cwd);
        let store = SessionStore::create(&cwd, "mock-1", "cachy", None).unwrap();
        let id = store.summary.id.clone();
        let short: String = id.chars().take(8).collect();

        // The listings show a short id; that is what gets pasted back.
        let found = find_by_id_or_title(&cwd, &short).unwrap().unwrap();
        assert_eq!(found.id, id);
        let found = find_by_id_or_title(&cwd, &id).unwrap().unwrap();
        assert_eq!(found.id, id);

        // Nonsense matches nothing rather than guessing.
        assert!(find_by_id_or_title(&cwd, "zzzzzz").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&store.dir);
        let _ = std::fs::remove_dir_all(&cwd);
        let _ = std::fs::remove_dir(paths::sessions_dir_for(&cwd));
    }
}
