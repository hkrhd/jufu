use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub repo_path: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct LogEntry {
    pub change_id: String,
    pub change_id_short: String,
    pub commit_id_short: String,
    pub date: String,
    pub author: String,
    pub description: String,
    pub description_first_line: String,
    pub bookmarks: Vec<String>,
    pub graph_lines: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Focus {
    Log,
    Diff,
}

#[derive(Clone, Debug)]
pub enum LoadState<T> {
    Loading,
    Ready(T),
    Error(String),
}

#[derive(Clone, Debug, Default)]
pub struct DiffCache {
    pub entries: HashMap<String, LoadState<Vec<String>>>,
}

impl DiffCache {
    pub fn get(&self, change_id: &str) -> Option<&LoadState<Vec<String>>> {
        self.entries.get(change_id)
    }

    pub fn insert(&mut self, change_id: String, state: LoadState<Vec<String>>) {
        self.entries.insert(change_id, state);
    }
}
