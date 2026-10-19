//! Filesystem layout for cachycli.
//!
//! Everything lives under `CACHY_HOME` (default `~/.cachy`):
//!
//! ```text
//! ~/.cachy/
//!   config.toml            # user configuration
//!   auth.json              # credentials (api keys, oauth tokens)
//!   models_cache.json      # cached model catalog
//!   version.json           # installed version bookkeeping
//!   sessions/              # persisted sessions, grouped by working directory
//!   memory/                # cross-session memory scopes
//!   skills/                # user scoped skills
//!   agents/                # user scoped agent definitions
//!   plugins/               # user scoped plugins
//!   logs/                  # log files
//! ```

use std::path::{Path, PathBuf};

/// Environment variable overriding the cachy home directory.
pub const HOME_ENV: &str = "CACHY_HOME";

/// Legacy/preferred home directory name under `$HOME`.
pub const HOME_DIR_NAME: &str = ".cachy";

/// Return the cachy home directory (creating nothing).
pub fn cachy_home() -> PathBuf {
    if let Ok(p) = std::env::var(HOME_ENV) {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(HOME_DIR_NAME)
}

pub fn config_file() -> PathBuf {
    cachy_home().join("config.toml")
}

pub fn managed_config_file() -> PathBuf {
    cachy_home().join("managed_config.toml")
}

pub fn requirements_file() -> PathBuf {
    cachy_home().join("requirements.toml")
}

pub fn auth_file() -> PathBuf {
    cachy_home().join("auth.json")
}

pub fn models_cache_file() -> PathBuf {
    cachy_home().join("models_cache.json")
}

pub fn version_file() -> PathBuf {
    cachy_home().join("version.json")
}

pub fn sessions_dir() -> PathBuf {
    cachy_home().join("sessions")
}

pub fn memory_dir() -> PathBuf {
    cachy_home().join("memory")
}

pub fn skills_dir() -> PathBuf {
    cachy_home().join("skills")
}

pub fn agents_dir() -> PathBuf {
    cachy_home().join("agents")
}

pub fn plugins_dir() -> PathBuf {
    cachy_home().join("plugins")
}

pub fn logs_dir() -> PathBuf {
    cachy_home().join("logs")
}

pub fn debug_dir() -> PathBuf {
    cachy_home().join("debug")
}

pub fn completions_dir() -> PathBuf {
    cachy_home().join("completions")
}

pub fn lsp_file() -> PathBuf {
    cachy_home().join("lsp.json")
}

pub fn sandbox_file() -> PathBuf {
    cachy_home().join("sandbox.toml")
}

pub fn bin_dir() -> PathBuf {
    cachy_home().join("bin")
}

pub fn downloads_dir() -> PathBuf {
    cachy_home().join("downloads")
}

/// URL-encode a working directory into the session grouping name used on disk.
/// The directory name for a working directory: `/` becomes `%2F`, so sessions
/// for `/home/me/proj` live in `<home>/sessions/%2Fhome%2Fme%2Fproj`.
pub fn encode_cwd(cwd: &Path) -> String {
    let mut out = String::new();
    for ch in cwd.to_string_lossy().chars() {
        match ch {
            '/' => out.push_str("%2F"),
            '\\' => out.push_str("%5C"),
            ':' => out.push_str("%3A"),
            ' ' => out.push_str("%20"),
            '%' => out.push_str("%25"),
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') => out.push(c),
            c => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

/// Directory holding sessions for one working directory.
pub fn sessions_dir_for(cwd: &Path) -> PathBuf {
    sessions_dir().join(encode_cwd(cwd))
}

/// Project scoped `.cachy` directory inside a workspace.
pub fn project_dir(cwd: &Path) -> PathBuf {
    cwd.join(".cachy")
}

/// Claude-compatible directory fallback (skills/agents/plugins are also read).
pub fn claude_dir(cwd: &Path) -> PathBuf {
    cwd.join(".claude")
}

pub fn user_claude_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

/// Ensure all base directories exist.
pub fn ensure_dirs() -> std::io::Result<()> {
    for dir in [
        cachy_home(),
        sessions_dir(),
        memory_dir(),
        skills_dir(),
        agents_dir(),
        plugins_dir(),
        logs_dir(),
        debug_dir(),
    ] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Compress a path for display: replace `$HOME` with `~` and shorten
/// intermediate directories, e.g. `~/P/lipecode/cachycli`.
pub fn display_path(path: &Path) -> String {
    let raw = path.to_string_lossy().to_string();
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy().to_string();
        if let Some(rest) = raw.strip_prefix(&home) {
            let rest = rest.trim_start_matches('/');
            if rest.is_empty() {
                return "~".into();
            }
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() <= 1 {
                return format!("~/{rest}");
            }
            let mut out = String::from("~/");
            for (i, part) in parts.iter().enumerate() {
                if i + 1 == parts.len() {
                    out.push_str(part);
                } else if part.is_empty() {
                    continue;
                } else {
                    out.push(part.chars().next().unwrap_or(' '));
                    out.push('/');
                }
            }
            return out;
        }
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_cwd() {
        assert_eq!(encode_cwd(Path::new("/home/lipe/x")), "%2Fhome%2Flipe%2Fx");
    }

    #[test]
    fn compresses_display_path() {
        let home = dirs::home_dir().unwrap();
        let p = home.join("Projects/lipecode/cachycli");
        let d = display_path(&p);
        assert!(d.starts_with("~/"), "{d}");
        assert!(d.ends_with("cachycli"), "{d}");
    }
}
