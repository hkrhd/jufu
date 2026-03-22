use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Local};
use serde::Deserialize;
use std::collections::HashSet;
use tokio::process::Command;

use crate::model::LogEntry;

const GRAPH_MARKER: &str = "JUFU:";

#[derive(Debug, Deserialize)]
struct Bookmark {
    name: String,
    #[serde(default)]
    remote: Option<String>,
}

pub async fn ensure_jj_available(repo_path: &Path) -> Result<()> {
    run_jj(repo_path, &["--version"])
        .await
        .map(|_| ())
        .map_err(|_| anyhow!("jj is required but was not found in PATH"))
}

pub async fn load_logs(repo_path: &Path) -> Result<Vec<LogEntry>> {
    let graph_output = run_jj(
        repo_path,
        &[
            "log",
            "-r",
            "::",
            "-T",
            "\"JUFU:\" ++ json(change_id) ++ \"\\t\" ++ json(commit_id) ++ \"\\t\" ++ json(author.name()) ++ \"\\t\" ++ json(author.timestamp()) ++ \"\\t\" ++ json(description) ++ \"\\t\" ++ json(local_bookmarks) ++ \"\\t\" ++ json(remote_bookmarks) ++ \"\\n\"",
        ],
    )
    .await
    .context("failed to load jj graph")?;

    parse_graph_lines(&graph_output)?
        .into_iter()
        .map(build_log_entry)
        .collect()
}

pub async fn load_diff_stat(repo_path: &Path, change_id: &str) -> Result<Vec<String>> {
    let output = run_jj(repo_path, &["diff", "-r", change_id, "--stat"])
        .await
        .with_context(|| format!("failed to load diff stat for {change_id}"))?;

    let lines = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if lines.is_empty() {
        Ok(vec!["(no changes)".to_string()])
    } else {
        Ok(lines)
    }
}

async fn run_jj(repo_path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("jj")
        .arg("-R")
        .arg(repo_path)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to run jj {}", args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("jj {}: {}", args.join(" "), stderr);
    }
}

fn build_log_entry(graph_entry: ParsedGraphEntry) -> Result<LogEntry> {
    let description = graph_entry.description.trim_end().to_string();

    Ok(LogEntry {
        change_id_short: short_change_id(&graph_entry.change_id),
        commit_id: graph_entry.commit_id,
        date: short_date(&graph_entry.author_timestamp)?,
        author: if graph_entry.author_name.is_empty() {
            "(unknown)".to_string()
        } else {
            graph_entry.author_name
        },
        description,
        bookmarks: collect_bookmarks(graph_entry.local_bookmarks, graph_entry.remote_bookmarks),
        graph_lines: graph_entry.lines,
        change_id: graph_entry.change_id,
    })
}

fn short_change_id(change_id: &str) -> String {
    change_id.chars().take(8).collect()
}

fn short_date(timestamp: &str) -> Result<String> {
    let parsed = DateTime::parse_from_rfc3339(timestamp)
        .with_context(|| format!("failed to parse author timestamp: {timestamp}"))?;
    Ok(parsed
        .with_timezone(&Local)
        .format("%y%m%dT%H:%M")
        .to_string())
}

#[derive(Debug)]
struct ParsedGraphEntry {
    change_id: String,
    commit_id: String,
    author_name: String,
    author_timestamp: String,
    description: String,
    local_bookmarks: Vec<Bookmark>,
    remote_bookmarks: Vec<Bookmark>,
    lines: Vec<String>,
}

fn parse_graph_lines(output: &str) -> Result<Vec<ParsedGraphEntry>> {
    let mut entries = Vec::new();
    let mut current: Option<ParsedGraphEntry> = None;

    for line in output.lines() {
        if let Some((prefix, payload)) = line.split_once(GRAPH_MARKER) {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }

            current = Some(parse_graph_payload(prefix, payload)?);
            continue;
        }

        if let Some(entry) = current.as_mut() {
            entry.lines.push(line.to_string());
        }
    }

    if let Some(entry) = current {
        entries.push(entry);
    }

    if entries.is_empty() {
        bail!("jj log returned no commits");
    }

    Ok(entries)
}

fn parse_graph_payload(prefix: &str, payload: &str) -> Result<ParsedGraphEntry> {
    let mut parts = payload.splitn(7, '\t');
    let change_id = parse_json_field::<String>(parts.next(), "change_id")?;
    let commit_id = parse_json_field::<String>(parts.next(), "commit_id")?;
    let author_name = parse_json_field::<String>(parts.next(), "author_name")?;
    let author_timestamp = parse_json_field::<String>(parts.next(), "author_timestamp")?;
    let description = parse_json_field::<String>(parts.next(), "description")?;
    let local_bookmarks = parse_json_field::<Vec<Bookmark>>(parts.next(), "local_bookmarks")?;
    let remote_bookmarks = parse_json_field::<Vec<Bookmark>>(parts.next(), "remote_bookmarks")?;

    Ok(ParsedGraphEntry {
        change_id,
        commit_id,
        author_name,
        author_timestamp,
        description,
        local_bookmarks,
        remote_bookmarks,
        lines: vec![prefix.to_string()],
    })
}

fn collect_bookmarks(
    local_bookmarks: Vec<Bookmark>,
    remote_bookmarks: Vec<Bookmark>,
) -> Vec<String> {
    let mut rendered = Vec::new();
    let mut seen = HashSet::new();

    for bookmark in local_bookmarks {
        if seen.insert(bookmark.name.clone()) {
            rendered.push(bookmark.name);
        }
    }

    for bookmark in remote_bookmarks {
        let Some(remote) = bookmark.remote else {
            continue;
        };
        let rendered_name = if remote == "git" {
            bookmark.name
        } else {
            format!("{}@{}", bookmark.name, remote)
        };
        if seen.insert(rendered_name.clone()) {
            rendered.push(rendered_name);
        }
    }

    rendered
}

fn parse_json_field<T>(value: Option<&str>, field_name: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = value.ok_or_else(|| anyhow!("missing field: {field_name}"))?;
    serde_json::from_str(raw).with_context(|| format!("failed to parse field: {field_name}"))
}

#[cfg(test)]
mod tests {
    use super::{Bookmark, collect_bookmarks, parse_graph_lines, short_date};

    #[test]
    fn parse_graph_groups_continuation_lines() {
        let output = "\
@  JUFU:\"a\"\t\"111111111111\"\t\"alice\"\t\"2026-03-22T00:00:00+09:00\"\t\"first\\n\"\t[]\t[]\n\
◆    JUFU:\"b\"\t\"222222222222\"\t\"bob\"\t\"2026-03-21T00:00:00+09:00\"\t\"second\\n\"\t[]\t[]\n\
├─╮\n\
│ ◆  JUFU:\"c\"\t\"333333333333\"\t\"carol\"\t\"2026-03-20T00:00:00+09:00\"\t\"third\\n\"\t[]\t[]\n\
├─╯\n";

        let entries = parse_graph_lines(output).expect("graph should parse");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].change_id, "a");
        assert_eq!(entries[0].author_name, "alice");
        assert_eq!(
            entries[1].lines,
            vec!["◆    ".to_string(), "├─╮".to_string()]
        );
        assert_eq!(
            entries[2].lines,
            vec!["│ ◆  ".to_string(), "├─╯".to_string()]
        );
    }

    #[test]
    fn short_date_formats_local_timestamp() {
        let formatted = short_date("2026-01-01T01:01:59+09:00").expect("timestamp should parse");
        assert_eq!(formatted, "260101T01:01");
    }

    #[test]
    fn collect_bookmarks_keeps_local_and_omits_git_remote_suffix() {
        let bookmarks = collect_bookmarks(
            vec![Bookmark {
                name: "main".to_string(),
                remote: None,
            }],
            vec![
                Bookmark {
                    name: "main".to_string(),
                    remote: Some("git".to_string()),
                },
                Bookmark {
                    name: "main".to_string(),
                    remote: Some("origin".to_string()),
                },
            ],
        );

        assert_eq!(
            bookmarks,
            vec!["main".to_string(), "main@origin".to_string()]
        );
    }
}
