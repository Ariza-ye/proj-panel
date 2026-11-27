pub mod claude_code;
pub mod cline_roo;
pub mod codex;
pub mod gemini;
pub mod jetbrains;
pub mod openchamber;
pub mod opencode;
pub mod paths;
pub mod vscode_family;
pub mod zed;

use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;

use crate::config::AppConfig;
use crate::model::{
    AgentKind, AgentProjectEntry, AgentSummary, AgentUsage, AggregatedProject, ScanReport,
};
use crate::scanner::paths::{
    dedup_key, format_display_path, is_noise_path, normalize_path, project_name_from_path,
};

/// Contract implemented by every AI agent project scanner.
pub trait AgentScanner: Send + Sync {
    fn agent(&self) -> AgentKind;
    fn scan(&self) -> Result<Vec<AgentProjectEntry>>;
}

/// Registry that executes all built-in agent scanners and aggregates their results.
pub struct ScannerRegistry {
    scanners: Vec<Box<dyn AgentScanner>>,
}

impl Default for ScannerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ScannerRegistry {
    pub fn new() -> Self {
        Self {
            scanners: vec![
                Box::new(claude_code::ClaudeCodeScanner),
                Box::new(codex::CodexScanner),
                Box::new(gemini::GeminiScanner),
                Box::new(opencode::OpenCodeScanner),
                Box::new(openchamber::OpenChamberScanner),
                Box::new(zed::ZedScanner),
                Box::new(vscode_family::VsCodeFamilyScanner::cursor()),
                Box::new(vscode_family::VsCodeFamilyScanner::windsurf()),
                Box::new(vscode_family::VsCodeFamilyScanner::vscode()),
                Box::new(vscode_family::VsCodeFamilyScanner::trae()),
                Box::new(vscode_family::VsCodeFamilyScanner::kiro()),
                Box::new(vscode_family::VsCodeFamilyScanner::antigravity()),
                Box::new(cline_roo::ExtensionAgentScanner::cline()),
                Box::new(cline_roo::ExtensionAgentScanner::roo_code()),
                Box::new(jetbrains::JetBrainsFamilyScanner::intellij_idea()),
                Box::new(jetbrains::JetBrainsFamilyScanner::pycharm()),
                Box::new(jetbrains::JetBrainsFamilyScanner::webstorm()),
                Box::new(jetbrains::JetBrainsFamilyScanner::goland()),
                Box::new(jetbrains::JetBrainsFamilyScanner::clion()),
                Box::new(jetbrains::JetBrainsFamilyScanner::phpstorm()),
                Box::new(jetbrains::JetBrainsFamilyScanner::rider()),
                Box::new(jetbrains::JetBrainsFamilyScanner::rustrover()),
                Box::new(jetbrains::JetBrainsFamilyScanner::rubymine()),
            ],
        }
    }

    /// Runs every registered scanner and aggregates all discovered projects into a single report,
    /// applying persisted ProjPanel custom names.
    pub fn scan_all(&self) -> ScanReport {
        let cfg = AppConfig::load();
        self.scan_all_with_config(&cfg)
    }

    /// Runs the enabled scanners and aggregates projects, applying custom names and
    /// dropping projects on the hidden list.
    pub fn scan_all_with_config(&self, cfg: &AppConfig) -> ScanReport {
        let started = Instant::now();
        let mut raw_entries: Vec<AgentProjectEntry> = Vec::new();
        let mut summaries: Vec<AgentSummary> = Vec::with_capacity(self.scanners.len());

        for scanner in &self.scanners {
            let agent = scanner.agent();
            if !cfg.is_agent_enabled(agent) {
                continue;
            }
            match scanner.scan() {
                Ok(entries) => {
                    let filtered: Vec<AgentProjectEntry> = entries
                        .into_iter()
                        .filter(|e| !is_noise_path(&e.path))
                        .collect();
                    let count = filtered.len();
                    raw_entries.extend(filtered);
                    summaries.push(AgentSummary {
                        agent,
                        project_count: count,
                        error: None,
                    });
                }
                Err(err) => {
                    summaries.push(AgentSummary {
                        agent,
                        project_count: 0,
                        error: Some(err.to_string()),
                    });
                }
            }
        }

        let mut projects = aggregate_entries_with_custom_names(raw_entries, &cfg.custom_names);
        projects.retain(|p| !cfg.is_project_hidden(&p.path));
        for summary in &mut summaries {
            if summary.error.is_none() {
                summary.project_count = projects
                    .iter()
                    .filter(|p| p.has_agent(summary.agent))
                    .count();
            }
        }
        ScanReport::from_results(projects, summaries, started.elapsed())
    }
}

/// Deduplicates raw entries by normalized path and merges per-agent usage metadata.
#[cfg(test)]
pub fn aggregate_entries(entries: Vec<AgentProjectEntry>) -> Vec<AggregatedProject> {
    aggregate_entries_with_custom_names(entries, &HashMap::new())
}

/// Deduplicates raw entries by normalized path, preserves per-agent custom names,
/// and applies optional ProjPanel custom name overrides keyed by `dedup_key`.
pub fn aggregate_entries_with_custom_names(
    entries: Vec<AgentProjectEntry>,
    custom_names: &HashMap<String, String>,
) -> Vec<AggregatedProject> {
    let mut grouped: HashMap<String, AggregatedProject> = HashMap::new();

    for entry in entries {
        let norm = normalize_path(&entry.path);
        if is_noise_path(&norm) {
            continue;
        }
        let key = dedup_key(&norm);
        let cleaned_display_name = entry
            .display_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let usage = AgentUsage {
            agent: entry.agent,
            display_name: cleaned_display_name,
            last_active: entry.last_active,
            session_count: entry.session_count,
            source: entry.source,
        };

        grouped
            .entry(key)
            .and_modify(|proj| {
                if let Some(existing_usage) =
                    proj.agents.iter_mut().find(|u| u.agent == usage.agent)
                {
                    let is_newer = usage.last_active > existing_usage.last_active;
                    if is_newer {
                        existing_usage.last_active = usage.last_active;
                    }
                    if usage.display_name.is_some()
                        && (existing_usage.display_name.is_none() || is_newer)
                    {
                        existing_usage.display_name = usage.display_name.clone();
                    }
                    existing_usage.session_count =
                        match (existing_usage.session_count, usage.session_count) {
                            (Some(a), Some(b)) => Some(a.saturating_add(b)),
                            (a, b) => a.or(b),
                        };
                } else {
                    proj.agents.push(usage.clone());
                }
                if usage.last_active > proj.last_active {
                    proj.last_active = usage.last_active;
                }
            })
            .or_insert_with(|| {
                let exists = norm.exists();
                let default_name = project_name_from_path(&norm);
                let display_path = format_display_path(&norm);
                let last_active = usage.last_active;
                AggregatedProject {
                    name: default_name.clone(),
                    default_name,
                    custom_name: None,
                    external_names: Vec::new(),
                    path: norm,
                    display_path,
                    exists,
                    agents: vec![usage],
                    last_active,
                }
            });
    }

    let mut projects: Vec<AggregatedProject> = grouped
        .into_iter()
        .map(|(key, mut proj)| {
            proj.agents.sort_by_key(|u| u.agent);
            let custom = custom_names.get(&key).cloned();
            proj.resolve_display_names(custom);
            proj
        })
        .collect();

    projects.sort_by(|a, b| {
        b.exists
            .cmp(&a.exists)
            .then_with(|| b.last_active.cmp(&a.last_active))
            .then_with(|| b.agents.len().cmp(&a.agents.len()))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    projects
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn aggregates_same_project_across_multiple_agents() {
        let entries = vec![
            AgentProjectEntry {
                agent: AgentKind::ClaudeCode,
                path: PathBuf::from("/tmp/demo-proj/"),
                display_name: None,
                last_active: Some(1_700_000_100),
                session_count: Some(3),
                source: "claude".into(),
            },
            AgentProjectEntry {
                agent: AgentKind::Codex,
                path: PathBuf::from("/tmp/demo-proj"),
                display_name: None,
                last_active: Some(1_700_000_500),
                session_count: Some(2),
                source: "codex".into(),
            },
        ];

        let projects = aggregate_entries(entries);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].agents.len(), 2);
        assert_eq!(projects[0].last_active, Some(1_700_000_500));
        assert_eq!(projects[0].name, "demo-proj");
        assert!(!projects[0].should_show_name_dropdown());
    }

    #[test]
    fn resolves_custom_and_external_display_names_with_priority() {
        let path = PathBuf::from("/tmp/thxpace-admin");
        let entries = vec![
            AgentProjectEntry {
                agent: AgentKind::ClaudeCode,
                path: path.clone(),
                display_name: None,
                last_active: Some(1_700_000_900),
                session_count: Some(1),
                source: "claude".into(),
            },
            AgentProjectEntry {
                agent: AgentKind::GeminiCli,
                path: path.clone(),
                display_name: Some("THXPACE-ADMIN".into()),
                last_active: Some(1_700_000_800),
                session_count: None,
                source: "gemini".into(),
            },
            AgentProjectEntry {
                agent: AgentKind::OpenChamber,
                path: path.clone(),
                display_name: Some("福建海丝admin".into()),
                last_active: Some(1_700_000_600),
                session_count: None,
                source: "openchamber".into(),
            },
            AgentProjectEntry {
                agent: AgentKind::Codex,
                path: path.clone(),
                display_name: Some("海丝-admin".into()),
                last_active: Some(1_700_000_700),
                session_count: Some(2),
                source: "codex".into(),
            },
        ];

        // Without ProjPanel custom name: picks the most recently active external name (Codex)
        // and filters out Gemini's case-only default name match.
        let projects = aggregate_entries(entries.clone());
        assert_eq!(projects.len(), 1);
        let proj = &projects[0];
        assert_eq!(proj.name, "海丝-admin");
        assert_eq!(proj.default_name, "thxpace-admin");
        assert_eq!(proj.external_names.len(), 2);
        assert_eq!(proj.external_names[0].agent, AgentKind::Codex);
        assert_eq!(proj.external_names[1].agent, AgentKind::OpenChamber);
        assert!(proj.should_show_name_dropdown());

        // With ProjPanel custom name: takes top priority and keeps dropdown available.
        let mut custom = HashMap::new();
        custom.insert(dedup_key(&path), "我的海丝后台".to_string());
        let with_custom = aggregate_entries_with_custom_names(entries, &custom);
        assert_eq!(with_custom[0].name, "我的海丝后台");
        assert_eq!(with_custom[0].custom_name.as_deref(), Some("我的海丝后台"));
        assert!(with_custom[0].should_show_name_dropdown());
    }

    #[test]
    fn dropdown_hidden_when_only_one_unique_external_name_and_no_custom_name() {
        let path = PathBuf::from("/tmp/service-a");
        let entries = vec![
            AgentProjectEntry {
                agent: AgentKind::Codex,
                path: path.clone(),
                display_name: Some("统一名称".into()),
                last_active: Some(100),
                session_count: None,
                source: "codex".into(),
            },
            AgentProjectEntry {
                agent: AgentKind::OpenChamber,
                path: path.clone(),
                display_name: Some("统一名称".into()),
                last_active: Some(90),
                session_count: None,
                source: "openchamber".into(),
            },
        ];

        let projects = aggregate_entries(entries.clone());
        assert_eq!(projects[0].name, "统一名称");
        assert!(!projects[0].should_show_name_dropdown());

        let mut custom = HashMap::new();
        custom.insert(dedup_key(&path), "本地命名".to_string());
        let with_custom = aggregate_entries_with_custom_names(entries, &custom);
        assert_eq!(with_custom[0].name, "本地命名");
        assert!(with_custom[0].should_show_name_dropdown());
    }
}
