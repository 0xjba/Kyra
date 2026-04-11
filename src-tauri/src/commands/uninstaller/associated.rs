use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::AssociatedFile;
use crate::commands::utils::path_size;

/// Directories under ~/Library to search, with human-readable category names.
const SEARCH_LOCATIONS: &[(&str, &str)] = &[
    ("Application Support", "App Data"),
    ("Application Scripts", "App Data"),
    ("Autosave Information", "App Data"),
    ("SyncedPreferences", "Preferences"),
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
    ("QuickLook", "Plug-ins"),
    ("PreferencePanes", "Plug-ins"),
    ("Services", "Plug-ins"),
    ("Screen Savers", "Plug-ins"),
    ("Frameworks", "Frameworks"),
    ("Spotlight", "Plug-ins"),
    ("ColorPickers", "Plug-ins"),
    ("Workflows", "Plug-ins"),
    ("Address Book Plug-Ins", "Plug-ins"),
    ("Accessibility", "Plug-ins"),
    ("Contextual Menu Items", "Plug-ins"),
];

/// Generate the set of lowercase naming variants for an app name, covering
/// the common inconsistencies between UI display name and on-disk folder
/// name. e.g. "Maestro Studio" produces {"maestro studio", "maestrostudio",
/// "maestro-studio", "maestro_studio"}. Also strips common version/channel
/// suffixes ("Nightly", "Beta", "Developer Edition", …) to add a base-name
/// variant — a "Zed Nightly" folder is often "zed" under ~/.config.
fn app_name_variants(app_name: &str) -> Vec<String> {
    if app_name.is_empty() {
        return Vec::new();
    }
    let mut variants: Vec<String> = Vec::new();
    let lower = app_name.to_lowercase();
    variants.push(lower.clone());
    variants.push(lower.replace(' ', ""));
    variants.push(lower.replace(' ', "-"));
    variants.push(lower.replace(' ', "_"));

    // Strip common version/channel suffixes for a base-name match.
    const VERSION_SUFFIXES: &[&str] = &[
        " nightly",
        " beta",
        " alpha",
        " dev",
        " canary",
        " preview",
        " insider",
        " edge",
        " stable",
        " release",
        " rc",
        " lts",
        " developer edition",
        " technology preview",
    ];
    for suffix in VERSION_SUFFIXES {
        if let Some(stripped) = lower.strip_suffix(suffix) {
            if stripped.len() >= 3 {
                variants.push(stripped.to_string());
                variants.push(stripped.replace(' ', ""));
                variants.push(stripped.replace(' ', "-"));
            }
            break;
        }
    }

    variants.sort();
    variants.dedup();
    variants.retain(|v| !v.is_empty());
    variants
}

/// Checks if a directory entry name matches the bundle ID or any of the
/// pre-computed app-name variants.
fn matches_app(entry_name: &str, bundle_id: &str, app_name_variants: &[String]) -> bool {
    let entry_lower = entry_name.to_lowercase();
    let bundle_lower = bundle_id.to_lowercase();

    // Exact bundle ID match (most reliable)
    if !bundle_id.is_empty() && entry_lower == bundle_lower {
        return true;
    }

    // Bundle ID prefix match (e.g., "com.example.app.helper")
    if !bundle_id.is_empty() && entry_lower.starts_with(&format!("{}.", bundle_lower)) {
        return true;
    }

    // App name variant match (handles space/hyphen/underscore/nospace
    // variations and stripped version-channel suffixes).
    for variant in app_name_variants {
        if entry_lower == *variant {
            return true;
        }
        // Some apps name their folder "<variant>.app" or "<variant>.workflow"
        // — strip a single extension for comparison.
        if let Some(dot) = entry_lower.rfind('.') {
            if &entry_lower[..dot] == variant {
                return true;
            }
        }
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

/// Discover files that well-known IDEs and development toolchains drop
/// outside the standard ~/Library tree — things we'd never find through a
/// generic bundle-id / app-name sweep because the folder names don't
/// correspond to either. Each branch is gated on a specific
/// bundle-id or app-name match so nothing runs unless we're actually
/// uninstalling that toolchain.
fn find_toolchain_specific(
    bundle_id: &str,
    app_name: &str,
    home: &Path,
) -> Vec<AssociatedFile> {
    let mut out: Vec<AssociatedFile> = Vec::new();
    let bundle_lower = bundle_id.to_lowercase();
    let name_lower = app_name.to_lowercase();

    // Helper to push a path if it exists and is non-empty.
    let push_path = |path: PathBuf, category: &str, results: &mut Vec<AssociatedFile>| {
        if !path.exists() {
            return;
        }
        let size = path_size(&path);
        if size == 0 {
            return;
        }
        let path_str = path.to_string_lossy().to_string();
        if results.iter().any(|r| r.path == path_str) {
            return;
        }
        results.push(AssociatedFile {
            path: path_str,
            category: category.to_string(),
            size,
            is_dir: path.is_dir(),
        });
    };

    // Huawei DevEco-Studio (HarmonyOS)
    if name_lower.contains("deveco") || bundle_lower.contains("huawei.deveco") {
        for rel in &[
            "DevEcoStudioProjects",
            "DevEco-Studio",
            "Library/Application Support/Huawei",
            "Library/Caches/Huawei",
            "Library/Logs/Huawei",
            "Library/Huawei",
            "Huawei",
            "HarmonyOS",
            ".huawei",
            ".ohos",
        ] {
            push_path(home.join(rel), "App Data", &mut out);
        }
    }

    // Android Studio — projects dir, SDK, ~/.android cache.
    let is_android_studio = (name_lower.contains("android") && name_lower.contains("studio"))
        || bundle_lower.contains("google.android.studio")
        || bundle_lower.contains("jetbrains.android");
    if is_android_studio {
        for rel in &["AndroidStudioProjects", "Library/Android", ".android"] {
            push_path(home.join(rel), "App Data", &mut out);
        }
        let google_support = home.join("Library/Application Support/Google");
        if google_support.is_dir() {
            if let Ok(entries) = fs::read_dir(&google_support) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("AndroidStudio") {
                        push_path(entry.path(), "App Data", &mut out);
                    }
                }
            }
        }
    }

    // Xcode — Developer dir is the big one, plus legacy ~/.Xcode.
    if name_lower.contains("xcode") || bundle_lower.contains("apple.dt.xcode") {
        push_path(home.join("Library/Developer"), "App Data", &mut out);
        push_path(home.join(".Xcode"), "App Data", &mut out);
    }

    // JetBrains IDEs — settings and caches are stored per-product under
    // ~/Library/Application Support/JetBrains/<ProductName><Version>.
    let jetbrains_products = [
        "IntelliJIdea", "PyCharm", "WebStorm", "GoLand", "RubyMine", "PhpStorm",
        "CLion", "DataGrip", "Rider", "AppCode", "DataSpell", "Fleet",
    ];
    let is_jetbrains = bundle_lower.contains("jetbrains")
        || jetbrains_products
            .iter()
            .any(|p| name_lower.contains(&p.to_lowercase()));
    if is_jetbrains {
        for rel in &[
            "Library/Application Support/JetBrains",
            "Library/Caches/JetBrains",
            "Library/Logs/JetBrains",
        ] {
            let base = home.join(rel);
            if !base.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(&base) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let entry_lower =
                        entry.file_name().to_string_lossy().to_lowercase();
                    let matches = jetbrains_products.iter().any(|p| {
                        let prod_lower = p.to_lowercase();
                        name_lower.contains(&prod_lower)
                            && entry_lower.starts_with(&prod_lower)
                    });
                    if matches {
                        push_path(entry.path(), "App Data", &mut out);
                    }
                }
            }
        }
    }

    // Unity / Unreal / Godot game engines.
    if name_lower.contains("unity") {
        push_path(home.join("Library/Unity"), "App Data", &mut out);
    }
    if name_lower.contains("unreal") {
        push_path(
            home.join("Library/Application Support/Epic"),
            "App Data",
            &mut out,
        );
    }
    if name_lower.contains("godot") {
        push_path(
            home.join("Library/Application Support/Godot"),
            "App Data",
            &mut out,
        );
    }

    // VS Code and its ShipIt updater cache.
    if bundle_lower.contains("microsoft.vscode") {
        push_path(home.join(".vscode"), "App Data", &mut out);
        push_path(
            home.join("Library/Caches/com.microsoft.VSCode.ShipIt"),
            "Caches",
            &mut out,
        );
        push_path(
            home.join("Library/Caches/com.microsoft.VSCodeInsiders.ShipIt"),
            "Caches",
            &mut out,
        );
    }

    // Docker Desktop leaves a ~/.docker config dir around.
    if name_lower.contains("docker") {
        push_path(home.join(".docker"), "App Data", &mut out);
    }

    // Maestro Studio — mobile test runner.
    if bundle_lower == "com.maestro.studio" || name_lower.contains("maestro studio") {
        push_path(home.join(".mobiledev"), "App Data", &mut out);
    }

    // Raycast — spawns multiple container bundles under variant names,
    // plus settings under ~/Library/Caches.
    if bundle_lower == "com.raycast.macos" {
        for rel in &[
            "Library/Application Support",
            "Library/Application Scripts",
            "Library/Containers",
        ] {
            let base = home.join(rel);
            if !base.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(&base) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let entry_lower =
                        entry.file_name().to_string_lossy().to_lowercase();
                    if entry_lower.contains("raycast") {
                        push_path(entry.path(), "App Data", &mut out);
                    }
                }
            }
        }
        // Explicit known-leftover container dirs.
        for leftover in &[
            "Library/Containers/com.raycast.macos.BrowserExtension",
            "Library/Containers/com.raycast.macos.RaycastAppIntents",
        ] {
            push_path(home.join(leftover), "Containers", &mut out);
        }
    }

    out
}

/// Searches ~/Library subdirectories for files associated with the given app.
pub fn find_associated(bundle_id: &str, app_name: &str, _app_path: &str) -> Vec<AssociatedFile> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let library = home.join("Library");
    let mut results = Vec::new();

    let name_variants = app_name_variants(app_name);

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
            if !matches_app(&entry_name, bundle_id, &name_variants) {
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

            // Sweep ~/Library/Preferences/ByHost for machine-specific
            // variants. These are named `<bundle_id>.<host_uuid>.plist`
            // and aren't caught by the bundle-id equality check above.
            let byhost_dir = prefs_dir.join("ByHost");
            if byhost_dir.exists() {
                if let Ok(entries) = fs::read_dir(&byhost_dir) {
                    let prefix = format!("{}.", bundle_id);
                    for entry in entries.filter_map(|e| e.ok()) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if !name.starts_with(&prefix) || !name.ends_with(".plist") {
                            continue;
                        }
                        let path = entry.path();
                        let size = path_size(&path);
                        if size == 0 {
                            continue;
                        }
                        let path_str = path.to_string_lossy().to_string();
                        if results.iter().any(|r| r.path == path_str) {
                            continue;
                        }
                        results.push(AssociatedFile {
                            path: path_str,
                            category: "Preferences".to_string(),
                            size,
                            is_dir: false,
                        });
                    }
                }
            }
        }
    }

    // Group Containers wildcard: entries here are typically named
    // `<team_id>.<bundle_id>` or similar, so an exact match won't catch
    // them. Walk the root and pick up anything whose name contains the
    // bundle id as a substring.
    if !bundle_id.is_empty() {
        let group_containers = library.join("Group Containers");
        if group_containers.exists() {
            if let Ok(entries) = fs::read_dir(&group_containers) {
                let needle = bundle_id.to_lowercase();
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if !name.contains(&needle) {
                        continue;
                    }
                    let path = entry.path();
                    let size = path_size(&path);
                    if size == 0 {
                        continue;
                    }
                    let path_str = path.to_string_lossy().to_string();
                    if results.iter().any(|r| r.path == path_str) {
                        continue;
                    }
                    results.push(AssociatedFile {
                        path: path_str,
                        category: "Group Containers".to_string(),
                        size,
                        is_dir: path.is_dir(),
                    });
                }
            }
        }
    }

    // sharedfilelist: recent-documents / most-recently-used lists, stored
    // per-bundle as `<bundle_id>.sfl4` under
    // ~/Library/Application Support/com.apple.sharedfilelist/. These aren't
    // huge but they leak document filenames the user expected to be gone.
    if !bundle_id.is_empty() {
        let sfl_root = library.join("Application Support/com.apple.sharedfilelist");
        if sfl_root.exists() {
            let target_name = format!("{}.sfl4", bundle_id);
            let target_name3 = format!("{}.sfl3", bundle_id);
            fn walk_sfl(
                dir: &Path,
                depth: usize,
                targets: &[&str],
                results: &mut Vec<AssociatedFile>,
            ) {
                if depth == 0 {
                    return;
                }
                let entries = match fs::read_dir(dir) {
                    Ok(e) => e,
                    Err(_) => return,
                };
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_dir() {
                        walk_sfl(&path, depth - 1, targets, results);
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !targets.iter().any(|t| name == *t) {
                        continue;
                    }
                    let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                    if size == 0 {
                        continue;
                    }
                    let path_str = path.to_string_lossy().to_string();
                    if results.iter().any(|r| r.path == path_str) {
                        continue;
                    }
                    results.push(AssociatedFile {
                        path: path_str,
                        category: "App Data".to_string(),
                        size,
                        is_dir: false,
                    });
                }
            }
            walk_sfl(
                &sfl_root,
                3,
                &[target_name.as_str(), target_name3.as_str()],
                &mut results,
            );
        }
    }

    // NSURLSession per-bundle download cache. Downloads initiated by apps
    // via NSURLSessionDownloadTask land here and can be megabytes-large.
    if !bundle_id.is_empty() {
        let dl_dir = library
            .join("Caches/com.apple.nsurlsessiond/Downloads")
            .join(bundle_id);
        if dl_dir.exists() {
            let size = path_size(&dl_dir);
            if size > 0 {
                let path_str = dl_dir.to_string_lossy().to_string();
                if !results.iter().any(|r| r.path == path_str) {
                    results.push(AssociatedFile {
                        path: path_str,
                        category: "Caches".to_string(),
                        size,
                        is_dir: true,
                    });
                }
            }
        }
    }

    // Specialized toolchain cleanup: IDE projects, SDKs, and engine
    // state folders that don't live under ~/Library/Application Support
    // and don't match the bundle id at all.
    for item in find_toolchain_specific(bundle_id, app_name, &home) {
        if !results.iter().any(|r| r.path == item.path) {
            results.push(item);
        }
    }

    // Dotfile / XDG roots outside ~/Library. Apps that originated on
    // Linux often write config to ~/.config/<name>, ~/.local/share/<name>,
    // or ~/.<name> instead of (or in addition to) ~/Library/Application Support.
    if !name_variants.is_empty() {
        let dot_roots: [(PathBuf, bool); 3] = [
            (home.join(".config"), false),
            (home.join(".local/share"), false),
            (home.clone(), true), // dotfile prefix match
        ];
        for (root, dot_prefix) in &dot_roots {
            if !root.is_dir() {
                continue;
            }
            let entries = match fs::read_dir(root) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let raw = entry.file_name().to_string_lossy().to_string();
                let check = if *dot_prefix {
                    // For dotfiles under $HOME, only accept names that
                    // begin with '.' and strip it for comparison.
                    if !raw.starts_with('.') {
                        continue;
                    }
                    raw[1..].to_lowercase()
                } else {
                    raw.to_lowercase()
                };
                if check.is_empty() {
                    continue;
                }
                let hit = name_variants.iter().any(|v| *v == check);
                // Also accept "<variant>rc" for classic rc-file naming.
                let rc_hit = !hit
                    && name_variants
                        .iter()
                        .any(|v| check.strip_suffix("rc") == Some(v.as_str()));
                if !hit && !rc_hit {
                    continue;
                }
                let path = entry.path();
                let size = path_size(&path);
                if size == 0 {
                    continue;
                }
                let path_str = path.to_string_lossy().to_string();
                if results.iter().any(|r| r.path == path_str) {
                    continue;
                }
                results.push(AssociatedFile {
                    path: path_str,
                    category: "App Data".to_string(),
                    size,
                    is_dir: path.is_dir(),
                });
            }
        }
    }

    results
}
