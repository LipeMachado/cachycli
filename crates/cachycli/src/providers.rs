//! Provider discovery, status and sign-in.
//!
//! A provider is the thing that answers your prompts: your opencode install,
//! Claude Code, a model you configured, or an editor agent. The interface
//! exposes this through `/provider`.

use std::path::Path;

use anyhow::Result;
use cachy_core::config::{BackendKind, Config};

#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub id: BackendKind,
    /// Name shown in the picker.
    pub label: &'static str,
    /// One line about what it is.
    pub summary: &'static str,
    pub installed: bool,
    pub connected: bool,
    /// Command that signs you in (run interactively).
    pub login: Vec<String>,
    /// Where to get it when it is not installed.
    pub install_hint: &'static str,
    /// Extra status text (account, model, …).
    pub detail: String,
}

/// Every provider the CLI can talk to, with its current state.
pub fn detect(config: &Config) -> Vec<ProviderInfo> {
    vec![opencode(), claude_code(), native(config), acp(config)]
}

pub fn opencode() -> ProviderInfo {
    let binary = which("opencode");
    let mut detail = String::new();
    let mut connected = false;
    if binary.is_some() {
        if let Some(path) = opencode_auth_file() {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(map) = value.as_object() {
                        let entries: Vec<&String> = map.keys().collect();
                        if !entries.is_empty() {
                            connected = true;
                            detail = entries
                                .into_iter()
                                .take(3)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ");
                        }
                    }
                }
            }
        }
    }
    ProviderInfo {
        id: BackendKind::Opencode,
        label: "opencode",
        summary: "your opencode install",
        installed: binary.is_some(),
        connected,
        // `console login` is the device flow (browser + code, no API key);
        // `auth login` is the key prompt. sign_in tries them in order.
        login: vec!["opencode".into(), "console".into(), "login".into()],
        install_hint: "https://opencode.ai",
        detail,
    }
}

pub fn claude_code() -> ProviderInfo {
    let binary = which("claude");
    let mut connected = false;
    let mut detail = String::new();
    if binary.is_some() {
        let home = dirs::home_dir().unwrap_or_default();
        let candidates = [
            home.join(".claude").join(".credentials.json"),
            home.join(".claude.json"),
            home.join(".config").join("claude").join("credentials.json"),
        ];
        for candidate in candidates {
            if let Ok(raw) = std::fs::read_to_string(&candidate) {
                if raw.contains("oauth") || raw.contains("accessToken") || raw.contains("api_key") {
                    connected = true;
                    break;
                }
            }
        }
        if connected {
            detail = "signed in".into();
        }
    }
    ProviderInfo {
        id: BackendKind::ClaudeCode,
        label: "Claude Code",
        summary: "the Claude Code CLI",
        installed: binary.is_some(),
        connected,
        login: vec!["claude".into()],
        install_hint: "npm install -g @anthropic-ai/claude-code",
        detail,
    }
}

pub fn native(config: &Config) -> ProviderInfo {
    let catalog = cachy_core::models::catalog(config);
    let ready = catalog
        .iter()
        .filter(|m| {
            cachy_core::models::api_key_for(m).is_some() || m.base_url.contains("localhost")
        })
        .count();
    let default = config
        .models
        .default
        .clone()
        .unwrap_or_else(|| "not set".into());
    ProviderInfo {
        id: BackendKind::Native,
        label: "Your models",
        summary: "your own model endpoints",
        installed: true,
        connected: ready > 0,
        login: vec!["cachycli".into(), "login".into()],
        install_hint: "add a [model.<name>] section to ~/.cachy/config.toml",
        detail: if ready > 0 {
            format!("{ready} ready · default {default}")
        } else {
            "no credentials yet".into()
        },
    }
}

pub fn acp(config: &Config) -> ProviderInfo {
    let command = config.agent.acp.command.clone();
    let url = config.agent.acp.url.clone();
    // A remote agent needs no local command; a url is enough.
    let installed = url.is_some() || which(&command).is_some();
    ProviderInfo {
        id: BackendKind::Acp,
        label: "Editor agent",
        summary: "an editor agent (ACP)",
        installed,
        connected: installed,
        login: Vec::new(),
        install_hint: "set [agent.acp] command (or url) in ~/.cachy/config.toml",
        detail: match (&url, installed) {
            (Some(url), _) => url.clone(),
            (None, true) => command,
            (None, false) => "command not found".into(),
        },
    }
}

/// Which provider to use when nothing is configured: the first one that is
/// installed and already signed in, else plain models.
pub fn default(config: &Config) -> BackendKind {
    if config.agent.provider.is_some() || config.agent.backend.is_some() {
        return config.agent.effective_backend();
    }
    for candidate in [opencode(), claude_code()] {
        if candidate.installed && candidate.connected {
            return candidate.id;
        }
    }
    for candidate in [opencode(), claude_code()] {
        if candidate.installed {
            return candidate.id;
        }
    }
    BackendKind::Native
}

/// Run a provider's sign-in flow in the foreground (the caller suspends the
/// interface first so the provider can open a browser and ask questions).
pub async fn sign_in(info: &ProviderInfo) -> Result<String> {
    if info.login.is_empty() {
        anyhow::bail!("{} has no sign-in step; {}", info.label, info.install_hint);
    }
    // Try the listed command, then any fallback the provider offers (opencode:
    // device login first, API-key prompt second).
    let mut candidates: Vec<Vec<String>> = vec![info.login.clone()];
    if info.id == BackendKind::Opencode {
        candidates.push(vec!["opencode".into(), "auth".into(), "login".into()]);
    }
    let mut last = String::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let (program, args) = candidate.split_first().expect("login command");
        match tokio::process::Command::new(program)
            .args(args)
            .status()
            .await
        {
            Ok(status) if status.success() => {
                return Ok(format!("{} sign-in finished", info.label))
            }
            Ok(status) => {
                last = format!("{program} {} exited with {status}", args.join(" "));
                let more = index + 1 < candidates.len();
                if !more {
                    return Ok(format!("{} sign-in: {last}", info.label));
                }
            }
            Err(e) => return Err(anyhow::anyhow!("could not run `{program}`: {e}")),
        }
    }
    Ok(format!("{} sign-in: {last}", info.label))
}

fn which(name: &str) -> Option<String> {
    if name.contains('/') {
        return Path::new(name).is_file().then(|| name.to_string());
    }
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

fn opencode_auth_file() -> Option<std::path::PathBuf> {
    if let Ok(data) = std::env::var("XDG_DATA_HOME") {
        let path = std::path::Path::new(&data)
            .join("opencode")
            .join("auth.json");
        if path.is_file() {
            return Some(path);
        }
    }
    let home = dirs::home_dir()?;
    [
        home.join(".local/share/opencode/auth.json"),
        home.join(".config/opencode/auth.json"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}
