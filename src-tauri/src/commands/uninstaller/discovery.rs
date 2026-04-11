use std::fs;
use std::path::Path;

use super::AppInfo;
use crate::commands::utils::path_size;

/// Patterns that indicate a data-sensitive app (password managers, VPNs, crypto tools).
const DATA_SENSITIVE_PATTERNS: &[&str] = &[
    "1password", "bitwarden", "lastpass", "keepass", "dashlane", "enpass",
    "keychain", "nordvpn", "expressvpn", "tunnelblick", "wireguard",
    "pgp", "gpg", "ssh",
];

/// Returns true if the bundle_id or app name suggests a data-sensitive app.
fn check_data_sensitive(bundle_id: &str, name: &str) -> bool {
    let bid = bundle_id.to_lowercase();
    let n = name.to_lowercase();
    DATA_SENSITIVE_PATTERNS
        .iter()
        .any(|pat| bid.contains(pat) || n.contains(pat))
}

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

    let path_str = app_path.to_string_lossy().to_string();
    let is_system = path_str.starts_with("/System/Applications/");
    let is_data_sensitive = check_data_sensitive(&bundle_id, &name);

    Some(AppInfo {
        bundle_id,
        name,
        version,
        path: path_str,
        size,
        is_system,
        is_data_sensitive,
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

/// Scans a Homebrew Caskroom directory for .app bundles inside version subdirectories.
fn scan_caskroom(caskroom: &Path) -> Vec<AppInfo> {
    let entries = match fs::read_dir(caskroom) {
        Ok(entries) => entries,
        Err(_) => return vec![],
    };

    let mut apps = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let cask_dir = entry.path();
        if !cask_dir.is_dir() {
            continue;
        }
        // Each cask has version subdirectories, scan each for .app bundles
        let versions = match fs::read_dir(&cask_dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for version_entry in versions.filter_map(|e| e.ok()) {
            let version_dir = version_entry.path();
            if !version_dir.is_dir() {
                continue;
            }
            apps.extend(scan_dir(&version_dir));
        }
    }
    apps
}

/// Scans /Applications, /System/Applications, ~/Applications, Homebrew
/// Caskrooms, and Setapp for installed apps. System apps are surfaced with
/// `is_system = true` so the UI can display them but prevent removal.
pub fn scan_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();

    // User-installed applications
    apps.extend(scan_dir(Path::new("/Applications")));

    // macOS built-in applications (surfaced read-only)
    apps.extend(scan_dir(Path::new("/System/Applications")));
    apps.extend(scan_dir(Path::new("/System/Applications/Utilities")));

    // User applications
    if let Some(home) = dirs::home_dir() {
        apps.extend(scan_dir(&home.join("Applications")));

        // Setapp applications
        let setapp_dir = home.join("Applications/Setapp");
        if setapp_dir.exists() {
            apps.extend(scan_dir(&setapp_dir));
        }
    }

    // Homebrew Caskroom locations
    for caskroom_path in &["/opt/homebrew/Caskroom", "/usr/local/Caskroom"] {
        let caskroom = Path::new(caskroom_path);
        if caskroom.exists() {
            apps.extend(scan_caskroom(caskroom));
        }
    }

    // Sort by name case-insensitively
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Deduplicate by path
    apps.dedup_by(|a, b| a.path == b.path);

    apps
}
