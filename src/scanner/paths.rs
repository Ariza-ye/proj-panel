use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use percent_encoding::percent_decode_str;

/// Returns the user's home directory.
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Returns the platform-standard application configuration directory for an app name.
///
/// - macOS: `~/Library/Application Support/<app_name>`
/// - Windows: `%APPDATA%/<app_name>`
/// - Linux/BSD: `~/.config/<app_name>`
pub fn app_config_dir(app_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().map(|d| d.join(app_name))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir().map(|d| d.join(app_name))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs::config_dir().map(|d| d.join(app_name))
    }
}

/// Returns the platform-standard local data directory for an app name.
///
/// - macOS: `~/Library/Application Support/<app_name>`
/// - Windows: `%LOCALAPPDATA%/<app_name>`
/// - Linux/BSD: `~/.local/share/<app_name>`
pub fn app_local_data_dir(app_name: &str) -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(app_name))
}

/// Returns `~/.local/share/<app_name>` in addition to `dirs::data_local_dir()`,
/// since many cross-platform CLI agents (such as OpenCode) use XDG paths on macOS too.
pub fn xdg_data_candidates(app_name: &str) -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    if let Some(home) = home_dir() {
        dirs_out.push(home.join(".local").join("share").join(app_name));
    }
    if let Some(local) = app_local_data_dir(app_name) {
        if !dirs_out.contains(&local) {
            dirs_out.push(local);
        }
    }
    dirs_out
}

/// Decodes a `file://` URI into a local filesystem path.
pub fn decode_file_uri(uri: &str) -> Option<PathBuf> {
    let trimmed = uri.trim();
    if !trimmed.starts_with("file://") {
        let p = PathBuf::from(trimmed);
        return if p.is_absolute() { Some(p) } else { None };
    }

    let rest = trimmed.strip_prefix("file://")?;
    // Ignore remote hosts (only accept empty host `file:///...` or `file://localhost/...`).
    let path_part = if let Some(after_localhost) = rest.strip_prefix("localhost/") {
        format!("/{after_localhost}")
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        return None;
    };

    let decoded = percent_decode_str(&path_part).decode_utf8().ok()?;
    let decoded_str = decoded.as_ref();

    // Handle Windows drive letter form `/C:/foo/bar` -> `C:/foo/bar`.
    #[cfg(windows)]
    let final_str = {
        let bytes = decoded_str.as_bytes();
        if bytes.len() >= 3 && bytes[0] == b'/' && bytes[2] == b':' {
            &decoded_str[1..]
        } else {
            decoded_str
        }
    };
    #[cfg(not(windows))]
    let final_str = decoded_str;

    Some(PathBuf::from(final_str))
}

/// Cleans lexical path components (redundant separators, `.` and `..`, trailing slashes)
/// without failing when the target directory no longer exists on disk.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Produces a stable deduplication key for a normalized path.
/// Uses lowercase on case-insensitive filesystems (macOS/Windows).
pub fn dedup_key(path: &Path) -> String {
    let s = path.to_string_lossy();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        s.to_lowercase()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        s.into_owned()
    }
}

/// Returns true if the path is a trivial root or home directory rather than a real project.
pub fn is_noise_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if s.is_empty() || s == "/" || s == "\\" {
        return true;
    }
    // Windows drive roots like `C:\` or `C:/`.
    if s.len() <= 3 && s.as_bytes().get(1) == Some(&b':') {
        return true;
    }
    if let Some(home) = home_dir() {
        if normalize_path(&home) == normalize_path(path) {
            return true;
        }
    }
    // Filter out internal scratch/chat/visualization directories created by agents themselves.
    let normalized_sep = s.replace('\\', "/");
    for marker in [
        "/.codex/visualizations/",
        "/.config/openchamber/chats/",
        "/.gemini/tmp/",
        "/.gemini/antigravity/brain/",
        "/.gemini/antigravity-cli/brain/",
    ] {
        if normalized_sep.contains(marker) {
            return true;
        }
    }
    false
}

/// Formats a path for compact display by replacing the home directory prefix with `~`.
pub fn format_display_path(path: &Path) -> String {
    if let Some(home) = home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            let rel_str = rel.to_string_lossy();
            if rel_str.is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", rel_str.replace('\\', "/"));
        }
    }
    path.to_string_lossy().into_owned()
}

/// Extracts a readable project name from the final path segment.
pub fn project_name_from_path(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Returns a file or directory's modification time as Unix seconds.
pub fn file_mtime_secs(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Normalizes a Unix timestamp that may be expressed in seconds, milliseconds, or microseconds.
pub fn normalize_unix_timestamp(raw: i64) -> Option<i64> {
    if raw <= 0 {
        return None;
    }
    if raw > 1_000_000_000_000_000 {
        // Microseconds
        Some(raw / 1_000_000)
    } else if raw > 100_000_000_000 {
        // Milliseconds
        Some(raw / 1_000)
    } else {
        Some(raw)
    }
}

/// Parses an RFC3339 or SQLite UTC timestamp string (`YYYY-MM-DD HH:MM:SS`) into Unix seconds.
pub fn parse_timestamp_str(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.timestamp());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc().timestamp());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(naive.and_utc().timestamp());
    }
    None
}

/// Opens a directory in the operating system's native file manager.
pub fn open_in_file_manager(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_encoded_file_uri() {
        let p = decode_file_uri("file:///Users/demo/my%20project/app").unwrap();
        assert_eq!(p, PathBuf::from("/Users/demo/my project/app"));
    }

    #[test]
    fn ignores_remote_file_uri() {
        assert!(decode_file_uri("file://remote-host/Users/demo/app").is_none());
    }

    #[test]
    fn normalizes_unix_timestamps() {
        assert_eq!(normalize_unix_timestamp(1_700_000_000), Some(1_700_000_000));
        assert_eq!(
            normalize_unix_timestamp(1_700_000_000_000),
            Some(1_700_000_000)
        );
        assert_eq!(
            normalize_unix_timestamp(1_700_000_000_000_000),
            Some(1_700_000_000)
        );
        assert_eq!(normalize_unix_timestamp(0), None);
    }

    #[test]
    fn parses_timestamp_strings() {
        assert!(parse_timestamp_str("2026-11-23 03:51:00").is_some());
        assert!(parse_timestamp_str("2026-11-23T03:51:00Z").is_some());
    }
}
