//! Hooks: project lifecycle scripts run on tool and session events.
//!
//! Discovery:
//! - `.cachy/hooks/*.json` (project; requires folder trust)
//! - `[[hooks.<Event>]]` tables in any config layer (additive)
//!
//! Hook file schema:
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       { "matcher": "Bash|Write", "hooks": [ { "type": "command", "command": "/path/script.sh", "timeout": 10 } ] }
//!     ]
//!   }
//! }
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
}

impl HookEvent {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "sessionstart" | "session_start" => Self::SessionStart,
            "sessionend" | "session_end" => Self::SessionEnd,
            "userpromptsubmit" | "user_prompt_submit" => Self::UserPromptSubmit,
            "pretooluse" | "pre_tool_use" => Self::PreToolUse,
            "posttooluse" | "post_tool_use" => Self::PostToolUse,
            "stop" => Self::Stop,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::Stop => "Stop",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookCommand {
    #[serde(rename = "type", default = "default_type")]
    pub kind: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_type() -> String {
    "command".into()
}
fn default_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookMatcher {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookCommand>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredHook {
    pub event: HookEvent,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout: u64,
    pub source: String,
}

/// Trust state for hook folders.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrustStore {
    #[serde(default)]
    pub trusted_folders: Vec<String>,
}

pub fn trust_store_path() -> PathBuf {
    paths::cachy_home().join("trusted_folders.json")
}

pub fn is_folder_trusted(cwd: &Path) -> bool {
    let store: TrustStore = std::fs::read_to_string(trust_store_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    store
        .trusted_folders
        .iter()
        .any(|f| Path::new(f).canonicalize().map(|p| p == canonical).unwrap_or(false))
}

pub fn trust_folder(cwd: &Path) -> std::io::Result<()> {
    let mut store: TrustStore = std::fs::read_to_string(trust_store_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let s = canonical.to_string_lossy().to_string();
    if !store.trusted_folders.contains(&s) {
        store.trusted_folders.push(s);
    }
    paths::ensure_dirs().ok();
    std::fs::write(trust_store_path(), serde_json::to_string_pretty(&store)?)?;
    Ok(())
}

/// Discover hooks from project files and config layers.
pub fn discover(cwd: &Path, config: &Config) -> Vec<DiscoveredHook> {
    let mut out = Vec::new();
    // Project hook files (only when trusted).
    let hooks_dir = cwd.join(".cachy").join("hooks");
    if hooks_dir.is_dir() && is_folder_trusted(cwd) {
        if let Ok(entries) = std::fs::read_dir(&hooks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(raw) = std::fs::read_to_string(&path) {
                        out.extend(parse_hook_file(&raw, &path.display().to_string()));
                    }
                }
            }
        }
    }
    // Config-defined hooks.
    for (event, matchers) in config_hooks(config) {
        for matcher in matchers {
            for hook in matcher.hooks {
                out.push(DiscoveredHook {
                    event,
                    matcher: matcher.matcher.clone(),
                    command: hook.command,
                    timeout: hook.timeout,
                    source: "config".into(),
                });
            }
        }
    }
    out
}

fn config_hooks(config: &Config) -> Vec<(HookEvent, Vec<HookMatcher>)> {
    let _ = config;
    Vec::new()
}

pub fn parse_hook_file(raw: &str, source: &str) -> Vec<DiscoveredHook> {
    let mut out = Vec::new();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else { return out };
    let Some(hooks) = value.get("hooks").and_then(|h| h.as_object()) else { return out };
    for (event_name, matchers) in hooks {
        let Some(event) = HookEvent::parse(event_name) else { continue };
        let Some(list) = matchers.as_array() else { continue };
        for matcher_value in list {
            let Ok(matcher) = serde_json::from_value::<HookMatcher>(matcher_value.clone()) else { continue };
            for hook in matcher.hooks {
                out.push(DiscoveredHook {
                    event,
                    matcher: matcher.matcher.clone(),
                    command: hook.command,
                    timeout: hook.timeout,
                    source: source.to_string(),
                });
            }
        }
    }
    out
}

/// Does a matcher (regex-ish alternation of tool names) apply to a tool?
pub fn matcher_applies(matcher: &Option<String>, tool: &str) -> bool {
    let Some(matcher) = matcher else { return true };
    if matcher.trim().is_empty() {
        return true;
    }
    matcher
        .split('|')
        .map(|p| p.trim())
        .any(|pattern| {
            if pattern.ends_with('*') {
                let prefix = pattern.trim_end_matches('*');
                tool.starts_with(prefix)
            } else {
                pattern.eq_ignore_ascii_case(tool)
            }
        })
}

/// Run the hooks for an event. Returns combined stdout of all hooks.
pub async fn run_event(
    hooks: &[DiscoveredHook],
    event: HookEvent,
    cwd: &Path,
    tool: Option<&str>,
    payload: &serde_json::Value,
) -> Vec<(String, String, bool)> {
    let mut results = Vec::new();
    for hook in hooks.iter().filter(|h| h.event == event) {
        if let Some(tool) = tool {
            if !matcher_applies(&hook.matcher, tool) {
                continue;
            }
        }
        let mut cmd = crate::tools::bash::shell_command(&hook.command);
        cmd.current_dir(cwd);
        cmd.env("CACHY_HOOK_EVENT", event.as_str());
        cmd.env("CACHY_HOOK_PAYLOAD", payload.to_string());
        if let Some(tool) = tool {
            cmd.env("CACHY_HOOK_TOOL", tool);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let result = tokio::time::timeout(std::time::Duration::from_secs(hook.timeout.max(1)), async {
            let child = cmd.spawn()?;
            child.wait_with_output().await
        })
        .await;
        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                results.push((hook.command.clone(), if stderr.is_empty() { stdout } else { stderr }, output.status.success()));
            }
            Ok(Err(e)) => results.push((hook.command.clone(), format!("spawn failed: {e}"), false)),
            Err(_) => results.push((hook.command.clone(), format!("timed out after {}s", hook.timeout), false)),
        }
    }
    results
}

/// Summary for `/hooks-list`.
pub fn describe(hooks: &[DiscoveredHook]) -> Vec<String> {
    let mut grouped: BTreeMap<String, Vec<&DiscoveredHook>> = BTreeMap::new();
    for hook in hooks {
        grouped
            .entry(hook.event.as_str().to_string())
            .or_default()
            .push(hook);
    }
    let mut out = Vec::new();
    for (event, list) in grouped {
        for hook in list {
            out.push(format!(
                "{event} [{}] {}{}",
                hook.source,
                hook.command,
                hook.matcher
                    .as_ref()
                    .map(|m| format!(" (matcher: {m})"))
                    .unwrap_or_default()
            ));
        }
    }
    out
}
