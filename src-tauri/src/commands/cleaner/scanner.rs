use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{CleanRule, PathInfo, ScanItem};
use crate::commands::utils::deletable_dir_size;

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

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;
    for path in candidates {
        let path_str = path.to_string_lossy().to_string();
        if is_whitelisted(&path_str, &settings.whitelist) {
            continue;
        }
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        if size < MIN_VM_IMAGE_SIZE {
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

/// Chromium-based browsers that keep versioned framework snapshots
/// under `~/Library/Application Support/<root>/Snapshots/<version>/`.
/// Only the most recent snapshot is actively used by the running
/// browser; older ones are retained for rollback and crashpad
/// symbolication.
const BROWSER_SNAPSHOT_ROOTS: &[&str] = &[
    "Google/Chrome",
    "Google/Chrome Canary",
    "Google/Chrome Beta",
    "Google/Chrome Dev",
    "Microsoft Edge",
    "Microsoft Edge Beta",
    "Microsoft Edge Dev",
    "Microsoft Edge Canary",
    "BraveSoftware/Brave-Browser",
    "BraveSoftware/Brave-Browser-Beta",
    "BraveSoftware/Brave-Browser-Nightly",
    "Chromium",
    "Vivaldi",
    "com.operasoftware.Opera",
    "Arc",
];

/// Number of browser framework snapshots to preserve per browser.
const BROWSER_SNAPSHOT_KEEP: usize = 1;

/// Special scan: enumerate `Snapshots/<version>/` directories under
/// each known Chromium-based browser's profile root and flag all but
/// the most recent for deletion.
fn scan_browser_old_snapshots() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let app_support = home.join("Library/Application Support");

    let mut paths: Vec<PathInfo> = Vec::new();
    let mut total_size: u64 = 0;

    for rel in BROWSER_SNAPSHOT_ROOTS {
        let snapshots_dir = app_support.join(rel).join("Snapshots");
        if !snapshots_dir.is_dir() {
            continue;
        }

        let mut versions: Vec<(PathBuf, SystemTime)> = Vec::new();
        let entries = match std::fs::read_dir(&snapshots_dir) {
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

        versions.sort_by(|a, b| b.1.cmp(&a.1));

        for (path, _) in versions.into_iter().skip(BROWSER_SNAPSHOT_KEEP) {
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
        "Browser framework snapshots (old)",
        &format!("{} bytes ({} paths)", total_size, paths.len()),
    );

    Some(ScanItem {
        rule_id: "browser_old_snapshots".into(),
        category: "Browsers".into(),
        label: "Browser framework snapshots (old)".into(),
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

/// Returns true if `path` (or any parent) is covered by a whitelisted entry.
fn is_whitelisted(path: &str, whitelist: &[String]) -> bool {
    whitelist.iter().any(|w| path == w || path.starts_with(&format!("{}/", w)))
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

/// Scans the filesystem for items matching the given rules.
/// Returns only rules that have at least one existing path with non-zero size.
pub fn scan_rules(rules: &[CleanRule]) -> Vec<ScanItem> {
    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    let mut results = Vec::new();

    for rule in rules {
        let mut found_paths = Vec::new();
        let mut total_size: u64 = 0;

        for raw_path in &rule.paths {
            let expanded = match expand_home(raw_path) {
                Some(p) => p,
                None => continue,
            };

            if !expanded.exists() {
                continue;
            }

            // Skip whitelisted paths
            let expanded_str = expanded.to_string_lossy().to_string();
            if is_whitelisted(&expanded_str, &settings.whitelist) {
                continue;
            }

            if let Some(max_age_days) = rule.max_age_days {
                // Age-filtered scanning: only include files older than the threshold
                if expanded.is_dir() {
                    let (old_paths, _old_total) = scan_with_age_filter(&expanded, max_age_days);
                    let before_count = found_paths.len();
                    for p in old_paths {
                        if !is_whitelisted(&p.path, &settings.whitelist) {
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
                let size = if expanded.is_dir() {
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

        if !found_paths.is_empty() {
            crate::commands::shared::log_operation(
                "SCAN",
                &rule.label,
                &format!("{} bytes ({} paths)", total_size, found_paths.len()),
            );
            results.push(ScanItem {
                rule_id: rule.id.clone(),
                category: rule.category.clone(),
                label: rule.label.clone(),
                paths: found_paths,
                total_size,
            });
        }
    }

    // Special scans (not covered by standard rules)
    if let Some(ds_store) = scan_ds_store_files() {
        results.push(ds_store);
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
    if let Some(jb_old) = scan_jetbrains_toolbox_old_builds() {
        results.push(jb_old);
    }
    if let Some(browser_snaps) = scan_browser_old_snapshots() {
        results.push(browser_snaps);
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

    results
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

/// Library subdirectories to scan for orphaned entries.
const ORPHAN_SCAN_DIRS: &[&str] = &[
    "Library/Application Support",
    "Library/Caches",
    "Library/Preferences",
    "Library/Preferences/ByHost",
    "Library/Saved Application State",
    "Library/WebKit",
    "Library/HTTPStorages",
    "Library/Containers",
    "Library/Group Containers",
    "Library/LaunchAgents",
    "Library/Logs",
    "Library/Cookies",
    "Library/Internet Plug-Ins",
    "Library/Autosave Information",
    "Library/Application Scripts",
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

/// Scan .app bundles in a directory and extract their CFBundleIdentifier values.
fn scan_apps_in_dir(dir: &Path, ids: &mut HashSet<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("app") {
            continue;
        }
        let plist_path = path.join("Contents/Info.plist");
        if let Ok(plist_val) = plist::Value::from_file(&plist_path) {
            if let Some(dict) = plist_val.as_dictionary() {
                if let Some(id) = dict.get("CFBundleIdentifier").and_then(|v| v.as_string()) {
                    ids.insert(id.to_lowercase());
                }
            }
        }
    }
}

/// Collect bundle IDs from all installed applications.
fn collect_installed_bundle_ids() -> HashSet<String> {
    let mut ids = HashSet::new();
    let system_dirs = ["/Applications", "/System/Applications"];

    for dir in &system_dirs {
        scan_apps_in_dir(Path::new(dir), &mut ids);
    }

    if let Some(home) = dirs::home_dir() {
        scan_apps_in_dir(&home.join("Applications"), &mut ids);
    }

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
    let installed_ids = collect_installed_bundle_ids();

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let mut items = Vec::new();

    for subdir in ORPHAN_SCAN_DIRS {
        let scan_dir = home.join(subdir);
        let entries = match std::fs::read_dir(&scan_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            if items.len() >= MAX_ORPHANED_ITEMS {
                return items;
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

            // Skip if it matches an installed app
            if matches_installed_app(&name_lower, &installed_ids) {
                continue;
            }

            // Skip containermanagerd-protected sandbox stubs
            if is_protected_container_stub(&path) {
                continue;
            }

            // Spotlight fallback: apps installed outside the three
            // standard Applications directories won't be in our
            // `installed_ids` set. Before flagging their data as
            // orphaned, ask mdfind whether any bundle with this ID
            // exists anywhere on disk. `stripped` already has the
            // `.plist` / `.savedstate` / `.binarycookies` suffixes
            // removed so it's the pure bundle ID.
            if mdfind_has_bundle_id(stripped) {
                continue;
            }

            // Skip whitelisted paths
            let path_str = path.to_string_lossy().to_string();
            if is_whitelisted(&path_str, &settings.whitelist) {
                continue;
            }

            // Must be old enough
            if !is_old_enough(&path) {
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

    items
}
