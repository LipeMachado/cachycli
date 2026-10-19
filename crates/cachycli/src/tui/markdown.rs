//! Markdown rendering for the scrollback: pulldown-cmark for structure,
//! syntect for code highlighting, output as ratatui `Line`s.

use std::sync::OnceLock;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::parsing::SyntaxSet;

use super::theme::Theme;

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn highlight_theme() -> &'static syntect::highlighting::Theme {
    static THEME: OnceLock<syntect::highlighting::Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let set = syntect::highlighting::ThemeSet::load_defaults();
        set.themes["base16-ocean.dark"].clone()
    })
}

/// A rendered block plus its kind (used for spacing decisions).
pub struct RenderedMarkdown {
    pub lines: Vec<Line<'static>>,
}

pub struct MarkdownRenderer<'a> {
    theme: &'a Theme,
}

impl<'a> MarkdownRenderer<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    pub fn render(&self, text: &str, width: usize) -> RenderedMarkdown {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut current: Vec<Span<'static>> = Vec::new();
        let mut in_code = false;
        let mut code_lang = String::new();
        let mut code_buffer = String::new();
        let mut list_stack: Vec<Option<u64>> = Vec::new();
        let mut in_heading = 0usize;
        let mut blockquote = false;
        let mut bold = false;

        let flush = |lines: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>| {
            if !current.is_empty() {
                lines.push(Line::from(std::mem::take(current)));
            }
        };

        let theme = self.theme;
        let width = width.max(20);

        let wrap_span = |span: Span<'static>,
                         prefix: Option<Span<'static>>,
                         lines: &mut Vec<Line<'static>>,
                         current: &mut Vec<Span<'static>>,
                         width: usize| {
            let text = span.content.to_string();
            let style = span.style;
            let mut first = true;
            let prefix_width = prefix
                .as_ref()
                .map(|p| p.content.chars().count())
                .unwrap_or(0);
            for (i, chunk) in wrap_text(&text, width.saturating_sub(prefix_width).max(8))
                .into_iter()
                .enumerate()
            {
                if !first || i > 0 {
                    flush(lines, current);
                }
                if let Some(p) = &prefix {
                    if i == 0 {
                        current.push(p.clone());
                    }
                }
                current.push(Span::styled(chunk, style));
                first = false;
            }
        };

        for event in Parser::new_ext(text, options) {
            match event {
                Event::Start(Tag::Paragraph) => {
                    flush(&mut lines, &mut current);
                }
                Event::End(TagEnd::Paragraph) => {
                    flush(&mut lines, &mut current);
                    lines.push(Line::default());
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    flush(&mut lines, &mut current);
                    in_heading = level as usize;
                }
                Event::End(TagEnd::Heading(_)) => {
                    flush(&mut lines, &mut current);
                    lines.push(Line::default());
                    in_heading = 0;
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    flush(&mut lines, &mut current);
                    blockquote = true;
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    flush(&mut lines, &mut current);
                    blockquote = false;
                    lines.push(Line::default());
                }
                Event::Start(Tag::List(start)) => {
                    flush(&mut lines, &mut current);
                    list_stack.push(start);
                }
                Event::End(TagEnd::List(_)) => {
                    flush(&mut lines, &mut current);
                    list_stack.pop();
                    if list_stack.is_empty() {
                        lines.push(Line::default());
                    }
                }
                Event::Start(Tag::Item) => {
                    flush(&mut lines, &mut current);
                    let depth = list_stack.len().saturating_sub(1);
                    let indent = "  ".repeat(depth);
                    let marker = match list_stack.last().copied().flatten() {
                        Some(n) => format!("{indent}{}. ", n),
                        None => format!("{indent}• "),
                    };
                    current.push(Span::styled(marker, Style::default().fg(theme.text_muted)));
                }
                Event::End(TagEnd::Item) => {
                    flush(&mut lines, &mut current);
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    flush(&mut lines, &mut current);
                    in_code = true;
                    code_buffer.clear();
                    code_lang = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                        _ => String::new(),
                    };
                }
                Event::End(TagEnd::CodeBlock) => {
                    in_code = false;
                    let highlighted = self.highlight_code(&code_buffer, &code_lang, width);
                    lines.extend(highlighted);
                    // Footer: "◇ lang   [Copy]"
                    let mut footer =
                        vec![Span::styled("◇ ", Style::default().fg(theme.text_muted))];
                    if !code_lang.is_empty() {
                        footer.push(Span::styled(
                            code_lang.clone(),
                            Style::default().fg(theme.text_muted),
                        ));
                    }
                    footer.push(Span::styled("   [Copy]", Style::default().fg(theme.accent)));
                    lines.push(Line::from(footer));
                    lines.push(Line::default());
                    code_buffer.clear();
                    code_lang.clear();
                }
                Event::Text(text) => {
                    if in_code {
                        code_buffer.push_str(&text);
                        continue;
                    }
                    let mut style = Style::default().fg(if in_heading > 0 {
                        theme.text_strong
                    } else {
                        theme.text
                    });
                    if bold || in_heading > 0 {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if blockquote {
                        style = style.fg(theme.text_muted);
                    }
                    let prefix = if blockquote {
                        Some(Span::styled("│ ", Style::default().fg(theme.border)))
                    } else {
                        None
                    };
                    wrap_span(
                        Span::styled(text.to_string(), style),
                        prefix,
                        &mut lines,
                        &mut current,
                        width,
                    );
                }
                Event::Code(code) => {
                    let prefix = if blockquote {
                        Some(Span::styled("│ ", Style::default().fg(theme.border)))
                    } else {
                        None
                    };
                    wrap_span(
                        Span::styled(format!("`{code}`"), Style::default().fg(theme.code_string)),
                        prefix,
                        &mut lines,
                        &mut current,
                        width,
                    );
                }
                Event::Start(Tag::Strong) => bold = true,
                Event::End(TagEnd::Strong) => bold = false,
                Event::SoftBreak => {
                    if in_code {
                        code_buffer.push('\n');
                    } else {
                        current.push(Span::raw(" "));
                    }
                }
                Event::HardBreak => flush(&mut lines, &mut current),
                Event::Rule => {
                    flush(&mut lines, &mut current);
                    let rule: String = "─".repeat(width.min(60));
                    lines.push(Line::from(Span::styled(
                        rule,
                        Style::default().fg(theme.border),
                    )));
                    lines.push(Line::default());
                }
                Event::Start(Tag::Link { dest_url, .. }) => {
                    let _ = dest_url;
                }
                Event::End(TagEnd::Link) => {}
                Event::TaskListMarker(checked) => {
                    let mark = if checked { "[x] " } else { "[ ] " };
                    current.push(Span::styled(
                        mark.to_string(),
                        Style::default().fg(theme.text_muted),
                    ));
                }
                _ => {}
            }
        }
        flush(&mut lines, &mut current);
        // Trim trailing empty lines
        while lines.last().map(|l| l.spans.is_empty()).unwrap_or(false) {
            lines.pop();
        }
        RenderedMarkdown { lines }
    }

    fn highlight_code(&self, code: &str, lang: &str, width: usize) -> Vec<Line<'static>> {
        let theme = self.theme;
        let ps = syntax_set();
        let syntax = ps
            .find_syntax_by_token(lang)
            .or_else(|| ps.find_syntax_by_extension(lang))
            .unwrap_or_else(|| ps.find_syntax_plain_text());
        let mut highlighter = syntect::easy::HighlightLines::new(syntax, highlight_theme());
        let mut out = Vec::new();
        for line in code.lines() {
            let mut spans = vec![Span::styled("  ", Style::default().fg(theme.code_text))];
            let regions = highlighter.highlight_line(line, ps).unwrap_or_default();
            let mut column = 2usize;
            for (style, text) in regions {
                if column >= width {
                    break;
                }
                let color = scope_color(&style.foreground, theme);
                let remaining = width - column;
                let taken: String = text.chars().take(remaining).collect();
                let taken_len = taken.chars().count();
                if taken_len > 0 {
                    spans.push(Span::styled(taken, Style::default().fg(color)));
                    column += taken_len;
                }
            }
            out.push(Line::from(spans));
        }
        if out.is_empty() {
            out.push(Line::from(Span::styled(
                "  ",
                Style::default().fg(theme.code_text),
            )));
        }
        out
    }
}

fn scope_color(foreground: &syntect::highlighting::Color, theme: &Theme) -> ratatui::style::Color {
    // Map syntect's palette to the theme's code colors by nearest role.
    let (r, g, b) = (foreground.r, foreground.g, foreground.b);
    // Syntect base16-ocean.dark role colors (approximate classification).
    let ocean_comment = (101, 115, 126);
    let ocean_keyword = (180, 142, 173);
    let ocean_string = (143, 161, 179);
    let ocean_number = (208, 135, 112);
    let ocean_function = (143, 161, 179);
    let distance = |a: (u8, u8, u8), b: (u8, u8, u8)| -> u32 {
        let d = |x: u8, y: u8| (x as i32 - y as i32).unsigned_abs();
        d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
    };
    let candidates = [
        (ocean_comment, theme.code_comment),
        (ocean_keyword, theme.code_keyword),
        (ocean_string, theme.code_string),
        (ocean_number, theme.code_number),
        (ocean_function, theme.code_function),
    ];
    let mut best = theme.code_text;
    let mut best_d = u32::MAX;
    for (probe, color) in candidates {
        let d = distance((r, g, b), probe);
        if d < best_d {
            best_d = d;
            best = color;
        }
    }
    best
}

/// Wrap text into chunks of at most `width` display columns (word-aware).
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthStr;
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_width = 0usize;
        for word in raw_line.split(' ') {
            let word_width = UnicodeWidthStr::width(word);
            let space = if line.is_empty() { 0 } else { 1 };
            if line_width + space + word_width > width && !line.is_empty() {
                out.push(std::mem::take(&mut line));
                line_width = 0;
            }
            if !line.is_empty() {
                line.push(' ');
                line_width += 1;
            }
            if word_width > width {
                // Hard-break very long tokens.
                let mut current = String::new();
                let mut current_width = 0usize;
                for ch in word.chars() {
                    let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if current_width + w > width && !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                        current_width = 0;
                    }
                    current.push(ch);
                    current_width += w;
                }
                line.push_str(&current);
                line_width += current_width;
            } else {
                line.push_str(word);
                line_width += word_width;
            }
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words() {
        let out = wrap_text("hello world this is a test", 10);
        assert_eq!(out, vec!["hello", "world this", "is a test"]);
    }
}
