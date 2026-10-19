//! Command-line interface definition (mirrors the reference CLI surface).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text.
    Plain,
    /// One JSON object after completion.
    Json,
    /// NDJSON session updates (agent native format).
    StreamingJson,
    /// NDJSON in the Anthropic Messages API wire format.
    StreamingMessagesJson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PermissionModeArg {
    #[value(name = "default")]
    Default,
    #[value(name = "acceptEdits")]
    AcceptEdits,
    #[value(name = "auto")]
    Auto,
    #[value(name = "dontAsk")]
    DontAsk,
    #[value(name = "bypassPermissions")]
    BypassPermissions,
    #[value(name = "plan")]
    Plan,
}

impl From<PermissionModeArg> for cachy_core::permissions::PermissionMode {
    fn from(value: PermissionModeArg) -> Self {
        match value {
            PermissionModeArg::Default => Self::Default,
            PermissionModeArg::AcceptEdits => Self::AcceptEdits,
            PermissionModeArg::Auto => Self::Auto,
            PermissionModeArg::DontAsk => Self::DontAsk,
            PermissionModeArg::BypassPermissions => Self::BypassPermissions,
            PermissionModeArg::Plan => Self::Plan,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "cachycli",
    version,
    about = "Cachy — a terminal AI coding agent that connects with opencode and other agents",
    long_about = None,
    propagate_version = true
)]
pub struct Cli {
    /// Initial prompt for the interactive session.
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Agent name or definition file path.
    #[arg(long, visible_alias = "agent-profile", value_name = "NAME")]
    pub agent: Option<String>,

    /// Inline subagent definitions as JSON.
    #[arg(long, value_name = "JSON")]
    pub agents: Option<String>,

    /// Permission allow rule (repeatable).
    #[arg(long = "allow", visible_aliases = ["allowedTools", "allowed-tools"], value_name = "RULE")]
    pub allow: Vec<String>,

    /// Permission deny rule (repeatable).
    #[arg(long = "deny", visible_alias = "disallowedTools", value_name = "RULE")]
    pub deny: Vec<String>,

    /// Auto-approve all tool executions.
    #[arg(long)]
    pub always_approve: bool,

    /// Continue the most recent session for the current working directory.
    #[arg(short = 'c', long)]
    pub r#continue: bool,

    /// Working directory.
    #[arg(long, value_name = "CWD")]
    pub cwd: Option<PathBuf>,

    /// Enable debug logging.
    #[arg(long)]
    pub debug: bool,

    /// Write debug logs to FILE.
    #[arg(long, value_name = "FILE")]
    pub debug_file: Option<PathBuf>,

    /// Disable web search and web fetch tools.
    #[arg(long)]
    pub disable_web_search: bool,

    /// Built-in tools to remove (comma-separated).
    #[arg(long, value_name = "TOOLS")]
    pub disallowed_tools: Option<String>,

    /// Create a new session ID when resuming.
    #[arg(long)]
    pub fork_session: bool,

    /// Open in the standard fullscreen TUI.
    #[arg(long)]
    pub fullscreen: bool,

    /// Emit incremental stream_event lines alongside whole messages.
    #[arg(long)]
    pub include_partial_messages: bool,

    /// JSON Schema for structured output.
    #[arg(long, value_name = "SCHEMA")]
    pub json_schema: Option<String>,

    /// Model ID to use.
    #[arg(short = 'm', long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Maximum number of agent turns.
    #[arg(long, value_name = "N")]
    pub max_turns: Option<u32>,

    /// Experimental scrollback-native rendering.
    #[arg(long)]
    pub minimal: bool,

    /// Run inline instead of using the alternate screen.
    #[arg(long)]
    pub no_alt_screen: bool,

    /// Disable plan mode.
    #[arg(long)]
    pub no_plan: bool,

    /// Disable subagent spawning.
    #[arg(long)]
    pub no_subagents: bool,

    /// Force subagent support on (the default).
    #[arg(long)]
    pub subagents: bool,

    /// Use the light theme for this session.
    #[arg(long)]
    pub light: bool,

    /// Exit after the first response (with a prompt).
    #[arg(long)]
    pub single_turn: bool,

    /// Extra plugin directory for this session.
    #[arg(long, value_name = "PATH")]
    pub plugin_dir: Option<PathBuf>,

    /// Model used by the web_search tool.
    #[arg(long, value_name = "MODEL")]
    pub web_search_model: Option<String>,

    /// Disable cross-session memory for this run.
    #[arg(long)]
    pub no_memory: bool,

    /// Start in the current directory even if it looks like a git submodule.
    #[arg(long, hide = true)]
    pub no_worktree: bool,

    /// Output format for headless mode.
    #[arg(long, value_enum, default_value = "plain")]
    pub output_format: OutputFormat,

    /// Single-turn prompt: print the response to stdout and exit.
    #[arg(
        short = 'p',
        long = "single",
        visible_alias = "prompt",
        value_name = "PROMPT"
    )]
    pub single: Option<String>,

    /// Permission mode.
    #[arg(long, value_enum)]
    pub permission_mode: Option<PermissionModeArg>,

    /// Single-turn prompt from a file.
    #[arg(long, value_name = "PATH")]
    pub prompt_file: Option<PathBuf>,

    /// Single-turn prompt as JSON content blocks.
    #[arg(long, value_name = "JSON")]
    pub prompt_json: Option<String>,

    /// Resume a session by ID or title.
    #[arg(short = 'r', long, value_name = "SESSION")]
    pub resume: Option<Option<String>>,

    /// Reasoning effort for reasoning models.
    #[arg(long, alias = "effort", value_name = "EFFORT")]
    pub reasoning_effort: Option<String>,

    /// Restore the original session's repository snapshot when resuming.
    #[arg(long)]
    pub restore_code: bool,

    /// Extra rules to append to the system prompt.
    #[arg(long, value_name = "RULES")]
    pub rules: Option<String>,

    /// Use a specific session UUID for a new conversation.
    #[arg(short = 's', long, value_name = "SESSION_ID")]
    pub session_id: Option<String>,

    /// Sandbox profile for filesystem and network access.
    #[arg(long, env = "CACHY_SANDBOX", value_name = "PROFILE")]
    pub sandbox: Option<String>,

    /// Override the agent's system prompt.
    #[arg(long, alias = "system-prompt", value_name = "PROMPT")]
    pub system_prompt_override: Option<String>,

    /// Built-in tools to allow (comma-separated).
    #[arg(long, value_name = "TOOLS")]
    pub tools: Option<String>,

    /// Send the prompt exactly as given.
    #[arg(long)]
    pub verbatim: bool,

    /// Start the session in a new git worktree.
    #[arg(short = 'w', long, value_name = "WORKTREE", num_args = 0..=1)]
    pub worktree: Option<Option<String>>,

    /// Branch, tag, or commit to base the worktree on.
    #[arg(long, alias = "ref", value_name = "REF")]
    pub worktree_ref: Option<String>,

    /// Provider to use: opencode, claude-code, native or acp.
    #[arg(long, visible_alias = "backend", value_name = "NAME")]
    pub provider: Option<String>,

    /// URL of a running opencode server (opencode backend).
    #[arg(long, value_name = "URL")]
    pub opencode_url: Option<String>,

    /// Command used to spawn an ACP agent (acp backend), e.g. "opencode acp".
    #[arg(long, value_name = "CMD")]
    pub acp_command: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run Cachy without the interactive UI (ACP agent over stdio, etc).
    Agent(AgentArgs),
    /// Lazy-clone a git repository (not supported in this build).
    Clone(CloneArgs),
    /// Generate shell completion scripts.
    Completions(CompletionsArgs),
    /// Register this machine as a Cursor private worker (not supported).
    CursorWorker,
    /// Open the Agent Dashboard view at startup.
    Dashboard,
    /// Check terminal, clipboard, color, and input support.
    Doctor(DoctorArgs),
    /// Show what the cachy home directory uses on disk.
    #[command(alias = "disk-usage")]
    Du,
    /// Export a session transcript as Markdown.
    Export(ExportArgs),
    /// Show the configuration Cachy discovers for this directory.
    Inspect(InspectArgs),
    /// Manage running leader processes.
    Leader(LeaderArgs),
    /// Sign in (store an API key).
    Login(LoginArgs),
    /// Sign out and clear cached credentials.
    Logout,
    /// Manage MCP server configurations.
    Mcp(McpArgs),
    /// Manage cross-session memory.
    Memory(MemoryArgs),
    /// List available models and exit.
    Models(ModelsArgs),
    /// Manage plugins and marketplace sources.
    Plugin(PluginArgs),
    /// List, search, or restore sessions.
    Sessions(SessionsArgs),
    /// Hand a session to a running `agent serve`, or fetch one back.
    Share(ShareArgs),
    /// Fetch and install managed configuration.
    Setup(SetupArgs),
    /// Export or upload session trace data.
    Trace(TraceArgs),
    /// Check for updates or install a specific version.
    Update(UpdateArgs),
    /// Remove what the installer put there (the binary, its symlink, PATH lines).
    Uninstall(UninstallArgs),
    /// Print persisted token and cost usage for a session.
    Usage(UsageArgs),
    /// Print version information.
    #[command(alias = "v")]
    Version,
    /// Manage git worktrees.
    Worktree(WorktreeArgs),
    /// Run any command with local clipboard support (forwards OSC 52).
    Wrap(WrapArgs),
    /// Alias for `agent stdio` compatibility with some clients.
    Acp,
}

#[derive(Debug, Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub mode: AgentMode,
}

#[derive(Debug, Subcommand)]
pub enum AgentMode {
    /// ACP agent over stdin/stdout (JSON-RPC).
    Stdio(AgentStdioArgs),
    /// Run as a WebSocket server other clients can drive.
    Serve(AgentServeArgs),
    /// Headless agent (optionally connect to a websocket relay).
    Headless(AgentHeadlessArgs),
}

#[derive(Debug, Args)]
pub struct AgentServeArgs {
    /// Address to bind.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Port to listen on.
    #[arg(long, default_value_t = 8765)]
    pub port: u16,
}

#[derive(Debug, Args)]
pub struct AgentStdioArgs {
    /// Model override.
    #[arg(short = 'm', long)]
    pub model: Option<String>,
    /// Start in always-approve mode.
    #[arg(long)]
    pub always_approve: bool,
    /// Force re-authentication (unsupported without an auth provider).
    #[arg(long)]
    pub reauth: bool,
}

#[derive(Debug, Args)]
pub struct AgentHeadlessArgs {
    /// Bridge stdin/stdout to a WebSocket agent (`cachycli agent serve`), so a
    /// client that only speaks stdio can drive a remote one.
    #[arg(long = "grok-ws-url", alias = "ws-url", value_name = "URL")]
    pub ws_url: Option<String>,
}

#[derive(Debug, Args)]
pub struct CloneArgs {
    pub url: Option<String>,
}

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    /// Shell: bash, zsh, fish, powershell, elvish.
    pub shell: String,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Session ID or title (default: most recent for this directory).
    pub session: Option<String>,
    /// Output file (default: <title>.md in the current directory).
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LeaderArgs {
    #[command(subcommand)]
    pub action: Option<LeaderAction>,
}

#[derive(Debug, Subcommand)]
pub enum LeaderAction {
    /// List running leaders.
    List,
    /// Stop a leader.
    Kill { id: Option<String> },
    /// Show leader status.
    Status,
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// API key to store (omit to be prompted).
    #[arg(long, value_name = "KEY")]
    pub api_key: Option<String>,
    /// Provider name this key belongs to (e.g. "openai", "opencode", "xai").
    #[arg(long, default_value = "default")]
    pub provider: String,
}

#[derive(Debug, Args)]
pub struct McpArgs {
    #[command(subcommand)]
    pub action: Option<McpAction>,
}

#[derive(Debug, Subcommand)]
pub enum McpAction {
    /// List configured MCP servers.
    List(McpListArgs),
    /// Add or update an MCP server in the user config.
    Add(McpAddArgs),
    /// Remove an MCP server from the user config.
    Remove(McpRemoveArgs),
    /// Connect and list the tools a server exposes.
    Test(McpTestArgs),
    /// Run cachycli as an MCP server over stdio (so other agents can call it).
    Serve(McpServeArgs),
}

#[derive(Debug, Args)]
pub struct McpListArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct McpAddArgs {
    pub name: String,
    /// Command to run (stdio transport).
    #[arg(long)]
    pub command: Option<String>,
    /// Extra arguments (after `--`).
    #[arg(long = "arg", value_name = "ARG")]
    pub args: Vec<String>,
    /// Remote URL (streamable HTTP transport).
    #[arg(long)]
    pub url: Option<String>,
    /// Header in `Name=Value` form (repeatable).
    #[arg(long = "header", value_name = "K=V")]
    pub headers: Vec<String>,
    /// Write to the project config instead of the user config.
    #[arg(long)]
    pub project: bool,
}

#[derive(Debug, Args)]
pub struct McpRemoveArgs {
    pub name: String,
    #[arg(long)]
    pub project: bool,
}

#[derive(Debug, Args)]
pub struct McpTestArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct McpServeArgs {
    /// Expose the sessions of this directory only.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub action: MemoryAction,
}

#[derive(Debug, Subcommand)]
pub enum MemoryAction {
    /// Show memory topics.
    List(MemoryListArgs),
    /// Delete memory for the current directory.
    Clear(MemoryClearArgs),
    /// Add a note to memory.
    Add { text: String },
}

#[derive(Debug, Args)]
pub struct MemoryListArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct MemoryClearArgs {
    /// Clear global memory instead of the workspace scope.
    #[arg(long)]
    pub global: bool,
    /// Clear both scopes.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct ModelsArgs {
    /// Emit JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub action: Option<PluginAction>,
}

#[derive(Debug, Subcommand)]
pub enum PluginAction {
    /// List discovered plugins.
    List,
    /// Trust a plugin directory (no-op marker in this build).
    Trust { path: String },
    /// Reload plugins (re-scan).
    Reload,
}

#[derive(Debug, Args)]
pub struct SessionsArgs {
    #[command(subcommand)]
    pub action: Option<SessionsAction>,
    /// Emit JSON (list).
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum SessionsAction {
    /// List sessions.
    List,
    /// Search sessions by title.
    Search { query: String },
    /// Print the command to restore a session.
    Restore { session: String },
    /// Delete a session.
    Delete { session: String },
}

#[derive(Debug, Args)]
pub struct ShareArgs {
    /// WebSocket url of the server (`cachycli agent serve --port 8765`).
    #[arg(long, value_name = "URL")]
    pub to: String,
    /// Session id (or title) to send; default: the latest one in this directory.
    #[arg(long, value_name = "ID")]
    pub session: Option<String>,
    /// Share id to fetch; default: the most recent one on the server.
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,
    /// List what the server holds instead of transferring.
    #[arg(long)]
    pub list: bool,
    /// Ask the server to forget a share id.
    #[arg(long, value_name = "ID")]
    pub remove: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    /// URL to fetch managed configuration from.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
}

#[derive(Debug, Args)]
pub struct TraceArgs {
    /// Session ID (default: most recent).
    pub session: Option<String>,
    /// Output file for the trace JSONL.
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    /// Upload the trace to the configured endpoint.
    #[arg(long)]
    pub upload: bool,
}

#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Also remove ~/.cachy: configuration, sessions, memory and plugins.
    #[arg(long)]
    pub purge: bool,
    /// Do not ask before --purge.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Args)]
#[command(disable_version_flag = true)]
pub struct UpdateArgs {
    /// Check for updates without installing.
    #[arg(long)]
    pub check: bool,
    /// Install a specific version.
    #[arg(long)]
    pub version: Option<String>,
    /// Emit JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct UsageArgs {
    /// Session ID (default: most recent for this directory).
    pub session: Option<String>,
}

#[derive(Debug, Args)]
pub struct WorktreeArgs {
    #[command(subcommand)]
    pub action: Option<WorktreeAction>,
}

#[derive(Debug, Subcommand)]
pub enum WorktreeAction {
    /// List git worktrees.
    List,
    /// Create a worktree under ~/.cachy/worktrees.
    Create {
        name: String,
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// Remove a worktree.
    Remove { name: String },
}

#[derive(Debug, Args)]
pub struct WrapArgs {
    /// Command to run (everything after the flags).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}
