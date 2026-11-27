use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::i18n::Language;
use crate::model::AgentKind;
use crate::scanner::paths::{app_config_dir, dedup_key, normalize_path};

const APP_CONFIG_DIR_NAME: &str = "ProjPanel";
const CONFIG_FILE_NAME: &str = "config.json";

pub const MIN_UI_FONT_SIZE: u8 = 14;
pub const DEFAULT_UI_FONT_SIZE: u8 = 16;
pub const MAX_UI_FONT_SIZE: u8 = 20;

/// Persistent user configuration for ProjPanel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Custom project display names keyed by normalized deduplication path key.
    #[serde(default)]
    pub custom_names: HashMap<String, String>,
    /// Identifiers (see `AgentKind::id`) of agents excluded from scanning and display.
    #[serde(default)]
    pub disabled_agents: Vec<String>,
    /// Hidden projects: normalized deduplication key mapped to the original path text.
    #[serde(default)]
    pub hidden_projects: HashMap<String, String>,
    /// Interface language.
    #[serde(default)]
    pub language: Language,
    /// 界面基准字号，未设置时使用默认字号。
    #[serde(default)]
    pub ui_font_size: Option<u8>,
    /// 界面字体族名称，未设置时跟随系统。
    #[serde(default)]
    pub ui_font_family: Option<String>,
}

impl AppConfig {
    /// 读取界面基准字号，并限制在支持的范围内。
    pub fn effective_ui_font_size(&self) -> u8 {
        self.ui_font_size
            .unwrap_or(DEFAULT_UI_FONT_SIZE)
            .clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE)
    }

    /// Returns the default configuration file path for the current platform.
    pub fn default_path() -> Option<PathBuf> {
        app_config_dir(APP_CONFIG_DIR_NAME).map(|dir| dir.join(CONFIG_FILE_NAME))
    }

    /// Loads configuration from the default path, returning defaults if unavailable.
    pub fn load() -> Self {
        let Some(path) = Self::default_path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    /// Loads configuration from the specified path, returning defaults on error.
    pub fn load_from(path: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    /// Persists configuration to the default path.
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path()
            .context("unable to resolve platform configuration directory for ProjPanel")?;
        self.save_to(&path)
    }

    /// Persists configuration to the specified path, creating parent directories if needed.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let content =
            serde_json::to_string_pretty(self).context("failed to serialize ProjPanel config")?;
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, &content)
            .with_context(|| format!("failed to write temp config {}", tmp_path.display()))?;
        if let Err(err) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            fs::write(path, &content).with_context(|| {
                format!(
                    "failed to save config {} after rename error ({err})",
                    path.display()
                )
            })?;
        }
        Ok(())
    }

    /// Returns whether the agent is enabled for scanning and display (enabled by default).
    pub fn is_agent_enabled(&self, agent: AgentKind) -> bool {
        let id = agent.id();
        !self.disabled_agents.iter().any(|d| d == id)
    }

    /// Enables or disables an agent. Returns true when the stored state changed.
    pub fn set_agent_enabled(&mut self, agent: AgentKind, enabled: bool) -> bool {
        if self.is_agent_enabled(agent) == enabled {
            return false;
        }
        if enabled {
            let id = agent.id();
            self.disabled_agents.retain(|d| d != id);
        } else {
            self.disabled_agents.push(agent.id().to_string());
        }
        true
    }

    /// Returns whether the project path is in the hidden list.
    pub fn is_project_hidden(&self, path: &Path) -> bool {
        let key = dedup_key(&normalize_path(path));
        self.hidden_projects.contains_key(&key)
    }

    /// Adds a project to the hidden list. Returns true when newly added.
    pub fn hide_project(&mut self, path: &Path) -> bool {
        let norm = normalize_path(path);
        let key = dedup_key(&norm);
        self.hidden_projects
            .insert(key, norm.to_string_lossy().into_owned())
            .is_none()
    }

    /// Removes a project from the hidden list by its deduplication key. Returns true when removed.
    pub fn unhide_project_key(&mut self, key: &str) -> bool {
        self.hidden_projects.remove(key).is_some()
    }

    /// Looks up the custom display name for a project path.
    pub fn custom_name_for(&self, path: &Path) -> Option<String> {
        let key = dedup_key(&normalize_path(path));
        self.custom_names
            .get(&key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Sets or clears the custom display name for a project path.
    /// Returns true if the stored value changed.
    pub fn set_custom_name(&mut self, path: &Path, name: Option<&str>) -> bool {
        let key = dedup_key(&normalize_path(path));
        let cleaned = name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

        match cleaned {
            Some(val) => {
                if self.custom_names.get(&key).map(String::as_str) == Some(val.as_str()) {
                    false
                } else {
                    self.custom_names.insert(key, val);
                    true
                }
            }
            None => self.custom_names.remove(&key).is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn saves_loads_and_clears_custom_names() {
        let tmp = tempdir().unwrap();
        let cfg_path = tmp.path().join("config.json");
        let proj_path = PathBuf::from("/Users/demo/project-a/");

        let mut cfg = AppConfig::default();
        assert!(cfg.set_custom_name(&proj_path, Some("  My Custom App  ")));
        assert!(!cfg.set_custom_name(&proj_path, Some("My Custom App")));
        cfg.save_to(&cfg_path).unwrap();

        let loaded = AppConfig::load_from(&cfg_path);
        assert_eq!(
            loaded.custom_name_for(Path::new("/Users/demo/project-a")),
            Some("My Custom App".to_string())
        );

        let mut updated = loaded;
        assert!(updated.set_custom_name(&proj_path, Some("   ")));
        assert_eq!(updated.custom_name_for(&proj_path), None);
        updated.save_to(&cfg_path).unwrap();

        let reloaded = AppConfig::load_from(&cfg_path);
        assert!(reloaded.custom_names.is_empty());
    }

    #[test]
    fn persists_agent_switches_and_hidden_projects() {
        let tmp = tempdir().unwrap();
        let cfg_path = tmp.path().join("config.json");
        let proj = PathBuf::from("/Users/demo/project-b/");

        let mut cfg = AppConfig::default();
        assert!(cfg.is_agent_enabled(AgentKind::Codex));
        assert!(cfg.set_agent_enabled(AgentKind::Codex, false));
        assert!(!cfg.set_agent_enabled(AgentKind::Codex, false));
        assert!(cfg.hide_project(&proj));
        assert!(!cfg.hide_project(&proj));
        cfg.save_to(&cfg_path).unwrap();

        let mut loaded = AppConfig::load_from(&cfg_path);
        assert!(!loaded.is_agent_enabled(AgentKind::Codex));
        assert!(loaded.is_agent_enabled(AgentKind::ClaudeCode));
        assert!(loaded.is_project_hidden(Path::new("/Users/demo/project-b")));

        let key = loaded.hidden_projects.keys().next().cloned().unwrap();
        assert!(loaded.unhide_project_key(&key));
        assert!(loaded.set_agent_enabled(AgentKind::Codex, true));
        assert!(loaded.hidden_projects.is_empty() && loaded.disabled_agents.is_empty());
    }
}
