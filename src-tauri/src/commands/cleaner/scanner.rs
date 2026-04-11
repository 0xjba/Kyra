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
fn scan_ds_store_files() -> Option<ScanItem> {
    let home = dirs::home_dir()?;
    let mut paths = Vec::new();
    let mut total_size = 0u64;
    let mut count = 0usize;
    let max_files = 500;

    let skip_dirs: &[&str] = &[
        ".Trash",
        "node_modules",
        ".git",
        "Library/Caches",
        "Library/Developer",
        ".npm",
        "target",
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
    "Library/Saved Application State",
    "Library/WebKit",
    "Library/HTTPStorages",
];

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
