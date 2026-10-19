//! Permission rules and approval decisions.
//!
//! Rules use `ToolPrefix(pattern)` syntax:
//! `Bash(rm*)`, `Edit(src/**)`, `Write(*)`, `Read(*)`, `Grep(*)`,
//! `WebFetch(domain:docs.rs)`, `MCPTool(server__tool)`. A bare prefix matches
//! every invocation of that kind. Claude Code's `Bash(cmd:*)` form is accepted.

use std::path::Path;

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};

/// Permission modes, matching the CLI surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Default,
    AcceptEdits,
    Auto,
    DontAsk,
    BypassPermissions,
    Plan,
}

impl PermissionMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "default" => Self::Default,
            "acceptEdits" => Self::AcceptEdits,
            "auto" => Self::Auto,
            "dontAsk" => Self::DontAsk,
            "bypassPermissions" => Self::BypassPermissions,
            "plan" => Self::Plan,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::Auto => "auto",
            Self::DontAsk => "dontAsk",
            Self::BypassPermissions => "bypassPermissions",
            Self::Plan => "plan",
        }
    }
    /// True when the mode auto-approves every tool.
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto | Self::BypassPermissions)
    }
}

/// What a tool invocation needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

/// One parsed permission rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pub kind: RuleKind,
    pub matcher: Option<GlobMatcher>,
    /// Exact domain match for `domain:host` web-fetch rules.
    pub domain: Option<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Bash,
    Edit,
    Write,
    Read,
    Grep,
    WebFetch,
    McpTool,
    Task,
    Any,
}

/// A compiled rule set.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    pub allow: Vec<Rule>,
    pub deny: Vec<Rule>,
}

impl RuleSet {
    pub fn parse(allow: &[String], deny: &[String]) -> Self {
        Self {
            allow: allow.iter().filter_map(|r| parse_rule(r)).collect(),
            deny: deny.iter().filter_map(|r| parse_rule(r)).collect(),
        }
    }

    /// Evaluate explicit rules; `None` means "no rule matched".
    pub fn evaluate(&self, kind: RuleKind, subject: &str) -> Option<Decision> {
        if self.deny.iter().any(|r| r.matches(kind, subject)) {
            return Some(Decision::Deny);
        }
        if self.allow.iter().any(|r| r.matches(kind, subject)) {
            return Some(Decision::Allow);
        }
        None
    }
}

impl Rule {
    pub fn matches(&self, kind: RuleKind, subject: &str) -> bool {
        if self.kind != RuleKind::Any && self.kind != kind {
            return false;
        }
        if let Some(domain) = &self.domain {
            let host = subject
                .split("//")
                .nth(1)
                .and_then(|s| s.split('/').next())
                .unwrap_or(subject)
                .trim_start_matches("www.")
                .to_ascii_lowercase();
            return host == *domain || host.ends_with(&format!(".{domain}"));
        }
        match &self.matcher {
            Some(m) => m.is_match(subject) || m.is_match(Path::new(subject)),
            None => true,
        }
    }
}

/// Rules that could not be understood, so a caller can say so instead of
/// silently ignoring a guard the user asked for.
pub fn invalid_rules(allow: &[String], deny: &[String]) -> Vec<String> {
    allow
        .iter()
        .chain(deny.iter())
        .filter(|raw| !raw.trim().is_empty() && parse_rule(raw).is_none())
        .cloned()
        .collect()
}

/// The prefixes a rule may start with, for a helpful message.
pub const RULE_PREFIXES: &str =
    "Bash(pattern), Edit(path), Write(path), Read(path), Grep(pattern), WebFetch(domain), \
     Task(agent), McpTool(name), Any(pattern) — or the tool names themselves";

/// Parse `Prefix(pattern)` (or bare prefix).
pub fn parse_rule(raw: &str) -> Option<Rule> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (prefix, inner) = match raw.split_once('(') {
        Some((p, rest)) => {
            let inner = rest.strip_suffix(')').unwrap_or(rest);
            (p.trim().to_ascii_lowercase(), Some(inner.trim().to_string()))
        }
        None => (raw.to_ascii_lowercase(), None),
    };
    // Both the rule kinds (Claude Code's vocabulary) and this tool's own
    // names, because `--deny run_terminal_cmd(*)` is what people type.
    let kind = match prefix.as_str() {
        "bash" | "shell" | "run_terminal_cmd" => RuleKind::Bash,
        "edit" | "strreplace" | "search_replace" | "notebook" => RuleKind::Edit,
        "write" => RuleKind::Write,
        "read" | "read_file" | "list_dir" => RuleKind::Read,
        "grep" | "glob" => RuleKind::Grep,
        "webfetch" | "fetch" | "web_fetch" | "web_search" => RuleKind::WebFetch,
        "mcptool" | "mcp" => RuleKind::McpTool,
        "task" | "agent" => RuleKind::Task,
        "" => RuleKind::Any,
        _ => return None,
    };
    let mut rule = Rule { kind, matcher: None, domain: None, raw: raw.to_string() };
    if let Some(inner) = inner {
        // Claude Code form `Bash(cmd:*)` == `Bash(cmd*)`.
        let pattern = inner.strip_prefix("cmd:").map(str::to_string).unwrap_or_else(|| inner.clone());
        if kind == RuleKind::WebFetch {
            if let Some(host) = pattern.strip_prefix("domain:") {
                rule.domain = Some(host.trim().to_ascii_lowercase());
                return Some(rule);
            }
        }
        let glob = Glob::new(&pattern).ok()?;
        rule.matcher = Some(glob.compile_matcher());
    }
    Some(rule)
}

/// The tools that can be gated, and the subject a rule is evaluated against.
pub fn rule_kind_for_tool(tool: &str) -> Option<RuleKind> {
    Some(match tool {
        "run_terminal_cmd" | "bash" | "Shell" => RuleKind::Bash,
        "search_replace" | "edit" | "write_file" | "StrReplace" | "EditNotebook" => RuleKind::Edit,
        "write" | "Write" => RuleKind::Write,
        "read_file" | "Read" => RuleKind::Read,
        "grep" | "grep_search" | "list_dir" | "glob" | "Glob" | "Grep" => RuleKind::Grep,
        "web_fetch" | "web_search" | "WebFetch" | "WebSearch" => RuleKind::WebFetch,
        "task" | "spawn_subagent" | "Task" | "Agent" => RuleKind::Task,
        _ => return None,
    })
}

/// Extract the rule-evaluation subject from tool arguments.
pub fn subject_for(tool: &str, arguments: &serde_json::Value) -> String {
    let s = |k: &str| arguments.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match tool {
        "run_terminal_cmd" | "bash" | "Shell" => s("command"),
        "read_file" | "write" | "search_replace" | "list_dir" => {
            let p = s("file_path");
            if p.is_empty() { s("path") } else { p }
        }
        "grep" | "grep_search" | "Grep" => {
            let p = s("path");
            if p.is_empty() { s("pattern") } else { p }
        }
        "web_fetch" | "WebFetch" => s("url"),
        "web_search" => s("query"),
        "task" | "spawn_subagent" => {
            let t = s("description");
            if t.is_empty() { s("prompt") } else { t }
        }
        _ => arguments.to_string(),
    }
}

/// Full permission evaluation for one tool call.
pub struct PermissionEngine {
    pub rules: RuleSet,
    pub mode: PermissionMode,
    pub always_approve: bool,
}

impl PermissionEngine {
    pub fn new(rules: RuleSet, mode: PermissionMode, always_approve: bool) -> Self {
        Self { rules, mode, always_approve }
    }

    /// Decide whether a tool call may run, must ask, or is denied.
    pub fn decide(&self, tool: &str, arguments: &serde_json::Value) -> Decision {
        let kind = rule_kind_for_tool(tool);
        let subject = subject_for(tool, arguments);
        if let Some(kind) = kind {
            if let Some(d) = self.rules.evaluate(kind, &subject) {
                return d;
            }
        }
        if self.always_approve || self.mode.is_auto() {
            return Decision::Allow;
        }
        match self.mode {
            PermissionMode::Plan => match tool {
                "read_file" | "grep" | "grep_search" | "list_dir" | "glob" | "Glob" | "Grep" | "todo_write"
                | "web_search" | "web_fetch" | "task" | "search_tool" | "use_tool" | "get_task_output" | "wait_tasks" => {
                    Decision::Allow
                }
                _ => Decision::Deny,
            },
            PermissionMode::AcceptEdits => match tool {
                "write" | "write_file" | "search_replace" | "StrReplace" | "edit" => Decision::Allow,
                "read_file" | "grep" | "grep_search" | "list_dir" | "glob" | "todo_write" | "task"
                | "get_task_output" | "wait_tasks" | "search_tool" | "use_tool" => Decision::Allow,
                _ => Decision::Ask,
            },
            PermissionMode::DontAsk => match tool {
                "read_file" | "grep" | "grep_search" | "list_dir" | "glob" | "todo_write" | "task"
                | "get_task_output" | "wait_tasks" | "search_tool" | "use_tool" => Decision::Allow,
                _ => Decision::Deny,
            },
            _ => match tool {
                // Read-only operations are always safe.
                "read_file" | "grep" | "grep_search" | "list_dir" | "glob" | "Glob" | "Grep" | "todo_write"
                | "get_task_output" | "wait_tasks" | "search_tool" | "use_tool" | "task" => Decision::Allow,
                _ => Decision::Ask,
            },
        }
    }
}

/// Options shown in the approval prompt (mirrors the four-way prompt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalChoice {
    AllowAlways,
    AllowSession,
    Allow,
    Reject,
}

impl ApprovalChoice {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AllowAlways => "Yes, and don't ask again for anything (always-approve mode)",
            Self::AllowSession => "Yes, allow all edits during this session",
            Self::Allow => "Yes",
            Self::Reject => "No, reject (type to add feedback)",
        }
    }
    pub const ALL: [ApprovalChoice; 4] = [
        ApprovalChoice::AllowAlways,
        ApprovalChoice::AllowSession,
        ApprovalChoice::Allow,
        ApprovalChoice::Reject,
    ];
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_names_and_rule_kinds_both_parse() {
        for raw in [
            "Bash(rm *)",
            "run_terminal_cmd(git *)",
            "Write(src/**)",
            "read_file(secrets/**)",
            "search_replace(*)",
            "web_search(example.com)",
        ] {
            assert!(parse_rule(raw).is_some(), "{raw} should parse");
        }
        // Typos are reported, not ignored.
        assert!(parse_rule("Bahs(*)").is_none());
        let bad = invalid_rules(&["Bash(ok)".into()], &["Bahs(*)".into(), "  ".into()]);
        assert_eq!(bad, vec!["Bahs(*)".to_string()]);
    }

    use super::*;

    #[test]
    fn deny_wins_over_allow() {
        let rs = RuleSet::parse(&["Bash(rm*)".into()], &["Bash(rm -rf*)".into()]);
        assert_eq!(rs.evaluate(RuleKind::Bash, "rm -rf /"), Some(Decision::Deny));
        assert_eq!(rs.evaluate(RuleKind::Bash, "rm tmp.txt"), Some(Decision::Allow));
        assert_eq!(rs.evaluate(RuleKind::Bash, "ls"), None);
    }

    #[test]
    fn web_fetch_domain_rule() {
        let rs = RuleSet::parse(&["WebFetch(domain:docs.rs)".into()], &[]);
        assert_eq!(rs.evaluate(RuleKind::WebFetch, "https://docs.rs/tokio"), Some(Decision::Allow));
        assert_eq!(rs.evaluate(RuleKind::WebFetch, "https://evil.com"), None);
    }

    #[test]
    fn claude_code_form() {
        let r = parse_rule("Bash(cmd:git *)").unwrap();
        assert!(r.matches(RuleKind::Bash, "git status"));
    }
}
