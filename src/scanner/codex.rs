use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde_json::Value;

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{
    file_mtime_secs, home_dir, normalize_path, normalize_unix_timestamp, parse_timestamp_str,
};

const WORKSPACE_ROOT_LABELS_PREFIX: &str = "electron-owner-value:electron-workspace-root-labels:";

/// Scans OpenAI Codex CLI session metadata under `~/.codex/sessions` and `~/.codex/archived_sessions`,
/// and enriches projects with custom names from `state_5.sqlite` and `.codex-global-state.json`.
pub struct CodexScanner;

impl AgentScanner for CodexScanner {
    fn agent(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let Some(home) = home_dir() else {
            return Ok(Vec::new());
        };
        let codex_dir = home.join(".codex");
        scan_codex_dir(&codex_dir)
    }
}

struct CodexProjectAcc {
    session_count: Option<u32>,
    last_active: Option<i64>,
    display_name: Option<String>,
    display_name_ts: Option<i64>,
    source: String,
}

pub(crate) fn scan_codex_dir(codex_dir: &Path) -> Result<Vec<AgentProjectEntry>> {
    if !codex_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut by_path: HashMap<PathBuf, CodexProjectAcc> = HashMap::new();

    for sub in ["sessions", "archived_sessions"] {
        let root = codex_dir.join(sub);
        if root.is_dir() {
            walk_jsonl_files(&root, 0, &mut by_path);
        }
    }

    // Read project metadata and custom names from Codex SQLite state databases (`state_*.sqlite`).
    let mut state_dbs: Vec<PathBuf> = fs::read_dir(codex_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("state_") && n.ends_with(".sqlite"))
        })
        .collect();
    state_dbs.sort();
    for db_path in state_dbs {
        let _ = read_codex_state_sqlite(&db_path, &mut by_path);
    }

    // Read fallback project labels from `.codex-global-state.json`.
    let global_state = codex_dir.join(".codex-global-state.json");
    if global_state.is_file() {
        read_codex_global_state(&global_state, &mut by_path);
    }

    let entries = by_path
        .into_iter()
        .map(|(path, acc)| AgentProjectEntry {
            agent: AgentKind::Codex,
            path,
            display_name: acc.display_name,
            last_active: acc.last_active,
            session_count: acc.session_count,
            source: acc.source,
        })
        .collect();

    Ok(entries)
}

fn upsert_codex_project_meta(
    acc: &mut HashMap<PathBuf, CodexProjectAcc>,
    path: PathBuf,
    display_name: Option<String>,
    ts: Option<i64>,
    source: &str,
) {
    let clean_name = display_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    acc.entry(path)
        .and_modify(|existing| {
            if ts > existing.last_active {
                existing.last_active = ts;
            }
            if let Some(name) = clean_name.as_ref()
                && (existing.display_name.is_none() || ts > existing.display_name_ts)
            {
                existing.display_name = Some(name.clone());
                existing.display_name_ts = ts;
            }
        })
        .or_insert_with(|| CodexProjectAcc {
            session_count: None,
            last_active: ts,
            display_name: clean_name.clone(),
            display_name_ts: if clean_name.is_some() { ts } else { None },
            source: source.to_string(),
        });
}

fn read_codex_state_sqlite(
    db_path: &Path,
    acc: &mut HashMap<PathBuf, CodexProjectAcc>,
) -> Result<()> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db_path, flags)?;
    let _ = conn.busy_timeout(Duration::from_millis(200));

    let source = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| format!("~/.codex/{n}"))
        .unwrap_or_else(|| "~/.codex/state.sqlite".to_string());

    let mut stmt = conn.prepare(
        "SELECT p.name, r.path, p.updated_at_ms \
         FROM projects p \
         JOIN project_roots r ON p.id = r.project_id \
         WHERE r.path IS NOT NULL AND r.path != ''",
    )?;
    let rows = stmt.query_map([], |row| {
        let name: Option<String> = row.get(0)?;
        let path: String = row.get(1)?;
        let updated_ms: Option<i64> = row.get(2)?;
        Ok((name, path, updated_ms))
    })?;

    for row in rows.flatten() {
        let (name, raw_path, updated_ms) = row;
        let norm = normalize_path(Path::new(&raw_path));
        if !norm.is_absolute() {
            continue;
        }
        let ts = updated_ms.and_then(normalize_unix_timestamp);
        upsert_codex_project_meta(acc, norm, name, ts, &source);
    }

    Ok(())
}

#[derive(Default)]
struct CodexGlobalStateData {
    local_projects: Option<Value>,
    workspace_root_label_maps: Vec<Value>,
}

impl<'de> Deserialize<'de> for CodexGlobalStateData {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RootVisitor;

        impl<'de> Visitor<'de> for RootVisitor {
            type Value = CodexGlobalStateData;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a Codex global state object")
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut out = CodexGlobalStateData::default();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "local-projects" {
                        out.local_projects = Some(map.next_value::<Value>()?);
                    } else if key.starts_with(WORKSPACE_ROOT_LABELS_PREFIX)
                        || key == "electron-workspace-root-labels"
                    {
                        out.workspace_root_label_maps
                            .push(map.next_value::<Value>()?);
                    } else {
                        let _ = map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(out)
            }
        }

        deserializer.deserialize_map(RootVisitor)
    }
}

fn read_codex_global_state(file_path: &Path, acc: &mut HashMap<PathBuf, CodexProjectAcc>) {
    let Ok(file) = File::open(file_path) else {
        return;
    };
    let reader = BufReader::new(file);
    let Ok(state) = serde_json::from_reader::<_, CodexGlobalStateData>(reader) else {
        return;
    };
    let fallback_ts = file_mtime_secs(file_path);

    if let Some(local_projects) = state.local_projects.as_ref().and_then(|v| v.as_object()) {
        for proj in local_projects.values() {
            let name = proj
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let ts = proj
                .get("updatedAt")
                .or_else(|| proj.get("createdAt"))
                .and_then(|v| v.as_i64())
                .and_then(normalize_unix_timestamp)
                .or(fallback_ts);
            if let Some(roots) = proj.get("rootPaths").and_then(|v| v.as_array()) {
                for root_val in roots {
                    if let Some(path_str) = root_val.as_str() {
                        let norm = normalize_path(Path::new(path_str));
                        if norm.is_absolute() {
                            upsert_codex_project_meta(
                                acc,
                                norm,
                                name.clone(),
                                ts,
                                "~/.codex/.codex-global-state.json",
                            );
                        }
                    }
                }
            }
        }
    }

    for value in &state.workspace_root_label_maps {
        if let Some(labels) = value.as_object() {
            for (path_str, label_val) in labels {
                let Some(label) = label_val.as_str() else {
                    continue;
                };
                let norm = normalize_path(Path::new(path_str));
                if !norm.is_absolute() {
                    continue;
                }
                upsert_codex_project_meta(
                    acc,
                    norm,
                    Some(label.to_string()),
                    None,
                    "~/.codex/.codex-global-state.json",
                );
            }
        }
    }
}

fn walk_jsonl_files(dir: &Path, depth: usize, acc: &mut HashMap<PathBuf, CodexProjectAcc>) {
    if depth > 5 {
        return;
    }
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    for item in read_dir.flatten() {
        let path = item.path();
        if path.is_dir() {
            walk_jsonl_files(&path, depth + 1, acc);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Some((cwds, ts)) = parse_codex_session_header(&path) {
                let effective_ts = ts.or_else(|| file_mtime_secs(&path));
                for cwd in cwds {
                    let norm = normalize_path(&cwd);
                    if !norm.is_absolute() {
                        continue;
                    }
                    acc.entry(norm)
                        .and_modify(|existing| {
                            existing.session_count =
                                Some(existing.session_count.unwrap_or(0).saturating_add(1));
                            if effective_ts > existing.last_active {
                                existing.last_active = effective_ts;
                            }
                        })
                        .or_insert_with(|| CodexProjectAcc {
                            session_count: Some(1),
                            last_active: effective_ts,
                            display_name: None,
                            display_name_ts: None,
                            source: "~/.codex/sessions".to_string(),
                        });
                }
            }
        }
    }
}

/// Reads only the first line (`session_meta`) of a Codex session JSONL file.
fn parse_codex_session_header(path: &Path) -> Option<(Vec<PathBuf>, Option<i64>)> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).ok()? == 0 {
        return None;
    }

    let val: Value = serde_json::from_str(&first_line).ok()?;
    let payload = val.get("payload").unwrap_or(&val);

    let mut cwds = Vec::new();
    if let Some(cwd) = payload.get("cwd").and_then(|v| v.as_str()) {
        cwds.push(PathBuf::from(cwd));
    }
    if let Some(roots) = payload
        .get("runtime_workspace_roots")
        .and_then(|v| v.as_array())
    {
        for root in roots {
            if let Some(s) = root.as_str() {
                let p = PathBuf::from(s);
                if !cwds.contains(&p) {
                    cwds.push(p);
                }
            }
        }
    }

    if cwds.is_empty() {
        return None;
    }

    let ts = payload
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(parse_timestamp_str)
        .or_else(|| {
            val.get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(parse_timestamp_str)
        });

    Some((cwds, ts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_codex_session_meta() {
        let tmp = tempdir().unwrap();
        let sess_dir = tmp.path().join("sessions/2026/11/25");
        fs::create_dir_all(&sess_dir).unwrap();
        fs::write(
            sess_dir.join("rollout-1.jsonl"),
            r#"{"type":"session_meta","payload":{"cwd":"/Users/demo/codex-proj","timestamp":"2026-11-25T10:00:00Z"}}"#,
        )
        .unwrap();

        let entries = scan_codex_dir(tmp.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("/Users/demo/codex-proj"));
        assert_eq!(entries[0].session_count, Some(1));
        assert!(entries[0].last_active.is_some());
    }

    #[test]
    fn reads_custom_names_from_state_sqlite_and_global_state() {
        let tmp = tempdir().unwrap();

        let db_path = tmp.path().join("state_5.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, updated_at_ms INTEGER NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE project_roots (project_id TEXT NOT NULL, position INTEGER NOT NULL, path TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, updated_at_ms) VALUES ('p1', 'SQLite Renamed', 1788000000000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_roots (project_id, position, path) VALUES ('p1', 0, '/Users/demo/app-a')",
            [],
        )
        .unwrap();
        drop(conn);

        fs::write(
            tmp.path().join(".codex-global-state.json"),
            r#"{
                "local-projects": {
                    "local-1": {
                        "name": "Older Name",
                        "rootPaths": ["/Users/demo/app-a"],
                        "updatedAt": 1780000000000
                    },
                    "local-2": {
                        "name": "Global Renamed",
                        "rootPaths": ["/Users/demo/app-b"],
                        "updatedAt": 1785000000000
                    }
                },
                "electron-owner-value:electron-workspace-root-labels:abc": {
                    "/Users/demo/app-c": "Label Only"
                }
            }"#,
        )
        .unwrap();

        let entries = scan_codex_dir(tmp.path()).unwrap();
        let find = |p: &str| {
            entries
                .iter()
                .find(|e| e.path == PathBuf::from(p))
                .expect("project should exist")
        };
        assert_eq!(
            find("/Users/demo/app-a").display_name.as_deref(),
            Some("SQLite Renamed")
        );
        assert_eq!(
            find("/Users/demo/app-b").display_name.as_deref(),
            Some("Global Renamed")
        );
        assert_eq!(
            find("/Users/demo/app-c").display_name.as_deref(),
            Some("Label Only")
        );
    }
}
