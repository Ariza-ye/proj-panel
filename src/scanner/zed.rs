use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{
    app_config_dir, app_local_data_dir, normalize_path, parse_timestamp_str,
};

/// Scans Zed editor SQLite workspace databases (`db/0-stable/db.sqlite`, `db/0-preview/db.sqlite`).
pub struct ZedScanner;

impl AgentScanner for ZedScanner {
    fn agent(&self) -> AgentKind {
        AgentKind::Zed
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let mut candidates = Vec::new();
        // macOS: ~/Library/Application Support/Zed/db
        if let Some(cfg) = app_config_dir("Zed") {
            candidates.push(cfg.join("db"));
        }
        // Linux: ~/.local/share/zed/db, Windows: %LOCALAPPDATA%/Zed/db
        for app_name in ["zed", "Zed"] {
            if let Some(local) = app_local_data_dir(app_name) {
                let db_dir = local.join("db");
                if !candidates.contains(&db_dir) {
                    candidates.push(db_dir);
                }
            }
        }

        let mut by_path: HashMap<PathBuf, AgentProjectEntry> = HashMap::new();
        for db_root in candidates {
            for channel in ["0-stable", "0-preview", "0-nightly", "0-dev"] {
                let db_file = db_root.join(channel).join("db.sqlite");
                if db_file.is_file() {
                    let _ = read_zed_db(&db_file, channel, &mut by_path);
                }
            }
        }

        Ok(by_path.into_values().collect())
    }
}

pub(crate) fn read_zed_db(
    db_path: &Path,
    channel: &str,
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
) -> Result<()> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db_path, flags)?;
    let _ = conn.busy_timeout(Duration::from_millis(200));

    let mut stmt = conn.prepare(
        "SELECT paths, timestamp FROM workspaces \
         WHERE paths IS NOT NULL AND paths != '' AND remote_connection_id IS NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        let paths: String = row.get(0)?;
        let ts: Option<String> = row.get(1)?;
        Ok((paths, ts))
    })?;

    for row in rows.flatten() {
        let (paths_blob, ts_str) = row;
        let ts = ts_str.as_deref().and_then(parse_timestamp_str);
        // Zed stores multi-root workspace paths separated by newlines.
        for raw_line in paths_blob.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let norm = normalize_path(Path::new(trimmed));
            if !norm.is_absolute() {
                continue;
            }
            by_path
                .entry(norm.clone())
                .and_modify(|existing| {
                    if ts > existing.last_active {
                        existing.last_active = ts;
                    }
                })
                .or_insert_with(|| AgentProjectEntry {
                    agent: AgentKind::Zed,
                    path: norm,
                    display_name: None,
                    last_active: ts,
                    session_count: None,
                    source: format!("Zed/{channel}/db.sqlite"),
                });
        }
    }

    Ok(())
}
