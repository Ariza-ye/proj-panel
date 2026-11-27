use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use gpui_kit::assets::IconName as LucideIconName;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Side, Sizable, Theme, WindowExt, h_flex, v_flex,
};
use gpui_kit::{
    Anchor, App, ClipboardItem, Context, Entity, FontWeight, KeyDownEvent, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*, px, rems,
    uniform_list,
};

use crate::app_icon::apply_dock_icon;
use crate::launcher::{
    DetectedLauncher, LauncherKind, partition_launchers_for_project, resolve_primary_launcher,
};
use crate::model::{AggregatedProject, ScanReport};
use crate::scanner::paths::dedup_key;
use crate::ui::icons::{agent_icon, launcher_icon, render_app_brand_icon};
use crate::ui::window::{format_relative_time, sample_memory_usage_text};

pub const POPOVER_WIDTH: f32 = 408.0;
pub const POPOVER_HEIGHT: f32 = 520.0;

const MAX_ROW_AGENT_ICONS: usize = 4;
const BLUR_ARM_GRACE: Duration = Duration::from_millis(120);

pub type LaunchProjectCallback = Arc<dyn Fn(PathBuf, LauncherKind, &mut App) + Send + Sync>;
pub type ProjectPathCallback = Arc<dyn Fn(PathBuf, &mut App) + Send + Sync>;
pub type SimpleAppCallback = Arc<dyn Fn(&mut App) + Send + Sync>;

/// Application-level callbacks wired into the status bar GPUI popover.
#[derive(Clone)]
pub struct TrayPopoverCallbacks {
    pub on_launch_project: LaunchProjectCallback,
    pub on_open_in_file_manager: ProjectPathCallback,
    pub on_copy_path: ProjectPathCallback,
    pub on_reveal_in_main_window: ProjectPathCallback,
    pub on_open_main_window: SimpleAppCallback,
    pub on_refresh: SimpleAppCallback,
    pub on_quit: SimpleAppCallback,
    pub on_blur_dismiss: SimpleAppCallback,
}

struct PopoverProjectCache {
    name: SharedString,
    name_lower: String,
    search_names_lower: String,
    display_path: SharedString,
    display_path_lower: String,
    stable_key: SharedString,
    agent_search_lower: String,
    fallback_date: SharedString,
}

impl PopoverProjectCache {
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
            stable_key: SharedString::from(dedup_key(&proj.path)),
            agent_search_lower,
            fallback_date,
        }
    }
}

/// Compact GPUI quick-action popover anchored to the system status bar icon.
/// Automatically destroys its window on focus loss so GPU/UI memory is reclaimed immediately.
pub struct TrayPopover {
    report: Arc<ScanReport>,
    launchers: Arc<Vec<DetectedLauncher>>,
    project_launcher_prefs: HashMap<PathBuf, LauncherKind>,
    last_launcher: Option<LauncherKind>,
    loading: bool,
    existing_count: usize,
    cached_projects: Vec<PopoverProjectCache>,
    filtered_indices: Vec<usize>,
    list_scroll_handle: UniformListScrollHandle,
    search_input: Entity<InputState>,
    query: String,
    memory_usage_text: Option<SharedString>,
    was_activated: bool,
    opened_at: Instant,
    dismissed: bool,
    callbacks: TrayPopoverCallbacks,
    _subscriptions: Vec<Subscription>,
}

impl TrayPopover {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        report: Arc<ScanReport>,
        launchers: Arc<Vec<DetectedLauncher>>,
        project_launcher_prefs: HashMap<PathBuf, LauncherKind>,
        last_launcher: Option<LauncherKind>,
        loading: bool,
        callbacks: TrayPopoverCallbacks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Theme::sync_system_appearance(Some(window), cx);

        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!(
                    "搜索项目、路径或 Agent... (Enter 快速打开)",
                    "Search projects, paths or Agents... (Enter to open)"
                ))
                .clean_on_escape()
        });
        search_input.update(cx, |input, cx| {
            input.focus(window, cx);
        });

        let search_sub = cx.subscribe_in(
            &search_input,
            window,
            |this, state, event, window, cx| match event {
                InputEvent::Change => {
                    let next_query = state.read(cx).value().to_string();
                    if next_query != this.query {
                        this.query = next_query;
                        this.rebuild_filtered(true);
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => {
                    this.launch_first_visible(window, cx);
                }
                _ => {}
            },
        );

        let activation_sub = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.was_activated = true;
            } else if !this.dismissed
                && (this.was_activated || this.opened_at.elapsed() >= BLUR_ARM_GRACE)
            {
                this.dismissed = true;
                let on_blur = this.callbacks.on_blur_dismiss.clone();
                on_blur(cx);
                window.remove_window();
            }
        });

        let appearance_sub = window.observe_window_appearance(|window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
            apply_dock_icon(cx.theme().is_dark());
        });

        let was_activated = window.is_window_active();

        let mut view = Self {
            report: Arc::new(ScanReport::default()),
            launchers,
            project_launcher_prefs,
            last_launcher,
            loading,
            existing_count: 0,
            cached_projects: Vec::new(),
            filtered_indices: Vec::new(),
            list_scroll_handle: UniformListScrollHandle::new(),
            search_input,
            query: String::new(),
            memory_usage_text: sample_memory_usage_text(),
            was_activated,
            opened_at: Instant::now(),
            dismissed: false,
            callbacks,
            _subscriptions: vec![search_sub, activation_sub, appearance_sub],
        };
        view.apply_report(report);
        view
    }

    /// Updates the live popover when background scanning or launcher preferences change.
    pub fn sync_state(
        &mut self,
        report: Arc<ScanReport>,
        launchers: Arc<Vec<DetectedLauncher>>,
        project_launcher_prefs: HashMap<PathBuf, LauncherKind>,
        last_launcher: Option<LauncherKind>,
        loading: bool,
        cx: &mut Context<Self>,
    ) {
        self.launchers = launchers;
        self.project_launcher_prefs = project_launcher_prefs;
        self.last_launcher = last_launcher;
        self.loading = loading;
        self.memory_usage_text = sample_memory_usage_text();
        self.apply_report(report);
        cx.notify();
    }

    fn apply_report(&mut self, report: Arc<ScanReport>) {
        self.existing_count = report.projects.iter().filter(|p| p.exists).count();
        self.cached_projects = report
            .projects
            .iter()
            .map(PopoverProjectCache::from_project)
            .collect();
        self.report = report;
        self.rebuild_filtered(false);
    }

    fn rebuild_filtered(&mut self, reset_scroll: bool) {
        let q = self.query.trim().to_lowercase();
        let projects = &self.report.projects;
        let cached = &self.cached_projects;

        let mut indices: Vec<usize> = (0..projects.len())
            .filter(|&i| {
                let proj = &projects[i];
                if !proj.exists {
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

        indices.sort_by(|&a, &b| {
            projects[b]
                .last_active
                .cmp(&projects[a].last_active)
                .then_with(|| cached[a].name_lower.cmp(&cached[b].name_lower))
        });

        self.filtered_indices = indices;
        if reset_scroll && !self.filtered_indices.is_empty() {
            self.list_scroll_handle
                .scroll_to_item_strict(0, ScrollStrategy::Top);
        }
    }

    fn dismiss(&mut self, window: &mut Window) {
        if !self.dismissed {
            self.dismissed = true;
            window.remove_window();
        }
    }

    fn launch_project_and_dismiss(
        &mut self,
        path: PathBuf,
        launcher: LauncherKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cb = self.callbacks.on_launch_project.clone();
        self.dismiss(window);
        cb(path, launcher, cx);
    }

    fn open_primary_or_dir_and_dismiss(
        &mut self,
        proj_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(proj) = self.report.projects.get(proj_idx) else {
            return;
        };
        let path = proj.path.clone();
        let primary = resolve_primary_launcher(
            proj,
            &self.launchers,
            &self.project_launcher_prefs,
            self.last_launcher,
        )
        .map(|l| l.kind);

        match primary {
            Some(kind) => self.launch_project_and_dismiss(path, kind, window, cx),
            None => {
                let cb = self.callbacks.on_open_in_file_manager.clone();
                self.dismiss(window);
                cb(path, cx);
            }
        }
    }

    fn launch_first_visible(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&proj_idx) = self.filtered_indices.first() {
            self.open_primary_or_dir_and_dismiss(proj_idx, window, cx);
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_dark = cx.theme().is_dark();
        let count_badge = if self.loading && self.existing_count == 0 {
            tr!("扫描中...", "Scanning...").to_string()
        } else {
            tr_fmt!("{} 个项目", "{} projects", self.existing_count)
        };

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(render_app_brand_icon(is_dark))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child("ProjPanel"),
                    )
                    .child(Tag::secondary().rounded_full().child(count_badge)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("popover-refresh")
                            .ghost()
                            .xsmall()
                            .icon(IconName::RefreshCw)
                            .loading(self.loading)
                            .tooltip(tr!("立即重新扫描", "Rescan now"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let cb = this.callbacks.on_refresh.clone();
                                cb(cx);
                            })),
                    )
                    .child(
                        Button::new("popover-open-main")
                            .outline()
                            .xsmall()
                            .icon(LucideIconName::ExternalLink)
                            .label(tr!("主窗口", "Main window"))
                            .tooltip(tr!("打开完整桌面主窗口", "Open the full desktop window"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let cb = this.callbacks.on_open_main_window.clone();
                                this.dismiss(window);
                                cb(cx);
                            })),
                    ),
            )
    }

    fn render_search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Input::new(&self.search_input)
                    .small()
                    .cleanable(true)
                    .prefix(
                        Icon::new(IconName::Search)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
    }

    fn render_external_names_dropdown(
        &self,
        proj: &AggregatedProject,
        cached: &PopoverProjectCache,
    ) -> Option<impl IntoElement> {
        if !proj.should_show_name_dropdown() {
            return None;
        }

        let stable_key = &cached.stable_key;
        let external_names = proj.external_names.clone();
        let has_custom = proj.custom_name.is_some();

        let trigger = Button::new(SharedString::from(format!("pop-names-btn-{stable_key}")))
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
                    .max_h(px(260.0))
                    .scrollable(true)
            });

        Some(div().flex_none().child(trigger))
    }

    fn render_open_in_button(
        &self,
        proj: &AggregatedProject,
        cached: &PopoverProjectCache,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let primary = resolve_primary_launcher(
            proj,
            &self.launchers,
            &self.project_launcher_prefs,
            self.last_launcher,
        )?
        .clone();

        let stable_key = &cached.stable_key;
        let primary_path = proj.path.clone();
        let primary_kind = primary.kind;

        let left_button = Button::new(SharedString::from(format!(
            "pop-open-btn-{}-{stable_key}",
            primary_kind.id()
        )))
        .icon(launcher_icon(primary_kind, cx))
        .label(primary_kind.short_label())
        .tooltip(tr_fmt!(
            "使用 {} 打开项目",
            "Open project with {}",
            primary_kind.display_name()
        ))
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.launch_project_and_dismiss(primary_path.clone(), primary_kind, window, cx);
        }));

        let menu_project_path = proj.path.clone();
        let view = cx.entity().downgrade();

        let split = DropdownButton::new(SharedString::from(format!("pop-open-split-{stable_key}")))
            .button(left_button)
            .menu_accessibility_label(tr!("选择打开方式", "Choose how to open"))
            .xsmall()
            .outline()
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _window, cx| {
                let Some(view_entity) = view.upgrade() else {
                    return menu;
                };
                let (matched_kinds, app_kinds, terminal_kinds) = {
                    let popover = view_entity.read(cx);
                    let Some(proj_ref) = popover
                        .report
                        .projects
                        .iter()
                        .find(|p| p.path == menu_project_path)
                    else {
                        return menu;
                    };
                    let (matched, apps, terminals) =
                        partition_launchers_for_project(proj_ref, &popover.launchers);
                    (
                        matched.into_iter().map(|l| l.kind).collect::<Vec<_>>(),
                        apps.into_iter().map(|l| l.kind).collect::<Vec<_>>(),
                        terminals.into_iter().map(|l| l.kind).collect::<Vec<_>>(),
                    )
                };

                let add_item = |menu: PopupMenu, kind: LauncherKind, cx: &App| {
                    let path = menu_project_path.clone();
                    let view = view.clone();
                    let is_current = kind == primary_kind;
                    let item = PopupMenuItem::new(kind.display_name())
                        .icon(launcher_icon(kind, cx))
                        .checked(is_current)
                        .on_click(move |_, window, cx| {
                            if let Some(view) = view.upgrade() {
                                view.update(cx, |this, cx| {
                                    this.launch_project_and_dismiss(path.clone(), kind, window, cx);
                                });
                            }
                        });
                    menu.item(item)
                };

                let mut has_prev = false;
                if !matched_kinds.is_empty() {
                    menu = menu.label(tr!("项目关联工具", "Project-linked tools"));
                    for kind in matched_kinds {
                        menu = add_item(menu, kind, cx);
                    }
                    has_prev = true;
                }
                if !app_kinds.is_empty() {
                    if has_prev {
                        menu = menu.separator();
                    }
                    menu = menu.label(tr!("编辑器与 AI 应用", "Editors & AI apps"));
                    for kind in app_kinds {
                        menu = add_item(menu, kind, cx);
                    }
                    has_prev = true;
                }
                if !terminal_kinds.is_empty() {
                    if has_prev {
                        menu = menu.separator();
                    }
                    menu = menu.label(tr!("终端", "Terminal"));
                    for kind in terminal_kinds {
                        menu = add_item(menu, kind, cx);
                    }
                }

                menu = menu.separator();

                let path_dir = menu_project_path.clone();
                let view_dir = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(file_manager_label())
                        .icon(IconName::FolderOpen)
                        .on_click(move |_, window, cx| {
                            if let Some(view) = view_dir.upgrade() {
                                view.update(cx, |this, cx| {
                                    let cb = this.callbacks.on_open_in_file_manager.clone();
                                    this.dismiss(window);
                                    cb(path_dir.clone(), cx);
                                });
                            }
                        }),
                );

                let path_copy = menu_project_path.clone();
                let view_copy = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(tr!("复制路径", "Copy path"))
                        .icon(IconName::Copy)
                        .on_click(move |_, window, cx| {
                            if let Some(view) = view_copy.upgrade() {
                                view.update(cx, |this, cx| {
                                    let cb = this.callbacks.on_copy_path.clone();
                                    this.dismiss(window);
                                    cb(path_copy.clone(), cx);
                                });
                            }
                        }),
                );

                let path_reveal = menu_project_path.clone();
                let view_reveal = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(tr!("在主窗口中查看", "Reveal in main window"))
                        .icon(LucideIconName::ExternalLink)
                        .on_click(move |_, window, cx| {
                            if let Some(view) = view_reveal.upgrade() {
                                view.update(cx, |this, cx| {
                                    let cb = this.callbacks.on_reveal_in_main_window.clone();
                                    this.dismiss(window);
                                    cb(path_reveal.clone(), cx);
                                });
                            }
                        }),
                );

                menu.check_side(Side::Right)
                    .min_w(px(172.0))
                    .max_h(px(280.0))
                    .scrollable(true)
            });

        Some(div().flex_none().child(split))
    }

    fn render_project_row(
        &self,
        proj_idx: usize,
        now_ts: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let proj = &self.report.projects[proj_idx];
        let cached = &self.cached_projects[proj_idx];
        let time_text = format_relative_time(proj.last_active, now_ts, &cached.fallback_date);
        let path_for_dir = proj.path.clone();

        let mut agent_icons = h_flex().flex_none().items_center().gap_1();
        for usage in proj.agents.iter().take(MAX_ROW_AGENT_ICONS) {
            agent_icons = agent_icons.child(agent_icon(usage.agent, cx).xsmall());
        }
        if proj.agents.len() > MAX_ROW_AGENT_ICONS {
            let extra = proj.agents.len() - MAX_ROW_AGENT_ICONS;
            agent_icons = agent_icons.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("+{extra}")),
            );
        }

        let card = v_flex()
            .id(SharedString::from(format!("popover-proj-{proj_idx}")))
            .w_full()
            .h(rems(4.0))
            .min_w_0()
            .overflow_hidden()
            .justify_between()
            .px_2p5()
            .py_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .hover(|s| s.bg(cx.theme().list_hover))
            .child(
                h_flex()
                    .w_full()
                    .h(rems(1.5))
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .size(px(6.0))
                                    .rounded_full()
                                    .flex_none()
                                    .bg(cx.theme().green),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().foreground)
                                    .truncate()
                                    .child(cached.name.clone()),
                            )
                            .when_some(
                                self.render_external_names_dropdown(proj, cached),
                                |row, dropdown| row.child(dropdown),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!("pop-dir-{proj_idx}")))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::FolderOpen)
                                    .tooltip(file_manager_label())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let cb = this.callbacks.on_open_in_file_manager.clone();
                                        this.dismiss(window);
                                        cb(path_for_dir.clone(), cx);
                                    })),
                            )
                            .when_some(self.render_open_in_button(proj, cached, cx), |row, btn| {
                                row.child(btn)
                            }),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .h(rems(1.125))
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(cached.display_path.clone()),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap_1p5()
                            .child(agent_icons)
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .whitespace_nowrap()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(time_text),
                            ),
                    ),
            );

        div().w_full().pb_1p5().child(card)
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.filtered_indices.is_empty() {
            let has_filter = !self.query.trim().is_empty();
            return div()
                .flex_1()
                .min_h_0()
                .w_full()
                .px_3()
                .py_4()
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
                                    tr!("正在扫描项目...", "Scanning projects...")
                                } else {
                                    tr!("没有匹配的项目", "No matching projects")
                                }))
                                .description(EmptyDescription::new().child(if self.loading {
                                    tr!(
                                        "正在读取各 AI Agent 与编辑器的工作区记录。",
                                        "Reading workspace records of AI Agents and editors."
                                    )
                                } else if has_filter {
                                    tr!(
                                        "尝试调整搜索关键词。",
                                        "Try adjusting the search keywords."
                                    )
                                } else {
                                    tr!(
                                        "未在本机检测到可用的项目目录。",
                                        "No usable project directories found on this machine."
                                    )
                                })),
                        )
                        .when(has_filter && !self.loading, |empty| {
                            empty.content(
                                EmptyContent::new().child(
                                    Button::new("popover-reset-filters")
                                        .outline()
                                        .xsmall()
                                        .label(tr!("清空搜索", "Clear search"))
                                        .on_click(cx.listener(|this, _, window, cx| {
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
                .into_any_element();
        }

        let item_count = self.filtered_indices.len();
        let now_ts = Utc::now().timestamp();

        // Reserve 14px on the right so the overlay scrollbar does not collide with row buttons.
        let list = uniform_list(
            "popover-projects-list",
            item_count,
            cx.processor(move |this, visible_range: Range<usize>, _window, cx| {
                let mut items = Vec::with_capacity(visible_range.len());
                for filtered_pos in visible_range {
                    if let Some(&proj_idx) = this.filtered_indices.get(filtered_pos) {
                        items.push(this.render_project_row(proj_idx, now_ts, cx));
                    }
                }
                items
            }),
        )
        .size_full()
        .pl_2p5()
        .pr_3p5()
        .pt_2()
        .pb_1()
        .track_scroll(&self.list_scroll_handle);

        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(list)
            .vertical_scrollbar(&self.list_scroll_handle)
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let left_text = match &self.memory_usage_text {
            Some(mem) => tr_fmt!(
                "显示 {} / {} · 内存 {mem}",
                "Showing {} / {} · Memory {mem}",
                self.filtered_indices.len(),
                self.existing_count
            ),
            None => tr_fmt!(
                "显示 {} / {} 个项目",
                "Showing {} / {} projects",
                self.filtered_indices.len(),
                self.existing_count
            ),
        };

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .px_3()
            .py_1p5()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.35))
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().size(px(6.0)).rounded_full().bg(if self.loading {
                        cx.theme().yellow
                    } else {
                        cx.theme().green
                    }))
                    .child(left_text),
            )
            .child(
                Button::new("popover-quit")
                    .ghost()
                    .xsmall()
                    .icon(LucideIconName::LogOut)
                    .label(tr!("退出", "Quit"))
                    .tooltip(tr!("退出 ProjPanel", "Quit ProjPanel"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let cb = this.callbacks.on_quit.clone();
                        this.dismiss(window);
                        cb(cx);
                    })),
            )
    }
}

impl Render for TrayPopover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .border_1()
            .border_color(cx.theme().border)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _cx| {
                if event.keystroke.key.as_str() == "escape" && this.query.is_empty() {
                    this.dismiss(window);
                }
            }))
            .child(self.render_header(cx))
            .child(self.render_search_bar(cx))
            .child(self.render_list(cx))
            .child(self.render_footer(cx))
    }
}

fn file_manager_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        tr!("在访达中打开", "Reveal in Finder")
    }
    #[cfg(target_os = "windows")]
    {
        tr!("在资源管理器中打开", "Open in File Explorer")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        tr!("在文件管理器中打开", "Open in file manager")
    }
}
