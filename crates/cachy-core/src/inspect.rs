//! `cachycli inspect` — show everything discovered for a directory.

use std::path::Path;

use serde::Serialize;

use crate::config::{Config, LoadedConfig};
use crate::hooks;
use crate::paths;
use crate::prompt;
use crate::skills;

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub cwd: String,
    pub config_sources: Vec<String>,
    pub instructions: Vec<InstructionFile>,
    pub skills: Vec<SkillInfo>,
    pub agents: Vec<String>,
    pub mcp_servers: Vec<McpInfo>,
    pub hooks: Vec<String>,
    pub models: Vec<ModelInfo>,
    pub backend: String,
    pub default_model: Option<String>,
    pub memory_enabled: bool,
    pub trusted_folder: bool,
}

#[derive(Debug, Serialize)]
pub struct InstructionFile {
    pub path: String,
    pub chars: usize,
}

#[derive(Debug, Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub source: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct McpInfo {
    pub name: String,
    pub transport: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub name: String,
    pub model_id: String,
    pub base_url: String,
    pub backend: String,
    pub builtin: bool,
}

pub fn inspect(cwd: &Path, loaded: &LoadedConfig) -> Inspection {
    let config = &loaded.config;
    let instruction_files = prompt::discover_rule_files(cwd)
        .into_iter()
        .filter_map(|path| {
            let chars = std::fs::read_to_string(&path).ok().map(|c| c.len())?;
            Some(InstructionFile { path: path.display().to_string(), chars })
        })
        .collect();

    let skill_list = skills::discover(cwd, config)
        .into_iter()
        .map(|s| SkillInfo {
            name: s.name,
            description: s.description,
            source: s.source,
            path: s.path.display().to_string(),
        })
        .collect();

    let agents = discover_agents(cwd);

    let mcp_servers = config
        .mcp_servers
        .iter()
        .map(|(name, cfg)| McpInfo {
            name: name.clone(),
            transport: if cfg.url.is_some() { "http".into() } else { "stdio".into() },
            enabled: cfg.enabled,
        })
        .collect();

    let hook_list = hooks::discover(cwd, config).iter().map(|h| {
        format!(
            "{} [{}] {}{}",
            h.event.as_str(),
            h.source,
            h.command,
            h.matcher.as_ref().map(|m| format!(" (matcher: {m})")).unwrap_or_default()
        )
    }).collect();

    let model_list = crate::models::catalog(config)
        .into_iter()
        .map(|m| ModelInfo {
            name: m.name,
            model_id: m.model_id,
            base_url: m.base_url,
            backend: m.api_backend.as_str().to_string(),
            builtin: m.builtin,
        })
        .collect();

    Inspection {
        cwd: cwd.display().to_string(),
        config_sources: loaded.sources.iter().map(|p| p.display().to_string()).collect(),
        instructions: instruction_files,
        skills: skill_list,
        agents,
        mcp_servers,
        hooks: hook_list,
        models: model_list,
        backend: format!("{:?}", config.agent.effective_backend()).to_lowercase(),
        default_model: config.models.default.clone(),
        memory_enabled: config.memory.enabled
            || std::env::var("CACHY_MEMORY").map(|v| v != "0").unwrap_or(false),
        trusted_folder: hooks::is_folder_trusted(cwd),
    }
}

pub fn discover_agents(cwd: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![
        paths::agents_dir(),
        dirs::home_dir().unwrap_or_default().join(".claude/agents"),
        cwd.join(".cachy/agents"),
        cwd.join(".claude/agents"),
    ];
    dirs.dedup();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md" || e == "toml").unwrap_or(false) {
                let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out
}

/// Human-readable rendering of an inspection.
pub fn render(inspection: &Inspection) -> String {
    let mut out = String::new();
    out.push_str(&format!("Working directory: {}\n", inspection.cwd));
    out.push_str(&format!(
        "Backend: {}    Default model: {}\n",
        inspection.backend,
        inspection.default_model.clone().unwrap_or_else(|| "(none)".into())
    ));
    out.push_str(&format!("Memory: {}\n", if inspection.memory_enabled { "enabled" } else { "disabled" }));
    out.push_str(&format!(
        "Folder trusted for hooks: {}\n\n",
        if inspection.trusted_folder { "yes" } else { "no" }
    ));

    out.push_str("Config sources:\n");
    for source in &inspection.config_sources {
        out.push_str(&format!("  - {source}\n"));
    }
    out.push('\n');

    out.push_str(&format!("Project instructions ({}):\n", inspection.instructions.len()));
    for file in &inspection.instructions {
        out.push_str(&format!("  - {} ({} chars)\n", file.path, file.chars));
    }
    out.push('\n');

    out.push_str(&format!("Skills ({}):\n", inspection.skills.len()));
    for skill in &inspection.skills {
        out.push_str(&format!("  - {} [{}] {}\n", skill.name, skill.source, skill.description));
    }
    out.push('\n');

    out.push_str(&format!("Agents ({}):\n", inspection.agents.len()));
    for agent in &inspection.agents {
        out.push_str(&format!("  - {agent}\n"));
    }
    out.push('\n');

    out.push_str(&format!("MCP servers ({}):\n", inspection.mcp_servers.len()));
    for server in &inspection.mcp_servers {
        out.push_str(&format!(
            "  - {} ({}): {}\n",
            server.name,
            server.transport,
            if server.enabled { "enabled" } else { "disabled" }
        ));
    }
    out.push('\n');

    out.push_str(&format!("Hooks ({}):\n", inspection.hooks.len()));
    for hook in &inspection.hooks {
        out.push_str(&format!("  - {hook}\n"));
    }
    out.push('\n');

    out.push_str(&format!("Models ({}):\n", inspection.models.len()));
    for model in &inspection.models {
        out.push_str(&format!(
            "  - {} -> {} @ {} ({}{})\n",
            model.name,
            model.model_id,
            model.base_url,
            model.backend,
            if model.builtin { ", builtin" } else { "" }
        ));
    }
    out
}

/// Utility used by commands that need a config without failing on parse errors.
pub fn load_config_lenient(cwd: &Path) -> (Config, Vec<String>) {
    match crate::config::load(cwd) {
        Ok(loaded) => {
            let sources = loaded.sources.iter().map(|p| p.display().to_string()).collect();
            (loaded.config, sources)
        }
        Err(_) => (Config::default(), Vec::new()),
    }
}
