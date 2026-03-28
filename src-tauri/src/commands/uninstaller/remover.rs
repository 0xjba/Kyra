use std::fs;
use std::path::Path;

use super::{UninstallProgress, UninstallResult};
use crate::commands::shared;
use crate::commands::utils::dir_size;

/// Paths that must never be deleted.
const PROTECTED_PATHS: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/etc",
    "/var/db",
    "/Library/Frameworks",
    "/Applications",
];

/// User-relative directories that must not be deleted as a whole.
const PROTECTED_HOME_DIRS: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "Pictures",
    "Music",
    "Movies",
];

/// Returns true if a path is safe to delete.
/// Allows deleting individual .app bundles inside /Applications (e.g. /Applications/Foo.app)
/// but blocks deleting /Applications itself or its non-.app contents.
fn is_safe_path(path: &str) -> bool {
    // Block exact protected system paths and their children
    for protected in PROTECTED_PATHS {
        if path == *protected {
            return false;
        }
        // Special case: allow /Applications/*.app but block /Applications itself
        if *protected == "/Applications" && path.starts_with("/Applications/") {
            // Only allow .app bundles directly in /Applications
            let remainder = &path["/Applications/".len()..];
            if remainder.contains('/') {
                // It's a path inside an app bundle — allow
                continue;
            }
            if !remainder.ends_with(".app") {
                return false;
            }
            continue;
        }
        if path.starts_with(&format!("{}/", protected)) {
            return false;
        }
    }

    // Block home directory itself and key user directories
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path == home_str.as_ref() {
            return false;
        }
        for dir in PROTECTED_HOME_DIRS {
            let protected = format!("{}/{}", home_str, dir);
            if path == protected {
                return false;
            }
        }
    }

    true
}

/// Removes the app bundle and selected associated files.
/// Calls `on_progress` after each item is processed.
pub fn remove_app_and_files<F>(
    app_path: &str,
    file_paths: &[String],
    dry_run: bool,
    mut on_progress: F,
) -> UninstallResult
where
    F: FnMut(&UninstallProgress),
{
    let mut bytes_freed: u64 = 0;
    let mut items_removed: usize = 0;
    let mut errors: Vec<String> = Vec::new();

    // Collect all paths to delete: associated files first, then the app bundle
    let mut all_paths: Vec<&str> = file_paths.iter().map(|s| s.as_str()).collect();
    all_paths.push(app_path);

    let items_total = all_paths.len();

    for (i, path_str) in all_paths.iter().enumerate() {
        let path = Path::new(path_str);

        // Safety check
        if !is_safe_path(path_str) {
            errors.push(format!("Skipped protected path: {}", path_str));
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        if !path.exists() {
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        let size = if path.is_dir() {
            dir_size(path)
        } else {
            path.metadata().map(|m| m.len()).unwrap_or(0)
        };

        if dry_run {
            bytes_freed += size;
            items_removed += 1;
        } else {
            let result = if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };

            match result {
                Ok(()) => {
                    bytes_freed += size;
                    items_removed += 1;
                    shared::log_operation("UNINSTALL", path_str, "OK");
                }
                Err(e) => {
                    shared::log_operation("UNINSTALL", path_str, &format!("ERROR: {}", e));
                    errors.push(format!("{}: {}", path_str, e));
                }
            }
        }

        on_progress(&UninstallProgress {
            current_item: path_str.to_string(),
            items_done: i + 1,
            items_total,
            bytes_freed,
        });
    }

    UninstallResult {
        items_removed,
        bytes_freed,
        errors,
    }
}

