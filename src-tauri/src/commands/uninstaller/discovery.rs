use std::fs;
use std::path::Path;

use super::AppInfo;
use crate::commands::utils::path_size;

/// Reads an app bundle's Info.plist and extracts metadata.
fn read_app_info(app_path: &Path) -> Option<AppInfo> {
    let plist_path = app_path.join("Contents/Info.plist");
    let plist = plist::Value::from_file(&plist_path).ok()?;
    let dict = plist.as_dictionary()?;

    let bundle_id = dict
        .get("CFBundleIdentifier")
        .and_then(|v| v.as_string())
        .unwrap_or("")
        .to_string();

    let name = dict
        .get("CFBundleName")
        .or_else(|| dict.get("CFBundleDisplayName"))
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| {
            app_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
        .to_string();

    let version = dict
        .get("CFBundleShortVersionString")
        .and_then(|v| v.as_string())
        .unwrap_or("")
        .to_string();

    let size = path_size(app_path);

    Some(AppInfo {
        bundle_id,
        name,
        version,
        path: app_path.to_string_lossy().to_string(),
        size,
    })
}

/// Scans a directory for .app bundles.
fn scan_dir(dir: &Path) -> Vec<AppInfo> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return vec![],
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "app")
                .unwrap_or(false)
        })
        .filter_map(|e| read_app_info(&e.path()))
        .collect()
}

/// Scans /Applications and ~/Applications for installed apps.
pub fn scan_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();

    // System applications
    apps.extend(scan_dir(Path::new("/Applications")));

    // User applications
    if let Some(home) = dirs::home_dir() {
        apps.extend(scan_dir(&home.join("Applications")));
    }

    // Sort by name case-insensitively
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Deduplicate by path
    apps.dedup_by(|a, b| a.path == b.path);

    apps
}
