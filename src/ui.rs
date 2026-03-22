use std::io;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::model::{Focus, LoadState, LogEntry};

pub type Terminal = DefaultTerminal;

pub fn init_terminal() -> io::Result<Terminal> {
    let terminal = ratatui::init();
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::event::EnableMouseCapture)?;
    Ok(terminal)
}

pub fn restore_terminal() -> io::Result<()> {
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    Ok(())
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(frame.area());
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(areas[1]);

    app.set_left_height(areas[0]);
    render_log_list(frame, areas[0], app);
    render_description(frame, right[0], app);
    render_diff(frame, right[1], app);
}

fn render_log_list(frame: &mut Frame, area: Rect, app: &App) {
    let block = bordered_block("Log", app.focus == Focus::Log);

    match &app.logs_state {
        LoadState::Loading => {
            frame.render_widget(Paragraph::new("Loading jj log...").block(block), area);
        }
        LoadState::Error(message) => {
            frame.render_widget(
                Paragraph::new(message.as_str())
                    .block(block)
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        LoadState::Ready(logs) => {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let lines = build_log_lines(
                logs,
                app.selected,
                app.log_top,
                inner.width as usize,
                inner.height as usize,
            );
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
        }
    }
}

fn render_description(frame: &mut Frame, area: Rect, app: &App) {
    let block = bordered_block("Description", false);
    let text = match app.selected_log() {
        Some(entry) => {
            if entry.description.is_empty() {
                "(no description)".to_string()
            } else {
                entry.description.clone()
            }
        }
        None => "No commit selected".to_string(),
    };
    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn render_diff(frame: &mut Frame, area: Rect, app: &App) {
    let block = bordered_block("Diff Stat", app.focus == Focus::Diff);
    let body = match app.current_diff_state() {
        Some(LoadState::Loading) => Paragraph::new("Loading jj diff --stat..."),
        Some(LoadState::Error(message)) => Paragraph::new(message.as_str()),
        Some(LoadState::Ready(lines)) => {
            Paragraph::new(lines.join("\n")).scroll((app.diff_scroll, 0))
        }
        None => Paragraph::new("No commit selected"),
    };
    frame.render_widget(body.block(block).wrap(Wrap { trim: false }), area);
}

fn build_log_lines(
    logs: &[LogEntry],
    selected: usize,
    top: usize,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for (index, entry) in logs.iter().enumerate().skip(top) {
        let row_style = if index == selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        let first_prefix = entry.graph_lines.first().cloned().unwrap_or_default();
        let main_text = format_log_line(entry, width.saturating_sub(display_width(&first_prefix)));
        lines.push(Line::from(vec![
            Span::styled(first_prefix, row_style.fg(Color::Cyan)),
            Span::styled(main_text, row_style),
        ]));

        for graph_line in entry.graph_lines.iter().skip(1) {
            lines.push(Line::from(Span::styled(
                graph_line.clone(),
                row_style.fg(Color::Cyan),
            )));
        }

        if lines.len() >= height {
            lines.truncate(height);
            break;
        }
    }

    lines
}

fn format_log_line(entry: &LogEntry, width: usize) -> String {
    let mut segments = vec![
        pad_or_truncate(&entry.change_id_short, 8),
        pad_or_truncate(&entry.date, 10),
        pad_or_truncate(&entry.author, 12),
        pad_or_truncate(&entry.commit_id_short, 12),
        pad_or_truncate(&entry.description_first_line, 40),
    ];

    if !entry.bookmarks.is_empty() {
        segments.push(format!("[{}]", join_bookmarks(&entry.bookmarks, 20)));
    }

    truncate_display_width(&segments.join("  "), width)
}

fn join_bookmarks(bookmarks: &[String], max_width: usize) -> String {
    let mut rendered = String::new();
    let mut hidden = 0usize;

    for bookmark in bookmarks {
        let candidate = if rendered.is_empty() {
            bookmark.clone()
        } else {
            format!("{rendered}, {bookmark}")
        };

        if display_width(&candidate) > max_width {
            hidden += 1;
            continue;
        }

        rendered = candidate;
    }

    if hidden > 0 {
        if rendered.is_empty() {
            format!("+{hidden}")
        } else {
            format!("{rendered}, +{hidden}")
        }
    } else {
        rendered
    }
}

fn pad_or_truncate(value: &str, width: usize) -> String {
    let mut truncated = truncate_display_width(value, width);
    let pad = width.saturating_sub(display_width(&truncated));
    truncated.push_str(&" ".repeat(pad));
    truncated
}

fn truncate_display_width(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = display_width(ellipsis);
    let target = max_width.saturating_sub(ellipsis_width);
    let mut result = String::new();

    for ch in value.chars() {
        let next_width = display_width(&result) + display_width(ch.encode_utf8(&mut [0; 4]));
        if next_width > target {
            break;
        }
        result.push(ch);
    }

    format!("{result}{ellipsis}")
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn bordered_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    let style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, style))
}
