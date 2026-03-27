pub mod executor;
pub mod rules;
pub mod scanner;

use serde::{Deserialize, Serialize};

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

/// Request to clean specific items.
#[derive(Deserialize)]
pub struct CleanRequest {
    pub rule_ids: Vec<String>,
    pub dry_run: bool,
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

use tauri::Emitter;

#[tauri::command]
pub fn scan_for_cleanables() -> Vec<ScanItem> {
    let rules = rules::all_rules();
    scanner::scan_rules(&rules)
}

#[tauri::command]
pub fn execute_clean(
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
