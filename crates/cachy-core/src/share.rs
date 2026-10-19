//! Portable session bundles: one JSON file that carries a conversation, so a
//! session can be handed to someone else without a server.
//!
//! `/share` writes `~/cachy-session-<id>.cachy`; `/import <file>` turns it back
//! into a session for the current directory.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::message::ChatMessage;
use crate::session::SessionStore;

/// Format marker so a stray JSON file is not mistaken for a bundle.
pub const BUNDLE_KIND: &str = "cachycli.session";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBundle {
    pub kind: String,
    pub version: u32,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model: String,
    pub agent: String,
    pub created_at: String,
    pub updated_at: String,
    /// The goal the session was working towards, when it had one.
    #[serde(default)]
    pub goal: Option<String>,
    pub messages: Vec<ChatMessage>,
}

/// Build a bundle from a stored session.
pub fn bundle(store: &SessionStore) -> Result<SessionBundle> {
    let summary = &store.summary;
    let messages = store.load_chat().unwrap_or_default();
    Ok(SessionBundle {
        kind: BUNDLE_KIND.to_string(),
        version: 1,
        id: summary.id.clone(),
        title: summary.title.clone(),
        cwd: summary.cwd.clone(),
        model: summary.current_model_id.clone(),
        agent: summary.agent_name.clone(),
        created_at: summary.created_at.clone(),
        updated_at: summary.updated_at.clone(),
        goal: summary.goal.clone(),
        messages,
    })
}

/// Serialize a bundle (pretty, so it can be read and diffed by a human).
pub fn to_json(bundle: &SessionBundle) -> Result<String> {
    Ok(serde_json::to_string_pretty(bundle)?)
}

/// Parse a bundle, refusing anything that is not one.
pub fn from_json(text: &str) -> Result<SessionBundle> {
    let bundle: SessionBundle =
        serde_json::from_str(text).context("this is not a CachyCLI session bundle")?;
    if bundle.kind != BUNDLE_KIND {
        anyhow::bail!(
            "this file is a {:?} bundle, not a {:?} one",
            bundle.kind,
            BUNDLE_KIND
        );
    }
    Ok(bundle)
}

/// Write a session into a bundle file, returning the path written.
pub fn share_to_file(store: &SessionStore, path: &Path) -> Result<std::path::PathBuf> {
    let bundle = bundle(store)?;
    let text = to_json(&bundle)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, text)?;
    Ok(path.to_path_buf())
}

/// Import a bundle into a new session for `cwd`, ready to resume.
pub fn import_into(cwd: &Path, text: &str) -> Result<SessionStore> {
    let bundle = from_json(text)?;
    let model = if bundle.model.is_empty() {
        "imported"
    } else {
        bundle.model.as_str()
    };
    let agent = if bundle.agent.is_empty() {
        "cachy"
    } else {
        bundle.agent.as_str()
    };
    let mut store = SessionStore::create(cwd, model, agent, None)?;
    store.rewrite_chat(&bundle.messages)?;
    if !bundle.title.is_empty() {
        store.rename(&bundle.title)?;
    }
    if let Some(goal) = bundle.goal.clone() {
        store.summary.goal = Some(goal);
        let _ = store.save_summary();
    }
    Ok(store)
}

/// Where `agent serve` keeps the sessions other people pushed.
#[derive(Debug, Clone)]
pub struct ShareStore {
    dir: std::path::PathBuf,
}

/// One stored bundle, as `list` shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareEntry {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model: String,
    pub messages: usize,
    pub bytes: usize,
    pub updated_at: String,
}

impl ShareStore {
    pub fn open(dir: impl Into<std::path::PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The server's default place: `~/.cachy/shares`.
    pub fn default_dir() -> std::path::PathBuf {
        crate::paths::cachy_home().join("shares")
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Store a bundle, replacing any earlier copy of the same session.
    pub fn put(&self, text: &str) -> Result<ShareEntry> {
        let bundle = from_json(text)?;
        let id = safe_id(&bundle.id)?;
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        std::fs::write(self.dir.join(format!("{id}.cachy")), text)?;
        Ok(entry_of(&bundle, text.len()))
    }

    /// Read one bundle back.
    pub fn get(&self, id: &str) -> Result<String> {
        let id = safe_id(id)?;
        let path = self.dir.join(format!("{id}.cachy"));
        std::fs::read_to_string(&path).with_context(|| format!("no shared session {id:?}"))
    }

    /// Everything stored, newest first.
    pub fn list(&self) -> Vec<ShareEntry> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<ShareEntry> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "cachy"))
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                let bundle = from_json(&text).ok()?;
                Some(entry_of(&bundle, text.len()))
            })
            .collect();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    /// Drop a stored bundle.
    pub fn remove(&self, id: &str) -> Result<()> {
        let id = safe_id(id)?;
        std::fs::remove_file(self.dir.join(format!("{id}.cachy")))
            .with_context(|| format!("no shared session {id:?}"))
    }
}

fn entry_of(bundle: &SessionBundle, bytes: usize) -> ShareEntry {
    ShareEntry {
        id: bundle.id.clone(),
        title: bundle.title.clone(),
        cwd: bundle.cwd.clone(),
        model: bundle.model.clone(),
        messages: bundle.messages.len(),
        bytes,
        updated_at: bundle.updated_at.clone(),
    }
}

/// Ids become file names, so refuse anything that could climb out of the directory.
fn safe_id(id: &str) -> Result<String> {
    let trimmed = id.trim();
    let usable = !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed != ".."
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !usable {
        anyhow::bail!("not a usable session id: {id:?}");
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cwd() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("cachy-share-test");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn round_trips_a_session() {
        let cwd = temp_cwd();
        let mut store = SessionStore::create(&cwd, "mock-1", "cachy", None).unwrap();
        store
            .append_chat(&ChatMessage::user("primeira pergunta"))
            .unwrap();
        store
            .append_chat(&ChatMessage::assistant("primeira resposta"))
            .unwrap();
        store.rename("conversa de teste").unwrap();

        let bundle = bundle(&store).unwrap();
        assert_eq!(bundle.kind, BUNDLE_KIND);
        assert_eq!(bundle.messages.len(), 2);
        assert_eq!(bundle.title, "conversa de teste");

        let text = to_json(&bundle).unwrap();
        let imported = import_into(&cwd, &text).unwrap();
        assert_eq!(imported.summary.title, "conversa de teste");
        let messages = imported.load_chat().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "primeira pergunta");
        assert_eq!(messages[1].content, "primeira resposta");

        let _ = std::fs::remove_dir_all(&imported.dir);
        let _ = std::fs::remove_dir_all(&store.dir);
        let _ = std::fs::remove_dir(crate::paths::sessions_dir_for(&cwd));
    }

    #[test]
    fn store_keeps_and_forgets_bundles() {
        let dir = std::env::temp_dir().join("cachy-share-store-test");
        let _ = std::fs::remove_dir_all(&dir);
        let store = ShareStore::open(&dir);
        let cwd = temp_cwd();
        let mut session = SessionStore::create(&cwd, "mock-1", "cachy", None).unwrap();
        session.append_chat(&ChatMessage::user("oi")).unwrap();
        session.rename("conversa").unwrap();
        let text = to_json(&bundle(&session).unwrap()).unwrap();

        let entry = store.put(&text).unwrap();
        assert_eq!(entry.messages, 1);
        assert_eq!(entry.title, "conversa");
        assert_eq!(store.get(&entry.id).unwrap(), text);
        assert_eq!(store.list().len(), 1);

        // Nothing climbs out of the directory, and nothing else gets in.
        assert!(store.get("../../etc/passwd").is_err());
        assert!(store
            .put(r#"{"kind":"something.else","version":1}"#)
            .is_err());

        store.remove(&entry.id).unwrap();
        assert!(store.list().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&session.dir);
        let _ = std::fs::remove_dir(crate::paths::sessions_dir_for(&cwd));
    }

    #[test]
    fn refuses_foreign_json() {
        let error = from_json(r#"{"kind":"something.else","version":1}"#).unwrap_err();
        assert!(error.to_string().contains("not a"));
    }
}
