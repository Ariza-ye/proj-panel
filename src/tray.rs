use anyhow::{Context, Result};
use futures::channel::mpsc::{self, UnboundedReceiver};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const MENU_ID_OPEN: &str = "open_window";
const MENU_ID_REFRESH: &str = "refresh_projects";
const MENU_ID_QUIT: &str = "quit_app";

/// High-level commands emitted by the system tray icon and its context menu.
#[derive(Debug, Clone, Copy)]
pub enum TrayCommand {
    ToggleWindow,
    OpenWindow,
    RefreshProjects,
    Quit,
}

/// Owns the live `TrayIcon` handle; dropping this struct removes the status bar icon.
pub struct TrayHandle {
    _tray_icon: TrayIcon,
}

impl TrayHandle {
    /// Initializes the cross-platform system tray icon and returns an async command receiver.
    ///
    /// Must be called on the main thread after the platform event loop has started.
    pub fn init() -> Result<(Self, UnboundedReceiver<TrayCommand>)> {
        let (tx, rx) = mpsc::unbounded();

        let menu = Menu::new();
        let item_open = MenuItem::with_id(MenuId::new(MENU_ID_OPEN), "打开主窗口", true, None);
        let item_refresh =
            MenuItem::with_id(MenuId::new(MENU_ID_REFRESH), "立即重新扫描", true, None);
        let item_quit = MenuItem::with_id(MenuId::new(MENU_ID_QUIT), "退出 ProjPanel", true, None);
        menu.append_items(&[
            &item_open,
            &item_refresh,
            &PredefinedMenuItem::separator(),
            &item_quit,
        ])
        .context("failed to build tray context menu")?;

        let icon = build_tray_icon_rgba().context("failed to build tray icon bitmap")?;

        let mut builder = TrayIconBuilder::new()
            .with_tooltip("ProjPanel — AI Agent 项目汇总")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false);

        #[cfg(target_os = "macos")]
        {
            builder = builder.with_icon_templated(icon);
        }
        #[cfg(not(target_os = "macos"))]
        {
            builder = builder.with_icon(icon);
        }

        let tray_icon = builder.build().context("failed to create system tray icon")?;

        let tx_click = tx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tx_click.unbounded_send(TrayCommand::ToggleWindow);
            }
        }));

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let cmd = match event.id().0.as_str() {
                MENU_ID_OPEN => Some(TrayCommand::OpenWindow),
                MENU_ID_REFRESH => Some(TrayCommand::RefreshProjects),
                MENU_ID_QUIT => Some(TrayCommand::Quit),
                _ => None,
            };
            if let Some(cmd) = cmd {
                let _ = tx.unbounded_send(cmd);
            }
        }));

        Ok((
            Self {
                _tray_icon: tray_icon,
            },
            rx,
        ))
    }
}

/// Generates a 32x32 monochrome RGBA bitmap representing a project panel glyph.
/// On macOS template mode, alpha controls the visible silhouette against light/dark menu bars.
fn build_tray_icon_rgba() -> Result<Icon> {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];

    let set_px = |buf: &mut [u8], x: usize, y: usize, alpha: u8| {
        if x < SIZE && y < SIZE {
            let idx = (y * SIZE + x) * 4;
            buf[idx] = 240;
            buf[idx + 1] = 244;
            buf[idx + 2] = 250;
            buf[idx + 3] = alpha;
        }
    };

    // Outer rounded frame (x: 4..28, y: 5..27, thickness 2px).
    for y in 5..27 {
        for x in 4..28 {
            let is_corner = (x == 4 || x == 27) && (y == 5 || y == 26);
            if is_corner {
                continue;
            }
            let on_border = x <= 5 || x >= 26 || y <= 6 || y >= 25;
            if on_border {
                set_px(&mut rgba, x, y, 255);
            }
        }
    }

    // Header divider (y: 10..12).
    for y in 10..12 {
        for x in 6..26 {
            set_px(&mut rgba, x, y, 220);
        }
    }

    // Three project list rows inside the frame.
    let rows = [
        (14..16, 8..11, 13..24),
        (18..20, 8..11, 13..22),
        (22..24, 8..11, 13..20),
    ];
    for (yr, dot_xr, bar_xr) in rows {
        for y in yr {
            for x in dot_xr.clone() {
                set_px(&mut rgba, x, y, 255);
            }
            for x in bar_xr.clone() {
                set_px(&mut rgba, x, y, 235);
            }
        }
    }

    Ok(Icon::from_rgba(rgba, SIZE as u32, SIZE as u32)?)
}
