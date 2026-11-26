use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::scanner::paths::{app_config_dir, dedup_key, normalize_path};

const APP_CONFIG_DIR_NAME: &str = "ProjPanel";
const CONFIG_FILE_NAME: &str = "config.json";

/// Persistent user configuration for ProjPanel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Custom project display names keyed by normalized deduplication path key.
    #[serde(default)]
    pub custom_names: HashMap<String, String>,
}

impl AppConfig {
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
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("failed to serialize ProjPanel config")?;
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
        let cleaned = name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

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
}
