use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
use gpui_kit::assets::IconName as LucideIconName;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarMenu, SidebarMenuItem, SidebarToggleButton,
};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme, ColorName, Disableable, Icon, IconName, IndexPath, Side, Sizable, Theme, ThemeMode,
    TitleBar, WindowExt, h_flex, v_flex,
};
use gpui_kit::{
    Anchor, App, ClipboardItem, Context, Entity, FontWeight, MouseButton, ScrollStrategy,
    SharedString, StyleRefinement, Subscription, Task, UniformListScrollHandle, WeakEntity, Window,
    div, prelude::*, px, rems, uniform_list,
};

use crate::app_icon::apply_dock_icon;
use crate::config::{AppConfig, DEFAULT_UI_FONT_SIZE, MAX_UI_FONT_SIZE, MIN_UI_FONT_SIZE};
use crate::i18n::{self, Language};
use crate::launcher::{
    DetectedLauncher, LauncherKind, detect_installed_launchers, open_project_with,
    partition_launchers_for_project, resolve_primary_launcher,
};
use crate::memory::{current_process_memory_bytes, format_memory_bytes, trim_process_memory};
use crate::model::{AgentKind, AggregatedProject, ScanReport};
use crate::scanner::ScannerRegistry;
use crate::scanner::paths::{dedup_key, open_in_file_manager};
use crate::ui::icons::{AgentIcon, agent_icon, launcher_icon};
use crate::ui::theme::{agent_accent, agent_color_name, apply_ui_typography, heat_level_color};

const MAX_CARD_BADGES: usize = 5;
const HEATMAP_WEEKS: usize = 24;
const HEATMAP_DAYS: usize = 7;
const MEMORY_POLL_INTERVAL_SECS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveView {
    Projects,
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortBy {
    LastActive,
    Name,
    AgentCount,
}

impl SortBy {
    fn from_index(idx: usize) -> Self {
        match idx {
            1 => Self::Name,
            2 => Self::AgentCount,
            _ => Self::LastActive,
        }
    }

    fn to_index(self) -> usize {
        match self {
            Self::LastActive => 0,
            Self::Name => 1,
            Self::AgentCount => 2,
        }
    }
}

/// Precomputed per-project strings and lowercase search keys derived once per scan report.
struct CachedProject {
    name: SharedString,
    name_lower: String,
    search_names_lower: String,
    display_path: SharedString,
    display_path_lower: String,
    full_path: SharedString,
    stable_key: SharedString,
    agent_search_lower: String,
    fallback_date: SharedString,
}

impl CachedProject {
    fn from_project(proj: &AggregatedProject) -> Self {
        let mut agent_search_lower = String::new();
        for usage in &proj.agents {
            agent_search_lower.push_str(&usage.agent.display_name().to_lowercase());
            agent_search_lower.push(' ');
            agent_search_lower.push_str(&usage.agent.short_label().to_lowercase());
            agent_search_lower.push(' ');
        }

        let mut search_names_lower = proj.name.to_lowercase();
        search_names_lower.push(' ');
        search_names_lower.push_str(&proj.default_name.to_lowercase());
        if let Some(custom) = &proj.custom_name {
            search_names_lower.push(' ');
            search_names_lower.push_str(&custom.to_lowercase());
        }
        for ext in &proj.external_names {
            search_names_lower.push(' ');
            search_names_lower.push_str(&ext.name.to_lowercase());
        }

        let fallback_date = proj
            .last_active
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|dt| SharedString::from(dt.with_timezone(&Local).format("%Y-%m-%d").to_string()))
            .unwrap_or_else(|| SharedString::from(tr!("较早", "Earlier")));

        Self {
            name: SharedString::from(proj.name.clone()),
            name_lower: proj.name.to_lowercase(),
            search_names_lower,
            display_path: SharedString::from(proj.display_path.clone()),
            display_path_lower: proj.display_path.to_lowercase(),
            full_path: SharedString::from(proj.path.to_string_lossy().into_owned()),
            stable_key: SharedString::from(dedup_key(&proj.path)),
            agent_search_lower,
            fallback_date,
        }
    }
}

#[derive(Default)]
struct DashboardMetrics {
    existing_count: usize,
    missing_count: usize,
}

struct HeatmapCache {
    start_day: NaiveDate,
    counts_by_day: HashMap<NaiveDate, u32>,
    active_days: usize,
    max_daily: u32,
}

impl HeatmapCache {
    fn from_report(report: &ScanReport) -> Self {
        let today = Utc::now().date_naive();
        let total_days = (HEATMAP_WEEKS * HEATMAP_DAYS) as i64;
        let start_day = today - Duration::days(total_days - 1);

        let mut counts_by_day: HashMap<NaiveDate, u32> = HashMap::new();
        for proj in &report.projects {
            for usage in &proj.agents {
                if let Some(ts) = usage.last_active
                    && let Some(dt) = Utc.timestamp_opt(ts, 0).single()
                {
                    let date = dt.date_naive();
                    if date >= start_day && date <= today {
                        *counts_by_day.entry(date).or_insert(0) += 1;
                    }
                }
            }
        }

        let active_days = counts_by_day.len();
        let max_daily = counts_by_day.values().copied().max().unwrap_or(1).max(1);

        Self {
            start_day,
            counts_by_day,
            active_days,
            max_daily,
        }
    }
}

pub type StateChangedCallback =
    Arc<dyn Fn(Arc<ScanReport>, Arc<Vec<DetectedLauncher>>, bool, &mut App) + Send + Sync>;
pub type LanguageChangedCallback = Arc<dyn Fn(&mut App) + Send + Sync>;
pub type LauncherUsedCallback = Arc<dyn Fn(PathBuf, LauncherKind, &mut App) + Send + Sync>;

/// Callbacks notifying application-level state (such as the system tray) about window updates.
#[derive(Clone, Default)]
pub struct ProjectWindowCallbacks {
    pub on_state_changed: Option<StateChangedCallback>,
    pub on_launcher_used: Option<LauncherUsedCallback>,
    pub on_language_changed: Option<LanguageChangedCallback>,
}

/// Main desktop window built with GPUI Kit components.
pub struct ProjectWindow {
    report: Arc<ScanReport>,
    app_config: AppConfig,
    cached_projects: Vec<CachedProject>,
    metrics: DashboardMetrics,
    active_agents: Vec<(AgentKind, usize)>,
    heatmap_cache: HeatmapCache,
    filtered_indices: Vec<usize>,
    list_scroll_handle: UniformListScrollHandle,
    launchers: Arc<Vec<DetectedLauncher>>,
    project_launcher_prefs: HashMap<PathBuf, LauncherKind>,
    last_launcher: Option<LauncherKind>,
    loading: bool,
    refresh_pending: bool,
    memory_usage_text: Option<SharedString>,
    _memory_monitor_task: Task<()>,
    search_input: Entity<InputState>,
    rename_input: Entity<InputState>,
    editing_project_path: Option<PathBuf>,
    query: String,
    active_view: ActiveView,
    selected_agent: Option<AgentKind>,
    selected_project_path: Option<PathBuf>,
    show_missing: bool,
    sort_by: SortBy,
    sidebar_collapsed: bool,
    _subscriptions: Vec<Subscription>,
    callbacks: ProjectWindowCallbacks,
}

impl ProjectWindow {
    pub fn new(
        initial_report: Arc<ScanReport>,
        initial_launchers: Arc<Vec<DetectedLauncher>>,
        initial_launcher_prefs: HashMap<PathBuf, LauncherKind>,
        initial_last_launcher: Option<LauncherKind>,
        callbacks: ProjectWindowCallbacks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let app_config = AppConfig::load();
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!(
                    "搜索项目名称、路径或 AI Agent...",
                    "Search project name, path or AI Agent..."
                ))
                .clean_on_escape()
        });
        let rename_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(tr!(
                "输入自定义名称，留空恢复默认",
                "Enter a custom name, leave empty to restore default"
            ))
        });

        let search_sub = cx.subscribe_in(&search_input, window, |this, state, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let next_query = state.read(cx).value().to_string();
                if next_query != this.query {
                    this.query = next_query;
                    this.rebuild_filtered(true);
                    cx.notify();
                }
            }
        });

        let rename_sub = cx.subscribe_in(&rename_input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.commit_rename(window, cx);
            }
        });

        let appearance_sub = window.observe_window_appearance(|window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
            apply_dock_icon(cx.theme().is_dark());
        });

        let empty_report = Arc::new(ScanReport::default());
        let heatmap_cache = HeatmapCache::from_report(&empty_report);
        let memory_usage_text = sample_memory_usage_text();
        let memory_monitor_task = cx.spawn(async move |this, cx| {
            // Reclaim temporary startup allocation fragments shortly after initial layout settles.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(800))
                .await;
            trim_process_memory();
            let initial_after_trim = sample_memory_usage_text();
            if this
                .update(cx, |panel, cx| {
                    if panel.memory_usage_text != initial_after_trim {
                        panel.memory_usage_text = initial_after_trim;
                        cx.notify();
                    }
                })
                .is_err()
            {
                return;
            }

            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(MEMORY_POLL_INTERVAL_SECS))
                    .await;
                let next = sample_memory_usage_text();
                let updated = this.update(cx, |panel, cx| {
                    if panel.memory_usage_text != next {
                        panel.memory_usage_text = next;
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        });

        let mut view = Self {
            report: empty_report,
            app_config,
            cached_projects: Vec::new(),
            metrics: DashboardMetrics::default(),
            active_agents: Vec::new(),
            heatmap_cache,
            filtered_indices: Vec::new(),
            list_scroll_handle: UniformListScrollHandle::new(),
            launchers: initial_launchers,
            project_launcher_prefs: initial_launcher_prefs,
            last_launcher: initial_last_launcher,
            loading: false,
            refresh_pending: false,
            memory_usage_text,
            _memory_monitor_task: memory_monitor_task,
            search_input,
            rename_input,
            editing_project_path: None,
            query: String::new(),
            active_view: ActiveView::Projects,
            selected_agent: None,
            selected_project_path: None,
            show_missing: false,
            sort_by: SortBy::LastActive,
            sidebar_collapsed: false,
            _subscriptions: vec![search_sub, rename_sub, appearance_sub],
            callbacks,
        };
        view.apply_report(initial_report);
        view.trigger_refresh(cx);
        view
    }

    /// 打开设置窗口，并用已安装字体初始化字体选择器。
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let families = cx.text_system().all_font_names();
        let selected = self
            .app_config
            .ui_font_family
            .as_ref()
            .and_then(|family| families.iter().position(|name| name == family))
            .map(IndexPath::new);
        let font_select = cx.new(|cx| {
            SelectState::new(SearchableVec::new(families), selected, window, cx).searchable(true)
        });
        let parent = cx.entity().downgrade();
        let panel = cx.new(|cx| {
            let subscription = cx.subscribe_in(
                &font_select,
                window,
                |this: &mut SettingsPanel, _, event, window, cx| {
                    let SelectEvent::Confirm(family) = event;
                    this.with_parent(cx, |panel, cx| {
                        if panel.app_config.ui_font_family != *family {
                            panel.app_config.ui_font_family = family.clone();
                            panel.apply_and_save_typography(window, cx);
                        }
                    });
                },
            );
            SettingsPanel {
                parent,
                tab: 0,
                font_select,
                _font_subscription: subscription,
            }
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.w(px(520.0)).child(panel.clone())
        });
    }

    /// 更新应用界面文字样式并保存用户偏好。
    fn apply_and_save_typography(&self, window: &mut Window, cx: &mut Context<Self>) {
        apply_ui_typography(&self.app_config, cx);
        self.save_config_or_notify(window, cx);
    }

    /// 切换并保存界面语言，刷新依赖语言的内容。
    fn set_language(&mut self, lang: Language, window: &mut Window, cx: &mut Context<Self>) {
        if i18n::current() == lang && self.app_config.language == lang {
            return;
        }
        i18n::set_current(lang);
        self.app_config.language = lang;
        self.save_config_or_notify(window, cx);

        self.search_input.update(cx, |input, cx| {
            input.set_placeholder(
                tr!(
                    "搜索项目名称、路径或 AI Agent...",
                    "Search project name, path or AI Agent..."
                ),
                window,
                cx,
            );
        });
        self.rename_input.update(cx, |input, cx| {
            input.set_placeholder(
                tr!(
                    "输入自定义名称，留空恢复默认",
                    "Enter a custom name, leave empty to restore default"
                ),
                window,
                cx,
            );
        });
        self.apply_report(self.report.clone());
        if let Some(cb) = self.callbacks.on_language_changed.clone() {
            cb(cx);
        }
        cx.notify();
    }

    /// Selects and scrolls to the given project in the main projects view, clearing blocking filters.
    pub fn focus_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.active_view = ActiveView::Projects;
        self.editing_project_path = None;

        if let Some(proj) = self.report.projects.iter().find(|p| p.path == path) {
            if !proj.exists {
                self.show_missing = true;
            }
            if self.selected_agent.is_some_and(|a| !proj.has_agent(a)) {
                self.selected_agent = None;
            }
        }
        if !self.query.is_empty() {
            self.query.clear();
            self.search_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
        }

        self.selected_project_path = Some(path.clone());
        self.rebuild_filtered(false);

        if let Some(filtered_pos) = self.filtered_indices.iter().position(|&idx| {
            self.report
                .projects
                .get(idx)
                .is_some_and(|p| p.path == path)
        }) {
            self.list_scroll_handle
                .scroll_to_item_strict(filtered_pos, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Syncs a launcher selection made outside the main window (for example from the tray menu).
    pub fn record_launcher_preference(
        &mut self,
        path: PathBuf,
        launcher: LauncherKind,
        cx: &mut Context<Self>,
    ) {
        self.project_launcher_prefs.insert(path, launcher);
        self.last_launcher = Some(launcher);
        cx.notify();
    }

    /// Rebuilds precomputed caches whenever a new scan report arrives.
    fn apply_report(&mut self, report: Arc<ScanReport>) {
        let mut existing_count = 0usize;
        let mut missing_count = 0usize;
        for proj in &report.projects {
            if proj.exists {
                existing_count += 1;
            } else {
                missing_count += 1;
            }
        }

        let mut active_agents: Vec<(AgentKind, usize)> = report
            .agent_summaries
            .iter()
            .filter(|s| s.project_count > 0)
            .map(|s| (s.agent, s.project_count))
            .collect();
        active_agents.sort_by(|a, b| b.1.cmp(&a.1));

        self.cached_projects = report
            .projects
            .iter()
            .map(CachedProject::from_project)
            .collect();
        self.metrics = DashboardMetrics {
            existing_count,
            missing_count,
        };
        self.active_agents = active_agents;
        self.heatmap_cache = HeatmapCache::from_report(&report);
        self.report = report;

        if let Some(selected) = &self.selected_project_path
            && !self.report.projects.iter().any(|p| &p.path == selected)
        {
            self.selected_project_path = None;
        }
        if let Some(editing) = &self.editing_project_path
            && !self.report.projects.iter().any(|p| &p.path == editing)
        {
            self.editing_project_path = None;
        }

        self.rebuild_filtered(false);
    }

    /// Recomputes the filtered and sorted index list when query, filters, sort order, or report change.
    fn rebuild_filtered(&mut self, reset_scroll: bool) {
        let q = self.query.trim().to_lowercase();
        let projects = &self.report.projects;
        let cached = &self.cached_projects;

        let mut indices: Vec<usize> = (0..projects.len())
            .filter(|&i| {
                let proj = &projects[i];
                if !self.show_missing && !proj.exists {
                    return false;
                }
                if let Some(agent) = self.selected_agent
                    && !proj.has_agent(agent)
                {
                    return false;
                }
                if q.is_empty() {
                    return true;
                }
                let meta = &cached[i];
                meta.search_names_lower.contains(&q)
                    || meta.display_path_lower.contains(&q)
                    || meta.agent_search_lower.contains(&q)
            })
            .collect();

        match self.sort_by {
            SortBy::LastActive => indices.sort_by(|&a, &b| {
                projects[b]
                    .last_active
                    .cmp(&projects[a].last_active)
                    .then_with(|| cached[a].name_lower.cmp(&cached[b].name_lower))
            }),
            SortBy::Name => {
                indices.sort_by(|&a, &b| cached[a].name_lower.cmp(&cached[b].name_lower))
            }
            SortBy::AgentCount => indices.sort_by(|&a, &b| {
                projects[b]
                    .agents
                    .len()
                    .cmp(&projects[a].agents.len())
                    .then_with(|| projects[b].last_active.cmp(&projects[a].last_active))
            }),
        }

        self.filtered_indices = indices;
        if reset_scroll && !self.filtered_indices.is_empty() {
            self.list_scroll_handle
                .scroll_to_item_strict(0, ScrollStrategy::Top);
        }
    }

    /// Triggers an asynchronous background scan across all registered AI agent scanners and launchers.
    pub fn trigger_refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        if let Some(cb) = self.callbacks.on_state_changed.clone() {
            let report = self.report.clone();
            let launchers = self.launchers.clone();
            cx.defer(move |cx| {
                cb(report, launchers, true, cx);
            });
        }
        cx.notify();

        let scan_config = self.app_config.clone();
        let task = cx.background_executor().spawn(async move {
            let report = Arc::new(ScannerRegistry::new().scan_all_with_config(&scan_config));
            let launchers = Arc::new(detect_installed_launchers());
            trim_process_memory();
            (report, launchers)
        });

        cx.spawn(async move |this, cx| {
            let (report, launchers) = task.await;
            let _ = this.update(cx, |panel, cx| {
                panel.launchers = launchers.clone();
                panel.loading = false;
                panel.memory_usage_text = sample_memory_usage_text();
                panel.apply_report(report.clone());
                if let Some(cb) = &panel.callbacks.on_state_changed {
                    cb(report, launchers, false, cx);
                }
                if panel.refresh_pending {
                    panel.refresh_pending = false;
                    panel.trigger_refresh(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts a rescan, or queues one to run right after the in-flight scan finishes.
    fn request_refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            self.refresh_pending = true;
        } else {
            self.trigger_refresh(cx);
        }
    }

    /// Persists the configuration and reports a failure as a notification.
    fn save_config_or_notify(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(err) = self.app_config.save() {
            window.push_notification(
                Notification::error(tr_fmt!(
                    "保存设置失败：{err}",
                    "Failed to save settings: {err}"
                )),
                cx,
            );
        }
    }

    /// Enables or disables scanning and display of an agent, then rescans.
    fn set_agent_enabled(
        &mut self,
        agent: AgentKind,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.app_config.set_agent_enabled(agent, enabled) {
            return;
        }
        if !enabled && self.selected_agent == Some(agent) {
            self.selected_agent = None;
        }
        self.save_config_or_notify(window, cx);
        self.request_refresh(cx);
        cx.notify();
    }

    /// Hides a project from the list and records it in the hidden list.
    fn hide_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .report
            .projects
            .iter()
            .find(|p| p.path == path)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        if !self.app_config.hide_project(&path) {
            return;
        }
        self.save_config_or_notify(window, cx);

        let mut updated = (*self.report).clone();
        updated.remove_project(&path);
        let report_arc = Arc::new(updated);
        self.apply_report(report_arc.clone());
        if let Some(cb) = &self.callbacks.on_state_changed {
            cb(report_arc, self.launchers.clone(), self.loading, cx);
        }
        window.push_notification(
            Notification::success(tr_fmt!(
                "已隐藏项目：{name}（可在设置中取消隐藏）",
                "Project hidden: {name} (can be unhidden in Settings)"
            )),
            cx,
        );
        cx.notify();
    }

    /// Removes a project from the hidden list and rescans to bring it back.
    fn unhide_project(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.app_config.unhide_project_key(key) {
            return;
        }
        self.save_config_or_notify(window, cx);
        self.request_refresh(cx);
        cx.notify();
    }

    /// Opens inline custom name editing for the specified project in the detail panel.
    fn begin_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let initial_value = self
            .report
            .projects
            .iter()
            .find(|p| p.path == path)
            .and_then(|p| p.custom_name.clone())
            .unwrap_or_default();

        self.selected_project_path = Some(path.clone());
        self.editing_project_path = Some(path);
        self.rename_input.update(cx, |input, cx| {
            input.set_value(initial_value, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Exits inline custom name editing without saving changes.
    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.editing_project_path.take().is_some() {
            cx.notify();
        }
    }

    /// Saves the current value in the rename input for the project being edited.
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.editing_project_path.clone() else {
            return;
        };
        let value = self.rename_input.read(cx).value().to_string();
        self.save_custom_name(&path, Some(value.as_str()), window, cx);
    }

    /// Persists a ProjPanel custom name override and updates the active report in memory.
    fn save_custom_name(
        &mut self,
        path: &Path,
        raw_name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.app_config.set_custom_name(path, raw_name);
        self.editing_project_path = None;

        if !changed {
            cx.notify();
            return;
        }

        if let Err(err) = self.app_config.save() {
            window.push_notification(
                Notification::error(tr_fmt!(
                    "保存自定义名称失败：{err}",
                    "Failed to save custom name: {err}"
                )),
                cx,
            );
            cx.notify();
            return;
        }

        let next_custom = self.app_config.custom_name_for(path);
        let mut updated_report = (*self.report).clone();
        for proj in &mut updated_report.projects {
            if proj.path == path {
                proj.resolve_display_names(next_custom.clone());
            }
        }
        let report_arc = Arc::new(updated_report);
        self.apply_report(report_arc.clone());
        if let Some(cb) = &self.callbacks.on_state_changed {
            cb(report_arc, self.launchers.clone(), self.loading, cx);
        }

        let msg = match next_custom {
            Some(name) => tr_fmt!("已设置自定义名称：{name}", "Custom name set: {name}"),
            None => tr!("已恢复默认项目名称", "Default project name restored").to_string(),
        };
        window.push_notification(Notification::success(msg), cx);
        cx.notify();
    }

    /// Renders a compact dropdown button next to the project name listing custom names from
    /// other editors/agents (with agent icons) for read-only inspection and click-to-copy.
    fn render_external_names_dropdown(
        &self,
        id_prefix: &str,
        proj: &AggregatedProject,
        cached: &CachedProject,
    ) -> Option<impl IntoElement> {
        if !proj.should_show_name_dropdown() {
            return None;
        }

        let stable_key = &cached.stable_key;
        let external_names = proj.external_names.clone();
        let has_custom = proj.custom_name.is_some();

        let trigger = Button::new(SharedString::from(format!(
            "{id_prefix}-names-btn-{stable_key}"
        )))
        .ghost()
        .xsmall()
        .icon(IconName::ChevronDown)
        .tooltip(tr!(
            "查看其他软件自定义名称（点击复制）",
            "View custom names from other apps (click to copy)"
        ))
        .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _window, cx| {
            menu = menu.label(tr!(
                "其他软件自定义名称（点击复制）",
                "Custom names from other apps (click to copy)"
            ));
            for (idx, item) in external_names.iter().enumerate() {
                let agent = item.agent;
                let name_to_copy = item.name.clone();
                let is_active_display = !has_custom && idx == 0;
                let entry = PopupMenuItem::new(item.name.clone())
                    .icon(agent_icon(agent, cx))
                    .checked(is_active_display)
                    .on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(name_to_copy.clone()));
                        window.push_notification(
                            Notification::success(tr_fmt!(
                                "已复制名称（{}）：{}",
                                "Name copied ({}): {}",
                                agent.short_label(),
                                name_to_copy
                            )),
                            cx,
                        );
                    });
                menu = menu.item(entry);
            }

            menu.check_side(Side::Right)
                .min_w(px(184.0))
                .max_h(px(300.0))
                .scrollable(true)
        });

        Some(
            div()
                .id(SharedString::from(format!(
                    "{id_prefix}-names-wrap-{stable_key}"
                )))
                .flex_none()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(trigger),
        )
    }

    /// Launches a project directory with the chosen external tool and records the selection.
    fn launch_project(
        &mut self,
        launcher: &DetectedLauncher,
        project_path: &Path,
        project_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project_launcher_prefs
            .insert(project_path.to_path_buf(), launcher.kind);
        self.last_launcher = Some(launcher.kind);
        if let Some(cb) = &self.callbacks.on_launcher_used {
            cb(project_path.to_path_buf(), launcher.kind, cx);
        }
        match open_project_with(launcher, project_path) {
            Ok(()) => {
                window.push_notification(
                    Notification::success(tr_fmt!(
                        "已使用 {} 打开：{}",
                        "Opened with {}: {}",
                        launcher.kind.display_name(),
                        project_name
                    )),
                    cx,
                );
            }
            Err(err) => {
                window.push_notification(
                    Notification::error(tr_fmt!(
                        "使用 {} 打开失败：{err}",
                        "Failed to open with {}: {err}",
                        launcher.kind.display_name()
                    )),
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Renders a split button for opening an existing project in an installed editor, AI app, or terminal.
    fn render_open_in_split_button(
        &self,
        id_prefix: &str,
        proj: &AggregatedProject,
        cached: &CachedProject,
        detail_mode: bool,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !proj.exists {
            return None;
        }
        let primary = resolve_primary_launcher(
            proj,
            &self.launchers,
            &self.project_launcher_prefs,
            self.last_launcher,
        )?
        .clone();

        let stable_key = &cached.stable_key;
        let primary_path = proj.path.clone();
        let primary_name = cached.name.clone();
        let primary_launcher = primary.clone();
        let primary_label = if detail_mode {
            tr_fmt!("用 {} 打开", "Open with {}", primary.kind.short_label())
        } else {
            primary.kind.short_label().to_string()
        };

        let left_button = Button::new(SharedString::from(format!(
            "{id_prefix}-btn-{}-{stable_key}",
            primary.kind.id()
        )))
        .icon(launcher_icon(primary.kind, cx))
        .label(primary_label)
        .tooltip(tr_fmt!(
            "使用 {} 打开项目",
            "Open project with {}",
            primary.kind.display_name()
        ))
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.launch_project(&primary_launcher, &primary_path, &primary_name, window, cx);
        }));

        let menu_project_path = proj.path.clone();
        let menu_project_name = cached.name.clone();
        let primary_kind = primary.kind;
        let view = cx.entity().downgrade();

        let split = DropdownButton::new(SharedString::from(format!(
            "{id_prefix}-split-{stable_key}"
        )))
        .button(left_button)
        .menu_accessibility_label(tr!("选择打开方式", "Choose how to open"))
        .small()
        .outline()
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _window, cx| {
            let Some(view_entity) = view.upgrade() else {
                return menu;
            };
            let (launchers, matched_kinds, app_kinds, terminal_kinds) = {
                let panel = view_entity.read(cx);
                let Some(proj_ref) = panel
                    .report
                    .projects
                    .iter()
                    .find(|p| p.path == menu_project_path)
                else {
                    return menu;
                };
                let (matched, apps, terminals) =
                    partition_launchers_for_project(proj_ref, &panel.launchers);
                (
                    panel.launchers.clone(),
                    matched.into_iter().map(|l| l.kind).collect::<Vec<_>>(),
                    apps.into_iter().map(|l| l.kind).collect::<Vec<_>>(),
                    terminals.into_iter().map(|l| l.kind).collect::<Vec<_>>(),
                )
            };

            let add_item = |menu: PopupMenu, kind: LauncherKind, cx: &App| {
                let Some(target) = launchers.iter().find(|l| l.kind == kind).cloned() else {
                    return menu;
                };
                let path = menu_project_path.clone();
                let name = menu_project_name.clone();
                let view = view.clone();
                let is_current = kind == primary_kind;
                let item = PopupMenuItem::new(kind.display_name())
                    .icon(launcher_icon(kind, cx))
                    .checked(is_current)
                    .on_click(move |_, window, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| {
                                this.launch_project(&target, &path, &name, window, cx);
                            });
                        }
                    });
                menu.item(item)
            };

            let mut has_prev_section = false;
            if !matched_kinds.is_empty() {
                menu = menu.label(tr!("项目关联工具", "Project-linked tools"));
                for kind in matched_kinds {
                    menu = add_item(menu, kind, cx);
                }
                has_prev_section = true;
            }

            if !app_kinds.is_empty() {
                if has_prev_section {
                    menu = menu.separator();
                }
                menu = menu.label(tr!("编辑器与 AI 应用", "Editors & AI apps"));
                for kind in app_kinds {
                    menu = add_item(menu, kind, cx);
                }
                has_prev_section = true;
            }

            if !terminal_kinds.is_empty() {
                if has_prev_section {
                    menu = menu.separator();
                }
                menu = menu.label(tr!("终端", "Terminal"));
                for kind in terminal_kinds {
                    menu = add_item(menu, kind, cx);
                }
            }

            menu.check_side(Side::Right)
                .min_w(px(176.0))
                .max_h(px(340.0))
                .scrollable(true)
        });

        Some(
            div()
                .id(SharedString::from(format!("{id_prefix}-wrap-{stable_key}")))
                .flex_none()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(split),
        )
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_dark = cx.theme().is_dark();

        TitleBar::new().child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .pr_2()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            SidebarToggleButton::new()
                                .side(Side::Left)
                                .collapsed(self.sidebar_collapsed)
                                .accessibility_label(tr!("切换侧边栏", "Toggle sidebar"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.sidebar_collapsed = !this.sidebar_collapsed;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child("ProjPanel"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr!(
                                    "AI Agent 统一项目管理",
                                    "Unified AI Agent project management"
                                )),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("titlebar-refresh")
                                .ghost()
                                .small()
                                .icon(IconName::RefreshCw)
                                .loading(self.loading)
                                .tooltip(tr!(
                                    "重新扫描所有 AI Agent (⌘/Ctrl + R)",
                                    "Rescan all AI Agents (⌘/Ctrl + R)"
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.trigger_refresh(cx);
                                })),
                        )
                        .child(
                            Button::new("titlebar-settings")
                                .ghost()
                                .small()
                                .icon(IconName::Settings)
                                .tooltip(tr!("设置", "Settings"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_settings(window, cx);
                                })),
                        )
                        .child(
                            Button::new("titlebar-theme")
                                .ghost()
                                .small()
                                .icon(if is_dark {
                                    IconName::Sun
                                } else {
                                    IconName::Moon
                                })
                                .tooltip(if is_dark {
                                    tr!("切换到浅色主题", "Switch to light theme")
                                } else {
                                    tr!("切换到深色主题", "Switch to dark theme")
                                })
                                .on_click(|_, window, cx| {
                                    let next = if cx.theme().is_dark() {
                                        ThemeMode::Light
                                    } else {
                                        ThemeMode::Dark
                                    };
                                    Theme::change(next, Some(window), cx);
                                    apply_dock_icon(next.is_dark());
                                }),
                        ),
                ),
        )
    }

    /// 渲染侧栏，工作台入口沿用设置标签的字号和字重。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let existing_count = self.metrics.existing_count;
        let workspace_label_style = StyleRefinement::default()
            .text_sm()
            .font_weight(FontWeight::NORMAL);
        let view_menu = SidebarMenu::new()
            .child(
                SidebarMenuItem::new(tr!("项目列表", "Projects"))
                    .label_style(workspace_label_style.clone())
                    .icon(IconName::FolderOpen)
                    .active(
                        self.active_view == ActiveView::Projects && self.selected_agent.is_none(),
                    )
                    .suffix(move |_, cx| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{existing_count}"))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.active_view = ActiveView::Projects;
                        if this.selected_agent.take().is_some() {
                            this.rebuild_filtered(true);
                        }
                        cx.notify();
                    })),
            )
            .child(
                SidebarMenuItem::new(tr!("活跃度概览", "Activity overview"))
                    .label_style(workspace_label_style)
                    .icon(IconName::ChartPie)
                    .active(self.active_view == ActiveView::Activity)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.active_view = ActiveView::Activity;
                        cx.notify();
                    })),
            );

        let mut agent_menu = SidebarMenu::new();

        for &(agent, count) in &self.active_agents {
            let selected =
                self.active_view == ActiveView::Projects && self.selected_agent == Some(agent);
            agent_menu = agent_menu.child(
                SidebarMenuItem::new(agent.display_name())
                    .icon(agent_icon(agent, cx))
                    .active(selected)
                    .suffix(move |_, cx| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{count}"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.selected_agent == Some(agent)
                            && this.active_view == ActiveView::Projects
                        {
                            this.selected_agent = None;
                        } else {
                            this.selected_agent = Some(agent);
                            this.active_view = ActiveView::Projects;
                        }
                        this.rebuild_filtered(true);
                        cx.notify();
                    })),
            );
        }

        let collapsed = self.sidebar_collapsed;

        Sidebar::new("main-sidebar")
            .side(Side::Left)
            .collapsible(SidebarCollapsible::Icon)
            .collapsed(collapsed)
            .w(px(232.0))
            .child(SidebarGroup::new(tr!("工作台", "Workspace")).child(view_menu))
            .child(SidebarGroup::new(tr!("AI Agent 筛选", "AI Agent filter")).child(agent_menu))
    }

    fn render_projects_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_project_idx = self
            .selected_project_path
            .as_ref()
            .and_then(|p| self.report.projects.iter().position(|proj| &proj.path == p));
        let missing_count = self.metrics.missing_count;
        let show_missing = self.show_missing;

        let toolbar = h_flex()
            .w_full()
            .pt_4()
            .px_4()
            .flex_wrap()
            .items_center()
            .gap_x_3()
            .gap_y_2()
            .child(
                div().flex_1().min_w(px(200.0)).max_w(px(440.0)).child(
                    Input::new(&self.search_input)
                        .small()
                        .cleanable(true)
                        .prefix(
                            Icon::new(IconName::Search)
                                .small()
                                .text_color(cx.theme().muted_foreground),
                        ),
                ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .whitespace_nowrap()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr_fmt!(
                                "显示已失效路径 ({missing_count})",
                                "Show missing paths ({missing_count})"
                            )),
                    )
                    .child(
                        Switch::new("switch-show-missing")
                            .small()
                            .checked(show_missing)
                            .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                if this.show_missing != *checked {
                                    this.show_missing = *checked;
                                    this.rebuild_filtered(true);
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .child(
                h_flex()
                    .ml_auto()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .when_some(self.selected_agent, |row, agent| {
                        row.child(
                            h_flex()
                                .flex_shrink_0()
                                .items_center()
                                .gap_1()
                                .child(
                                    Tag::color(agent_color_name(agent)).rounded_full().child(
                                        h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(Icon::new(AgentIcon(agent)).xsmall())
                                            .child(tr_fmt!(
                                                "筛选: {}",
                                                "Filter: {}",
                                                agent.display_name()
                                            )),
                                    ),
                                )
                                .child(
                                    Button::new("btn-clear-agent-filter")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Close)
                                        .tooltip(tr!("清除 Agent 筛选", "Clear Agent filter"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.selected_agent = None;
                                            this.rebuild_filtered(true);
                                            cx.notify();
                                        })),
                                ),
                        )
                    })
                    .child(
                        div().flex_shrink_0().child(
                            TabBar::new("sort-by-tabs")
                                .segmented()
                                .selected_index(self.sort_by.to_index())
                                .on_click(cx.listener(|this, idx: &usize, _, cx| {
                                    let next = SortBy::from_index(*idx);
                                    if this.sort_by != next {
                                        this.sort_by = next;
                                        this.rebuild_filtered(true);
                                        cx.notify();
                                    }
                                }))
                                .child(Tab::new().label(tr!("最近活跃", "Last active")))
                                .child(Tab::new().label(tr!("名称", "Name")))
                                .child(Tab::new().label(tr!("Agent 数", "Agents"))),
                        ),
                    ),
            );

        let list_area = if self.filtered_indices.is_empty() {
            let has_filter = !self.query.trim().is_empty() || self.selected_agent.is_some();
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .px_4()
                .pb_4()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Empty::new()
                        .header(
                            EmptyHeader::new()
                                .media(
                                    EmptyMedia::new()
                                        .with_variant(EmptyMediaVariant::Icon)
                                        .child(Icon::new(IconName::Inbox)),
                                )
                                .title(EmptyTitle::new().child(if self.loading {
                                    tr!("正在扫描本机 AI Agent 项目...", "Scanning local AI Agent projects...")
                                } else {
                                    tr!("没有匹配的项目", "No matching projects")
                                }))
                                .description(EmptyDescription::new().child(if self.loading {
                                    tr!("请稍候，正在读取各工具的工作区与会话记录。", "Please wait while workspace and session records are being read.")
                                } else if has_filter {
                                    tr!("尝试调整搜索关键词或切换左侧 AI Agent 筛选条件。", "Try adjusting the search keywords or the AI Agent filter on the left.")
                                } else {
                                    tr!("未在本机检测到已知 AI Agent 或编辑器的项目记录。", "No project records from known AI Agents or editors were found on this machine.")
                                })),
                        )
                        .when(has_filter && !self.loading, |empty| {
                            empty.content(
                                EmptyContent::new().child(
                                    Button::new("btn-reset-all-filters")
                                        .outline()
                                        .small()
                                        .label(tr!("重置筛选条件", "Reset filters"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.selected_agent = None;
                                            this.query.clear();
                                            this.rebuild_filtered(true);
                                            this.search_input.update(cx, |input, cx| {
                                                input.set_value("", window, cx);
                                            });
                                            cx.notify();
                                        })),
                                ),
                            )
                        }),
                )
                .into_any_element()
        } else {
            let item_count = self.filtered_indices.len();
            let now_ts = Utc::now().timestamp();

            // Virtualized list renders only the visible subset of uniform-height cards.
            // Reserve 16px on the right so cards do not collide with the overlay scrollbar.
            let list = uniform_list(
                "projects-uniform-list",
                item_count,
                cx.processor(move |this, visible_range: Range<usize>, _window, cx| {
                    let mut items = Vec::with_capacity(visible_range.len());
                    for filtered_pos in visible_range {
                        if let Some(&proj_idx) = this.filtered_indices.get(filtered_pos) {
                            items.push(this.render_project_card(proj_idx, now_ts, cx));
                        }
                    }
                    items
                }),
            )
            .size_full()
            .pl_4()
            .pr_4()
            .pb_2()
            .track_scroll(&self.list_scroll_handle);

            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(list)
                .vertical_scrollbar(&self.list_scroll_handle)
                .into_any_element()
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .gap_3()
            .child(toolbar)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(list_area),
                    )
                    .when_some(selected_project_idx, |row, proj_idx| {
                        row.child(div().h_full().pb_4().child(Separator::vertical()))
                            .child(
                                div()
                                    .h_full()
                                    .pl_3()
                                    .pr_4()
                                    .pb_4()
                                    .child(self.render_project_detail(proj_idx, cx)),
                            )
                    }),
            )
    }

    fn render_project_card(
        &self,
        proj_idx: usize,
        now_ts: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let proj = &self.report.projects[proj_idx];
        let cached = &self.cached_projects[proj_idx];
        let is_selected = self.selected_project_path.as_ref() == Some(&proj.path);
        let path_for_select = proj.path.clone();
        let path_for_rename = proj.path.clone();
        let path_for_open = proj.path.clone();
        let path_for_copy = cached.full_path.clone();
        let proj_name_for_open = cached.name.clone();
        let time_text = format_relative_time(proj.last_active, now_ts, &cached.fallback_date);

        let mut badges = h_flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .items_center()
            .gap_1p5();
        for usage in proj.agents.iter().take(MAX_CARD_BADGES) {
            badges = badges.child(
                Tag::color(agent_color_name(usage.agent))
                    .rounded_full()
                    .flex_none()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(Icon::new(AgentIcon(usage.agent)).xsmall())
                            .child(usage.agent.short_label()),
                    ),
            );
        }
        if proj.agents.len() > MAX_CARD_BADGES {
            let extra = proj.agents.len() - MAX_CARD_BADGES;
            badges = badges.child(
                Tag::secondary()
                    .rounded_full()
                    .flex_none()
                    .child(format!("+{extra}")),
            );
        }

        let card = v_flex()
            .id(SharedString::from(format!("project-card-{proj_idx}")))
            .w_full()
            .h(rems(6.5))
            .min_w_0()
            .overflow_hidden()
            .justify_between()
            .px_3p5()
            .py_2p5()
            .rounded(cx.theme().radius)
            .border_1()
            .cursor_pointer()
            .bg(if is_selected {
                cx.theme().list_active
            } else {
                cx.theme().group_box
            })
            .border_color(if is_selected {
                cx.theme().list_active_border
            } else {
                cx.theme().border
            })
            .hover(|s| {
                if is_selected {
                    s
                } else {
                    s.bg(cx.theme().list_hover)
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.selected_project_path.as_ref() == Some(&path_for_select) {
                    this.selected_project_path = None;
                } else {
                    this.selected_project_path = Some(path_for_select.clone());
                }
                if this.editing_project_path != this.selected_project_path {
                    this.editing_project_path = None;
                }
                cx.notify();
            }))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .items_center()
                            .gap_1p5()
                            .min_w_0()
                            .child(div().size(px(8.0)).rounded_full().flex_none().bg(
                                if proj.exists {
                                    cx.theme().green
                                } else {
                                    cx.theme().muted_foreground
                                },
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if proj.exists {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .truncate()
                                    .child(cached.name.clone()),
                            )
                            .when_some(
                                self.render_external_names_dropdown("card", proj, cached),
                                |row, dropdown| row.child(dropdown),
                            )
                            .when(!proj.exists, |row| {
                                row.child(
                                    Tag::secondary()
                                        .rounded_full()
                                        .child(tr!("目录不存在", "Directory missing")),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .when_some(
                                self.render_open_in_split_button(
                                    "card-open-in",
                                    proj,
                                    cached,
                                    false,
                                    cx,
                                ),
                                |row, open_in| row.child(open_in),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(time_text),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(cached.display_path.clone()),
            )
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(badges)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .flex_none()
                            .child(
                                Button::new(SharedString::from(format!("rename-{proj_idx}")))
                                    .ghost()
                                    .xsmall()
                                    .icon(LucideIconName::Pencil)
                                    .tooltip(tr!("自定义项目名称", "Custom project name"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.begin_rename(path_for_rename.clone(), window, cx);
                                    })),
                            )
                            .when(proj.exists, |actions| {
                                actions.child(
                                    Button::new(SharedString::from(format!("open-{proj_idx}")))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::FolderOpen)
                                        .tooltip(tr!("在文件管理器中打开", "Open in file manager"))
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            match open_in_file_manager(&path_for_open) {
                                                Ok(()) => {
                                                    window.push_notification(
                                                        Notification::success(tr_fmt!(
                                                            "已打开目录：{}",
                                                            "Opened directory: {}",
                                                            proj_name_for_open
                                                        )),
                                                        cx,
                                                    );
                                                }
                                                Err(err) => {
                                                    window.push_notification(
                                                        Notification::error(tr_fmt!(
                                                            "打开目录失败：{err}",
                                                            "Failed to open directory: {err}"
                                                        )),
                                                        cx,
                                                    );
                                                }
                                            }
                                        }),
                                )
                            })
                            .child(
                                Button::new(SharedString::from(format!("copy-{proj_idx}")))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Copy)
                                    .tooltip(tr!("复制完整路径", "Copy full path"))
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            path_for_copy.to_string(),
                                        ));
                                        window.push_notification(
                                            Notification::success(tr!(
                                                "已复制项目路径到剪贴板",
                                                "Project path copied to clipboard"
                                            )),
                                            cx,
                                        );
                                    }),
                            ),
                    ),
            );

        div().w_full().pb_2().child(card)
    }

    fn render_project_detail(&self, proj_idx: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let proj = &self.report.projects[proj_idx];
        let cached = &self.cached_projects[proj_idx];
        let path_for_open = proj.path.clone();
        let path_for_rename = proj.path.clone();
        let path_for_reset = proj.path.clone();
        let path_for_hide = proj.path.clone();
        let path_for_copy = cached.full_path.clone();
        let proj_name = cached.name.clone();
        let is_editing = self.editing_project_path.as_ref() == Some(&proj.path);
        let has_custom_name = proj.custom_name.is_some();
        let show_default_name_hint = proj.name != proj.default_name;
        let default_name_hint = tr_fmt!("目录原名：{}", "Original name: {}", proj.default_name);

        let mut usages = v_flex().w_full().min_w_0().gap_2().px_4().pb_3();
        for usage in &proj.agents {
            let session_text = match usage.session_count {
                Some(n) => tr_fmt!("{n} 次会话", "{n} sessions"),
                None => tr!("已关联工作区", "Workspace linked").to_string(),
            };
            let agent_custom_name = usage
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(&proj.default_name))
                .map(|s| s.to_string());

            usages = usages.child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .p_2p5()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted.opacity(0.45))
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                Tag::color(agent_color_name(usage.agent))
                                    .rounded_full()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(Icon::new(AgentIcon(usage.agent)).xsmall())
                                            .child(usage.agent.display_name()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(session_text),
                            ),
                    )
                    .when_some(agent_custom_name, |card, custom_alias| {
                        card.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(tr_fmt!(
                                    "自定义名称：{custom_alias}",
                                    "Custom name: {custom_alias}"
                                )),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child(tr_fmt!(
                                "最近活跃：{}",
                                "Last active: {}",
                                format_exact_time(usage.last_active)
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(tr_fmt!("来源：{}", "Source: {}", usage.source)),
                    ),
            );
        }

        let header_row = if is_editing {
            h_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&self.rename_input).small()),
                )
                .child(
                    Button::new("btn-save-rename")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Check)
                        .tooltip(tr!("保存自定义名称 (Enter)", "Save custom name (Enter)"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.commit_rename(window, cx);
                        })),
                )
                .when(has_custom_name, |row| {
                    row.child(
                        Button::new("btn-reset-rename")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Undo2)
                            .tooltip(tr!(
                                "清除自定义名称，恢复默认",
                                "Clear custom name and restore default"
                            ))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.save_custom_name(&path_for_reset, None, window, cx);
                            })),
                    )
                })
                .child(
                    Button::new("btn-cancel-rename")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip(tr!("取消编辑", "Cancel editing"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_rename(cx);
                        })),
                )
        } else {
            h_flex()
                .items_center()
                .justify_between()
                .gap_1p5()
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .min_w_0()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(cached.name.clone()),
                        )
                        .when_some(
                            self.render_external_names_dropdown("detail", proj, cached),
                            |row, dropdown| row.child(dropdown),
                        ),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("btn-edit-custom-name")
                                .ghost()
                                .xsmall()
                                .icon(LucideIconName::Pencil)
                                .tooltip(tr!("自定义项目名称", "Custom project name"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.begin_rename(path_for_rename.clone(), window, cx);
                                })),
                        )
                        .child(
                            Button::new("btn-close-detail")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip(tr!("关闭详情", "Close details"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.selected_project_path = None;
                                    this.editing_project_path = None;
                                    cx.notify();
                                })),
                        ),
                )
        };

        v_flex()
            .w(px(308.0))
            .flex_none()
            .h_full()
            .min_h_0()
            .overflow_hidden()
            .gap_3()
            .pt_3()
            .rounded(cx.theme().radius)
            .bg(cx.theme().group_box)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .px_4()
                    .child(header_row)
                    .when(show_default_name_hint, |col| {
                        col.child(
                            h_flex()
                                .items_center()
                                .gap_1p5()
                                .when(has_custom_name, |row| {
                                    row.child(
                                        Tag::secondary()
                                            .rounded_full()
                                            .child(tr!("已自定义", "Customized")),
                                    )
                                })
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(default_name_hint),
                                ),
                        )
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(if proj.exists {
                                Tag::color(ColorName::Green)
                                    .rounded_full()
                                    .child(tr!("本地存在", "Exists locally"))
                            } else {
                                Tag::secondary()
                                    .rounded_full()
                                    .child(tr!("目录已移除", "Directory removed"))
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr_fmt!(
                                        "最近：{}",
                                        "Recent: {}",
                                        format_exact_time(proj.last_active)
                                    )),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr!("完整路径", "Full path")),
                            )
                            .child(
                                div()
                                    .p_2()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().muted.opacity(0.4))
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(cached.full_path.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .when_some(
                                self.render_open_in_split_button(
                                    "detail-open-in",
                                    proj,
                                    cached,
                                    true,
                                    cx,
                                ),
                                |row, open_in| row.child(open_in),
                            )
                            .when(proj.exists, |row| {
                                row.child(
                                    Button::new("detail-open-dir")
                                        .outline()
                                        .small()
                                        .icon(IconName::FolderOpen)
                                        .label(tr!("打开目录", "Open directory"))
                                        .on_click(move |_, window, cx| match open_in_file_manager(
                                            &path_for_open,
                                        ) {
                                            Ok(()) => {
                                                window.push_notification(
                                                    Notification::success(tr_fmt!(
                                                        "已打开目录：{proj_name}",
                                                        "Opened directory: {proj_name}"
                                                    )),
                                                    cx,
                                                );
                                            }
                                            Err(err) => {
                                                window.push_notification(
                                                    Notification::error(tr_fmt!(
                                                        "打开失败：{err}",
                                                        "Failed to open: {err}"
                                                    )),
                                                    cx,
                                                );
                                            }
                                        }),
                                )
                            })
                            .child(
                                Button::new("detail-copy-path")
                                    .outline()
                                    .small()
                                    .icon(IconName::Copy)
                                    .label(tr!("复制路径", "Copy path"))
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            path_for_copy.to_string(),
                                        ));
                                        window.push_notification(
                                            Notification::success(tr!(
                                                "已复制完整路径",
                                                "Full path copied"
                                            )),
                                            cx,
                                        );
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        Icon::new(IconName::Eye)
                                            .xsmall()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().foreground)
                                            .child(tr!("在项目列表中显示", "Show in project list")),
                                    ),
                            )
                            .child(
                                Switch::new("detail-hide-switch")
                                    .small()
                                    .checked(true)
                                    .on_change(cx.listener(
                                        move |this, checked: &bool, window, cx| {
                                            if !*checked {
                                                this.hide_project(
                                                    path_for_hide.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        },
                                    )),
                            ),
                    )
                    .child(Separator::horizontal())
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(tr_fmt!(
                                "关联 AI Agent ({})",
                                "Linked AI Agents ({})",
                                proj.agents.len()
                            )),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(usages),
            )
    }

    fn render_activity_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let max_count = self
            .active_agents
            .first()
            .map(|(_, c)| *c)
            .unwrap_or(1)
            .max(1);
        let total_projects = self.report.projects.len().max(1);

        let mut agent_rows = v_flex().gap_2();
        if self.active_agents.is_empty() {
            agent_rows = agent_rows.child(
                div()
                    .py_4()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr!(
                        "暂未检测到活跃的 AI Agent 项目记录。",
                        "No active AI Agent project records detected yet."
                    )),
            );
        } else {
            for &(agent, count) in &self.active_agents {
                let bar_pct = (count as f32 / max_count as f32) * 100.0;
                let share_pct = ((count * 100) / total_projects).min(100);
                let color = agent_accent(agent, cx);

                agent_rows = agent_rows.child(
                    h_flex()
                        .id(SharedString::from(format!("activity-agent-{agent:?}")))
                        .items_center()
                        .gap_3()
                        .px_2()
                        .py_1p5()
                        .rounded(cx.theme().radius)
                        .cursor_pointer()
                        .hover(|s| s.bg(cx.theme().list_hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected_agent = Some(agent);
                            this.active_view = ActiveView::Projects;
                            this.rebuild_filtered(true);
                            cx.notify();
                        }))
                        .child(
                            h_flex()
                                .w(px(136.0))
                                .flex_none()
                                .items_center()
                                .gap_2()
                                .child(agent_icon(agent, cx).small())
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(cx.theme().foreground)
                                        .truncate()
                                        .child(agent.display_name()),
                                ),
                        )
                        .child(
                            div().flex_1().child(
                                Progress::new(SharedString::from(format!("prog-{agent:?}")))
                                    .value(bar_pct)
                                    .color(color),
                            ),
                        )
                        .child(
                            h_flex()
                                .w(px(92.0))
                                .flex_none()
                                .justify_end()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child(tr_fmt!("{count} 项目", "{count} projects")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{share_pct}%")),
                                ),
                        ),
                );
            }
        }

        let mut scanner_rows = v_flex().gap_1p5();
        for summary in &self.report.agent_summaries {
            let status_tag = if let Some(err) = &summary.error {
                Tag::color(ColorName::Red)
                    .rounded_full()
                    .child(tr_fmt!("扫描失败: {err}", "Scan failed: {err}"))
            } else if summary.project_count > 0 {
                Tag::color(ColorName::Green).rounded_full().child(tr_fmt!(
                    "{} 个项目",
                    "{} projects",
                    summary.project_count
                ))
            } else {
                Tag::secondary()
                    .rounded_full()
                    .child(tr!("未检测到数据", "No data detected"))
            };

            scanner_rows = scanner_rows.child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(agent_icon(summary.agent, cx).small())
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground)
                                    .child(summary.agent.display_name()),
                            ),
                    )
                    .child(status_tag),
            );
        }

        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .p_4()
                    .child(
                        GroupBox::new()
                            .outline()
                            .title(tr!("Agent 项目分布", "Projects by Agent"))
                            .child(agent_rows),
                    )
                    .child(self.render_heatmap_group(cx))
                    .child(
                        GroupBox::new()
                            .outline()
                            .title(tr!("扫描器诊断状态", "Scanner diagnostics"))
                            .child(scanner_rows),
                    ),
            )
    }

    fn render_heatmap_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cache = &self.heatmap_cache;
        let start_day = cache.start_day;
        let active_days = cache.active_days;
        let max_daily = cache.max_daily;

        let mut month_headers = h_flex()
            .w_full()
            .justify_between()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        for step in [0usize, 4, 8, 12, 16, 20, 23] {
            let d = start_day + Duration::days((step * HEATMAP_DAYS) as i64);
            month_headers = month_headers.child(crate::i18n::month_label(d.month()));
        }

        let mut grid = h_flex().w_full().justify_between().gap_1();
        for w in 0..HEATMAP_WEEKS {
            let mut col = v_flex().gap_1();
            for d in 0..HEATMAP_DAYS {
                let date = start_day + Duration::days((w * HEATMAP_DAYS + d) as i64);
                let c = cache.counts_by_day.get(&date).copied().unwrap_or(0);
                let level = if c == 0 {
                    0
                } else if c * 4 <= max_daily {
                    1
                } else if c * 2 <= max_daily {
                    2
                } else if c * 4 <= max_daily * 3 {
                    3
                } else {
                    4
                };
                col = col.child(
                    div()
                        .size(px(14.0))
                        .rounded(px(3.0))
                        .bg(heat_level_color(level, cx)),
                );
            }
            grid = grid.child(col);
        }

        let mut legend = h_flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(tr!("少", "Less"));
        for lvl in 0..=4u8 {
            legend = legend.child(
                div()
                    .size(px(10.0))
                    .rounded(px(2.0))
                    .bg(heat_level_color(lvl, cx)),
            );
        }
        legend = legend.child(tr!("多", "More"));

        GroupBox::new()
            .outline()
            .title(tr_fmt!("近 24 周项目活跃度（共 {active_days} 个活跃日）", "Project activity over the last 24 weeks ({active_days} active days)"))
            .child(
                v_flex().gap_2().child(month_headers).child(grid).child(
                    h_flex()
                        .pt_1()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr_fmt!(
                                    "汇总自 {} 个项目的 Agent 活跃时间戳 · 耗时 {} ms", "Aggregated from Agent activity timestamps of {} projects · took {} ms",
                                    self.report.projects.len(),
                                    self.report.elapsed_ms
                                )),
                        )
                        .child(legend),
                ),
            )
    }

    fn render_status_bar(&self, visible_count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let left_text = match (self.loading, &self.memory_usage_text) {
            (true, Some(mem)) => tr_fmt!(
                "内存 {mem} · 正在扫描本机 AI Agent 项目...",
                "Memory {mem} · Scanning local AI Agent projects..."
            ),
            (true, None) => tr!(
                "正在扫描本机 AI Agent 项目...",
                "Scanning local AI Agent projects..."
            )
            .to_string(),
            (false, Some(mem)) => tr_fmt!(
                "内存 {mem} · 扫描耗时 {} ms",
                "Memory {mem} · Scan took {} ms",
                self.report.elapsed_ms
            ),
            (false, None) => tr_fmt!("扫描耗时 {} ms", "Scan took {} ms", self.report.elapsed_ms),
        };

        let right_text = tr_fmt!(
            "当前显示 {} / {} 个项目 · ⌘/Ctrl+R 刷新",
            "Showing {} / {} projects · ⌘/Ctrl+R to refresh",
            visible_count,
            self.report.projects.len()
        );

        StatusBar::new()
            .left(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().size(px(7.0)).rounded_full().bg(if self.loading {
                        cx.theme().yellow
                    } else {
                        cx.theme().green
                    }))
                    .child(left_text),
            )
            .right(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(right_text),
            )
    }
}

/// 设置窗口的常规、软件启用和隐藏项目面板。
struct SettingsPanel {
    parent: WeakEntity<ProjectWindow>,
    /// 当前选中的标签页索引。
    tab: usize,
    font_select: Entity<SelectState<SearchableVec<String>>>,
    _font_subscription: Subscription,
}

impl SettingsPanel {
    /// 渲染语言切换、字号调节和系统字体选择。
    fn render_general(&self, font_size: u8, cx: &mut Context<Self>) -> impl IntoElement {
        let mut language_buttons = h_flex().gap_2();
        for language in Language::ALL {
            let button = Button::new(SharedString::from(format!("lang-{}", language.index())))
                .small()
                .label(language.native_name())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.with_parent(cx, |panel, cx| panel.set_language(language, window, cx));
                }));
            language_buttons = language_buttons.child(if language == i18n::current() {
                button.primary()
            } else {
                button.outline()
            });
        }

        let mut size_controls = h_flex().gap_2().items_center();
        for (id, label, description, next_size, disabled) in [
            (
                "font-smaller",
                "−",
                tr!("缩小文字", "Decrease text size"),
                font_size - 1,
                font_size == MIN_UI_FONT_SIZE,
            ),
            (
                "font-larger",
                "+",
                tr!("放大文字", "Increase text size"),
                font_size + 1,
                font_size == MAX_UI_FONT_SIZE,
            ),
            (
                "font-size-reset",
                tr!("恢复默认", "Reset"),
                tr!("恢复默认字号", "Reset text size"),
                DEFAULT_UI_FONT_SIZE,
                font_size == DEFAULT_UI_FONT_SIZE,
            ),
        ] {
            size_controls = size_controls.child(
                Button::new(id)
                    .outline()
                    .small()
                    .label(label)
                    .accessibility_label(description)
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.with_parent(cx, |panel, cx| {
                            panel.app_config.ui_font_size =
                                (next_size != DEFAULT_UI_FONT_SIZE).then_some(next_size);
                            panel.apply_and_save_typography(window, cx);
                        });
                    })),
            );
        }

        let font_label = tr!("文字字体", "Font family");
        v_flex()
            .w_full()
            .gap_4()
            .child(
                v_flex()
                    .gap_2()
                    .child(tr!("界面语言", "Interface language"))
                    .child(language_buttons),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(tr_fmt!("文字大小 · {font_size} px", "Text size · {font_size} px"))
                    .child(size_controls),
            )
            .child(
                v_flex().gap_2().child(font_label).child(
                    Select::new(&self.font_select)
                        .accessibility_label(font_label)
                        .placeholder(tr!("跟随系统", "System default"))
                        .search_placeholder(tr!("搜索字体", "Search fonts"))
                        .cleanable(true)
                        .w_full(),
                ),
            )
    }

    /// 更新主窗口状态并刷新设置面板。
    fn with_parent(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ProjectWindow, &mut Context<ProjectWindow>),
    ) {
        if let Some(parent) = self.parent.upgrade() {
            parent.update(cx, f);
        }
        cx.notify();
    }
}

impl Render for SettingsPanel {
    /// 渲染当前设置标签页，列表正文使用用户设置的字号。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(parent) = self.parent.upgrade() else {
            return div().into_any_element();
        };
        let (enabled_flags, mut hidden, font_size) = {
            let panel = parent.read(cx);
            let flags: Vec<(AgentKind, bool)> = AgentKind::ALL
                .iter()
                .map(|&agent| (agent, panel.app_config.is_agent_enabled(agent)))
                .collect();
            let hidden: Vec<(String, String)> = panel
                .app_config
                .hidden_projects
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (flags, hidden, panel.app_config.effective_ui_font_size())
        };
        hidden.sort_by(|a, b| a.1.cmp(&b.1));
        let content_font_size = px(f32::from(font_size));

        let mut agent_rows = v_flex().gap_1();
        for (agent, enabled) in enabled_flags {
            agent_rows = agent_rows.child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1p5()
                    .rounded(cx.theme().radius)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(agent_icon(agent, cx).small())
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if enabled {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(agent.display_name()),
                            ),
                    )
                    .child(
                        Switch::new(SharedString::from(format!("switch-agent-{}", agent.id())))
                            .small()
                            .checked(enabled)
                            .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                                let checked = *checked;
                                this.with_parent(cx, |panel, pcx| {
                                    panel.set_agent_enabled(agent, checked, window, pcx)
                                });
                            })),
                    ),
            );
        }

        let mut hidden_rows = v_flex().gap_1().text_size(content_font_size);
        if hidden.is_empty() {
            hidden_rows = hidden_rows.child(
                div()
                    .py_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr!(
                        "没有被隐藏的项目。可在项目详情中关闭显示开关来隐藏项目。",
                        "No hidden projects. Turn off the visibility switch in the project details to hide one."
                    )),
            );
        }
        let unhide_label = tr!("取消隐藏", "Unhide");
        for (idx, (key, path)) in hidden.into_iter().enumerate() {
            hidden_rows = hidden_rows.child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(cx.theme().radius)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(path),
                    )
                    .child(
                        Button::new(SharedString::from(format!("unhide-{idx}")))
                            .outline()
                            .small()
                            .flex_none()
                            .min_h(content_font_size * 1.5)
                            .icon(IconName::Eye)
                            .accessibility_label(unhide_label)
                            .child(div().text_size(content_font_size).child(unhide_label))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.with_parent(cx, |panel, pcx| {
                                    panel.unhide_project(&key, window, pcx)
                                });
                            })),
                    ),
            );
        }

        let content = match self.tab {
            0 => self.render_general(font_size, cx).into_any_element(),
            1 => v_flex()
                .w_full()
                .gap_2()
                .text_size(content_font_size)
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr!(
                            "关闭后不扫描，也不在左侧展示",
                            "Disabled apps are not scanned or shown in the sidebar"
                        )),
                )
                .child(agent_rows)
                .into_any_element(),
            _ => hidden_rows.into_any_element(),
        };

        v_flex()
            .w_full()
            .gap_3()
            .child(
                // 右侧预留关闭按钮的位置，避免与 Tab 栏重叠
                div().w_full().pr_10().child(
                    TabBar::new("settings-tabs")
                        .segmented()
                        .selected_index(self.tab)
                        .on_click(cx.listener(|this, idx: &usize, _, cx| {
                            this.tab = *idx;
                            cx.notify();
                        }))
                        .child(Tab::new().label(tr!("常规", "General")))
                        .child(Tab::new().label(tr!("软件启用", "Apps")))
                        .child(Tab::new().label(tr!("隐藏项目", "Hidden projects"))),
                ),
            )
            .child(
                div()
                    .h(px(360.0))
                    .overflow_y_scrollbar()
                    .child(div().w_full().min_w_0().pr_2().child(content)),
            )
            .into_any_element()
    }
}

impl Render for ProjectWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible_count = self.filtered_indices.len();

        let main_content =
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .min_h_0()
                .map(|el| match self.active_view {
                    ActiveView::Projects => el.child(self.render_projects_view(cx)),
                    ActiveView::Activity => el.child(self.render_activity_view(cx)),
                });

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.render_sidebar(cx))
                    .child(main_content),
            )
            .child(self.render_status_bar(visible_count, cx))
    }
}

pub(crate) fn sample_memory_usage_text() -> Option<SharedString> {
    current_process_memory_bytes().map(|bytes| SharedString::from(format_memory_bytes(bytes)))
}

pub(crate) fn format_relative_time(
    ts: Option<i64>,
    now: i64,
    fallback_date: &SharedString,
) -> SharedString {
    let Some(ts) = ts else {
        return SharedString::from(tr!("未知", "Unknown"));
    };
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        SharedString::from(tr!("刚刚", "Just now"))
    } else if diff < 3600 {
        SharedString::from(tr_fmt!("{} 分钟前", "{} min ago", diff / 60))
    } else if diff < 86_400 {
        SharedString::from(tr_fmt!("{} 小时前", "{} h ago", diff / 3600))
    } else if diff < 86_400 * 30 {
        SharedString::from(tr_fmt!("{} 天前", "{} d ago", diff / 86_400))
    } else {
        fallback_date.clone()
    }
}

fn format_exact_time(ts: Option<i64>) -> String {
    let Some(ts) = ts else {
        return tr!("未知", "Unknown").to_string();
    };
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| tr!("未知", "Unknown").to_string())
}
