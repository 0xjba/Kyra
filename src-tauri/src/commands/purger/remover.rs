use std::fs;
use std::path::Path;

use super::{PurgeProgress, PurgeResult};

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

/// Returns true if a path is safe to delete for purge operations.
fn is_safe_path(path: &str) -> bool {
    for protected in PROTECTED_PATHS {
        if path == *protected || path.starts_with(&format!("{}/", protected)) {
            return false;
        }
    }

    // Block home directory itself
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path == home_str.as_ref() {
            return false;
        }
    }

    true
}

/// Removes selected artifact directories.
pub fn remove_artifacts<F>(
    paths: &[String],
    dry_run: bool,
    mut on_progress: F,
) -> PurgeResult
where
    F: FnMut(&PurgeProgress),
{
    let mut bytes_freed: u64 = 0;
    let mut items_removed: usize = 0;
    let mut errors: Vec<String> = Vec::new();
    let items_total = paths.len();

    for (i, path_str) in paths.iter().enumerate() {
        let path = Path::new(path_str);

        if !is_safe_path(path_str) {
            errors.push(format!("Skipped protected path: {}", path_str));
            on_progress(&PurgeProgress {
                current_item: path_str.clone(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        if !path.exists() {
            on_progress(&PurgeProgress {
                current_item: path_str.clone(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        let size = dir_size(path);

        if dry_run {
            bytes_freed += size;
            items_removed += 1;
        } else {
            match fs::remove_dir_all(path) {
                Ok(()) => {
                    bytes_freed += size;
                    items_removed += 1;
                }
                Err(e) => {
                    errors.push(format!("{}: {}", path_str, e));
                }
            }
        }

        on_progress(&PurgeProgress {
            current_item: path_str.clone(),
            items_done: i + 1,
            items_total,
            bytes_freed,
        });
    }

    PurgeResult {
        items_removed,
        bytes_freed,
        errors,
    }
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
