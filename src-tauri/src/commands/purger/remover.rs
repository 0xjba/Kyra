use std::fs;
use std::path::Path;

use super::{PurgeProgress, PurgeResult};
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

/// Known artifact directory names that are valid purge targets.
const VALID_ARTIFACT_NAMES: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", ".nuxt",
    "__pycache__", ".pytest_cache", "Pods", ".gradle", ".build",
];

/// Returns true if a path is safe to delete for purge operations.
/// Canonicalizes the path to prevent traversal attacks.
fn is_safe_path(path_str: &str) -> bool {
    // Canonicalize to resolve any .. or symlinks
    let canonical = match fs::canonicalize(path_str) {
        Ok(p) => p,
        Err(_) => return false, // Can't resolve = don't delete
    };
    let path = canonical.to_string_lossy();

    for protected in PROTECTED_PATHS {
        if path.as_ref() == *protected || path.starts_with(&format!("{}/", protected)) {
            return false;
        }
    }

    // Block home directory itself
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path.as_ref() == home_str.as_ref() {
            return false;
        }
    }

    // Verify the directory name is a known artifact type
    if let Some(name) = canonical.file_name().and_then(|n| n.to_str()) {
        if !VALID_ARTIFACT_NAMES.contains(&name)
            && !name.ends_with(".egg-info")
        {
            return false;
        }
    } else {
        return false;
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
                    shared::log_operation("PURGE", path_str, "OK");
                }
                Err(e) => {
                    shared::log_operation("PURGE", path_str, &format!("ERROR: {}", e));
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

