#[macro_use]
mod i18n;
mod app_icon;
mod config;
mod launcher;
mod memory;
mod model;
mod scanner;
mod tray;
mod ui;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::component::{ActiveTheme, TitleBar};
use gpui_kit::{
    AnyWindowHandle, App, Bounds, ClipboardItem, DisplayId, Entity, Global, KeyBinding, Menu,
    MenuItem, Pixels, WindowBounds, WindowKind, WindowOptions, actions, prelude::*, px, size,
};

use crate::launcher::{
    DetectedLauncher, LauncherKind, detect_installed_launchers, open_project_with,
};
use crate::model::ScanReport;
use crate::scanner::ScannerRegistry;
use crate::scanner::paths::open_in_file_manager;
use crate::tray::{TrayAnchor, TrayCommand, TrayHandle, compute_popover_bounds};
use crate::ui::icons::AppAssets;
use crate::ui::theme::apply_ui_typography;
use crate::ui::tray_popover::{POPOVER_HEIGHT, POPOVER_WIDTH, TrayPopover, TrayPopoverCallbacks};
use crate::ui::window::{ProjectWindow, ProjectWindowCallbacks};

const APP_NAME: &str = "ProjPanel";
const APP_ID: &str = "com.projpanel.app";
const DEFAULT_WINDOW_WIDTH: f32 = 1100.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 720.0;
const MIN_WINDOW_WIDTH: f32 = 780.0;
const MIN_WINDOW_HEIGHT: f32 = 520.0;
const POPOVER_BLUR_REOPEN_GUARD: Duration = Duration::from_millis(220);

actions!(proj_panel, [Quit, RefreshProjects]);

struct TrayGlobal {
    _handle: TrayHandle,
}

impl Global for TrayGlobal {}

struct AppState {
    main_window: Option<(AnyWindowHandle, Entity<ProjectWindow>)>,
    tray_popover: Option<(AnyWindowHandle, Entity<TrayPopover>)>,
    last_popover_dismissed_at: Option<Instant>,
    latest_report: Arc<ScanReport>,
    latest_launchers: Arc<Vec<DetectedLauncher>>,
    project_launcher_prefs: HashMap<PathBuf, LauncherKind>,
    last_launcher: Option<LauncherKind>,
    has_tray: bool,
    is_scanning: bool,
}

impl Global for AppState {}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--dump" || a == "--json") {
        run_cli_dump(args.iter().any(|a| a == "--json"));
        return;
    }
    let start_in_background = args.iter().any(|a| a == "--background");

    let app = gpui_kit::application().with_assets(AppAssets);
    // Handle macOS dock icon clicks / app reopen when the window was closed.
    app.on_reopen(|cx| {
        open_or_focus_main_window(cx);
    });

    app.run(move |cx: &mut App| {
        gpui_kit::init(cx);
        cx.set_app_identity(APP_ID, APP_NAME);
        app_icon::apply_dock_icon(cx.theme().is_dark());

        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-r", RefreshProjects, None),
            KeyBinding::new("ctrl-r", RefreshProjects, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &RefreshProjects, cx| refresh_projects(cx));
        let app_config = config::AppConfig::load();
        i18n::set_current(app_config.language);
        apply_ui_typography(&app_config, cx);
        apply_app_menus(cx);

        cx.set_global(AppState {
            main_window: None,
            tray_popover: None,
            last_popover_dismissed_at: None,
            latest_report: Arc::new(ScanReport::default()),
            latest_launchers: Arc::new(Vec::new()),
            project_launcher_prefs: HashMap::new(),
            last_launcher: None,
            has_tray: false,
            is_scanning: false,
        });

        cx.on_window_closed(|cx, closed_id| {
            if !cx.has_global::<AppState>() {
                return;
            }
            let (was_popover, was_main, has_tray) = {
                let state = cx.global_mut::<AppState>();
                let was_popover = state
                    .tray_popover
                    .as_ref()
                    .is_some_and(|(h, _)| h.window_id() == closed_id);
                if was_popover {
                    state.tray_popover = None;
                }
                let was_main = state
                    .main_window
                    .as_ref()
                    .is_some_and(|(h, _)| h.window_id() == closed_id);
                if was_main {
                    state.main_window = None;
                    state.is_scanning = false;
                }
                (was_popover, was_main, state.has_tray)
            };

            let remaining = cx.windows().len();
            if remaining == 0 && !has_tray {
                cx.quit();
                return;
            }
            if was_popover || (was_main && remaining == 0) {
                schedule_memory_trim(cx);
            }
        })
        .detach();

        match TrayHandle::init() {
            Ok((tray_handle, mut tray_rx)) => {
                cx.global_mut::<AppState>().has_tray = true;
                cx.set_global(TrayGlobal {
                    _handle: tray_handle,
                });

                cx.spawn(async move |cx| {
                    while let Some(cmd) = tray_rx.next().await {
                        cx.update(|cx| handle_tray_command(cmd, cx));
                    }
                })
                .detach();

                if start_in_background {
                    trigger_background_cache_refresh(cx);
                } else {
                    open_or_focus_main_window(cx);
                }
            }
            Err(err) => {
                eprintln!(
                    "warning: system tray unavailable ({err:#}); opening main window directly"
                );
                open_or_focus_main_window(cx);
            }
        }
    });
}

/// Installs the application menu using the current interface language.
fn apply_app_menus(cx: &mut App) {
    cx.set_menus([Menu::new(APP_NAME).items([
        MenuItem::action(tr!("重新扫描项目", "Rescan projects"), RefreshProjects),
        MenuItem::separator(),
        MenuItem::action(tr_fmt!("退出 {APP_NAME}", "Quit {APP_NAME}"), Quit),
    ])]);
}

/// Refreshes shell-level UI (application menu and tray menu) after a language change.
fn apply_language_to_shell(cx: &mut App) {
    apply_app_menus(cx);
    if let Some(tray) = cx.try_global::<TrayGlobal>() {
        tray._handle.apply_language();
    }
}

fn handle_tray_command(cmd: TrayCommand, cx: &mut App) {
    match cmd {
        TrayCommand::TogglePopover { rect, anchor } => toggle_tray_popover(rect, anchor, cx),
        TrayCommand::OpenWindow => {
            open_or_focus_main_window(cx);
        }
        TrayCommand::RefreshProjects => refresh_projects(cx),
        TrayCommand::Quit => cx.quit(),
    }
}

fn toggle_tray_popover(
    click_rect: tray_icon::Rect,
    captured_anchor: Option<TrayAnchor>,
    cx: &mut App,
) {
    if close_tray_popover(cx) {
        return;
    }

    if let Some(dismissed_at) = cx.global::<AppState>().last_popover_dismissed_at
        && dismissed_at.elapsed() < POPOVER_BLUR_REOPEN_GUARD
    {
        return;
    }

    let placement = resolve_popover_placement(click_rect, captured_anchor, cx);
    let bounds = placement.bounds;
    let display_id = placement.display_id;
    let macos_screen_frame = placement.macos_screen_frame;
    let (report, launchers, prefs, last_launcher, loading, needs_initial_scan) = {
        let state = cx.global::<AppState>();
        (
            state.latest_report.clone(),
            state.latest_launchers.clone(),
            state.project_launcher_prefs.clone(),
            state.last_launcher,
            state.is_scanning,
            state.latest_report.scanned_at == 0 && !state.is_scanning,
        )
    };

    let callbacks = TrayPopoverCallbacks {
        on_launch_project: Arc::new(|path, launcher, cx| {
            launch_project_from_tray(path, launcher, cx);
        }),
        on_open_in_file_manager: Arc::new(|path, _cx| {
            if let Err(err) = open_in_file_manager(&path) {
                eprintln!(
                    "error: failed to open {} in file manager: {err}",
                    path.display()
                );
            }
        }),
        on_copy_path: Arc::new(|path, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                path.to_string_lossy().into_owned(),
            ));
        }),
        on_reveal_in_main_window: Arc::new(|path, cx| {
            if let Some((handle, view)) = open_or_focus_main_window(cx) {
                let _ = handle.update(cx, |_, window, cx| {
                    view.update(cx, |panel, cx| {
                        panel.focus_project(path, window, cx);
                    });
                });
            }
        }),
        on_open_main_window: Arc::new(|cx| {
            open_or_focus_main_window(cx);
        }),
        on_refresh: Arc::new(|cx| {
            refresh_projects(cx);
        }),
        on_quit: Arc::new(|cx| {
            cx.quit();
        }),
        on_blur_dismiss: Arc::new(|cx| {
            if cx.has_global::<AppState>() {
                cx.global_mut::<AppState>().last_popover_dismissed_at = Some(Instant::now());
            }
        }),
    };

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        app_id: Some(APP_ID.to_string()),
        kind: WindowKind::PopUp,
        display_id,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        focus: true,
        show: true,
        ..Default::default()
    };

    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            TrayPopover::new(
                report,
                launchers,
                prefs,
                last_launcher,
                loading || needs_initial_scan,
                callbacks,
                window,
                cx,
            )
        })
    }) {
        Ok((handle, view)) => {
            {
                let state = cx.global_mut::<AppState>();
                state.tray_popover = Some((handle, view));
                state.last_popover_dismissed_at = None;
            }
            cx.activate(true);
            let _ = handle.update(cx, |_, window, _| {
                #[cfg(target_os = "macos")]
                if let Some(screen_frame) = macos_screen_frame {
                    crate::tray::apply_macos_window_frame(window, bounds, screen_frame);
                }
                #[cfg(not(target_os = "macos"))]
                let _ = macos_screen_frame;
                window.activate_window();
            });
            if needs_initial_scan {
                trigger_background_cache_refresh(cx);
            }
        }
        Err(err) => {
            eprintln!("error: failed to open tray popover: {err:#}");
        }
    }
}

fn close_tray_popover(cx: &mut App) -> bool {
    let existing = cx.global_mut::<AppState>().tray_popover.take();
    if let Some((handle, _)) = existing {
        let _ = handle.update(cx, |_, window, _| {
            window.remove_window();
        });
        schedule_memory_trim(cx);
        true
    } else {
        false
    }
}

struct PopoverPlacement {
    bounds: Bounds<Pixels>,
    display_id: Option<DisplayId>,
    macos_screen_frame: Option<(f64, f64, f64)>,
}

fn resolve_popover_placement(
    click_rect: tray_icon::Rect,
    captured_anchor: Option<TrayAnchor>,
    cx: &App,
) -> PopoverPlacement {
    if let Some(anchor) = captured_anchor {
        let display_id = anchor
            .display_id
            .filter(|id| cx.displays().iter().any(|d| d.id() == *id));
        let bounds = compute_popover_bounds(anchor, POPOVER_WIDTH, POPOVER_HEIGHT);
        #[cfg(target_os = "macos")]
        let macos_screen_frame = anchor.macos_screen_frame;
        #[cfg(not(target_os = "macos"))]
        let macos_screen_frame = None;
        return PopoverPlacement {
            bounds,
            display_id,
            macos_screen_frame,
        };
    }

    // Cross-platform fallback using primary display visible bounds and tray click rect.
    let display = cx.primary_display();
    let display_id = display.as_ref().map(|d| d.id());
    let visible = display.map(|d| d.visible_bounds()).unwrap_or_else(|| {
        Bounds::new(
            gpui_kit::point(px(0.0), px(0.0)),
            size(px(1280.0), px(800.0)),
        )
    });

    let has_click_rect = click_rect.size.width > 0 && click_rect.size.height > 0;
    let anchor = if has_click_rect {
        TrayAnchor {
            display_id,
            icon_x: click_rect.position.x as f32,
            icon_width: click_rect.size.width as f32,
            icon_bottom_y: (click_rect.position.y + click_rect.size.height as f64) as f32,
            visible_origin_x: f32::from(visible.origin.x),
            visible_origin_y: f32::from(visible.origin.y),
            visible_width: f32::from(visible.size.width),
            visible_height: f32::from(visible.size.height),
            #[cfg(any(target_os = "macos", test))]
            macos_screen_frame: None,
        }
    } else {
        TrayAnchor {
            display_id,
            icon_x: f32::from(visible.origin.x + visible.size.width) - POPOVER_WIDTH / 2.0 - 12.0,
            icon_width: 24.0,
            icon_bottom_y: f32::from(visible.origin.y),
            visible_origin_x: f32::from(visible.origin.x),
            visible_origin_y: f32::from(visible.origin.y),
            visible_width: f32::from(visible.size.width),
            visible_height: f32::from(visible.size.height),
            #[cfg(any(target_os = "macos", test))]
            macos_screen_frame: None,
        }
    };

    PopoverPlacement {
        bounds: compute_popover_bounds(anchor, POPOVER_WIDTH, POPOVER_HEIGHT),
        display_id,
        macos_screen_frame: None,
    }
}

fn schedule_memory_trim(cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(150))
            .await;
        crate::memory::trim_process_memory();

        cx.background_executor()
            .timer(Duration::from_millis(450))
            .await;
        let no_windows = cx.update(|cx| cx.windows().is_empty());
        if no_windows {
            crate::memory::trim_process_memory();
        }
    })
    .detach();
}

fn launch_project_from_tray(path: PathBuf, launcher_kind: LauncherKind, cx: &mut App) {
    let detected = {
        let state = cx.global::<AppState>();
        state
            .latest_launchers
            .iter()
            .find(|l| l.kind == launcher_kind)
            .cloned()
    };

    let Some(launcher) = detected else {
        eprintln!(
            "warning: launcher {} is no longer available",
            launcher_kind.display_name()
        );
        return;
    };

    if let Err(err) = open_project_with(&launcher, &path) {
        eprintln!(
            "error: failed to open {} with {}: {err}",
            path.display(),
            launcher_kind.display_name()
        );
        return;
    }

    let main_window = {
        let state = cx.global_mut::<AppState>();
        state
            .project_launcher_prefs
            .insert(path.clone(), launcher_kind);
        state.last_launcher = Some(launcher_kind);
        state.main_window.clone()
    };

    if let Some((_, view)) = main_window {
        view.update(cx, |panel, cx| {
            panel.record_launcher_preference(path, launcher_kind, cx);
        });
    }
}

fn sync_open_popover(cx: &mut App) {
    if !cx.has_global::<AppState>() {
        return;
    }
    let snapshot = {
        let state = cx.global::<AppState>();
        state.tray_popover.as_ref().map(|(_, view)| {
            (
                view.clone(),
                state.latest_report.clone(),
                state.latest_launchers.clone(),
                state.project_launcher_prefs.clone(),
                state.last_launcher,
                state.is_scanning,
            )
        })
    };

    if let Some((view, report, launchers, prefs, last_launcher, loading)) = snapshot {
        view.update(cx, |popover, cx| {
            popover.sync_state(report, launchers, prefs, last_launcher, loading, cx);
        });
    }
}

fn refresh_projects(cx: &mut App) {
    if let Some((_, view)) = current_main_window(cx) {
        view.update(cx, |panel, cx| {
            panel.trigger_refresh(cx);
        });
    } else {
        trigger_background_cache_refresh(cx);
    }
}

fn open_or_focus_main_window(cx: &mut App) -> Option<(AnyWindowHandle, Entity<ProjectWindow>)> {
    close_tray_popover(cx);

    if let Some((handle, view)) = current_main_window(cx) {
        let ok = handle.update(cx, |_, window, _| {
            window.activate_window();
        });
        if ok.is_ok() {
            cx.activate(true);
            return Some((handle, view));
        }
        clear_main_window(cx);
    }

    let (initial_report, initial_launchers, initial_prefs, initial_last_launcher) = {
        let state = cx.global::<AppState>();
        (
            state.latest_report.clone(),
            state.latest_launchers.clone(),
            state.project_launcher_prefs.clone(),
            state.last_launcher,
        )
    };
    let window_size = size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT));
    let bounds = Bounds::centered(None, window_size, cx);

    let callbacks = ProjectWindowCallbacks {
        on_language_changed: Some(Arc::new(|cx| apply_language_to_shell(cx))),
        on_state_changed: Some(Arc::new(|report, launchers, loading, cx| {
            if cx.has_global::<AppState>() {
                {
                    let state = cx.global_mut::<AppState>();
                    state.latest_report = report;
                    if !loading || !launchers.is_empty() {
                        state.latest_launchers = launchers;
                    }
                    state.is_scanning = loading;
                }
                sync_open_popover(cx);
            }
        })),
        on_launcher_used: Some(Arc::new(|path, launcher, cx| {
            if cx.has_global::<AppState>() {
                {
                    let state = cx.global_mut::<AppState>();
                    state.project_launcher_prefs.insert(path, launcher);
                    state.last_launcher = Some(launcher);
                }
                sync_open_popover(cx);
            }
        })),
    };

    let mut titlebar = TitleBar::title_bar_options();
    titlebar.title = Some(APP_NAME.into());

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        titlebar: Some(titlebar),
        app_id: Some(APP_ID.to_string()),
        kind: WindowKind::Normal,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        focus: true,
        show: true,
        ..TitleBar::window_options()
    };

    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            ProjectWindow::new(
                initial_report,
                initial_launchers,
                initial_prefs,
                initial_last_launcher,
                callbacks,
                window,
                cx,
            )
        })
    }) {
        Ok((handle, view)) => {
            cx.global_mut::<AppState>().main_window = Some((handle, view.clone()));
            cx.activate(true);
            Some((handle, view))
        }
        Err(err) => {
            eprintln!("error: failed to open main window: {err:#}");
            None
        }
    }
}

fn current_main_window(cx: &App) -> Option<(AnyWindowHandle, Entity<ProjectWindow>)> {
    cx.global::<AppState>().main_window.clone()
}

fn clear_main_window(cx: &mut App) {
    cx.global_mut::<AppState>().main_window = None;
}

fn trigger_background_cache_refresh(cx: &mut App) {
    {
        let state = cx.global_mut::<AppState>();
        if state.is_scanning {
            return;
        }
        state.is_scanning = true;
    }
    sync_open_popover(cx);

    let task = cx.background_executor().spawn(async move {
        let report = Arc::new(ScannerRegistry::new().scan_all());
        let launchers = Arc::new(detect_installed_launchers());
        crate::memory::trim_process_memory();
        (report, launchers)
    });

    cx.spawn(async move |cx| {
        let (report, launchers) = task.await;
        cx.update(|cx| {
            if cx.has_global::<AppState>() {
                {
                    let state = cx.global_mut::<AppState>();
                    state.latest_report = report;
                    state.latest_launchers = launchers;
                    state.is_scanning = false;
                }
                sync_open_popover(cx);
            }
        });
    })
    .detach();
}

fn run_cli_dump(as_json: bool) {
    let report = ScannerRegistry::new().scan_all();
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
        );
        return;
    }

    println!(
        "ProjPanel scan complete: {} projects across {} active agents ({} ms)",
        report.projects.len(),
        report.active_agent_count(),
        report.elapsed_ms
    );
    println!();
    println!("Detected Agents:");
    for summary in &report.agent_summaries {
        if summary.project_count > 0 || summary.error.is_some() {
            match &summary.error {
                Some(err) => println!("  - {:<14} ERROR: {err}", summary.agent.display_name()),
                None => println!(
                    "  - {:<14} {} projects",
                    summary.agent.display_name(),
                    summary.project_count
                ),
            }
        }
    }
    println!();
    println!("Detected Launchers:");
    for launcher in crate::launcher::detect_installed_launchers() {
        println!(
            "  - {:<16} ({})",
            launcher.kind.display_name(),
            launcher.kind.id()
        );
    }
    println!();
    println!("Projects:");
    for proj in &report.projects {
        let agents: Vec<&str> = proj.agents.iter().map(|u| u.agent.short_label()).collect();
        let status = if proj.exists { "OK" } else { "MISSING" };
        println!(
            "  [{status:<7}] {:<32} [{}]  {}",
            proj.name,
            agents.join(", "),
            proj.display_path
        );
    }
}
