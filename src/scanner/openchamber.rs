use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{
    app_config_dir, file_mtime_secs, home_dir, normalize_path, normalize_unix_timestamp,
};

const ARCHIVED_KEY_PREFIX: &str = "__archived__:";

/// Scans OpenChamber configuration (`settings.json`), per-project configs (`projects/*.json`),
/// session folder groupings (`sessions-directories.json`), and enriches discovered projects
/// with session counts and timestamps from `message-search.sqlite`.
pub struct OpenChamberScanner;

impl AgentScanner for OpenChamberScanner {
    fn agent(&self) -> AgentKind {
        AgentKind::OpenChamber
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let mut by_path: HashMap<PathBuf, AgentProjectEntry> = HashMap::new();
        for base in openchamber_config_candidates() {
            if !base.is_dir() {
                continue;
            }
            scan_openchamber_dir(&base, &mut by_path);
        }
        Ok(by_path.into_values().collect())
    }
}

fn openchamber_config_candidates() -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    let mut push_unique = |p: PathBuf| {
        if !dirs_out.contains(&p) {
            dirs_out.push(p);
        }
    };

    if let Some(custom) = env::var_os("OPENCHAMBER_DATA_DIR") {
        let trimmed = custom.to_string_lossy().trim().to_string();
        if !trimmed.is_empty() {
            push_unique(PathBuf::from(trimmed));
        }
    }

    if let Some(home) = home_dir() {
        push_unique(home.join(".config").join("openchamber"));
    }

    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        let trimmed = xdg.to_string_lossy().trim().to_string();
        if !trimmed.is_empty() {
            push_unique(PathBuf::from(trimmed).join("openchamber"));
        }
    }

    if let Some(cfg) = app_config_dir("openchamber") {
        push_unique(cfg);
    }

    dirs_out
}

pub(crate) fn scan_openchamber_dir(base: &Path, by_path: &mut HashMap<PathBuf, AgentProjectEntry>) {
    // 1. Primary project registry: `settings.json` (`projects` array and `lastDirectory`).
    let settings_file = base.join("settings.json");
    if settings_file.is_file() {
        scan_settings_json(&settings_file, by_path);
    }

    // 2. Per-project settings files: `projects/*.json`.
    let projects_dir = base.join("projects");
    if projects_dir.is_dir() {
        scan_projects_dir(&projects_dir, by_path);
    }

    // 3. Session directory groupings: `sessions-directories.json` (`foldersMap`).
    let session_dirs_file = base.join("sessions-directories.json");
    if session_dirs_file.is_file() {
        scan_sessions_directories_json(&session_dirs_file, by_path);
    }

    // 4. Enrich discovered OpenChamber projects with session count and activity timestamps
    //    from `message-search.sqlite` without importing unrelated OpenCode global sessions.
    let search_db = base.join("message-search.sqlite");
    if search_db.is_file() && !by_path.is_empty() {
        let _ = enrich_from_message_search_db(&search_db, by_path);
    }
}

fn scan_settings_json(settings_file: &Path, by_path: &mut HashMap<PathBuf, AgentProjectEntry>) {
    let fallback_mtime = file_mtime_secs(settings_file);
    let Ok(raw) = fs::read_to_string(settings_file) else {
        return;
    };
    let Ok(root) = serde_json::from_str::<Value>(&raw) else {
        return;
    };

    if let Some(projects) = root.get("projects").and_then(|v| v.as_array()) {
        for item in projects {
            let Some(path_str) = item.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            let norm = normalize_path(Path::new(path_str));
            if !norm.is_absolute() {
                continue;
            }
            let display_name = item
                .get("label")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let last_active = item
                .get("lastOpenedAt")
                .and_then(|v| v.as_i64())
                .and_then(normalize_unix_timestamp)
                .or_else(|| {
                    item.get("addedAt")
                        .and_then(|v| v.as_i64())
                        .and_then(normalize_unix_timestamp)
                })
                .or(fallback_mtime);

            upsert_entry(
                by_path,
                norm,
                display_name,
                last_active,
                None,
                "openchamber/settings.json",
            );
        }
    }

    if let Some(last_dir) = root.get("lastDirectory").and_then(|v| v.as_str()) {
        let norm = normalize_path(Path::new(last_dir));
        if norm.is_absolute() {
            upsert_entry(
                by_path,
                norm,
                None,
                fallback_mtime,
                None,
                "openchamber/settings.json",
            );
        }
    }
}

fn scan_projects_dir(projects_dir: &Path, by_path: &mut HashMap<PathBuf, AgentProjectEntry>) {
    let Ok(entries) = fs::read_dir(projects_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let mtime = file_mtime_secs(&path);

        let mut project_path: Option<PathBuf> = None;
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(val) = serde_json::from_str::<Value>(&raw)
            && let Some(raw_p) = val.get("projectPath").and_then(|v| v.as_str())
        {
            let norm = normalize_path(Path::new(raw_p));
            if norm.is_absolute() {
                project_path = Some(norm);
            }
        }

        if project_path.is_none()
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && let Some(decoded) = decode_project_id_path(stem)
        {
            let norm = normalize_path(&decoded);
            if norm.is_absolute() {
                project_path = Some(norm);
            }
        }

        if let Some(norm) = project_path {
            upsert_entry(by_path, norm, None, mtime, None, "openchamber/projects");
        }
    }
}

fn scan_sessions_directories_json(
    file_path: &Path,
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
) {
    let fallback_mtime = file_mtime_secs(file_path);
    let Ok(raw) = fs::read_to_string(file_path) else {
        return;
    };
    let Ok(root) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(folders_map) = root.get("foldersMap").and_then(|v| v.as_object()) else {
        return;
    };

    let mut sessions_by_dir: HashMap<PathBuf, (HashSet<String>, Option<i64>)> = HashMap::new();

    for (raw_key, folders_val) in folders_map {
        let dir_str = raw_key
            .strip_prefix(ARCHIVED_KEY_PREFIX)
            .unwrap_or(raw_key.as_str());
        let norm = normalize_path(Path::new(dir_str));
        if !norm.is_absolute() {
            continue;
        }

        let entry = sessions_by_dir
            .entry(norm)
            .or_insert_with(|| (HashSet::new(), None));

        if let Some(folders) = folders_val.as_array() {
            for folder in folders {
                if let Some(ts) = folder
                    .get("createdAt")
                    .and_then(|v| v.as_i64())
                    .and_then(normalize_unix_timestamp)
                    && entry.1.is_none_or(|cur| ts > cur)
                {
                    entry.1 = Some(ts);
                }
                if let Some(session_ids) = folder.get("sessionIds").and_then(|v| v.as_array()) {
                    for sid in session_ids.iter().filter_map(|v| v.as_str()) {
                        if !sid.is_empty() {
                            entry.0.insert(sid.to_string());
                        }
                    }
                }
            }
        }
    }

    for (norm, (session_ids, ts)) in sessions_by_dir {
        let count = if session_ids.is_empty() {
            None
        } else {
            Some(session_ids.len() as u32)
        };
        upsert_entry(
            by_path,
            norm,
            None,
            ts.or(fallback_mtime),
            count,
            "openchamber/sessions-directories.json",
        );
    }
}

fn enrich_from_message_search_db(
    db_path: &Path,
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
) -> Result<()> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db_path, flags)?;
    let _ = conn.busy_timeout(Duration::from_millis(200));

    let mut stmt = conn.prepare(
        "SELECT directory, COUNT(*), MAX(COALESCE(synced_updated_at, created_at)) \
         FROM sessions \
         WHERE directory IS NOT NULL AND directory != '' \
         GROUP BY directory",
    )?;

    let rows = stmt.query_map([], |row| {
        let dir: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        let latest_ts: Option<i64> = row.get(2)?;
        Ok((dir, count, latest_ts))
    })?;

    for row in rows.flatten() {
        let (dir, count, latest_ts) = row;
        let norm = normalize_path(Path::new(&dir));
        let Some(existing) = by_path.get_mut(&norm) else {
            continue;
        };

        if let Some(ts) = latest_ts.and_then(normalize_unix_timestamp)
            && existing.last_active.is_none_or(|cur| ts > cur)
        {
            existing.last_active = Some(ts);
        }
        if count > 0 {
            let count_u32 = count.min(u32::MAX as i64) as u32;
            existing.session_count = Some(
                existing
                    .session_count
                    .map_or(count_u32, |c| c.max(count_u32)),
            );
        }
    }

    Ok(())
}

fn upsert_entry(
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
    path: PathBuf,
    display_name: Option<String>,
    last_active: Option<i64>,
    session_count: Option<u32>,
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
            if let Some(n) = session_count {
                existing.session_count = Some(existing.session_count.map_or(n, |cur| cur.max(n)));
            }
        })
        .or_insert_with(|| AgentProjectEntry {
            agent: AgentKind::OpenChamber,
            path,
            display_name,
            last_active,
            session_count,
            source: source.to_string(),
        });
}

/// Decodes OpenChamber's `path_<base64url>` project identifier back to a filesystem path.
fn decode_project_id_path(id: &str) -> Option<PathBuf> {
    let encoded = id.strip_prefix("path_")?;
    let bytes = decode_base64url(encoded)?;
    let s = String::from_utf8(bytes).ok()?;
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

fn decode_base64url(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity((input.len() * 3) / 4);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;

    for b in input.bytes() {
        if b == b'=' {
            break;
        }
        let val: u32 = match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn decodes_base64url_project_id() {
        let decoded = decode_project_id_path("path_L1VzZXJzL2RlbW8vYXBw").unwrap();
        assert_eq!(decoded, PathBuf::from("/Users/demo/app"));
        assert!(decode_project_id_path("other_id").is_none());
    }

    #[test]
    fn scans_settings_projects_session_dirs_and_sqlite() {
        let tmp = tempdir().unwrap();

        fs::write(
            tmp.path().join("settings.json"),
            r#"{
                "projects": [
                    {
                        "id": "path_L1VzZXJzL2RlbW8vYXBw",
                        "path": "/Users/demo/app",
                        "label": "Demo App",
                        "addedAt": 1770000000000,
                        "lastOpenedAt": 1770000500000
                    }
                ],
                "lastDirectory": "/Users/demo/recent"
            }"#,
        )
        .unwrap();

        let projects_dir = tmp.path().join("projects");
        fs::create_dir_all(&projects_dir).unwrap();
        fs::write(
            projects_dir.join("path_L1VzZXJzL2RlbW8vY29uZmln.json"),
            r#"{"projectPath": "/Users/demo/config"}"#,
        )
        .unwrap();
        // Encodes `/Users/demo/legacy` via base64url without an explicit projectPath field.
        fs::write(
            projects_dir.join("path_L1VzZXJzL2RlbW8vbGVnYWN5.json"),
            r#"{"version": 1, "scheduledTasks": []}"#,
        )
        .unwrap();

        fs::write(
            tmp.path().join("sessions-directories.json"),
            r#"{
                "version": 1,
                "foldersMap": {
                    "__archived__:/Users/demo/archived": [
                        {
                            "id": "f1",
                            "name": "root",
                            "sessionIds": ["s1", "s2", "s3"],
                            "createdAt": 1770000100000
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let db_path = tmp.path().join("message-search.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0,
                synced_updated_at INTEGER
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, directory, created_at, synced_updated_at) VALUES
                ('s1', '/Users/demo/app', 1770000000000, 1770000900000),
                ('s2', '/Users/demo/app', 1770000100000, 1770000800000),
                ('s3', '/Users/demo/unrelated-opencode', 1770000200000, 1770000999000)",
            [],
        )
        .unwrap();
        drop(conn);

        let mut by_path = HashMap::new();
        scan_openchamber_dir(tmp.path(), &mut by_path);

        assert_eq!(by_path.len(), 5);
        assert!(!by_path.contains_key(&PathBuf::from("/Users/demo/unrelated-opencode")));

        let app = by_path.get(&PathBuf::from("/Users/demo/app")).unwrap();
        assert_eq!(app.display_name.as_deref(), Some("Demo App"));
        assert_eq!(app.session_count, Some(2));
        assert_eq!(app.last_active, Some(1_770_000_900));

        let archived = by_path.get(&PathBuf::from("/Users/demo/archived")).unwrap();
        assert_eq!(archived.session_count, Some(3));
        assert_eq!(archived.last_active, Some(1_770_000_100));

        assert!(by_path.contains_key(&PathBuf::from("/Users/demo/recent")));
        assert!(by_path.contains_key(&PathBuf::from("/Users/demo/config")));
        assert!(by_path.contains_key(&PathBuf::from("/Users/demo/legacy")));
    }
}
