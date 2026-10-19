//! Slash command registry shown by the prompt menu and the command palette.

#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub args: Option<&'static str>,
    pub category: &'static str,
}

pub const COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/model",
        aliases: &["/m", "/models"],
        description: "Switch model",
        args: Some("[name]"),
        category: "Model & Input",
    },
    SlashCommand {
        name: "/provider",
        aliases: &["/providers", "/backend", "/connect"],
        description: "Connect a provider: opencode, Claude Code, your models",
        args: Some("[name]"),
        category: "Providers",
    },
    SlashCommand {
        name: "/login",
        aliases: &["/signin"],
        description: "Sign in to the current provider (browser login when available)",
        args: None,
        category: "Providers",
    },
    SlashCommand {
        name: "/logout",
        aliases: &["/signout"],
        description: "Sign out of the current provider",
        args: None,
        category: "Providers",
    },
    SlashCommand {
        name: "/new",
        aliases: &["/clear"],
        description: "Start a new session",
        args: None,
        category: "Session",
    },
    SlashCommand {
        name: "/rewind",
        aliases: &[],
        description: "Drop the last turns from the session",
        args: Some("[n]"),
        category: "Session",
    },
    SlashCommand {
        name: "/fork",
        aliases: &[],
        description: "Copy this session into a new one",
        args: None,
        category: "Session",
    },
    SlashCommand {
        name: "/timeline",
        aliases: &[],
        description:
            "Turns of this session with their durations, or of every session (`all [project]`)",
        args: Some("[all [project]]"),
        category: "Session",
    },
    SlashCommand {
        name: "/notify",
        aliases: &[],
        description: "Tell me when a long turn ends: off, bell or a desktop notification",
        args: Some("[off|bell|osc]"),
        category: "Interface",
    },
    SlashCommand {
        name: "/jump",
        aliases: &[],
        description: "Scroll back to an earlier turn",
        args: None,
        category: "Session",
    },
    SlashCommand {
        name: "/workflow",
        aliases: &["/workflows"],
        description: "Run a multi-step job with subagents",
        args: Some("[name]"),
        category: "Tools",
    },
    SlashCommand {
        name: "/search",
        aliases: &["/find"],
        description: "Search every saved session",
        args: Some("<text>"),
        category: "Session",
    },
    SlashCommand {
        name: "/goal",
        aliases: &[],
        description: "Set the session goal, or verify it with subagents",
        args: Some("[text|verify|clear]"),
        category: "Session",
    },
    SlashCommand {
        name: "/share",
        aliases: &[],
        description: "Write this session to a portable .cachy file, or push it to a server",
        args: Some("[path|push|pull]"),
        category: "Session",
    },
    SlashCommand {
        name: "/shares",
        aliases: &[],
        description: "What a server is holding, and remove <id>",
        args: Some("[[remove <id>] [url]]"),
        category: "Session",
    },
    SlashCommand {
        name: "/import",
        aliases: &["/open"],
        description: "Open a session bundle written by /share",
        args: Some("<file>"),
        category: "Session",
    },
    SlashCommand {
        name: "/worktree",
        aliases: &["/worktrees"],
        description: "List, create or remove git worktrees",
        args: Some("[name|remove <name>]"),
        category: "Project",
    },
    SlashCommand {
        name: "/tasks",
        aliases: &[],
        description: "Background commands and subagents",
        args: None,
        category: "Panels",
    },
    SlashCommand {
        name: "/resume",
        aliases: &["/load", "/continue", "/sessions"],
        description: "Resume a previous session",
        args: Some("[id|title]"),
        category: "Session",
    },
    SlashCommand {
        name: "/rename",
        aliases: &[],
        description: "Rename the current session",
        args: Some("<title>"),
        category: "Session",
    },
    SlashCommand {
        name: "/delete",
        aliases: &[],
        description: "Delete this session",
        args: None,
        category: "Session",
    },
    SlashCommand {
        name: "/session-info",
        aliases: &[],
        description: "Show session info",
        args: None,
        category: "Session",
    },
    SlashCommand {
        name: "/export",
        aliases: &[],
        description: "Export the conversation to a Markdown file",
        args: Some("[filename]"),
        category: "Session",
    },
    SlashCommand {
        name: "/dashboard",
        aliases: &[],
        description: "Open the sessions dashboard",
        args: None,
        category: "Session",
    },
    SlashCommand {
        name: "/compact",
        aliases: &["/compress"],
        description: "Compact conversation history",
        args: None,
        category: "Context",
    },
    SlashCommand {
        name: "/context",
        aliases: &[],
        description: "Show context window usage",
        args: None,
        category: "Context",
    },
    SlashCommand {
        name: "/usage",
        aliases: &[],
        description: "Show token usage for this session",
        args: None,
        category: "Context",
    },
    SlashCommand {
        name: "/memories",
        aliases: &["/memory", "/mem"],
        description: "Browse and manage memory",
        args: Some("[clear]"),
        category: "Context",
    },
    SlashCommand {
        name: "/remember",
        aliases: &[],
        description: "Save a note to memory",
        args: Some("<text>"),
        category: "Context",
    },
    SlashCommand {
        name: "/flush",
        aliases: &[],
        description: "Wait for memory capture of finished turns",
        args: None,
        category: "Context",
    },
    SlashCommand {
        name: "/skills",
        aliases: &["/skill"],
        description: "List or inject a skill",
        args: Some("[name]"),
        category: "Tools",
    },
    SlashCommand {
        name: "/personas",
        aliases: &["/agents"],
        description: "Agent profiles from .cachy/agents and .claude/agents",
        args: Some("[name]"),
        category: "Tools",
    },
    SlashCommand {
        name: "/plugins",
        aliases: &["/plugin", "/marketplace"],
        description: "Manage plugins (list, reload, trust)",
        args: Some("[list|reload|trust]"),
        category: "Tools",
    },
    SlashCommand {
        name: "/mcp",
        aliases: &[],
        description: "Show MCP servers and their tools",
        args: Some("[list|reload]"),
        category: "Tools",
    },
    SlashCommand {
        name: "/hooks-list",
        aliases: &[],
        description: "Show hooks loaded in this session",
        args: None,
        category: "Tools",
    },
    SlashCommand {
        name: "/hooks-trust",
        aliases: &[],
        description: "Trust this folder for hooks",
        args: None,
        category: "Tools",
    },
    SlashCommand {
        name: "/todos",
        aliases: &["/plan", "/view-plan"],
        description: "Toggle the todo panel",
        args: None,
        category: "Panels",
    },
    SlashCommand {
        name: "/always-approve",
        aliases: &["/yolo", "/auto", "/approve"],
        description: "Toggle always-approve mode",
        args: Some("[on|off]"),
        category: "Model & Input",
    },
    SlashCommand {
        name: "/multiline",
        aliases: &["/ml"],
        description: "Toggle multiline input (swap Enter and Shift+Enter)",
        args: None,
        category: "Model & Input",
    },
    SlashCommand {
        name: "/permissions",
        aliases: &[],
        description: "Show the active permission mode and rules",
        args: None,
        category: "Model & Input",
    },
    SlashCommand {
        name: "/mode",
        aliases: &[],
        description: "Switch mode: normal, plan, always-approve or auto-review",
        args: Some("[normal|plan|auto|review]"),
        category: "Model & Input",
    },
    SlashCommand {
        name: "/palette",
        aliases: &["/commands", "/cmd"],
        description: "Open the command palette",
        args: None,
        category: "Appearance",
    },
    SlashCommand {
        name: "/density",
        aliases: &[],
        description: "How much of each tool call is shown",
        args: Some("[compact|normal|verbose]"),
        category: "Appearance",
    },
    SlashCommand {
        name: "/timestamps",
        aliases: &[],
        description: "Toggle message timestamps",
        args: Some("[on|off]"),
        category: "Appearance",
    },
    SlashCommand {
        name: "/copy",
        aliases: &[],
        description: "Copy the last answer to the clipboard",
        args: None,
        category: "Session",
    },
    SlashCommand {
        name: "/effort",
        aliases: &[],
        description: "Reasoning effort for the current model",
        args: Some("[low|medium|high|none]"),
        category: "Model & Input",
    },
    SlashCommand {
        name: "/bench",
        aliases: &["/speed"],
        description: "Time each model and show the fastest ones",
        args: None,
        category: "Model & Input",
    },
    SlashCommand {
        name: "/doctor",
        aliases: &[],
        description: "Check terminal, configuration and provider support",
        args: None,
        category: "Project",
    },
    SlashCommand {
        name: "/privacy",
        aliases: &[],
        description: "What leaves this machine (nothing by default)",
        args: None,
        category: "Project",
    },
    SlashCommand {
        name: "/toggle-mouse-reporting",
        aliases: &["/mouse"],
        description: "Let the terminal select text with the mouse again",
        args: None,
        category: "Appearance",
    },
    SlashCommand {
        name: "/theme",
        aliases: &[],
        description: "Change the theme",
        args: Some("[name]"),
        category: "Appearance",
    },
    SlashCommand {
        name: "/settings",
        aliases: &["/config", "/preferences", "/prefs", "/configure"],
        description: "Open the settings modal",
        args: None,
        category: "Appearance",
    },
    SlashCommand {
        name: "/shortcuts",
        aliases: &["/keys", "/keybindings"],
        description: "Show keyboard shortcuts",
        args: None,
        category: "Appearance",
    },
    SlashCommand {
        name: "/init",
        aliases: &[],
        description: "Create an AGENTS.md for this project",
        args: None,
        category: "Project",
    },
    SlashCommand {
        name: "/inspect",
        aliases: &[],
        description: "Show discovered configuration for this directory",
        args: None,
        category: "Project",
    },
    SlashCommand {
        name: "/help",
        aliases: &["/?", "/commands"],
        description: "Show help",
        args: None,
        category: "Project",
    },
    SlashCommand {
        name: "/exit",
        aliases: &["/quit", "/q", "/logout"],
        description: "Exit the TUI",
        args: None,
        category: "Project",
    },
];

/// Filter commands by a prefix; matches name and aliases, ranks prefix hits.
pub fn filter(prefix: &str) -> Vec<&'static SlashCommand> {
    let query = prefix.trim_start_matches('/').to_lowercase();
    if query.is_empty() {
        return COMMANDS.iter().collect();
    }
    let mut scored: Vec<(i32, &'static SlashCommand)> = Vec::new();
    for cmd in COMMANDS {
        let name = cmd.name.trim_start_matches('/').to_lowercase();
        let mut best: Option<i32> = None;
        // Equal scores keep the registry order (stable sort), so the most
        // common command wins an ambiguous prefix such as `/mo`.
        let mut consider = |candidate: &str, weight: i32| {
            // `weight` is 0 for the real name and negative for an alias, so a
            // command never loses to another one's alias.
            let score = if candidate == query {
                1000 + weight
            } else if candidate.starts_with(&query) {
                500 + weight
            } else if candidate.contains(&query) {
                300 + weight
            } else {
                return;
            };
            best = Some(best.map(|b: i32| b.max(score)).unwrap_or(score));
        };
        consider(&name, 0);
        for alias in cmd.aliases {
            consider(alias.trim_start_matches('/'), -1);
        }
        if let Some(score) = best {
            scored.push((score, cmd));
        }
    }
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    scored.into_iter().map(|(_, c)| c).collect()
}

/// Fixed options accepted after `/name ` — the menu shows these while typing
/// the argument (`/mode p` → plan). Dynamic ones (models, themes, providers,
/// sessions) are built by the interface.
pub const ARG_OPTIONS: &[(&str, &[&str])] = &[
    ("/mode", &["normal", "plan", "auto", "review"]),
    ("/density", &["compact", "normal", "verbose"]),
    ("/timestamps", &["on", "off"]),
    ("/effort", &["low", "medium", "high", "none"]),
    ("/always-approve", &["on", "off"]),
    ("/plugins", &["list", "reload", "trust"]),
    ("/mcp", &["list", "reload"]),
    ("/memories", &["clear"]),
];

/// Options for the command name, if it takes a fixed list.
pub fn fixed_arg_options(name: &str) -> Option<&'static [&'static str]> {
    ARG_OPTIONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, values)| *values)
}

/// Resolve a typed command (with aliases) to its registry entry name.
pub fn resolve(input: &str) -> Option<&'static SlashCommand> {
    let name = input.split_whitespace().next()?.to_lowercase();
    COMMANDS
        .iter()
        .find(|c| c.name == name || c.aliases.iter().any(|a| *a == name))
}

/// Split `/cmd args` into (command, args).
pub fn split_command(input: &str) -> (String, String) {
    let trimmed = input.trim();
    match trimmed.split_once(char::is_whitespace) {
        Some((cmd, rest)) => (cmd.to_lowercase(), rest.trim().to_string()),
        None => (trimmed.to_lowercase(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters() {
        let results = filter("/mo");
        assert_eq!(results[0].name, "/model");
    }

    #[test]
    fn resolves_alias() {
        assert_eq!(resolve("/yolo").unwrap().name, "/always-approve");
        assert_eq!(resolve("/quit").unwrap().name, "/exit");
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    #[test]
    fn remember_matches_itself_first() {
        let list = filter("/remember");
        let names: Vec<&str> = list.iter().map(|c| c.name).collect();
        assert_eq!(names.first().copied(), Some("/remember"), "got {names:?}");
        assert!(
            !names.contains(&"/memories"),
            "/memories does not match \"remember\": {names:?}"
        );
    }

    #[test]
    fn exact_names_win_and_prefixes_come_next() {
        let list = filter("/mo");
        let names: Vec<&str> = list.iter().map(|c| c.name).collect();
        assert!(names.contains(&"/model"), "got {names:?}");
        assert!(names.contains(&"/mode"), "got {names:?}");
        let list = filter("/timestamps");
        assert_eq!(list.first().map(|c| c.name), Some("/timestamps"));
    }
}
