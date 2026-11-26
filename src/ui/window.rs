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
use gpui_kit::component::separator::Separator;
use gpui_kit::component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarMenu, SidebarMenuItem, SidebarToggleButton,
};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme, ColorName, Icon, IconName, Side, Sizable, Theme, ThemeMode, TitleBar, WindowExt,
    h_flex, v_flex,
};
use gpui_kit::{
    Anchor, App, ClipboardItem, Context, Entity, FontWeight, MouseButton, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, div, img, prelude::*, px,
    uniform_list,
};

use crate::app_icon::apply_dock_icon;
use crate::config::AppConfig;
use crate::launcher::{
    DetectedLauncher, LauncherKind, detect_installed_launchers, open_project_with,
    partition_launchers_for_project, resolve_primary_launcher,
};
use crate::memory::{current_process_memory_bytes, format_memory_bytes};
use crate::model::{AgentKind, AggregatedProject, ScanReport};
use crate::scanner::ScannerRegistry;
use crate::scanner::paths::{dedup_key, open_in_file_manager};
use crate::ui::icons::{AgentIcon, agent_icon, app_icon_asset_path};
use crate::ui::theme::{agent_accent, agent_color_name, heat_level_color};

const CARD_HEIGHT: f32 = 104.0;
const MAX_CARD_BADGES: usize = 5;
const HEATMAP_WEEKS: usize = 24;
const HEATMAP_DAYS: usize = 7;
const MEMORY_POLL_INTERVAL_SECS: u64 = 2;

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
            .unwrap_or_else(|| SharedString::from("较早"));

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
    on_report_updated: Option<Arc<dyn Fn(Arc<ScanReport>, &mut gpui_kit::App) + Send + Sync>>,
}

impl ProjectWindow {
    pub fn new(
        initial_report: Arc<ScanReport>,
        on_report_updated: Option<Arc<dyn Fn(Arc<ScanReport>, &mut gpui_kit::App) + Send + Sync>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let app_config = AppConfig::load();
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("搜索项目名称、路径或 AI Agent...")
                .clean_on_escape()
        });
        let rename_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("输入自定义名称，留空恢复默认")
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
            launchers: Arc::new(Vec::new()),
            project_launcher_prefs: HashMap::new(),
            last_launcher: None,
            loading: false,
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
            on_report_updated,
        };
        view.apply_report(initial_report);
        view.trigger_refresh(cx);
        view
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
        cx.notify();

        let custom_names = self.app_config.custom_names.clone();
        let task = cx.background_executor().spawn(async move {
            let report = Arc::new(ScannerRegistry::new().scan_all_with_custom_names(&custom_names));
            let launchers = Arc::new(detect_installed_launchers());
            (report, launchers)
        });

        cx.spawn(async move |this, cx| {
            let (report, launchers) = task.await;
            let _ = this.update(cx, |panel, cx| {
                panel.launchers = launchers;
                panel.loading = false;
                panel.memory_usage_text = sample_memory_usage_text();
                panel.apply_report(report.clone());
                if let Some(cb) = &panel.on_report_updated {
                    cb(report, cx);
                }
                cx.notify();
            });
        })
        .detach();
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
                Notification::error(format!("保存自定义名称失败：{err}")),
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
        if let Some(cb) = &self.on_report_updated {
            cb(report_arc, cx);
        }

        let msg = match next_custom {
            Some(name) => format!("已设置自定义名称：{name}"),
            None => "已恢复默认项目名称".to_string(),
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
        .tooltip("查看其他软件自定义名称（点击复制）")
        .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _window, cx| {
            menu = menu.label("其他软件自定义名称（点击复制）");
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
                            Notification::success(format!(
                                "已复制名称（{}）：{}",
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
        match open_project_with(launcher, project_path) {
            Ok(()) => {
                window.push_notification(
                    Notification::success(format!(
                        "已使用 {} 打开：{}",
                        launcher.kind.display_name(),
                        project_name
                    )),
                    cx,
                );
            }
            Err(err) => {
                window.push_notification(
                    Notification::error(format!(
                        "使用 {} 打开失败：{err}",
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
            format!("用 {} 打开", primary.kind.short_label())
        } else {
            primary.kind.short_label().to_string()
        };

        let left_button = Button::new(SharedString::from(format!(
            "{id_prefix}-btn-{}-{stable_key}",
            primary.kind.id()
        )))
        .icon(launcher_icon(primary.kind, cx))
        .label(primary_label)
        .tooltip(format!("使用 {} 打开项目", primary.kind.display_name()))
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
        .menu_accessibility_label("选择打开方式")
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
                menu = menu.label("项目关联工具");
                for kind in matched_kinds {
                    menu = add_item(menu, kind, cx);
                }
                has_prev_section = true;
            }

            if !app_kinds.is_empty() {
                if has_prev_section {
                    menu = menu.separator();
                }
                menu = menu.label("编辑器与 AI 应用");
                for kind in app_kinds {
                    menu = add_item(menu, kind, cx);
                }
                has_prev_section = true;
            }

            if !terminal_kinds.is_empty() {
                if has_prev_section {
                    menu = menu.separator();
                }
                menu = menu.label("终端");
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
                .id(SharedString::from(format!(
                    "{id_prefix}-wrap-{stable_key}"
                )))
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
                                .accessibility_label("切换侧边栏")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.sidebar_collapsed = !this.sidebar_collapsed;
                                    cx.notify();
                                })),
                        )
                        .child(
                            img(app_icon_asset_path(is_dark))
                                .size(px(20.0))
                                .flex_shrink_0(),
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
                                .child("AI Agent 项目工作台"),
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
                                .tooltip("重新扫描所有 AI Agent (⌘/Ctrl + R)")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.trigger_refresh(cx);
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
                                    "切换到浅色主题"
                                } else {
                                    "切换到深色主题"
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

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let existing_count = self.metrics.existing_count;
        let view_menu = SidebarMenu::new()
            .child(
                SidebarMenuItem::new("项目列表")
                    .icon(IconName::FolderOpen)
                    .active(self.active_view == ActiveView::Projects && self.selected_agent.is_none())
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
                SidebarMenuItem::new("活跃度概览")
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
            .child(SidebarGroup::new("工作台").child(view_menu))
            .child(SidebarGroup::new("AI Agent 筛选").child(agent_menu))
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
                            .child(format!("显示已失效路径 ({missing_count})")),
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
                                    Tag::color(agent_color_name(agent))
                                        .rounded_full()
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_1()
                                                .child(Icon::new(AgentIcon(agent)).xsmall())
                                                .child(format!("筛选: {}", agent.display_name())),
                                        ),
                                )
                                .child(
                                    Button::new("btn-clear-agent-filter")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Close)
                                        .tooltip("清除 Agent 筛选")
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
                                .child(Tab::new().label("最近活跃"))
                                .child(Tab::new().label("名称"))
                                .child(Tab::new().label("Agent 数")),
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
                                    "正在扫描本机 AI Agent 项目..."
                                } else {
                                    "没有匹配的项目"
                                }))
                                .description(EmptyDescription::new().child(if self.loading {
                                    "请稍候，正在读取各工具的工作区与会话记录。"
                                } else if has_filter {
                                    "尝试调整搜索关键词或切换左侧 AI Agent 筛选条件。"
                                } else {
                                    "未在本机检测到已知 AI Agent 或编辑器的项目记录。"
                                })),
                        )
                        .when(has_filter && !self.loading, |empty| {
                            empty.content(
                                EmptyContent::new().child(
                                    Button::new("btn-reset-all-filters")
                                        .outline()
                                        .small()
                                        .label("重置筛选条件")
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
            .h(px(CARD_HEIGHT))
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
                            .child(
                                div()
                                    .size(px(8.0))
                                    .rounded_full()
                                    .flex_none()
                                    .bg(if proj.exists {
                                        cx.theme().green
                                    } else {
                                        cx.theme().muted_foreground
                                    }),
                            )
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
                                row.child(Tag::secondary().rounded_full().child("目录不存在"))
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
                                    .tooltip("自定义项目名称")
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
                                        .tooltip("在文件管理器中打开")
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            match open_in_file_manager(&path_for_open) {
                                                Ok(()) => {
                                                    window.push_notification(
                                                        Notification::success(format!(
                                                            "已打开目录：{}",
                                                            proj_name_for_open
                                                        )),
                                                        cx,
                                                    );
                                                }
                                                Err(err) => {
                                                    window.push_notification(
                                                        Notification::error(format!(
                                                            "打开目录失败：{err}"
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
                                    .tooltip("复制完整路径")
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            path_for_copy.to_string(),
                                        ));
                                        window.push_notification(
                                            Notification::success("已复制项目路径到剪贴板"),
                                            cx,
                                        );
                                    }),
                            ),
                    ),
            );

        div().w_full().pb_2().child(card)
    }

    fn render_project_detail(
        &self,
        proj_idx: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let proj = &self.report.projects[proj_idx];
        let cached = &self.cached_projects[proj_idx];
        let path_for_open = proj.path.clone();
        let path_for_rename = proj.path.clone();
        let path_for_reset = proj.path.clone();
        let path_for_copy = cached.full_path.clone();
        let proj_name = cached.name.clone();
        let is_editing = self.editing_project_path.as_ref() == Some(&proj.path);
        let has_custom_name = proj.custom_name.is_some();
        let show_default_name_hint = proj.name != proj.default_name;
        let default_name_hint = format!("目录原名：{}", proj.default_name);

        let mut usages = v_flex().w_full().min_w_0().gap_2().px_4().pb_3();
        for usage in &proj.agents {
            let session_text = match usage.session_count {
                Some(n) => format!("{n} 次会话"),
                None => "已关联工作区".to_string(),
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
                                .child(format!("自定义名称：{custom_alias}")),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child(format!("最近活跃：{}", format_exact_time(usage.last_active))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(format!("来源：{}", usage.source)),
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
                        .tooltip("保存自定义名称 (Enter)")
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
                            .tooltip("清除自定义名称，恢复默认")
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
                        .tooltip("取消编辑")
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
                                .tooltip("自定义项目名称")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.begin_rename(path_for_rename.clone(), window, cx);
                                })),
                        )
                        .child(
                            Button::new("btn-close-detail")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("关闭详情")
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
                                    row.child(Tag::secondary().rounded_full().child("已自定义"))
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
                                    .child("本地存在")
                            } else {
                                Tag::secondary().rounded_full().child("目录已移除")
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("最近：{}", format_exact_time(proj.last_active))),
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
                                    .child("完整路径"),
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
                                        .label("打开目录")
                                        .on_click(move |_, window, cx| {
                                            match open_in_file_manager(&path_for_open) {
                                                Ok(()) => {
                                                    window.push_notification(
                                                        Notification::success(format!(
                                                            "已打开目录：{proj_name}"
                                                        )),
                                                        cx,
                                                    );
                                                }
                                                Err(err) => {
                                                    window.push_notification(
                                                        Notification::error(format!(
                                                            "打开失败：{err}"
                                                        )),
                                                        cx,
                                                    );
                                                }
                                            }
                                        }),
                                )
                            })
                            .child(
                                Button::new("detail-copy-path")
                                    .outline()
                                    .small()
                                    .icon(IconName::Copy)
                                    .label("复制路径")
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            path_for_copy.to_string(),
                                        ));
                                        window.push_notification(
                                            Notification::success("已复制完整路径"),
                                            cx,
                                        );
                                    }),
                            ),
                    )
                    .child(Separator::horizontal())
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("关联 AI Agent ({})", proj.agents.len())),
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
                    .child("暂未检测到活跃的 AI Agent 项目记录。"),
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
                                        .child(format!("{count} 项目")),
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
                    .child(format!("扫描失败: {err}"))
            } else if summary.project_count > 0 {
                Tag::color(ColorName::Green)
                    .rounded_full()
                    .child(format!("{} 个项目", summary.project_count))
            } else {
                Tag::secondary().rounded_full().child("未检测到数据")
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
                            .title("Agent 项目分布（点击条目筛选对应项目）")
                            .child(agent_rows),
                    )
                    .child(self.render_heatmap_group(cx))
                    .child(
                        GroupBox::new()
                            .outline()
                            .title("扫描器诊断状态")
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
            month_headers = month_headers.child(format!("{}月", d.month()));
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
            .child("少");
        for lvl in 0..=4u8 {
            legend = legend.child(
                div()
                    .size(px(10.0))
                    .rounded(px(2.0))
                    .bg(heat_level_color(lvl, cx)),
            );
        }
        legend = legend.child("多");

        GroupBox::new()
            .outline()
            .title(format!("近 24 周项目活跃度（共 {active_days} 个活跃日）"))
            .child(
                v_flex()
                    .gap_2()
                    .child(month_headers)
                    .child(grid)
                    .child(
                        h_flex()
                            .pt_1()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "汇总自 {} 个项目的 Agent 活跃时间戳 · 耗时 {} ms",
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
            (true, Some(mem)) => format!("内存 {mem} · 正在扫描本机 AI Agent 项目..."),
            (true, None) => "正在扫描本机 AI Agent 项目...".to_string(),
            (false, Some(mem)) => format!("内存 {mem} · 扫描耗时 {} ms", self.report.elapsed_ms),
            (false, None) => format!("扫描耗时 {} ms", self.report.elapsed_ms),
        };

        let right_text = format!(
            "当前显示 {} / {} 个项目 · ⌘/Ctrl+R 刷新",
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
                    .child(
                        div()
                            .size(px(7.0))
                            .rounded_full()
                            .bg(if self.loading {
                                cx.theme().yellow
                            } else {
                                cx.theme().green
                            }),
                    )
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

impl Render for ProjectWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible_count = self.filtered_indices.len();

        let main_content = v_flex()
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

fn sample_memory_usage_text() -> Option<SharedString> {
    current_process_memory_bytes().map(|bytes| SharedString::from(format_memory_bytes(bytes)))
}

fn launcher_icon(kind: LauncherKind, cx: &App) -> Icon {
    match kind.linked_agent() {
        Some(agent) => agent_icon(agent, cx),
        None => Icon::new(IconName::SquareTerminal),
    }
}

fn format_relative_time(
    ts: Option<i64>,
    now: i64,
    fallback_date: &SharedString,
) -> SharedString {
    let Some(ts) = ts else {
        return SharedString::from("未知");
    };
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        SharedString::from("刚刚")
    } else if diff < 3600 {
        SharedString::from(format!("{} 分钟前", diff / 60))
    } else if diff < 86_400 {
        SharedString::from(format!("{} 小时前", diff / 3600))
    } else if diff < 86_400 * 30 {
        SharedString::from(format!("{} 天前", diff / 86_400))
    } else {
        fallback_date.clone()
    }
}

fn format_exact_time(ts: Option<i64>) -> String {
    let Some(ts) = ts else {
        return "未知".to_string();
    };
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "未知".to_string())
}
