use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::{is_safe_path, CleanProgress, CleanResult, ScanItem};
use crate::commands::shared;
use crate::commands::utils::{dir_size, is_protected_user_data_component};

/// Recursively delete a directory tree while preserving any subdirectory
/// whose name is a protected user-data component (Service Worker,
/// IndexedDB, Local Storage, …). If any protected subdirs are
/// preserved, the root directory itself is left in place; otherwise
/// the root is removed. Returns `Ok(true)` if the root was removed,
/// `Ok(false)` if protected content kept it alive.
fn safe_remove_dir_all(path: &Path) -> std::io::Result<bool> {
    // If the root itself is a protected component, refuse outright.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if is_protected_user_data_component(name) {
            return Ok(false);
        }
    }

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => return Err(e),
    };

    let mut preserved_any = false;
    for entry in entries.flatten() {
        let child = entry.path();

        if child.is_symlink() {
            // Remove the symlink itself, never follow.
            let _ = fs::remove_file(&child);
            continue;
        }

        if child.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_protected_user_data_component(&name) {
                preserved_any = true;
                continue;
            }
            match safe_remove_dir_all(&child)? {
                true => {} // child fully removed
                false => preserved_any = true,
            }
        } else {
            fs::remove_file(&child)?;
        }
    }

    if preserved_any {
        Ok(false)
    } else {
        fs::remove_dir(path)?;
        Ok(true)
    }
}

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

/// Translate an `std::io::Error` from a delete operation into a short
/// human-readable diagnostic. macOS returns the same `PermissionDenied`
/// kind for SIP protection, per-file immutable flags, root ownership,
/// and access-control list restrictions, so the raw message ("Operation
/// not permitted") is rarely actionable. Where possible we map raw OS
/// error codes to a specific hint the user can act on.
fn classify_delete_error(err: &std::io::Error) -> String {
    use std::io::ErrorKind;

    let base = err.to_string();
    let hint: Option<&'static str> = match err.raw_os_error() {
        Some(1) => Some("operation not permitted — path may be SIP-protected, immutable (chflags uchg/schg), or owned by root"),
        Some(13) => Some("access denied — check the file's ACL and your user's write permission on the parent directory"),
        Some(16) => Some("file in use by a running process — quit the owning app and retry"),
        Some(30) => Some("read-only filesystem"),
        Some(66) => Some("directory not empty — a protected user-data subdir (Service Worker / IndexedDB / …) was preserved"),
        Some(35) => Some("resource temporarily unavailable — another process holds a lock"),
        _ => None,
    };

    if let Some(hint) = hint {
        return format!("{} ({})", base, hint);
    }

    match err.kind() {
        ErrorKind::PermissionDenied => format!(
            "{} (permission denied — may be SIP-protected or owned by root)",
            base
        ),
        ErrorKind::NotFound => format!("{} (already removed)", base),
        ErrorKind::ReadOnlyFilesystem => format!("{} (read-only filesystem)", base),
        _ => base,
    }
}

/// Extract the `YYYY-MM-DD-HHMMSS` date portion from a local snapshot
/// identifier like `com.apple.TimeMachine.2025-11-02-120000.local`.
/// Returns `None` if the input doesn't match the expected shape.
fn extract_snapshot_date(full_name: &str) -> Option<String> {
    let after_prefix = full_name.strip_prefix("com.apple.TimeMachine.")?;
    let without_suffix = after_prefix.strip_suffix(".local")?;
    if without_suffix.is_empty() {
        return None;
    }
    Some(without_suffix.to_string())
}

/// Delete a single APFS local Time Machine snapshot via
/// `tmutil deletelocalsnapshots <date>`. The scanner encodes snapshots
/// as `tmutil://<full-identifier>`; this helper decodes the scheme,
/// extracts the date portion, and issues the tmutil call.
fn tmutil_delete_local_snapshot(pseudo_path: &str) -> Result<(), String> {
    let identifier = pseudo_path
        .strip_prefix("tmutil://")
        .ok_or_else(|| "invalid snapshot identifier".to_string())?;
    let date = extract_snapshot_date(identifier)
        .ok_or_else(|| format!("unrecognised snapshot identifier: {}", identifier))?;

    let output = Command::new("/usr/bin/tmutil")
        .arg("deletelocalsnapshots")
        .arg(&date)
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

        // Defense-in-depth: never clear a protected user-data component
        // even if it somehow ends up at the top level of a container dir.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if is_protected_user_data_component(name) {
                continue;
            }
        }

        let size = if path.is_dir() {
            dir_size(&path)
        } else {
            path.metadata().map(|m| m.len()).unwrap_or(0)
        };

        let result = if permanent {
            if path.is_dir() {
                safe_remove_dir_all(&path).map(|_| ())
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
                errs.push(format!("{}: {}", path.display(), classify_delete_error(&e)));
            }
        }
    }

    (freed, errs)
}

/// Deletes all paths for the given scan items.
/// Calls `on_progress` after each path is processed for smooth UI updates.
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
    let paths_total: usize = items.iter().map(|it| it.paths.len()).sum();
    let mut paths_done: usize = 0;

    // Emit initial progress so the UI immediately shows "Starting..." instead
    // of being stuck at null until the first deletion completes.
    on_progress(&CleanProgress {
        current_item: items.first().map(|it| it.label.clone()).unwrap_or_default(),
        items_done: 0,
        items_total,
        paths_done: 0,
        paths_total,
        bytes_freed: 0,
    });

    for (i, item) in items.iter().enumerate() {
        let mut item_had_success = false;

        // Time Machine failed backups must be deleted through tmutil so the
        // TM catalogue stays consistent — we never touch .inProgress dirs
        // with filesystem calls.
        let is_tm_failed_rule = item.rule_id == "special_tm_failed_backups";
        // APFS local snapshots are removed via tmutil deletelocalsnapshots.
        // Paths for this rule are pseudo-URIs of the form `tmutil://<id>`.
        let is_tm_snapshot_rule = item.rule_id == "special_tm_local_snapshots";
        // Unavailable Xcode simulators are cleaned via `xcrun simctl delete unavailable`
        // with fallback to manual directory deletion. Paths are pseudo-URIs
        // of the form `simctl_unavailable://<UDID>`.
        let is_simctl_unavail_rule = item.rule_id == "dev_xcode_unavailable_sims";

        // For the unavailable simulators rule, run the bulk command once
        // rather than per-path. Track whether we already ran it.
        let mut simctl_bulk_ran = false;

        for path_info in &item.paths {
            // Skip safe-path / whitelist checks for pseudo-URIs
            // because they are not real filesystem paths.
            if !is_tm_snapshot_rule && !is_simctl_unavail_rule {
                if !is_safe_path(&path_info.path) {
                    let reason = "skipped: protected path (SIP / system directory)";
                    shared::log_operation("CLEAN", &path_info.path, reason);
                    errors.push(format!("{}: {}", path_info.path, reason));
                    continue;
                }

                if is_whitelisted(&path_info.path, &whitelist_set) {
                    let reason = "skipped: on user whitelist";
                    shared::log_operation("CLEAN", &path_info.path, reason);
                    errors.push(format!("{}: {}", path_info.path, reason));
                    continue;
                }
            }

            if dry_run {
                bytes_freed += path_info.size;
                item_had_success = true;
                paths_done += 1;
                on_progress(&CleanProgress {
                    current_item: item.label.clone(),
                    items_done: i,
                    items_total,
                    paths_done,
                    paths_total,
                    bytes_freed,
                });
            } else if is_tm_failed_rule {
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
                paths_done += 1;
                on_progress(&CleanProgress {
                    current_item: item.label.clone(),
                    items_done: i,
                    items_total,
                    paths_done,
                    paths_total,
                    bytes_freed,
                });
            } else if is_tm_snapshot_rule {
                match tmutil_delete_local_snapshot(&path_info.path) {
                    Ok(()) => {
                        bytes_freed += path_info.size;
                        item_had_success = true;
                        shared::log_operation(
                            "CLEAN",
                            &path_info.path,
                            "tmutil deletelocalsnapshots",
                        );
                    }
                    Err(e) => {
                        shared::log_operation(
                            "CLEAN",
                            &path_info.path,
                            &format!("tmutil deletelocalsnapshots failed: {}", e),
                        );
                        errors.push(format!("{}: {}", path_info.path, e));
                    }
                }
                paths_done += 1;
                on_progress(&CleanProgress {
                    current_item: item.label.clone(),
                    items_done: i,
                    items_total,
                    paths_done,
                    paths_total,
                    bytes_freed,
                });
            } else if is_simctl_unavail_rule {
                // Run `xcrun simctl delete unavailable` once for the
                // whole batch, then fall back to manual dir deletion
                // for any remaining orphaned device directories.
                if !simctl_bulk_ran {
                    simctl_bulk_ran = true;
                    let simctl_ok = Command::new("/usr/bin/xcrun")
                        .args(["simctl", "delete", "unavailable"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if simctl_ok {
                        shared::log_operation(
                            "CLEAN",
                            "xcrun simctl delete unavailable",
                            "success",
                        );
                    } else {
                        shared::log_operation(
                            "CLEAN",
                            "xcrun simctl delete unavailable",
                            "failed, falling back to manual deletion",
                        );
                    }
                }
                // Try manual deletion of the device directory as fallback
                if let Some(udid) = path_info.path.strip_prefix("simctl_unavailable://") {
                    if let Some(home) = dirs::home_dir() {
                        let device_dir = home.join(format!(
                            "Library/Developer/CoreSimulator/Devices/{}",
                            udid
                        ));
                        if device_dir.is_dir() {
                            match safe_remove_dir_all(&device_dir) {
                                Ok(_) => {
                                    bytes_freed += path_info.size;
                                    item_had_success = true;
                                    shared::log_operation(
                                        "CLEAN",
                                        &path_info.path,
                                        "manual device dir removal",
                                    );
                                }
                                Err(e) => {
                                    shared::log_operation(
                                        "CLEAN",
                                        &path_info.path,
                                        &format!("manual removal failed: {}", e),
                                    );
                                    errors.push(format!("{}: {}", path_info.path, e));
                                }
                            }
                        } else {
                            // Device dir already removed by simctl
                            bytes_freed += path_info.size;
                            item_had_success = true;
                        }
                    }
                }
                paths_done += 1;
                on_progress(&CleanProgress {
                    current_item: item.label.clone(),
                    items_done: i,
                    items_total,
                    paths_done,
                    paths_total,
                    bytes_freed,
                });
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
                            safe_remove_dir_all(path).map(|_| ())
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
                            let diagnosis = classify_delete_error(&e);
                            shared::log_operation(
                                "CLEAN",
                                &path_info.path,
                                &format!("ERROR: {}", diagnosis),
                            );
                            errors.push(format!("{}: {}", path_info.path, diagnosis));
                        }
                    }
                }
                paths_done += 1;
                on_progress(&CleanProgress {
                    current_item: item.label.clone(),
                    items_done: i,
                    items_total,
                    paths_done,
                    paths_total,
                    bytes_freed,
                });
            }
        }

        if item_had_success {
            items_cleaned += 1;
            cleaned_ids.push(item.rule_id.clone());
        }
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
