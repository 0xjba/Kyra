use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::AssociatedFile;
use crate::commands::utils::path_size;

/// Directories under ~/Library to search, with human-readable category names.
const SEARCH_LOCATIONS: &[(&str, &str)] = &[
    ("Application Support", "App Data"),
    ("Preferences", "Preferences"),
    ("Caches", "Caches"),
    ("Containers", "Containers"),
    ("Group Containers", "Group Containers"),
    ("Logs", "Logs"),
    ("Saved Application State", "Saved State"),
    ("WebKit", "WebKit Data"),
    ("HTTPStorages", "HTTP Storage"),
    ("Cookies", "Cookies"),
    ("Internet Plug-Ins", "Plug-ins"),
    ("Input Methods", "Plug-ins"),
];

/// Checks if a directory entry name matches the bundle ID or app name.
fn matches_app(entry_name: &str, bundle_id: &str, app_name: &str) -> bool {
    let entry_lower = entry_name.to_lowercase();
    let bundle_lower = bundle_id.to_lowercase();
    let name_lower = app_name.to_lowercase();

    // Exact bundle ID match (most reliable)
    if !bundle_id.is_empty() && entry_lower == bundle_lower {
        return true;
    }

    // Bundle ID prefix match (e.g., "com.example.app.helper")
    if !bundle_id.is_empty() && entry_lower.starts_with(&format!("{}.", bundle_lower)) {
        return true;
    }

    // App name match (for directories named after the app)
    if !app_name.is_empty() && entry_lower == name_lower {
        return true;
    }

    false
}

/// Validates a bundle identifier so it can be safely used as a filesystem
/// glob component. Only alphanumerics, dots, hyphens, and underscores are
/// allowed — this prevents wildcard or path-separator injection when we
/// look up BOM receipts by bundle id.
fn is_valid_bundle_id(bundle_id: &str) -> bool {
    !bundle_id.is_empty()
        && bundle_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Returns true if the given absolute path is a location we consider safe
/// to surface as a removable receipt-listed file.
fn is_receipt_path_safe(path: &str) -> bool {
    // Hard blocks — system locations, package managers, private trees.
    const BLOCKED_PREFIXES: &[&str] = &[
        "/System/", "/usr/bin/", "/usr/lib/", "/usr/sbin/", "/bin/", "/sbin/",
        "/private/", "/Library/Extensions/",
    ];
    for blocked in BLOCKED_PREFIXES {
        if path.starts_with(blocked) {
            return false;
        }
    }

    // Whitelisted roots — anything outside this list is ignored.
    const ALLOWED_PREFIXES: &[&str] = &[
        "/Applications/",
        "/Library/Application Support/",
        "/Library/Caches/",
        "/Library/Logs/",
        "/Library/Preferences/",
        "/Library/LaunchAgents/",
        "/Library/LaunchDaemons/",
        "/Library/PrivilegedHelperTools/",
    ];
    if !ALLOWED_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return false;
    }

    // Never surface the top-level container directories themselves.
    matches!(path, "/Applications" | "/Library") == false
}

/// Categorise a receipt-listed path by which whitelisted root it falls under.
fn categorize_receipt_path(path: &str) -> &'static str {
    if path.starts_with("/Applications/") {
        "Application"
    } else if path.starts_with("/Library/Application Support/") {
        "App Data"
    } else if path.starts_with("/Library/Caches/") {
        "Caches"
    } else if path.starts_with("/Library/Logs/") {
        "Logs"
    } else if path.starts_with("/Library/Preferences/") {
        "Preferences"
    } else if path.starts_with("/Library/LaunchAgents/")
        || path.starts_with("/Library/LaunchDaemons/")
    {
        "Launch Agents"
    } else if path.starts_with("/Library/PrivilegedHelperTools/") {
        "Launch Daemons"
    } else {
        "App Data"
    }
}

/// Discovers files installed by a package receipt (.bom) matching the given
/// bundle id. Reads `/var/db/receipts/<bundle_id>*.bom` via `lsbom -f -s`
/// and filters the results through `is_receipt_path_safe` so only paths in
/// known user-owned locations are surfaced.
fn find_receipt_files(bundle_id: &str) -> Vec<AssociatedFile> {
    if !is_valid_bundle_id(bundle_id) {
        return vec![];
    }

    // /var/db/receipts is a symlink to /private/var/db/receipts on macOS;
    // either works. We use the canonical form to avoid traversing symlinks
    // into /private at safety-check time.
    let receipts_dir = Path::new("/private/var/db/receipts");
    if !receipts_dir.exists() {
        return vec![];
    }

    let entries = match fs::read_dir(receipts_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let prefix = bundle_id;
    let mut bom_files: Vec<PathBuf> = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(prefix) && name.ends_with(".bom") {
            bom_files.push(entry.path());
        }
    }

    if bom_files.is_empty() {
        return vec![];
    }

    let mut results: Vec<AssociatedFile> = Vec::new();

    for bom in bom_files {
        let output = match Command::new("/usr/bin/lsbom")
            .arg("-f")
            .arg("-s")
            .arg(&bom)
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let raw = line.trim();
            if raw.is_empty() {
                continue;
            }

            // Strip leading "./" if present and ensure absolute.
            let without_dot = raw.strip_prefix('.').unwrap_or(raw);
            let absolute = if without_dot.starts_with('/') {
                without_dot.to_string()
            } else {
                format!("/{}", without_dot)
            };

            // Traversal and control-char defense.
            if absolute.contains("..") {
                continue;
            }
            if absolute.chars().any(|c| c.is_control()) {
                continue;
            }

            // Collapse duplicate slashes.
            let normalized = {
                let mut out = String::with_capacity(absolute.len());
                let mut prev_slash = false;
                for c in absolute.chars() {
                    if c == '/' {
                        if !prev_slash {
                            out.push(c);
                        }
                        prev_slash = true;
                    } else {
                        out.push(c);
                        prev_slash = false;
                    }
                }
                out
            };

            if !is_receipt_path_safe(&normalized) {
                continue;
            }

            let path = Path::new(&normalized);
            if !path.exists() {
                continue;
            }

            let size = path_size(path);
            if size == 0 {
                continue;
            }

            if results.iter().any(|r| r.path == normalized) {
                continue;
            }

            results.push(AssociatedFile {
                path: normalized.clone(),
                category: categorize_receipt_path(&normalized).to_string(),
                size,
                is_dir: path.is_dir(),
            });
        }
    }

    results
}

/// Searches ~/Library subdirectories for files associated with the given app.
pub fn find_associated(bundle_id: &str, app_name: &str, _app_path: &str) -> Vec<AssociatedFile> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let library = home.join("Library");
    let mut results = Vec::new();

    for (dir_name, category) in SEARCH_LOCATIONS {
        let search_dir = library.join(dir_name);
        if !search_dir.exists() {
            continue;
        }

        let entries = match fs::read_dir(&search_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if !matches_app(&entry_name, bundle_id, app_name) {
                continue;
            }

            let path = entry.path();
            let size = path_size(&path);
            if size == 0 {
                continue;
            }

            results.push(AssociatedFile {
                path: path.to_string_lossy().to_string(),
                category: category.to_string(),
                size,
                is_dir: path.is_dir(),
            });
        }
    }

    // Scan LaunchAgents, LaunchDaemons, and PrivilegedHelperTools
    if !bundle_id.is_empty() {
        let launch_dirs: Vec<(std::path::PathBuf, &str)> = vec![
            (home.join("Library/LaunchAgents"), "Launch Agents"),
            (std::path::PathBuf::from("/Library/LaunchAgents"), "Launch Agents"),
            (std::path::PathBuf::from("/Library/LaunchDaemons"), "Launch Daemons"),
            (std::path::PathBuf::from("/Library/PrivilegedHelperTools"), "Launch Daemons"),
        ];

        let bundle_lower = bundle_id.to_lowercase();

        for (dir, category) in &launch_dirs {
            if !dir.exists() {
                continue;
            }
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let entry_name = entry.file_name().to_string_lossy().to_lowercase();
                if entry_name.contains(&bundle_lower) {
                    let path = entry.path();
                    let size = path_size(&path);
                    if size == 0 {
                        continue;
                    }
                    let path_str = path.to_string_lossy().to_string();
                    if !results.iter().any(|r| r.path == path_str) {
                        results.push(AssociatedFile {
                            path: path_str,
                            category: category.to_string(),
                            size,
                            is_dir: path.is_dir(),
                        });
                    }
                }
            }
        }
    }

    // Package receipt discovery — finds files installed by .pkg installers
    // that wouldn't otherwise be caught by bundle-id search under ~/Library.
    for receipt_file in find_receipt_files(bundle_id) {
        if !results.iter().any(|r| r.path == receipt_file.path) {
            results.push(receipt_file);
        }
    }

    // Also check ~/Library/Preferences for .plist files matching bundle ID
    if !bundle_id.is_empty() {
        let prefs_dir = library.join("Preferences");
        if prefs_dir.exists() {
            let plist_name = format!("{}.plist", bundle_id);
            let plist_path = prefs_dir.join(&plist_name);
            if plist_path.exists() {
                let size = plist_path.metadata().map(|m| m.len()).unwrap_or(0);
                // Avoid duplicates — only add if not already found
                if size > 0 && !results.iter().any(|r| r.path == plist_path.to_string_lossy().as_ref()) {
                    results.push(AssociatedFile {
                        path: plist_path.to_string_lossy().to_string(),
                        category: "Preferences".to_string(),
                        size,
                        is_dir: false,
                    });
                }
            }
        }
    }

    results
}
