//! Cross-session memory: markdown topics + observation inbox per scope.
//!
//! ```text
//! ~/.cachy/memory/
//!   global/
//!     MEMORY.md
//!     topics/*.md
//!     observations/_inbox/*.md
//!   workspaces/<slug>-<hash8>/
//!     ...
//! ```

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::paths;

/// Scope is either global or tied to a workspace slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    Workspace(PathBuf),
}

impl Scope {
    pub fn scope_dir(&self) -> PathBuf {
        match self {
            Scope::Global => paths::memory_dir().join("global"),
            Scope::Workspace(slug) => paths::memory_dir().join("workspaces").join(slug),
        }
    }
}

/// Slug + short hash for a workspace, derived from the git remote when present.
pub fn workspace_slug(cwd: &Path) -> String {
    let mut name = cwd
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".into());
    if name.is_empty() {
        name = "workspace".into();
    }
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let key = git_remote(cwd).unwrap_or_else(|| cwd.to_string_lossy().to_string());
    format!("{sanitized}-{:08x}", fnv1a(&key) as u32)
}

fn git_remote(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Ensure the scope layout exists.
pub fn ensure_scope(scope: &Scope) -> Result<PathBuf> {
    let dir = scope.scope_dir();
    std::fs::create_dir_all(dir.join("topics"))?;
    std::fs::create_dir_all(dir.join("observations").join("_inbox"))?;
    Ok(dir)
}

/// Write an observation note into the inbox.
pub fn add_observation(scope: &Scope, text: &str) -> Result<PathBuf> {
    let dir = ensure_scope(scope)?;
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir
        .join("observations")
        .join("_inbox")
        .join(format!("{ts}-{}.md", &id[..8]));
    let content = format!("# Observation\n\n{}\n", text.trim());
    std::fs::write(&path, content)?;
    rebuild_index(scope)?;
    Ok(path)
}

/// Create or append to a topic note.
pub fn write_topic(scope: &Scope, topic: &str, text: &str) -> Result<PathBuf> {
    let dir = ensure_scope(scope)?;
    let slug: String = topic
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let path = dir.join("topics").join(format!("{slug}.md"));
    let mut content = if path.is_file() {
        std::fs::read_to_string(&path)?
    } else {
        format!("# {topic}\n")
    };
    content.push_str(&format!("\n{}\n", text.trim()));
    std::fs::write(&path, content)?;
    rebuild_index(scope)?;
    Ok(path)
}

/// List topic files.
pub fn topics(scope: &Scope) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let dir = scope.scope_dir().join("topics");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let title = std::fs::read_to_string(&path)
                .ok()
                .and_then(|c| {
                    c.lines()
                        .find(|l| l.starts_with("# "))
                        .map(|l| l[2..].trim().to_string())
                })
                .unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                });
            out.push((title, path));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Rebuild `MEMORY.md` from topics.
pub fn rebuild_index(scope: &Scope) -> Result<()> {
    let dir = ensure_scope(scope)?;
    let mut index = String::from("# Memory index\n\n");
    for (title, path) in topics(scope) {
        let rel = path.strip_prefix(&dir).unwrap_or(&path);
        index.push_str(&format!("- [{title}]({})\n", rel.display()));
    }
    std::fs::write(dir.join("MEMORY.md"), index)?;
    Ok(())
}

/// The index text injected into the system prompt.
pub fn index_text(scope: &Scope) -> Option<String> {
    let path = scope.scope_dir().join("MEMORY.md");
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Clear a scope's memory files.
pub fn clear(scope: &Scope) -> Result<()> {
    let dir = scope.scope_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_stable() {
        let a = workspace_slug(Path::new("/tmp/some-project"));
        let b = workspace_slug(Path::new("/tmp/some-project"));
        assert_eq!(a, b);
        assert!(a.starts_with("some-project-"));
    }
}
