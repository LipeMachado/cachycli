//! Credentials: `~/.cachy/auth.json`, environment keys and external auth providers.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::paths;

/// Auth state persisted at `~/.cachy/auth.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthState {
    #[serde(default)]
    pub version: u32,
    /// Provider name -> credential.
    #[serde(default)]
    pub providers: BTreeMap<String, Credential>,
    /// Default API key used for OpenAI-compatible endpoints when no
    /// provider-specific key is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Credential {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl AuthState {
    pub fn is_authenticated(&self) -> bool {
        self.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false)
            || self.providers.values().any(|c| {
                c.key.as_ref().map(|k| !k.is_empty()).unwrap_or(false)
                    || c.access_token.as_ref().map(|k| !k.is_empty()).unwrap_or(false)
            })
            || std::env::var("OPENAI_API_KEY").is_ok()
            || std::env::var("ANTHROPIC_API_KEY").is_ok()
            || std::env::var("OPENCODE_API_KEY").is_ok()
    }
}

pub fn load() -> AuthState {
    let path = paths::auth_file();
    if !path.is_file() {
        return AuthState::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(state: &AuthState) -> Result<()> {
    paths::ensure_dirs().ok();
    let path = paths::auth_file();
    let json = serde_json::to_string_pretty(state)?;
    write_private(&path, &json)?;
    Ok(())
}

pub fn clear() -> Result<()> {
    let path = paths::auth_file();
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

/// Write a file with 0600 permissions on unix.
fn write_private(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Resolve credentials for a model base URL: explicit api key, env keys,
/// auth.json providers, then external auth provider.
pub fn credential_for(config: &Config, base_url: &str, explicit: Option<&str>, env_keys: &[String]) -> Option<String> {
    if let Some(k) = explicit {
        if !k.is_empty() {
            return Some(k.to_string());
        }
    }
    for key in env_keys {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    let state = load();
    if let Some(k) = &state.api_key {
        if !k.is_empty() {
            return Some(k.clone());
        }
    }
    // Match by provider host in provider name or stored provider entries.
    let host = base_url
        .split("//")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .unwrap_or("");
    for (name, cred) in &state.providers {
        let key = cred.key.clone().or_else(|| cred.access_token.clone());
        if let Some(key) = key {
            if !key.is_empty() && (name.contains(host) || host.contains(name.as_str())) {
                return Some(key);
            }
        }
    }
    if let Some(cmd) = external_provider_command(config) {
        if let Ok(token) = run_external_provider(&cmd) {
            return Some(token);
        }
    }
    None
}

/// `[auth] auth_provider_command` or `CACHY_AUTH_PROVIDER_COMMAND`.
pub fn external_provider_command(config: &Config) -> Option<String> {
    if let Some(cmd) = &config.auth.auth_provider_command {
        if !cmd.is_empty() {
            return Some(cmd.clone());
        }
    }
    std::env::var("CACHY_AUTH_PROVIDER_COMMAND").ok().filter(|s| !s.is_empty())
}

/// Run the external auth provider. Semantics:
/// - the command prints the token on stdout
/// - status/diagnostic text goes to stderr
/// - JSON output (`{"token": "...", "expires_in": n}`) enables refresh info
pub fn run_external_provider(command: &str) -> Result<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .with_context(|| format!("running auth provider {command}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "auth provider failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        anyhow::bail!("auth provider printed no token on stdout");
    }
    if stdout.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if let Some(t) = v.get("token").and_then(|t| t.as_str()) {
                return Ok(t.to_string());
            }
        }
    }
    Ok(stdout)
}
