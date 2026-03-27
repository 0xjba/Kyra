use std::fs;
use std::path::Path;

use super::{DirNode, ScanProgress};

/// Recursively scans a directory and builds a size-annotated tree.
/// `max_depth` limits how deep the tree goes (0 = just the root).
/// Calls `on_progress` periodically during the scan.
pub fn scan_directory<F>(
    root_path: &str,
    max_depth: usize,
    mut on_progress: F,
) -> DirNode
where
    F: FnMut(&ScanProgress),
{
    let mut files_scanned: usize = 0;
    let mut total_size_acc: u64 = 0;
    let path = Path::new(root_path);

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(root_path)
        .to_string();

    let mut root = build_tree(path, 0, max_depth, &mut files_scanned, &mut total_size_acc, &mut on_progress);
    root.name = name;
    root.path = root_path.to_string();
    root
}

fn build_tree<F>(
    path: &Path,
    current_depth: usize,
    max_depth: usize,
    files_scanned: &mut usize,
    running_total: &mut u64,
    on_progress: &mut F,
) -> DirNode
where
    F: FnMut(&ScanProgress),
{
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    // Skip symlinks
    if path.is_symlink() {
        return DirNode {
            name,
            path: path.to_string_lossy().to_string(),
            size: 0,
            is_dir: false,
            children: vec![],
        };
    }

    if path.is_file() {
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        *files_scanned += 1;
        *running_total += size;

        if *files_scanned % 500 == 0 {
            on_progress(&ScanProgress {
                current_path: path.to_string_lossy().to_string(),
                files_scanned: *files_scanned,
                total_size: *running_total,
            });
        }

        return DirNode {
            name,
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: false,
            children: vec![],
        };
    }

    // Directory
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            return DirNode {
                name,
                path: path.to_string_lossy().to_string(),
                size: 0,
                is_dir: true,
                children: vec![],
            };
        }
    };

    let mut children = Vec::new();
    let mut total_size: u64 = 0;

    if current_depth < max_depth {
        for entry in entries.filter_map(|e| e.ok()) {
            let child_path = entry.path();
            if child_path.is_symlink() {
                continue;
            }
            let child = build_tree(&child_path, current_depth + 1, max_depth, files_scanned, running_total, on_progress);
            total_size += child.size;
            children.push(child);
        }

        // Sort children by size descending
        children.sort_by(|a, b| b.size.cmp(&a.size));
    } else {
        // At max depth, just calculate total size without building children
        for entry in entries.filter_map(|e| e.ok()) {
            let child_path = entry.path();
            if child_path.is_symlink() {
                continue;
            }
            total_size += flat_size(&child_path, files_scanned, running_total, on_progress);
        }
    }

    DirNode {
        name,
        path: path.to_string_lossy().to_string(),
        size: total_size,
        is_dir: true,
        children,
    }
}

/// Calculates size without building child nodes (used at max depth).
fn flat_size<F>(path: &Path, files_scanned: &mut usize, running_total: &mut u64, on_progress: &mut F) -> u64
where
    F: FnMut(&ScanProgress),
{
    if path.is_symlink() {
        return 0;
    }
    if path.is_file() {
        *files_scanned += 1;
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        *running_total += size;
        if *files_scanned % 500 == 0 {
            on_progress(&ScanProgress {
                current_path: path.to_string_lossy().to_string(),
                files_scanned: *files_scanned,
                total_size: *running_total,
            });
        }
        return size;
    }
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_symlink() {
                0
            } else {
                flat_size(&p, files_scanned, running_total, on_progress)
            }
        })
        .sum()
}
