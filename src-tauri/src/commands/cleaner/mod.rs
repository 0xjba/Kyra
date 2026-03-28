pub mod executor;
pub mod rules;
pub mod scanner;

use serde::Serialize;

/// A cleaning rule definition — pure data describing what to scan.
#[derive(Clone, Serialize)]
pub struct CleanRule {
    pub id: String,
    pub category: String,
    pub label: String,
    pub paths: Vec<String>,
}

/// A single found path with its size.
#[derive(Clone, Serialize)]
pub struct PathInfo {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
}

/// Result of scanning a single rule — what was found and how big it is.
#[derive(Clone, Serialize)]
pub struct ScanItem {
    pub rule_id: String,
    pub category: String,
    pub label: String,
    pub paths: Vec<PathInfo>,
    pub total_size: u64,
}

/// Progress event emitted during cleaning.
#[derive(Clone, Serialize)]
pub struct CleanProgress {
    pub current_item: String,
    pub items_done: usize,
    pub items_total: usize,
    pub bytes_freed: u64,
}

/// Final result of a clean operation.
#[derive(Clone, Serialize)]
pub struct CleanResult {
    pub items_cleaned: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

/// Paths that must never be deleted.
const PROTECTED_PATHS: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/etc",
    "/var/db",
    "/Applications",
    "/Library/Frameworks",
];

/// Returns true if a path is safe to delete (not in the protected list).
pub fn is_safe_path(path: &str) -> bool {
    for protected in PROTECTED_PATHS {
        if path == *protected || path.starts_with(&format!("{}/", protected)) {
            return false;
        }
    }
    true
}

#[derive(Clone, Serialize)]
pub struct RunningApp {
    pub name: String,
    pub rule_ids: Vec<String>,
}

use tauri::Emitter;

#[tauri::command]
pub async fn scan_for_cleanables() -> Vec<ScanItem> {
    let rules = rules::all_rules();
    scanner::scan_rules(&rules)
}

#[tauri::command]
pub async fn execute_clean(
    app: tauri::AppHandle,
    rule_ids: Vec<String>,
    dry_run: bool,
) -> Result<CleanResult, String> {
    // First scan to get current state of selected items
    let rules = rules::all_rules();
    let all_items = scanner::scan_rules(&rules);
    let selected_items: Vec<ScanItem> = all_items
        .into_iter()
        .filter(|item| rule_ids.contains(&item.rule_id))
        .collect();

    if selected_items.is_empty() {
        return Ok(CleanResult {
            items_cleaned: 0,
            bytes_freed: 0,
            errors: vec![],
        });
    }

    let result = executor::execute_clean_items(&selected_items, dry_run, |progress| {
        let _ = app.emit("clean-progress", progress);
    });

    Ok(result)
}

#[tauri::command]
pub fn check_running_processes(rule_ids: Vec<String>) -> Vec<RunningApp> {
    use sysinfo::System;

    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let app_rules: Vec<(&str, &[&str])> = vec![
        ("Safari", &["browser_safari"]),
        ("Google Chrome", &["browser_chrome"]),
        ("Firefox", &["browser_firefox"]),
        ("Microsoft Edge", &["browser_edge"]),
        ("Brave Browser", &["browser_brave"]),
        ("Arc", &["browser_arc"]),
        ("Discord", &["app_discord"]),
        ("Slack", &["app_slack"]),
        ("Spotify", &["app_spotify"]),
        ("zoom.us", &["app_zoom"]),
        ("Microsoft Teams", &["app_teams"]),
        ("Code Helper", &["dev_vscode"]),
    ];

    let process_names: Vec<String> = sys
        .processes()
        .values()
        .map(|p| p.name().to_string_lossy().to_string())
        .collect();

    let mut running: Vec<RunningApp> = Vec::new();

    for (app_name, rules) in &app_rules {
        let matching_rules: Vec<String> = rules
            .iter()
            .filter(|r| rule_ids.contains(&r.to_string()))
            .map(|r| r.to_string())
            .collect();

        if matching_rules.is_empty() {
            continue;
        }

        let is_running = process_names.iter().any(|pn| pn.contains(app_name));

        if is_running {
            running.push(RunningApp {
                name: app_name.to_string(),
                rule_ids: matching_rules,
            });
        }
    }

    running
}
