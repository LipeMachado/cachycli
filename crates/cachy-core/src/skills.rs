//! Skill discovery and loading.
//!
//! Skills are directories containing `SKILL.md` with YAML-ish frontmatter:
//!
//! ```markdown
//! ---
//! name: commit
//! description: Create well-formatted git commits.
//! ---
//! ...instructions...
//! ```
//!
//! Discovery order (highest priority first):
//! 1. `./.cachy/skills/`
//! 2. `<repo root>/.cachy/skills/`
//! 3. `~/.cachy/skills/`
//! 4. `[skills] paths` from config
//! 5. `./.claude/skills/`, `~/.claude/skills/` (Claude Code compat)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Full markdown body (without frontmatter).
    pub body: String,
    pub path: PathBuf,
    pub source: String,
}

/// Parse a `SKILL.md` file: frontmatter + body.
pub fn parse_skill(path: &Path, source: &str) -> Option<Skill> {
    let raw = std::fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&raw);
    let mut name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut description = String::new();
    for line in frontmatter.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
            match key.as_str() {
                "name" => {
                    if !value.is_empty() {
                        name = value;
                    }
                }
                "description" => description = value,
                _ => {}
            }
        }
    }
    if name.is_empty() {
        return None;
    }
    Some(Skill {
        name,
        description,
        body: body.trim().to_string(),
        path: path.to_path_buf(),
        source: source.to_string(),
    })
}

fn split_frontmatter(raw: &str) -> (String, String) {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = rest[..end].to_string();
            let body = rest[end + 4..].to_string();
            return (fm, body);
        }
    }
    (String::new(), raw.to_string())
}

/// Discover all visible skills, deduplicated by name (first wins).
pub fn discover(cwd: &Path, config: &Config) -> Vec<Skill> {
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    let mut push = |dir: PathBuf, source: &str| {
        if dir.is_dir() {
            sources.push((dir, source.to_string()));
        }
    };
    push(cwd.join(".cachy/skills"), "local");
    if let Some(root) = git_root(cwd) {
        if root != cwd {
            push(root.join(".cachy/skills"), "repo");
        }
    }
    push(paths::skills_dir(), "user");
    for extra in &config.skills.paths {
        let expanded = shellexpand(extra);
        push(expanded, "config");
    }
    push(cwd.join(".claude/skills"), "claude-local");
    push(paths::user_claude_dir().join("skills"), "claude-user");

    let mut map: BTreeMap<String, Skill> = BTreeMap::new();
    for (dir, source) in sources {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let skill_md = entry.path().join("SKILL.md");
            if skill_md.is_file() {
                if let Some(skill) = parse_skill(&skill_md, &source) {
                    map.entry(skill.name.clone()).or_insert(skill);
                }
            }
        }
    }
    map.into_values().collect()
}

pub fn find<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    skills.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// Summary list injected into the system prompt.
pub fn prompt_index(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut out = String::from("<skills>\n");
    for s in skills {
        out.push_str(&format!("- {}: {}\n", s.name, s.description));
    }
    out.push_str("</skills>");
    out
}

pub fn git_root(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

fn shellexpand(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter() {
        let dir = std::env::temp_dir().join(format!("skill-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("commit")).unwrap();
        std::fs::write(
            dir.join("commit/SKILL.md"),
            "---\nname: commit\ndescription: Make commits.\n---\n\n# Steps\n1. Do it\n",
        )
        .unwrap();
        let s = parse_skill(&dir.join("commit/SKILL.md"), "user").unwrap();
        assert_eq!(s.name, "commit");
        assert_eq!(s.description, "Make commits.");
        assert!(s.body.contains("# Steps"));
        std::fs::remove_dir_all(dir).ok();
    }
}

#[cfg(test)]
mod hostile_tests {
    use super::*;

    #[test]
    fn hostile_skill_files_do_not_panic() {
        let dir = std::env::temp_dir().join("cachy-skill-hostile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("commit")).unwrap();

        let cases: Vec<(&str, String)> = vec![
            ("SKILL.md", "---\nname: commit\ndescription: ok\n---\ncorpo".into()),
            ("SKILL.md", "---\nname:\n---\ncorpo".into()),
            ("SKILL.md", "---\nsem fim\n".into()),
            ("SKILL.md", "\u{feff}---\nname: com bom\n---\ncorpo".into()),
            ("SKILL.md", "---\r\nname: crlf\r\n---\r\ncorpo".into()),
            ("SKILL.md", String::new()),
            ("SKILL.md", "---\ndescription: 日本語\n---\ncorpo".into()),
        ];
        for (name, content) in cases {
            let path = dir.join("commit").join(name);
            std::fs::write(&path, content).unwrap();
            let _ = parse_skill(&path, "user");
        }

        let binary = dir.join("commit/SKILL.md");
        std::fs::write(&binary, [0xff, 0xfe]).unwrap();
        assert!(parse_skill(&binary, "user").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
