use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{is_safe_path, uses_pseudo_paths, CleanRule, PathInfo, ScanItem};
use crate::commands::utils::{deletable_dir_size, dir_size};

// ── Special Scan Functions ───────────────────────────────────────────

/// Recursively find .DS_Store files under `dir`, respecting skip list and max count.
fn walk_ds_store(
    dir: &Path,
    paths: &mut Vec<PathInfo>,
    total: &mut u64,
    count: &mut usize,
    max: usize,
    skip: &[&str],
) {
    if *count >= max {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if *count >= max {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip symlinks
        if path.is_symlink() {
            continue;
        }

        if name == ".DS_Store" {
            if let Ok(meta) = path.metadata() {
                let size = meta.len();
                paths.push(PathInfo {
                    path: path.to_string_lossy().to_string(),
                    size,
                    is_dir: false,
                });
                *total += size;
                *count += 1;
            }
        } else if path.is_dir() && !skip.iter().any(|s| name == *s) {
            walk_ds_store(&path, paths, total, count, max, skip);
        }
    }
}

/// Special scan: find .DS_Store files throughout the home directory.
/// The skip list excludes directories where walking is expensive
/// (package caches, build output, SCM history, VM images) or where
/// .DS_Store entries are not user-visible and therefore not worth
/// reporting. Raised to 2000 files because large multi-repo setups
/// routinely exceed the old 500-file cap and users expect a single
/// sweep to reach the entire home tree.
fn scan_ds_store_files() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let mut paths = Vec::new();
    let mut total_size = 0u64;
    let mut count = 0usize;
    let max_files = 2000;

    let skip_dirs: &[&str] = &[
        // Trash and SCM history — large, noisy, not worth touching
        ".Trash",
        ".git",
        ".hg",
        ".svn",
        // Package managers and language-specific caches
        "node_modules",
        "bower_components",
        ".npm",
        ".yarn",
        ".pnpm-store",
        ".cargo",
        ".rustup",
        ".gradle",
        ".m2",
        ".ivy2",
        "vendor",
        // Build output directories
        "target",
        "build",
        "dist",
        ".next",
        ".nuxt",
        ".turbo",
        // macOS Library internals — already cleaned by other rules
        "Library/Caches",
        "Library/Developer",
        "Library/Containers",
        "Library/Group Containers",
        // APFS filesystem metadata
        ".Spotlight-V100",
        ".DocumentRevisions-V100",
        ".fseventsd",
        ".TemporaryItems",
    ];

    walk_ds_store(&home, &mut paths, &mut total_size, &mut count, max_files, skip_dirs);

    if paths.is_empty() {
        return None;
    }

    let label = format!(".DS_Store Files ({})", count);
    crate::commands::shared::log_operation(
        "SCAN",
        &label,
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "maint_ds_store".into(),
        category: "Maintenance".into(),
        label,
        paths,
        total_size,
    })
}

/// Minimum age (in hours) before an incomplete Time Machine backup is
/// considered cleanup-eligible. A three-day window keeps legitimate
/// retry/resume scenarios out of the scan results.
const TM_INPROGRESS_MIN_AGE_HOURS: u64 = 72;

/// Returns true if Time Machine is configured on this machine (the
/// `com.apple.TimeMachine` domain exists and has an `AutoBackup` key).
/// Used to short-circuit the scan early on systems without TM.
fn tm_is_configured() -> bool {
    let out = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "/Library/Preferences/com.apple.TimeMachine", "AutoBackup"])
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Returns true if a Time Machine backup is currently running via
/// `tmutil status`. Skipping during active backups avoids racing with
/// backupd mid-write.
fn tm_is_running() -> bool {
    let out = std::process::Command::new("/usr/bin/tmutil").arg("status").output();
    match out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains("Running = 1")
        }
        _ => false,
    }
}

/// Special scan: find abandoned Time Machine in-progress backup bundles
/// older than `TM_INPROGRESS_MIN_AGE_HOURS`. Walks the top-level
/// `/Volumes` looking for TM target volumes (identified by a
/// `Backups.backupdb` root) and collects `*.inProgress` /
/// `*.inprogress` directories at a bounded depth.
///
/// Gates applied, in order:
/// - TM must be configured on this machine
/// - TM must not currently be running (active backup)
/// - Each found in-progress dir must be older than the safety window
///
/// The scanner only *reports* these paths; the executor runs the actual
/// deletion through `tmutil delete` so the TM catalogue is kept
/// consistent.
fn scan_tm_failed_backups() -> Option<ScanItem> {
    if !tm_is_configured() || tm_is_running() {
        return None;
    }

    let volumes_root = Path::new("/Volumes");
    if !volumes_root.is_dir() {
        return None;
    }

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(TM_INPROGRESS_MIN_AGE_HOURS * 3600))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    fn walk_inprogress(
        dir: &Path,
        depth: usize,
        cutoff: SystemTime,
        paths: &mut Vec<PathInfo>,
        total: &mut u64,
    ) {
        if depth == 0 {
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if lower.ends_with(".inprogress") {
                if let Ok(meta) = path.metadata() {
                    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    if modified < cutoff {
                        let size = deletable_dir_size(&path);
                        if size > 0 {
                            paths.push(PathInfo {
                                path: path.to_string_lossy().to_string(),
                                size,
                                is_dir: true,
                            });
                            *total += size;
                        }
                    }
                }
                continue; // don't descend into the in-progress dir itself
            }
            walk_inprogress(&path, depth - 1, cutoff, paths, total);
        }
    }

    let volumes = match std::fs::read_dir(volumes_root) {
        Ok(v) => v,
        Err(_) => return None,
    };
    for vol_entry in volumes.flatten() {
        let vol_path = vol_entry.path();
        if !vol_path.is_dir() || vol_path.is_symlink() {
            continue;
        }
        let backupdb = vol_path.join("Backups.backupdb");
        if !backupdb.is_dir() {
            continue;
        }
        walk_inprogress(&backupdb, 3, cutoff, &mut paths, &mut total_size);
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Time Machine Failed Backups",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "special_tm_failed_backups".into(),
        category: "Maintenance".into(),
        label: "Time Machine Failed Backups".into(),
        paths,
        total_size,
    })
}

/// Number of Xcode DeviceSupport versions to preserve per platform.
/// Symbol bundles for real-device debugging can be ~2 GB each and
/// redownload automatically when you attach a device; keeping the
/// most recent few avoids blocking debugging of active hardware.
const XCODE_DEVICE_SUPPORT_KEEP: usize = 3;

/// Special scan: enumerate `~/Library/Developer/Xcode/*DeviceSupport/*`
/// subdirectories, sort by modified time descending, and flag all but
/// the `XCODE_DEVICE_SUPPORT_KEEP` most recent for deletion.
fn scan_xcode_device_support() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let roots = [
        home.join("Library/Developer/Xcode/iOS DeviceSupport"),
        home.join("Library/Developer/Xcode/watchOS DeviceSupport"),
        home.join("Library/Developer/Xcode/tvOS DeviceSupport"),
        home.join("Library/Developer/Xcode/visionOS DeviceSupport"),
    ];

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for root in &roots {
        if !root.is_dir() {
            continue;
        }

        // Collect (path, modified_time) for each version subdir.
        let mut versions: Vec<(PathBuf, SystemTime)> = Vec::new();
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() || p.is_symlink() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            versions.push((p, modified));
        }

        // Sort newest first so the top N are preserved.
        versions.sort_by(|a, b| b.1.cmp(&a.1));

        for (path, _) in versions.into_iter().skip(XCODE_DEVICE_SUPPORT_KEEP) {
            let size = deletable_dir_size(&path);
            if size == 0 {
                continue;
            }
            paths.push(PathInfo {
                path: path.to_string_lossy().to_string(),
                size,
                is_dir: true,
            });
            total_size += size;
        }
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Xcode Device Support (old versions)",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "dev_xcode_device_support".into(),
        category: "Developer Tools".into(),
        label: "Xcode Device Support (old versions)".into(),
        paths,
        total_size,
    })
}

/// Special scan: report APFS local Time Machine snapshots. macOS keeps
/// these automatically and they auto-expire after ~24 hours, but users
/// who are short on space can reclaim them manually. Since APFS is
/// copy-on-write, the actual reclaimable bytes per snapshot are not
/// cheaply computable — we surface the snapshots with a nominal
/// placeholder size so the UI has something to display, and the
/// executor shells out to `tmutil deletelocalsnapshots` per entry.
fn scan_tm_local_snapshots() -> Option<ScanItem> {
    // Skip entirely if Time Machine isn't configured on this host.
    if !tm_is_configured() {
        return None;
    }

    let out = std::process::Command::new("/usr/bin/tmutil")
        .args(["listlocalsnapshots", "/"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    // Expected line format: `com.apple.TimeMachine.2025-11-02-120000.local`
    // We preserve the full identifier in the path so the executor can
    // extract the date portion unambiguously.
    let snapshot_names: Vec<String> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("com.apple.TimeMachine."))
        .map(|l| l.to_string())
        .collect();

    if snapshot_names.is_empty() {
        return None;
    }

    // Nominal per-snapshot size so the UI shows a non-zero value. A
    // typical APFS local snapshot reclaims tens to hundreds of MB, but
    // the actual reclaim is not cheaply computable — this is a hint,
    // not a promise.
    const NOMINAL_SNAPSHOT_SIZE: u64 = 50 * 1024 * 1024; // 50 MB

    let paths: Vec<PathInfo> = snapshot_names
        .iter()
        .map(|name| PathInfo {
            path: format!("tmutil://{}", name),
            size: NOMINAL_SNAPSHOT_SIZE,
            is_dir: false,
        })
        .collect();
    let total_size = NOMINAL_SNAPSHOT_SIZE * paths.len() as u64;

    crate::commands::shared::log_operation(
        "SCAN",
        "Time Machine Local Snapshots",
        &format!("{} snapshots (nominal {} bytes)", paths.len(), total_size),
    );

    Some(ScanItem {
        rule_id: "special_tm_local_snapshots".into(),
        category: "Maintenance".into(),
        label: format!("Time Machine Local Snapshots ({})", paths.len()),
        paths,
        total_size,
    })
}

/// File extensions that indicate a virtual-machine disk image.
const VM_IMAGE_EXTENSIONS: &[&str] =
    &["qcow2", "img", "vmdk", "vhd", "vhdx", "vdi", "raw", "dmg"];

/// Bundle IDs used by Claude Desktop releases. If any of these are
/// installed (or Spotlight can find them anywhere), we leave the VM
/// data alone — it's still owned by a live app.
const CLAUDE_DESKTOP_BUNDLE_IDS: &[&str] =
    &["com.anthropic.claudefordesktop", "com.anthropic.claude"];

/// Returns true if a Claude Desktop bundle is present anywhere on disk.
fn claude_desktop_installed(installed_ids: &HashSet<String>) -> bool {
    for id in CLAUDE_DESKTOP_BUNDLE_IDS {
        if installed_ids.contains(*id) {
            return true;
        }
        if mdfind_has_bundle_id(id) {
            return true;
        }
    }
    false
}

/// Walk a directory tree looking for files whose extension matches
/// `VM_IMAGE_EXTENSIONS`. Results accumulate into `out`; walk stops
/// after `max_results` to keep pathological trees bounded.
fn find_vm_disk_images(root: &Path, out: &mut Vec<PathBuf>, max_results: usize) {
    if out.len() >= max_results {
        return;
    }
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if out.len() >= max_results {
            return;
        }
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            find_vm_disk_images(&path, out, max_results);
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let lower = ext.to_lowercase();
            if VM_IMAGE_EXTENSIONS.iter().any(|e| *e == lower) {
                out.push(path);
            }
        }
    }
}

/// Special scan: Claude Desktop virtual-machine disk images left behind
/// after the app is uninstalled. Claude Desktop uses VMs for its
/// sandboxed compute environments and their disk images can exceed a
/// few GB each; uninstalling the app does not always reap them.
///
/// We only surface images when:
/// - Claude Desktop is NOT installed anywhere Spotlight can find it
/// - The image file is at least 100 MB (smaller files are usually
///   config/metadata, not VM disks)
/// - The file is outside any whitelisted path
fn scan_orphaned_claude_vms() -> Option<ScanItem> {
    let installed_ids = collect_installed_bundle_ids();
    if claude_desktop_installed(&installed_ids) {
        return None;
    }

    let home = dirs::home_dir()?;
    let roots = [
        home.join("Library/Application Support/Claude"),
        home.join("Library/Containers/com.anthropic.claudefordesktop"),
        home.join("Library/Containers/com.anthropic.claude"),
        home.join("Library/Caches/com.anthropic.claudefordesktop"),
        home.join("Library/Caches/com.anthropic.claude"),
    ];

    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in &roots {
        if root.is_dir() {
            find_vm_disk_images(root, &mut candidates, 50);
        }
    }

    const MIN_VM_IMAGE_SIZE: u64 = 100 * 1024 * 1024; // 100 MB
    const ORPHAN_AGE_DAYS: u64 = 7;

    let age_cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(ORPHAN_AGE_DAYS * 86400))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;
    for path in candidates {
        let path_str = path.to_string_lossy().to_string();
        if is_whitelisted(&path_str, &settings.whitelist) {
            continue;
        }
        let meta = match path.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len();
        if size < MIN_VM_IMAGE_SIZE {
            continue;
        }
        // Only flag VM images that have not been modified for at least
        // ORPHAN_AGE_DAYS days, avoiding false positives right after
        // the app is removed.
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified > age_cutoff {
            continue;
        }
        paths.push(PathInfo {
            path: path_str,
            size,
            is_dir: false,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Orphaned Claude Desktop VM disks",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "orphan_claude_desktop_vm".into(),
        category: "Orphaned Data".into(),
        label: "Orphaned Claude Desktop VM disks".into(),
        paths,
        total_size,
    })
}

/// Special scan: find per-user LaunchAgents whose referenced program
/// no longer exists on disk. Each `.plist` under `~/Library/LaunchAgents`
/// nominates a binary via `Program` or `ProgramArguments[0]`; if that
/// binary is gone, the agent is a dead stub left behind by an
/// uninstalled app or a broken installer. We only touch the per-user
/// agents directory — system-wide LaunchDaemons require root and are
/// out of scope.
fn scan_orphaned_launch_agents() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let agents_dir = home.join("Library/LaunchAgents");
    if !agents_dir.is_dir() {
        return None;
    }

    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(e) => e,
        Err(_) => return None,
    };

    for entry in entries.flatten() {
        let plist_path = entry.path();
        if plist_path.is_symlink() || !plist_path.is_file() {
            continue;
        }
        let name = match plist_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.ends_with(".plist") {
            continue;
        }
        // Skip any Apple-owned agents that somehow land here.
        if name.starts_with("com.apple.") {
            continue;
        }
        // Respect whitelist
        let path_str = plist_path.to_string_lossy().to_string();
        if is_whitelisted(&path_str, &settings.whitelist) {
            continue;
        }

        // Parse the plist and pull out the referenced binary.
        let plist = match plist::Value::from_file(&plist_path) {
            Ok(v) => v,
            Err(_) => continue, // malformed plist — leave it alone
        };
        let dict = match plist.as_dictionary() {
            Some(d) => d,
            None => continue,
        };

        let program_path: Option<String> = if let Some(prog) =
            dict.get("Program").and_then(|v| v.as_string())
        {
            Some(prog.to_string())
        } else if let Some(args) = dict.get("ProgramArguments").and_then(|v| v.as_array()) {
            args.first().and_then(|v| v.as_string()).map(|s| s.to_string())
        } else {
            None
        };

        let Some(program) = program_path else {
            continue;
        };

        // Expand `~` just in case — LaunchAgents typically use absolute
        // paths but we'll be defensive.
        let expanded = if let Some(rest) = program.strip_prefix("~/") {
            home.join(rest)
        } else {
            PathBuf::from(&program)
        };

        if expanded.exists() {
            continue;
        }

        let size = plist_path.metadata().map(|m| m.len()).unwrap_or(0);
        paths.push(PathInfo {
            path: path_str,
            size,
            is_dir: false,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Orphaned LaunchAgents",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "orphan_launch_agents".into(),
        category: "Orphaned Data".into(),
        label: "Orphaned LaunchAgents".into(),
        paths,
        total_size,
    })
}

/// Number of JetBrains Toolbox IDE builds to preserve per product.
/// Toolbox keeps older builds around for rollback, but only the most
/// recent is actively used; each old build is typically 1–2 GB.
const JETBRAINS_TOOLBOX_KEEP: usize = 1;

/// Special scan: surface old JetBrains Toolbox IDE builds beyond the
/// most recent one per product. Toolbox stores each product under
/// `~/Library/Application Support/JetBrains/Toolbox/apps/<Product>/ch-0/<build>/`
/// and the active build is whichever a running IDE launcher points at.
/// We sort by mtime and preserve the top `JETBRAINS_TOOLBOX_KEEP`.
fn scan_jetbrains_toolbox_old_builds() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let apps_root = home.join("Library/Application Support/JetBrains/Toolbox/apps");
    if !apps_root.is_dir() {
        return None;
    }

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    let products = match std::fs::read_dir(&apps_root) {
        Ok(e) => e,
        Err(_) => return None,
    };

    for product_entry in products.flatten() {
        let product_path = product_entry.path();
        if !product_path.is_dir() || product_path.is_symlink() {
            continue;
        }

        // Each product has a channel subdir (typically ch-0). Walk any
        // channels we find so this works for users with release/EAP
        // channels installed side-by-side.
        let channels = match std::fs::read_dir(&product_path) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for channel_entry in channels.flatten() {
            let channel_path = channel_entry.path();
            if !channel_path.is_dir() || channel_path.is_symlink() {
                continue;
            }

            // Collect build subdirectories with their mtime.
            let mut builds: Vec<(PathBuf, SystemTime)> = Vec::new();
            let build_entries = match std::fs::read_dir(&channel_path) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for build_entry in build_entries.flatten() {
                let build_path = build_entry.path();
                if !build_path.is_dir() || build_path.is_symlink() {
                    continue;
                }
                // Skip non-build metadata files like `.history.json`.
                let name = match build_path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                if name.starts_with('.') {
                    continue;
                }
                let modified = build_entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                builds.push((build_path, modified));
            }

            // Sort newest first, preserve the top N.
            builds.sort_by(|a, b| b.1.cmp(&a.1));

            for (path, _) in builds.into_iter().skip(JETBRAINS_TOOLBOX_KEEP) {
                let size = deletable_dir_size(&path);
                if size == 0 {
                    continue;
                }
                paths.push(PathInfo {
                    path: path.to_string_lossy().to_string(),
                    size,
                    is_dir: true,
                });
                total_size += size;
            }
        }
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "JetBrains Toolbox (old builds)",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "dev_jetbrains_toolbox_old".into(),
        category: "Developer Tools".into(),
        label: "JetBrains Toolbox (old builds)".into(),
        paths,
        total_size,
    })
}

/// Returns true if any Xcode simulator runtime is currently booted,
/// determined via `xcrun simctl list devices booted`. When a simulator
/// is booted we must not touch its dyld shared cache — the running
/// runtime mmap's those files and clearing them crashes the simulator
/// in non-obvious ways (missing frameworks, symbol lookup failures).
fn xcode_simulator_is_booted() -> bool {
    let out = std::process::Command::new("/usr/bin/xcrun")
        .args(["simctl", "list", "devices", "booted"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // When no devices are booted simctl prints "-- Unavailable: …"
            // sections or just the header; an active boot shows a line
            // containing "(Booted)".
            stdout.contains("(Booted)")
        }
        _ => false,
    }
}

/// Special scan: detect unavailable Xcode simulators via `xcrun simctl`.
/// These are simulators whose runtimes have been removed and can be
/// safely deleted with `xcrun simctl delete unavailable`. The executor
/// runs that command first, then falls back to manual deletion of
/// orphaned device directories if the command fails.
fn scan_xcode_unavailable_simulators() -> Option<ScanItem> {
    // Check if xcrun simctl is available
    let simctl_check = std::process::Command::new("/usr/bin/xcrun")
        .args(["simctl", "list", "devices", "unavailable"])
        .output();
    let output = match simctl_check {
        Ok(o) if o.status.success() => o,
        _ => return None,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse UDIDs of unavailable simulators
    let mut udids: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if !line.contains("(unavailable") {
            continue;
        }
        // Extract UUID: look for (XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX)
        if let Some(start) = line.find('(') {
            let after = &line[start + 1..];
            if let Some(end) = after.find(')') {
                let candidate = &after[..end];
                // Validate UUID format (36 chars: 8-4-4-4-12 hex)
                if candidate.len() == 36
                    && candidate.chars().enumerate().all(|(i, c)| {
                        if i == 8 || i == 13 || i == 18 || i == 23 {
                            c == '-'
                        } else {
                            c.is_ascii_hexdigit()
                        }
                    })
                {
                    udids.push(candidate.to_string());
                }
            }
        }
    }

    if udids.is_empty() {
        return None;
    }

    let home = dirs::home_dir()?;
    let devices_root = home.join("Library/Developer/CoreSimulator/Devices");

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for udid in &udids {
        let device_dir = devices_root.join(udid);
        let size = if device_dir.is_dir() {
            deletable_dir_size(&device_dir)
        } else {
            // Device dir may not exist on disk; use nominal size
            4096
        };
        // Store the UDID as a pseudo-path so the executor can use
        // `xcrun simctl delete unavailable` and fall back to manual
        // deletion of these specific device directories.
        paths.push(PathInfo {
            path: format!("simctl_unavailable://{}", udid),
            size,
            is_dir: true,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Xcode Unavailable Simulators",
        &format!("{} simulators, {} bytes", udids.len(), total_size),
    );

    Some(ScanItem {
        rule_id: "dev_xcode_unavailable_sims".into(),
        category: "Developer Tools".into(),
        label: format!("Xcode Unavailable Simulators ({})", udids.len()),
        paths,
        total_size,
    })
}

/// Special scan: Xcode simulator caches, gated on no booted simulators.
/// Returns `None` if Xcode command-line tools aren't present, the cache
/// is empty, or any simulator is currently running.
fn scan_xcode_simulator_caches() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let cache = home.join("Library/Developer/CoreSimulator/Caches");
    if !cache.is_dir() {
        return None;
    }

    if xcode_simulator_is_booted() {
        crate::commands::shared::log_operation(
            "SCAN",
            "Xcode Simulator Caches",
            "skipped: simulator currently booted",
        );
        return None;
    }

    let size = deletable_dir_size(&cache);
    if size == 0 {
        return None;
    }

    let path_str = cache.to_string_lossy().to_string();
    crate::commands::shared::log_operation(
        "SCAN",
        "Xcode Simulator Caches",
        &format!("{} bytes", size),
    );

    Some(ScanItem {
        rule_id: "dev_xcode_simulators".into(),
        category: "Developer Tools".into(),
        label: "Xcode Simulator Caches".into(),
        paths: vec![PathInfo {
            path: path_str,
            size,
            is_dir: true,
        }],
        total_size: size,
    })
}

/// Special scan: dynamically discover macOS installer apps in /Applications.
/// Safety gates:
///   1. Glob `/Applications/Install macOS*.app` — catches future OS names
///   2. Skip if installer process is currently running (`pgrep -f`)
///   3. Skip if installer's DTPlatformVersion matches current macOS major
///   4. Skip if younger than 14 days
fn scan_macos_installers() -> Option<ScanItem> {
    let apps_dir = Path::new("/Applications");
    if !apps_dir.is_dir() {
        return None;
    }

    // Get current macOS major version for the version guard
    let current_major = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let ver = String::from_utf8_lossy(&o.stdout).trim().to_string();
                ver.split('.').next().map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let cutoff = SystemTime::now() - Duration::from_secs(14 * 86400);
    let mut paths = Vec::new();
    let mut total_size = 0u64;

    let entries = match std::fs::read_dir(apps_dir) {
        Ok(e) => e,
        Err(_) => return None,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only match "Install macOS *.app"
        if !name.starts_with("Install macOS ") || !name.ends_with(".app") {
            continue;
        }
        if !path.is_dir() {
            continue;
        }

        // Gate 1: skip if installer is currently running
        let pgrep_out = std::process::Command::new("/usr/bin/pgrep")
            .arg("-f")
            .arg(&path.to_string_lossy().to_string())
            .output();
        if let Ok(o) = pgrep_out {
            if o.status.success() && !o.stdout.is_empty() {
                crate::commands::shared::log_operation(
                    "SCAN", &name, "skipped: installer is currently running",
                );
                continue;
            }
        }

        // Gate 2: skip if DTPlatformVersion matches current macOS major
        if !current_major.is_empty() {
            let info_plist = path.join("Contents/Info.plist");
            if info_plist.exists() {
                if let Ok(plist_val) = plist::Value::from_file(&info_plist) {
                    if let Some(dict) = plist_val.as_dictionary() {
                        if let Some(ver) = dict.get("DTPlatformVersion").and_then(|v| v.as_string()) {
                            let installer_major = ver.split('.').next().unwrap_or("");
                            if installer_major == current_major {
                                crate::commands::shared::log_operation(
                                    "SCAN", &name,
                                    &format!("skipped: matches current macOS version ({})", current_major),
                                );
                                continue;
                            }
                        }
                    }
                }
            }
        }

        // Gate 3: skip if younger than 14 days
        if let Ok(meta) = path.metadata() {
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if modified > cutoff {
                let age_days = SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default()
                    .as_secs() / 86400;
                crate::commands::shared::log_operation(
                    "SCAN", &name,
                    &format!("skipped: only {} days old (need 14+)", age_days),
                );
                continue;
            }
        }

        let size = deletable_dir_size(&path);
        if size == 0 {
            continue;
        }

        paths.push(PathInfo {
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: true,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Old macOS Installers",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "system_macos_installers".into(),
        category: "System".into(),
        label: "Old macOS Installers".into(),
        paths,
        total_size,
    })
}

/// Special scan: find incomplete download files in ~/Downloads.
fn scan_incomplete_downloads() -> Option<ScanItem> {
    let downloads = dirs::home_dir()?.join("Downloads");
    let mut paths = Vec::new();
    let mut total_size = 0u64;

    let extensions = [".crdownload", ".part", ".download", ".partial"];

    for entry in std::fs::read_dir(&downloads).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();

        if extensions.iter().any(|ext| lower.ends_with(ext)) {
            let path = entry.path();
            let path_str = path.to_string_lossy().to_string();

            // Skip files that are still being written to (active download)
            if let Ok(lsof_out) = std::process::Command::new("lsof")
                .arg(&path_str)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
            {
                if lsof_out.success() {
                    // File is still open by a process — skip it
                    continue;
                }
            }

            if let Ok(meta) = path.metadata() {
                let size = meta.len();
                paths.push(PathInfo {
                    path: path.to_string_lossy().to_string(),
                    size,
                    is_dir: path.is_dir(),
                });
                total_size += size;
            }
        }
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Incomplete Downloads",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "maint_incomplete_downloads".into(),
        category: "Maintenance".into(),
        label: "Incomplete Downloads".into(),
        paths,
        total_size,
    })
}

/// Expands `~` at the start of a path to the user's home directory.
fn expand_home(path: &str) -> Option<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir().map(|home| home.join(rest))
    } else if path == "~" {
        dirs::home_dir()
    } else {
        Some(PathBuf::from(path))
    }
}

#[cfg(test)]
fn expand_home_glob(path: &str) -> Vec<PathBuf> {
    expand_home_glob_in(path, dirs::home_dir().as_deref())
}

/// Like `expand_home`, but supports a single `*` wildcard segment in the path.
/// For example, `~/Library/.../Profiles/*/cache2` will enumerate all entries
/// under the `Profiles` directory and return each matching `cache2` child.
/// Patterns like `~/Downloads/*.part` match files whose names end with `.part`.
/// If the path contains no `*`, behaves identically to `expand_home` (returns 0 or 1 path).
/// `~` expands against `home`.
fn expand_home_glob_in(path: &str, home: Option<&Path>) -> Vec<PathBuf> {
    // First, expand the home directory prefix.
    let expanded_str = if let Some(rest) = path.strip_prefix("~/") {
        match home {
            Some(home) => format!("{}/{}", home.display(), rest),
            None => return Vec::new(),
        }
    } else if path == "~" {
        match home {
            Some(home) => return vec![home.to_path_buf()],
            None => return Vec::new(),
        }
    } else {
        path.to_string()
    };

    if !expanded_str.contains('*') {
        let p = PathBuf::from(&expanded_str);
        return vec![p];
    }

    // Split at the first `*`.
    let (before_star, after_star) = match expanded_str.split_once('*') {
        Some(pair) => pair,
        None => return vec![PathBuf::from(&expanded_str)],
    };

    // `before_star` ends with `/` for directory wildcards (e.g. `.../Profiles/`)
    // or ends with a filename prefix for file wildcards (e.g. `.../Downloads/`).
    // `after_star` starts with the remainder (e.g. `/cache2` or `.part`).

    // Determine the parent directory to enumerate and any filename prefix.
    let parent_dir: &str;
    let name_prefix: &str;
    if let Some(idx) = before_star.rfind('/') {
        parent_dir = &before_star[..idx];
        name_prefix = &before_star[idx + 1..];
    } else {
        parent_dir = ".";
        name_prefix = before_star;
    }

    let entries = match std::fs::read_dir(parent_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();
    for entry in entries.flatten() {
        let entry_name = entry.file_name();
        let entry_name_str = entry_name.to_string_lossy();

        // Check that the entry name starts with the prefix (if any).
        if !name_prefix.is_empty() && !entry_name_str.starts_with(name_prefix) {
            continue;
        }

        // Check that the entry name ends with the suffix (if any).
        // `after_star` could be `/cache2` (directory wildcard) or `.part` (file wildcard).
        if after_star.starts_with('/') {
            // Directory wildcard: entry must be a directory, and we append the remainder.
            let suffix = &after_star[1..]; // strip leading `/`
            let candidate = entry.path().join(suffix);
            results.push(candidate);
        } else {
            // File wildcard: entry name must end with after_star (e.g. `.part`).
            if !entry_name_str.ends_with(after_star) {
                continue;
            }
            results.push(entry.path());
        }
    }

    results
}

/// Returns true if `path` (or any parent) is covered by a whitelisted entry.
fn is_whitelisted(path: &str, whitelist: &[String]) -> bool {
    whitelist.iter().any(|w| path == w || path.starts_with(&format!("{}/", w)))
}

/// Scan external volumes for macOS metadata debris (.Spotlight-V100, .Trashes, .fseventsd).
fn scan_external_volumes_metadata() -> Vec<ScanItem> {
    let mut items = Vec::new();
    let volumes = std::path::Path::new("/Volumes");
    if !volumes.exists() {
        return items;
    }

    // Get boot volume name to skip it
    let boot_volume = std::process::Command::new("diskutil")
        .args(["info", "-plist", "/"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            // Extract VolumeName — look for the key then grab the string value
            let key = "<key>VolumeName</key>";
            text.find(key).and_then(|pos| {
                let after = &text[pos + key.len()..];
                let start = after.find("<string>")? + 8;
                let end = after[start..].find("</string>")?;
                Some(after[start..start + end].to_string())
            })
        })
        .unwrap_or_default();

    if let Ok(entries) = std::fs::read_dir(volumes) {
        for entry in entries.flatten() {
            let vol_path = entry.path();
            let vol_name = entry.file_name().to_string_lossy().to_string();

            if vol_name == boot_volume || vol_name.starts_with('.') || !vol_path.is_dir() {
                continue;
            }

            let metadata_dirs = [".Spotlight-V100", ".Trashes", ".fseventsd"];
            let mut vol_paths = Vec::new();
            let mut vol_size: u64 = 0;

            for meta_name in &metadata_dirs {
                let meta_path = vol_path.join(meta_name);
                if meta_path.exists() {
                    let size = dir_size(&meta_path);
                    vol_size += size;
                    vol_paths.push(PathInfo {
                        path: meta_path.to_string_lossy().to_string(),
                        size,
                        is_dir: true,
                    });
                }
            }

            // Also check for .DS_Store at top level
            let ds_store = vol_path.join(".DS_Store");
            if ds_store.exists() {
                let size = std::fs::metadata(&ds_store).map(|m| m.len()).unwrap_or(0);
                vol_size += size;
                vol_paths.push(PathInfo {
                    path: ds_store.to_string_lossy().to_string(),
                    size,
                    is_dir: false,
                });
            }

            if !vol_paths.is_empty() && vol_size > 0 {
                items.push(ScanItem {
                    rule_id: format!("external_vol_{}", vol_name.to_lowercase().replace(' ', "_")),
                    category: "System".into(),
                    label: format!("External Volume Metadata ({})", vol_name),
                    paths: vol_paths,
                    total_size: vol_size,
                });
            }
        }
    }

    items
}

/// Scans a directory for entries older than `max_age_days` days.
/// Returns individual PathInfo items for each old entry and their combined size.
fn scan_with_age_filter(dir: &Path, max_age_days: u32) -> (Vec<PathInfo>, u64) {
    let cutoff = SystemTime::now() - Duration::from_secs(max_age_days as u64 * 86400);
    let mut paths = Vec::new();
    let mut total = 0u64;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                continue;
            }

            if let Ok(meta) = path.metadata() {
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if modified < cutoff {
                    let size = if path.is_dir() {
                        deletable_dir_size(&path)
                    } else {
                        meta.len()
                    };
                    if size > 0 {
                        paths.push(PathInfo {
                            path: path.to_string_lossy().to_string(),
                            size,
                            is_dir: path.is_dir(),
                        });
                        total += size;
                    }
                }
            }
        }
    }
    (paths, total)
}

// ── Browser Old Framework Version Detection ─────────────────────────

/// A browser whose `Versions/` directory under its framework bundle
/// may contain old, no-longer-active framework versions.
struct BrowserFrameworkTarget {
    /// Human-readable label for the scan result (e.g. "Chrome Old Framework Versions").
    label: &'static str,
    /// Rule ID used in the resulting `ScanItem`.
    rule_id: &'static str,
    /// Exact process name for `pgrep -x` to detect whether the browser is running.
    process_name: &'static str,
    /// Relative path from the `.app` bundle to the `Versions/` directory.
    versions_subpath: &'static str,
}

/// Candidate `.app` locations: `/Applications` and `~/Applications`.
fn browser_app_paths(app_name: &str) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(format!("/Applications/{}", app_name))];
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(format!("Applications/{}", app_name)));
    }
    paths
}

/// Returns true if the process with the exact name is currently running.
fn is_exact_process_running(name: &str) -> bool {
    let out = std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", name])
        .output();
    match out {
        Ok(o) => o.status.success() && !o.stdout.is_empty(),
        Err(_) => false,
    }
}

/// Scan a single browser's framework `Versions/` directory for old
/// (non-current) version subdirectories.
///
/// Logic:
/// 1. Check if the browser process is running — skip if so.
/// 2. For each candidate `.app` location, locate `Versions/`.
/// 3. Resolve the `Current` symlink to identify the active version.
/// 4. Collect all other version directories as cleanable.
fn scan_single_browser_framework(
    target: &BrowserFrameworkTarget,
    whitelist: &[String],
) -> Option<ScanItem> {
    if is_exact_process_running(target.process_name) {
        crate::commands::shared::log_operation(
            "SCAN",
            target.label,
            &format!("skipped: {} is running", target.process_name),
        );
        return None;
    }

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    // Derive the .app bundle name from the versions_subpath
    // (everything before "/Contents/...")
    let app_bundle_name = target.versions_subpath
        .split("/Contents/")
        .next()
        .unwrap_or("");

    for app_dir in browser_app_paths(app_bundle_name) {
        let versions_dir = app_dir.join(
            target.versions_subpath
                .strip_prefix(app_bundle_name)
                .unwrap_or("")
                .trim_start_matches('/')
        );

        if !versions_dir.is_dir() {
            continue;
        }

        let current_link = versions_dir.join("Current");
        if !current_link.is_symlink() {
            continue;
        }

        let current_version = match std::fs::read_link(&current_link) {
            Ok(target) => {
                // The symlink target may be just "130.0.0.0" or a full path;
                // we only need the final component.
                target
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            }
            Err(_) => continue,
        };
        if current_version.is_empty() {
            continue;
        }

        // Verify the Current symlink target actually exists; if broken,
        // skip to avoid accidentally deleting the active version.
        if !versions_dir.join(&current_version).is_dir() {
            crate::commands::shared::log_operation(
                "SCAN",
                target.label,
                "skipped: Current symlink target is broken",
            );
            continue;
        }

        let entries = match std::fs::read_dir(&versions_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || path.is_symlink() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name == "Current" || name == current_version {
                continue;
            }
            let path_str = path.to_string_lossy().to_string();
            if is_whitelisted(&path_str, whitelist) {
                crate::commands::shared::log_operation(
                    "SCAN",
                    &path_str,
                    &format!("skipped: on user whitelist ({})", target.label),
                );
                continue;
            }
            let size = deletable_dir_size(&path);
            if size == 0 {
                continue;
            }
            paths.push(PathInfo {
                path: path_str,
                size,
                is_dir: true,
            });
            total_size += size;
        }
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        target.label,
        &format!("{} bytes ({} dirs)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: target.rule_id.to_string(),
        category: "Browsers".into(),
        label: format!("{} ({} old)", target.label, paths.len()),
        paths,
        total_size,
    })
}

/// Special scan: find old EdgeUpdater version directories under
/// `~/Library/Application Support/Microsoft/EdgeUpdater/apps/msedge-stable/`.
/// Unlike the framework scanner, the EdgeUpdater has no `Current` symlink —
/// we keep only the latest version (by version sort) and flag the rest.
fn scan_edge_updater_old_versions(whitelist: &[String]) -> Option<ScanItem> {
    if is_exact_process_running("Microsoft Edge") {
        crate::commands::shared::log_operation(
            "SCAN",
            "Edge Updater Old Versions",
            "skipped: Microsoft Edge is running",
        );
        return None;
    }

    let home = dirs::home_dir()?;
    let updater_dir = home.join("Library/Application Support/Microsoft/EdgeUpdater/apps/msedge-stable");
    if !updater_dir.is_dir() {
        return None;
    }

    let mut version_dirs: Vec<(String, PathBuf)> = Vec::new();
    let entries = match std::fs::read_dir(&updater_dir) {
        Ok(e) => e,
        Err(_) => return None,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || path.is_symlink() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            version_dirs.push((name.to_string(), path));
        }
    }

    // Need at least 2 versions to have something to clean
    if version_dirs.len() < 2 {
        return None;
    }

    // Sort by version string descending to find the latest
    version_dirs.sort_by(|a, b| version_compare(&b.0, &a.0));
    let latest = &version_dirs[0].0;

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for (name, path) in &version_dirs {
        if name == latest {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        if is_whitelisted(&path_str, whitelist) {
            crate::commands::shared::log_operation(
                "SCAN",
                &path_str,
                "skipped: on user whitelist (Edge Updater Old Versions)",
            );
            continue;
        }
        let size = deletable_dir_size(path);
        if size == 0 {
            continue;
        }
        paths.push(PathInfo {
            path: path_str,
            size,
            is_dir: true,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Edge Updater Old Versions",
        &format!("{} bytes ({} dirs)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "browser_edge_updater_old_versions".into(),
        category: "Browsers".into(),
        label: format!("Edge Updater Old Versions ({} old)", paths.len()),
        paths,
        total_size,
    })
}

/// Simple version comparison using numeric segments split on `.`.
/// Falls back to lexicographic comparison for non-numeric segments.
fn version_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts: Vec<&str> = a.split('.').collect();
    let b_parts: Vec<&str> = b.split('.').collect();
    let max_len = a_parts.len().max(b_parts.len());
    for i in 0..max_len {
        let a_seg = a_parts.get(i).copied().unwrap_or("0");
        let b_seg = b_parts.get(i).copied().unwrap_or("0");
        match (a_seg.parse::<u64>(), b_seg.parse::<u64>()) {
            (Ok(an), Ok(bn)) => match an.cmp(&bn) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            },
            _ => match a_seg.cmp(b_seg) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            },
        }
    }
    std::cmp::Ordering::Equal
}

/// Scan all supported browsers for old framework versions, plus
/// Edge's updater directory.
fn scan_browser_old_framework_versions() -> Vec<ScanItem> {
    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let whitelist = &settings.whitelist;

    let targets = [
        BrowserFrameworkTarget {
            label: "Chrome Old Framework Versions",
            rule_id: "browser_chrome_old_framework",
            process_name: "Google Chrome",
            versions_subpath: "Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions",
        },
        BrowserFrameworkTarget {
            label: "Edge Old Framework Versions",
            rule_id: "browser_edge_old_framework",
            process_name: "Microsoft Edge",
            versions_subpath: "Microsoft Edge.app/Contents/Frameworks/Microsoft Edge Framework.framework/Versions",
        },
        BrowserFrameworkTarget {
            label: "Brave Old Framework Versions",
            rule_id: "browser_brave_old_framework",
            process_name: "Brave Browser",
            versions_subpath: "Brave Browser.app/Contents/Frameworks/Brave Browser Framework.framework/Versions",
        },
    ];

    let mut items = Vec::new();
    for target in &targets {
        if let Some(item) = scan_single_browser_framework(target, whitelist) {
            items.push(item);
        }
    }
    if let Some(edge_updater) = scan_edge_updater_old_versions(whitelist) {
        items.push(edge_updater);
    }
    items
}

/// Scan orphaned system-level services (LaunchDaemons, LaunchAgents,
/// PrivilegedHelperTools) left behind after apps are uninstalled.
/// These live under `/Library/` and normally require root to read,
/// so we shell out to `find` with sudo. Each found plist/helper is
/// checked against Spotlight to see if the owning app still exists;
/// entries whose owning app is gone are flagged. The scan is gated on
/// `sudo -n true` succeeding (i.e. the user has an active sudo
/// ticket) — we never prompt for a password during a scan.
///
/// Items returned carry `needs_admin: true` via the `system_services://`
/// pseudo-path prefix so the executor knows to use elevated privileges.
fn scan_orphaned_system_services() -> Option<ScanItem> {
    // Only proceed if we already have a sudo ticket — never prompt.
    let sudo_check = std::process::Command::new("sudo")
        .args(["-n", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match sudo_check {
        Ok(s) if s.success() => {}
        _ => return None,
    }

    let scan_dirs: &[(&str, &str)] = &[
        ("/Library/LaunchDaemons", "*.plist"),
        ("/Library/LaunchAgents", "*.plist"),
        ("/Library/PrivilegedHelperTools", "*"),
    ];

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for (dir, _pattern) in scan_dirs {
        let dir_path = Path::new(dir);
        if !dir_path.is_dir() {
            continue;
        }

        // Use sudo find to list entries
        let output = std::process::Command::new("sudo")
            .args(["find", dir, "-maxdepth", "1", "-not", "-name", ".", "-print0"])
            .output();
        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        for entry_str in stdout.split('\0') {
            let entry_str = entry_str.trim();
            if entry_str.is_empty() || entry_str == *dir {
                continue;
            }

            let entry_path = Path::new(entry_str);
            let filename = match entry_path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Skip Apple system files
            if filename.starts_with("com.apple.") {
                continue;
            }

            // For plist dirs, only look at .plist files
            if dir.contains("Launch") && !filename.ends_with(".plist") {
                continue;
            }

            // Extract the bundle ID
            let bundle_id = filename
                .trim_end_matches(".plist")
                .to_string();

            // Only consider entries that look like bundle IDs
            if !bundle_id.contains('.') {
                continue;
            }

            // Check if the owning app is still installed via Spotlight
            if mdfind_has_bundle_id(&bundle_id) {
                continue;
            }

            // Get file size via sudo stat
            let size = std::process::Command::new("sudo")
                .args(["stat", "-f", "%z", entry_str])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        String::from_utf8_lossy(&o.stdout)
                            .trim()
                            .parse::<u64>()
                            .ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            paths.push(PathInfo {
                path: format!("system_services://{}", entry_str),
                size,
                is_dir: false,
            });
            total_size += size;
        }
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Orphaned System Services",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "orphan_system_services".into(),
        category: "Orphaned Data".into(),
        label: format!("Orphaned System Services ({})", paths.len()),
        paths,
        total_size,
    })
}

/// Collect the set of active mount points on the system. Used to
/// determine whether a simulator runtime volume is currently in use.
fn collect_mount_points() -> Vec<String> {
    let output = std::process::Command::new("mount").output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    // mount output format: <device> on <mountpoint> (<options>)
                    let parts: Vec<&str> = line.splitn(4, ' ').collect();
                    if parts.len() >= 3 && parts[1] == "on" {
                        Some(parts[2].to_string())
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Returns true if a path (or any sub-path) appears in the list of
/// active mount points, indicating the runtime volume is in use.
fn is_path_mounted(path: &Path, mount_points: &[String]) -> bool {
    let path_str = path.to_string_lossy();
    mount_points.iter().any(|mp| {
        mp == path_str.as_ref() || mp.starts_with(&format!("{}/", path_str))
    })
}

/// Special scan: find unused Xcode simulator runtime volumes under
/// `/Library/Developer/CoreSimulator/Volumes` and
/// `/Library/Developer/CoreSimulator/Cryptex`. These can be multiple
/// GB each. A volume is considered "in use" if it (or any sub-path)
/// appears as an active mount point; unused volumes are flagged for
/// cleanup. Requires the directories to exist and at least one
/// candidate to be found.
fn scan_xcode_simulator_runtime_volumes() -> Option<ScanItem> {
    let volumes_root = Path::new("/Library/Developer/CoreSimulator/Volumes");
    let cryptex_root = Path::new("/Library/Developer/CoreSimulator/Cryptex");

    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in &[volumes_root, cryptex_root] {
        if !root.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !path.is_symlink() {
                candidates.push(path);
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    let mount_points = collect_mount_points();
    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for candidate in &candidates {
        // Skip volumes that are actively mounted (in use by a runtime)
        if is_path_mounted(candidate, &mount_points) {
            continue;
        }

        let size = deletable_dir_size(candidate);
        if size == 0 {
            continue;
        }

        paths.push(PathInfo {
            path: candidate.to_string_lossy().to_string(),
            size,
            is_dir: true,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Xcode Simulator Runtime Volumes (unused)",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "dev_xcode_sim_runtime_volumes".into(),
        category: "Developer Tools".into(),
        label: format!("Xcode Simulator Runtime Volumes ({} unused)", paths.len()),
        paths,
        total_size,
    })
}

/// Scans the filesystem for items matching the given rules.
/// Returns only rules that have at least one existing path with non-zero size.
/// Collects the existing, non-whitelisted, non-empty paths a standard rule
/// matches. Returns `None` when nothing matched.
fn collect_rule_paths(rule: &CleanRule, whitelist: &[String]) -> Option<ScanItem> {
    collect_rule_paths_in(rule, whitelist, dirs::home_dir().as_deref())
}

pub(super) fn collect_rule_paths_in(rule: &CleanRule, whitelist: &[String], home: Option<&Path>) -> Option<ScanItem> {
    let mut found_paths = Vec::new();
    let mut total_size: u64 = 0;

    for raw_path in &rule.paths {
        let expanded_paths = expand_home_glob_in(raw_path, home);

        for expanded in expanded_paths {
            if !expanded.exists() {
                continue;
            }

            // Skip whitelisted paths
            let expanded_str = expanded.to_string_lossy().to_string();
            if is_whitelisted(&expanded_str, whitelist) {
                crate::commands::shared::log_operation(
                    "SCAN",
                    &expanded_str,
                    &format!("skipped: on user whitelist (rule: {})", rule.label),
                );
                continue;
            }

            if let Some(max_age_days) = rule.max_age_days {
                // Age-filtered scanning: only include files older than the threshold
                if expanded.is_dir() {
                    let (old_paths, _old_total) = scan_with_age_filter(&expanded, max_age_days);
                    let before_count = found_paths.len();
                    for p in old_paths {
                        if !is_whitelisted(&p.path, whitelist) {
                            total_size += p.size;
                            found_paths.push(p);
                        }
                    }
                    let added = found_paths.len() - before_count;
                    if added > 0 {
                        crate::commands::shared::log_operation(
                            "SCAN",
                            &rule.label,
                            &format!("age-filter>{} days: {} paths from {}", max_age_days, added, expanded.display()),
                        );
                    }
                }
            } else {
                // Standard scanning: include the entire path
                let size = if expanded.is_dir() && super::executor::is_container_dir(&expanded_str) {
                    container_deletable_size(&expanded)
                } else if expanded.is_dir() {
                    deletable_dir_size(&expanded)
                } else {
                    expanded.metadata().map(|m| m.len()).unwrap_or(0)
                };

                if size == 0 {
                    continue;
                }

                found_paths.push(PathInfo {
                    path: expanded.to_string_lossy().to_string(),
                    size,
                    is_dir: expanded.is_dir(),
                });
                total_size += size;
            }
        }
    }

    if found_paths.is_empty() {
        return None;
    }
    crate::commands::shared::log_operation(
        "SCAN",
        &rule.label,
        &format!("{} bytes ({} paths)", total_size, found_paths.len()),
    );
    Some(ScanItem {
        rule_id: rule.id.clone(),
        category: rule.category.clone(),
        label: rule.label.clone(),
        paths: found_paths,
        total_size,
    })
}

/// Drops every path the executor's guard would refuse, so the UI never
/// offers space that can't be freed. Items left empty are dropped too.
pub(super) fn drop_guard_refused_paths(items: Vec<ScanItem>) -> Vec<ScanItem> {
    items
        .into_iter()
        .filter_map(|mut item| {
            if uses_pseudo_paths(&item.rule_id) {
                return Some(item);
            }
            let before = item.paths.len();
            item.paths.retain(|p| {
                if !is_safe_path(&p.path) {
                    return false;
                }
                match crate::commands::data_guard::check_general(Path::new(&p.path)) {
                    Ok(()) => true,
                    Err(refusal) => {
                        crate::commands::shared::log_operation("SCAN", &p.path, &format!("skipped: {}", refusal));
                        false
                    }
                }
            });
            if item.paths.len() != before {
                item.total_size = item.paths.iter().map(|p| p.size).sum();
            }
            (!item.paths.is_empty()).then_some(item)
        })
        .collect()
}

/// Size of what clearing a container folder (~/Library/Caches, …) removes:
/// the executor empties it entry by entry and skips symlinks, protected
/// user-data folders and anything the data guard refuses, so those are
/// not counted either.
fn container_deletable_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| !p.is_symlink())
        .filter(|p| {
            !p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(crate::commands::utils::is_protected_user_data_component)
        })
        .filter(|p| crate::commands::data_guard::check_general(p).is_ok())
        .map(|p| {
            if p.is_dir() {
                deletable_dir_size(&p)
            } else {
                p.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Keeps each path in the first item that lists it. Several rules can find
/// the same folder (a sandbox cache under its own rule and the dynamic
/// container scan, a log folder under its tool's rule and User Logs);
/// listing it twice counted its bytes twice. Items left empty are dropped.
pub fn drop_duplicate_paths(items: Vec<ScanItem>) -> Vec<ScanItem> {
    let mut seen: HashSet<String> = HashSet::new();
    items
        .into_iter()
        .filter_map(|mut item| {
            let before = item.paths.len();
            item.paths.retain(|p| seen.insert(p.path.clone()));
            if item.paths.len() != before {
                item.total_size = item.paths.iter().map(|p| p.size).sum();
            }
            (!item.paths.is_empty()).then_some(item)
        })
        .collect()
}

pub fn scan_rules(rules: &[CleanRule]) -> Vec<ScanItem> {
    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let mut results = Vec::new();

    for rule in rules {
        // Skip Spotify cache if offline music is present.
        // offline.bnk exists even with no offline downloads; only treat
        // it as evidence when it has real content (>1 KB). Encrypted
        // track blobs (*.file) in PersistentCache/Storage are reliable.
        if rule.id == "spotify_cache" {
            let home = dirs::home_dir().unwrap_or_default();
            let spotify_data = home.join("Library/Application Support/Spotify");
            let bnk_file = spotify_data.join("PersistentCache/Storage/offline.bnk");
            let bnk_size = bnk_file.metadata().map(|m| m.len()).unwrap_or(0);
            let has_track_blobs = {
                let storage_dir = spotify_data.join("PersistentCache/Storage");
                storage_dir.is_dir()
                    && std::fs::read_dir(&storage_dir)
                        .ok()
                        .map(|entries| {
                            entries
                                .flatten()
                                .any(|e| {
                                    e.file_name()
                                        .to_string_lossy()
                                        .ends_with(".file")
                                })
                        })
                        .unwrap_or(false)
            };
            if bnk_size > 1024 || has_track_blobs {
                crate::commands::shared::log_operation(
                    "SCAN",
                    &rule.label,
                    "skipped: offline music downloads detected",
                );
                continue;
            }
        }

        if let Some(item) = collect_rule_paths(rule, &settings.whitelist) {
            results.push(item);
        }
    }

    // Special scans (not covered by standard rules)
    if let Some(ds_store) = scan_ds_store_files() {
        results.push(ds_store);
    }
    if let Some(installers) = scan_macos_installers() {
        results.push(installers);
    }
    if let Some(incomplete) = scan_incomplete_downloads() {
        results.push(incomplete);
    }
    if let Some(tm_failed) = scan_tm_failed_backups() {
        results.push(tm_failed);
    }
    if let Some(xcode_ds) = scan_xcode_device_support() {
        results.push(xcode_ds);
    }
    if let Some(xcode_sim) = scan_xcode_simulator_caches() {
        results.push(xcode_sim);
    }
    if let Some(xcode_unavail) = scan_xcode_unavailable_simulators() {
        results.push(xcode_unavail);
    }
    if let Some(jb_old) = scan_jetbrains_toolbox_old_builds() {
        results.push(jb_old);
    }
    if let Some(orphan_agents) = scan_orphaned_launch_agents() {
        results.push(orphan_agents);
    }
    if let Some(claude_vms) = scan_orphaned_claude_vms() {
        results.push(claude_vms);
    }
    if let Some(tm_snaps) = scan_tm_local_snapshots() {
        results.push(tm_snaps);
    }
    if let Some(sys_services) = scan_orphaned_system_services() {
        results.push(sys_services);
    }
    if let Some(sim_vols) = scan_xcode_simulator_runtime_volumes() {
        results.push(sim_vols);
    }
    results.extend(scan_browser_old_framework_versions());
    results.extend(scan_external_volumes_metadata());
    results.extend(scan_service_worker_caches());
    results.extend(scan_dynamic_container_caches());
    if let Some(doc_idx) = scan_xcode_doc_stale_indexes() {
        results.push(doc_idx);
    }
    if let Some(rust_tc) = scan_rust_old_toolchains() {
        results.push(rust_tc);
    }
    if let Some(ndk) = scan_android_ndk_old_versions() {
        results.push(ndk);
    }

    drop_guard_refused_paths(results)
}

// ── Service Worker Cache Cleaning ──────────────────────────────────

/// Domains whose Service Worker caches must be preserved (web editors,
/// Google Workspace offline, code platforms, collaboration PWAs).
const PROTECTED_SW_DOMAINS: &[&str] = &[
    "capcut.com",
    "photopea.com",
    "pixlr.com",
    "docs.google.com",
    "sheets.google.com",
    "slides.google.com",
    "drive.google.com",
    "mail.google.com",
    "github.com",
    "gitlab.com",
    "codepen.io",
    "codesandbox.io",
    "replit.com",
    "stackblitz.com",
    "notion.so",
    "figma.com",
    "linear.app",
    "excalidraw.com",
];

/// Editor Service Worker cache roots (relative to ~/Library/Application Support).
/// Browsers are deliberately absent: their CacheStorage holds PWA offline data.
const SW_CACHE_ROOTS: &[(&str, &str)] = &[
    ("Code/Service Worker/CacheStorage", "VS Code"),
    ("Cursor/Service Worker/CacheStorage", "Cursor"),
];

/// Scan Service Worker CacheStorage directories across browsers, skipping
/// entries whose folder name contains a protected domain.
fn scan_service_worker_caches() -> Vec<ScanItem> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let app_support = home.join("Library/Application Support");
    let mut items = Vec::new();

    for (rel_path, browser_name) in SW_CACHE_ROOTS {
        let cache_root = app_support.join(rel_path);
        if !cache_root.is_dir() {
            continue;
        }

        let mut paths: Vec<PathInfo> = Vec::new();
        let mut total_size: u64 = 0;

        // CacheStorage has a two-level structure: origin_hash/cache_name
        let entries = match std::fs::read_dir(&cache_root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || path.is_symlink() {
                continue;
            }
            // Extract domain hint from directory name
            let name = entry.file_name().to_string_lossy().to_string();
            let is_protected = PROTECTED_SW_DOMAINS.iter().any(|d| name.contains(d));
            if is_protected {
                continue;
            }
            let size = deletable_dir_size(&path);
            if size == 0 {
                continue;
            }
            paths.push(PathInfo {
                path: path.to_string_lossy().to_string(),
                size,
                is_dir: true,
            });
            total_size += size;
        }

        // Also clean ScriptCache alongside CacheStorage
        let script_cache = cache_root.parent().map(|p| p.join("ScriptCache"));
        if let Some(sc) = script_cache {
            if sc.is_dir() {
                let size = deletable_dir_size(&sc);
                if size > 0 {
                    paths.push(PathInfo {
                        path: sc.to_string_lossy().to_string(),
                        size,
                        is_dir: true,
                    });
                    total_size += size;
                }
            }
        }

        if !paths.is_empty() {
            crate::commands::shared::log_operation(
                "SCAN",
                &format!("{} Service Worker Cache", browser_name),
                &format!("{} bytes ({} paths)", total_size, paths.len()),
            );
            items.push(ScanItem {
                rule_id: format!("sw_cache_{}", browser_name.to_lowercase().replace(' ', "_")),
                category: "Browsers".into(),
                label: format!("{} Service Worker Cache", browser_name),
                paths,
                total_size,
            });
        }
    }

    items
}

// ── Dynamic Container Cache Scanning ─────────────────────────────

/// Dynamically scan ~/Library/Containers/*/Data/Library/Caches for
/// containers not already covered by hardcoded rules. This catches
/// sandboxed apps that accumulate cache without a dedicated CleanRule.
fn scan_dynamic_container_caches() -> Vec<ScanItem> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let containers_dir = home.join("Library/Containers");
    if !containers_dir.is_dir() {
        return Vec::new();
    }

    // Bundle IDs already covered by hardcoded rules in rules.rs
    let known_containers: HashSet<&str> = [
        "com.apple.appstore",
        "com.apple.appstoreagent",
        "com.apple.stocks",
        "com.apple.wallpaper.agent",
        "com.apple.mediaanalysisd",
        "com.apple.geod",
        "com.apple.AMPArtworkAgent",
        "com.apple.podcasts",
        "com.apple.news",
        "com.apple.Maps",
        "com.apple.weather",
        "com.apple.Home",
        "com.apple.freeform",
        "com.apple.wallpaper.extension.aerials",
        // Containers covered by dedicated rules
        "com.ranchero.NetNewsWire-Evergreen",
        "com.ideasoncanvas.mindnode",
        "com.apple.mail",
    ].iter().copied().collect();

    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    let entries = match std::fs::read_dir(&containers_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let container_path = entry.path();
        if !container_path.is_dir() || container_path.is_symlink() {
            continue;
        }
        let bundle_id = match container_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        // Skip known containers (already have dedicated rules)
        if known_containers.contains(bundle_id) {
            continue;
        }

        // Scan both Data/Library/Caches and Data/tmp inside each container.
        let scan_subdirs = [
            container_path.join("Data/Library/Caches"),
            container_path.join("Data/tmp"),
        ];

        for caches_dir in &scan_subdirs {
            if !caches_dir.is_dir() {
                continue;
            }

            let cache_str = caches_dir.to_string_lossy().to_string();
            if is_whitelisted(&cache_str, &settings.whitelist) {
                continue;
            }

            let size = deletable_dir_size(caches_dir);
            // Only report if cache is meaningful (> 1 MB)
            if size < 1024 * 1024 {
                continue;
            }

            paths.push(PathInfo {
                path: cache_str,
                size,
                is_dir: true,
            });
            total_size += size;
        }
    }

    if paths.is_empty() {
        return Vec::new();
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Sandboxed App Caches (dynamic)",
        &format!("{} bytes ({} containers)", total_size, paths.len()),
    );

    vec![ScanItem {
        rule_id: "dynamic_container_caches".into(),
        category: "System".into(),
        label: format!("Sandboxed App Caches ({} apps)", paths.len()),
        paths,
        total_size,
    }]
}

// ── Xcode Documentation Stale Index ─────────────────────────────

/// Scan for stale Xcode documentation indexes, keeping only the newest.
/// The DocumentationCache directory under /Library/Developer/Xcode/ can
/// contain multiple `DeveloperDocumentation*.index` entries; only the
/// most recent is actively used.
fn scan_xcode_doc_stale_indexes() -> Option<ScanItem> {
    let doc_cache = Path::new("/Library/Developer/Xcode/DocumentationCache");
    if !doc_cache.is_dir() {
        return None;
    }

    let mut entries: Vec<(PathBuf, SystemTime)> = Vec::new();
    let dir_entries = std::fs::read_dir(doc_cache).ok()?;
    for entry in dir_entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("DeveloperDocumentation") && name.ends_with(".index") {
            let path = entry.path();
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((path, modified));
        }
    }

    // Need at least 2 to have stale ones
    if entries.len() < 2 {
        return None;
    }

    // Sort newest first, skip the first (keep it)
    entries.sort_by(|a, b| b.1.cmp(&a.1));

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for (path, _) in entries.into_iter().skip(1) {
        let size = if path.is_dir() {
            deletable_dir_size(&path)
        } else {
            path.metadata().map(|m| m.len()).unwrap_or(0)
        };
        if size == 0 {
            continue;
        }
        paths.push(PathInfo {
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: path.is_dir(),
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Xcode Documentation (stale indexes)",
        &format!("{} bytes ({} stale)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "dev_xcode_doc_stale_index".into(),
        category: "Developer Tools".into(),
        label: format!("Xcode Documentation ({} stale indexes)", paths.len()),
        paths,
        total_size,
    })
}

// ── Rust Old Toolchains ──────────────────────────────────────────

/// Number of Rust toolchains to preserve (the most recent by mtime).
const RUST_TOOLCHAIN_KEEP: usize = 2;

/// Scan `~/.rustup/toolchains` for old toolchain versions, keeping the
/// most recent N. Each toolchain can be 500 MB+.
fn scan_rust_old_toolchains() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let toolchains_dir = home.join(".rustup/toolchains");
    if !toolchains_dir.is_dir() {
        return None;
    }

    let mut versions: Vec<(PathBuf, SystemTime)> = Vec::new();
    let entries = std::fs::read_dir(&toolchains_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || path.is_symlink() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        versions.push((path, modified));
    }

    if versions.len() <= RUST_TOOLCHAIN_KEEP {
        return None;
    }

    versions.sort_by(|a, b| b.1.cmp(&a.1));

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for (path, _) in versions.into_iter().skip(RUST_TOOLCHAIN_KEEP) {
        let size = deletable_dir_size(&path);
        if size == 0 {
            continue;
        }
        paths.push(PathInfo {
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: true,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Rust Toolchains (old versions)",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "dev_rust_old_toolchains".into(),
        category: "Developer Tools".into(),
        label: format!("Rust Toolchains ({} old)", paths.len()),
        paths,
        total_size,
    })
}

// ── Android NDK Old Versions ─────────────────────────────────────

/// Number of Android NDK versions to preserve.
const ANDROID_NDK_KEEP: usize = 1;

/// Scan `~/Library/Android/sdk/ndk` for old NDK versions, keeping the
/// most recent. Each NDK version can be several GB.
fn scan_android_ndk_old_versions() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let ndk_dir = home.join("Library/Android/sdk/ndk");
    if !ndk_dir.is_dir() {
        return None;
    }

    let mut versions: Vec<(String, PathBuf)> = Vec::new();
    let entries = std::fs::read_dir(&ndk_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || path.is_symlink() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            versions.push((name.to_string(), path));
        }
    }

    if versions.len() <= ANDROID_NDK_KEEP {
        return None;
    }

    // Sort by version string descending (newest first)
    versions.sort_by(|a, b| version_compare(&b.0, &a.0));

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for (_name, path) in versions.into_iter().skip(ANDROID_NDK_KEEP) {
        let size = deletable_dir_size(&path);
        if size == 0 {
            continue;
        }
        paths.push(PathInfo {
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: true,
        });
        total_size += size;
    }

    if paths.is_empty() {
        return None;
    }

    crate::commands::shared::log_operation(
        "SCAN",
        "Android NDK (old versions)",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "dev_android_ndk_old".into(),
        category: "Developer Tools".into(),
        label: format!("Android NDK ({} old versions)", paths.len()),
        paths,
        total_size,
    })
}

// ── Orphaned App Data Detection ───────────────────────────────────────

/// Patterns that must never be flagged as orphaned (sensitive / system data).
/// Matched as substrings against the lowercased directory name.
const ORPHAN_NEVER_DELETE: &[&str] = &[
    // Password managers
    "1password", "bitwarden", "lastpass", "keepass", "dashlane", "enpass",
    "keychain", "ssh", "gpg", "gnupg", "security",
    // Apple system data
    "com.apple.",
    // Browsers (vendor + product names) — their user profiles live under
    // these directories and contain sessions, extensions, wallets, passwords.
    "google", "chrome", "chromium",
    "mozilla", "firefox",
    "bravesoftware", "brave",
    "microsoft edge", "microsoft",
    "company.thebrowser", "arc",
    "operasoftware", "opera",
    "vivaldi",
    "safari",
    "tor browser", "tor",
    // Crypto wallets (native apps / extension data locations)
    "metamask", "phantom", "rainbow", "trust wallet", "coinbase wallet",
    "exodus", "electrum", "ledger", "trezor", "wallet",
    // Common dev / secrets directories
    "aws", "gcloud", "azure", "kube", "docker",
];

/// Maximum number of orphaned items to return.
const MAX_ORPHANED_ITEMS: usize = 100;

/// Minimum age in days before an entry is considered orphaned.
const ORPHAN_MIN_AGE_DAYS: u64 = 30;

/// Minimum size in bytes — skip entries smaller than this.
const ORPHAN_MIN_SIZE: u64 = 1024; // 1 KB

/// Library subdirectories to scan for orphaned entries. Only regenerable
/// caches, logs and window state: app data folders may hold wallets,
/// message history or saves that outlive the app, so they are never
/// offered as leftovers.
const ORPHAN_SCAN_DIRS: &[&str] = &[
    "Library/Caches",
    "Library/Logs",
    "Library/Saved Application State",
];

/// Returns true if a `~/Library/Containers/<bundle>/` directory is a
/// containermanagerd-protected sandbox stub. macOS restores these on
/// next launch and attempting to remove them triggers an admin password
/// prompt that still ends in failure, so the orphan scanner must not
/// surface them.
fn is_protected_container_stub(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    path.join(".com.apple.containermanagerd.metadata.plist").exists()
}

/// Ask Spotlight whether any bundle with the given identifier exists
/// anywhere on disk. Used as a fallback for the orphan scanner so we
/// don't flag data belonging to an app installed outside the standard
/// `/Applications`, `/System/Applications`, and `~/Applications`
/// locations (e.g. command-line installers, homebrew casks pointed at
/// unusual prefixes, user-side `open` targets, mounted DMGs the user
/// runs directly). Returns `false` if Spotlight is disabled or the
/// command isn't available — the orphan scanner already applies other
/// safety gates, so a failed lookup just defaults to "not found".
fn mdfind_has_bundle_id(bundle_id: &str) -> bool {
    // Protect against shell metacharacters in the bundle ID by passing
    // it as a single argument via process::Command. mdfind's query
    // language uses single-quoted strings — embed the id verbatim.
    let query = format!("kMDItemCFBundleIdentifier == '{}'", bundle_id);
    let out = std::process::Command::new("/usr/bin/mdfind")
        .arg(&query)
        .output();
    match out {
        Ok(o) if o.status.success() => !o.stdout.is_empty(),
        _ => false,
    }
}

/// Collect CFBundleIdentifier values of .app bundles in `dir`, descending
/// `depth` levels into plain folders (`/Applications/Setapp`, `Utilities`,
/// vendor folders).
fn scan_apps_in_dir(dir: &Path, depth: usize, ids: &mut HashSet<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("app") {
            let plist_path = path.join("Contents/Info.plist");
            if let Ok(plist_val) = plist::Value::from_file(&plist_path) {
                if let Some(dict) = plist_val.as_dictionary() {
                    if let Some(id) = dict.get("CFBundleIdentifier").and_then(|v| v.as_string()) {
                        ids.insert(id.to_lowercase());
                    }
                }
            }
            continue;
        }
        if depth > 0 && entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            scan_apps_in_dir(&path, depth - 1, ids);
        }
    }
}

/// Bundle ids of every app bundle Spotlight has indexed, wherever it lives.
/// One query per scan, bounded so a busy index cannot stall scanning.
fn spotlight_app_bundle_ids() -> HashSet<String> {
    use std::io::Read;
    let mut ids = HashSet::new();
    let mut child = match std::process::Command::new("/usr/bin/mdfind")
        .args([
            "-attr",
            "kMDItemCFBundleIdentifier",
            "kMDItemContentType == 'com.apple.application-bundle'",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return ids,
    };
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    for line in reader.join().unwrap_or_default().lines() {
        if let Some((_, id)) = line.rsplit_once("kMDItemCFBundleIdentifier = ") {
            let id = id.trim();
            if !id.is_empty() && id != "(null)" {
                ids.insert(id.to_lowercase());
            }
        }
    }
    ids
}

/// Collect bundle IDs from all installed applications: the standard
/// Applications folders (two levels deep, so Setapp and vendor folders are
/// covered) plus everything Spotlight/LaunchServices has indexed.
fn collect_installed_bundle_ids() -> HashSet<String> {
    let mut ids = HashSet::new();
    for dir in ["/Applications", "/System/Applications"] {
        scan_apps_in_dir(Path::new(dir), 2, &mut ids);
    }
    if let Some(home) = dirs::home_dir() {
        scan_apps_in_dir(&home.join("Applications"), 2, &mut ids);
    }
    ids.extend(spotlight_app_bundle_ids());
    ids
}

/// Check if a directory entry name matches any installed bundle ID.
/// Returns true if the name looks like it could belong to an installed app.
///
/// This is intentionally permissive — a false positive just means we don't
/// flag the folder as orphaned, but a false negative could cause us to delete
/// live app data (browser profiles, extensions, crypto wallets, etc).
fn matches_installed_app(name_lower: &str, installed_ids: &HashSet<String>) -> bool {
    // Direct match: "com.google.chrome" == "com.google.chrome"
    if installed_ids.contains(name_lower) {
        return true;
    }

    for id in installed_ids {
        // Prefix match: "com.example" is an ancestor of "com.example.helper"
        if id.starts_with(name_lower) && id[name_lower.len()..].starts_with('.') {
            return true;
        }
        if name_lower.starts_with(id.as_str()) && name_lower[id.len()..].starts_with('.') {
            return true;
        }

        // Middle-component match: directory "Google" should match bundle
        // "com.google.Chrome" because "google" is a component of the bundle ID.
        // This catches vendor/product folders like "Google", "Mozilla",
        // "BraveSoftware", "Firefox" that don't map 1:1 to the bundle ID.
        for component in id.split('.') {
            if component.is_empty() || component == "com" || component == "org" || component == "net" || component == "io" {
                continue;
            }
            if component == name_lower {
                return true;
            }
            // Also match if the directory name contains the component
            // (e.g. "BraveSoftware" should match "brave")
            if name_lower.contains(component) || component.contains(name_lower) {
                return true;
            }
        }
    }

    false
}

/// Check if an entry name is in the never-delete list.
fn is_orphan_protected(name_lower: &str) -> bool {
    ORPHAN_NEVER_DELETE.iter().any(|pattern| name_lower.contains(pattern))
}

/// Returns true if the path's modification time is older than `ORPHAN_MIN_AGE_DAYS` days.
fn is_old_enough(path: &Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    age.as_secs() > ORPHAN_MIN_AGE_DAYS * 24 * 60 * 60
}

/// Returns true if a directory/file name looks like a reverse-DNS bundle ID
/// (e.g. `com.google.Chrome`, `org.mozilla.firefox`, `net.whatsapp.WhatsApp`).
///
/// This is the primary safety filter for the orphan scanner: we only consider
/// entries that look like bundle IDs as orphan candidates. Vendor directories
/// like `Google`, `Firefox`, `BraveSoftware`, `Arc` — which contain live user
/// data (extensions, crypto wallets, sessions) — are invisible to the scanner
/// because their names do not match any bundle-ID prefix.
fn looks_like_bundle_id(name_lower: &str) -> bool {
    // Must contain at least one dot (reverse-DNS format)
    if !name_lower.contains('.') {
        return false;
    }
    // Must start with a known reverse-DNS TLD prefix
    const BUNDLE_PREFIXES: &[&str] = &[
        "com.", "org.", "net.", "io.", "co.", "ai.", "dev.", "app.", "me.",
        "edu.", "gov.", "biz.", "info.", "xyz.", "tv.", "us.", "uk.", "de.",
        "fr.", "jp.", "cn.", "ru.", "it.", "es.", "nl.", "br.", "au.", "ca.",
        "ch.", "at.", "se.", "no.", "fi.", "pl.", "eu.",
    ];
    BUNDLE_PREFIXES.iter().any(|p| name_lower.starts_with(p))
}

/// Scan for orphaned application data from uninstalled apps.
/// Returns a list of ScanItems, one per orphaned entry found.
pub fn scan_orphaned_data() -> Vec<ScanItem> {
    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let installed_ids = collect_installed_bundle_ids();
    scan_orphaned_data_in(&home, &installed_ids, &settings.whitelist, &mdfind_has_bundle_id)
}

/// `scan_orphaned_data` against an explicit home, installed-app set and
/// Spotlight lookup.
pub(super) fn scan_orphaned_data_in(
    home: &Path,
    installed_ids: &HashSet<String>,
    whitelist: &[String],
    spotlight_has_bundle: &dyn Fn(&str) -> bool,
) -> Vec<ScanItem> {
    let mut items = Vec::new();

    'dirs: for subdir in ORPHAN_SCAN_DIRS {
        let scan_dir = home.join(subdir);
        let entries = match std::fs::read_dir(&scan_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            if items.len() >= MAX_ORPHANED_ITEMS {
                break 'dirs;
            }

            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            let name_lower = name.to_lowercase();

            // PRIMARY SAFETY: only consider entries that look like bundle IDs.
            // Vendor folders like "Google", "Firefox", "BraveSoftware" are
            // invisible to this scanner — they contain live user data and
            // must never be flagged as orphaned.
            let stripped = name_lower
                .trim_end_matches(".savedstate")
                .trim_end_matches(".binarycookies")
                .trim_end_matches(".plist");
            if !looks_like_bundle_id(stripped) {
                continue;
            }

            // Skip protected patterns (defense in depth)
            if is_orphan_protected(&name_lower) {
                continue;
            }

            // Browser and other irreplaceable data is never orphaned,
            // installed or not.
            if crate::commands::browser_guard::is_browser_bundle_id(&name)
                || crate::commands::data_guard::bundle_category(&name).is_some()
            {
                crate::commands::shared::log_operation(
                    "SCAN",
                    &path.to_string_lossy(),
                    "skipped: protected app data (orphan candidate)",
                );
                continue;
            }

            // Skip if it matches an installed app
            if matches_installed_app(&name_lower, installed_ids) {
                continue;
            }

            // Skip containermanagerd-protected sandbox stubs
            if is_protected_container_stub(&path) {
                crate::commands::shared::log_operation(
                    "SCAN",
                    &path.to_string_lossy(),
                    "skipped: containermanagerd-protected sandbox stub",
                );
                continue;
            }

            // Spotlight fallback: apps installed outside the three
            // standard Applications directories won't be in our
            // `installed_ids` set. Before flagging their data as
            // orphaned, ask mdfind whether any bundle with this ID
            // exists anywhere on disk. `stripped` already has the
            // `.plist` / `.savedstate` / `.binarycookies` suffixes
            // removed so it's the pure bundle ID.
            if spotlight_has_bundle(stripped) {
                continue;
            }

            // Skip whitelisted paths
            let path_str = path.to_string_lossy().to_string();
            if is_whitelisted(&path_str, whitelist) {
                crate::commands::shared::log_operation(
                    "SCAN",
                    &path_str,
                    "skipped: on user whitelist (orphan candidate)",
                );
                continue;
            }

            // Must be old enough
            if !is_old_enough(&path) {
                continue;
            }

            // Anything holding protected data stays, whoever owns it; a
            // tree too big to inspect stays too.
            let scan = crate::commands::data_guard::scan_tree(&path, 5, 4_000);
            if scan != crate::commands::data_guard::TreeScan::Clean
                || crate::commands::browser_guard::contains_profile_markers(&path, 5)
            {
                crate::commands::shared::log_operation(
                    "SCAN",
                    &path_str,
                    &format!("skipped: {:?} (orphan candidate)", scan),
                );
                continue;
            }

            // Calculate size, skip tiny entries
            let size = if path.is_dir() {
                deletable_dir_size(&path)
            } else {
                path.metadata().map(|m| m.len()).unwrap_or(0)
            };

            if size < ORPHAN_MIN_SIZE {
                continue;
            }

            let rule_id = format!("orphan_{}", name_lower.replace('.', "_"));

            crate::commands::shared::log_operation(
                "SCAN",
                &name,
                &format!("{} bytes (orphaned)", size),
            );

            items.push(ScanItem {
                rule_id,
                category: "Orphaned Data".to_string(),
                label: name,
                paths: vec![PathInfo {
                    path: path_str,
                    size,
                    is_dir: path.is_dir(),
                }],
                total_size: size,
            });
        }
    }

    drop_guard_refused_paths(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{canon, mkdir, s, set_age_days, workspace_tempdir, write_file};
    use std::os::unix::fs::symlink;

    fn rule(paths: &[String], max_age_days: Option<u32>) -> CleanRule {
        CleanRule {
            id: "test_rule".into(),
            category: "Test".into(),
            label: "Test".into(),
            paths: paths.to_vec(),
            max_age_days,
        }
    }

    #[test]
    fn rule_collects_existing_non_empty_paths_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(&root.join("cache/a.bin"), 2_000);
        write_file(&root.join("single.log"), 300);
        mkdir(&root.join("empty"));

        let r = rule(
            &[
                s(&root.join("cache")),
                s(&root.join("single.log")),
                s(&root.join("empty")),
                s(&root.join("missing")),
            ],
            None,
        );
        let item = collect_rule_paths(&r, &[]).unwrap();
        let got: Vec<&str> = item.paths.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(got, vec![s(&root.join("cache")), s(&root.join("single.log"))]);
        assert!(item.paths[0].is_dir);
        assert!(!item.paths[1].is_dir);
        assert_eq!(item.paths[1].size, 300);
        assert_eq!(item.total_size, item.paths.iter().map(|p| p.size).sum::<u64>());
        assert_eq!(item.rule_id, "test_rule");
    }

    fn item_with(rule_id: &str, paths: &[(String, u64)]) -> ScanItem {
        ScanItem {
            rule_id: rule_id.into(),
            category: "Test".into(),
            label: "Test".into(),
            paths: paths
                .iter()
                .map(|(p, size)| PathInfo { path: p.clone(), size: *size, is_dir: true })
                .collect(),
            total_size: paths.iter().map(|(_, size)| size).sum(),
        }
    }

    #[test]
    fn scanner_never_reports_paths_the_executor_refuses() {
        let ok = workspace_tempdir();
        let ok_path = s(&canon(&ok).join("cache"));
        write_file(&canon(&ok).join("cache/a.bin"), 10);
        let refused_tmp = tempfile::tempdir().unwrap();
        let refused = [
            s(refused_tmp.path()),
            "/private/tmp/kyra-test".to_string(),
            "/private/var/log".to_string(),
            "/usr/local/var/homebrew/locks".to_string(),
            "/System/Library/Caches".to_string(),
            "/Users/x/../../System".to_string(),
        ];

        let mut mixed: Vec<(String, u64)> = refused.iter().map(|p| (p.clone(), 1_000)).collect();
        mixed.push((ok_path.clone(), 10));
        let items = vec![
            item_with("mixed", &mixed),
            item_with("all_refused", &refused.iter().map(|p| (p.clone(), 5)).collect::<Vec<_>>()),
            item_with("special_tm_local_snapshots", &[("tmutil://com.apple.TimeMachine.2024-01-01-000000.local".into(), 7)]),
        ];

        let kept = drop_guard_refused_paths(items);
        let ids: Vec<&str> = kept.iter().map(|i| i.rule_id.as_str()).collect();
        assert_eq!(ids, vec!["mixed", "special_tm_local_snapshots"]);
        assert_eq!(kept[0].paths.len(), 1);
        assert_eq!(kept[0].paths[0].path, ok_path);
        assert_eq!(kept[0].total_size, 10);
        for item in &kept {
            if uses_pseudo_paths(&item.rule_id) {
                continue;
            }
            assert!(item.paths.iter().all(|p| is_safe_path(&p.path)));
        }
    }

    #[test]
    fn rule_with_nothing_on_disk_yields_no_item() {
        let dir = tempfile::tempdir().unwrap();
        let r = rule(&[s(&dir.path().join("nothing"))], None);
        assert!(collect_rule_paths(&r, &[]).is_none());
    }

    #[test]
    fn whitelisted_rule_paths_are_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(&root.join("a/x.bin"), 100);
        write_file(&root.join("b/x.bin"), 100);
        let r = rule(&[s(&root.join("a")), s(&root.join("b"))], None);
        let item = collect_rule_paths(&r, &[s(&root.join("a"))]).unwrap();
        assert_eq!(item.paths.len(), 1);
        assert_eq!(item.paths[0].path, s(&root.join("b")));
    }

    #[test]
    fn age_filter_only_picks_old_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("logs");
        write_file(&root.join("old.log"), 100);
        write_file(&root.join("new.log"), 100);
        write_file(&root.join("olddir/x.log"), 100);
        set_age_days(&root.join("old.log"), 30);
        set_age_days(&root.join("olddir"), 30);
        let outside = tempfile::tempdir().unwrap();
        write_file(&outside.path().join("t.bin"), 100);
        set_age_days(&outside.path().join("t.bin"), 30);
        symlink(outside.path().join("t.bin"), root.join("old_link")).unwrap();

        let r = rule(&[s(&root)], Some(7));
        let item = collect_rule_paths(&r, &[s(&root.join("olddir"))]).unwrap();
        let got: Vec<&str> = item.paths.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(got, vec![s(&root.join("old.log"))]);

        let (all_old, _) = scan_with_age_filter(&root, 7);
        let mut names: Vec<String> = all_old.iter().map(|p| p.path.clone()).collect();
        names.sort();
        assert_eq!(names, vec![s(&root.join("old.log")), s(&root.join("olddir"))]);
    }

    #[test]
    fn glob_expands_directory_and_file_wildcards() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        mkdir(&root.join("Profiles/p1/cache2"));
        mkdir(&root.join("Profiles/p2"));
        write_file(&root.join("Downloads/a.part"), 1);
        write_file(&root.join("Downloads/b.zip"), 1);
        write_file(&root.join("Downloads/c.part"), 1);

        let mut dirs_found = expand_home_glob(&format!("{}/Profiles/*/cache2", s(root)));
        dirs_found.sort();
        assert_eq!(
            dirs_found,
            vec![root.join("Profiles/p1/cache2"), root.join("Profiles/p2/cache2")]
        );

        let mut parts = expand_home_glob(&format!("{}/Downloads/*.part", s(root)));
        parts.sort();
        assert_eq!(parts, vec![root.join("Downloads/a.part"), root.join("Downloads/c.part")]);

        assert!(expand_home_glob(&format!("{}/missing/*/x", s(root))).is_empty());
        assert_eq!(expand_home_glob("/plain/path"), vec![PathBuf::from("/plain/path")]);
    }

    #[test]
    fn home_expansion_only_touches_leading_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home("~/Library/Caches"), Some(home.join("Library/Caches")));
        assert_eq!(expand_home("~"), Some(home.clone()));
        assert_eq!(expand_home("/tmp/~/x"), Some(PathBuf::from("/tmp/~/x")));
        assert_eq!(expand_home_glob("~"), vec![home]);
    }

    #[test]
    fn scanner_whitelist_is_component_aware() {
        let wl = vec!["/a/b".to_string()];
        assert!(is_whitelisted("/a/b", &wl));
        assert!(is_whitelisted("/a/b/c", &wl));
        assert!(!is_whitelisted("/a/bc", &wl));
    }

    #[test]
    fn version_compare_is_numeric_per_segment() {
        use std::cmp::Ordering::*;
        assert_eq!(version_compare("1.10.0", "1.9.9"), Greater);
        assert_eq!(version_compare("120.0.6099.1", "120.0.6099.1"), Equal);
        assert_eq!(version_compare("1.2", "1.2.0"), Equal);
        assert_eq!(version_compare("1.2", "1.2.1"), Less);
        assert_eq!(version_compare("1.0-beta", "1.0-alpha"), Greater);
    }

    #[test]
    fn mounted_volume_detection_covers_subpaths() {
        let mounts = vec!["/Library/Developer/CoreSimulator/Volumes/iOS_21A/mnt".to_string()];
        assert!(is_path_mounted(Path::new("/Library/Developer/CoreSimulator/Volumes/iOS_21A"), &mounts));
        assert!(!is_path_mounted(Path::new("/Library/Developer/CoreSimulator/Volumes/iOS_2"), &mounts));
        assert!(!is_path_mounted(Path::new("/Library/Developer/CoreSimulator/Volumes/iOS_22B"), &mounts));
    }

    #[test]
    fn bundle_id_shape_is_required_for_orphan_candidates() {
        assert!(looks_like_bundle_id("com.example.app"));
        assert!(looks_like_bundle_id("io.github.tool"));
        assert!(!looks_like_bundle_id("google"));
        assert!(!looks_like_bundle_id("firefox"));
        assert!(!looks_like_bundle_id("my.folder"));
        assert!(!looks_like_bundle_id("com"));
    }

    #[test]
    fn installed_app_matching_is_permissive() {
        let ids: HashSet<String> = ["com.google.chrome", "com.bravesoftware.brave-browser"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(matches_installed_app("com.google.chrome", &ids));
        assert!(matches_installed_app("com.google.chrome.helper", &ids));
        assert!(matches_installed_app("com.google", &ids));
        assert!(matches_installed_app("google", &ids));
        assert!(matches_installed_app("bravesoftware", &ids));
        assert!(!matches_installed_app("com.unrelated.tool", &ids));
    }

    #[test]
    fn sensitive_names_are_never_orphans() {
        for n in ["com.1password.1password", "com.agilebits.keychain", "org.mozilla.firefox", "io.metamask", "com.apple.dt", "com.docker.docker"] {
            assert!(is_orphan_protected(n), "{n}");
        }
        assert!(!is_orphan_protected("com.example.pdfviewer"));
    }

    #[test]
    fn orphan_age_gate_uses_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("fresh");
        let old = dir.path().join("old");
        mkdir(&fresh);
        mkdir(&old);
        set_age_days(&old, ORPHAN_MIN_AGE_DAYS + 1);
        assert!(!is_old_enough(&fresh));
        assert!(is_old_enough(&old));
        assert!(!is_old_enough(&dir.path().join("missing")));
    }

    #[test]
    fn container_stub_needs_marker_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = dir.path().join("com.foo");
        mkdir(&c);
        assert!(!is_protected_container_stub(&c));
        write_file(&c.join(".com.apple.containermanagerd.metadata.plist"), 1);
        assert!(is_protected_container_stub(&c));
    }

    #[test]
    fn a_path_found_by_two_rules_is_listed_once() {
        let items = vec![
            item_with("user_logs", &[("/h/Library/Logs/CoreSimulator".into(), 300), ("/h/Library/Logs/.DS_Store".into(), 6)]),
            item_with("dev_xcode_sim_logs", &[("/h/Library/Logs/CoreSimulator".into(), 300), ("/h/Library/Developer/CoreSimulator/Caches".into(), 50)]),
            item_with("ds_store", &[("/h/Library/Logs/.DS_Store".into(), 6)]),
            item_with("sys_coredevice_cache", &[("/h/C/Data/Library/Caches".into(), 70)]),
            item_with("dynamic_container_caches", &[("/h/C/Data/Library/Caches".into(), 70), ("/h/D/Data/tmp".into(), 9)]),
        ];
        let out = drop_duplicate_paths(items);
        let ids: Vec<&str> = out.iter().map(|i| i.rule_id.as_str()).collect();
        assert_eq!(ids, ["user_logs", "dev_xcode_sim_logs", "sys_coredevice_cache", "dynamic_container_caches"]);
        assert_eq!(out[1].total_size, 50);
        assert_eq!(out[3].total_size, 9);
        assert_eq!(out.iter().map(|i| i.total_size).sum::<u64>(), 300 + 6 + 50 + 70 + 9);
    }

    #[test]
    fn container_size_leaves_out_what_clearing_it_skips() {
        let dir = workspace_tempdir();
        let caches = canon(&dir).join("Caches");
        write_file(&caches.join("com.example.app/blob.bin"), 40_000);
        write_file(&caches.join("loose.bin"), 1_000);
        write_file(&caches.join("IndexedDB/keep.bin"), 90_000);
        let outside = workspace_tempdir();
        write_file(&canon(&outside).join("big.bin"), 200_000);
        symlink(canon(&outside), caches.join("link")).unwrap();

        let size = container_deletable_size(&caches);
        assert_eq!(size, deletable_dir_size(&caches.join("com.example.app")) + 1_000);
    }
}
