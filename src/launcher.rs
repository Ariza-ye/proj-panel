use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::model::{AgentKind, AggregatedProject};

/// High-level grouping for external launch targets shown in the Open In menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LauncherCategory {
    Editor,
    AiApp,
    Terminal,
}

/// Supported external tools that can open a project directory directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LauncherKind {
    // Editors and IDEs
    VsCode,
    Cursor,
    Windsurf,
    Trae,
    Kiro,
    Zed,
    IntelliJIdea,
    PyCharm,
    WebStorm,
    GoLand,
    CLion,
    PhpStorm,
    Rider,
    RustRover,
    RubyMine,

    // AI desktop applications
    Antigravity,
    CodexApp,
    ClaudeDesktop,
    OpenCodeApp,
    OpenChamberApp,

    // Terminal emulators
    Ghostty,
    Kitty,
    Herdr,
    ITerm2,
    WezTerm,
    Warp,
    Alacritty,
    SystemTerminal,
}

impl LauncherKind {
    /// All known launcher definitions in default presentation order.
    pub const ALL: &'static [LauncherKind] = &[
        Self::Zed,
        Self::VsCode,
        Self::IntelliJIdea,
        Self::Trae,
        Self::Cursor,
        Self::Windsurf,
        Self::Kiro,
        Self::PyCharm,
        Self::WebStorm,
        Self::GoLand,
        Self::CLion,
        Self::PhpStorm,
        Self::Rider,
        Self::RustRover,
        Self::RubyMine,
        Self::Antigravity,
        Self::CodexApp,
        Self::ClaudeDesktop,
        Self::OpenCodeApp,
        Self::OpenChamberApp,
        Self::Ghostty,
        Self::Kitty,
        Self::Herdr,
        Self::ITerm2,
        Self::WezTerm,
        Self::Warp,
        Self::Alacritty,
        Self::SystemTerminal,
    ];

    /// Stable string identifier for UI element keys and state maps.
    pub const fn id(self) -> &'static str {
        match self {
            Self::VsCode => "vscode",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Trae => "trae",
            Self::Kiro => "kiro",
            Self::Zed => "zed",
            Self::IntelliJIdea => "intellij-idea",
            Self::PyCharm => "pycharm",
            Self::WebStorm => "webstorm",
            Self::GoLand => "goland",
            Self::CLion => "clion",
            Self::PhpStorm => "phpstorm",
            Self::Rider => "rider",
            Self::RustRover => "rustrover",
            Self::RubyMine => "rubymine",
            Self::Antigravity => "antigravity",
            Self::CodexApp => "codex-app",
            Self::ClaudeDesktop => "claude-desktop",
            Self::OpenCodeApp => "opencode-app",
            Self::OpenChamberApp => "openchamber-app",
            Self::Ghostty => "ghostty",
            Self::Kitty => "kitty",
            Self::Herdr => "herdr",
            Self::ITerm2 => "iterm2",
            Self::WezTerm => "wezterm",
            Self::Warp => "warp",
            Self::Alacritty => "alacritty",
            Self::SystemTerminal => "system-terminal",
        }
    }

    /// Full label displayed in dropdown menus and notifications.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::VsCode => "VS Code",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::Trae => "Trae",
            Self::Kiro => "Kiro",
            Self::Zed => "Zed",
            Self::IntelliJIdea => "IntelliJ IDEA",
            Self::PyCharm => "PyCharm",
            Self::WebStorm => "WebStorm",
            Self::GoLand => "GoLand",
            Self::CLion => "CLion",
            Self::PhpStorm => "PhpStorm",
            Self::Rider => "Rider",
            Self::RustRover => "RustRover",
            Self::RubyMine => "RubyMine",
            Self::Antigravity => "Antigravity",
            Self::CodexApp => "Codex",
            Self::ClaudeDesktop => "Claude",
            Self::OpenCodeApp => "OpenCode",
            Self::OpenChamberApp => "OpenChamber",
            Self::Ghostty => "Ghostty",
            Self::Kitty => "Kitty",
            Self::Herdr => "Herdr",
            Self::ITerm2 => "iTerm2",
            Self::WezTerm => "WezTerm",
            Self::Warp => "Warp",
            Self::Alacritty => "Alacritty",
            Self::SystemTerminal => {
                #[cfg(target_os = "macos")]
                {
                    "Terminal"
                }
                #[cfg(target_os = "windows")]
                {
                    "终端"
                }
                #[cfg(all(unix, not(target_os = "macos")))]
                {
                    "系统终端"
                }
            }
        }
    }

    /// Compact label used inside the split button to prevent crowding narrow project cards.
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::VsCode => "VS Code",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::Trae => "Trae",
            Self::Kiro => "Kiro",
            Self::Zed => "Zed",
            Self::IntelliJIdea => "IDEA",
            Self::PyCharm => "PyCharm",
            Self::WebStorm => "WebStorm",
            Self::GoLand => "GoLand",
            Self::CLion => "CLion",
            Self::PhpStorm => "PhpStorm",
            Self::Rider => "Rider",
            Self::RustRover => "RustRover",
            Self::RubyMine => "RubyMine",
            Self::Antigravity => "Agy",
            Self::CodexApp => "Codex",
            Self::ClaudeDesktop => "Claude",
            Self::OpenCodeApp => "OpenCode",
            Self::OpenChamberApp => "Chamber",
            Self::Ghostty => "Ghostty",
            Self::Kitty => "Kitty",
            Self::Herdr => "Herdr",
            Self::ITerm2 => "iTerm2",
            Self::WezTerm => "WezTerm",
            Self::Warp => "Warp",
            Self::Alacritty => "Alacritty",
            Self::SystemTerminal => "终端",
        }
    }

    /// Category used to group items in the dropdown menu.
    pub const fn category(self) -> LauncherCategory {
        match self {
            Self::VsCode
            | Self::Cursor
            | Self::Windsurf
            | Self::Trae
            | Self::Kiro
            | Self::Zed
            | Self::IntelliJIdea
            | Self::PyCharm
            | Self::WebStorm
            | Self::GoLand
            | Self::CLion
            | Self::PhpStorm
            | Self::Rider
            | Self::RustRover
            | Self::RubyMine => LauncherCategory::Editor,
            Self::Antigravity
            | Self::CodexApp
            | Self::ClaudeDesktop
            | Self::OpenCodeApp
            | Self::OpenChamberApp => LauncherCategory::AiApp,
            Self::Ghostty
            | Self::Kitty
            | Self::Herdr
            | Self::ITerm2
            | Self::WezTerm
            | Self::Warp
            | Self::Alacritty
            | Self::SystemTerminal => LauncherCategory::Terminal,
        }
    }

    /// Corresponding [`AgentKind`] when this launcher shares an icon and project affinity.
    pub const fn linked_agent(self) -> Option<AgentKind> {
        match self {
            Self::VsCode => Some(AgentKind::VsCode),
            Self::Cursor => Some(AgentKind::Cursor),
            Self::Windsurf => Some(AgentKind::Windsurf),
            Self::Trae => Some(AgentKind::Trae),
            Self::Kiro => Some(AgentKind::Kiro),
            Self::Zed => Some(AgentKind::Zed),
            Self::IntelliJIdea => Some(AgentKind::IntelliJIdea),
            Self::PyCharm => Some(AgentKind::PyCharm),
            Self::WebStorm => Some(AgentKind::WebStorm),
            Self::GoLand => Some(AgentKind::GoLand),
            Self::CLion => Some(AgentKind::CLion),
            Self::PhpStorm => Some(AgentKind::PhpStorm),
            Self::Rider => Some(AgentKind::Rider),
            Self::RustRover => Some(AgentKind::RustRover),
            Self::RubyMine => Some(AgentKind::RubyMine),
            Self::Antigravity => Some(AgentKind::Antigravity),
            Self::CodexApp => Some(AgentKind::Codex),
            Self::ClaudeDesktop => Some(AgentKind::ClaudeCode),
            Self::OpenCodeApp => Some(AgentKind::OpenCode),
            Self::OpenChamberApp => Some(AgentKind::OpenChamber),
            Self::Ghostty
            | Self::Kitty
            | Self::Herdr
            | Self::ITerm2
            | Self::WezTerm
            | Self::Warp
            | Self::Alacritty
            | Self::SystemTerminal => None,
        }
    }
}

/// Platform-specific launch invocation strategy for a detected tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchStrategy {
    /// macOS: `open -a <app_bundle> <project_dir>`
    #[cfg(target_os = "macos")]
    MacOpenApp(PathBuf),
    /// macOS: `open -na <app_bundle> --args <extra_args...> <project_dir>`
    #[cfg(target_os = "macos")]
    MacOpenNewInstanceWithArgs {
        app: PathBuf,
        extra_args: Vec<String>,
    },
    /// macOS Herdr: uses `Herdr do space.new --folder <dir>` with `open -a` fallback.
    #[cfg(target_os = "macos")]
    MacHerdrApp(PathBuf),
    /// Direct binary execution.
    CliCommand {
        program: PathBuf,
        args_before_dir: Vec<String>,
        pass_dir_as_arg: bool,
        set_current_dir: bool,
    },
}

/// A launch target confirmed to be installed on the current machine.
#[derive(Debug, Clone)]
pub struct DetectedLauncher {
    pub kind: LauncherKind,
    pub strategy: LaunchStrategy,
}

/// Selects the default launcher for a project's split button:
/// 1. Explicit per-project choice from this session
/// 2. Most recently active agent on the project that maps to an installed launcher
/// 3. Global last-used launcher
/// 4. First available installed launcher
pub fn resolve_primary_launcher<'a>(
    proj: &AggregatedProject,
    installed: &'a [DetectedLauncher],
    project_prefs: &HashMap<PathBuf, LauncherKind>,
    last_used: Option<LauncherKind>,
) -> Option<&'a DetectedLauncher> {
    if installed.is_empty() {
        return None;
    }

    if let Some(pref) = project_prefs.get(&proj.path)
        && let Some(found) = installed.iter().find(|l| l.kind == *pref)
    {
        return Some(found);
    }

    let mut best: Option<(Option<i64>, &'a DetectedLauncher)> = None;
    for usage in &proj.agents {
        if let Some(found) = installed
            .iter()
            .find(|l| l.kind.linked_agent() == Some(usage.agent))
            && best.is_none_or(|(best_ts, _)| usage.last_active > best_ts)
        {
            best = Some((usage.last_active, found));
        }
    }
    if let Some((_, found)) = best {
        return Some(found);
    }

    if let Some(last) = last_used
        && let Some(found) = installed.iter().find(|l| l.kind == last)
    {
        return Some(found);
    }

    installed.first()
}

/// Partitions installed launchers into menu sections without duplicates:
/// (project-matched tools, remaining editors/AI apps, terminals).
pub fn partition_launchers_for_project<'a>(
    proj: &AggregatedProject,
    installed: &'a [DetectedLauncher],
) -> (
    Vec<&'a DetectedLauncher>,
    Vec<&'a DetectedLauncher>,
    Vec<&'a DetectedLauncher>,
) {
    let mut matched = Vec::new();
    let mut matched_kinds = HashSet::new();

    let mut ranked_agents: Vec<(AgentKind, Option<i64>)> = proj
        .agents
        .iter()
        .map(|a| (a.agent, a.last_active))
        .collect();
    ranked_agents.sort_by_key(|(_, ts)| std::cmp::Reverse(*ts));
    for (agent, _) in ranked_agents {
        for launcher in installed {
            if launcher.kind.linked_agent() == Some(agent) && matched_kinds.insert(launcher.kind) {
                matched.push(launcher);
            }
        }
    }

    let mut apps = Vec::new();
    let mut terminals = Vec::new();
    for launcher in installed {
        if matched_kinds.contains(&launcher.kind) {
            continue;
        }
        match launcher.kind.category() {
            LauncherCategory::Editor | LauncherCategory::AiApp => apps.push(launcher),
            LauncherCategory::Terminal => terminals.push(launcher),
        }
    }

    (matched, apps, terminals)
}

/// Scans the local machine once for installed editors, AI desktop apps, and terminals.
pub fn detect_installed_launchers() -> Vec<DetectedLauncher> {
    let mut found = Vec::new();
    #[cfg(target_os = "macos")]
    let app_bundles = collect_macos_app_bundles();

    for &kind in LauncherKind::ALL {
        #[cfg(target_os = "macos")]
        let strategy = detect_macos_launcher(kind, &app_bundles);
        #[cfg(target_os = "windows")]
        let strategy = detect_windows_launcher(kind);
        #[cfg(all(unix, not(target_os = "macos")))]
        let strategy = detect_linux_launcher(kind);

        if let Some(strategy) = strategy {
            found.push(DetectedLauncher { kind, strategy });
        }
    }

    found
}

/// Launches the specified external tool targeting `project_dir`.
pub fn open_project_with(launcher: &DetectedLauncher, project_dir: &Path) -> std::io::Result<()> {
    match &launcher.strategy {
        #[cfg(target_os = "macos")]
        LaunchStrategy::MacOpenApp(app_path) => {
            Command::new("open")
                .arg("-a")
                .arg(app_path)
                .arg(project_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        LaunchStrategy::MacOpenNewInstanceWithArgs { app, extra_args } => {
            let mut cmd = Command::new("open");
            cmd.arg("-na").arg(app).arg("--args");
            for arg in extra_args {
                cmd.arg(arg);
            }
            cmd.arg(project_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        LaunchStrategy::MacHerdrApp(app_path) => {
            let app_path = app_path.clone();
            let dir = project_dir.to_path_buf();
            // Run CLI dispatch off the UI thread; fall back to launching the bundle directly.
            std::thread::spawn(move || {
                let herdr_bin = app_path.join("Contents/MacOS/Herdr");
                let ok = if herdr_bin.is_file() {
                    Command::new(&herdr_bin)
                        .arg("do")
                        .arg("space.new")
                        .arg("--folder")
                        .arg(&dir)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false)
                } else {
                    false
                };

                if !ok {
                    let _ = Command::new("open")
                        .arg("-a")
                        .arg(&app_path)
                        .arg(&dir)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                } else {
                    let _ = Command::new("open")
                        .arg("-a")
                        .arg(&app_path)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
            });
            Ok(())
        }
        LaunchStrategy::CliCommand {
            program,
            args_before_dir,
            pass_dir_as_arg,
            set_current_dir,
        } => {
            let mut cmd = Command::new(program);
            for arg in args_before_dir {
                cmd.arg(arg);
            }
            if *pass_dir_as_arg {
                cmd.arg(project_dir);
            }
            if *set_current_dir {
                cmd.current_dir(project_dir);
            }
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                // Avoid flashing a transient console window for GUI wrappers (.cmd/.bat).
                if !matches!(launcher.kind.category(), LauncherCategory::Terminal) {
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
            }
            cmd.spawn()?;
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
struct AppBundleEntry {
    name_lower: String,
    path: PathBuf,
}

#[cfg(target_os = "macos")]
fn collect_macos_app_bundles() -> Vec<AppBundleEntry> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
        PathBuf::from("/Applications/Utilities"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
        roots.push(home.join("Applications").join("JetBrains Toolbox"));
    }

    let mut bundles = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.ends_with(".app") {
                bundles.push(AppBundleEntry {
                    name_lower: name.to_lowercase(),
                    path,
                });
            }
        }
    }
    bundles
}

#[cfg(target_os = "macos")]
fn find_macos_app_exact(bundles: &[AppBundleEntry], candidates: &[&str]) -> Option<PathBuf> {
    for &candidate in candidates {
        let target = candidate.to_lowercase();
        if let Some(found) = bundles.iter().find(|b| b.name_lower == target) {
            return Some(found.path.clone());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn find_macos_app_prefix(bundles: &[AppBundleEntry], prefixes: &[&str]) -> Option<PathBuf> {
    for &prefix in prefixes {
        let target = prefix.to_lowercase();
        if let Some(found) = bundles
            .iter()
            .find(|b| b.name_lower.starts_with(&target) && b.name_lower.ends_with(".app"))
        {
            return Some(found.path.clone());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn find_macos_app_by_bundle_id(bundles: &[AppBundleEntry], bundle_ids: &[&str]) -> Option<PathBuf> {
    bundles
        .iter()
        .find(|b| macos_bundle_has_id(&b.path, bundle_ids))
        .map(|b| b.path.clone())
}

#[cfg(target_os = "macos")]
fn macos_bundle_has_id(app_path: &Path, bundle_ids: &[&str]) -> bool {
    let plist_path = app_path.join("Contents/Info.plist");
    let Ok(bytes) = std::fs::read(&plist_path) else {
        return false;
    };

    if bytes.starts_with(b"bplist") {
        // Pre-filter binary plists by raw ASCII bytes to avoid spawning plutil unnecessarily.
        let maybe_match = bundle_ids.iter().any(|id| {
            let needle = id.as_bytes();
            !needle.is_empty() && bytes.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
        });
        if !maybe_match {
            return false;
        }
        let Ok(output) = Command::new("plutil")
            .args(["-extract", "CFBundleIdentifier", "raw"])
            .arg(&plist_path)
            .output()
        else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        let id = String::from_utf8_lossy(&output.stdout);
        let id = id.trim();
        return bundle_ids.iter().any(|target| id.eq_ignore_ascii_case(target));
    }

    let Ok(xml) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let Some(id) = extract_xml_plist_string(xml, "CFBundleIdentifier") else {
        return false;
    };
    bundle_ids.iter().any(|target| id.eq_ignore_ascii_case(target))
}

#[cfg(target_os = "macos")]
fn extract_xml_plist_string<'a>(xml: &'a str, key: &str) -> Option<&'a str> {
    let key_tag = format!("<key>{key}</key>");
    let mut rest = xml;
    while let Some(pos) = rest.find(&key_tag) {
        let after_key = rest[pos + key_tag.len()..].trim_start();
        if let Some(after_open) = after_key.strip_prefix("<string>")
            && let Some(end) = after_open.find("</string>")
        {
            return Some(after_open[..end].trim());
        }
        rest = &rest[pos + key_tag.len()..];
    }
    None
}

#[cfg(target_os = "macos")]
fn find_macos_cli_fallback(names: &[&str]) -> Option<PathBuf> {
    if let Some(p) = find_in_path(names) {
        return Some(p);
    }
    let mut extra_dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = dirs::home_dir() {
        extra_dirs.push(home.join(".local/bin"));
    }
    for dir in extra_dirs {
        for &name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_macos_launcher(
    kind: LauncherKind,
    bundles: &[AppBundleEntry],
) -> Option<LaunchStrategy> {
    let cli_fallback = |names: &[&str], args_before_dir: Vec<String>, pass_dir: bool, set_cwd: bool| {
        find_macos_cli_fallback(names).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir,
            pass_dir_as_arg: pass_dir,
            set_current_dir: set_cwd,
        })
    };

    match kind {
        LauncherKind::VsCode => find_macos_app_exact(
            bundles,
            &[
                "Visual Studio Code.app",
                "Visual Studio Code - Insiders.app",
            ],
        )
        .map(LaunchStrategy::MacOpenApp)
        .or_else(|| cli_fallback(&["code", "code-insiders"], Vec::new(), true, false)),
        LauncherKind::Cursor => find_macos_app_exact(bundles, &["Cursor.app"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["cursor"], Vec::new(), true, false)),
        LauncherKind::Windsurf => find_macos_app_exact(bundles, &["Windsurf.app"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["windsurf"], Vec::new(), true, false)),
        LauncherKind::Trae => find_macos_app_exact(bundles, &["Trae.app"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["trae"], Vec::new(), true, false)),
        LauncherKind::Kiro => find_macos_app_exact(bundles, &["Kiro.app"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["kiro"], Vec::new(), true, false)),
        LauncherKind::Zed => find_macos_app_exact(bundles, &["Zed.app", "Zed Preview.app"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["zed"], Vec::new(), true, false)),
        LauncherKind::IntelliJIdea => find_macos_app_prefix(bundles, &["IntelliJ IDEA"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["idea"], Vec::new(), true, false)),
        LauncherKind::PyCharm => find_macos_app_prefix(bundles, &["PyCharm"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["pycharm"], Vec::new(), true, false)),
        LauncherKind::WebStorm => find_macos_app_prefix(bundles, &["WebStorm"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["webstorm"], Vec::new(), true, false)),
        LauncherKind::GoLand => find_macos_app_prefix(bundles, &["GoLand"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["goland"], Vec::new(), true, false)),
        LauncherKind::CLion => find_macos_app_prefix(bundles, &["CLion"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["clion"], Vec::new(), true, false)),
        LauncherKind::PhpStorm => find_macos_app_prefix(bundles, &["PhpStorm"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["phpstorm"], Vec::new(), true, false)),
        LauncherKind::Rider => find_macos_app_prefix(bundles, &["Rider"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["rider"], Vec::new(), true, false)),
        LauncherKind::RustRover => find_macos_app_prefix(bundles, &["RustRover"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["rustrover"], Vec::new(), true, false)),
        LauncherKind::RubyMine => find_macos_app_prefix(bundles, &["RubyMine"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["rubymine"], Vec::new(), true, false)),
        LauncherKind::Antigravity => find_macos_app_exact(bundles, &["Antigravity.app"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| cli_fallback(&["antigravity"], Vec::new(), true, false)),
        LauncherKind::CodexApp => find_macos_app_exact(bundles, &["Codex.app"])
            .or_else(|| find_macos_app_by_bundle_id(bundles, &["com.openai.codex"]))
            .map(LaunchStrategy::MacOpenApp),
        LauncherKind::ClaudeDesktop => {
            find_macos_app_exact(bundles, &["Claude.app"]).map(LaunchStrategy::MacOpenApp)
        }
        LauncherKind::OpenCodeApp => {
            find_macos_app_exact(bundles, &["OpenCode.app"]).map(LaunchStrategy::MacOpenApp)
        }
        LauncherKind::OpenChamberApp => {
            find_macos_app_exact(bundles, &["OpenChamber.app"]).map(LaunchStrategy::MacOpenApp)
        }
        LauncherKind::Ghostty => {
            find_macos_app_exact(bundles, &["Ghostty.app"]).map(LaunchStrategy::MacOpenApp)
        }
        LauncherKind::Kitty => find_macos_app_exact(bundles, &["kitty.app", "Kitty.app"])
            .map(|app| LaunchStrategy::MacOpenNewInstanceWithArgs {
                app,
                extra_args: vec!["--directory".to_string()],
            })
            .or_else(|| cli_fallback(&["kitty"], vec!["--directory".to_string()], true, false)),
        LauncherKind::Herdr => {
            find_macos_app_exact(bundles, &["Herdr.app"]).map(LaunchStrategy::MacHerdrApp)
        }
        LauncherKind::ITerm2 => find_macos_app_exact(bundles, &["iTerm.app", "iTerm2.app"])
            .map(LaunchStrategy::MacOpenApp),
        LauncherKind::WezTerm => find_macos_app_exact(bundles, &["WezTerm.app"])
            .map(LaunchStrategy::MacOpenApp)
            .or_else(|| {
                cli_fallback(
                    &["wezterm"],
                    vec!["start".to_string(), "--cwd".to_string()],
                    true,
                    false,
                )
            }),
        LauncherKind::Warp => {
            find_macos_app_exact(bundles, &["Warp.app"]).map(LaunchStrategy::MacOpenApp)
        }
        LauncherKind::Alacritty => find_macos_app_exact(bundles, &["Alacritty.app"])
            .map(|app| LaunchStrategy::MacOpenNewInstanceWithArgs {
                app,
                extra_args: vec!["--working-directory".to_string()],
            })
            .or_else(|| {
                cli_fallback(
                    &["alacritty"],
                    vec!["--working-directory".to_string()],
                    true,
                    false,
                )
            }),
        LauncherKind::SystemTerminal => {
            find_macos_app_exact(bundles, &["Terminal.app"]).map(LaunchStrategy::MacOpenApp)
        }
    }
}

fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for &name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn find_windows_program(path_names: &[&str], known_paths: &[PathBuf]) -> Option<PathBuf> {
    if let Some(p) = find_in_path(path_names) {
        return Some(p);
    }
    known_paths.iter().find(|p| p.is_file()).cloned()
}

#[cfg(target_os = "windows")]
fn detect_windows_launcher(kind: LauncherKind) -> Option<LaunchStrategy> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);

    let simple_cli = |path_names: &[&str], extra: Vec<PathBuf>| {
        find_windows_program(path_names, &extra).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: Vec::new(),
            pass_dir_as_arg: true,
            set_current_dir: false,
        })
    };

    match kind {
        LauncherKind::VsCode => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("Programs/Microsoft VS Code/bin/code.cmd"));
                extra.push(d.join("Programs/Microsoft VS Code/Code.exe"));
            }
            if let Some(d) = &program_files {
                extra.push(d.join("Microsoft VS Code/bin/code.cmd"));
            }
            simple_cli(&["code.cmd", "code.exe", "code"], extra)
        }
        LauncherKind::Cursor => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("Programs/cursor/Cursor.exe"));
            }
            simple_cli(&["cursor.cmd", "cursor.exe", "cursor"], extra)
        }
        LauncherKind::Windsurf => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("Programs/Windsurf/Windsurf.exe"));
            }
            simple_cli(&["windsurf.cmd", "windsurf.exe", "windsurf"], extra)
        }
        LauncherKind::Trae => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("Programs/Trae/Trae.exe"));
            }
            simple_cli(&["trae.cmd", "trae.exe", "trae"], extra)
        }
        LauncherKind::Kiro => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("Programs/Kiro/Kiro.exe"));
            }
            simple_cli(&["kiro.cmd", "kiro.exe", "kiro"], extra)
        }
        LauncherKind::Zed => simple_cli(&["zed.exe", "zed"], Vec::new()),
        LauncherKind::IntelliJIdea => simple_cli(&["idea64.exe", "idea.cmd", "idea.bat"], Vec::new()),
        LauncherKind::PyCharm => simple_cli(&["pycharm64.exe", "pycharm.cmd", "pycharm.bat"], Vec::new()),
        LauncherKind::WebStorm => simple_cli(&["webstorm64.exe", "webstorm.cmd", "webstorm.bat"], Vec::new()),
        LauncherKind::GoLand => simple_cli(&["goland64.exe", "goland.cmd", "goland.bat"], Vec::new()),
        LauncherKind::CLion => simple_cli(&["clion64.exe", "clion.cmd", "clion.bat"], Vec::new()),
        LauncherKind::PhpStorm => simple_cli(&["phpstorm64.exe", "phpstorm.cmd", "phpstorm.bat"], Vec::new()),
        LauncherKind::Rider => simple_cli(&["rider64.exe", "rider.cmd", "rider.bat"], Vec::new()),
        LauncherKind::RustRover => simple_cli(&["rustrover64.exe", "rustrover.cmd", "rustrover.bat"], Vec::new()),
        LauncherKind::RubyMine => simple_cli(&["rubymine64.exe", "rubymine.cmd", "rubymine.bat"], Vec::new()),
        LauncherKind::Antigravity => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("Programs/Antigravity/Antigravity.exe"));
            }
            simple_cli(&["antigravity.cmd", "antigravity.exe", "antigravity"], extra)
        }
        LauncherKind::CodexApp => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("Programs/Codex/Codex.exe"));
            }
            simple_cli(&["Codex.exe"], extra)
        }
        LauncherKind::ClaudeDesktop => {
            let mut extra = Vec::new();
            if let Some(d) = &local_app_data {
                extra.push(d.join("AnthropicClaude/claude.exe"));
            }
            simple_cli(&[], extra)
        }
        LauncherKind::OpenCodeApp => simple_cli(&["OpenCode.exe"], Vec::new()),
        LauncherKind::OpenChamberApp => simple_cli(&["OpenChamber.exe"], Vec::new()),
        LauncherKind::Ghostty => find_in_path(&["ghostty.exe"]).map(|program| {
            LaunchStrategy::CliCommand {
                program,
                args_before_dir: Vec::new(),
                pass_dir_as_arg: false,
                set_current_dir: true,
            }
        }),
        LauncherKind::Kitty => find_in_path(&["kitty.exe"]).map(|program| {
            LaunchStrategy::CliCommand {
                program,
                args_before_dir: vec!["--directory".to_string()],
                pass_dir_as_arg: true,
                set_current_dir: false,
            }
        }),
        LauncherKind::Herdr => find_in_path(&["herdr.exe", "Herdr.exe"]).map(|program| {
            LaunchStrategy::CliCommand {
                program,
                args_before_dir: Vec::new(),
                pass_dir_as_arg: true,
                set_current_dir: true,
            }
        }),
        LauncherKind::ITerm2 => None,
        LauncherKind::WezTerm => find_in_path(&["wezterm-gui.exe", "wezterm.exe"]).map(|program| {
            LaunchStrategy::CliCommand {
                program,
                args_before_dir: vec!["start".to_string(), "--cwd".to_string()],
                pass_dir_as_arg: true,
                set_current_dir: false,
            }
        }),
        LauncherKind::Warp => find_in_path(&["warp.exe"]).map(|program| {
            LaunchStrategy::CliCommand {
                program,
                args_before_dir: Vec::new(),
                pass_dir_as_arg: true,
                set_current_dir: true,
            }
        }),
        LauncherKind::Alacritty => find_in_path(&["alacritty.exe"]).map(|program| {
            LaunchStrategy::CliCommand {
                program,
                args_before_dir: vec!["--working-directory".to_string()],
                pass_dir_as_arg: true,
                set_current_dir: false,
            }
        }),
        LauncherKind::SystemTerminal => {
            if let Some(wt) = find_in_path(&["wt.exe"]) {
                return Some(LaunchStrategy::CliCommand {
                    program: wt,
                    args_before_dir: vec!["-d".to_string()],
                    pass_dir_as_arg: true,
                    set_current_dir: false,
                });
            }
            find_in_path(&["pwsh.exe", "powershell.exe", "cmd.exe"]).map(|program| {
                LaunchStrategy::CliCommand {
                    program,
                    args_before_dir: Vec::new(),
                    pass_dir_as_arg: false,
                    set_current_dir: true,
                }
            })
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_linux_launcher(kind: LauncherKind) -> Option<LaunchStrategy> {
    let simple_cli = |names: &[&str]| {
        find_in_path(names).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: Vec::new(),
            pass_dir_as_arg: true,
            set_current_dir: false,
        })
    };

    match kind {
        LauncherKind::VsCode => simple_cli(&["code", "code-insiders"]),
        LauncherKind::Cursor => simple_cli(&["cursor"]),
        LauncherKind::Windsurf => simple_cli(&["windsurf"]),
        LauncherKind::Trae => simple_cli(&["trae"]),
        LauncherKind::Kiro => simple_cli(&["kiro"]),
        LauncherKind::Zed => simple_cli(&["zed", "zeditor"]),
        LauncherKind::IntelliJIdea => simple_cli(&["idea", "intellij-idea-ultimate", "intellij-idea-community"]),
        LauncherKind::PyCharm => simple_cli(&["pycharm", "pycharm-professional", "pycharm-community"]),
        LauncherKind::WebStorm => simple_cli(&["webstorm"]),
        LauncherKind::GoLand => simple_cli(&["goland"]),
        LauncherKind::CLion => simple_cli(&["clion"]),
        LauncherKind::PhpStorm => simple_cli(&["phpstorm"]),
        LauncherKind::Rider => simple_cli(&["rider"]),
        LauncherKind::RustRover => simple_cli(&["rustrover"]),
        LauncherKind::RubyMine => simple_cli(&["rubymine"]),
        LauncherKind::Antigravity => simple_cli(&["antigravity"]),
        LauncherKind::CodexApp => simple_cli(&["codex-desktop"]),
        LauncherKind::ClaudeDesktop => simple_cli(&["claude-desktop"]),
        LauncherKind::OpenCodeApp => simple_cli(&["opencode-desktop"]),
        LauncherKind::OpenChamberApp => simple_cli(&["openchamber"]),
        LauncherKind::Ghostty => find_in_path(&["ghostty"]).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: Vec::new(),
            pass_dir_as_arg: false,
            set_current_dir: true,
        }),
        LauncherKind::Kitty => find_in_path(&["kitty"]).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: vec!["--directory".to_string()],
            pass_dir_as_arg: true,
            set_current_dir: false,
        }),
        LauncherKind::Herdr => find_in_path(&["herdr", "herdr-app"]).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: Vec::new(),
            pass_dir_as_arg: true,
            set_current_dir: true,
        }),
        LauncherKind::ITerm2 => None,
        LauncherKind::WezTerm => find_in_path(&["wezterm"]).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: vec!["start".to_string(), "--cwd".to_string()],
            pass_dir_as_arg: true,
            set_current_dir: false,
        }),
        LauncherKind::Warp => simple_cli(&["warp-terminal"]),
        LauncherKind::Alacritty => find_in_path(&["alacritty"]).map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: vec!["--working-directory".to_string()],
            pass_dir_as_arg: true,
            set_current_dir: false,
        }),
        LauncherKind::SystemTerminal => find_in_path(&[
            "x-terminal-emulator",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "xterm",
        ])
        .map(|program| LaunchStrategy::CliCommand {
            program,
            args_before_dir: Vec::new(),
            pass_dir_as_arg: false,
            set_current_dir: true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentUsage;

    fn dummy_launcher(kind: LauncherKind) -> DetectedLauncher {
        DetectedLauncher {
            kind,
            strategy: LaunchStrategy::CliCommand {
                program: PathBuf::from(kind.id()),
                args_before_dir: Vec::new(),
                pass_dir_as_arg: true,
                set_current_dir: false,
            },
        }
    }

    fn dummy_project(path: &str, agents: Vec<(AgentKind, Option<i64>)>) -> AggregatedProject {
        AggregatedProject {
            name: "demo".to_string(),
            default_name: "demo".to_string(),
            custom_name: None,
            external_names: Vec::new(),
            path: PathBuf::from(path),
            display_path: path.to_string(),
            exists: true,
            agents: agents
                .into_iter()
                .map(|(agent, last_active)| AgentUsage {
                    agent,
                    display_name: None,
                    last_active,
                    session_count: None,
                    source: "test".to_string(),
                })
                .collect(),
            last_active: Some(1_700_000_000),
        }
    }

    #[test]
    fn primary_launcher_respects_priority_order() {
        let installed = vec![
            dummy_launcher(LauncherKind::VsCode),
            dummy_launcher(LauncherKind::Zed),
            dummy_launcher(LauncherKind::Ghostty),
        ];
        let proj = dummy_project(
            "/tmp/demo",
            vec![
                (AgentKind::VsCode, Some(100)),
                (AgentKind::Zed, Some(200)),
            ],
        );

        // Prefers most recently active linked agent when no explicit preference exists.
        let picked = resolve_primary_launcher(&proj, &installed, &HashMap::new(), None).unwrap();
        assert_eq!(picked.kind, LauncherKind::Zed);

        // Explicit per-project preference overrides agent affinity.
        let mut prefs = HashMap::new();
        prefs.insert(PathBuf::from("/tmp/demo"), LauncherKind::Ghostty);
        let picked = resolve_primary_launcher(&proj, &installed, &prefs, Some(LauncherKind::VsCode)).unwrap();
        assert_eq!(picked.kind, LauncherKind::Ghostty);

        // Falls back to global last_used when project has no linked installed launcher.
        let cli_only = dummy_project("/tmp/cli", vec![(AgentKind::GeminiCli, Some(500))]);
        let picked = resolve_primary_launcher(
            &cli_only,
            &installed,
            &HashMap::new(),
            Some(LauncherKind::Ghostty),
        )
        .unwrap();
        assert_eq!(picked.kind, LauncherKind::Ghostty);
    }

    #[test]
    fn partitions_menu_without_duplicates() {
        let installed = vec![
            dummy_launcher(LauncherKind::Zed),
            dummy_launcher(LauncherKind::VsCode),
            dummy_launcher(LauncherKind::Ghostty),
            dummy_launcher(LauncherKind::SystemTerminal),
        ];
        let proj = dummy_project("/tmp/demo", vec![(AgentKind::Zed, Some(300))]);

        let (matched, apps, terminals) = partition_launchers_for_project(&proj, &installed);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].kind, LauncherKind::Zed);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].kind, LauncherKind::VsCode);
        assert_eq!(terminals.len(), 2);
    }

    #[test]
    fn all_launchers_have_unique_ids() {
        let mut seen = HashSet::new();
        for &kind in LauncherKind::ALL {
            assert!(seen.insert(kind.id()), "duplicate launcher id: {}", kind.id());
        }
    }

    #[test]
    fn detects_local_launchers_without_duplicates() {
        let detected = detect_installed_launchers();
        let mut seen = HashSet::new();
        for item in &detected {
            assert!(seen.insert(item.kind), "duplicate detected launcher: {:?}", item.kind);
        }
        #[cfg(target_os = "macos")]
        assert!(
            detected.iter().any(|l| l.kind == LauncherKind::SystemTerminal),
            "macOS should detect built-in Terminal.app"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_codex_app_by_bundle_identifier() {
        let tmp = tempfile::tempdir().unwrap();

        let classic_app = tmp.path().join("ChatGPT Classic.app");
        std::fs::create_dir_all(classic_app.join("Contents")).unwrap();
        std::fs::write(
            classic_app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key>
  <string>com.openai.chat</string>
</dict></plist>"#,
        )
        .unwrap();

        let codex_renamed_app = tmp.path().join("ChatGPT.app");
        std::fs::create_dir_all(codex_renamed_app.join("Contents")).unwrap();
        std::fs::write(
            codex_renamed_app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleDisplayName</key>
  <string>ChatGPT</string>
  <key>CFBundleIdentifier</key>
  <string>com.openai.codex</string>
</dict></plist>"#,
        )
        .unwrap();

        let bundles = vec![
            AppBundleEntry {
                name_lower: "chatgpt classic.app".to_string(),
                path: classic_app,
            },
            AppBundleEntry {
                name_lower: "chatgpt.app".to_string(),
                path: codex_renamed_app.clone(),
            },
        ];

        let strategy = detect_macos_launcher(LauncherKind::CodexApp, &bundles);
        match strategy {
            Some(LaunchStrategy::MacOpenApp(path)) => assert_eq!(path, codex_renamed_app),
            other => panic!("expected MacOpenApp for renamed Codex bundle, got {other:?}"),
        }
    }
}
