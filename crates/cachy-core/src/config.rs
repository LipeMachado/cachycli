//! Configuration model and layered loading.
//!
//! Precedence (lowest to highest):
//! 1. Built-in defaults
//! 2. `~/.cachy/managed_config.toml` (enterprise managed settings)
//! 3. `~/.cachy/config.toml` (user config)
//! 4. `<project>/.cachy/config.toml` (project config; MCP servers and models)
//! 5. CLI flags / environment variables

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Which agent backend (provider) a session talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    /// Built-in agent loop with an OpenAI/Anthropic compatible model.
    #[default]
    Native,
    /// Drive an `opencode` install (its sessions, models and tools).
    Opencode,
    /// Drive the Claude Code CLI in headless streaming mode.
    ClaudeCode,
    /// Talk to any Agent Client Protocol agent over stdio.
    Acp,
}

impl BackendKind {
    /// Human label used in the interface and in `[agent] provider`.
    pub fn provider_name(&self) -> &'static str {
        match self {
            BackendKind::Native => "native",
            BackendKind::Opencode => "opencode",
            BackendKind::ClaudeCode => "claude-code",
            BackendKind::Acp => "acp",
        }
    }

    /// Parse a provider name from config or the CLI.
    pub fn from_provider(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "opencode" | "oc" => BackendKind::Opencode,
            "claude-code" | "claude" | "cc" => BackendKind::ClaudeCode,
            "native" | "models" | "model" => BackendKind::Native,
            "acp" | "editor" => BackendKind::Acp,
            _ => return None,
        })
    }
}

/// Root configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub cli: CliConfig,
    pub models: ModelsConfig,
    pub endpoints: EndpointsConfig,
    pub agent: AgentConfig,
    pub features: FeaturesConfig,
    pub ui: UiConfig,
    pub session: SessionConfig,
    pub share: ShareConfig,
    pub tools: ToolsConfig,
    pub toolset: ToolsetConfig,
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    pub skills: SkillsConfig,
    pub plugins: PluginsConfig,
    pub subagents: SubagentsConfig,
    pub memory: MemoryConfig,
    pub auth: AuthConfig,
    /// Custom model definitions: `[model.<name>]`.
    #[serde(rename = "model")]
    pub custom_models: BTreeMap<String, ModelConfig>,
    /// User keybindings: `[keys] send = "ctrl+j"`.
    pub keys: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CliConfig {
    pub auto_update: bool,
    /// Default output format for headless mode.
    pub output_format: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelsConfig {
    /// Default model name for new sessions.
    pub default: Option<String>,
    /// Model used by the `web_search` tool.
    pub web_search: Option<String>,
    /// Fallback model used when the default is unavailable.
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EndpointsConfig {
    /// Base URL for model listing/inference (OpenAI compatible).
    pub models_base_url: Option<String>,
    pub models_list_url: Option<String>,
}

/// Which backend the interactive agent uses, plus per-backend settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    /// Selected backend. `None` means "not specified in this layer".
    pub backend: Option<BackendKind>,
    /// Provider name (`opencode`, `claude-code`, `native`, `acp`). Takes
    /// precedence over `backend` and is what `/provider` writes.
    pub provider: Option<String>,
    /// Agent definition name or path used for new sessions.
    pub name: Option<String>,
    /// Reasoning effort for models that support it (`low`, `medium`, `high`).
    pub reasoning_effort: Option<String>,
    /// How many verifier subagents `/goal verify` runs.
    pub goal_verifier_count: usize,
    pub definition: Option<String>,
    /// opencode backend settings.
    pub opencode: OpencodeConfig,
    /// ACP backend settings.
    pub acp: AcpConfig,
    /// Claude Code backend settings.
    pub claude: ClaudeCodeConfig,
    /// Default system prompt override.
    pub system_prompt_override: Option<String>,
    /// Extra rules appended to the system prompt.
    pub rules: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            backend: None,
            provider: None,
            name: None,
            reasoning_effort: None,
            goal_verifier_count: 3,
            definition: None,
            opencode: OpencodeConfig::default(),
            acp: AcpConfig::default(),
            claude: ClaudeCodeConfig::default(),
            system_prompt_override: None,
            rules: Vec::new(),
        }
    }
}

impl AgentConfig {
    /// The effective backend for this config (defaults to native).
    pub fn effective_backend(&self) -> BackendKind {
        if let Some(provider) = &self.provider {
            if let Some(kind) = BackendKind::from_provider(provider) {
                return kind;
            }
        }
        self.backend.unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OpencodeTransport {
    /// Use a server when one is configured/reachable, otherwise the CLI.
    #[default]
    Auto,
    /// HTTP + SSE against an opencode server.
    Server,
    /// Spawn `opencode run --format json` per prompt.
    Cli,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpencodeConfig {
    /// URL of a running opencode server. When unset the agent starts one.
    pub url: Option<String>,
    /// Host/port used when cachycli starts the server itself.
    pub hostname: String,
    pub port: u16,
    /// Automatically start `opencode serve` when no server is reachable.
    pub autostart: bool,
    /// Command used to start the server.
    pub command: String,
    /// Extra args for the server command.
    pub args: Vec<String>,
    /// Model override for opencode sessions, as `provider/model`.
    pub model: Option<String>,
    /// Agent (mode) name passed to opencode.
    pub agent: Option<String>,
    /// Transport selection.
    pub transport: OpencodeTransport,
}

impl Default for OpencodeConfig {
    fn default() -> Self {
        Self {
            hostname: "127.0.0.1".into(),
            port: 0,
            autostart: true,
            command: "opencode".into(),
            url: None,
            args: vec!["serve".into()],
            model: None,
            agent: None,
            transport: OpencodeTransport::Auto,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClaudeCodeConfig {
    /// Model override passed to `claude --model`.
    pub model: Option<String>,
    /// Skip Claude Code's own permission prompts.
    pub always_approve: bool,
    /// Extra arguments appended to every `claude` invocation.
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AcpConfig {
    /// Command to spawn an ACP agent (e.g. `opencode acp`).
    pub command: String,
    /// WebSocket URL of a remote ACP agent (`ws://host:port`). When set, no
    /// process is spawned — the agent is reached over the socket instead
    /// (`cachycli agent serve` speaks ACP there).
    pub url: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Working directory override for the spawned agent.
    pub cwd: Option<PathBuf>,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            command: "opencode".into(),
            url: None,
            args: vec!["acp".into()],
            env: BTreeMap::new(),
            cwd: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeaturesConfig {
    /// Accepted so an existing configuration keeps parsing; this build has no
    /// cloud, so these have nothing to switch. `web_fetch`/`web_search` below
    /// are the ones that do something.
    pub support_permission: bool,
    /// No telemetry exists to turn off (see the README: nothing leaves the
    /// machine).
    pub telemetry: bool,
    pub feedback: bool,
    pub codebase_indexing: bool,
    pub lsp_tools: bool,
    /// Enable the web_fetch tool.
    pub web_fetch: bool,
    /// Enable the web_search tool.
    pub web_search: bool,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            support_permission: true,
            telemetry: false,
            feedback: false,
            codebase_indexing: false,
            lsp_tools: false,
            web_fetch: true,
            web_search: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub max_thoughts_width: u16,
    /// Theme name: `cachy-night`, `cachy-day`, `tokyonight`, `rosepine-moon`, `oscura-midnight`.
    pub theme: String,
    /// `fullscreen` or `minimal`.
    pub screen_mode: String,
    pub timestamps: bool,
    pub show_thinking: bool,
    pub tool_density: String,
    /// Whether the plan/todo panel is open.
    pub todo_panel: bool,
    /// How to tell you a long turn finished: `off`, `bell` (BEL) or `osc`
    /// (a desktop notification on terminals that support OSC 9).
    pub notify: String,
    /// Turns shorter than this many seconds never notify.
    pub notify_after: u64,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            max_thoughts_width: 120,
            theme: "cachy-night".into(),
            screen_mode: "fullscreen".into(),
            timestamps: true,
            show_thinking: true,
            notify: "off".into(),
            notify_after: 20,
            tool_density: "compact".into(),
            todo_panel: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionConfig {
    pub auto_compact_threshold_percent: u8,
    pub load_envrc: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auto_compact_threshold_percent: 85,
            load_envrc: true,
        }
    }
}

/// `[share]`: where `/share push` and `/share pull` send sessions by default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShareConfig {
    /// WebSocket url of a running `cachycli agent serve`.
    pub server: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolsConfig {
    pub respect_gitignore: bool,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolsetConfig {
    pub bash: BashToolConfig,
    pub web_fetch: WebFetchConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BashToolConfig {
    pub timeout_secs: f64,
    pub output_byte_limit: usize,
}

impl Default for BashToolConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 120.0,
            output_byte_limit: 65536,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebFetchConfig {
    pub proxy_endpoint: Option<String>,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpServerConfig {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub enabled: bool,
    pub startup_timeout_sec: u64,
    pub tool_timeout_sec: u64,
    pub tool_timeouts: BTreeMap<String, u64>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
            enabled: true,
            startup_timeout_sec: 30,
            tool_timeout_sec: 60,
            tool_timeouts: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkillsConfig {
    pub paths: Vec<String>,
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginsConfig {
    /// JSON index of installable plugins: an `http(s)` URL or a local path.
    /// Each entry is `{"name": …, "description": …, "git": …}` or `{"path": …}`.
    pub index: Option<String>,
    pub paths: Vec<String>,
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SubagentsConfig {
    pub enabled: bool,
    pub toggle: BTreeMap<String, bool>,
    pub models: BTreeMap<String, String>,
    /// How many subagents may run at the same time.
    pub max_concurrent: usize,
    /// How deep subagents may nest (1 = they cannot spawn their own).
    pub max_depth: usize,
}

impl Default for SubagentsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            toggle: BTreeMap::new(),
            models: BTreeMap::new(),
            max_concurrent: 4,
            max_depth: 2,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub index: MemoryIndexConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryIndexConfig {
    pub max_chunk_chars: usize,
    pub chunk_overlap_chars: usize,
}

impl Default for MemoryIndexConfig {
    fn default() -> Self {
        Self {
            max_chunk_chars: 1600,
            chunk_overlap_chars: 320,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    pub auth_provider_command: Option<String>,
    pub auth_provider_label: Option<String>,
    pub auth_token_ttl: Option<u64>,
    /// OIDC settings for SSO.
    pub oidc_issuer: Option<String>,
    pub oidc_client_id: Option<String>,
}

/// A single `[model.<name>]` table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelConfig {
    /// Identifier sent to the API. Defaults to the table name.
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub api_key: Option<String>,
    #[serde(deserialize_with = "string_or_vec")]
    pub env_key: Option<Vec<String>>,
    pub auth_provider: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_completion_tokens: Option<u32>,
    pub context_window: Option<u64>,
    pub context_windows: Vec<u64>,
    /// `chat` (default) or `responses`/`messages`/`anthropic`.
    pub api_backend: Option<String>,
    /// Extra headers sent with requests.
    pub headers: BTreeMap<String, String>,
}

fn string_or_vec<'de, D>(de: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    let v = Option::<OneOrMany>::deserialize(de)?;
    Ok(v.map(|v| match v {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(list) => list,
    }))
}

/// Result of loading configuration, keeping provenance for `inspect`.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub sources: Vec<PathBuf>,
    pub project_config: Option<PathBuf>,
}

/// Load configuration for a working directory.
pub fn load(cwd: &Path) -> Result<LoadedConfig> {
    let mut sources = Vec::new();
    let mut config = Config::default();

    let managed = paths::managed_config_file();
    if managed.is_file() {
        merge_file(&mut config, &managed)?;
        sources.push(managed);
    }

    let user = paths::config_file();
    if user.is_file() {
        merge_file(&mut config, &user)?;
        sources.push(user);
    }

    let mut project_config = None;
    let repo_root = cwd
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(|dir| dir.to_path_buf());
    let home = dirs::home_dir();

    for dir in project_config_dirs(cwd, repo_root.as_deref(), home.as_deref()) {
        let path = dir.join(".cachy").join("config.toml");
        // The cachy home is the user config, wherever it lives.
        if path.is_file() && path != paths::config_file() {
            merge_file(&mut config, &path)?;
            sources.push(path.clone());
            project_config = Some(path);
        }
    }

    apply_env_overrides(&mut config);

    Ok(LoadedConfig {
        config,
        sources,
        project_config,
    })
}

/// Where project configs are looked for: from the repo root down to `cwd`,
/// never above your home directory (nothing there is a project, and `~/.cachy`
/// is the user config).
fn project_config_dirs(cwd: &Path, repo_root: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(root) = repo_root {
        for dir in root.ancestors() {
            if home.is_some_and(|home| home.starts_with(dir)) {
                continue;
            }
            dirs.push(dir.to_path_buf());
        }
        dirs.reverse();
        dirs.push(root.to_path_buf());
    }
    dirs.push(cwd.to_path_buf());
    dirs.dedup();
    dirs
}

fn merge_file(base: &mut Config, path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let layer: Config =
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
    merge(base, layer);
    Ok(())
}

/// Merge a higher-priority layer into the base configuration.
pub fn merge(base: &mut Config, layer: Config) {
    // simple field-by-field merge; None in a layer means "not specified"
    if layer.cli.auto_update {
        base.cli.auto_update = true;
    }
    if let Some(fmt) = layer.cli.output_format.clone() {
        base.cli.output_format = Some(fmt);
    }
    if let Some(v) = layer.models.default.clone() {
        base.models.default = Some(v);
    }
    if let Some(v) = layer.models.web_search.clone() {
        base.models.web_search = Some(v);
    }
    if let Some(v) = layer.models.fallback.clone() {
        base.models.fallback = Some(v);
    }
    if let Some(v) = layer.endpoints.models_base_url.clone() {
        base.endpoints.models_base_url = Some(v);
    }
    if let Some(v) = layer.endpoints.models_list_url.clone() {
        base.endpoints.models_list_url = Some(v);
    }
    if layer.agent.backend.is_some() {
        base.agent.backend = layer.agent.backend;
    }
    if layer.agent.provider.is_some() {
        base.agent.provider = layer.agent.provider.clone();
    }
    if layer.agent.name.is_some() {
        base.agent.name = layer.agent.name.clone();
    }
    if layer.agent.definition.is_some() {
        base.agent.definition = layer.agent.definition.clone();
    }
    if layer.agent.reasoning_effort.is_some() {
        base.agent.reasoning_effort = layer.agent.reasoning_effort.clone();
    }
    if layer.agent.goal_verifier_count != AgentConfig::default().goal_verifier_count {
        base.agent.goal_verifier_count = layer.agent.goal_verifier_count;
    }
    if layer.agent.opencode.url.is_some() {
        base.agent.opencode.url = layer.agent.opencode.url.clone();
    }
    if layer.agent.opencode.port != 0 {
        base.agent.opencode.port = layer.agent.opencode.port;
    }
    if layer.agent.opencode.command != OpencodeConfig::default().command {
        base.agent.opencode.command = layer.agent.opencode.command.clone();
    }
    if !layer.agent.opencode.args.is_empty() {
        base.agent.opencode.args = layer.agent.opencode.args.clone();
    }
    if layer.agent.opencode.model.is_some() {
        base.agent.opencode.model = layer.agent.opencode.model.clone();
    }
    if layer.agent.opencode.agent.is_some() {
        base.agent.opencode.agent = layer.agent.opencode.agent.clone();
    }
    if layer.agent.acp.command != AcpConfig::default().command {
        base.agent.acp.command = layer.agent.acp.command.clone();
    }
    if !layer.agent.acp.args.is_empty() {
        base.agent.acp.args = layer.agent.acp.args.clone();
    }
    if !layer.agent.acp.env.is_empty() {
        base.agent.acp.env.extend(layer.agent.acp.env.clone());
    }
    if layer.agent.acp.cwd.is_some() {
        base.agent.acp.cwd = layer.agent.acp.cwd.clone();
    }
    if layer.agent.acp.url.is_some() {
        base.agent.acp.url = layer.agent.acp.url.clone();
    }
    if layer.agent.claude.model.is_some() {
        base.agent.claude.model = layer.agent.claude.model.clone();
    }
    if layer.agent.claude.always_approve {
        base.agent.claude.always_approve = true;
    }
    if !layer.agent.claude.args.is_empty() {
        base.agent.claude.args = layer.agent.claude.args.clone();
    }
    if layer.agent.system_prompt_override.is_some() {
        base.agent.system_prompt_override = layer.agent.system_prompt_override.clone();
    }
    base.agent.rules.extend(layer.agent.rules.clone());

    merge_features(&mut base.features, &layer.features);
    merge_ui(&mut base.ui, &layer.ui);
    merge_session(&mut base.session, &layer.session);
    merge_share(&mut base.share, &layer.share);
    merge_tools(&mut base.tools, &layer.tools);
    merge_skills(&mut base.skills, &layer.skills);
    merge_plugins(&mut base.plugins, &layer.plugins);
    merge_subagents(&mut base.subagents, &layer.subagents);
    merge_memory(&mut base.memory, &layer.memory);
    merge_auth(&mut base.auth, &layer.auth);

    if !layer.mcp_servers.is_empty() {
        for (k, v) in layer.mcp_servers {
            base.mcp_servers.insert(k, v);
        }
    }
    for (k, v) in layer.custom_models {
        base.custom_models.insert(k, v);
    }
    if layer.toolset.bash.timeout_secs > 0.0 {
        base.toolset.bash.timeout_secs = layer.toolset.bash.timeout_secs;
    }
    if layer.toolset.bash.output_byte_limit > 0 {
        base.toolset.bash.output_byte_limit = layer.toolset.bash.output_byte_limit;
    }
    if layer.toolset.web_fetch.proxy_endpoint.is_some() {
        base.toolset.web_fetch.proxy_endpoint = layer.toolset.web_fetch.proxy_endpoint.clone();
    }
    if !layer.toolset.web_fetch.allowed_domains.is_empty() {
        base.toolset.web_fetch.allowed_domains = layer.toolset.web_fetch.allowed_domains.clone();
    }
}

fn merge_features(base: &mut FeaturesConfig, l: &FeaturesConfig) {
    let d = FeaturesConfig::default();
    if l.telemetry != d.telemetry {
        base.telemetry = l.telemetry;
    }
    if l.feedback != d.feedback {
        base.feedback = l.feedback;
    }
    if l.codebase_indexing != d.codebase_indexing {
        base.codebase_indexing = l.codebase_indexing;
    }
    if l.lsp_tools != d.lsp_tools {
        base.lsp_tools = l.lsp_tools;
    }
    if l.web_fetch != d.web_fetch {
        base.web_fetch = l.web_fetch;
    }
    if l.web_search != d.web_search {
        base.web_search = l.web_search;
    }
    if l.support_permission != d.support_permission {
        base.support_permission = l.support_permission;
    }
}

fn merge_ui(base: &mut UiConfig, l: &UiConfig) {
    let d = UiConfig::default();
    if l.max_thoughts_width != d.max_thoughts_width {
        base.max_thoughts_width = l.max_thoughts_width;
    }
    if l.theme != d.theme {
        base.theme = l.theme.clone();
    }
    if l.screen_mode != d.screen_mode {
        base.screen_mode = l.screen_mode.clone();
    }
    if l.timestamps != d.timestamps {
        base.timestamps = l.timestamps;
    }
    if l.show_thinking != d.show_thinking {
        base.show_thinking = l.show_thinking;
    }
    if l.tool_density != d.tool_density {
        base.tool_density = l.tool_density.clone();
    }
    if l.notify != d.notify {
        base.notify = l.notify.clone();
    }
    if l.notify_after != d.notify_after {
        base.notify_after = l.notify_after;
    }
}

fn merge_session(base: &mut SessionConfig, l: &SessionConfig) {
    let d = SessionConfig::default();
    if l.auto_compact_threshold_percent != d.auto_compact_threshold_percent {
        base.auto_compact_threshold_percent = l.auto_compact_threshold_percent;
    }
    if l.load_envrc != d.load_envrc {
        base.load_envrc = l.load_envrc;
    }
}

fn merge_share(base: &mut ShareConfig, l: &ShareConfig) {
    if l.server.is_some() {
        base.server = l.server.clone();
    }
}

fn merge_tools(base: &mut ToolsConfig, l: &ToolsConfig) {
    let d = ToolsConfig::default();
    if l.respect_gitignore != d.respect_gitignore {
        base.respect_gitignore = l.respect_gitignore;
    }
}

fn merge_skills(base: &mut SkillsConfig, l: &SkillsConfig) {
    if !l.paths.is_empty() {
        base.paths.extend(l.paths.clone());
    }
    if !l.ignore.is_empty() {
        base.ignore.extend(l.ignore.clone());
    }
}

fn merge_plugins(base: &mut PluginsConfig, l: &PluginsConfig) {
    if l.index.is_some() {
        base.index = l.index.clone();
    }
    if !l.paths.is_empty() {
        base.paths.extend(l.paths.clone());
    }
    if !l.disabled.is_empty() {
        base.disabled.extend(l.disabled.clone());
    }
}

fn merge_subagents(base: &mut SubagentsConfig, l: &SubagentsConfig) {
    if l.max_concurrent != SubagentsConfig::default().max_concurrent {
        base.max_concurrent = l.max_concurrent;
    }
    if l.max_depth != SubagentsConfig::default().max_depth {
        base.max_depth = l.max_depth;
    }
    if l.max_concurrent != SubagentsConfig::default().max_concurrent {
        base.max_concurrent = l.max_concurrent;
    }
    if l.max_depth != SubagentsConfig::default().max_depth {
        base.max_depth = l.max_depth;
    }
    if l.max_concurrent != SubagentsConfig::default().max_concurrent {
        base.max_concurrent = l.max_concurrent;
    }
    if l.max_depth != SubagentsConfig::default().max_depth {
        base.max_depth = l.max_depth;
    }
    if l.max_concurrent != SubagentsConfig::default().max_concurrent {
        base.max_concurrent = l.max_concurrent;
    }
    if l.max_depth != SubagentsConfig::default().max_depth {
        base.max_depth = l.max_depth;
    }
    if !l.enabled {
        base.enabled = false;
    }
    if !l.toggle.is_empty() {
        base.toggle.extend(l.toggle.clone());
    }
    if !l.models.is_empty() {
        base.models.extend(l.models.clone());
    }
}

fn merge_memory(base: &mut MemoryConfig, l: &MemoryConfig) {
    if l.enabled {
        base.enabled = true;
    }
    if l.index.max_chunk_chars != MemoryIndexConfig::default().max_chunk_chars {
        base.index.max_chunk_chars = l.index.max_chunk_chars;
    }
    if l.index.chunk_overlap_chars != MemoryIndexConfig::default().chunk_overlap_chars {
        base.index.chunk_overlap_chars = l.index.chunk_overlap_chars;
    }
}

fn merge_auth(base: &mut AuthConfig, l: &AuthConfig) {
    if l.auth_provider_command.is_some() {
        base.auth_provider_command = l.auth_provider_command.clone();
    }
    if l.auth_provider_label.is_some() {
        base.auth_provider_label = l.auth_provider_label.clone();
    }
    if l.auth_token_ttl.is_some() {
        base.auth_token_ttl = l.auth_token_ttl;
    }
    if l.oidc_issuer.is_some() {
        base.oidc_issuer = l.oidc_issuer.clone();
    }
    if l.oidc_client_id.is_some() {
        base.oidc_client_id = l.oidc_client_id.clone();
    }
}

fn apply_env_overrides(config: &mut Config) {
    if let Ok(v) = std::env::var("CACHY_MODELS_BASE_URL") {
        config.endpoints.models_base_url = Some(v);
    }
    if let Ok(v) = std::env::var("CACHY_MODELS_LIST_URL") {
        config.endpoints.models_list_url = Some(v);
    }
    if let Ok(v) = std::env::var("CACHY_MODEL") {
        config.models.default = Some(v);
    }
    if let Ok(v) = std::env::var("CACHY_PROVIDER").or_else(|_| std::env::var("CACHY_AGENT_BACKEND"))
    {
        config.agent.provider = Some(v);
    }
    if let Ok(v) = std::env::var("OPENCODE_SERVER_URL") {
        config.agent.opencode.url = Some(v);
    }
    if let Ok(v) = std::env::var("CACHY_WEB_FETCH") {
        config.features.web_fetch = v != "0";
    }
    if let Ok(v) = std::env::var("CACHY_RESPECT_GITIGNORE") {
        config.tools.respect_gitignore = v != "0";
    }
    if let Ok(v) = std::env::var("CACHY_SUBAGENTS") {
        config.subagents.enabled = v != "0";
    }
    if let Ok(v) = std::env::var("CACHY_MEMORY") {
        config.memory.enabled = v != "0";
    }
    if let Ok(v) = std::env::var("CACHY_THEME") {
        config.ui.theme = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A setting added to a new section has to survive the merge; forgetting one
    /// is how `[agent.acp] url` used to disappear.
    #[test]
    fn merge_keeps_section_settings() {
        let mut base = Config::default();
        let layer: Config = toml::from_str(
            r#"
[share]
server = "ws://build-box:8765"

[agent.acp]
url = "ws://build-box:8765"
"#,
        )
        .unwrap();
        merge(&mut base, layer);
        assert_eq!(base.share.server.as_deref(), Some("ws://build-box:8765"));
        assert_eq!(base.agent.acp.url.as_deref(), Some("ws://build-box:8765"));

        let mut base = Config::default();
        let layer: Config = toml::from_str("[ui]\nnotify = \"bell\"\nnotify_after = 45\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(base.ui.notify, "bell");
        assert_eq!(base.ui.notify_after, 45);

        let mut base = Config::default();
        let layer: Config = toml::from_str("[ui]\nnotify = \"bell\"\nnotify_after = 45\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(base.ui.notify, "bell");
        assert_eq!(base.ui.notify_after, 45);
    }

    #[test]
    fn project_configs_stop_at_home() {
        let cwd = Path::new("/home/me/proj/sub");
        let dirs = project_config_dirs(
            cwd,
            Some(Path::new("/home/me/proj")),
            Some(Path::new("/home/me")),
        );
        assert!(
            dirs.contains(&PathBuf::from("/home/me/proj/sub")),
            "cwd applies"
        );
        assert!(
            dirs.contains(&PathBuf::from("/home/me/proj")),
            "the repo root applies"
        );
        assert!(
            !dirs.iter().any(|dir| dir == Path::new("/home/me")),
            "home is the user config, not a project: {dirs:?}"
        );
        assert!(
            !dirs.iter().any(|dir| dir == Path::new("/")),
            "nothing above home: {dirs:?}"
        );
    }

    #[test]
    fn parses_custom_model() {
        let toml = r#"
[models]
default = "my-model"

[model.my-model]
model = "llama-3.1-70b"
base_url = "http://localhost:8080/v1"
name = "Local Llama"
env_key = ["LOCAL_KEY", "OTHER"]
context_window = 128000
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let m = cfg.custom_models.get("my-model").unwrap();
        assert_eq!(m.name.as_deref(), Some("Local Llama"));
        assert_eq!(m.env_key.as_ref().unwrap().len(), 2);
        assert_eq!(cfg.models.default.as_deref(), Some("my-model"));
    }
}
