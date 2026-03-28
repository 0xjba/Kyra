use std::fs;
use std::path::Path;

use super::{is_safe_path, CleanProgress, CleanResult, ScanItem};
use crate::commands::shared;

/// Deletes all paths for the given scan items.
/// Calls `on_progress` after each item is processed.
/// If `dry_run` is true, reports what would be deleted without actually deleting.
pub fn execute_clean_items<F>(
    items: &[ScanItem],
    dry_run: bool,
    mut on_progress: F,
) -> CleanResult
where
    F: FnMut(&CleanProgress),
{
    let mut bytes_freed: u64 = 0;
    let mut items_cleaned: usize = 0;
    let mut errors: Vec<String> = Vec::new();
    let items_total = items.len();

    for (i, item) in items.iter().enumerate() {
        for path_info in &item.paths {
            if !is_safe_path(&path_info.path) {
                errors.push(format!("Skipped protected path: {}", path_info.path));
                continue;
            }

            if dry_run {
                bytes_freed += path_info.size;
                continue;
            }

            let path = Path::new(&path_info.path);
            let result = if path_info.is_dir {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };

            match result {
                Ok(()) => {
                    bytes_freed += path_info.size;
                    shared::log_operation("CLEAN", &path_info.path, "OK");
                }
                Err(e) => {
                    shared::log_operation("CLEAN", &path_info.path, &format!("ERROR: {}", e));
                    errors.push(format!("{}: {}", path_info.path, e));
                }
            }
        }

        items_cleaned = i + 1;

        on_progress(&CleanProgress {
            current_item: item.label.clone(),
            items_done: items_cleaned,
            items_total,
            bytes_freed,
        });
    }

    CleanResult {
        items_cleaned,
        bytes_freed,
        errors,
    }
}
