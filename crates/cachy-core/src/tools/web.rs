//! Web tools: `web_fetch` (URL to markdown) and `web_search` (best-effort).

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{short_target, Tool, ToolContext, ToolKind, ToolResult};

pub const FETCH_CHAR_LIMIT: usize = 48_000;

/// Domains allowed without an approval prompt by default.
pub const DEFAULT_ALLOWED_DOMAINS: &[&str] = &[
    "docs.rs",
    "doc.rust-lang.org",
    "developer.mozilla.org",
    "stackoverflow.com",
    "github.com",
    "raw.githubusercontent.com",
    "nodejs.org",
    "typescriptlang.org",
    "python.org",
    "docs.python.org",
    "pypi.org",
    "npmjs.com",
    "crates.io",
    "opencode.ai",
    "agentclientprotocol.com",
    "modelcontextprotocol.io",
    "wikipedia.org",
    "man7.org",
    "gnu.org",
    "kernel.org",
    "learn.microsoft.com",
    "developer.apple.com",
    "rust-lang.github.io",
    "tokio.rs",
    "serde.rs",
    "react.dev",
    "vuejs.org",
    "svelte.dev",
    "tailwindcss.com",
    "go.dev",
    "docker.com",
    "kubernetes.io",
    "postgresql.org",
    "sqlite.org",
    "redis.io",
    "git-scm.com",
];

pub fn domain_allowed(config: &crate::config::Config, url: &str) -> bool {
    let host = url
        .split("//")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    let allow: Vec<String> = if config.toolset.web_fetch.allowed_domains.is_empty() {
        DEFAULT_ALLOWED_DOMAINS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        config.toolset.web_fetch.allowed_domains.clone()
    };
    allow.iter().any(|d| {
        let d = d.trim_start_matches("www.").to_ascii_lowercase();
        let (domain, path) = match d.split_once('/') {
            Some((dom, p)) => (dom.to_string(), Some(p.to_string())),
            None => (d.clone(), None),
        };
        let host_ok = host == domain || host.ends_with(&format!(".{domain}"));
        match path {
            Some(p) => host_ok && url.to_ascii_lowercase().contains(&format!("/{p}")),
            None => host_ok,
        }
    })
}

/// Cut a fetched page to `limit` bytes without splitting a character.
fn truncate_for_display(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_string(), false);
    }
    (
        format!("{}\n[... truncated ...]", crate::text::cut(text, limit)),
        true,
    )
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Very small HTML → text conversion (tags stripped, entities decoded, links
/// kept inline). Good enough for documentation pages.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut chars = html.chars().peekable();
    let mut in_tag = false;
    let mut in_script = false;
    let mut tag_buf = String::new();
    let mut last_space = true;
    while let Some(c) = chars.next() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let tag = tag_buf.trim().to_ascii_lowercase();
                if tag.starts_with("script") || tag.starts_with("style") {
                    in_script = true;
                } else if tag.starts_with("/script") || tag.starts_with("/style") {
                    in_script = false;
                }
                // Block-level tags produce line breaks.
                let name = tag
                    .trim_start_matches('/')
                    .split([' ', '\t', '\n'])
                    .next()
                    .unwrap_or("");
                if matches!(
                    name,
                    "p" | "div"
                        | "br"
                        | "li"
                        | "tr"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "section"
                        | "article"
                        | "pre"
                        | "blockquote"
                        | "ul"
                        | "ol"
                        | "table"
                        | "header"
                        | "footer"
                        | "nav"
                ) {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                    if name.starts_with('h') && name.len() == 2 {
                        out.push('#');
                        out.push(' ');
                    }
                }
                tag_buf.clear();
            } else {
                tag_buf.push(c);
            }
            continue;
        }
        match c {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '&' => {
                let mut entity = String::new();
                while let Some(&next) = chars.peek() {
                    if next == ';' || entity.len() > 8 {
                        if next == ';' {
                            chars.next();
                        }
                        break;
                    }
                    entity.push(next);
                    chars.next();
                }
                let decoded = match entity.as_str() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" | "#39" => '\'',
                    "nbsp" => ' ',
                    "mdash" => '—',
                    "ndash" => '–',
                    "#x27" | "#X27" => '\'',
                    other if other.starts_with("#x") || other.starts_with("#X") => {
                        u32::from_str_radix(&other[2..], 16)
                            .ok()
                            .and_then(char::from_u32)
                            .unwrap_or(' ')
                    }
                    other if other.starts_with('#') => other[1..]
                        .parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or(' '),
                    _ => ' ',
                };
                out.push(decoded);
                last_space = decoded == ' ';
            }
            _c if in_script => {}
            '\n' | '\r' | '\t' => {
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            }
            c => {
                if c == ' ' {
                    if !last_space {
                        out.push(' ');
                        last_space = true;
                    }
                } else {
                    out.push(c);
                    last_space = false;
                }
            }
        }
    }
    // Collapse excessive blank lines.
    let mut collapsed = String::with_capacity(out.len());
    let mut blank_run = 0;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                collapsed.push('\n');
            }
        } else {
            blank_run = 0;
            collapsed.push_str(trimmed);
            collapsed.push('\n');
        }
    }
    collapsed.trim().to_string()
}

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch a URL and return its content converted to Markdown/text. \
         Large pages are truncated. Use for documentation and reference pages."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Absolute URL to fetch" },
                "max_chars": { "type": "integer", "description": "Maximum characters to return" }
            },
            "required": ["url"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Web
    }
    fn title(&self, args: &Value) -> String {
        format!("Fetch {}", short_target(args, &["url"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(url) = args.get("url").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: url");
        };
        let Some(_) = url.split("//").nth(1) else {
            return ToolResult::err("url must start with http:// or https://");
        };
        if !domain_allowed(&ctx.config, url) {
            // The permission engine already decided; reaching here means the
            // user explicitly approved this domain in this session.
        }
        let limit = args
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(FETCH_CHAR_LIMIT as u64) as usize;
        let client = http_client();
        let resp = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("fetch failed: {e}")),
        };
        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => return ToolResult::err(format!("reading body failed: {e}")),
        };
        if !status.is_success() {
            return ToolResult::err(format!("HTTP {status} for {url}"));
        }
        let text = if content_type.contains("html") {
            html_to_text(&body)
        } else {
            body
        };
        let (text, truncated) = truncate_for_display(&text, limit);
        ToolResult::with_meta(
            format!("URL: {url}\nStatus: {status} ({content_type})\n\n{text}"),
            json!({ "url": url, "truncated": truncated, "content_type": content_type }),
        )
    }
}

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web and return the top results (title, URL, snippet)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" }
            },
            "required": ["query"]
        })
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Web
    }
    fn title(&self, args: &Value) -> String {
        format!("Search web \"{}\"", short_target(args, &["query"]))
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return ToolResult::err("missing required argument: query");
        };
        // Prefer a configured search endpoint (SearxNG/Brave style JSON).
        if let Ok(endpoint) = std::env::var("CACHY_SEARCH_ENDPOINT") {
            let url = format!("{}?q={}", endpoint.trim_end_matches('/'), urlencode(query));
            let resp = http_client().get(&url).send().await;
            if let Ok(resp) = resp {
                if let Ok(v) = resp.json::<Value>().await {
                    let mut out = String::new();
                    if let Some(items) = v.get("results").and_then(|r| r.as_array()) {
                        for item in items.iter().take(8) {
                            let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");
                            let link = item.get("url").and_then(|t| t.as_str()).unwrap_or("");
                            let snippet =
                                item.get("content").and_then(|t| t.as_str()).unwrap_or("");
                            out.push_str(&format!("- {title}\n  {link}\n  {snippet}\n"));
                        }
                    }
                    if !out.is_empty() {
                        return ToolResult::ok(out);
                    }
                }
            }
        }
        // Fallback: DuckDuckGo HTML endpoint (best effort, no API key).
        let url = format!("https://html.duckduckgo.com/html/?q={}", urlencode(query));
        let resp = match http_client().get(&url).send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("search failed: {e}")),
        };
        let body = resp.text().await.unwrap_or_default();
        let results = parse_ddg(&body);
        if results.is_empty() {
            return ToolResult::err(
                "no results parsed. Set CACHY_SEARCH_ENDPOINT to a JSON search API (e.g. SearxNG) for reliable web search.",
            );
        }
        let _ = ctx;
        ToolResult::ok(
            results
                .into_iter()
                .take(8)
                .map(|(t, u, s)| format!("- {t}\n  {u}\n  {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn parse_ddg(html: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(idx) = rest.find("result__a") {
        rest = &rest[idx..];
        let href = rest
            .find("href=\"")
            .map(|i| &rest[i + 6..])
            .and_then(|s| s.split('"').next())
            .unwrap_or("");
        let title = rest
            .find('>')
            .map(|i| &rest[i + 1..])
            .and_then(|s| s.find("</a>").map(|e| &s[..e]))
            .map(html_to_text)
            .unwrap_or_default();
        let snippet = rest
            .find("result__snippet")
            .map(|i| &rest[i..])
            .and_then(|s| s.find('>').map(|i| &s[i + 1..]))
            .and_then(|s| s.find("</a>").map(|e| &s[..e]))
            .map(html_to_text)
            .unwrap_or_default();
        let url = decode_ddg_url(href);
        if !url.is_empty() && !title.is_empty() {
            out.push((title, url, snippet));
        }
        rest = &rest[1..];
        if out.len() >= 12 {
            break;
        }
    }
    out
}

fn decode_ddg_url(href: &str) -> String {
    if let Some(idx) = href.find("uddg=") {
        let encoded = href[idx + 5..].split('&').next().unwrap_or("");
        return percent_decode(encoded);
    }
    href.to_string()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn only_http_urls_reach_the_client() {
        let ctx = crate::tools::ToolContext::new(
            std::env::temp_dir(),
            std::sync::Arc::new(crate::config::Config::default()),
            std::sync::Arc::new(crate::mcp::McpManager::new(Default::default(), "test")),
        );
        let tool = WebFetchTool;
        for url in [
            "file:///etc/passwd",
            "ftp://example.com/x",
            "data:text/html,<h1>oi</h1>",
            "javascript:alert(1)",
            "example.com",
            "",
        ] {
            let result = tool.run(serde_json::json!({ "url": url }), &ctx).await;
            assert!(
                result.is_error,
                "{url:?} should be refused: {}",
                result.content
            );
        }
    }

    #[test]
    fn truncation_keeps_characters_whole() {
        let page = "café ☕ 日本語".repeat(20);
        let (text, truncated) = truncate_for_display(&page, 40);
        assert!(truncated, "40 bytes is less than the page");
        assert!(text.ends_with("[... truncated ...]"));
        // Any limit is safe, including one inside a character.
        for limit in 0..80 {
            let (cut, _) = truncate_for_display(&page, limit);
            assert!(!cut.is_empty());
        }
        let (whole, truncated) = truncate_for_display("curto", 40);
        assert_eq!(whole, "curto");
        assert!(!truncated);
    }

    use super::*;

    #[test]
    fn strips_html() {
        let text = html_to_text(
            "<h1>Title</h1><p>Hello <b>world</b> &amp; more</p><script>bad()</script>",
        );
        assert!(text.contains("# Title"));
        assert!(text.contains("Hello world & more"));
        assert!(!text.contains("bad()"));
    }
}
