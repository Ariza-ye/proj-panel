use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

/// Supported AI agent and AI-assisted editor identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    GeminiCli,
    OpenCode,
    OpenChamber,
    Zed,
    Cursor,
    Windsurf,
    VsCode,
    Trae,
    Kiro,
    Antigravity,
    Cline,
    RooCode,
    IntelliJIdea,
    PyCharm,
    WebStorm,
    GoLand,
    CLion,
    PhpStorm,
    Rider,
    RustRover,
    RubyMine,
}

impl AgentKind {
    /// Human-readable display name shown in the UI and CLI output.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::GeminiCli => "Gemini CLI",
            Self::OpenCode => "OpenCode",
            Self::OpenChamber => "OpenChamber",
            Self::Zed => "Zed",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::VsCode => "VS Code",
            Self::Trae => "Trae",
            Self::Kiro => "Kiro",
            Self::Antigravity => "Antigravity",
            Self::Cline => "Cline",
            Self::RooCode => "Roo Code",
            Self::IntelliJIdea => "IntelliJ IDEA",
            Self::PyCharm => "PyCharm",
            Self::WebStorm => "WebStorm",
            Self::GoLand => "GoLand",
            Self::CLion => "CLion",
            Self::PhpStorm => "PhpStorm",
            Self::Rider => "Rider",
            Self::RustRover => "RustRover",
            Self::RubyMine => "RubyMine",
        }
    }

    /// Short badge label for compact UI chips.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude",
            Self::Codex => "Codex",
            Self::GeminiCli => "Gemini",
            Self::OpenCode => "OpenCode",
            Self::OpenChamber => "OpenChamber",
            Self::Zed => "Zed",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::VsCode => "VS Code",
            Self::Trae => "Trae",
            Self::Kiro => "Kiro",
            Self::Antigravity => "Antigravity",
            Self::Cline => "Cline",
            Self::RooCode => "Roo",
            Self::IntelliJIdea => "IDEA",
            Self::PyCharm => "PyCharm",
            Self::WebStorm => "WebStorm",
            Self::GoLand => "GoLand",
            Self::CLion => "CLion",
            Self::PhpStorm => "PhpStorm",
            Self::Rider => "Rider",
            Self::RustRover => "RustRover",
            Self::RubyMine => "RubyMine",
        }
    }
}

/// Raw project entry reported by an individual agent scanner before aggregation.
#[derive(Debug, Clone)]
pub struct AgentProjectEntry {
    pub agent: AgentKind,
    pub path: PathBuf,
    pub display_name: Option<String>,
    /// Unix timestamp in seconds when this project was last touched by the agent.
    pub last_active: Option<i64>,
    pub session_count: Option<u32>,
    /// Diagnostic label indicating which config file or database produced this entry.
    pub source: String,
}

/// Per-agent usage metadata attached to an aggregated project.
#[derive(Debug, Clone, Serialize)]
pub struct AgentUsage {
    pub agent: AgentKind,
    pub display_name: Option<String>,
    pub last_active: Option<i64>,
    pub session_count: Option<u32>,
    pub source: String,
}

/// Custom project name assigned inside an external editor or AI agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalProjectName {
    pub agent: AgentKind,
    pub name: String,
}

/// Unified project record deduplicated by normalized filesystem path across all agents.
#[derive(Debug, Clone, Serialize)]
pub struct AggregatedProject {
    pub name: String,
    pub default_name: String,
    pub custom_name: Option<String>,
    pub external_names: Vec<ExternalProjectName>,
    pub path: PathBuf,
    pub display_path: String,
    pub exists: bool,
    pub agents: Vec<AgentUsage>,
    pub last_active: Option<i64>,
}

impl AggregatedProject {
    pub fn has_agent(&self, agent: AgentKind) -> bool {
        self.agents.iter().any(|u| u.agent == agent)
    }

    /// Recomputes external custom names and resolves the primary display name using:
    /// 1. ProjPanel custom name
    /// 2. Most recently active external agent custom name
    /// 3. Default directory name
    pub fn resolve_display_names(&mut self, custom_name: Option<String>) {
        self.custom_name = custom_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut candidates: Vec<(Option<i64>, AgentKind, String)> = self
            .agents
            .iter()
            .filter_map(|usage| {
                let raw = usage.display_name.as_deref()?.trim();
                if raw.is_empty() || raw.eq_ignore_ascii_case(&self.default_name) {
                    return None;
                }
                Some((usage.last_active, usage.agent, raw.to_string()))
            })
            .collect();

        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        self.external_names = candidates
            .into_iter()
            .map(|(_, agent, name)| ExternalProjectName { agent, name })
            .collect();

        self.name = self
            .custom_name
            .clone()
            .or_else(|| self.external_names.first().map(|item| item.name.clone()))
            .unwrap_or_else(|| self.default_name.clone());
    }

    /// Returns true when the project name should show a dropdown to inspect external custom names.
    pub fn should_show_name_dropdown(&self) -> bool {
        if self.external_names.is_empty() {
            return false;
        }
        if self.custom_name.is_some() {
            return true;
        }
        let first = self.external_names[0].name.as_str();
        self.external_names.iter().any(|item| item.name != first)
    }
}

/// Per-agent scan summary showing detected agent presence and record count.
#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub agent: AgentKind,
    pub project_count: usize,
    pub error: Option<String>,
}

/// Complete output of a single aggregation scan across all registered scanners.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanReport {
    pub projects: Vec<AggregatedProject>,
    pub agent_summaries: Vec<AgentSummary>,
    pub elapsed_ms: u64,
    pub scanned_at: i64,
}

impl ScanReport {
    pub fn from_results(
        projects: Vec<AggregatedProject>,
        agent_summaries: Vec<AgentSummary>,
        elapsed: Duration,
    ) -> Self {
        Self {
            projects,
            agent_summaries,
            elapsed_ms: elapsed.as_millis() as u64,
            scanned_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn active_agent_count(&self) -> usize {
        self.agent_summaries
            .iter()
            .filter(|s| s.project_count > 0)
            .count()
    }
}
