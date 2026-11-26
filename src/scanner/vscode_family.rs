use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{app_config_dir, decode_file_uri, file_mtime_secs, normalize_path};

/// Unified scanner for VS Code and VS Code-derived AI IDEs (Cursor, Windsurf, Trae, Kiro, Antigravity).
pub struct VsCodeFamilyScanner {
    agent: AgentKind,
    app_dir_names: &'static [&'static str],
}

impl VsCodeFamilyScanner {
    pub fn cursor() -> Self {
        Self {
            agent: AgentKind::Cursor,
            app_dir_names: &["Cursor"],
        }
    }

    pub fn windsurf() -> Self {
        Self {
            agent: AgentKind::Windsurf,
            app_dir_names: &["Windsurf"],
        }
    }

    pub fn vscode() -> Self {
        Self {
            agent: AgentKind::VsCode,
            app_dir_names: &["Code", "Code - Insiders"],
        }
    }

    pub fn trae() -> Self {
        Self {
            agent: AgentKind::Trae,
            app_dir_names: &["Trae"],
        }
    }

    pub fn kiro() -> Self {
        Self {
            agent: AgentKind::Kiro,
            app_dir_names: &["Kiro"],
        }
    }

    pub fn antigravity() -> Self {
        Self {
            agent: AgentKind::Antigravity,
            app_dir_names: &["Antigravity"],
        }
    }
}

impl AgentScanner for VsCodeFamilyScanner {
    fn agent(&self) -> AgentKind {
        self.agent
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let mut by_path: HashMap<PathBuf, AgentProjectEntry> = HashMap::new();

        for app_dir in self.app_dir_names {
            let Some(base) = app_config_dir(app_dir) else {
                continue;
            };
            let ws_storage = base.join("User").join("workspaceStorage");
            if ws_storage.is_dir() {
                scan_workspace_storage(&ws_storage, self.agent, app_dir, &mut by_path);
            }
        }

        Ok(by_path.into_values().collect())
    }
}

pub(crate) fn scan_workspace_storage(
    ws_storage: &Path,
    agent: AgentKind,
    app_label: &str,
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
) {
    let Ok(entries) = fs::read_dir(ws_storage) else {
        return;
    };

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let ws_json = dir.join("workspace.json");
        if !ws_json.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&ws_json) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };

        // Prefer state.vscdb mtime for most recent workspace activity; fall back to workspace.json.
        let state_db = dir.join("state.vscdb");
        let last_active = file_mtime_secs(&state_db)
            .or_else(|| file_mtime_secs(&ws_json))
            .or_else(|| file_mtime_secs(&dir));

        let mut raw_entries: Vec<(PathBuf, Option<String>)> = Vec::new();
        if let Some(folder_uri) = val.get("folder").and_then(|v| v.as_str()) {
            if let Some(p) = decode_file_uri(folder_uri) {
                raw_entries.push((p, None));
            }
        } else if let Some(ws_uri) = val.get("workspace").and_then(|v| v.as_str()) {
            if let Some(ws_file) = decode_file_uri(ws_uri) {
                // Multi-root `.code-workspace` file: parse its `folders` if readable,
                // otherwise fall back to its parent directory.
                let expanded = parse_code_workspace_file(&ws_file);
                if expanded.is_empty() {
                    if let Some(parent) = ws_file.parent() {
                        raw_entries.push((parent.to_path_buf(), None));
                    }
                } else {
                    raw_entries.extend(expanded);
                }
            }
        }

        for (raw_p, folder_name) in raw_entries {
            let norm = normalize_path(&raw_p);
            if !norm.is_absolute() {
                continue;
            }
            by_path
                .entry(norm.clone())
                .and_modify(|existing| {
                    let is_newer = last_active > existing.last_active;
                    if is_newer {
                        existing.last_active = last_active;
                    }
                    if folder_name.is_some()
                        && (existing.display_name.is_none() || is_newer)
                    {
                        existing.display_name = folder_name.clone();
                    }
                })
                .or_insert_with(|| AgentProjectEntry {
                    agent,
                    path: norm,
                    display_name: folder_name,
                    last_active,
                    session_count: None,
                    source: format!("{app_label}/workspaceStorage"),
                });
        }
    }
}

fn parse_code_workspace_file(ws_file: &Path) -> Vec<(PathBuf, Option<String>)> {
    let Ok(raw) = fs::read_to_string(ws_file) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let Some(folders) = val.get("folders").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let base_dir = ws_file.parent();
    let mut out = Vec::new();
    for folder in folders {
        let name = folder
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(p_str) = folder.get("path").and_then(|v| v.as_str()) {
            let p = PathBuf::from(p_str);
            if p.is_absolute() {
                out.push((p, name));
            } else if let Some(base) = base_dir {
                out.push((base.join(p), name));
            }
        } else if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
            if let Some(p) = decode_file_uri(uri) {
                out.push((p, name));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_vscode_workspace_storage() {
        let tmp = tempdir().unwrap();
        let slot = tmp.path().join("abc123");
        fs::create_dir_all(&slot).unwrap();
        fs::write(
            slot.join("workspace.json"),
            r#"{"folder":"file:///Users/demo/sample%20repo"}"#,
        )
        .unwrap();

        let mut map = HashMap::new();
        scan_workspace_storage(tmp.path(), AgentKind::Cursor, "Cursor", &mut map);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&PathBuf::from("/Users/demo/sample repo")));
    }

    #[test]
    fn parses_custom_folder_names_in_code_workspace() {
        let tmp = tempdir().unwrap();
        let ws_file = tmp.path().join("demo.code-workspace");
        let repo_dir = tmp.path().join("backend");
        fs::create_dir_all(&repo_dir).unwrap();
        fs::write(
            &ws_file,
            r#"{"folders":[{"path":"backend","name":"Backend Service"}]}"#,
        )
        .unwrap();

        let slot = tmp.path().join("slot1");
        fs::create_dir_all(&slot).unwrap();
        let ws_uri = format!("file://{}", ws_file.to_string_lossy());
        fs::write(
            slot.join("workspace.json"),
            serde_json::json!({"workspace": ws_uri}).to_string(),
        )
        .unwrap();

        let mut map = HashMap::new();
        scan_workspace_storage(tmp.path(), AgentKind::VsCode, "Code", &mut map);
        let entry = map.get(&normalize_path(&repo_dir)).unwrap();
        assert_eq!(entry.display_name.as_deref(), Some("Backend Service"));
    }
}
