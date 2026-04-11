use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::{is_safe_path, CleanProgress, CleanResult, ScanItem};
use crate::commands::shared;
use crate::commands::utils::dir_size;

/// Delete a Time Machine in-progress backup bundle via `tmutil delete`,
/// which walks the TM catalogue so the backup database stays consistent.
/// Plain `rm -rf` leaves orphaned index entries and can corrupt subsequent
/// incremental backups, so we never fall back to filesystem deletion for
/// these paths.
fn tmutil_delete(path: &str) -> Result<(), String> {
    let output = Command::new("/usr/bin/tmutil")
        .arg("delete")
        .arg(path)
        .output()
        .map_err(|e| format!("tmutil spawn failed: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

/// Returns true if `path` (or any parent) is covered by a whitelisted entry.
fn is_whitelisted(path: &str, whitelist: &HashSet<&str>) -> bool {
    // O(1) exact match
    if whitelist.contains(path) {
        return true;
    }
    // Check if any whitelisted entry is a parent directory of path
    whitelist.iter().any(|w| path.starts_with(&format!("{}/", w)))
}

/// Delete the contents of a directory without removing the directory itself.
/// Returns (bytes_freed, errors).
fn delete_dir_contents(dir: &Path, permanent: bool) -> (u64, Vec<String>) {
    let mut freed: u64 = 0;
    let mut errs: Vec<String> = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errs.push(format!("{}: {}", dir.display(), e));
            return (freed, errs);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }

        let size = if path.is_dir() {
            dir_size(&path)
        } else {
            path.metadata().map(|m| m.len()).unwrap_or(0)
        };

        let result = if permanent {
            if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            }
        } else {
            trash::delete(&path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        };

        match result {
            Ok(()) => {
                freed += size;
                let action = if permanent { "DELETED" } else { "TRASHED" };
                shared::log_operation("CLEAN", &path.to_string_lossy(), action);
            }
            Err(e) => {
                errs.push(format!("{}: {}", path.display(), e));
            }
        }
    }

    (freed, errs)
}

/// Deletes all paths for the given scan items.
/// Calls `on_progress` after each item is processed.
/// If `dry_run` is true, reports what would be deleted without actually deleting.
pub fn execute_clean_items<F>(
    items: &[ScanItem],
    dry_run: bool,
    permanent: bool,
    mut on_progress: F,
) -> CleanResult
where
    F: FnMut(&CleanProgress),
{
    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let whitelist_set: HashSet<&str> = settings.whitelist.iter().map(|s| s.as_str()).collect();
    let mut bytes_freed: u64 = 0;
    let mut items_cleaned: usize = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut cleaned_ids: Vec<String> = Vec::new();
    let items_total = items.len();

    for (i, item) in items.iter().enumerate() {
        let mut item_had_success = false;

        // Time Machine failed backups must be deleted through tmutil so the
        // TM catalogue stays consistent — we never touch .inProgress dirs
        // with filesystem calls.
        let is_tm_rule = item.rule_id == "special_tm_failed_backups";

        for path_info in &item.paths {
            if !is_safe_path(&path_info.path) {
                errors.push(format!("Skipped protected path: {}", path_info.path));
                continue;
            }

            if is_whitelisted(&path_info.path, &whitelist_set) {
                errors.push(format!("Skipped whitelisted path: {}", path_info.path));
                continue;
            }

            if dry_run {
                bytes_freed += path_info.size;
                item_had_success = true;
            } else if is_tm_rule {
                match tmutil_delete(&path_info.path) {
                    Ok(()) => {
                        bytes_freed += path_info.size;
                        item_had_success = true;
                        shared::log_operation("CLEAN", &path_info.path, "tmutil delete");
                    }
                    Err(e) => {
                        shared::log_operation(
                            "CLEAN",
                            &path_info.path,
                            &format!("tmutil delete failed: {}", e),
                        );
                        errors.push(format!("{}: {}", path_info.path, e));
                    }
                }
            } else {
                let path = Path::new(&path_info.path);

                // For directories that are top-level containers (e.g. ~/Library/Caches),
                // delete contents instead of the directory itself to avoid permission errors
                // from macOS locking the parent directory.
                if path_info.is_dir && is_container_dir(&path_info.path) {
                    let (freed, errs) = delete_dir_contents(path, permanent);
                    if freed > 0 {
                        bytes_freed += freed;
                        item_had_success = true;
                    }
                    errors.extend(errs);
                } else {
                    let delete_result = if permanent {
                        if path_info.is_dir {
                            fs::remove_dir_all(path)
                        } else {
                            fs::remove_file(path)
                        }
                    } else {
                        trash::delete(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                    };
                    match delete_result {
                        Ok(()) => {
                            bytes_freed += path_info.size;
                            item_had_success = true;
                            let action = if permanent { "DELETED" } else { "TRASHED" };
                            shared::log_operation("CLEAN", &path_info.path, action);
                        }
                        Err(e) => {
                            shared::log_operation("CLEAN", &path_info.path, &format!("ERROR: {}", e));
                            errors.push(format!("{}: {}", path_info.path, e));
                        }
                    }
                }
            }
        }

        if item_had_success {
            items_cleaned += 1;
            cleaned_ids.push(item.rule_id.clone());
        }

        on_progress(&CleanProgress {
            current_item: item.label.clone(),
            items_done: i + 1,
            items_total,
            bytes_freed,
        });
    }

    CleanResult {
        items_cleaned,
        bytes_freed,
        errors,
        cleaned_ids,
    }
}

/// Returns true if the path is a well-known container directory whose contents
/// should be deleted rather than the directory itself (macOS recreates these).
fn is_container_dir(path: &str) -> bool {
    let home = match dirs::home_dir() {
        Some(h) => h.to_string_lossy().to_string(),
        None => return false,
    };

    let containers = [
        format!("{}/Library/Caches", home),
        format!("{}/Library/Logs", home),
        "/Library/Caches".to_string(),
        "/Library/Logs".to_string(),
        "/private/var/log".to_string(),
    ];

    containers.iter().any(|c| path == c.as_str())
}
