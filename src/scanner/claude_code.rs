use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{file_mtime_secs, home_dir, normalize_path, parse_timestamp_str};

/// Scans Claude Code configuration (`~/.claude.json`) and session history (`~/.claude/projects/`).
pub struct ClaudeCodeScanner;

impl AgentScanner for ClaudeCodeScanner {
    fn agent(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let Some(home) = home_dir() else {
            return Ok(Vec::new());
        };
        scan_claude_dir(
            &home.join(".claude.json"),
            &home.join(".claude").join("projects"),
        )
    }
}

#[derive(Deserialize)]
struct ClaudeConfigFile {
    #[serde(default)]
    projects: Option<Value>,
}

pub(crate) fn scan_claude_dir(
    claude_json: &Path,
    projects_dir: &Path,
) -> Result<Vec<AgentProjectEntry>> {
    let mut by_path: HashMap<PathBuf, AgentProjectEntry> = HashMap::new();

    // 1. Parse `~/.claude.json` top-level `projects` map (authoritative exact paths).
    if claude_json.is_file() {
        if let Ok(file) = File::open(claude_json) {
            let reader = BufReader::new(file);
            if let Ok(config) = serde_json::from_reader::<_, ClaudeConfigFile>(reader) {
                if let Some(projects) = config.projects.as_ref().and_then(|v| v.as_object()) {
                    let fallback_mtime = file_mtime_secs(claude_json);
                    for (raw_path, meta) in projects {
                        let p = normalize_path(Path::new(raw_path));
                        if !p.is_absolute() {
                            continue;
                        }
                        let last_active = extract_claude_project_timestamp(meta).or(fallback_mtime);
                        by_path.insert(
                            p.clone(),
                            AgentProjectEntry {
                                agent: AgentKind::ClaudeCode,
                                path: p,
                                display_name: None,
                                last_active,
                                session_count: None,
                                source: "~/.claude.json".to_string(),
                            },
                        );
                    }
                }
            }
        }
    }

    // 2. Parse `~/.claude/projects/<encoded-dir>/*.jsonl` to capture session counts,
    //    exact `cwd` paths, and latest session activity timestamps.
    if projects_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(projects_dir) {
            for entry in entries.flatten() {
                let dir_path = entry.path();
                if !dir_path.is_dir() {
                    continue;
                }
                if let Some((cwd, sessions, latest_ts)) = inspect_claude_project_folder(&dir_path) {
                    let norm = normalize_path(&cwd);
                    if !norm.is_absolute() {
                        continue;
                    }
                    by_path
                        .entry(norm.clone())
                        .and_modify(|existing| {
                            existing.session_count =
                                Some(existing.session_count.unwrap_or(0).saturating_add(sessions));
                            if latest_ts > existing.last_active {
                                existing.last_active = latest_ts;
                            }
                            existing.source = "~/.claude/projects".to_string();
                        })
                        .or_insert_with(|| AgentProjectEntry {
                            agent: AgentKind::ClaudeCode,
                            path: norm,
                            display_name: None,
                            last_active: latest_ts,
                            session_count: Some(sessions),
                            source: "~/.claude/projects".to_string(),
                        });
                }
            }
        }
    }

    Ok(by_path.into_values().collect())
}

fn extract_claude_project_timestamp(meta: &Value) -> Option<i64> {
    if let Some(s) = meta.get("lastUpdated").and_then(|v| v.as_str()) {
        if let Some(ts) = parse_timestamp_str(s) {
            return Some(ts);
        }
    }
    if let Some(n) = meta.get("lastCostTimestamp").and_then(|v| v.as_i64()) {
        return crate::scanner::paths::normalize_unix_timestamp(n);
    }
    None
}

/// Extracts exact `cwd`, jsonl session count, and latest mtime from a Claude project folder.
fn inspect_claude_project_folder(dir: &Path) -> Option<(PathBuf, u32, Option<i64>)> {
    let read_dir = fs::read_dir(dir).ok()?;
    let mut session_files: Vec<PathBuf> = Vec::new();
    let mut latest_ts = file_mtime_secs(dir);

    for item in read_dir.flatten() {
        let p = item.path();
        if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Some(ts) = file_mtime_secs(&p) {
                if latest_ts.is_none_or(|cur| ts > cur) {
                    latest_ts = Some(ts);
                }
            }
            session_files.push(p);
        }
    }

    if session_files.is_empty() {
        return None;
    }

    // Inspect the first few lines of session files to find an authoritative `cwd` field.
    let mut found_cwd: Option<PathBuf> = None;
    for session_file in session_files.iter().take(5) {
        if let Some(cwd) = extract_cwd_from_jsonl(session_file) {
            found_cwd = Some(cwd);
            break;
        }
    }

    let cwd = found_cwd?;
    Some((cwd, session_files.len() as u32, latest_ts))
}

fn extract_cwd_from_jsonl(path: &Path) -> Option<PathBuf> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().take(20).flatten() {
        if !line.contains("\"cwd\"") {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(&line) {
            if let Some(cwd) = val.get("cwd").and_then(|v| v.as_str()) {
                let p = PathBuf::from(cwd);
                if p.is_absolute() {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_claude_json_and_sessions() {
        let tmp = tempdir().unwrap();
        let claude_json = tmp.path().join(".claude.json");
        let projects_dir = tmp.path().join("projects");
        let sub = projects_dir.join("-Users-demo-my-app");
        fs::create_dir_all(&sub).unwrap();

        fs::write(
            &claude_json,
            r#"{"projects":{"/Users/demo/my-app":{},"\/Users\/demo\/other":{}}}"#,
        )
        .unwrap();
        fs::write(
            sub.join("sess1.jsonl"),
            "{\"type\":\"user\",\"cwd\":\"/Users/demo/my-app\"}\n",
        )
        .unwrap();

        let entries = scan_claude_dir(&claude_json, &projects_dir).unwrap();
        assert_eq!(entries.len(), 2);
        let my_app = entries
            .iter()
            .find(|e| e.path == PathBuf::from("/Users/demo/my-app"))
            .unwrap();
        assert_eq!(my_app.session_count, Some(1));
    }
}
