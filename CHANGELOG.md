# Changelog

## 0.3.1

- `/model` offers only the models of providers you are **signed in to**:
  Claude Code without a login and endpoints without a key are no longer in
  the list, so picking one cannot produce a failed turn.
- `/provider` says the same: "needs sign-in" when the CLI is installed but has
  no credentials, "signed in" when it does. Codex and Gemini are checked
  through their own credential files and environment keys.

## 0.3.0

- `/model` shows the models of **every provider you are signed in to**, grouped
  by provider, not only the one you are on. Choosing a model from another
  provider switches to it and applies the model, and both are remembered.
- The CLI agents (Codex, Gemini, Claude Code) have no command that lists their
  models, so their names come from a small built-in list; `[agent.<name>] model`
  still overrides anything.

## 0.2.0

- Two more CLI agents: **Codex** (`codex exec --json`) and **Gemini**
  (`gemini -p --output-format stream-json`), driven headless like Claude Code
  already was, each keeping its own session id so `--resume` continues the
  conversation there.
- **Grok** through its endpoint: `grok-4` and `grok-4-mini` are in the model
  list, `XAI_API_KEY` is the only thing to set.
- `/provider` lists all of them with what it knows: installed, signed in, or
  not there yet.

## 0.1.1

- The model list is read from the opencode server first (the CLI is only a
  fallback), so `/model` offers the models your login actually has.
- A model list that cannot be read says so once, calmly, instead of repeating
  itself on every refresh.
- A turn that fails now explains itself in plain output, not only in JSON.
- `cachycli uninstall` removes the binary, its symlink and the PATH lines the
  installer added (`--purge` also removes `~/.cachy`, after asking).

## 0.1.0 — first release

First release: a terminal coding agent written in Rust with a ratatui /
crossterm interface that drives opencode, Claude Code, your own
OpenAI-compatible or Anthropic endpoints, or any ACP agent.

### Interface
- Fullscreen TUI with a welcome card, prompt box, header status and hint
  line, a consistent theme and a modal for every picker.
- Themes: Cachy Night (default), Cachy Day, Tokyo Night, Rosé Pine Moon,
  Oscura Midnight.
- Status strip while a turn runs: animated dots, elapsed time, token counter
  and a clickable `[stop]`; `Thought for Xs` blocks that collapse; message
  queueing (`N queued`) while the agent is busy.
- Slash-command menu with argument options and the current value preselected,
  `@` fuzzy file picker with hidden-file (`!`) support and `path:10-50`
  ranges, command palette (Ctrl+P), keyboard shortcuts modal (Ctrl+X),
  settings modal (F2), resume picker (Ctrl+R), todo panel (Ctrl+T), history
  with Up/Down, input undo (Ctrl+U), multi-line input (Ctrl+M).
- Markdown rendering with syntax highlighting, inline diffs, tool-call blocks
  and the four-option approval prompt.
- Session tools in the interface: `/bench` (measured time-to-first-token,
  annotates `/model`), `/timeline` (per-turn duration bars) and `/timeline all`
  (the same across every session, read back from the stored update streams),
  `/tasks`
  (live task rows, `x` to kill, Enter for output), `/jump`, `/rewind`,
  `/fork`, `/search` (across sessions), `/worktree`, `/density`,
  `/timestamps`, `/copy`, `/doctor`, `/privacy`, `/export`, `/share`,
  `/import`, `/goal`, `/workflow`, `/personas`, `/plugin`, `/login`,
  `/logout`, `/provider`.
- Dashboard and resume pickers with per-session bars, and `/tasks` with a
  frozen duration once a task finishes.
- `/notify` (also `[ui] notify` / `notify_after`) rings the terminal bell or
  posts a desktop notification when a turn longer than the threshold ends —
  never for a turn you cancelled yourself.

### Agent engine
- Native agent loop over OpenAI-compatible chat completions and the Anthropic
  Messages API, with streaming, tool calls, subagents, TODO tracking and
  automatic context compaction.
- Personas (agent definitions), goals with verification (`/goal verify` runs
  N verifier subagents), workflows and multi-step plans (`plan_workflow` with
  `depends_on`, waves and `save`), and the tools that drive them
  (`set_goal`, `run_workflow`, `plan_workflow`).
- Subagents with a depth guard and a concurrency limit (`[subagents]`).
- Tools: `run_terminal_cmd`, `read_file`, `write`, `search_replace`,
  `list_dir`, `grep`, `glob`, `web_fetch`, `web_search`, `todo_write`, `task`,
  `get_task_output`, `wait_tasks`, `kill_task`, `search_tool`, `use_tool`.
- Permission engine with `--allow` / `--deny` rules, permission modes,
  always-approve, and per-session approvals.
- Cross-session memory, skills discovery and hooks with folder trust.

### Providers
- `opencode`: drives your installation through its server (`opencode serve`,
  V1 API + SSE) with the CLI as fallback; the model is sent per message, so
  switching models keeps the session and its cache. Restarts the server and
  retries once when credentials go stale.
- `claude-code`: drives the Claude Code CLI with your subscription.
- `native`: the built-in loop over any OpenAI-compatible or Anthropic
  endpoint.
- `acp`: any Agent Client Protocol agent — spawned over stdio, or reached over
  a WebSocket with `[agent.acp] url` (`cachycli agent serve` speaks ACP).
- `/provider` lists what is installed and signed in, and runs the login flow
  (device login first, API key as fallback).

### Sharing and serving
- `/share` writes a session to a portable `.cachy` bundle (readable JSON:
  messages, model, goal); `/import <file>` opens one.
- `cachycli agent serve --port 8765` is a WebSocket server that speaks two
  dialects on one port: JSON-RPC clients get the full **ACP** server
  (`initialize`, sessions, prompts, permissions, streaming updates), plain
  clients get the simple prompt/event protocol.
- `cachycli share --to ws://host:8765` hands a session to a running server,
  `--id <id>` fetches one back, `--list` shows what it holds and `--remove <id>`
  makes it forget one; in the interface it is `/share push`, `/share pull <id>`
  and `/shares`, with `[share] server` as the default url. Bundles stay in the
  server's `~/.cachy/shares`.
- `cachycli agent stdio` exposes cachycli to editors as an ACP agent, and
  `cachycli mcp serve` exposes it as an MCP server.
- Plugin index: point `[plugins] index` at a JSON file or URL and
  `/plugin search <term>` installs entries by clone or copy.

### Sessions and configuration
- Resuming a session continues the provider's own conversation as well:
  the id of the opencode/ACP/Claude Code session is stored with the transcript,
  and `--resume` (or Ctrl+R) asks that provider to carry on instead of starting
  fresh. `/new` resets both sides. The transcript is written for every provider,
  so `/export`, `/share`, the dashboard and `/timeline all` work the same way.
- Sessions persisted under `~/.cachy/sessions/<encoded-cwd>/<id>/` with
  `summary.json`, `chat_history.jsonl`, `updates.jsonl`, `plan.json`,
  `system_prompt.txt` and `signals.json`.
- Layered configuration (`managed_config.toml`, `config.toml`, project
  `.cachy/config.toml` from the repository root down to the working directory,
  never above the home directory), custom `[model.*]` endpoints, `[mcp_servers.*]`,
  `[skills]`, `[subagents]`, `[memory]`, `[ui]`, `[keys]`, `[share]`.
- Project rules from `AGENTS.md` / `AGENT.md` / `CLAUDE.md`.
- Terminal safety: the screen, cursor, mouse mode and raw mode are restored on
  exit, on panic and on SIGINT/SIGTERM; the mouse is disabled while a child
  process (a login) owns the terminal.

### CLI
- Headless mode (`-p`) with `plain`, `json`, `streaming-json` and
  `streaming-messages-json` outputs; `--resume`, `--continue`, named sessions.
- Subcommands: `agent`, `mcp`, `models`, `sessions`, `share`, `export`,
  `inspect`, `doctor`, `memory`, `plugin`, `completions`, `update`, `usage`,
  `du`, `worktree`, `wrap`, `trace`, `setup`, `login`, `logout`, `version`,
  `dashboard`.
- Installers for Linux/macOS (`install.sh`) and Windows (`install.ps1`), and a
  GitHub Actions release workflow producing archives for Linux, macOS and
  Windows on x86_64 and aarch64.

### Site
- `cachycli-web`: the marketing site and the guide (Next.js), with the
  interface reproduced in motion, a per-topic guide under `/guide/<topic>`, and
  the version taken from `NEXT_PUBLIC_CACHY_VERSION` at build time.
