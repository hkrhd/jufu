use std::collections::HashSet;

use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::jj;
use crate::model::{AppConfig, DiffCache, Focus, LoadState, LogEntry};

pub type CommandSender = UnboundedSender<BackgroundEvent>;

#[derive(Debug)]
pub enum BackgroundEvent {
    LogsLoaded(anyhow::Result<Vec<LogEntry>>),
    DiffLoaded {
        change_id: String,
        result: anyhow::Result<Vec<String>>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub enum ControlFlow {
    Continue,
    Exit,
}

pub struct App {
    config: AppConfig,
    pub logs_state: LoadState<Vec<LogEntry>>,
    pub selected: usize,
    pub log_top: usize,
    pub focus: Focus,
    pub diff_scroll: u16,
    pub diff_cache: DiffCache,
    inflight_diffs: HashSet<String>,
    pub left_height: u16,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            logs_state: LoadState::Loading,
            selected: 0,
            log_top: 0,
            focus: Focus::Log,
            diff_scroll: 0,
            diff_cache: DiffCache::default(),
            inflight_diffs: HashSet::new(),
            left_height: 0,
        }
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn selected_log(&self) -> Option<&LogEntry> {
        match &self.logs_state {
            LoadState::Ready(logs) => logs.get(self.selected),
            _ => None,
        }
    }

    pub fn handle_event(&mut self, event: Event, sender: &CommandSender) -> ControlFlow {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Char('c')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    ControlFlow::Exit
                }
                KeyCode::Char('q') => ControlFlow::Exit,
                KeyCode::Up => {
                    self.scroll_up(sender);
                    ControlFlow::Continue
                }
                KeyCode::Down => {
                    self.scroll_down(sender);
                    ControlFlow::Continue
                }
                KeyCode::Enter => {
                    self.toggle_focus();
                    ControlFlow::Continue
                }
                _ => ControlFlow::Continue,
            },
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => self.scroll_up(sender),
                    MouseEventKind::ScrollDown => self.scroll_down(sender),
                    _ => {}
                }
                ControlFlow::Continue
            }
            _ => ControlFlow::Continue,
        }
    }

    pub fn apply_background_event(&mut self, event: BackgroundEvent, sender: &CommandSender) {
        match event {
            BackgroundEvent::LogsLoaded(result) => match result {
                Ok(logs) => {
                    self.logs_state = LoadState::Ready(logs);
                    self.selected = 0;
                    self.log_top = 0;
                    self.diff_scroll = 0;
                    self.ensure_selected_diff_loaded(sender);
                }
                Err(error) => {
                    self.logs_state = LoadState::Error(format_error(&error));
                }
            },
            BackgroundEvent::DiffLoaded { change_id, result } => {
                self.inflight_diffs.remove(&change_id);
                match result {
                    Ok(lines) => self.diff_cache.insert(change_id, LoadState::Ready(lines)),
                    Err(error) => self
                        .diff_cache
                        .insert(change_id, LoadState::Error(format_error(&error))),
                }
            }
        }
    }

    pub fn set_left_height(&mut self, area: Rect) {
        self.left_height = area.height;
        self.ensure_selected_visible();
    }

    pub fn current_diff_state(&self) -> Option<&LoadState<Vec<String>>> {
        let change_id = self.selected_log()?.change_id.as_str();
        self.diff_cache.get(change_id)
    }

    fn scroll_up(&mut self, sender: &CommandSender) {
        match self.focus {
            Focus::Log => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.diff_scroll = 0;
                    self.ensure_selected_visible();
                    self.ensure_selected_diff_loaded(sender);
                }
            }
            Focus::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_sub(1);
            }
        }
    }

    fn scroll_down(&mut self, sender: &CommandSender) {
        match self.focus {
            Focus::Log => {
                if let LoadState::Ready(logs) = &self.logs_state
                    && self.selected + 1 < logs.len()
                {
                    self.selected += 1;
                    self.diff_scroll = 0;
                    self.ensure_selected_visible();
                    self.ensure_selected_diff_loaded(sender);
                }
            }
            Focus::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_add(1);
            }
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Log => Focus::Diff,
            Focus::Diff => Focus::Log,
        };
    }

    fn ensure_selected_diff_loaded(&mut self, sender: &CommandSender) {
        let Some(entry) = self.selected_log() else {
            return;
        };
        let change_id = entry.change_id.clone();
        if self.diff_cache.get(&change_id).is_some() || self.inflight_diffs.contains(&change_id) {
            return;
        }

        self.diff_cache
            .insert(change_id.clone(), LoadState::Loading);
        self.inflight_diffs.insert(change_id.clone());

        let tx = sender.clone();
        let repo_path = self.config.repo_path.clone();
        tokio::spawn(async move {
            let result = jj::load_diff_stat(&repo_path, &change_id).await;
            let _ = tx.send(BackgroundEvent::DiffLoaded { change_id, result });
        });
    }

    fn ensure_selected_visible(&mut self) {
        let LoadState::Ready(logs) = &self.logs_state else {
            return;
        };
        if logs.is_empty() || self.left_height == 0 {
            return;
        }
        if self.selected < self.log_top {
            self.log_top = self.selected;
        }

        let available_height = self.left_height.saturating_sub(2) as usize;
        if available_height == 0 {
            return;
        }

        while total_block_height(logs, self.log_top, self.selected) > available_height {
            self.log_top += 1;
        }
    }
}

fn total_block_height(logs: &[LogEntry], start: usize, end: usize) -> usize {
    logs[start..=end]
        .iter()
        .map(|entry| entry.graph_lines.len().max(1))
        .sum()
}

fn format_error(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\ncaused by: ")
}

#[cfg(test)]
mod tests {
    use crate::model::LogEntry;

    use super::total_block_height;

    #[test]
    fn block_height_counts_multiline_entries() {
        let logs = vec![
            LogEntry {
                graph_lines: vec!["@  ".to_string()],
                ..LogEntry::default()
            },
            LogEntry {
                graph_lines: vec!["◆  ".to_string(), "│".to_string(), "~".to_string()],
                ..LogEntry::default()
            },
        ];

        assert_eq!(total_block_height(&logs, 0, 1), 4);
    }
}
