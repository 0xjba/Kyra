use std::fs;

use super::AssociatedFile;
use crate::commands::utils::path_size;

/// Directories under ~/Library to search, with human-readable category names.
const SEARCH_LOCATIONS: &[(&str, &str)] = &[
    ("Application Support", "App Data"),
    ("Preferences", "Preferences"),
    ("Caches", "Caches"),
    ("Containers", "Containers"),
    ("Group Containers", "Group Containers"),
    ("Logs", "Logs"),
    ("Saved Application State", "Saved State"),
    ("WebKit", "WebKit Data"),
    ("HTTPStorages", "HTTP Storage"),
];

/// Checks if a directory entry name matches the bundle ID or app name.
fn matches_app(entry_name: &str, bundle_id: &str, app_name: &str) -> bool {
    let entry_lower = entry_name.to_lowercase();
    let bundle_lower = bundle_id.to_lowercase();
    let name_lower = app_name.to_lowercase();

    // Exact bundle ID match (most reliable)
    if !bundle_id.is_empty() && entry_lower == bundle_lower {
        return true;
    }

    // Bundle ID prefix match (e.g., "com.example.app.helper")
    if !bundle_id.is_empty() && entry_lower.starts_with(&format!("{}.", bundle_lower)) {
        return true;
    }

    // App name match (for directories named after the app)
    if !app_name.is_empty() && entry_lower == name_lower {
        return true;
    }

    false
}

/// Searches ~/Library subdirectories for files associated with the given app.
pub fn find_associated(bundle_id: &str, app_name: &str, _app_path: &str) -> Vec<AssociatedFile> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let library = home.join("Library");
    let mut results = Vec::new();

    for (dir_name, category) in SEARCH_LOCATIONS {
        let search_dir = library.join(dir_name);
        if !search_dir.exists() {
            continue;
        }

        let entries = match fs::read_dir(&search_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if !matches_app(&entry_name, bundle_id, app_name) {
                continue;
            }

            let path = entry.path();
            let size = path_size(&path);
            if size == 0 {
                continue;
            }

            results.push(AssociatedFile {
                path: path.to_string_lossy().to_string(),
                category: category.to_string(),
                size,
                is_dir: path.is_dir(),
            });
        }
    }

    // Also check ~/Library/Preferences for .plist files matching bundle ID
    if !bundle_id.is_empty() {
        let prefs_dir = library.join("Preferences");
        if prefs_dir.exists() {
            let plist_name = format!("{}.plist", bundle_id);
            let plist_path = prefs_dir.join(&plist_name);
            if plist_path.exists() {
                let size = plist_path.metadata().map(|m| m.len()).unwrap_or(0);
                // Avoid duplicates — only add if not already found
                if size > 0 && !results.iter().any(|r| r.path == plist_path.to_string_lossy().as_ref()) {
                    results.push(AssociatedFile {
                        path: plist_path.to_string_lossy().to_string(),
                        category: "Preferences".to_string(),
                        size,
                        is_dir: false,
                    });
                }
            }
        }
    }

    results
}
