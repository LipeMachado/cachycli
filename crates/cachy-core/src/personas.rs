//! Agent profiles ("personas"): Markdown files that shape how the agent
//! behaves. Discovered like skills, so project, user and Claude Code layouts
//! all work:
//!
//! - `<cwd>/.cachy/agents/*.md`
//! - `<git root>/.cachy/agents/*.md`
//! - `~/.cachy/agents/*.md`
//! - `<cwd>/.claude/agents/*.md` and `~/.claude/agents/*.md`
//!
//! A file may start with frontmatter (`name`, `description`); the rest is the
//! persona text that gets injected into the system prompt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::paths;

#[derive(Debug, Clone)]
pub struct Persona {
    pub name: String,
    pub description: String,
    /// Text injected into the system prompt.
    pub body: String,
    /// Where it came from (`local`, `repo`, `user`, `claude`).
    pub source: String,
    pub path: PathBuf,
}

impl Persona {
    /// `name — description` for pickers.
    pub fn label(&self) -> String {
        if self.description.is_empty() {
            self.name.clone()
        } else {
            format!("{} — {}", self.name, self.description)
        }
    }
}

/// Every persona visible from this directory, first source winning.
pub fn discover(cwd: &Path, _config: &Config) -> Vec<Persona> {
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    let mut push = |dir: PathBuf, source: &str| {
        if dir.is_dir() {
            sources.push((dir, source.to_string()));
        }
    };
    push(cwd.join(".cachy/agents"), "local");
    if let Some(root) = crate::skills::git_root(cwd) {
        if root != cwd {
            push(root.join(".cachy/agents"), "repo");
        }
    }
    push(paths::agents_dir(), "user");
    push(cwd.join(".claude/agents"), "claude-local");
    push(paths::user_claude_dir().join("agents"), "claude-user");

    let mut map: BTreeMap<String, Persona> = BTreeMap::new();
    for (dir, source) in sources {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_markdown = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false);
            if !path.is_file() || !is_markdown {
                continue;
            }
            if let Some(persona) = parse_persona(&path, &source) {
                map.entry(persona.name.clone()).or_insert(persona);
            }
        }
    }
    map.into_values().collect()
}

/// Find one by name (case-insensitive).
pub fn find(cwd: &Path, config: &Config, name: &str) -> Option<Persona> {
    let wanted = name.trim().to_lowercase();
    discover(cwd, config)
        .into_iter()
        .find(|persona| persona.name.to_lowercase() == wanted)
}

fn parse_persona(path: &Path, source: &str) -> Option<Persona> {
    let text = std::fs::read_to_string(path).ok()?;
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let mut name = stem.clone();
    let mut description = String::new();
    let mut body = text.clone();

    // Optional `--- key: value ---` frontmatter.
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
            for line in front.lines() {
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                let value = value.trim().trim_matches('"').to_string();
                match key.trim().to_lowercase().as_str() {
                    "name" if !value.is_empty() => name = value,
                    "description" if !value.is_empty() => description = value,
                    _ => {}
                }
            }
        }
    }
    if description.is_empty() {
        // Prefer a leading `# Title`, else the first real line.
        let lines: Vec<&str> = body.lines().map(|line| line.trim()).collect();
        let title = lines
            .iter()
            .find(|line| line.starts_with('#'))
            .map(|line| line.trim_start_matches(['#', ' ']).to_string());
        let first = lines
            .iter()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.to_string());
        description = title
            .or(first)
            .unwrap_or_default()
            .chars()
            .take(80)
            .collect();
    }
    if body.trim().is_empty() {
        return None;
    }
    Some(Persona {
        name,
        description,
        body,
        source: source.to_string(),
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let dir = std::env::temp_dir().join("cachy-persona-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("reviewer.md");
        std::fs::write(
            &file,
            "---\nname: reviewer\ndescription: Reviews code\n---\nYou review code carefully.\n",
        )
        .unwrap();
        let persona = parse_persona(&file, "user").expect("persona");
        assert_eq!(persona.name, "reviewer");
        assert_eq!(persona.description, "Reviews code");
        assert!(persona.body.contains("review code carefully"));
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn plain_markdown_uses_the_stem_and_first_line() {
        let dir = std::env::temp_dir().join("cachy-persona-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("tester.md");
        std::fs::write(&file, "# Tester\n\nAlways write tests first.\n").unwrap();
        let persona = parse_persona(&file, "local").expect("persona");
        assert_eq!(persona.name, "tester");
        assert_eq!(persona.description, "Tester");
        assert!(persona.body.contains("write tests first"));
        let _ = std::fs::remove_file(&file);
    }
}

#[cfg(test)]
mod hostile_tests {
    use super::*;

    /// Odd files a user (or a checkout) can drop in the agents directory: every
    /// one of them must be skipped or parsed, never a panic.
    #[test]
    fn hostile_persona_files_do_not_panic() {
        let dir = std::env::temp_dir().join("cachy-persona-hostile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let cases: Vec<(&str, String)> = vec![
            ("unclosed.md", "---\nname: sem fim\n".into()),
            ("dashes.md", "---".into()),
            (
                "bom.md",
                "\u{feff}---\nname: com bom\ndescription: ok\n---\ncorpo".into(),
            ),
            (
                "crlf.md",
                "---\r\nname: crlf\r\ndescription: ok\r\n---\r\ncorpo\r\n".into(),
            ),
            ("empty.md", String::new()),
            (
                "blank-front.md",
                "---\nname:\ndescription:   \n---\n".into(),
            ),
            (
                "tags.md",
                "---\nname: <script>alert(1)</script>\ndescription: 日本語 の 説明\n---\nbody"
                    .into(),
            ),
            ("huge.md", "x".repeat(200_000)),
            ("many-dashes.md", "---\n---\n---\nname: depois".into()),
        ];
        for (name, content) in cases {
            let path = dir.join(name);
            std::fs::write(&path, content).unwrap();
            if let Some(persona) = parse_persona(&path, "user") {
                assert!(!persona.name.is_empty(), "{name} parsed without a name");
                assert_eq!(persona.source, "user");
            }
        }

        // Not UTF-8 at all: skipped.
        let binary = dir.join("binary.md");
        std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x01]).unwrap();
        assert!(parse_persona(&binary, "user").is_none());

        // A directory that looks like a persona file.
        std::fs::create_dir_all(dir.join("dir.md")).unwrap();
        assert!(parse_persona(&dir.join("dir.md"), "user").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
