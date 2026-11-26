//! Embedded application icons (light and dark variants) and macOS Dock icon synchronization.

/// Light-mode macOS application icon (512x512 PNG, Apple squircle geometry).
pub const APP_ICON_LIGHT_PNG: &[u8] = include_bytes!("../assets/icons/app/app-icon-light.png");

/// Dark-mode macOS application icon (512x512 PNG, Apple squircle geometry).
pub const APP_ICON_DARK_PNG: &[u8] = include_bytes!("../assets/icons/app/app-icon-dark.png");

/// Returns the embedded PNG bytes matching the requested color scheme.
pub const fn icon_png_bytes(is_dark: bool) -> &'static [u8] {
    if is_dark {
        APP_ICON_DARK_PNG
    } else {
        APP_ICON_LIGHT_PNG
    }
}

/// Applies the light or dark application icon to the macOS Dock at runtime.
///
/// Must be called on the main thread. On non-macOS platforms this is a no-op.
#[cfg(target_os = "macos")]
pub fn apply_dock_icon(is_dark: bool) {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(icon_png_bytes(is_dark));
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `image` is a valid retained `NSImage` passed on the main thread.
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
}

/// No-op placeholder on non-macOS platforms.
#[cfg(not(target_os = "macos"))]
pub fn apply_dock_icon(_is_dark: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    #[test]
    fn embedded_app_icons_are_valid_512px_pngs() {
        for (label, bytes) in [("light", APP_ICON_LIGHT_PNG), ("dark", APP_ICON_DARK_PNG)] {
            assert!(
                bytes.len() > 24 && &bytes[..8] == PNG_SIGNATURE,
                "{label} app icon is not a valid PNG"
            );
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
            assert_eq!((width, height), (512, 512), "{label} app icon dimensions mismatch");
        }
        assert_ne!(
            APP_ICON_LIGHT_PNG, APP_ICON_DARK_PNG,
            "light and dark app icons must differ"
        );
    }
}
