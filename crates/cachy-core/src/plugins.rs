//! Plugin index: a JSON list of installable plugins, so `/plugin search` has
//! something to search without a server of our own.
//!
//! ```json
//! [
//!   { "name": "team-hooks", "description": "Shared hooks", "git": "https://…/team-hooks.git" },
//!   { "name": "local-thing", "path": "/opt/local-thing" }
//! ]
//! ```
//!
//! Point `[plugins] index` at a URL or a file; installing clones (git) or copies
//! (path) the entry into `~/.cachy/plugins/<name>`.

/// One entry of the plugin index.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PluginEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Git URL to clone.
    #[serde(default)]
    pub git: Option<String>,
    /// Local directory to copy.
    #[serde(default)]
    pub path: Option<String>,
}

impl PluginEntry {
    pub fn label(&self) -> String {
        if self.description.is_empty() {
            self.name.clone()
        } else {
            format!("{} — {}", self.name, self.description)
        }
    }
}

/// Read the plugin index: an `http(s)` URL or a local file.
pub async fn fetch_index(source: &str) -> anyhow::Result<Vec<PluginEntry>> {
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        let client = reqwest::Client::builder()
            .user_agent(crate::user_agent())
            .build()?;
        client
            .get(source)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?
    } else {
        let expanded = match source.strip_prefix("~/") {
            Some(rest) => dirs::home_dir()
                .unwrap_or_default()
                .join(rest),
            None => std::path::PathBuf::from(source),
        };
        std::fs::read_to_string(expanded)?
    };
    let entries: Vec<PluginEntry> = serde_json::from_str(&text)?;
    Ok(entries)
}

/// Install one entry under the user's plugins directory.
pub fn install_entry(entry: &PluginEntry) -> anyhow::Result<std::path::PathBuf> {
    let target = crate::paths::plugins_dir().join(&entry.name);
    if target.exists() {
        anyhow::bail!("{} already exists", target.display());
    }
    std::fs::create_dir_all(crate::paths::plugins_dir())?;
    if let Some(git) = &entry.git {
        let status = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                git,
                target.to_string_lossy().as_ref(),
            ])
            .status()?;
        if !status.success() {
            anyhow::bail!("git clone failed for {git}");
        }
        return Ok(target);
    }
    if let Some(path) = &entry.path {
        let source = std::path::PathBuf::from(path);
        if !source.is_dir() {
            anyhow::bail!("{} is not a directory", source.display());
        }
        copy_dir(&source, &target)?;
        return Ok(target);
    }
    anyhow::bail!("entry \"{}\" has neither git nor path", entry.name)
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_index() {
        let entries: Vec<PluginEntry> = serde_json::from_str(
            r#"[{"name":"hooks","description":"Team hooks","git":"https://example.com/hooks.git"},
                {"name":"local","path":"/tmp/local"}]"#,
        )
        .expect("index");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label(), "hooks — Team hooks");
        assert!(entries[0].git.is_some());
        assert!(entries[1].path.is_some());
    }

    #[test]
    fn installs_from_a_local_path() {
        let source = std::env::temp_dir().join("cachy-plugin-source");
        let _ = std::fs::remove_dir_all(&source);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("plugin.json"), "{}").unwrap();
        let entry = PluginEntry {
            name: "cachy-plugin-test".into(),
            description: String::new(),
            git: None,
            path: Some(source.display().to_string()),
        };
        // Only assert the copy logic, not the user's plugins directory.
        let target = std::env::temp_dir().join("cachy-plugin-target");
        let _ = std::fs::remove_dir_all(&target);
        copy_dir(&source, &target).unwrap();
        assert!(target.join("plugin.json").is_file());
        let _ = std::fs::remove_dir_all(&source);
        let _ = std::fs::remove_dir_all(&target);
        let _ = entry;
    }
}
