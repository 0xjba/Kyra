use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::{is_safe_path, uses_pseudo_paths, CleanProgress, CleanResult, ScanItem};
use crate::commands::{data_guard, shared};
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

    // read_dir would follow a symlinked root and empty its target.
    if path.is_symlink() {
        fs::remove_file(path)?;
        return Ok(true);
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

/// Maps a `simctl_unavailable://<UDID>` pseudo-path to the simulator's
/// device directory. The UDID must look like one, since it is joined onto
/// a real path and a value like `../../..` would escape the Devices dir.
fn simctl_device_dir(home: &Path, pseudo_path: &str) -> Option<std::path::PathBuf> {
    let udid = pseudo_path.strip_prefix("simctl_unavailable://")?;
    if udid.is_empty() || !udid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None;
    }
    Some(home.join("Library/Developer/CoreSimulator/Devices").join(udid))
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
        if let Err(refusal) = data_guard::check_general(&path) {
            shared::log_operation("CLEAN", &path.to_string_lossy(), &format!("skipped: {}", refusal));
            continue;
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
    on_progress: F,
) -> CleanResult
where
    F: FnMut(&CleanProgress),
{
    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    execute_clean_items_with_whitelist(items, dry_run, permanent, &settings.whitelist, on_progress)
}

fn execute_clean_items_with_whitelist<F>(
    items: &[ScanItem],
    dry_run: bool,
    permanent: bool,
    whitelist: &[String],
    mut on_progress: F,
) -> CleanResult
where
    F: FnMut(&CleanProgress),
{
    let whitelist_set: HashSet<&str> = whitelist.iter().map(|s| s.as_str()).collect();
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
            if !uses_pseudo_paths(&item.rule_id) {
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

                if let Err(refusal) = data_guard::check_general(Path::new(&path_info.path)) {
                    let reason = format!("skipped: {}", refusal);
                    shared::log_operation("CLEAN", &path_info.path, &reason);
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
                let device_dir = dirs::home_dir()
                    .and_then(|home| simctl_device_dir(&home, &path_info.path));
                match device_dir {
                    Some(device_dir) if device_dir.is_dir() => {
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
                    }
                    Some(_) => {
                        // Device dir already removed by simctl
                        bytes_freed += path_info.size;
                        item_had_success = true;
                    }
                    None if path_info.path.starts_with("simctl_unavailable://") => {
                        errors.push(format!("{}: invalid simulator identifier", path_info.path));
                    }
                    None => {}
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
    ];

    containers.iter().any(|c| path == c.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::browser_guard::fixtures as browser_fixtures;
    use crate::commands::data_guard::fixtures as data_fixtures;
    use crate::commands::cleaner::PathInfo;
    use crate::commands::test_support::{canon, mkdir, s, workspace_tempdir, write_file};
    use std::os::unix::fs::symlink;

    fn item(rule_id: &str, paths: &[(&Path, u64)]) -> ScanItem {
        let paths: Vec<PathInfo> = paths
            .iter()
            .map(|(p, size)| PathInfo {
                path: s(p),
                size: *size,
                is_dir: p.is_dir(),
            })
            .collect();
        ScanItem {
            rule_id: rule_id.into(),
            category: "Test".into(),
            label: rule_id.into(),
            total_size: paths.iter().map(|p| p.size).sum(),
            paths,
        }
    }

    fn raw_item(rule_id: &str, raw_paths: &[&str], is_dir: bool) -> ScanItem {
        ScanItem {
            rule_id: rule_id.into(),
            category: "Test".into(),
            label: rule_id.into(),
            paths: raw_paths
                .iter()
                .map(|p| PathInfo { path: p.to_string(), size: 100, is_dir })
                .collect(),
            total_size: 100 * raw_paths.len() as u64,
        }
    }

    fn run(items: &[ScanItem], dry_run: bool, whitelist: &[String]) -> CleanResult {
        execute_clean_items_with_whitelist(items, dry_run, true, whitelist, |_| {})
    }

    fn rule_items_for_home(home: &Path) -> Vec<ScanItem> {
        use crate::commands::cleaner::{rules, scanner};
        let items = rules::all_rules()
            .into_iter()
            .filter_map(|mut rule| {
                // Absolute rule paths point at the real system; only the
                // home-relative ones can be aimed at the fake home.
                rule.paths.retain(|p| p.starts_with("~/"));
                scanner::collect_rule_paths_in(&rule, &[], Some(home))
            })
            .collect();
        scanner::drop_guard_refused_paths(items)
    }

    fn hostile_items(fx: &browser_fixtures::FakeBrowsers) -> Vec<ScanItem> {
        let paths: Vec<PathInfo> = fx
            .protected
            .keys()
            .chain(fx.protected_dirs.iter())
            .map(|p| PathInfo { path: s(p), size: 1, is_dir: p.is_dir() })
            .collect();
        vec![ScanItem {
            rule_id: "orphan_hostile".into(),
            category: "Test".into(),
            label: "hostile".into(),
            total_size: paths.len() as u64,
            paths,
        }]
    }

    #[test]
    fn every_cleaner_rule_on_a_fake_browser_home_removes_only_cache_leaves() {
        let dir = workspace_tempdir();
        let fx = browser_fixtures::build(&canon(&dir).join("home"));
        let items = rule_items_for_home(&fx.home);

        for item in &items {
            for p in &item.paths {
                let offered = Path::new(&p.path);
                for file in fx.protected.keys() {
                    assert!(!file.starts_with(offered), "rule {} offers {} which holds {}", item.rule_id, p.path, s(file));
                }
            }
        }

        let dry = run(&items, true, &[]);
        assert!(dry.bytes_freed > 0);
        fx.assert_profiles_intact();

        let real = run(&items, false, &[]);
        assert!(real.bytes_freed > 0, "{:?}", real.errors);
        fx.assert_profiles_intact();
        for rel in [
            "Library/Application Support/Google/Chrome/Default/Cache",
            "Library/Application Support/Google/Chrome/Default/Code Cache",
            "Library/Application Support/Google/Chrome/Default/GPUCache",
            "Library/Application Support/Google/Chrome/ShaderCache",
            "Library/Application Support/Google/Chrome/component_crx_cache",
            "Library/Application Support/BraveSoftware/Brave-Browser/GrShaderCache",
            "Library/Application Support/net.imput.helium/extensions_crx_cache",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/cache2",
            "Library/Application Support/Firefox/Profiles/efgh.dev-edition-default/cache2",
            "Library/Caches/Google/Chrome",
            "Library/Caches/com.apple.Safari",
            "Library/Caches/company.thebrowser.Browser",
        ] {
            assert!(!fx.home.join(rel).exists(), "{rel} should have been cleaned");
        }
    }

    #[test]
    fn executor_refuses_every_browser_profile_path_it_is_handed() {
        let dir = workspace_tempdir();
        let fx = browser_fixtures::build(&canon(&dir).join("home"));
        let items = hostile_items(&fx);
        let total = items[0].paths.len();

        let dry = run(&items, true, &[]);
        assert_eq!(dry.bytes_freed, 0);
        let real = run(&items, false, &[]);
        fx.assert_profiles_intact();
        assert_eq!(real.bytes_freed, 0);
        assert_eq!(real.items_cleaned, 0);
        let refused = real.errors.iter().filter(|e| e.contains(crate::commands::browser_guard::ERROR_CODE)).count();
        assert_eq!(refused, total, "{:#?}", real.errors);
    }

    #[test]
    fn every_cleaner_rule_on_a_fake_data_home_removes_only_cache_leaves() {
        let dir = workspace_tempdir();
        let fx = data_fixtures::build(&canon(&dir).join("home"));
        let items = rule_items_for_home(&fx.home);

        for item in &items {
            for p in &item.paths {
                for file in fx.protected.keys() {
                    assert!(!file.starts_with(&p.path), "rule {} offers {} which holds {}", item.rule_id, p.path, s(file));
                }
            }
        }
        run(&items, true, &[]);
        fx.assert_intact();
        let real = run(&items, false, &[]);
        assert!(real.bytes_freed > 0, "{:?}", real.errors);
        fx.assert_intact();
        for rel in [
            "Library/Application Support/Signal/Cache",
            "Library/Application Support/Slack/Cache",
            "Library/Application Support/Slack/Code Cache",
            "Library/Application Support/discord/Cache",
            "Library/Application Support/Microsoft/Teams/Cache",
            "Library/Application Support/Microsoft/Teams/logs",
            "Library/Messages/StickerCache",
            "Library/Messages/Caches/Previews/Attachments",
            "Library/Containers/com.microsoft.Word/Data/tmp",
            "Library/Application Support/Blackmagic Design/DaVinci Resolve/Cache",
            "Library/Application Support/Adobe/Common/Media Cache Files",
            "Library/Application Support/Notion/Cache",
            "Library/Application Support/Steam/appcache",
            "Library/Application Support/Steam/steamapps/shadercache",
            "Library/Application Support/minecraft/logs",
            "Library/Developer/Xcode/UserData/IB Support",
            ".aws/cli/cache",
            ".kube/cache",
            ".docker/buildx/cache",
        ] {
            assert!(!fx.path(rel).exists(), "{rel} should have been cleaned");
        }
    }

    #[test]
    fn executor_refuses_every_protected_path_it_is_handed() {
        let dir = workspace_tempdir();
        let fx = data_fixtures::build(&canon(&dir).join("home"));
        let paths: Vec<PathInfo> = data_fixtures::PROTECTED
            .iter()
            .map(|rel| fx.path(rel))
            .chain(fx.protected_dirs.iter().cloned())
            .map(|p| PathInfo { path: s(&p), size: 1, is_dir: p.is_dir() })
            .collect();
        let total = paths.len();
        let items = vec![ScanItem {
            rule_id: "orphan_hostile".into(),
            category: "Test".into(),
            label: "hostile".into(),
            total_size: total as u64,
            paths,
        }];

        let real = run(&items, false, &[]);
        fx.assert_intact();
        assert_eq!(real.bytes_freed, 0);
        assert_eq!(real.items_cleaned, 0);
        assert_eq!(real.errors.len(), total, "{:#?}", real.errors);
    }

    #[test]
    fn orphan_scan_offers_only_caches_logs_and_window_state_of_gone_apps() {
        use crate::commands::cleaner::scanner;
        use crate::commands::test_support::set_age_days;
        let dir = workspace_tempdir();
        let home = canon(&dir).join("home");
        let bx = browser_fixtures::build(&home);
        let fx = data_fixtures::build(&home);
        let offer = [
            "Library/Caches/com.example.goneapp",
            "Library/Logs/com.example.goneapp",
            "Library/Saved Application State/com.example.goneapp.savedState",
        ];
        let never = [
            "Library/Application Support/com.example.goneapp",
            "Library/Containers/com.example.goneapp",
            "Library/Group Containers/ABCDE12345.com.example.goneapp",
            "Library/Preferences/com.example.goneapp.plist",
            "Library/Caches/com.example.gonewallet",
            "Library/Caches/ru.keepcoder.Telegram",
        ];
        for rel in offer.iter().chain(&never) {
            write_file(&home.join(rel).join("data.bin"), 4096);
        }
        write_file(&home.join("Library/Caches/com.example.gonewallet/keys/wallet.dat"), 4096);
        for rel in offer.iter().chain(&never) {
            set_age_days(&home.join(rel), 400);
        }

        let items = scanner::scan_orphaned_data_in(&home, &HashSet::new(), &[], &|_| false);
        let mut offered: Vec<String> = items.iter().flat_map(|i| i.paths.iter().map(|p| p.path.clone())).collect();
        offered.sort();
        let mut want: Vec<String> = offer.iter().map(|rel| s(&home.join(rel))).collect();
        want.sort();
        assert_eq!(offered, want);

        run(&items, false, &[]);
        bx.assert_profiles_intact();
        fx.assert_intact();
        for rel in never {
            assert!(home.join(rel).join("data.bin").exists(), "{rel}");
        }
    }

    #[test]
    fn permanent_clean_removes_files_and_directories() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let file = root.join("old.log");
        write_file(&file, 1_000);
        let cache = root.join("cache");
        write_file(&cache.join("a/b.bin"), 2_000);

        let result = run(&[item("r1", &[(&file, 1_000), (&cache, 2_000)])], false, &[]);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.bytes_freed, 3_000);
        assert_eq!(result.items_cleaned, 1);
        assert_eq!(result.cleaned_ids, vec!["r1".to_string()]);
        assert!(!file.exists());
        assert!(!cache.exists());
        assert!(root.exists());
    }

    #[test]
    fn dry_run_counts_bytes_but_deletes_nothing() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let file = root.join("x.log");
        write_file(&file, 500);

        let result = run(&[item("r1", &[(&file, 500)])], true, &[]);

        assert_eq!(result.bytes_freed, 500);
        assert_eq!(result.items_cleaned, 1);
        assert!(file.exists());
    }

    #[test]
    fn protected_and_critical_paths_are_refused() {
        let mut paths = vec![
            "/".to_string(),
            "/System/Library/Caches".to_string(),
            "/system/library/caches".to_string(),
            "/usr/lib".to_string(),
            "/private/var/db".to_string(),
            "/Applications".to_string(),
            "/Users".to_string(),
            "/Library/Caches/../../System".to_string(),
        ];
        if let Some(home) = dirs::home_dir() {
            paths.push(s(&home));
            paths.push(s(&home.join("Documents")));
            paths.push(s(&home.join("Library")));
        }
        let refs: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();
        // dry_run keeps this harmless even if a guard regressed.
        let result = run(&[raw_item("r1", &refs, true)], true, &[]);

        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors.len(), paths.len(), "{:?}", result.errors);
        assert!(result.errors.iter().all(|e| e.contains("protected path")));
    }

    #[test]
    fn whitelisted_paths_and_their_children_are_kept() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let keep = root.join("keep");
        write_file(&keep.join("inner/data.bin"), 100);
        let keep_child = keep.join("inner");
        let near_miss = root.join("keeper.bin");
        write_file(&near_miss, 100);

        let whitelist = vec![s(&keep)];
        let result = run(
            &[item("r1", &[(&keep, 100), (&keep_child, 100), (&near_miss, 100)])],
            false,
            &whitelist,
        );

        assert!(keep_child.join("data.bin").exists());
        assert!(!near_miss.exists(), "sibling sharing a name prefix is not whitelisted");
        assert_eq!(result.bytes_freed, 100);
        assert_eq!(
            result.errors.iter().filter(|e| e.contains("whitelist")).count(),
            2
        );
    }

    #[test]
    fn protected_user_data_subdirectories_survive() {
        let dir = workspace_tempdir();
        let profile = canon(&dir).join("Profile");
        write_file(&profile.join("Cache/blob.bin"), 1_000);
        write_file(&profile.join("IndexedDB/db.bin"), 1_000);
        write_file(&profile.join("nested/Local Storage/ls.bin"), 1_000);

        run(&[item("r1", &[(&profile, 3_000)])], false, &[]);

        assert!(!profile.join("Cache").exists());
        assert!(profile.join("IndexedDB/db.bin").exists());
        assert!(profile.join("nested/Local Storage/ls.bin").exists());
    }

    #[test]
    fn protected_component_as_root_is_never_removed() {
        let dir = workspace_tempdir();
        let sw = canon(&dir).join("Service Worker");
        write_file(&sw.join("x.bin"), 10);
        assert_eq!(safe_remove_dir_all(&sw).unwrap(), false);
        assert!(sw.join("x.bin").exists());
    }

    #[test]
    fn symlinks_are_unlinked_never_followed() {
        let outside = workspace_tempdir();
        let target = canon(&outside).join("precious");
        write_file(&target.join("file.txt"), 10);

        let dir = workspace_tempdir();
        let root = canon(&dir);
        let cache = root.join("cache");
        write_file(&cache.join("junk.bin"), 10);
        symlink(&target, cache.join("link_to_precious")).unwrap();
        let top_link = root.join("linked_cache");
        symlink(&target, &top_link).unwrap();

        let result = run(&[item("r1", &[(&cache, 10), (&top_link, 10)])], false, &[]);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(target.join("file.txt").exists());
        assert!(!cache.exists());
        assert!(fs::symlink_metadata(&top_link).is_err());
    }

    #[test]
    fn missing_paths_are_reported_not_counted() {
        let dir = workspace_tempdir();
        let gone = canon(&dir).join("gone.bin");
        let result = run(&[raw_item("r1", &[&s(&gone)], false)], false, &[]);
        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn snapshot_rule_rejects_non_snapshot_paths_without_touching_them() {
        let dir = workspace_tempdir();
        let file = canon(&dir).join("important.txt");
        write_file(&file, 10);
        let p = s(&file);
        let result = run(
            &[raw_item("special_tm_local_snapshots", &[&p, "tmutil://not-a-snapshot"], false)],
            false,
            &[],
        );
        assert!(file.exists());
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn progress_is_reported_from_start_to_finish() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let (a, b) = (root.join("a"), root.join("b"));
        write_file(&a, 10);
        write_file(&b, 10);
        let mut events = Vec::new();
        execute_clean_items_with_whitelist(
            &[item("r1", &[(&a, 10)]), item("r2", &[(&b, 10)])],
            false,
            true,
            &[],
            |p| events.push((p.paths_done, p.paths_total, p.bytes_freed)),
        );
        assert_eq!(events.first(), Some(&(0, 2, 0)));
        assert_eq!(events.last(), Some(&(2, 2, 20)));
    }

    #[test]
    fn container_dirs_are_emptied_in_place() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        write_file(&root.join("a.bin"), 100);
        write_file(&root.join("sub/b.bin"), 100);
        write_file(&root.join("IndexedDB/keep.bin"), 100);
        let outside = workspace_tempdir();
        write_file(&canon(&outside).join("keep.txt"), 1);
        symlink(canon(&outside), root.join("link")).unwrap();

        let (freed, errs) = delete_dir_contents(&root, true);

        assert!(errs.is_empty(), "{errs:?}");
        assert!(freed > 0);
        assert!(root.exists());
        assert!(!root.join("a.bin").exists());
        assert!(!root.join("sub").exists());
        assert!(root.join("IndexedDB/keep.bin").exists());
        assert!(canon(&outside).join("keep.txt").exists());
    }

    #[test]
    fn container_dir_detection_is_exact() {
        assert!(is_container_dir("/Library/Caches"));
        assert!(is_container_dir("/Library/Logs"));
        assert!(!is_container_dir("/private/var/log"));
        assert!(!is_container_dir("/Library/Caches/com.foo"));
        assert!(!is_container_dir("/Library"));
        if let Some(home) = dirs::home_dir() {
            assert!(is_container_dir(&s(&home.join("Library/Caches"))));
            assert!(is_container_dir(&s(&home.join("Library/Logs"))));
            assert!(!is_container_dir(&s(&home.join("Library"))));
        }
    }

    #[test]
    fn whitelist_matching_is_component_aware() {
        let wl: HashSet<&str> = ["/a/b"].into_iter().collect();
        assert!(is_whitelisted("/a/b", &wl));
        assert!(is_whitelisted("/a/b/c", &wl));
        assert!(!is_whitelisted("/a/bc", &wl));
        assert!(!is_whitelisted("/a", &wl));
    }

    #[test]
    fn snapshot_dates_are_extracted_strictly() {
        assert_eq!(
            extract_snapshot_date("com.apple.TimeMachine.2025-11-02-120000.local").as_deref(),
            Some("2025-11-02-120000")
        );
        assert_eq!(extract_snapshot_date("com.apple.TimeMachine..local"), None);
        assert_eq!(extract_snapshot_date("com.apple.TimeMachine.2025-11-02"), None);
        assert_eq!(extract_snapshot_date("/Users/x/Documents"), None);
    }

    #[test]
    fn invalid_snapshot_pseudo_paths_fail_before_running_tmutil() {
        assert!(tmutil_delete_local_snapshot("/Users/x/Documents").is_err());
        assert!(tmutil_delete_local_snapshot("tmutil://garbage").is_err());
    }

    #[test]
    fn simulator_udids_cannot_escape_the_devices_dir() {
        let home = Path::new("/Users/tester");
        assert_eq!(
            simctl_device_dir(home, "simctl_unavailable://A1B2C3D4-0000-1111-2222-333344445555"),
            Some(home.join("Library/Developer/CoreSimulator/Devices/A1B2C3D4-0000-1111-2222-333344445555"))
        );
        for bad in [
            "simctl_unavailable://../../../../Documents",
            "simctl_unavailable://",
            "simctl_unavailable://abc/def",
            "simctl_unavailable:///etc",
            "/Users/tester/Library/Developer/CoreSimulator/Devices/ABC",
        ] {
            assert_eq!(simctl_device_dir(home, bad), None, "{bad}");
        }
    }

    #[test]
    fn delete_errors_get_actionable_hints() {
        let e = std::io::Error::from_raw_os_error(1);
        assert!(classify_delete_error(&e).contains("SIP"));
        let e = std::io::Error::from_raw_os_error(16);
        assert!(classify_delete_error(&e).contains("in use"));
        let e = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        assert!(classify_delete_error(&e).contains("already removed"));
        let e = std::io::Error::new(std::io::ErrorKind::Other, "weird");
        assert_eq!(classify_delete_error(&e), "weird");
    }

    #[test]
    fn empty_directory_tree_is_removed() {
        let dir = workspace_tempdir();
        let tree = canon(&dir).join("t");
        mkdir(&tree.join("a/b/c"));
        assert_eq!(safe_remove_dir_all(&tree).unwrap(), true);
        assert!(!tree.exists());
    }
}
