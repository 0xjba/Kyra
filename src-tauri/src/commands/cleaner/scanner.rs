use std::fs;
use std::path::{Path, PathBuf};

use super::{CleanRule, PathInfo, ScanItem};

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

/// Calculates the total size of a directory recursively.
/// Does not follow symlinks. Silently skips files/dirs it can't read.
fn dir_size(path: &Path) -> u64 {
    if path.is_symlink() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_symlink() {
                0
            } else if p.is_dir() {
                dir_size(&p)
            } else {
                p.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Scans the filesystem for items matching the given rules.
/// Returns only rules that have at least one existing path with non-zero size.
pub fn scan_rules(rules: &[CleanRule]) -> Vec<ScanItem> {
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

            let size = if expanded.is_dir() {
                dir_size(&expanded)
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

        if !found_paths.is_empty() {
            results.push(ScanItem {
                rule_id: rule.id.clone(),
                category: rule.category.clone(),
                label: rule.label.clone(),
                paths: found_paths,
                total_size,
            });
        }
    }

    results
}
