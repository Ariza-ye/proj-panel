use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{
    app_config_dir, file_mtime_secs, normalize_path, normalize_unix_timestamp,
};

const HOST_EDITORS: &[&str] = &["Code", "Cursor", "Windsurf", "Trae", "Kiro", "Antigravity"];

/// Scans Cline (`saoudrizwan.claude-dev`) and Roo Code (`rooveterinaryinc.roo-cline`)
/// task history inside VS Code-family editors' `globalStorage`.
pub struct ExtensionAgentScanner {
    agent: AgentKind,
    extension_id: &'static str,
}

impl ExtensionAgentScanner {
    pub fn cline() -> Self {
        Self {
            agent: AgentKind::Cline,
            extension_id: "saoudrizwan.claude-dev",
        }
    }

    pub fn roo_code() -> Self {
        Self {
            agent: AgentKind::RooCode,
            extension_id: "rooveterinaryinc.roo-cline",
        }
    }
}

impl AgentScanner for ExtensionAgentScanner {
    fn agent(&self) -> AgentKind {
        self.agent
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let mut by_path: HashMap<PathBuf, AgentProjectEntry> = HashMap::new();

        for host in HOST_EDITORS {
            let Some(base) = app_config_dir(host) else {
                continue;
            };
            let ext_dir = base
                .join("User")
                .join("globalStorage")
                .join(self.extension_id);
            if !ext_dir.is_dir() {
                continue;
            }
            let history_file = ext_dir.join("state").join("taskHistory.json");
            if history_file.is_file() {
                parse_task_history(&history_file, self.agent, host, &mut by_path);
            }
        }

        Ok(by_path.into_values().collect())
    }
}

fn parse_task_history(
    history_file: &Path,
    agent: AgentKind,
    host: &str,
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
) {
    let Ok(raw) = fs::read_to_string(history_file) else {
        return;
    };
    let Ok(Value::Array(tasks)) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let fallback_ts = file_mtime_secs(history_file);

    for task in tasks {
        let raw_dir = task
            .get("workspaceRoot")
            .or_else(|| task.get("cwd"))
            .or_else(|| task.get("workspace"))
            .and_then(|v| v.as_str());
        let Some(dir_str) = raw_dir else {
            continue;
        };
        let norm = normalize_path(Path::new(dir_str));
        if !norm.is_absolute() {
            continue;
        }
        let ts = task
            .get("ts")
            .or_else(|| task.get("timestamp"))
            .and_then(|v| v.as_i64())
            .and_then(normalize_unix_timestamp)
            .or(fallback_ts);

        by_path
            .entry(norm.clone())
            .and_modify(|existing| {
                existing.session_count = Some(existing.session_count.unwrap_or(0) + 1);
                if ts > existing.last_active {
                    existing.last_active = ts;
                }
            })
            .or_insert_with(|| AgentProjectEntry {
                agent,
                path: norm,
                display_name: None,
                last_active: ts,
                session_count: Some(1),
                source: format!("{host}/{}", agent.short_label()),
            });
    }
}
