mod app_icon;
mod config;
mod launcher;
mod memory;
mod model;
mod scanner;
mod tray;
mod ui;

use std::sync::Arc;

use futures::StreamExt;
use gpui_kit::component::{ActiveTheme, TitleBar};
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Entity, Global, KeyBinding, Menu, MenuItem, WindowBounds,
    WindowKind, WindowOptions, actions, prelude::*, px, size,
};

use crate::model::ScanReport;
use crate::scanner::ScannerRegistry;
use crate::tray::{TrayCommand, TrayHandle};
use crate::ui::icons::AppAssets;
use crate::ui::window::ProjectWindow;

const APP_NAME: &str = "ProjPanel";
const APP_ID: &str = "com.projpanel.app";
const DEFAULT_WINDOW_WIDTH: f32 = 1100.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 720.0;
const MIN_WINDOW_WIDTH: f32 = 780.0;
const MIN_WINDOW_HEIGHT: f32 = 520.0;

actions!(proj_panel, [Quit, RefreshProjects]);

struct TrayGlobal {
    _handle: TrayHandle,
}

impl Global for TrayGlobal {}

struct AppState {
    main_window: Option<(AnyWindowHandle, Entity<ProjectWindow>)>,
    latest_report: Arc<ScanReport>,
    has_tray: bool,
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
        cx.set_menus([Menu::new(APP_NAME).items([
            MenuItem::action("重新扫描项目", RefreshProjects),
            MenuItem::separator(),
            MenuItem::action(format!("退出 {APP_NAME}"), Quit),
        ])]);

        cx.set_global(AppState {
            main_window: None,
            latest_report: Arc::new(ScanReport::default()),
            has_tray: false,
        });

        cx.on_window_closed(|cx, _| {
                if !cx.has_global::<AppState>() {
                    return;
                }
                let remaining = cx.windows().len();
                if remaining == 0 {
                    let has_tray = {
                        let state = cx.global_mut::<AppState>();
                        state.main_window = None;
                        state.has_tray
                    };
                    if !has_tray {
                        cx.quit();
                    } else {
                        cx.spawn(async move |cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(300))
                                .await;
                            crate::memory::trim_process_memory();
                        })
                        .detach();
                    }
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
                            let _ = cx.update(|cx| handle_tray_command(cmd, cx));
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

fn handle_tray_command(cmd: TrayCommand, cx: &mut App) {
    match cmd {
        TrayCommand::ToggleWindow => toggle_main_window(cx),
        TrayCommand::OpenWindow => open_or_focus_main_window(cx),
        TrayCommand::RefreshProjects => refresh_projects(cx),
        TrayCommand::Quit => cx.quit(),
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

fn toggle_main_window(cx: &mut App) {
    if let Some((handle, _)) = current_main_window(cx) {
        let is_active = handle
            .update(cx, |_, window, _| window.is_window_active())
            .unwrap_or(false);
        if is_active {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            clear_main_window(cx);
            return;
        }
    }
    open_or_focus_main_window(cx);
}

fn open_or_focus_main_window(cx: &mut App) {
    if let Some((handle, _)) = current_main_window(cx) {
        let ok = handle.update(cx, |_, window, _| {
            window.activate_window();
        });
        if ok.is_ok() {
            cx.activate(true);
            return;
        }
        clear_main_window(cx);
    }

    let initial_report = cx.global::<AppState>().latest_report.clone();
    let window_size = size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT));
    let bounds = Bounds::centered(None, window_size, cx);

    let on_report_updated: Arc<dyn Fn(Arc<ScanReport>, &mut App) + Send + Sync> =
        Arc::new(|report, cx| {
            if cx.has_global::<AppState>() {
                cx.global_mut::<AppState>().latest_report = report;
            }
        });

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
        cx.new(|cx| ProjectWindow::new(initial_report, Some(on_report_updated), window, cx))
    }) {
        Ok((handle, view)) => {
            cx.global_mut::<AppState>().main_window = Some((handle, view));
            cx.activate(true);
        }
        Err(err) => {
            eprintln!("error: failed to open main window: {err:#}");
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
    let task = cx.background_executor().spawn(async move {
        let report = Arc::new(ScannerRegistry::new().scan_all());
        crate::memory::trim_process_memory();
        report
    });

    cx.spawn(async move |cx| {
        let report = task.await;
        let _ = cx.update(|cx| {
            if cx.has_global::<AppState>() {
                cx.global_mut::<AppState>().latest_report = report;
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
