//! Rendering: header, scrollback, prompt box, todo panel, modals.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::app::{App, HitAction, Item, Modal, NoticeKind};
use super::markdown::MarkdownRenderer;
use super::theme::{Theme, LOGO};

/// Render the whole UI.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.clear_hits();
    app.menu_rect = Rect::default();
    // Paint the background.
    frame.render_widget(
        Block::default().style(Style::default().bg(app.theme.bg).fg(app.theme.text)),
        area,
    );

    let header_height = 2u16;
    let editor_lines = app.editor.display_lines().len().clamp(1, 8) as u16;
    let input_lines = if app.approval.is_some() {
        7
    } else {
        editor_lines + 2
    };
    let menu_height = if app.slash_menu_open {
        (app.menu_len().min(8) as u16 + 2).min(area.height / 2)
    } else if app.file_picker_open {
        (app.file_candidates.len().min(8) as u16 + 2).min(area.height / 2)
    } else {
        0
    };
    let hint_height = 2u16;

    let status_rows = if app.busy { 1 } else { 0 };
    let constraints = vec![
        Constraint::Length(header_height),
        Constraint::Min(6),
        Constraint::Length(menu_height),
        Constraint::Length(status_rows),
        Constraint::Length(input_lines),
        Constraint::Length(hint_height),
    ];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    draw_header(frame, app, chunks[0]);
    draw_scrollback(frame, app, chunks[1]);
    if menu_height > 0 {
        if app.slash_menu_open {
            draw_slash_menu(frame, app, chunks[2]);
        } else {
            draw_file_picker(frame, app, chunks[2]);
        }
    }
    if status_rows > 0 {
        draw_status_strip(frame, app, chunks[3]);
    }
    if let Some(approval) = &app.approval {
        let count = approval.options.len();
        let base = chunks[4];
        let rects: Vec<Rect> = (0..count)
            .map(|index| Rect {
                x: base.x,
                y: base.y + 1 + index as u16,
                width: base.width,
                height: 1,
            })
            .filter(|rect| rect.y < base.y + base.height)
            .collect();
        for (index, rect) in rects.into_iter().enumerate() {
            app.register_hit(rect, HitAction::ApprovalRow(index));
        }
    }

    let (prompt_area, hint_area) = if app.approval.is_some() {
        draw_approval(frame, app, chunks[4], chunks[5]);
        (chunks[4], chunks[5])
    } else {
        draw_prompt(frame, app, chunks[4]);
        draw_hint_line(frame, app, chunks[5]);
        (chunks[4], chunks[5])
    };
    let _ = (prompt_area, hint_area);

    if app.has_modal() {
        if let Some(modal) = app.take_modal_for_render() {
            draw_modal(frame, app, &modal, area);
            app.push_modal(modal);
        }
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let path = super::short_path(&app.cwd);
    let mut left = vec![
        Span::raw("  "),
        Span::styled(path, Style::default().fg(theme.text_dim)),
    ];
    if app.backend_label != "native" {
        left.push(Span::styled(
            format!("  [{}]", app.backend_label),
            Style::default().fg(theme.accent),
        ));
    }
    // The session goal rides along the header so it is never forgotten.
    if let Some(goal) = &app.goal {
        let first = goal.lines().next().unwrap_or("");
        let shown: String = first.chars().take(48).collect();
        left.push(Span::styled(
            format!("  ◎ {shown}"),
            Style::default().fg(theme.warning),
        ));
    }
    let used = human_tokens(app.context_used);
    let size = human_tokens(app.context_size);
    let right_text = format!("{used} / {size} │ [Dashboard] ");
    let line = Line::from(left);
    frame.render_widget(Paragraph::new(line), area);
    let right = Paragraph::new(Line::from(Span::styled(
        right_text,
        Style::default().fg(theme.text_muted),
    )))
    .alignment(Alignment::Right);
    frame.render_widget(right, area);
}

/// Build all scrollback lines and remember the totals for scrolling.
fn scrollback_lines(app: &mut App, width: u16) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let content_width = width.saturating_sub(6).max(24) as usize;

    // The welcome card is shown until the first item arrives.
    if matches!(app.items.first(), Some(Item::Welcome)) {
        out.extend(draw_welcome_lines(app, width));
    }

    app.item_offsets.clear();
    for item in app.items.iter_mut() {
        app.item_offsets.push(out.len());
        match item {
            Item::Welcome => {}
            Item::User { text, ts } => {
                out.push(Line::default());
                let mut spans = vec![
                    Span::raw("     "),
                    Span::styled("❯ ", Style::default().fg(app.theme.text_strong)),
                ];
                let display: String = text.replace('\n', " ⏎ ");
                spans.push(Span::styled(
                    display,
                    Style::default().fg(app.theme.text_strong),
                ));
                let used: usize = spans.iter().map(|s| s.content.width()).sum();
                let ts_width = if app.ui_timestamps { ts.width() } else { 0 };
                if app.ui_timestamps && (used + ts_width + 2) < width as usize {
                    let pad = width as usize - used - ts_width - 1;
                    spans.push(Span::raw(" ".repeat(pad)));
                    spans.push(Span::styled(
                        ts.clone(),
                        Style::default().fg(app.theme.text_muted),
                    ));
                }
                out.push(Line::from(spans));
                out.push(Line::default());
            }
            Item::AssistantText { text, cache } => {
                let renderer = MarkdownRenderer::new(&app.theme);
                if !cache.is_valid(width, &app.theme) {
                    let rendered = renderer.render(text, content_width);
                    let indented = rendered
                        .lines
                        .into_iter()
                        .map(|line| indent_line(line, 5))
                        .collect();
                    cache.store(width, &app.theme, indented);
                }
                out.extend(cache.lines.iter().cloned());
                out.push(Line::default());
            }
            Item::Reasoning {
                text,
                expanded,
                cache,
                seconds,
            } => {
                let line_count = text.lines().count();
                let label = match seconds {
                    Some(seconds) => format!("Thought for {}", format_short_secs(*seconds)),
                    None => format!(
                        "Thinking ({} line{})",
                        line_count,
                        if line_count == 1 { "" } else { "s" }
                    ),
                };
                let mut header = vec![
                    Span::raw("     "),
                    Span::styled("◆ ", Style::default().fg(app.theme.text_muted)),
                    Span::styled(
                        format!("{label}{}", if *expanded { "  ▾" } else { "  ▸" }),
                        Style::default().fg(app.theme.text_muted),
                    ),
                ];
                out.push(Line::from(std::mem::take(&mut header)));
                if *expanded {
                    let renderer = MarkdownRenderer::new(&app.theme);
                    if !cache.is_valid(width, &app.theme) {
                        let dimmed = renderer
                            .render(text, content_width)
                            .lines
                            .into_iter()
                            .map(|line| dim_line(line, &app.theme))
                            .map(|line| indent_line(line, 7))
                            .collect();
                        cache.store(width, &app.theme, dimmed);
                    }
                    out.extend(cache.lines.iter().cloned());
                } else {
                    // Show the first two lines dimmed as a preview.
                    for line in text.lines().take(2) {
                        out.push(Line::from(vec![
                            Span::raw("       "),
                            Span::styled(
                                line.chars().take(content_width).collect::<String>(),
                                Style::default().fg(app.theme.text_dim),
                            ),
                        ]));
                    }
                }
                out.push(Line::default());
            }
            Item::ToolCall(call) => {
                // Density decides how much of a finished call is shown.
                let summary_lines = match app.ui_tool_density.as_str() {
                    "verbose" => 20,
                    "normal" => 6,
                    _ => 0,
                };
                let status_icon = match call.status.as_str() {
                    "completed" => "◈",
                    "failed" => "✕",
                    "in_progress" | "running" | "pending" => "◆",
                    _ => "◇",
                };
                // Colour by tool family, the way the reference does.
                let tint = match call.kind.as_str() {
                    "bash" => app.theme.accent,
                    "edit" | "write" => app.theme.code_type,
                    "read" | "search" => app.theme.success,
                    "web" => app.theme.code_function,
                    _ => app.theme.accent,
                };
                let mut spans = vec![
                    Span::raw("     "),
                    Span::styled(
                        format!("{status_icon} "),
                        Style::default().fg(match call.status.as_str() {
                            "failed" => app.theme.error,
                            "completed" => app.theme.success,
                            _ => tint,
                        }),
                    ),
                    Span::styled(
                        if call.title.is_empty() {
                            call.name.clone()
                        } else {
                            call.title.clone()
                        },
                        Style::default().fg(app.theme.text_strong),
                    ),
                ];
                let elapsed = call
                    .elapsed
                    .unwrap_or_else(|| call.started.elapsed().as_secs_f64());
                if call.status == "in_progress" || call.status == "running" {
                    spans.push(Span::styled(
                        format!("  {:.1}s", elapsed),
                        Style::default().fg(app.theme.text_muted),
                    ));
                } else if let Some(meta) = &call.meta {
                    if let (Some(added), Some(removed)) = (
                        meta.get("added").and_then(|v| v.as_i64()),
                        meta.get("removed").and_then(|v| v.as_i64()),
                    ) {
                        spans.push(Span::styled(
                            format!(" +{added}/-{removed}"),
                            Style::default().fg(app.theme.text_muted),
                        ));
                    }
                }
                out.push(Line::from(spans));
                // While a call is running, show what it is doing.
                if call.status == "in_progress" || call.status == "running" {
                    let preview = tool_preview(&call.arguments);
                    if !preview.is_empty() {
                        out.push(Line::from(vec![
                            Span::raw("       "),
                            Span::styled(
                                truncate_display(&preview, content_width),
                                Style::default().fg(app.theme.text_dim),
                            ),
                        ]));
                    }
                }
                if let Some(summary) = &call.summary {
                    if call.status != "in_progress" && summary_lines > 0 {
                        for line in summary.lines().take(summary_lines) {
                            out.push(Line::from(vec![
                                Span::raw("       "),
                                Span::styled(
                                    truncate_display(line, content_width),
                                    Style::default().fg(app.theme.text_dim),
                                ),
                            ]));
                        }
                    }
                }
                out.push(Line::default());
            }
            Item::Diff { path, diff, cache } => {
                out.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled("◆ ", Style::default().fg(app.theme.text_muted)),
                    Span::styled(
                        format!("Edit {path}"),
                        Style::default().fg(app.theme.text_strong),
                    ),
                ]));
                if !cache.is_valid(width, &app.theme) {
                    let mut lines = Vec::new();
                    for line in diff.lines() {
                        let style = if line.starts_with('+') {
                            Style::default().fg(app.theme.diff_added)
                        } else if line.starts_with('-') {
                            Style::default().fg(app.theme.diff_removed)
                        } else if line.starts_with("@@") {
                            Style::default().fg(app.theme.code_function)
                        } else {
                            Style::default().fg(app.theme.text_dim)
                        };
                        lines.push(Line::from(vec![
                            Span::raw("       "),
                            Span::styled(truncate_display(line, content_width), style),
                        ]));
                    }
                    cache.store(width, &app.theme, lines);
                }
                out.extend(cache.lines.iter().cloned());
                out.push(Line::default());
            }
            Item::Todos { todos, cache } => {
                out.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled("◆ Plan", Style::default().fg(app.theme.text_strong)),
                ]));
                if !cache.is_valid(width, &app.theme) {
                    let mut lines = Vec::new();
                    for todo in todos {
                        let (mark, style) = match todo.status.as_str() {
                            "completed" => (
                                "[x]",
                                Style::default()
                                    .fg(app.theme.text_muted)
                                    .add_modifier(Modifier::CROSSED_OUT),
                            ),
                            "in_progress" => ("[>]", Style::default().fg(app.theme.accent)),
                            "cancelled" => ("[-]", Style::default().fg(app.theme.text_muted)),
                            _ => ("[ ]", Style::default().fg(app.theme.text)),
                        };
                        lines.push(Line::from(vec![
                            Span::raw("       "),
                            Span::styled(format!("{mark} "), style),
                            Span::styled(todo.content.clone(), style),
                        ]));
                    }
                    cache.store(width, &app.theme, lines);
                }
                out.extend(cache.lines.iter().cloned());
                out.push(Line::default());
            }
            Item::Notice { text, kind } => {
                let color = match kind {
                    NoticeKind::Info => app.theme.text_dim,
                    NoticeKind::Warning => app.theme.warning,
                    NoticeKind::Error => app.theme.error,
                };
                for line in text.lines() {
                    for wrapped in super::markdown::wrap_text(line, content_width) {
                        out.push(Line::from(vec![
                            Span::raw("     "),
                            Span::styled(wrapped, Style::default().fg(color)),
                        ]));
                    }
                }
                out.push(Line::default());
            }
            Item::TurnSummary { text } => {
                out.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(text.clone(), Style::default().fg(app.theme.text_muted)),
                ]));
                out.push(Line::default());
            }
            Item::Error { text } => {
                for line in text.lines() {
                    out.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled(
                            truncate_display(line, content_width),
                            Style::default().fg(app.theme.error),
                        ),
                    ]));
                }
                out.push(Line::default());
            }
        }
    }

    // Live thinking, so a slow model still shows something moving.
    if app.ui_thinking {
        if let Some(reasoning) = &app.streaming_reasoning {
            let trimmed = reasoning.trim();
            if !trimmed.is_empty() {
                let lines: Vec<&str> = trimmed.lines().collect();
                let shown = lines.len().min(6);
                out.push(Line::default());
                out.push(indent_line(
                    Line::from(vec![
                        Span::styled("◆ ", Style::default().fg(app.theme.text_muted)),
                        Span::styled(
                            format!("Thinking… · {} lines", lines.len()),
                            Style::default().fg(app.theme.text_muted),
                        ),
                    ]),
                    5,
                ));
                let width = (app.ui_thoughts_width as usize).min(content_width);
                for line in &lines[lines.len() - shown..] {
                    out.push(indent_line(
                        Line::from(Span::styled(
                            truncate_display(line, width),
                            Style::default().fg(app.theme.text_muted),
                        )),
                        5,
                    ));
                }
            }
        }
    }

    // Live streaming text (not yet a completed message).
    if let Some(text) = &app.streaming_text {
        if !text.trim().is_empty() {
            let renderer = MarkdownRenderer::new(&app.theme);
            let rendered = renderer.render(&text.clone(), content_width);
            for line in rendered.lines {
                out.push(indent_line(line, 5));
            }
        }
    }

    app.total_lines = out.len();
    out
}

fn draw_welcome_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::default());
    out.push(Line::default());

    let card_width = ((width as usize).saturating_sub(6)).clamp(60, 116);
    let inner_width = card_width.saturating_sub(4);
    let left_margin = "   ";

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Logo + title rows.
    let title = format!("{} {}  ", cachy_core::APP_TITLE, cachy_core::VERSION);
    let tagline = "A coding agent for your terminal.";
    let connect = "Drives opencode, ACP agents and MCP servers.";
    for (i, logo) in LOGO.iter().enumerate() {
        let logo = *logo;
        let logo_width = LOGO.iter().map(|row| row.width()).max().unwrap_or(0);
        let right_text = match i {
            1 => title.clone(),
            3 => tagline.to_string(),
            5 => connect.to_string(),
            _ => String::new(),
        };
        let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
        spans.push(Span::styled(
            pad_right(logo, logo_width),
            Style::default().fg(theme.text_muted),
        ));
        spans.push(Span::raw("   "));
        if i == 1 {
            spans.push(Span::styled(
                right_text.clone(),
                Style::default()
                    .fg(theme.text_strong)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if !right_text.is_empty() {
            spans.push(Span::styled(
                right_text,
                Style::default().fg(theme.text_muted),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::default());
    // Action hints.
    let actions: [(&str, &str); 4] = [
        ("Resume session", "ctrl+r"),
        ("Sessions dashboard", "ctrl+p"),
        ("Keyboard shortcuts", "ctrl+x"),
        ("Quit", "ctrl+q"),
    ];
    for (label, key) in actions {
        let used = 2 + label.chars().count() + key.chars().count();
        let pad = inner_width.saturating_sub(used);
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(label.to_string(), Style::default().fg(theme.text_strong)),
            Span::raw(" ".repeat(pad)),
            Span::styled(key.to_string(), Style::default().fg(theme.text_dim)),
        ]));
    }

    // Wrap lines in the card frame. Every interior row is clamped to exactly
    // `inner_width` columns so the borders stay aligned.
    let border = Style::default().fg(theme.border);
    out.push(Line::from(vec![
        Span::raw(left_margin),
        Span::styled(
            format!("╭{}╮", "─".repeat(card_width.saturating_sub(2))),
            border,
        ),
    ]));
    for mut line in lines {
        // Measure by character count (content is ASCII plus single-width
        // box-drawing/braille glyphs in this card).
        let content_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        if content_width > inner_width {
            // Truncate from the end of the longest span until it fits.
            let overflow = content_width - inner_width;
            let mut remaining = overflow;
            while remaining > 0 {
                let Some(last) = line.spans.last_mut() else {
                    break;
                };
                let count = last.content.chars().count();
                let drop = remaining.min(count);
                let keep = count - drop;
                last.content = last.content.chars().take(keep).collect::<String>().into();
                remaining -= drop;
                if last.content.is_empty() {
                    line.spans.pop();
                }
            }
        }
        let content_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        let mut spans = vec![Span::styled("│ ", border)];
        spans.extend(line.spans);
        if content_width < inner_width {
            spans.push(Span::raw(" ".repeat(inner_width - content_width)));
        }
        spans.push(Span::styled(" │", border));
        let mut row = vec![Span::raw(left_margin)];
        row.extend(spans);
        out.push(Line::from(row));
    }
    out.push(Line::from(vec![
        Span::raw(left_margin),
        Span::styled(
            format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))),
            border,
        ),
    ]));

    out.push(Line::default());
    out.push(Line::default());
    // Privacy note (honest equivalent of the reference banner).
    out.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Local by default", Style::default().fg(theme.text_strong)),
        Span::styled(
            " — no telemetry. Prompts go only to the model endpoints you configure.",
            Style::default().fg(theme.text_dim),
        ),
    ]));
    out.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Connect opencode", Style::default().fg(theme.accent)),
        Span::styled(
            " to connect opencode, Claude Code or your own models.",
            Style::default().fg(theme.text_dim),
        ),
    ]));
    out
}

fn draw_scrollback(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let lines = scrollback_lines(app, area.width);
    app.viewport_lines = area.height as usize;
    let total = lines.len();
    let max_offset = total.saturating_sub(area.height as usize);
    let offset = app.scroll_from_bottom.min(max_offset);
    let visible_end = total.saturating_sub(offset);
    let visible_start = visible_end.saturating_sub(area.height as usize);
    let visible: Vec<Line<'static>> = lines[visible_start..visible_end].to_vec();
    app.scrollback_rect = area;
    app.scrollback_text = visible
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    let paragraph = Paragraph::new(Text::from(visible))
        .style(Style::default().bg(app.theme.bg))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);

    // Drag selection highlight, painted over the rendered rows.
    if let Some(selection) = app.selection {
        let (start, end) = selection.normalized();
        let theme = &app.theme;
        let right = area.x + area.width.saturating_sub(1);
        let bottom = area.y + area.height.saturating_sub(1);
        for row in start.1..=end.1.min(bottom) {
            if row < area.y {
                continue;
            }
            let from = if row == start.1 { start.0 } else { area.x }.max(area.x);
            let to = if row == end.1 { end.0 } else { right }.min(right);
            for col in from..=to {
                let cell = &mut frame.buffer_mut()[(col, row)];
                cell.set_bg(theme.surface_strong);
                cell.set_fg(theme.text_strong);
            }
        }
    }

    // Scrollbar on the right edge when scrolled.
    if max_offset > 0 {
        let bar_x = area.x + area.width.saturating_sub(1);
        let total_height = area.height as usize;
        let thumb_height = ((total_height * total_height) / total.max(1)).max(1);
        let max_thumb_offset = total_height.saturating_sub(thumb_height);
        let thumb_offset = (max_thumb_offset * offset)
            .checked_div(max_offset)
            .unwrap_or(0);
        for row in 0..total_height {
            let in_thumb = row >= thumb_offset && row < thumb_offset + thumb_height;
            let ch = if in_thumb { "█" } else { "│" };
            let color = if in_thumb {
                app.theme.text_muted
            } else {
                app.theme.border
            };
            frame.buffer_mut()[(bar_x, area.y + row as u16)]
                .set_symbol(ch)
                .set_style(Style::default().fg(color));
        }
    }
}

fn draw_slash_menu(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height < 3 {
        return;
    }
    let theme = &app.theme;
    let border_color = theme.border;
    let matches = app.slash_matches();
    let row_count = app.menu_len();
    let visible = (area.height as usize).saturating_sub(2).min(row_count);
    let selected = app.slash_selected.min(row_count.saturating_sub(1));
    // Scroll the selection into view.
    let start = if selected >= visible {
        selected + 1 - visible
    } else {
        0
    };
    app.menu_rect = area;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let rule_width = area.width as usize;
    // Top rule, with the scroll position at the right end when it matters.
    let position = if row_count > visible {
        format!("{}/{}", selected + 1, row_count)
    } else {
        String::new()
    };
    let mut rule = "─".repeat(rule_width.saturating_sub(position.width() + 1));
    if !position.is_empty() {
        rule.push_str(&position);
        rule.push('─');
    } else {
        rule = "─".repeat(rule_width);
    }
    lines.push(Line::from(Span::styled(
        rule,
        Style::default().fg(theme.border),
    )));
    if !app.arg_options.is_empty() {
        for (i, (value, hint)) in app.arg_options.iter().enumerate().skip(start).take(visible) {
            let is_selected = i == selected;
            let hovered = matches!(app.hover, Some(HitAction::SlashRow(index)) if index == i);
            let bg = if is_selected {
                theme.surface
            } else if hovered {
                theme.hover
            } else {
                theme.bg
            };
            let mut spans = vec![
                Span::raw("  "),
                Span::styled(
                    if is_selected { "❯ " } else { "  " }.to_string(),
                    Style::default().fg(theme.text_strong).bg(bg),
                ),
                Span::styled(value.clone(), Style::default().fg(theme.text_strong).bg(bg)),
            ];
            let used: usize = 4 + value.width();
            if !hint.is_empty() {
                let desc_width = area.width as usize;
                if used + hint.width() + 4 < desc_width {
                    spans.push(Span::raw(" ".repeat(desc_width - used - hint.width() - 3)));
                    spans.push(Span::styled(
                        hint.clone(),
                        Style::default().fg(theme.text_muted).bg(bg),
                    ));
                } else {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        hint.clone(),
                        Style::default().fg(theme.text_muted).bg(bg),
                    ));
                }
            }
            let used: usize = spans.iter().map(|s| s.content.width()).sum();
            if used < area.width as usize {
                spans.push(Span::styled(
                    " ".repeat(area.width as usize - used),
                    Style::default().bg(bg),
                ));
            }
            lines.push(Line::from(spans));
        }
        for (offset, index) in (start..app.arg_options.len()).take(visible).enumerate() {
            let y = area.y + 1 + offset as u16;
            if y < area.y + area.height {
                app.register_hit(
                    Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    },
                    HitAction::SlashRow(index),
                );
            }
        }
        if lines.len() < area.height as usize {
            lines.push(Line::from(Span::styled(
                "─".repeat(area.width as usize),
                Style::default().fg(border_color),
            )));
        }
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
        return;
    }

    for (i, cmd) in matches.iter().enumerate().skip(start).take(visible) {
        let is_selected = i == selected;
        let hovered = matches!(app.hover, Some(HitAction::SlashRow(index)) if index == i);
        let bg = if is_selected {
            theme.surface
        } else if hovered {
            theme.hover
        } else {
            theme.bg
        };
        let mut spans = vec![
            Span::raw("  "),
            Span::styled(
                if is_selected { "❯ " } else { "  " }.to_string(),
                Style::default().fg(theme.text_strong).bg(bg),
            ),
            Span::styled(
                cmd.name.to_string(),
                Style::default().fg(theme.text_strong).bg(bg),
            ),
        ];
        if let Some(args) = cmd.args {
            spans.push(Span::styled(
                format!(" {args}"),
                Style::default().fg(theme.text_muted).bg(bg),
            ));
        }
        let used: usize = 4 + cmd.name.width() + cmd.args.map(|a| a.width() + 1).unwrap_or(0);
        let desc_width = area.width as usize;
        if used + cmd.description.width() + 4 < desc_width {
            let pad = desc_width - used - cmd.description.width() - 3;
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled(
                cmd.description,
                Style::default().fg(theme.text_muted).bg(bg),
            ));
        } else {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                cmd.description,
                Style::default().fg(theme.text_muted).bg(bg),
            ));
        }
        // Pad the row to full width with the background.
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        if used < desc_width {
            spans.push(Span::styled(
                " ".repeat(desc_width - used),
                Style::default().bg(bg),
            ));
        }
        lines.push(Line::from(spans));
    }
    for (offset, cmd_index) in (start..matches.len()).take(visible).enumerate() {
        let y = area.y + 1 + offset as u16;
        if y < area.y + area.height {
            app.register_hit(
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                HitAction::SlashRow(cmd_index),
            );
        }
    }
    // Closing rule, so the dropdown reads as a box like the reference.
    if lines.len() < area.height as usize {
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(border_color),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn draw_file_picker(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height < 3 {
        return;
    }
    let theme = &app.theme;
    let total = app.file_index.len();
    if total == 0 {
        return;
    }
    let visible = (area.height as usize).saturating_sub(2);
    app.menu_rect = area;
    let selected = app.file_selected;
    let start = if selected >= visible {
        selected + 1 - visible
    } else {
        0
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(theme.border),
    )));
    for (i, candidate) in app
        .file_candidates
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
    {
        let is_selected = i == selected;
        let hovered = matches!(app.hover, Some(HitAction::FileRow(index)) if index == i);
        let bg = if is_selected {
            theme.surface
        } else if hovered {
            theme.hover
        } else {
            theme.bg
        };
        let label = truncate_display(candidate, area.width.saturating_sub(6) as usize);
        let mut spans = vec![
            Span::raw("  "),
            Span::styled(
                if is_selected { "❯ " } else { "  " }.to_string(),
                Style::default().fg(theme.text_strong).bg(bg),
            ),
            Span::styled(format!("@{label}"), Style::default().fg(theme.text).bg(bg)),
        ];
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        if used < area.width as usize {
            spans.push(Span::styled(
                " ".repeat(area.width as usize - used),
                Style::default().bg(bg),
            ));
        }
        lines.push(Line::from(spans));
    }
    for (offset, candidate_index) in (start..app.file_candidates.len()).take(visible).enumerate() {
        let y = area.y + 1 + offset as u16;
        if y < area.y + area.height {
            app.register_hit(
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                HitAction::FileRow(candidate_index),
            );
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn draw_prompt(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = &app.theme;
    app.prompt_rect = area;
    // One animation only: the dots live in the running strip above the box.
    let busy_marker = "❯";
    let mut lines: Vec<Line<'static>> = Vec::new();
    let selection = app
        .prompt_selection
        .map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
    // Byte offset of the first character of each display line.
    let mut line_offsets: Vec<usize> = Vec::new();
    {
        let mut offset = 0usize;
        for (_, text) in app.editor.display_lines() {
            line_offsets.push(offset);
            // +1 for the newline that separated the lines
            offset += text.len() + 1;
        }
    }
    for (i, line) in app.editor.display_lines().iter().enumerate() {
        let prefix = if i == 0 {
            format!(" {busy_marker} ")
        } else {
            "   ".to_string()
        };
        let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.text_strong))];
        match selection {
            Some((start, end)) => {
                let line_start = line_offsets.get(i).copied().unwrap_or(0);
                let line_end = line_start + line.1.len();
                // Intersect the selection with this line.
                let from = start.max(line_start).min(line_end) - line_start;
                let to = end.max(line_start).min(line_end) - line_start;
                let (from, to) = (from.min(line.1.len()), to.min(line.1.len()));
                let chars: Vec<char> = line.1.chars().collect();
                let byte_to_char = |byte: usize| line.1[..byte.min(line.1.len())].chars().count();
                let (fc, tc) = (byte_to_char(from), byte_to_char(to));
                if fc > 0 {
                    spans.push(Span::styled(
                        chars[..fc].iter().collect::<String>(),
                        Style::default().fg(theme.text_strong),
                    ));
                }
                if tc > fc {
                    spans.push(Span::styled(
                        chars[fc..tc].iter().collect::<String>(),
                        Style::default()
                            .fg(theme.text_strong)
                            .bg(theme.surface_strong),
                    ));
                }
                if tc < chars.len() {
                    spans.push(Span::styled(
                        chars[tc..].iter().collect::<String>(),
                        Style::default().fg(theme.text_strong),
                    ));
                }
            }
            None => spans.push(Span::styled(
                line.1.clone(),
                Style::default().fg(theme.text_strong),
            )),
        }
        lines.push(Line::from(spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(" {busy_marker} "),
            Style::default().fg(theme.text_strong),
        )]));
    }
    // While completing a command's arguments, show its signature dimmed, like
    // the reference does (`/model <model> [window] [effort]`).
    let argument_empty = app
        .editor
        .text
        .trim()
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest.trim().is_empty())
        .unwrap_or(true);
    if app.slash_menu_open && !app.arg_options.is_empty() && argument_empty {
        if let Some(cmd) = super::slash::resolve(&app.editor.text) {
            if let Some(args) = cmd.args {
                if let Some(last) = lines.last_mut() {
                    let separator = if app.editor.text.ends_with(' ') {
                        ""
                    } else {
                        " "
                    };
                    last.spans.push(Span::styled(
                        format!("{separator}{args}"),
                        Style::default().fg(app.theme.text_muted),
                    ));
                }
            }
        }
    }

    // Bottom-right of the box: the model, plus the mode when one is active.
    let mut badge_parts: Vec<String> = Vec::new();
    if let Some(badge) = app.mode.badge() {
        badge_parts.push(badge.to_string());
    }
    if let Some(effort) = &app.effort {
        badge_parts.push(effort.clone());
    }
    let right_label = if badge_parts.is_empty() {
        format!("{} ", app.model_label)
    } else {
        format!("{} ({}) ", app.model_label, badge_parts.join(" · "))
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focused))
        .style(Style::default().bg(theme.bg))
        .title_bottom(
            Line::from(Span::styled(
                right_label,
                Style::default().fg(theme.text_muted),
            ))
            .alignment(Alignment::Right),
        )
        .padding(Padding::horizontal(0));
    let paragraph = Paragraph::new(Text::from(lines)).block(block);
    frame.render_widget(paragraph, area);

    // Click anywhere in the box to place the caret there.
    let input_rows = app.editor.display_lines().len().clamp(1, 8) as u16;
    for row in 0..input_rows {
        let y = area.y + 1 + row;
        if y >= area.y + area.height {
            break;
        }
        app.register_hit(
            Rect {
                x: area.x + 1,
                y,
                width: area.width.saturating_sub(2),
                height: 1,
            },
            HitAction::PromptCaret {
                row: row as usize,
                origin_x: area.x + 4,
            },
        );
    }

    // Show the terminal's own caret where the text cursor is.
    if !app.has_modal() && app.approval.is_none() {
        let (row, _) = app.editor.cursor_row_col();
        let row = row.min(input_rows.saturating_sub(1) as usize);
        let column = app.editor.cursor_col_display();
        let x = area.x + 4 + column.min(area.width.saturating_sub(6) as usize) as u16;
        let y = area.y + 1 + row as u16;
        frame.set_cursor_position(ratatui::layout::Position::new(x, y));
    }
}

/// Running strip: animated blocks, what it is doing, elapsed time and the
/// clickable [stop] with the token counter on the right.
fn draw_status_strip(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = &app.theme;
    const DOTS: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let block = DOTS[app.spinner_frame % DOTS.len()];
    let elapsed = app
        .turn_started
        .map(|at| at.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let what = match &app.running_tool {
        Some(tool) => format!("Running {}", truncate_display(tool, 48)),
        None => "Waiting for response".to_string(),
    };
    let mut left_spans = vec![
        Span::raw("  "),
        Span::styled(block.to_string(), Style::default().fg(theme.accent)),
        Span::styled(format!(" {what} "), Style::default().fg(theme.text)),
        Span::styled(
            format!("{elapsed:.1}s"),
            Style::default().fg(theme.text_muted),
        ),
    ];
    // Nothing back yet? Say what the options are instead of just counting up.
    if app.first_token_at.is_none() && elapsed > 20.0 {
        left_spans.push(Span::styled(
            "  · no answer yet — Esc cancels, /model switches",
            Style::default().fg(theme.warning),
        ));
    }
    let left = Line::from(left_spans);
    frame.render_widget(Paragraph::new(left), area);

    // Right side: tokens + [stop]
    let tokens = app.stream_tokens;
    let token_text = if tokens > 0 {
        format!("⇣{}  ", human_tokens(tokens))
    } else {
        String::new()
    };
    let stop = "[stop]";
    let width = token_text.width() + stop.width() + 4;
    if area.width as usize > width + 8 {
        let x = area.x + area.width - width as u16;
        let stop_x = x + token_text.width() as u16;
        let style = if matches!(app.hover, Some(super::app::HitAction::StopTurn)) {
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_muted)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(token_text, Style::default().fg(theme.text_dim)),
                Span::styled(stop, style),
                Span::raw(" "),
            ])),
            Rect {
                x,
                y: area.y,
                width: width as u16,
                height: 1,
            },
        );
        app.register_hit(
            Rect {
                x: stop_x,
                y: area.y,
                width: stop.width() as u16,
                height: 1,
            },
            super::app::HitAction::StopTurn,
        );
    }
}

/// `0.6s`, `12s`, `1m20s` — the short form the reference uses.
fn format_short_secs(secs: f64) -> String {
    if secs < 1.0 {
        format!("{:.1}s", secs)
    } else if secs < 60.0 {
        format!("{:.0}s", secs)
    } else {
        let mins = (secs / 60.0).floor();
        format!("{}m{:.0}s", mins, secs - mins * 60.0)
    }
}

fn draw_hint_line(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    // While a quit is armed the confirmation owns the line, so it is readable.
    let armed = app
        .quit_armed
        .map(|at| at.elapsed() < std::time::Duration::from_secs(4))
        .unwrap_or(false);
    if armed {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Press Ctrl+C again to quit",
                    Style::default()
                        .fg(theme.error)
                        .add_modifier(Modifier::BOLD),
                ),
            ])),
            area,
        );
        return;
    }
    let hints: Vec<(&str, &str)> = if app.busy {
        let mut hints: Vec<(&str, &str)> =
            vec![("cancel", "Ctrl+c"), ("cancel", "Esc"), ("queue", "Enter")];
        if !app.queue.is_empty() {
            hints.push(("send now", "Enter"));
        }
        hints
    } else {
        vec![
            ("send", "Enter"),
            ("newline", "Alt+Enter"),
            ("mode", "Shift+Tab"),
            ("shortcuts", "Ctrl+x"),
        ]
    };
    let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
    for (i, (label, key)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  │  ", Style::default().fg(theme.border)));
        }
        spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(theme.text_dim),
        ));
        spans.push(Span::styled(
            format!(":{label}"),
            Style::default().fg(theme.text_muted),
        ));
    }
    if let Some((message, at)) = &app.status_message {
        if at.elapsed().as_secs() < 6 {
            spans.push(Span::styled("    ", Style::default()));
            spans.push(Span::styled(
                message.clone(),
                Style::default().fg(theme.accent),
            ));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_approval(frame: &mut Frame, app: &mut App, prompt_area: Rect, hint_area: Rect) {
    let theme = &app.theme;
    let Some(approval) = &app.approval else {
        return;
    };
    // The approval block replaces the prompt box visually.
    let mut lines: Vec<Line<'static>> = Vec::new();
    let title_line = if approval
        .title
        .to_lowercase()
        .starts_with(&approval.tool.to_lowercase())
        || approval.title.to_lowercase().starts_with("bash ")
    {
        format!("Allow {}?", first_line(&approval.title))
    } else {
        format!(
            "Allow {} to {}?",
            approval.tool,
            first_line(&approval.title)
        )
    };
    lines.push(Line::from(vec![
        Span::styled("  ┃  ", Style::default().fg(theme.accent)),
        Span::styled("›  ", Style::default().fg(theme.text_strong)),
        Span::styled(title_line, Style::default().fg(theme.text_strong)),
    ]));
    for (i, option) in approval.options.iter().enumerate() {
        let selected = i == approval.selected;
        let hovered = matches!(app.hover, Some(HitAction::ApprovalRow(index)) if index == i);
        let row_bg = if hovered { theme.hover } else { theme.bg };
        let marker = if selected { "●" } else { "○" };
        let mut spans = vec![
            Span::styled("  ┃  ", Style::default().fg(theme.accent).bg(row_bg)),
            Span::styled(
                format!("{} ", i + 1),
                Style::default().fg(theme.text_muted).bg(row_bg),
            ),
            Span::styled(
                format!("({marker}) "),
                Style::default()
                    .fg(if selected || hovered {
                        theme.accent
                    } else {
                        theme.text_muted
                    })
                    .bg(row_bg),
            ),
            Span::styled(
                option.clone(),
                Style::default()
                    .fg(if selected || hovered {
                        theme.text_strong
                    } else {
                        theme.text
                    })
                    .bg(row_bg),
            ),
        ];
        if i == 3 && !approval.feedback.is_empty() {
            spans.push(Span::styled(
                format!(": {}", approval.feedback),
                Style::default().fg(theme.text_dim),
            ));
        }
        if !approval.detail.is_empty() && i == 0 {
            // Keep the first row clean; detail lives below the options.
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(Span::styled(
        "  ┃",
        Style::default().fg(theme.accent),
    )));
    if !approval.detail.is_empty() {
        let detail = truncate_display(
            &first_line(&approval.detail),
            prompt_area.width.saturating_sub(8) as usize,
        );
        lines.push(Line::from(vec![
            Span::styled("  ┃  ", Style::default().fg(theme.accent)),
            Span::styled(detail, Style::default().fg(theme.text_dim)),
        ]));
        lines.push(Line::from(Span::styled(
            "  ┃",
            Style::default().fg(theme.accent),
        )));
    }
    let paragraph = Paragraph::new(Text::from(lines));
    frame.render_widget(paragraph, prompt_area);

    let hint = Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(
                "1/{option_count}:select",
                option_count = approval.options.len().max(1)
            ),
            Style::default().fg(theme.text_dim),
        ),
        Span::styled("  │  ", Style::default().fg(theme.border)),
        Span::styled("Tab:next option", Style::default().fg(theme.text_dim)),
        Span::styled("  │  ", Style::default().fg(theme.border)),
        Span::styled("Ctrl+o:always-approve", Style::default().fg(theme.text_dim)),
        Span::styled("  │  ", Style::default().fg(theme.border)),
        Span::styled("Ctrl+c:cancel", Style::default().fg(theme.text_dim)),
        Span::styled("  │  ", Style::default().fg(theme.border)),
        Span::styled("Esc:scrollback", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(Paragraph::new(hint), hint_area);
}

fn draw_modal(frame: &mut Frame, app: &mut App, modal: &Modal, area: Rect) {
    match modal {
        Modal::Quit => draw_quit(frame, app, area),
        Modal::ModelFallback { model, reason } => {
            let lines = vec![
                Line::from(Span::styled(
                    format!("  {reason}."),
                    Style::default().fg(app.theme.text),
                )),
                Line::default(),
                Line::from(Span::styled(
                    "  Switch to a model that needs no key?",
                    Style::default().fg(app.theme.text_dim),
                )),
                Line::from(vec![
                    Span::styled("  ❯ ", Style::default().fg(app.theme.accent)),
                    Span::styled(model.clone(), Style::default().fg(app.theme.text_strong)),
                    Span::styled(
                        "   Enter to switch · Esc to keep",
                        Style::default().fg(app.theme.text_muted),
                    ),
                ]),
                Line::default(),
                Line::from(Span::styled(
                    "  /login does a device login (browser, no key) — or pick GitHub Copilot there. New key: opencode.ai/console → Personal API keys",
                    Style::default().fg(app.theme.text_dim),
                )),
            ];
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Model unavailable",
                String::new(),
                lines,
                "Enter switch  |  Esc keep",
                96,
                11,
            );
            register_modal_hits(app, modal_box, false, 0, HitAction::ModalRow);
        }
        Modal::Palette(state) => {
            // Render every line plus the item index of each list row, so the
            // window can follow the selection through category headers.
            let mut all_lines: Vec<Line<'static>> = Vec::new();
            let mut item_rows: Vec<(usize, usize)> = Vec::new(); // (line, item)
            let mut current_category = String::new();
            for (i, item) in state.items.iter().enumerate() {
                if item.category != current_category {
                    if !current_category.is_empty() {
                        all_lines.push(Line::default());
                    }
                    current_category = item.category.clone();
                    all_lines.push(Line::from(Span::styled(
                        format!("  {}", item.category),
                        Style::default().fg(app.theme.text_muted),
                    )));
                }
                let selected = i == state.selected;
                let hovered = matches!(app.hover, Some(HitAction::ModalRow(index)) if index == i);
                let bg = if selected {
                    app.theme.surface
                } else if hovered {
                    app.theme.hover
                } else {
                    app.theme.bg
                };
                let mut spans = vec![
                    Span::styled(
                        if selected { " ◆ " } else { " · " }.to_string(),
                        Style::default().fg(app.theme.accent).bg(bg),
                    ),
                    Span::styled(
                        item.label.clone(),
                        Style::default().fg(app.theme.text_strong).bg(bg),
                    ),
                ];
                let used: usize = spans.iter().map(|s| s.content.width()).sum();
                if !item.shortcut.is_empty() {
                    // pad to the right edge
                    let target = 74usize.saturating_sub(item.shortcut.width() + 4);
                    if used < target {
                        spans.push(Span::styled(
                            " ".repeat(target - used),
                            Style::default().bg(bg),
                        ));
                    }
                    spans.push(Span::styled(
                        item.shortcut.clone(),
                        Style::default().fg(app.theme.text_dim).bg(bg),
                    ));
                }
                item_rows.push((all_lines.len(), i));
                all_lines.push(Line::from(spans));
            }
            let viewport = modal_viewport(area, 26);
            let selected_line = item_rows
                .iter()
                .find(|(_, item)| *item == state.selected)
                .map(|(line, _)| *line)
                .unwrap_or(0);
            let (start, visible) = App::visible_window(selected_line, all_lines.len(), viewport);
            let window: Vec<Line<'static>> =
                all_lines.into_iter().skip(start).take(visible).collect();
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Commands",
                format!("   search: {}", state.query),
                window,
                "↑/↓ nav  |  Enter select  |  Esc close",
                80,
                26,
            );
            // Map rendered rows back to their item index.
            for (line, item) in &item_rows {
                if *line < start || *line >= start + visible {
                    continue;
                }
                if let Some(rect) = modal_row_rect(modal_box, true, 0, (*line - start) as u16) {
                    app.register_hit(rect, HitAction::ModalRow(*item));
                }
            }
            app.register_hit(
                Rect {
                    x: modal_box.x + modal_box.width.saturating_sub(8),
                    y: modal_box.y,
                    width: 7,
                    height: 1,
                },
                HitAction::ModalClose,
            );
        }
        Modal::Shortcuts(state) => {
            let rows = super::app::shortcut_rows(&state.search, &state.expanded, &app.keys);
            let viewport = modal_viewport(area, 30);
            let (start, visible) = App::visible_window(state.selected, rows.len(), viewport);
            let mut lines: Vec<Line<'static>> = Vec::new();
            for (index, row) in rows.iter().enumerate().skip(start).take(visible) {
                let selected = index == state.selected;
                let hovered = matches!(app.hover, Some(HitAction::ModalRow(hit)) if hit == index);
                let bg = if selected {
                    app.theme.surface
                } else if hovered {
                    app.theme.hover
                } else {
                    app.theme.bg
                };
                if row.label.is_empty() {
                    let open = rows
                        .get(index + 1)
                        .map(|next| next.group == row.group && !next.label.is_empty())
                        .unwrap_or(false);
                    let marker = if open { "▾" } else { "›" };
                    lines.push(Line::from(Span::styled(
                        format!("  {marker} {}", row.group),
                        Style::default()
                            .fg(if selected {
                                app.theme.text_strong
                            } else {
                                app.theme.text
                            })
                            .bg(bg),
                    )));
                    continue;
                }
                let mut spans = vec![
                    Span::raw("      "),
                    Span::styled(
                        format!("◆ {}", row.label),
                        Style::default()
                            .fg(if row.action.is_some() {
                                app.theme.text_strong
                            } else {
                                app.theme.text
                            })
                            .bg(bg),
                    ),
                ];
                let used: usize = 6 + 2 + row.label.width();
                let target = 74usize.saturating_sub(row.keys.width() + 3);
                if used < target {
                    spans.push(Span::styled(
                        " ".repeat(target - used),
                        Style::default().bg(bg),
                    ));
                }
                let key_style = if row.action.is_some() {
                    Style::default().fg(app.theme.accent).bg(bg)
                } else {
                    Style::default().fg(app.theme.text_dim).bg(bg)
                };
                spans.push(Span::styled(row.keys.clone(), key_style));
                lines.push(Line::from(spans));
            }
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Keyboard Shortcuts",
                search_subtitle(state.searching, &state.search, "/ to search"),
                lines,
                "↑/↓ nav  |  Enter rebind  |  d default  |  / search  |  Esc close",
                84,
                30,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Settings(state) => {
            let items = super::app::settings_items_filtered(&state.search, app);
            let viewport = modal_viewport(area, 20);
            let (start, visible) =
                App::visible_window(state.selected, items.len(), viewport.max(1));
            let mut lines: Vec<Line<'static>> = Vec::new();
            for (i, item) in items.iter().enumerate().skip(start).take(visible) {
                let selected = i == state.selected;
                let hovered = matches!(app.hover, Some(HitAction::ModalRow(hit)) if hit == i);
                let bg = if selected {
                    app.theme.surface
                } else if hovered {
                    app.theme.hover
                } else {
                    app.theme.bg
                };
                let mut spans = vec![
                    Span::styled("  ▸ ", Style::default().fg(app.theme.text_muted).bg(bg)),
                    Span::styled(
                        item.label.to_string(),
                        Style::default().fg(app.theme.text_strong).bg(bg),
                    ),
                ];
                let used: usize = spans.iter().map(|s| s.content.width()).sum();
                if !item.value.is_empty() {
                    let target = 74usize.saturating_sub(item.value.width() + 3);
                    if used < target {
                        spans.push(Span::styled(
                            " ".repeat(target - used),
                            Style::default().bg(bg),
                        ));
                    }
                    spans.push(Span::styled(
                        item.value.clone(),
                        Style::default().fg(app.theme.text).bg(bg),
                    ));
                }
                lines.push(Line::from(spans));
            }
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Settings",
                search_subtitle(state.searching, &state.search, "/ to search"),
                lines,
                "↑/↓ nav  |  Enter edit  |  / search  |  F2/Esc close",
                80,
                20,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Resume(state) | Modal::Sessions(state) => {
            if state.items.iter().all(|item| item == "No sessions yet") {
                let modal_box = modal_frame(
                    frame,
                    app,
                    area,
                    "Resume session",
                    format!("   / to search        {}", query_text(&state.query)),
                    vec![Line::from(Span::styled(
                        "    No matches",
                        Style::default().fg(app.theme.text_muted),
                    ))],
                    "↑/↓ nav  |  Enter open  |  Esc close",
                    82,
                    28,
                );
                app.register_hit(
                    Rect {
                        x: modal_box.x + modal_box.width.saturating_sub(8),
                        y: modal_box.y,
                        width: 7,
                        height: 1,
                    },
                    HitAction::ModalClose,
                );
            } else {
                let viewport = modal_viewport(area, 28);
                let (start, visible) =
                    App::visible_window(state.selected, state.items.len(), viewport.max(1));
                let mut lines: Vec<Line<'static>> = Vec::new();
                for (i, item) in state.items.iter().enumerate().skip(start).take(visible) {
                    let selected = i == state.selected;
                    let hovered = matches!(app.hover, Some(HitAction::ModalRow(hit)) if hit == i);
                    let bg = if selected {
                        app.theme.surface
                    } else if hovered {
                        app.theme.hover
                    } else {
                        app.theme.bg
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            if selected { "  ◆ " } else { "  · " }.to_string(),
                            Style::default().fg(app.theme.accent).bg(bg),
                        ),
                        Span::styled(item.clone(), Style::default().fg(app.theme.text).bg(bg)),
                    ]));
                }
                let modal_box = modal_frame(
                    frame,
                    app,
                    area,
                    "Resume session",
                    format!("   / to search        {}", query_text(&state.query)),
                    lines,
                    "↑/↓ nav  |  Enter open  |  Esc close",
                    82,
                    28,
                );
                register_modal_hits_from(
                    app,
                    modal_box,
                    true,
                    0,
                    visible,
                    start,
                    HitAction::ModalRow,
                );
            }
        }
        Modal::Model(state) => {
            let (lines, start, visible) = picker_lines(app, state, modal_viewport(area, 24));
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Switch model",
                format!("   / to search        {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter select  |  Esc close",
                76,
                24,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::SignIn(state) => {
            let (rows, start, visible) = picker_lines(app, state, modal_viewport(area, 22));
            let mut lines: Vec<Line<'static>> = Vec::new();
            lines.push(Line::from(Span::styled(
                "  Which one do you want to sign in to?",
                Style::default().fg(app.theme.text_dim),
            )));
            lines.push(Line::default());
            lines.extend(rows);
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Sign in",
                format!("   / to search        {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter sign in  |  Esc close",
                92,
                22,
            );
            register_modal_hits_from(app, modal_box, true, 2, visible, start, HitAction::ModalRow);
        }
        Modal::Provider(state) => {
            let (rows, start, visible) = picker_lines(app, state, modal_viewport(area, 22));
            let mut lines: Vec<Line<'static>> = Vec::new();
            lines.push(Line::from(Span::styled(
                "  Where should your messages go?",
                Style::default().fg(app.theme.text_dim),
            )));
            lines.push(Line::default());
            lines.extend(rows);
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Providers",
                format!("   / to search        {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter connect  |  Esc close",
                92,
                22,
            );
            register_modal_hits_from(app, modal_box, true, 2, visible, start, HitAction::ModalRow);
        }
        Modal::Workflow(state) => {
            let (lines, start, visible) = picker_lines(app, state, modal_viewport(area, 20));
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Workflows",
                format!("   / to search        {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter run  |  Esc close",
                88,
                20,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Plugin(state) => {
            let (lines, start, visible) = picker_lines(app, state, modal_viewport(area, 22));
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Plugins",
                format!("   search: {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter install  |  Esc close",
                92,
                22,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Tasks { rows, selected } => {
            let viewport = modal_viewport(area, 24);
            let (start, visible) = App::visible_window(*selected, rows.len(), viewport.max(1));
            let mut lines: Vec<Line<'static>> = Vec::new();
            if rows.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  no background tasks in this session",
                    Style::default().fg(app.theme.text_muted),
                )));
            }
            for (index, row) in rows.iter().enumerate().skip(start).take(visible) {
                let is_selected = index == *selected;
                let hovered = matches!(app.hover, Some(HitAction::ModalRow(hit)) if hit == index);
                let bg = if is_selected {
                    app.theme.surface
                } else if hovered {
                    app.theme.hover
                } else {
                    app.theme.bg
                };
                let elapsed = if row.finished {
                    row.frozen
                } else {
                    row.started.elapsed().as_secs_f64()
                };
                let status_style = if row.finished {
                    Style::default().fg(app.theme.success).bg(bg)
                } else {
                    Style::default().fg(app.theme.accent).bg(bg)
                };
                // Running tasks get a moving bar: we do not know how long they
                // will take, so it shows "alive", not a fake percentage.
                let bar = if row.finished {
                    None
                } else {
                    const WIDTH: usize = 8;
                    let phase = (app.spinner_frame / 2) % (WIDTH * 2);
                    let head = if phase < WIDTH {
                        phase
                    } else {
                        WIDTH * 2 - phase - 1
                    };
                    let mut bar = String::new();
                    for index in 0..WIDTH {
                        bar.push(if index == head { '▇' } else { '▁' });
                    }
                    Some(bar)
                };
                let mut spans = vec![
                    Span::styled(
                        if is_selected { "  ◆ " } else { "  · " }.to_string(),
                        Style::default().fg(app.theme.accent).bg(bg),
                    ),
                    Span::styled(
                        format!("{:<8} ", row.id),
                        Style::default().fg(app.theme.text_dim).bg(bg),
                    ),
                    Span::styled(
                        format!("{:<9} ", row.kind),
                        Style::default().fg(app.theme.text_muted).bg(bg),
                    ),
                    Span::styled(
                        format!("{:>5} ", format!("{elapsed:.0}s")),
                        Style::default().fg(app.theme.text_muted).bg(bg),
                    ),
                    match &bar {
                        Some(bar) => Span::styled(
                            format!("{bar} "),
                            Style::default().fg(app.theme.accent).bg(bg),
                        ),
                        None => Span::raw(""),
                    },
                    Span::styled(
                        row.description.clone(),
                        Style::default().fg(app.theme.text_strong).bg(bg),
                    ),
                ];
                let used: usize = spans.iter().map(|s| s.content.width()).sum();
                let tail = format!("{} ", row.status);
                if used + tail.width() < area.width as usize {
                    spans.push(Span::styled(
                        " ".repeat(area.width as usize - used - tail.width() - 1),
                        Style::default().bg(bg),
                    ));
                    spans.push(Span::styled(tail, status_style));
                }
                lines.push(Line::from(spans));
            }
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Tasks",
                format!(
                    "   limits: {} at a time · depth {}",
                    app.ui_task_limits.0, app.ui_task_limits.1
                ),
                lines,
                "↑/↓ nav  |  Enter output  |  x kill  |  r refresh  |  Esc close",
                96,
                24,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Search(state) => {
            let (lines, start, visible) = picker_lines(app, state, modal_viewport(area, 24));
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Search sessions",
                format!("   matches            {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter open  |  Esc close",
                96,
                24,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Jump(state) => {
            let (lines, start, visible) = picker_lines(app, state, modal_viewport(area, 24));
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Jump to a turn",
                format!("   / to search        {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter jump  |  Esc close",
                84,
                24,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Persona(state) => {
            let (lines, start, visible) = picker_lines(app, state, modal_viewport(area, 22));
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Agent profiles",
                format!("   / to search        {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter apply  |  Esc close",
                86,
                22,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Theme(state) => {
            let (lines, start, visible) = picker_lines(app, state, modal_viewport(area, 16));
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Theme",
                format!("   / to search        {}", query_text(&state.query)),
                lines,
                "↑/↓ nav  |  Enter select  |  Esc close",
                70,
                16,
            );
            register_modal_hits_from(app, modal_box, true, 0, visible, start, HitAction::ModalRow);
        }
        Modal::Info {
            title,
            lines,
            scroll,
        } => {
            let rendered: Vec<Line<'static>> = lines
                .iter()
                .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(app.theme.text))))
                .collect();
            let visible: Vec<Line<'static>> = rendered
                .into_iter()
                .skip(*scroll as usize)
                .take(20)
                .collect();
            let modal_box = modal_frame(
                frame,
                app,
                area,
                title,
                String::new(),
                visible,
                "↑/↓ scroll  |  Esc close",
                90,
                24,
            );
            register_modal_hits(app, modal_box, false, 0, HitAction::ModalRow);
        }
        Modal::Memory { lines, scroll } => {
            let rendered: Vec<Line<'static>> = lines
                .iter()
                .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(app.theme.text))))
                .collect();
            let visible: Vec<Line<'static>> = rendered
                .into_iter()
                .skip(*scroll as usize)
                .take(20)
                .collect();
            let modal_box = modal_frame(
                frame,
                app,
                area,
                "Memory",
                String::new(),
                visible,
                "↑/↓ scroll  |  Esc close",
                86,
                24,
            );
            register_modal_hits(app, modal_box, false, 0, HitAction::ModalRow);
        }
    }
}

/// Visible picker rows plus the index of the first one.
fn picker_lines(
    app: &App,
    state: &super::app::PickerState,
    viewport: usize,
) -> (Vec<Line<'static>>, usize, usize) {
    if state.items.is_empty() {
        let lines = vec![Line::from(Span::styled(
            "    Nothing available",
            Style::default().fg(app.theme.text_muted),
        ))];
        return (lines, 0, 1);
    }
    let (start, visible) = App::visible_window(state.selected, state.items.len(), viewport.max(1));
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, item) in state.items.iter().enumerate().skip(start).take(visible) {
        let selected = i == state.selected;
        let hovered = matches!(app.hover, Some(HitAction::ModalRow(hit)) if hit == i);
        let bg = if selected {
            app.theme.surface
        } else if hovered {
            app.theme.hover
        } else {
            app.theme.bg
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "  ◆ " } else { "  · " }.to_string(),
                Style::default().fg(app.theme.accent).bg(bg),
            ),
            Span::styled(item.clone(), Style::default().fg(app.theme.text).bg(bg)),
        ]));
    }
    (lines, start, visible)
}

#[allow(clippy::too_many_arguments)]
fn modal_frame(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    title: &str,
    subtitle: String,
    lines: Vec<Line<'static>>,
    footer: &str,
    width: u16,
    height: u16,
) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect {
        x,
        y,
        width,
        height,
    };
    app.modal_rect = modal_area;
    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(app.theme.border_modal))
        .style(Style::default().bg(app.theme.bg))
        .title(
            Line::from(Span::styled(
                format!("─ {title} "),
                Style::default().fg(app.theme.text_muted),
            ))
            .alignment(Alignment::Left),
        )
        .title(
            Line::from(Span::styled(
                " [✗] ",
                Style::default().fg(if matches!(app.hover, Some(HitAction::ModalClose)) {
                    app.theme.error
                } else {
                    app.theme.text_muted
                }),
            ))
            .alignment(Alignment::Right),
        )
        .title_bottom(
            Line::from(Span::styled(
                format!(" {footer} "),
                Style::default().fg(app.theme.text_muted),
            ))
            .alignment(Alignment::Center),
        );
    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    let mut text_lines: Vec<Line<'static>> = Vec::new();
    if !subtitle.is_empty() {
        text_lines.push(Line::from(Span::styled(
            subtitle,
            Style::default().fg(app.theme.text_dim),
        )));
        text_lines.push(Line::from(Span::styled(
            "  ".to_string() + &"─".repeat(inner.width.saturating_sub(3) as usize),
            Style::default().fg(app.theme.border),
        )));
    }
    text_lines.extend(lines);
    let paragraph = Paragraph::new(Text::from(text_lines)).style(Style::default().bg(app.theme.bg));
    frame.render_widget(paragraph, inner);
    modal_area
}

/// Register row/close hit areas for a modal: rows start after the title and
/// the optional two-line header.
/// Rows of a modal that are visible for a given modal height.
fn modal_viewport(area: Rect, height: u16) -> usize {
    let h = height.min(area.height.saturating_sub(2));
    h.saturating_sub(4) as usize
}

/// Rectangle of one rendered modal row (`top` = extra lines above the list).
fn modal_row_rect(modal: Rect, subtitle: bool, top: u16, row: u16) -> Option<Rect> {
    let offset = if subtitle { 2u16 } else { 0u16 } + top;
    let y = modal.y + 1 + offset + row;
    if y + 1 >= modal.y + modal.height {
        return None;
    }
    Some(Rect {
        x: modal.x + 1,
        y,
        width: modal.width.saturating_sub(2),
        height: 1,
    })
}

/// Register clicks for `rows` rendered list rows starting at item `start`.
fn register_modal_hits_from(
    app: &mut App,
    modal: Rect,
    subtitle: bool,
    top: u16,
    rows: usize,
    start: usize,
    action: impl Fn(usize) -> HitAction,
) {
    for row in 0..rows {
        if let Some(rect) = modal_row_rect(modal, subtitle, top, row as u16) {
            app.register_hit(rect, action(start + row));
        }
    }
    // The [✗] in the top-right corner closes this level.
    app.register_hit(
        Rect {
            x: modal.x + modal.width.saturating_sub(8),
            y: modal.y,
            width: 7,
            height: 1,
        },
        HitAction::ModalClose,
    );
}

fn register_modal_hits(
    app: &mut App,
    modal: Rect,
    subtitle: bool,
    rows: usize,
    action: impl Fn(usize) -> HitAction,
) {
    register_modal_hits_from(app, modal, subtitle, 0, rows, 0, action)
}

/// The query without the marker slash the hint suggests.
fn query_text(query: &str) -> String {
    query.trim_start_matches('/').to_string()
}

/// Subtitle line for a modal search field (shows the query while typing).
fn search_subtitle(searching: bool, query: &str, hint: &str) -> String {
    if searching || !query.is_empty() {
        format!("   / {query}_")
    } else {
        format!("   {hint}")
    }
}

fn draw_quit(frame: &mut Frame, app: &App, area: Rect) {
    let width = 52.min(area.width.saturating_sub(4));
    let height = 7.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let quit_area = Rect {
        x,
        y,
        width,
        height,
    };
    frame.render_widget(Clear, quit_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.border_focused))
        .style(Style::default().bg(app.theme.bg));
    let inner = block.inner(quit_area);
    frame.render_widget(block, quit_area);
    let text = Text::from(vec![
        Line::default(),
        Line::from(Span::styled(
            "Quit cachycli?",
            Style::default()
                .fg(app.theme.text_strong)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::default(),
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(app.theme.text_strong)),
            Span::styled(" confirm  ", Style::default().fg(app.theme.text_muted)),
            Span::styled("Esc", Style::default().fg(app.theme.text_strong)),
            Span::styled(" cancel", Style::default().fg(app.theme.text_muted)),
        ])
        .alignment(Alignment::Center),
    ]);
    frame.render_widget(Paragraph::new(text), inner);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// One-line preview of a running tool call.
fn tool_preview(arguments: &serde_json::Value) -> String {
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
    ] {
        if let Some(value) = arguments.get(key).and_then(|v| v.as_str()) {
            let line = value.lines().next().unwrap_or("");
            if !line.trim().is_empty() {
                return line.to_string();
            }
        }
    }
    String::new()
}

fn indent_line(line: Line<'static>, indent: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    spans.extend(line.spans);
    Line::from(spans)
}

fn dim_line(line: Line<'static>, theme: &Theme) -> Line<'static> {
    let spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|s| {
            let style = s.style.fg(theme.text_dim);
            Span::styled(s.content, style)
        })
        .collect();
    Line::from(spans)
}

fn pad_right(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

fn truncate_display(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        width += w;
    }
    out.push('…');
    out
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn human_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
