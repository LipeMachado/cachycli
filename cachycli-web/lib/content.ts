/** Shared content for the marketing page and the guide. */

export const VERSION = process.env.NEXT_PUBLIC_CACHY_VERSION ?? "0.1.0";

export const GITHUB = "https://github.com/LipeMachado/cachycli";
export const RELEASES = `${GITHUB}/releases`;
export const RAW_INSTALL = `https://raw.githubusercontent.com/LipeMachado/cachycli/main/install.sh`;
export const RAW_INSTALL_PS1 = `https://raw.githubusercontent.com/LipeMachado/cachycli/main/install.ps1`;

/* ------------------------------------------------------------------ */
/*  Marketing                                                          */
/* ------------------------------------------------------------------ */

export const MARQUEE_ITEMS = [
  "opencode",
  "Claude Code",
  "agent profiles",
  "goals + verifiers",
  "workflows",
  "MCP servers",
  "editor agents",
  "sessions on disk",
  "subagents",
  "bench",
  "5 themes",
  "any model",
  "multi-line input",
  "plan mode",
];

export const HIGHLIGHTS: { title: string; body: string }[] = [
  {
    title: "One prompt box",
    body: "Switch between opencode, Claude Code, an editor agent and your own models without changing tools.",
  },
  {
    title: "Agents that keep the thread",
    body: "Profiles shape the agent, a goal rides in the header and survives restarts, and verifier subagents prove the job is done.",
  },
  {
    title: "Work that runs itself",
    body: "Multi-step workflows and agent-written plans run as subagents — you watch them in /tasks, and can stop any of them.",
  },
  {
    title: "Answers you can read",
    body: "Markdown, highlighted code, inline diffs and compact tool blocks — plus how long a turn took and when the first token arrived.",
  },
  {
    title: "Measured, not guessed",
    body: "/bench times the first token of every model on your machine, so the model list shows real numbers instead of names.",
  },
  {
    title: "Nothing hidden",
    body: "Sessions, keys and settings stay in a folder you can open, back up and delete — and nothing is uploaded anywhere.",
  },
];

export type Provider = {
  id: "providers" | "models" | "editor";
  name: string;
  index: string;
  tagline: string;
  body: string;
  command: string;
  config: string;
  shot: string;
  shotW: number;
  shotH: number;
  shotCaption: string;
  points: string[];
};

export const PROVIDERS: Provider[] = [
  {
    id: "providers",
    name: "providers",
    index: "01",
    tagline: "opencode and Claude Code, one interface.",
    body:
      "CachyCLI talks to the providers you already use — your opencode install or your Claude Code subscription — and draws every message, thought and tool call in its own interface. Pick one from /provider and the sign-in runs for you.",
    command: "cachycli --provider opencode",
    config: `[agent]
provider = "opencode"    # or "claude-code"`,
    shot: "/shots/12-providers.webp",
    shotW: 1878,
    shotH: 787,
    shotCaption: "connect opencode or Claude Code",
    points: [
      "Your opencode install: models, logins and tools",
      "Your Claude Code subscription, driven from here",
      "Swap providers mid-session",
    ],
  },
  {
    id: "models",
    name: "your models",
    index: "02",
    tagline: "Any model you can reach.",
    body:
      "Point CachyCLI at any compatible endpoint — a hosted API or something running on your own machine — and it works out of the box.",
    command: "cachycli --provider native",
    config: `[model.my-model]
model = "llama-3.1-70b"
base_url = "http://localhost:8080/v1"`,
    shot: "/shots/05-chat.webp",
    shotW: 1878,
    shotH: 1389,
    shotCaption: "your model · tool call and answer",
    points: ["Hosted or local endpoints", "Pick a model per session"],
  },
  {
    id: "editor",
    name: "your editor",
    index: "03",
    tagline: "Any agent with a standard plug.",
    body:
      "Connect CachyCLI to an agent that speaks the editor standard, or expose CachyCLI to your editor and keep the sessions you already have.",
    command: 'cachycli --provider acp --acp-command "opencode acp"',
    config: `[agent]
provider = "acp"

[agent.acp]
command = "opencode"
args = ["acp"]`,
    shot: "/shots/06-slash.webp",
    shotW: 1878,
    shotH: 1389,
    shotCaption: "editor agent · commands",
    points: ["Editors on one side, terminal on the other", "Same sessions in both"],
  },
];

export const STEPS: { title: string; body: string }[] = [
  { title: "Install", body: "One command for your system, or the Windows installer." },
  { title: "Connect", body: "opencode, an editor agent, or a model of your own." },
  { title: "Work", body: "Ask, review, approve. Sessions are saved as you go." },
];

/* ------------------------------------------------------------------ */
/*  Guide                                                              */
/* ------------------------------------------------------------------ */

export const TOOLS: { name: string; summary: string }[] = [
  { name: "run_terminal_cmd", summary: "Run shell commands, including long-running ones in the background." },
  { name: "read_file", summary: "Read a file with line numbers and ranges." },
  { name: "write", summary: "Create or replace a file." },
  { name: "search_replace", summary: "Make precise edits; every change shows as a diff." },
  { name: "list_dir", summary: "List what is in a folder." },
  { name: "grep", summary: "Search inside files with a pattern." },
  { name: "glob", summary: "Find files by name pattern." },
  { name: "web_fetch", summary: "Read a web page as text." },
  { name: "web_search", summary: "Search the web." },
  { name: "todo_write", summary: "Keep the task list you see in the plan panel." },
  { name: "task", summary: "Send a side task to a helper agent." },
  { name: "get_task_output", summary: "Check on a background command or helper." },
  { name: "wait_tasks", summary: "Wait for background work to finish." },
  { name: "kill_task", summary: "Stop a running task." },
  { name: "search_tool", summary: "Find tools from your connected servers." },
  { name: "use_tool", summary: "Use one of those tools." },
];

export const SHORTCUTS: { key: string; action: string }[] = [
  { key: "Enter", action: "Send the message" },
  { key: "Shift+Enter / Alt+Enter", action: "New line" },
  { key: "Ctrl+M", action: "Switch multi-line input" },
  { key: "Ctrl+C / Esc", action: "Stop the current turn, or clear the input" },
  { key: "Ctrl+Q / Ctrl+D", action: "Quit, with a confirmation" },
  { key: "Ctrl+O", action: "Turn always-approve on or off" },
  { key: "Ctrl+T", action: "Show or hide the plan panel" },
  { key: "Ctrl+R", action: "Resume a session" },
  { key: "Ctrl+P", action: "Command palette" },
  { key: "Ctrl+X / Ctrl+.", action: "Keyboard shortcuts" },
  { key: "F2 / Ctrl+,", action: "Settings" },
  { key: "Ctrl+U / Ctrl+Y", action: "Undo or redo your last input change" },
  { key: "Ctrl+A / Ctrl+E", action: "Start or end of the line" },
  { key: "Shift+Tab", action: "Cycle mode: normal → plan → always-approve" },
  { key: "PgUp / PgDn, wheel", action: "Scroll; mouse clicks and the wheel work inside panels too" },
  { key: "/mode", action: "Switch Normal / Plan / Always-approve" },
  { key: "/toggle-mouse-reporting", action: "Hand the mouse back to the terminal for text selection" },
  { key: "@", action: "Attach a file (add ! for hidden files, or path:10-50 for lines)" },
  { key: "/", action: "Open the command menu" },
];

export const SLASH_COMMANDS: { command: string; description: string }[] = [
  { command: "/model", description: "Switch model (list shows measured speed)" },
  { command: "/provider", description: "Connect opencode, Claude Code, your models or ACP" },
  { command: "/login", description: "Sign in (device login when the provider has one)" },
  { command: "/logout", description: "Sign out of the current provider" },
  { command: "/bench", description: "Time each model and annotate the list" },
  { command: "/effort", description: "Reasoning effort for the current model" },
  { command: "/personas", description: "Agent profiles from .cachy/agents and .claude/agents" },
  { command: "/goal", description: "Set the session goal, or verify it with subagents" },
  { command: "/workflow", description: "Run a multi-step job as subagents" },
  { command: "/tasks", description: "Background commands and subagents" },
  { command: "/timeline", description: "Turns with durations and tool counts" },
  { command: "/jump", description: "Scroll back to an earlier turn" },
  { command: "/rewind", description: "Drop the last turns from the session" },
  { command: "/fork", description: "Copy the session into a new one" },
  { command: "/search", description: "Search every saved session" },
  { command: "/worktree", description: "List, create or remove git worktrees" },
  { command: "/density", description: "How much of each tool call is shown" },
  { command: "/timestamps", description: "Toggle message timestamps" },
  { command: "/copy", description: "Copy the last answer to the clipboard" },
  { command: "/doctor", description: "Check terminal, configuration and provider support" },
  { command: "/privacy", description: "What leaves this machine (nothing by default)" },
  { command: "/backend", description: "Alias of /provider" },
  { command: "/new", description: "Start a new session" },
  { command: "/resume", description: "Resume a previous session" },
  { command: "/rename", description: "Rename the current session" },
  { command: "/delete", description: "Delete the current session" },
  { command: "/export", description: "Save the conversation as Markdown" },
  { command: "/dashboard", description: "Browse all sessions" },
  { command: "/compact", description: "Shrink the conversation history" },
  { command: "/context", description: "Show how much context is used" },
  { command: "/usage", description: "Show tokens for this session" },
  { command: "/memories", description: "Browse and manage memory" },
  { command: "/remember", description: "Save a note to memory" },
  { command: "/skills", description: "List or inject a skill" },
  { command: "/plugins", description: "Manage plugins" },
  { command: "/mcp", description: "Show connected tool servers" },
  { command: "/hooks-list", description: "Show hooks in this project" },
  { command: "/hooks-trust", description: "Trust this folder for hooks" },
  { command: "/todos", description: "Show or hide the task panel" },
  { command: "/always-approve", description: "Turn always-approve on or off" },
  { command: "/multiline", description: "Switch multi-line input" },
  { command: "/permissions", description: "Show the current permission mode" },
  { command: "/theme", description: "Change the theme" },
  { command: "/settings", description: "Open settings" },
  { command: "/shortcuts", description: "Show keyboard shortcuts" },
  { command: "/init", description: "Create an AGENTS.md for this project" },
  { command: "/inspect", description: "Show what CachyCLI found here" },
  { command: "/help", description: "Show help" },
  { command: "/exit", description: "Leave the interface" },
];

export const CLI_FLAGS: { flag: string; description: string }[] = [
  { flag: "-p, --single <PROMPT>", description: "Ask one question and exit" },
  { flag: "-m, --model <MODEL>", description: "Use a specific model" },
  { flag: "-r, --resume [<ID_OR_TITLE>]", description: "Resume a session" },
  { flag: "-c, --continue", description: "Continue the most recent session" },
  { flag: "-s, --session-id <ID>", description: "Keep a named session for scripts" },
  { flag: "-w, --worktree [<NAME>]", description: "Work in a new git worktree" },
  { flag: "--backend <NAME>", description: "opencode, native or acp" },
  { flag: "--opencode-url <URL>", description: "Use a running opencode server" },
  { flag: "--acp-command <CMD>", description: "The agent command to connect to" },
  { flag: "--output-format <FMT>", description: "plain, json or streaming formats" },
  { flag: "--always-approve", description: "Approve tools without asking" },
  { flag: "--permission-mode <MODE>", description: "How much the agent may do unattended" },
  { flag: "--allow / --deny <RULE>", description: "Allow or block specific actions" },
  { flag: "--tools / --disallowed-tools", description: "Limit which tools exist" },
  { flag: "--max-turns <N>", description: "Stop after N turns" },
  { flag: "--rules <TEXT>", description: "Add your own instructions" },
];

export const SUBCOMMANDS: { name: string; description: string }[] = [
  { name: "agent stdio", description: "Let an editor connect to CachyCLI" },
  { name: "mcp", description: "Manage, test and serve tool servers" },
  { name: "models", description: "List available models" },
  { name: "sessions", description: "List, search, restore or delete sessions" },
  { name: "export", description: "Save a conversation as Markdown" },
  { name: "inspect", description: "Show what was found for this folder" },
  { name: "doctor", description: "Check your setup" },
  { name: "memory", description: "Manage saved notes" },
  { name: "completions", description: "Shell tab-completion for bash, zsh, fish, PowerShell" },
  { name: "update", description: "Check for or install updates" },
  { name: "usage", description: "Show tokens for a session" },
  { name: "du", description: "Show how much disk CachyCLI uses" },
  { name: "worktree", description: "Manage git worktrees" },
  { name: "login / logout", description: "Store or remove a key" },
  { name: "dashboard", description: "Open the sessions view" },
];

export const FAQ: { q: string; a: string }[] = [
  {
    q: "Do I need an account?",
    a: "No. CachyCLI has no sign-up and sends nothing anywhere by itself. It talks to the endpoints you configure and keeps everything in one local folder.",
  },
  {
    q: "Can I use it with my editor?",
    a: "Yes. Editors that support the agent standard can run CachyCLI directly, and you can keep the same sessions you have in the terminal.",
  },
  {
    q: "Which platforms are supported?",
    a: "Linux, macOS and Windows. There is a one-line installer for each, and a direct download for the compiled program.",
  },
  {
    q: "How do I add my own tools?",
    a: "Connect a tool server (MCP). Add it once and the agent can call its tools from then on.",
  },
  {
    q: "Where are my conversations?",
    a: "In a folder inside your home directory, one folder per project. You can open, copy, search or delete them like any other file.",
  },
  {
    q: "Can I stop the agent from doing things?",
    a: "Yes. Rules can allow or block specific actions, there are permission modes, and every unclear action asks first.",
  },
];
