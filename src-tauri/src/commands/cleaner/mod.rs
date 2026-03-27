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
