use std::collections::HashSet;

use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;
use unicode_width::UnicodeWidthStr;

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

#[derive(Debug, Eq, PartialEq)]
pub enum Effect {
    LoadLogs,
    LoadDiff { change_id: String },
}

#[derive(Debug, Eq, PartialEq)]
pub struct Update {
    pub control_flow: ControlFlow,
    pub effects: Vec<Effect>,
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
    pub diff_height: u16,
    pub max_graph_prefix_width: usize,
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
            diff_height: 0,
            max_graph_prefix_width: 0,
        }
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn startup_effects(&self) -> Vec<Effect> {
        vec![Effect::LoadLogs]
    }

    pub fn selected_log(&self) -> Option<&LogEntry> {
        match &self.logs_state {
            LoadState::Ready(logs) => logs.get(self.selected),
            _ => None,
        }
    }

    pub fn handle_event(&mut self, event: Event) -> Update {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Char('c')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    Update::exit()
                }
                KeyCode::Char('q') => Update::exit(),
                KeyCode::Up => Update::continue_with(self.scroll_up()),
                KeyCode::Down => Update::continue_with(self.scroll_down()),
                KeyCode::Enter => {
                    self.toggle_focus();
                    Update::continue_without_effects()
                }
                _ => Update::continue_without_effects(),
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => Update::continue_with(self.scroll_up()),
                MouseEventKind::ScrollDown => Update::continue_with(self.scroll_down()),
                _ => Update::continue_without_effects(),
            },
            _ => Update::continue_without_effects(),
        }
    }

    pub fn apply_background_event(&mut self, event: BackgroundEvent) -> Vec<Effect> {
        match event {
            BackgroundEvent::LogsLoaded(result) => match result {
                Ok(logs) => {
                    self.logs_state = LoadState::Ready(logs);
                    self.selected = 0;
                    self.log_top = 0;
                    self.diff_scroll = 0;
                    self.diff_cache = DiffCache::default();
                    self.inflight_diffs.clear();
                    self.max_graph_prefix_width = max_graph_prefix_width(&self.logs_state);
                    self.clamp_diff_scroll();
                    self.ensure_selected_diff_loaded().into_iter().collect()
                }
                Err(error) => {
                    self.logs_state = LoadState::Error(format_error(&error));
                    self.diff_scroll = 0;
                    self.diff_cache = DiffCache::default();
                    self.inflight_diffs.clear();
                    self.max_graph_prefix_width = 0;
                    Vec::new()
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
                self.clamp_diff_scroll();
                Vec::new()
            }
        }
    }

    pub fn set_left_height(&mut self, area: Rect) {
        self.left_height = area.height;
        self.ensure_selected_visible();
    }

    pub fn set_diff_height(&mut self, area: Rect) {
        self.diff_height = area.height;
        self.clamp_diff_scroll();
    }

    pub fn current_diff_state(&self) -> Option<&LoadState<Vec<String>>> {
        let change_id = self.selected_log()?.change_id.as_str();
        self.diff_cache.get(change_id)
    }

    fn scroll_up(&mut self) -> Vec<Effect> {
        match self.focus {
            Focus::Log => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.diff_scroll = 0;
                    self.ensure_selected_visible();
                    return self.ensure_selected_diff_loaded().into_iter().collect();
                }
                Vec::new()
            }
            Focus::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_sub(1);
                Vec::new()
            }
        }
    }

    fn scroll_down(&mut self) -> Vec<Effect> {
        match self.focus {
            Focus::Log => {
                if let LoadState::Ready(logs) = &self.logs_state
                    && self.selected + 1 < logs.len()
                {
                    self.selected += 1;
                    self.diff_scroll = 0;
                    self.ensure_selected_visible();
                    return self.ensure_selected_diff_loaded().into_iter().collect();
                }
                Vec::new()
            }
            Focus::Diff => {
                self.diff_scroll = self
                    .diff_scroll
                    .saturating_add(1)
                    .min(self.max_diff_scroll());
                Vec::new()
            }
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Log => Focus::Diff,
            Focus::Diff => Focus::Log,
        };
    }

    fn ensure_selected_diff_loaded(&mut self) -> Option<Effect> {
        let entry = self.selected_log()?;
        let change_id = entry.change_id.clone();
        if self.diff_cache.get(&change_id).is_some() || self.inflight_diffs.contains(&change_id) {
            return None;
        }

        self.diff_cache
            .insert(change_id.clone(), LoadState::Loading);
        self.inflight_diffs.insert(change_id.clone());
        Some(Effect::LoadDiff { change_id })
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

    fn clamp_diff_scroll(&mut self) {
        self.diff_scroll = self.diff_scroll.min(self.max_diff_scroll());
    }

    fn max_diff_scroll(&self) -> u16 {
        let available_height = self.diff_height.saturating_sub(2) as usize;
        if available_height == 0 {
            return 0;
        }

        match self.current_diff_state() {
            Some(LoadState::Ready(lines)) => lines.len().saturating_sub(available_height) as u16,
            _ => 0,
        }
    }
}

impl Update {
    fn continue_with(effects: Vec<Effect>) -> Self {
        Self {
            control_flow: ControlFlow::Continue,
            effects,
        }
    }

    fn continue_without_effects() -> Self {
        Self::continue_with(Vec::new())
    }

    fn exit() -> Self {
        Self {
            control_flow: ControlFlow::Exit,
            effects: Vec::new(),
        }
    }
}

fn total_block_height(logs: &[LogEntry], start: usize, end: usize) -> usize {
    logs[start..=end]
        .iter()
        .map(|entry| entry.graph_lines.len().max(1))
        .sum()
}

fn max_graph_prefix_width(logs_state: &LoadState<Vec<LogEntry>>) -> usize {
    match logs_state {
        LoadState::Ready(logs) => logs
            .iter()
            .filter_map(|entry| entry.graph_lines.first())
            .map(|line| UnicodeWidthStr::width(line.as_str()))
            .max()
            .unwrap_or(0),
        _ => 0,
    }
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
    use std::path::PathBuf;

    use ratatui::layout::Rect;

    use crate::model::{AppConfig, LoadState, LogEntry};

    use super::{App, BackgroundEvent, Effect, total_block_height};

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

    #[test]
    fn startup_effects_request_log_load() {
        let app = App::new(AppConfig {
            repo_path: PathBuf::from("."),
        });

        assert_eq!(app.startup_effects(), vec![Effect::LoadLogs]);
    }

    #[test]
    fn logs_loaded_requests_selected_diff_once() {
        let mut app = App::new(AppConfig {
            repo_path: PathBuf::from("."),
        });

        let effects = app.apply_background_event(BackgroundEvent::LogsLoaded(Ok(vec![LogEntry {
            change_id: "abc123".to_string(),
            graph_lines: vec!["@".to_string()],
            ..LogEntry::default()
        }])));

        assert_eq!(
            effects,
            vec![Effect::LoadDiff {
                change_id: "abc123".to_string()
            }]
        );
        assert!(matches!(app.current_diff_state(), Some(LoadState::Loading)));
    }

    #[test]
    fn diff_scroll_is_clamped_to_loaded_content() {
        let mut app = App::new(AppConfig {
            repo_path: PathBuf::from("."),
        });
        app.apply_background_event(BackgroundEvent::LogsLoaded(Ok(vec![LogEntry {
            change_id: "abc123".to_string(),
            graph_lines: vec!["@".to_string()],
            ..LogEntry::default()
        }])));
        app.set_diff_height(Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 4,
        });
        app.diff_scroll = 10;

        app.apply_background_event(BackgroundEvent::DiffLoaded {
            change_id: "abc123".to_string(),
            result: Ok(vec!["1".to_string(), "2".to_string(), "3".to_string()]),
        });

        assert_eq!(app.diff_scroll, 1);
    }
}
