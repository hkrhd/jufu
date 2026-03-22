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

const CHANGE_ID_WIDTH: usize = 8;
const DATE_WIDTH: usize = 12;
const BOOKMARK_WIDTH: usize = 20;
const AUTHOR_WIDTH: usize = 6;
const COLUMN_GAP_WIDTH: usize = 2;
const LOG_LINE_WIDTH: usize =
    CHANGE_ID_WIDTH + DATE_WIDTH + BOOKMARK_WIDTH + AUTHOR_WIDTH + (COLUMN_GAP_WIDTH * 3);

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
        Some(entry) => format_description(entry),
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
    let max_graph_prefix_width = logs
        .iter()
        .filter_map(|entry| entry.graph_lines.first())
        .map(|line| display_width(line))
        .max()
        .unwrap_or(0);

    for (index, entry) in logs.iter().enumerate().skip(top) {
        let row_style = if index == selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        let first_prefix = entry.graph_lines.first().cloned().unwrap_or_default();
        let padded_prefix = pad_or_truncate(&first_prefix, max_graph_prefix_width);
        let main_text = format_log_line(entry, width.saturating_sub(max_graph_prefix_width));
        lines.push(Line::from(vec![
            Span::styled(padded_prefix, row_style.fg(Color::Cyan)),
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
    let fixed = [
        pad_or_truncate(&entry.change_id_short, CHANGE_ID_WIDTH),
        pad_or_truncate(&entry.date, DATE_WIDTH),
        pad_or_truncate(
            &join_bookmarks(&entry.bookmarks, BOOKMARK_WIDTH),
            BOOKMARK_WIDTH,
        ),
        pad_or_truncate(&entry.author, AUTHOR_WIDTH),
    ]
    .join("  ");

    let rendered = if display_width(&fixed) == LOG_LINE_WIDTH {
        fixed
    } else {
        pad_or_truncate(&fixed, LOG_LINE_WIDTH)
    };

    truncate_display_width(&rendered, width)
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

fn format_description(entry: &LogEntry) -> String {
    let description = if entry.description.is_empty() {
        "(no description)"
    } else {
        entry.description.as_str()
    };

    format!("commit ID: {}\n\n{}", entry.commit_id, description)
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

#[cfg(test)]
mod tests {
    use super::{
        AUTHOR_WIDTH, BOOKMARK_WIDTH, CHANGE_ID_WIDTH, COLUMN_GAP_WIDTH, DATE_WIDTH,
        LOG_LINE_WIDTH, build_log_lines, display_width, format_description, format_log_line,
    };
    use crate::model::LogEntry;

    #[test]
    fn format_log_line_uses_fixed_width_columns() {
        let entry = LogEntry {
            change_id_short: "abcdefgh".to_string(),
            date: "260101T01:01".to_string(),
            author: "verylongname".to_string(),
            bookmarks: vec![
                "main".to_string(),
                "release/2026".to_string(),
                "feature".to_string(),
            ],
            ..LogEntry::default()
        };

        let line = format_log_line(&entry, LOG_LINE_WIDTH);
        assert_eq!(display_width(&line), LOG_LINE_WIDTH);
        assert!(line.contains("abcdefgh"));
        assert!(line.contains("260101T01:01"));
        assert!(line.contains("main"));
        assert!(line.contains("veryl…"));
        assert!(!line.contains("123456789abc"));
    }

    #[test]
    fn empty_bookmarks_still_reserve_column_width() {
        let entry = LogEntry {
            change_id_short: "abcdefgh".to_string(),
            date: "260101T01:01".to_string(),
            author: "alice".to_string(),
            ..LogEntry::default()
        };

        let line = format_log_line(&entry, LOG_LINE_WIDTH);
        let bookmark_start = CHANGE_ID_WIDTH + COLUMN_GAP_WIDTH + DATE_WIDTH + COLUMN_GAP_WIDTH;
        let bookmark_end = bookmark_start + BOOKMARK_WIDTH;
        let author_start = bookmark_end + COLUMN_GAP_WIDTH;
        let author_end = author_start + AUTHOR_WIDTH;

        assert_eq!(
            &line[bookmark_start..bookmark_end],
            " ".repeat(BOOKMARK_WIDTH)
        );
        assert_eq!(&line[author_start..author_end], "alice ");
    }

    #[test]
    fn format_description_includes_commit_id_header() {
        let entry = LogEntry {
            commit_id: "1234567890abcdef".to_string(),
            description: "subject\nbody".to_string(),
            ..LogEntry::default()
        };

        assert_eq!(
            format_description(&entry),
            "commit ID: 1234567890abcdef\n\nsubject\nbody"
        );
    }

    #[test]
    fn build_log_lines_aligns_change_id_column() {
        let logs = vec![
            LogEntry {
                change_id_short: "aaaabbbb".to_string(),
                date: "260101T01:01".to_string(),
                author: "alice".to_string(),
                graph_lines: vec!["@  ".to_string()],
                ..LogEntry::default()
            },
            LogEntry {
                change_id_short: "ccccdddd".to_string(),
                date: "260101T01:01".to_string(),
                author: "bob".to_string(),
                graph_lines: vec!["├─╮ ".to_string()],
                ..LogEntry::default()
            },
        ];

        let lines = build_log_lines(&logs, 0, 0, 80, 10);
        let rendered = lines
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();

        let first_pos = rendered[0]
            .find("aaaabbbb")
            .expect("first change id should exist");
        let second_pos = rendered[1]
            .find("ccccdddd")
            .expect("second change id should exist");

        assert_eq!(
            display_width(&rendered[0][..first_pos]),
            display_width(&rendered[1][..second_pos])
        );
    }
}
