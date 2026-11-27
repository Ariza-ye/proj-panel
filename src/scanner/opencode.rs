use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{
    file_mtime_secs, normalize_path, normalize_unix_timestamp, xdg_data_candidates,
};

/// Scans OpenCode SQLite database (`opencode.db`) and legacy JSON storage (`storage/project/*.json`).
pub struct OpenCodeScanner;

impl AgentScanner for OpenCodeScanner {
    fn agent(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let mut by_path: HashMap<PathBuf, AgentProjectEntry> = HashMap::new();
        for base in xdg_data_candidates("opencode") {
            if !base.is_dir() {
                continue;
            }
            scan_opencode_dir(&base, &mut by_path);
        }
        Ok(by_path.into_values().collect())
    }
}

pub(crate) fn scan_opencode_dir(base: &Path, by_path: &mut HashMap<PathBuf, AgentProjectEntry>) {
    // 1. Primary source: SQLite `opencode.db` (`project` table).
    let db_path = base.join("opencode.db");
    if db_path.is_file() {
        let _ = read_opencode_sqlite(&db_path, by_path);
    }

    // 2. Fallback / legacy JSON storage: `storage/project/*.json`.
    let storage_dir = base.join("storage").join("project");
    if storage_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&storage_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(raw) = fs::read_to_string(&p) {
                    if let Ok(val) = serde_json::from_str::<Value>(&raw) {
                        if let Some(worktree) = val.get("worktree").and_then(|v| v.as_str()) {
                            let norm = normalize_path(Path::new(worktree));
                            if !norm.is_absolute() {
                                continue;
                            }
                            let ts = val
                                .get("time")
                                .and_then(|t| t.get("updated").or_else(|| t.get("created")))
                                .and_then(|v| v.as_i64())
                                .and_then(normalize_unix_timestamp)
                                .or_else(|| file_mtime_secs(&p));
                            let name = val
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .filter(|s| !s.is_empty());

                            upsert_opencode_entry(
                                by_path,
                                norm,
                                name,
                                ts,
                                "opencode/storage/project",
                            );
                        }
                    }
                }
            }
        }
    }
}

fn read_opencode_sqlite(
    db_path: &Path,
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
) -> Result<()> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db_path, flags)?;
    let _ = conn.busy_timeout(Duration::from_millis(200));

    let mut stmt = conn.prepare(
        "SELECT worktree, name, time_updated, time_created FROM project WHERE worktree IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        let worktree: String = row.get(0)?;
        let name: Option<String> = row.get(1)?;
        let updated: Option<i64> = row.get(2)?;
        let created: Option<i64> = row.get(3)?;
        Ok((worktree, name, updated, created))
    })?;

    for row in rows.flatten() {
        let (worktree, name, updated, created) = row;
        let norm = normalize_path(Path::new(&worktree));
        if !norm.is_absolute() {
            continue;
        }
        let ts = updated
            .and_then(normalize_unix_timestamp)
            .or_else(|| created.and_then(normalize_unix_timestamp));
        let clean_name = name.filter(|s| !s.trim().is_empty());
        upsert_opencode_entry(by_path, norm, clean_name, ts, "opencode.db");
    }

    Ok(())
}

fn upsert_opencode_entry(
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
    path: PathBuf,
    display_name: Option<String>,
    last_active: Option<i64>,
    source: &str,
) {
    by_path
        .entry(path.clone())
        .and_modify(|existing| {
            let is_newer = last_active > existing.last_active;
            if is_newer {
                existing.last_active = last_active;
            }
            if display_name.is_some() && (existing.display_name.is_none() || is_newer) {
                existing.display_name = display_name.clone();
            }
        })
        .or_insert_with(|| AgentProjectEntry {
            agent: AgentKind::OpenCode,
            path,
            display_name,
            last_active,
            session_count: None,
            source: source.to_string(),
        });
}
