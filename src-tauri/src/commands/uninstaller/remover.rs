use std::fs;
use std::path::Path;

use super::{UninstallProgress, UninstallResult};

/// Paths that must never be deleted.
/// Note: `/Applications` is intentionally excluded so that individual `.app`
/// bundles inside it (e.g. `/Applications/SomeApp.app`) can be deleted.
/// Only truly dangerous system roots are protected here.
const PROTECTED_PATHS: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/etc",
    "/var/db",
    "/Library/Frameworks",
];

/// Returns true if a path is safe to delete.
fn is_safe_path(path: &str) -> bool {
    for protected in PROTECTED_PATHS {
        if path == *protected || path.starts_with(&format!("{}/", protected)) {
            return false;
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
) -> Result<UninstallResult, String>
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
                }
                Err(e) => {
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

    Ok(UninstallResult {
        items_removed,
        bytes_freed,
        errors,
    })
}

/// Recursively calculates directory size.
fn dir_size(path: &Path) -> u64 {
    if path.is_symlink() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_symlink() {
                0
            } else if p.is_dir() {
                dir_size(&p)
            } else {
                p.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}
