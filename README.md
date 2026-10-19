# Cachy (`cachycli`)

A terminal coding agent written in **Rust** with **ratatui + crossterm**: a
full-screen interface — welcome card, prompt box, status header, modals,
approval prompts, themes — that drives whichever agent you already use.

- **opencode** — its models, logins and tools, through its server or its CLI
- **Claude Code** — the CLI you are subscribed to
- **any OpenAI-compatible or Anthropic endpoint** — the built-in agent loop
- **any ACP agent** — over stdio, or over a WebSocket
- **MCP servers**, in both directions: Cachy as a client, and as a server

Sessions, subagents, goals, workflows, worktrees, memory and sharing are
Cachy's own, so they work the same whichever provider answers.

```
   ⣀⣀⣀⣀⡀    ⣀⣀⡀
  ⣼⣿⠟⠛⠛⢿⣦  ⢸⣿⠛⢛⣿⡆   Cachy 0.1.0
  ⣿⣿⣀⣀⣀⣀⣹⣿⡆ ⢸⣿⢠⣿⠟⠁
  ⠘⠿⣿⣿⣿⣿⠿⠃ ⢸⣿⣿⡀      A terminal AI coding agent that connects
   ⣼⣿⠋⠉⠙⢿⣆ ⢸⣿⠹⣿⣆    with opencode and other agents.
  ⣼⣿⣧⣀⣀⣀⣼⣿⡆⢸⣿⡆⠙⣿⣆
  ⠛⠛⠛⠛⠛⠛⠛⠛⠃⠘⠛⠃ ⠛⠛
```

## Keys and logins

Providers are authenticated through the tool that owns them (CachyCLI drives your
existing install), so there is nothing extra to configure here.

- **opencode** — `/login` inside CachyCLI runs `opencode console login`, the
  **device login**: it prints a code and a URL
  (`https://opencode.ai/console/device?user_code=…`), you authorize in the
  browser and the credential is stored — **no API key to copy**. Fallbacks:
  - In the same menu you can also choose **GitHub Copilot** (browser login, no
    key, needs a Copilot subscription).
  - The key prompt (`opencode auth login` → OpenCode) accepts a key from
    **https://opencode.ai/console** → **Personal API keys** (`sk-…` / `oc_sk_…`).
  - `opencode auth list` shows what is stored; `opencode auth logout <provider>`
    removes it.
- **Claude Code** — uses the `claude` CLI's own login (`claude` and `/login`).
- **Your models** — `/provider` → *your models*, then set an API key for the
  endpoint you configured (`cachycli login --api-key …`).
- **Editor agents (ACP)** — whatever the agent itself uses (e.g. `opencode acp`).

If a credential stops working, CachyCLI says so and offers to switch to a model
that needs no key, so you are never stuck mid-session.

## Install

**Linux / macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/LipeMachado/cachycli/main/install.sh | bash
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/LipeMachado/cachycli/main/install.ps1 | iex
```

**From source** (any platform with a Rust toolchain):

```bash
cargo install --git https://github.com/LipeMachado/cachycli cachycli
# or, in a checkout:
cargo build --release -p cachycli
```

Verify:

```bash
cachycli version
cachycli doctor
```

## Quick start

```bash
# Just run it
cachycli

# First time: connect something to answer your prompts
#   (inside the interface)
/provider
```

`/provider` opens a picker with the state of each provider; choosing one that is
not signed in runs its sign-in for you (the browser opens where it should) and
then switches the session over. CachyCLI picks a sensible default on its own —
the first provider that is installed and signed in, usually opencode.

```bash
# Pick a provider for one run
cachycli --provider opencode
cachycli --provider claude-code -p "summarize this diff"

# Headless / scripting
cachycli -p "Explain this codebase"
cachycli -p "List TODOs" --output-format json | jq -r .text

# Expose cachycli as an ACP agent for IDEs (Zed, Neovim, Emacs)
cachycli agent stdio
```

## Providers

| Provider | How it works | Pick it |
| --- | --- | --- |
| `opencode` | Drives your `opencode` installation (models, logins, tools) — CLI transport by default, HTTP + SSE server optional | `/provider` or `--provider opencode` |
| `claude-code` | Drives the Claude Code CLI (`claude -p --output-format stream-json`) with your subscription | `/provider` or `--provider claude-code` |
| `native` | Built-in agent loop (tools, subagents, compaction) over any OpenAI-compatible or Anthropic endpoint | `/provider` or `--provider native` |
| `acp` | Connects to any Agent Client Protocol agent — over stdio, or over a WebSocket when `[agent.acp] url` is set — and renders its updates | `/provider` or `--provider acp` |

An ACP agent can also live on another machine — the socket replaces the command:

```toml
[agent]
provider = "acp"

[agent.acp]
url = "ws://build-box:8765"   # `cachycli agent serve` on the other side
```

Remember a choice in `~/.cachy/config.toml`:

```toml
[agent]
provider = "opencode"
```

## Version

One place to bump: `[workspace.package] version` in `Cargo.toml`. `cachycli
version` reads it, and the release workflow names its assets with it.

`cachycli update` installs the latest release (`--check` only looks,
`--version x.y.z` picks one).

## Uninstalling

`cachycli uninstall` removes what the installer put there — the binary, the
`~/.cachy/bin` symlink and the PATH line it added to `~/.bashrc`, `~/.zshrc`
and `~/.profile`. Your data is kept:

```bash
cachycli uninstall            # binary, symlink, PATH lines
cachycli uninstall --purge    # and ~/.cachy (config, sessions, memory), after asking
cachycli uninstall --purge --yes   # without the question
```

A binary outside your home (a package manager, `/usr/local`) is left alone
with a note saying so, since whatever installed it should remove it.

### opencode integration (both directions)

```toml
# ~/.cachy/config.toml
[agent]
backend = "opencode"

[agent.opencode]
command = "opencode"     # the binary to run
transport = "auto"       # auto | cli | server
# url = "http://127.0.0.1:4096"
# model = "provider/model"
# agent = "build"
```

* **cachycli → opencode**: `cachycli --backend opencode` renders opencode's
  answers, thoughts and tool calls inside the cachycli interface.
* **opencode → cachycli**: register cachycli as an MCP server so opencode can
  call it:

  ```bash
  opencode mcp add cachy --command "cachycli mcp serve"
  ```

  This exposes `cachy_run`, `cachy_models` and `cachy_sessions`.

### Models (native backend)

```toml
# ~/.cachy/config.toml
[models]
default = "my-model"

[model.my-model]
model = "llama-3.1-70b"
base_url = "http://localhost:8080/v1"     # any OpenAI-compatible endpoint
name = "Local Llama"
env_key = "LOCAL_API_KEY"                 # or api_key = "..."
context_window = 128000
```

Built-in presets (need only an API key): `gpt-5`, `gpt-4o`,
`claude-sonnet-4-5`, `ollama`, `lmstudio`, `vllm`, `opencode-zen`.

```bash
cachycli models                 # list the catalog and credential status
cachycli login --api-key sk-... # store a credential in ~/.cachy/auth.json
```

## Interface

* **Welcome card** with logo, tagline and action hints (`ctrl+r` resume,
  `ctrl+p` command palette, `ctrl+x` shortcuts, `ctrl+q` quit).
* **Prompt box** with the `❯` marker, spinner while a turn runs, and the
  current model on the bottom-right border.
* **Header** with the (compressed) working directory, backend tag, context
  usage (`used / window`) and `[Dashboard]`.
* **Tool calls** as compact blocks (`◆` running, `◈` completed) with elapsed
  time, diff stats and output previews; **inline diffs** for edits; **TODO
  panel**; **"Worked for Xs"** after each turn.
* **Approval prompts** with the four options (always / this session / yes /
  reject with feedback).
* **Modals**: command palette (Ctrl+P), keyboard shortcuts (Ctrl+X), settings
  (F2 / Ctrl+,), resume picker (Ctrl+R), model picker (`/model`), theme picker
  (`/theme`), session dashboard (`/dashboard`).
* **Themes**: Cachy Night (default), Cachy Day, Tokyo Night, Rosé Pine Moon,
  Oscura Midnight.
* **Task panel** (`/tasks`): background commands and subagents with live elapsed
  time; `Enter` shows a task's full output, `x` kills it, `r` refreshes.
* **Running strip** above the prompt: animated blocks, what is running, the
  elapsed time, tokens so far and a clickable `[stop]`.

### Agents

* **Profiles** (`/personas`): Markdown files in `.cachy/agents/`,
  `~/.cachy/agents/` or `.claude/agents/` that get injected into the system
  prompt. `/persona <name>` applies one, `none` clears it.
* **Goals** (`/goal`): the goal rides in the header, goes into every system
  prompt and is stored with the session. `/goal verify` runs
  `[agent] goal_verifier_count` (3) verifier subagents that read the repository
  and answer *met / not met / unclear*. The agent can set its own goal with the
  `set_goal` tool.
* **Workflows** (`/workflow`): a named list of steps
  (`.cachy/workflows/<name>.md` with `## Step:` sections, or `.json`); every step
  runs as a subagent. The `run_workflow` tool lets the model pick one itself.
* **Plans the model writes** (`plan_workflow` tool): up to 8 steps with
  `depends_on`; independent steps run together and dependents receive the
  reports of the steps they need. `save: true` stores the plan as a workflow.
* **Limits**: `[subagents] max_concurrent` (queue instead of stampede) and
  `max_depth` (the task tool refuses deeper nesting).
* **Long sessions**: `/compact` summarises older segments while the newest
  messages stay verbatim; `/rewind`, `/fork`, `/timeline`, `/jump` and
  `/search` cover the rest.
* **`/bench`** times the first token of each model on your machine and annotates
  the model list with the measured seconds.

### Keyboard

| Key | Action |
| --- | --- |
| `Enter` | Send |
| `Shift+Enter` / `Alt+Enter` | Newline |
| `Ctrl+M` | Toggle multiline input |
| `Ctrl+C` / `Esc` | Cancel turn (or clear input) |
| `Ctrl+Q` / `Ctrl+D` | Quit (with confirmation) |
| `Ctrl+O` | Toggle always-approve |
| `Ctrl+T` | Toggle TODO panel |
| `Ctrl+R` | Resume session picker |
| `Ctrl+P` | Command palette |
| `Ctrl+X` / `Ctrl+.` | Keyboard shortcuts |
| `F2` / `Ctrl+,` | Settings |
| `Ctrl+U` | Undo input change |
| `Ctrl+A` / `Ctrl+E` | Start / end of line |
| `Shift+Tab` | Cycle mode (Normal → Plan → Always-approve) |
| `PgUp` / `PgDn`, wheel | Scroll scrollback |
| `@` | Fuzzy file picker (`@!` includes hidden files, `path:10-50` ranges) |
| `/` | Slash commands |

### Slash commands

Sessions: `/new`, `/resume`, `/rename`, `/delete`, `/session-info`, `/fork`,
`/rewind`, `/jump`, `/search`, `/export`, `/import`, `/share`, `/shares`,
`/timeline`, `/tasks`, `/dashboard`, `/compact`, `/context`, `/usage`,
`/worktree`, `/goal`, `/workflow`, `/personas`.

Providers and models: `/provider`, `/model`, `/effort`, `/bench`, `/login`,
`/logout`, `/privacy`.

Interface: `/theme`, `/density`, `/timestamps`, `/notify`, `/copy`,
`/multiline`, `/permissions`, `/always-approve` (`/yolo`), `/todos`, `/mode`,
`/settings`, `/shortcuts`, `/palette`, `/toggle-mouse-reporting`, `/doctor`.

Extending it: `/skills`, `/plugins`, `/mcp`, `/hooks-list`, `/hooks-trust`,
`/memories`, `/remember`, `/flush`, `/init`, `/inspect`, `/help`, `/exit`.

## Tools

| Tool | Purpose |
| --- | --- |
| `run_terminal_cmd` | Shell commands, timeouts, background tasks |
| `read_file` / `write` / `search_replace` | Read with line numbers, create, precise edits |
| `list_dir` / `grep` / `glob` | Discovery (gitignore-aware) |
| `web_fetch` / `web_search` | Documentation fetch; search via `CACHY_SEARCH_ENDPOINT` or DuckDuckGo |
| `todo_write` | Session task list (rendered in the TODO panel) |
| `task` / `get_task_output` / `wait_tasks` / `kill_task` | Subagents (`general-purpose`, `explore`, `plan`) |
| `search_tool` / `use_tool` | Call MCP tools from configured servers |

Filter tools with `--tools` (allowlist) and `--disallowed-tools` (denylist);
gate them with `--allow` / `--deny` rules (`Bash(npm*)`, `Edit(src/**)`,
`WebFetch(domain:docs.rs)`, …) and permission modes (`default`, `acceptEdits`,
`auto`, `dontAsk`, `bypassPermissions`, `plan`).

## Serving CachyCLI to other clients

```bash
cachycli agent serve --port 8765      # WebSocket, localhost by default
```

Two dialects on the same port. If the first frame is JSON-RPC (a `method` field)
the socket speaks **ACP** — the same server `cachycli agent stdio` runs, so editor
clients that support a WebSocket transport can connect here instead of using pipes.

Otherwise it is the simple protocol: one agent per connection, in the server's
directory. Send `{"prompt": "run the tests"}`
(or a plain text frame) and read the events back as JSON — `{"type":"text","data":…}`,
`{"type":"thought",…}`, `{"type":"tool",…}`, `{"type":"done","stop":…}`. Send
`{"cancel": true}` to stop a running turn.

A client that only speaks stdio can drive a remote agent through the relay —
stdin/stdout on one side, the socket on the other:

```bash
cachycli agent headless --ws-url ws://build-box:8765
```

Sessions travel over the same socket, so a server doubles as a handover point:

```
client → {"share": "<bundle>"}   stores it; answers {"type":"shared","id":…,…}
client → {"fetch": "<id>"}       answers {"type":"bundle","id":…,"data":"…"} ("" = newest)
client → {"shares": true}        answers {"type":"shares","items":[…]}
```

Nothing listens on the network unless you run it, and the bundles stay in the
server's own `~/.cachy/shares`.

## Plugins from an index

Point `[plugins] index` at a JSON file or URL:

```json
[
  { "name": "team-hooks", "description": "Shared hooks", "git": "https://…/team-hooks.git" },
  { "name": "local-thing", "path": "/opt/local-thing" }
]
```

Then `/plugin search <term>` lists what matches and installing clones (git) or copies
(path) the entry into `~/.cachy/plugins/<name>`; `/plugin` shows what is loaded and
which index is in use.

## MCP

```toml
# ~/.cachy/config.toml
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path"]

[mcp_servers.remote]
url = "https://mcp.example.com/mcp"
headers = { authorization = "Bearer ${MCP_TOKEN}" }
```

```bash
cachycli mcp list
cachycli mcp add linear --command npx --arg -y --arg mcp-remote --arg https://mcp.linear.app/mcp
cachycli mcp test filesystem
cachycli mcp serve          # expose cachycli itself as an MCP server
```

## Sessions

Sessions persist automatically:

```
~/.cachy/sessions/<encoded-cwd>/<session-id>/
  summary.json          # title, timestamps, model, counts
  chat_history.jsonl    # messages sent to the model
  updates.jsonl         # UI-level stream (messages, tool calls, usage)
  plan.json             # todo state
  system_prompt.txt     # last assembled system prompt
  signals.json          # turn counter and token usage
```

```bash
cachycli sessions list
cachycli sessions search "login bug"
cachycli --resume "fix login bug"      # by id or title
cachycli --continue                    # most recent for this directory
cachycli -p "and now the tests" -s my-session   # named headless session
cachycli export --output session.md
```

A session remembers which conversation the *provider* is having, too: resume
one and opencode, ACP or Claude Code continues that session instead of starting
a fresh one (`/new` starts over on both sides). The transcript is written
whichever provider runs the model, so `/export`, `/share`, the dashboard and
`/timeline all` see the same thing everywhere.

`/timeline` shows this session's turns with a bar scaled to each duration;
`/timeline all` shows every session's, read back from the stored update
streams — when each turn ran, how long it took, how much work was in it and
which project it was in; `/timeline all <project>` narrows it down.

### Sharing a session

`/share` writes a session to a portable `.cachy` file (readable JSON: messages,
model, goal); the other side opens it with `/import <file>`. The same bundle can
travel over the network instead of through a chat window:

```bash
# on the machine that has the session
cachycli share --to ws://build-box:8765            # latest session here
cachycli share --to ws://build-box:8765 --session "fix login bug"

# on the other machine
cachycli share --to ws://build-box:8765 --list
cachycli share --to ws://build-box:8765 --id 01a150ec-…       # imports, ready to resume
cachycli share --to ws://build-box:8765 --remove 01a150ec-…   # the server forgets it
```

In the interface it is `/share push [url]`, `/share pull <id> [url]` and
`/shares` (what the server holds, with `/shares remove <id>`); set the server
once and the url can be left out:

```toml
[share]
server = "ws://build-box:8765"
```

The server keeps what it received under its own `~/.cachy/shares`, so a pushed
session survives a restart and can be fetched by anyone who can reach the port
(it binds localhost by default).

## Configuration

Layered, highest priority last:

1. `~/.cachy/managed_config.toml` (enterprise)
2. `~/.cachy/config.toml`
3. `<project>/.cachy/config.toml` (MCP servers, models), from the repository
   root down to the working directory — never above your home directory, where
   `~/.cachy` is the user config
4. CLI flags and `CACHY_*` environment variables

```toml
[cli]
auto_update = false

[models]
default = "my-model"

[features]
web_fetch = true
web_search = false
# The rest of [features] (support_permission, telemetry, feedback,
# codebase_indexing, lsp_tools) is accepted so an existing config keeps
# parsing; they have nothing to switch here — Cachy talks to no cloud and
# ships no LSP client.

[ui]
theme = "cachy-night"        # cachy-night | cachy-day | tokyonight | rosepine-moon | oscura-midnight
max_thoughts_width = 120
show_thinking = true
notify = "off"                # off | bell | osc (desktop notification)
notify_after = 20             # only turns longer than this notify

[session]
auto_compact_threshold_percent = 85

[tools]
respect_gitignore = true

[toolset.bash]
timeout_secs = 120
output_byte_limit = 65536

[skills]
paths = ["~/my-team-skills"]

[subagents]
enabled = true

[subagents.models]
explore = "gpt-4o"

[memory]
enabled = false
```

Project rules come from `AGENTS.md` / `AGENT.md` / `CLAUDE.md` (repo root →
cwd). Skills are `SKILL.md` directories in `.cachy/skills`, `~/.cachy/skills`
and `~/.claude/skills`. Hooks live in `.cachy/hooks/*.json` and require
`/hooks-trust`.

Environment variables: `CACHY_HOME`, `CACHY_MODEL`, `CACHY_MODELS_BASE_URL`,
`CACHY_AGENT_BACKEND`, `CACHY_MEMORY`, `CACHY_SUBAGENTS`, `CACHY_THEME`,
`CACHY_SEARCH_ENDPOINT`, `CACHY_UPDATE_REPO`, `OPENCODE_SERVER_URL`,
`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENCODE_API_KEY`, `NO_COLOR`.

## CLI reference

```
cachycli [OPTIONS] [PROMPT] [COMMAND]

Options:
  -p, --single <PROMPT>          headless single-turn prompt
  -m, --model <MODEL>            model id
  -r, --resume [<ID_OR_TITLE>]   resume a session by id or title
  -c, --continue                 continue the most recent session
  -s, --session-id <ID>          named headless session
  -w, --worktree [<NAME>]        start in a new git worktree
      --backend <BACKEND>        native | opencode | acp
      --opencode-url <URL>       attach to an opencode server
      --acp-command <CMD>        ACP agent command (e.g. "opencode acp")
      --output-format <FMT>      plain | json | streaming-json | streaming-messages-json
      --always-approve           auto-approve tools
      --permission-mode <MODE>   default | acceptEdits | auto | dontAsk | bypassPermissions | plan
      --allow / --deny <RULE>    permission rules (repeatable)
      --tools / --disallowed-tools <LIST>
      --max-turns <N>            headless turn budget
      --rules <TEXT>             extra system-prompt rules
      --no-alt-screen / --minimal
```

Commands: `agent stdio`, `mcp`, `models`, `sessions`, `export`, `inspect`,
`doctor`, `memory`, `plugin`, `completions`, `update`, `uninstall`, `usage`, `du`,
`worktree`, `wrap`, `trace`, `setup`, `login`, `logout`, `version`,
`dashboard`, `leader`, `clone`, `cursor-worker`.

## Development

```bash
cargo build                       # debug build
cargo test                        # unit tests
cargo run -p cachycli -- doctor   # smoke test
cargo build --release -p cachycli # release binary at target/release/cachycli
```

Workspace layout:

```
crates/cachy-core     agent engine, providers, tools, sessions, MCP, ACP
crates/cachycli       CLI, subcommands and the ratatui TUI
cachycli-web          the site: landing page and the guide (Next.js)
install.sh/ps1        installers; .github/workflows builds the releases
```

The site lives here on purpose. It is not a separate product: its guide
documents these exact commands and flags, and its screenshots are captured
from this binary, so a feature and the page that explains it land in the same
commit. The release workflow only builds the crates; the site deploys on its
own (Vercel), from `cachycli-web/`.

## License

Apache-2.0. Cachy is an independent project: it is not affiliated with, or
endorsed by, opencode, Anthropic or any other provider it can drive.
