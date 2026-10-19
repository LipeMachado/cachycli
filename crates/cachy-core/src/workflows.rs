//! Workflows: named, multi-step jobs that run as subagents.
//!
//! Discovered like skills and personas:
//!
//! - `<cwd>/.cachy/workflows/*.{md,json}` and `<git root>/.cachy/workflows/`
//! - `~/.cachy/workflows/*.{md,json}`
//!
//! A `.json` file is `{"name": …, "description": …, "steps": [{"title": …,
//! "prompt": …}]}`; a `.md` file uses frontmatter for name/description and one
//! `## Step: <title>` section per step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::paths;

#[derive(Debug, Clone)]
pub struct WorkflowStep {
    pub title: String,
    pub prompt: String,
}

#[derive(Debug, Clone)]
pub struct Workflow {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub source: String,
    pub path: PathBuf,
}

impl Workflow {
    pub fn label(&self) -> String {
        if self.description.is_empty() {
            format!("{} ({} steps)", self.name, self.steps.len())
        } else {
            format!(
                "{} — {} ({} steps)",
                self.name,
                self.description,
                self.steps.len()
            )
        }
    }
}

/// Every workflow visible from this directory, first source winning.
pub fn discover(cwd: &Path) -> Vec<Workflow> {
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    let mut push = |dir: PathBuf, source: &str| {
        if dir.is_dir() {
            sources.push((dir, source.to_string()));
        }
    };
    push(cwd.join(".cachy/workflows"), "local");
    if let Some(root) = crate::skills::git_root(cwd) {
        if root != cwd {
            push(root.join(".cachy/workflows"), "repo");
        }
    }
    push(paths::cachy_home().join("workflows"), "user");

    let mut map: BTreeMap<String, Workflow> = BTreeMap::new();
    for (dir, source) in sources {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(workflow) = parse_workflow(&path, &source) {
                if !workflow.steps.is_empty() {
                    map.entry(workflow.name.clone()).or_insert(workflow);
                }
            }
        }
    }
    map.into_values().collect()
}

/// Find one by name (case-insensitive).
pub fn find(cwd: &Path, name: &str) -> Option<Workflow> {
    let wanted = name.trim().to_lowercase();
    discover(cwd)
        .into_iter()
        .find(|workflow| workflow.name.to_lowercase() == wanted)
}

fn parse_workflow(path: &Path, source: &str) -> Option<Workflow> {
    let text = std::fs::read_to_string(path).ok()?;
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = path.file_stem()?.to_string_lossy().to_string();

    if extension == "json" {
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&stem)
            .to_string();
        let description = value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let steps: Vec<WorkflowStep> = value
            .get("steps")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let prompt = item.get("prompt").and_then(|v| v.as_str())?.to_string();
                        let title = item
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("step")
                            .to_string();
                        Some(WorkflowStep { title, prompt })
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Some(Workflow {
            name,
            description,
            steps,
            source: source.to_string(),
            path: path.to_path_buf(),
        });
    }

    // Markdown: optional frontmatter, then `## Step: title` sections.
    let mut name = stem;
    let mut description = String::new();
    let mut body = text.as_str();
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            body = rest[end + 4..].trim_start_matches(['\r', '\n']);
            for line in front.lines() {
                let Some((key, value)) = line.split_once(':') else { continue };
                let value = value.trim().trim_matches('"').to_string();
                match key.trim().to_lowercase().as_str() {
                    "name" if !value.is_empty() => name = value,
                    "description" if !value.is_empty() => description = value,
                    _ => {}
                }
            }
        }
    }
    let mut steps: Vec<WorkflowStep> = Vec::new();
    let mut current: Option<WorkflowStep> = None;
    for line in body.lines() {
        let is_heading = line.trim_start().starts_with("## ");
        if is_heading {
            if let Some(step) = current.take() {
                if !step.prompt.trim().is_empty() {
                    steps.push(step);
                }
            }
            let title = line
                .trim_start()
                .trim_start_matches("##")
                .trim()
                .trim_start_matches("Step:")
                .trim()
                .to_string();
            current = Some(WorkflowStep {
                title,
                prompt: String::new(),
            });
            continue;
        }
        if let Some(step) = current.as_mut() {
            step.prompt.push_str(line);
            step.prompt.push('\n');
        }
    }
    if let Some(step) = current.take() {
        if !step.prompt.trim().is_empty() {
            steps.push(step);
        }
    }
    Some(Workflow {
        name,
        description,
        steps,
        source: source.to_string(),
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_markdown_steps() {
        let dir = std::env::temp_dir().join("cachy-workflow-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("review.md");
        std::fs::write(
            &file,
            "---\nname: review\ndescription: Two passes\n---\n## Step: look\nRead the diff.\n\n## Step: judge\nList the risks.\n",
        )
        .unwrap();
        let workflow = parse_workflow(&file, "user").expect("workflow");
        assert_eq!(workflow.name, "review");
        assert_eq!(workflow.description, "Two passes");
        assert_eq!(workflow.steps.len(), 2);
        assert_eq!(workflow.steps[0].title, "look");
        assert!(workflow.steps[0].prompt.contains("Read the diff"));
        assert_eq!(workflow.steps[1].title, "judge");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn parses_json_steps() {
        let dir = std::env::temp_dir().join("cachy-workflow-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("ship.json");
        std::fs::write(
            &file,
            r#"{"name":"ship","description":"Cut a release","steps":[{"title":"test","prompt":"Run the tests"},{"title":"tag","prompt":"Tag the release"}]}"#,
        )
        .unwrap();
        let workflow = parse_workflow(&file, "local").expect("workflow");
        assert_eq!(workflow.name, "ship");
        assert_eq!(workflow.steps.len(), 2);
        assert_eq!(workflow.steps[1].title, "tag");
        let _ = std::fs::remove_file(&file);
    }
}

#[cfg(test)]
mod hostile_tests {
    use super::*;

    #[test]
    fn hostile_workflow_files_do_not_panic() {
        let dir = std::env::temp_dir().join("cachy-workflow-hostile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let markdown: Vec<(&str, String)> = vec![
            ("unclosed.md", "---\nname: sem fim\n".into()),
            ("empty.md", String::new()),
            ("bom.md", "\u{feff}---\nname: bom\n---\n## Step: um\nfaça algo".into()),
            ("crlf.md", "---\r\nname: crlf\r\n---\r\n## Step: um\r\nfaça".into()),
            ("no-steps.md", "---\nname: sem passos\n---\nsó texto".into()),
            ("huge.md", format!("---\nname: grande\n---\n{}", "passo ".repeat(20_000))),
        ];
        for (name, content) in markdown {
            let path = dir.join(name);
            std::fs::write(&path, content).unwrap();
            let _ = parse_workflow(&path, "user");
        }

        let json: Vec<(&str, &str)> = vec![
            ("empty.json", "{}"),
            ("broken.json", "{ nao e json }"),
            ("steps-not-array.json", r#"{"name":"x","steps":"nao"}"#),
            ("steps-empty.json", r#"{"name":"x","steps":[{}]}"#),
            ("steps-null.json", r#"{"name":null,"steps":[null]}"#),
            ("huge.json", r#"{"name":"x","steps":[{"prompt":"p","title":"t"}]}"#),
        ];
        for (name, content) in json {
            let path = dir.join(name);
            std::fs::write(&path, content).unwrap();
            let _ = parse_workflow(&path, "user");
        }

        let binary = dir.join("binary.md");
        std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
        assert!(parse_workflow(&binary, "user").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
