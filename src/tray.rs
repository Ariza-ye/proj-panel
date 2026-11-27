use anyhow::{Context, Result};
use futures::channel::mpsc::{self, UnboundedReceiver};
use gpui_kit::{Bounds, DisplayId, Pixels, point, px, size};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

const MENU_ID_OPEN: &str = "open_window";
const MENU_ID_REFRESH: &str = "refresh_projects";
const MENU_ID_QUIT: &str = "quit_app";

const POPOVER_SCREEN_MARGIN: f32 = 8.0;
const POPOVER_ANCHOR_GAP: f32 = 6.0;
#[cfg(any(target_os = "macos", test))]
const FALLBACK_ICON_WIDTH: f64 = 28.0;
#[cfg(any(target_os = "macos", test))]
const MIN_MENU_BAR_HEIGHT: f32 = 25.0;

/// High-level commands emitted by the system tray icon and its fallback context menu.
#[derive(Debug, Clone)]
pub enum TrayCommand {
    TogglePopover {
        rect: Rect,
        anchor: Option<TrayAnchor>,
    },
    OpenWindow,
    RefreshProjects,
    Quit,
}

/// Logical display-relative anchor geometry for positioning the GPUI tray popover.
#[derive(Debug, Clone, Copy)]
pub struct TrayAnchor {
    pub display_id: Option<DisplayId>,
    pub icon_x: f32,
    pub icon_width: f32,
    pub icon_bottom_y: f32,
    pub visible_origin_x: f32,
    pub visible_origin_y: f32,
    pub visible_width: f32,
    pub visible_height: f32,
    /// Global AppKit screen frame `(origin_x, origin_y, height)` for exact multi-monitor placement.
    #[cfg(any(target_os = "macos", test))]
    pub macos_screen_frame: Option<(f64, f64, f64)>,
}

/// Owns the live `TrayIcon` handle; dropping this struct removes the status bar icon.
pub struct TrayHandle {
    _tray_icon: TrayIcon,
    item_open: MenuItem,
    item_refresh: MenuItem,
    item_quit: MenuItem,
}

fn tray_tooltip() -> &'static str {
    tr!(
        "ProjPanel — AI Agent 项目汇总",
        "ProjPanel — AI Agent project overview"
    )
}

impl TrayHandle {
    /// Re-applies the current interface language to the tray tooltip and context menu.
    pub fn apply_language(&self) {
        self.item_open
            .set_text(tr!("打开主窗口", "Open main window"));
        self.item_refresh
            .set_text(tr!("立即重新扫描", "Rescan now"));
        self.item_quit
            .set_text(tr!("退出 ProjPanel", "Quit ProjPanel"));
        let _ = self._tray_icon.set_tooltip(Some(tray_tooltip()));
    }

    /// Initializes the cross-platform system tray icon and returns an async command receiver.
    ///
    /// Left click toggles the GPUI project popover; right click opens the minimal context menu.
    /// Must be called on the main thread after the platform event loop has started.
    pub fn init() -> Result<(Self, UnboundedReceiver<TrayCommand>)> {
        let (tx, rx) = mpsc::unbounded();

        let menu = Menu::new();
        let item_open = MenuItem::with_id(
            MenuId::new(MENU_ID_OPEN),
            tr!("打开主窗口", "Open main window"),
            true,
            None,
        );
        let item_refresh = MenuItem::with_id(
            MenuId::new(MENU_ID_REFRESH),
            tr!("立即重新扫描", "Rescan now"),
            true,
            None,
        );
        let item_quit = MenuItem::with_id(
            MenuId::new(MENU_ID_QUIT),
            tr!("退出 ProjPanel", "Quit ProjPanel"),
            true,
            None,
        );
        menu.append_items(&[
            &item_open,
            &item_refresh,
            &PredefinedMenuItem::separator(),
            &item_quit,
        ])
        .context("failed to build tray context menu")?;

        let icon = build_tray_icon_rgba().context("failed to build tray icon bitmap")?;

        let mut builder = TrayIconBuilder::new()
            .with_tooltip(tray_tooltip())
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_menu_on_right_click(true);

        #[cfg(target_os = "macos")]
        {
            builder = builder.with_icon_templated(icon);
        }
        #[cfg(not(target_os = "macos"))]
        {
            builder = builder.with_icon(icon);
        }

        let tray_icon = builder
            .build()
            .context("failed to create system tray icon")?;

        let tx_click = tx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            // Trigger on MouseDown on macOS so the popover responds immediately like native menu bar extras
            // and before window focus transitions race with MouseUp.
            #[cfg(target_os = "macos")]
            let expected_state = MouseButtonState::Down;
            #[cfg(not(target_os = "macos"))]
            let expected_state = MouseButtonState::Up;

            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state,
                rect,
                ..
            } = event
                && button_state == expected_state
            {
                // Capture screen & cursor coordinates synchronously inside mouseDown on macOS so
                // multi-monitor clicks always anchor to the exact screen where the user clicked.
                #[cfg(target_os = "macos")]
                let anchor = capture_macos_click_anchor(rect);
                #[cfg(not(target_os = "macos"))]
                let anchor = None;

                let _ = tx_click.unbounded_send(TrayCommand::TogglePopover { rect, anchor });
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
                item_open,
                item_refresh,
                item_quit,
            },
            rx,
        ))
    }
}

/// Captures the exact clicked screen and icon anchor on macOS using `NSEvent::mouseLocation()`
/// instead of `NSStatusItem::button().window()`, which can report a stale mirror screen on
/// multi-monitor setups with separate Spaces.
#[cfg(target_os = "macos")]
fn capture_macos_click_anchor(click_rect: Rect) -> Option<TrayAnchor> {
    use objc2::msg_send;
    use objc2_app_kit::{NSEvent, NSScreen};
    use objc2_foundation::{MainThreadMarker, ns_string};

    let mtm = MainThreadMarker::new()?;
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return None;
    }

    let mut chosen_screen = None;
    let mut best_dist_sq = f64::INFINITY;
    for screen in screens.iter() {
        let f = screen.frame();
        let inside_x = mouse.x >= f.origin.x && mouse.x <= f.origin.x + f.size.width;
        let inside_y = mouse.y >= f.origin.y && mouse.y <= f.origin.y + f.size.height;
        if inside_x && inside_y {
            chosen_screen = Some(screen);
            break;
        }
        let dx = if mouse.x < f.origin.x {
            f.origin.x - mouse.x
        } else if mouse.x > f.origin.x + f.size.width {
            mouse.x - (f.origin.x + f.size.width)
        } else {
            0.0
        };
        let dy = if mouse.y < f.origin.y {
            f.origin.y - mouse.y
        } else if mouse.y > f.origin.y + f.size.height {
            mouse.y - (f.origin.y + f.size.height)
        } else {
            0.0
        };
        let dist_sq = dx * dx + dy * dy;
        if dist_sq < best_dist_sq {
            best_dist_sq = dist_sq;
            chosen_screen = Some(screen);
        }
    }

    let screen = chosen_screen?;
    let screen_frame = screen.frame();
    let visible_frame = screen.visibleFrame();
    let scale = screen.backingScaleFactor().max(1.0);

    let screen_number: usize = screen
        .deviceDescription()
        .objectForKey(ns_string!("NSScreenNumber"))
        .map(|num| unsafe { msg_send![&*num, unsignedIntegerValue] })
        .unwrap_or(0);
    let display_id = (screen_number != 0).then(|| DisplayId::new(screen_number as u64));

    let (icon_x, icon_width) = resolve_macos_icon_horizontal_span(
        click_rect.position.x,
        click_rect.size.width as f64,
        scale,
        mouse.x,
        screen_frame.origin.x,
        screen_frame.size.width,
    );

    let screen_top = screen_frame.origin.y + screen_frame.size.height;
    let visible_origin_x = (visible_frame.origin.x - screen_frame.origin.x) as f32;
    let visible_origin_y =
        (screen_top - (visible_frame.origin.y + visible_frame.size.height)) as f32;
    let icon_bottom_y = visible_origin_y.max(MIN_MENU_BAR_HEIGHT);

    Some(TrayAnchor {
        display_id,
        icon_x,
        icon_width,
        icon_bottom_y,
        visible_origin_x,
        visible_origin_y,
        visible_width: visible_frame.size.width as f32,
        visible_height: visible_frame.size.height as f32,
        macos_screen_frame: Some((
            screen_frame.origin.x,
            screen_frame.origin.y,
            screen_frame.size.height,
        )),
    })
}

/// Resolves display-relative `(icon_x, icon_width)` on macOS, verifying that the reported tray
/// click rect actually belongs to the clicked screen and covers the cursor; otherwise falls back
/// to centering around `mouse_x`.
#[cfg(any(target_os = "macos", test))]
fn resolve_macos_icon_horizontal_span(
    rect_phys_x: f64,
    rect_phys_width: f64,
    scale: f64,
    mouse_x: f64,
    screen_origin_x: f64,
    screen_width: f64,
) -> (f32, f32) {
    let rect_x = rect_phys_x / scale;
    let rect_w = rect_phys_width / scale;
    let screen_right = screen_origin_x + screen_width;

    let rect_on_screen = rect_w > 0.0
        && rect_w <= 200.0
        && rect_x >= screen_origin_x - 4.0
        && rect_x + rect_w <= screen_right + 4.0;
    let rect_near_cursor = mouse_x >= rect_x - 24.0 && mouse_x <= rect_x + rect_w + 24.0;

    if rect_on_screen && rect_near_cursor {
        ((rect_x - screen_origin_x) as f32, rect_w as f32)
    } else {
        (
            (mouse_x - screen_origin_x - FALLBACK_ICON_WIDTH / 2.0) as f32,
            FALLBACK_ICON_WIDTH as f32,
        )
    }
}

/// Converts display-relative top-left GPUI popover bounds into global AppKit `(x, y, width, height)`
/// (bottom-left origin) for direct `NSWindow::setFrame:display:`.
#[cfg(any(target_os = "macos", test))]
pub fn compute_macos_global_frame(
    bounds: Bounds<Pixels>,
    screen_frame: (f64, f64, f64),
) -> (f64, f64, f64, f64) {
    let (screen_origin_x, screen_origin_y, screen_height) = screen_frame;
    let width = f32::from(bounds.size.width) as f64;
    let height = f32::from(bounds.size.height) as f64;
    let global_x = screen_origin_x + f32::from(bounds.origin.x) as f64;
    let global_top_y = screen_origin_y + screen_height - f32::from(bounds.origin.y) as f64;
    let global_bottom_y = global_top_y - height;
    (global_x, global_bottom_y, width, height)
}

/// Applies the exact global AppKit frame directly to a GPUI window on macOS, bypassing
/// `initWithContentRect:screen:` multi-monitor origin offset quirks.
#[cfg(target_os = "macos")]
pub fn apply_macos_window_frame(
    window: &gpui_kit::Window,
    bounds: Bounds<Pixels>,
    screen_frame: (f64, f64, f64),
) {
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };

    let (x, y, width, height) = compute_macos_global_frame(bounds, screen_frame);
    let target_frame = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));

    unsafe {
        let ns_view = &*appkit.ns_view.as_ptr().cast::<NSView>();
        if let Some(ns_window) = ns_view.window() {
            ns_window.setFrame_display(target_frame, true);
        }
    }
}

/// Computes display-relative logical bounds for the popover window, centered under the tray icon
/// and clamped within the target display's visible area.
pub fn compute_popover_bounds(
    anchor: TrayAnchor,
    preferred_width: f32,
    preferred_height: f32,
) -> Bounds<Pixels> {
    let max_width = (anchor.visible_width - POPOVER_SCREEN_MARGIN * 2.0).max(280.0);
    let max_height = (anchor.visible_height - POPOVER_SCREEN_MARGIN * 2.0).max(260.0);
    let width = preferred_width.min(max_width);
    let height = preferred_height.min(max_height);

    let icon_center_x = anchor.icon_x + anchor.icon_width / 2.0;
    let min_x = anchor.visible_origin_x + POPOVER_SCREEN_MARGIN;
    let max_x =
        (anchor.visible_origin_x + anchor.visible_width - width - POPOVER_SCREEN_MARGIN).max(min_x);
    let x = (icon_center_x - width / 2.0).clamp(min_x, max_x);

    let min_y = anchor.visible_origin_y + POPOVER_ANCHOR_GAP;
    let max_y = (anchor.visible_origin_y + anchor.visible_height - height - POPOVER_SCREEN_MARGIN)
        .max(min_y);
    let below_icon_y = anchor.icon_bottom_y + POPOVER_ANCHOR_GAP;
    let y = if below_icon_y + height <= anchor.visible_origin_y + anchor.visible_height {
        below_icon_y.max(min_y)
    } else {
        // Taskbar at bottom of screen (e.g. Windows): place above the tray icon.
        (anchor.icon_bottom_y - height - POPOVER_ANCHOR_GAP).clamp(min_y, max_y)
    };

    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_popover_under_tray_icon_and_clamps_near_right_edge() {
        let centered_anchor = TrayAnchor {
            display_id: None,
            icon_x: 900.0,
            icon_width: 28.0,
            icon_bottom_y: 25.0,
            visible_origin_x: 0.0,
            visible_origin_y: 25.0,
            visible_width: 1512.0,
            visible_height: 920.0,
            macos_screen_frame: None,
        };
        let bounds = compute_popover_bounds(centered_anchor, 400.0, 540.0);
        assert_eq!(bounds.origin.x, px(714.0));
        assert_eq!(bounds.origin.y, px(31.0));
        assert_eq!(bounds.size.width, px(400.0));
        assert_eq!(bounds.size.height, px(540.0));

        let right_edge_anchor = TrayAnchor {
            icon_x: 1460.0,
            ..centered_anchor
        };
        let clamped = compute_popover_bounds(right_edge_anchor, 400.0, 540.0);
        assert_eq!(clamped.origin.x, px(1512.0 - 400.0 - POPOVER_SCREEN_MARGIN));
    }

    #[test]
    fn clamps_popover_height_on_small_displays() {
        let small_anchor = TrayAnchor {
            display_id: None,
            icon_x: 400.0,
            icon_width: 24.0,
            icon_bottom_y: 24.0,
            visible_origin_x: 0.0,
            visible_origin_y: 24.0,
            visible_width: 800.0,
            visible_height: 400.0,
            macos_screen_frame: None,
        };
        let bounds = compute_popover_bounds(small_anchor, 400.0, 540.0);
        assert_eq!(bounds.size.height, px(400.0 - POPOVER_SCREEN_MARGIN * 2.0));
    }

    #[test]
    fn falls_back_to_cursor_when_macos_tray_rect_belongs_to_another_display() {
        // Secondary display at x = 1512..4072 (scale 1.0), user clicked at x = 3800,
        // but tray-icon reported stale mirror rect from primary display at x = 1200.
        let (icon_x, icon_w) =
            resolve_macos_icon_horizontal_span(1200.0, 28.0, 1.0, 3800.0, 1512.0, 2560.0);
        assert_eq!(icon_w, FALLBACK_ICON_WIDTH as f32);
        assert_eq!(icon_x, (3800.0 - 1512.0 - FALLBACK_ICON_WIDTH / 2.0) as f32);

        // Valid Retina (scale 2.0) rect on the same display matches cursor.
        let (valid_x, valid_w) = resolve_macos_icon_horizontal_span(
            3800.0 * 2.0,
            28.0 * 2.0,
            2.0,
            3812.0,
            1512.0,
            2560.0,
        );
        assert_eq!(valid_x, (3800.0 - 1512.0) as f32);
        assert_eq!(valid_w, 28.0);
    }

    #[test]
    fn computes_exact_global_appkit_frame_across_secondary_displays() {
        // Monitor to the right with negative Y offset: origin (1512, -98), height 1440 (top = 1342).
        let bounds = Bounds::new(point(px(2084.0), px(31.0)), size(px(408.0), px(520.0)));
        let (x, y, w, h) = compute_macos_global_frame(bounds, (1512.0, -98.0, 1440.0));
        assert_eq!(x, 3596.0);
        assert_eq!(y, 1342.0 - 31.0 - 520.0);
        assert_eq!(w, 408.0);
        assert_eq!(h, 520.0);

        // Monitor to the left: origin (-1920, 0), height 1080.
        let left_bounds = Bounds::new(point(px(1504.0), px(31.0)), size(px(408.0), px(520.0)));
        let (lx, ly, lw, lh) = compute_macos_global_frame(left_bounds, (-1920.0, 0.0, 1080.0));
        assert_eq!(lx, -416.0);
        assert_eq!(ly, 1080.0 - 31.0 - 520.0);
        assert_eq!(lw, 408.0);
        assert_eq!(lh, 520.0);
    }
}
