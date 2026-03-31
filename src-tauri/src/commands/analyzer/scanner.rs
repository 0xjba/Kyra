use std::fs;
use std::path::Path;

use super::{DirNode, ScanProgress};
use crate::commands::utils::dir_size;

/// Directories treated as atomic — never recurse into them.
const FOLDED_DIRS: &[&str] = &[
    "node_modules", ".git", "venv", ".venv", "target", "build", "dist",
    "__pycache__", ".pytest_cache", "Pods", "DerivedData", ".gradle",
    ".next", ".nuxt", "vendor", ".turbo", ".parcel-cache", ".angular",
    ".svelte-kit", ".astro", "coverage", ".cxx", ".expo", ".dart_tool",
    "zig-out", ".zig-cache", "obj", "bin", ".build",
    ".Spotlight-V100", ".fseventsd", ".Trashes",
    "CachedData", "CachedExtensions", "GPUCache", "Cache",
];

/// Directories that are safe to delete (build artifacts, caches).
const CLEANABLE_NAMES: &[&str] = &[
    "node_modules", "target", "build", "dist", "venv", ".venv",
    "__pycache__", ".pytest_cache", "Pods", "DerivedData",
    ".next", ".nuxt", ".gradle", ".turbo", "coverage",
    "Cache", "Caches", "CachedData", "GPUCache",
];

/// Top-level directories to skip when scanning from `/`.
const SKIP_DIRS: &[&str] = &[
    "dev", "cores", "System", "sbin", "bin", "etc", "var",
    "Volumes", "Network", ".vol", ".Spotlight-V100", ".fseventsd",
    "private",
];

fn is_folded(name: &str) -> bool {
    FOLDED_DIRS.iter().any(|&d| d == name)
}

fn is_cleanable(name: &str) -> bool {
    CLEANABLE_NAMES.iter().any(|&d| d == name)
}

fn should_skip_root_child(name: &str) -> bool {
    SKIP_DIRS.iter().any(|&d| d == name)
}

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
    let is_root = root_path == "/";

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(root_path)
        .to_string();

    let mut root = build_tree(path, 0, max_depth, is_root, &mut files_scanned, &mut total_size_acc, &mut on_progress);
    root.name = name;
    root.path = root_path.to_string();
    root
}

fn build_tree<F>(
    path: &Path,
    current_depth: usize,
    max_depth: usize,
    is_root_scan: bool,
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
            is_cleanable: false,
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
            is_cleanable: false,
            children: vec![],
        };
    }

    // Directory — check if it should be folded (treated as atomic leaf)
    if current_depth > 0 && is_folded(&name) {
        let size = dir_size(path);
        *running_total += size;

        on_progress(&ScanProgress {
            current_path: path.to_string_lossy().to_string(),
            files_scanned: *files_scanned,
            total_size: *running_total,
        });

        return DirNode {
            name: name.clone(),
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: true,
            is_cleanable: is_cleanable(&name),
            children: vec![],
        };
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            return DirNode {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                size: 0,
                is_dir: true,
                is_cleanable: is_cleanable(&name),
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

            // Skip system directories when scanning from /
            if is_root_scan && current_depth == 0 {
                if let Some(child_name) = child_path.file_name().and_then(|n| n.to_str()) {
                    if should_skip_root_child(child_name) {
                        continue;
                    }
                }
            }

            let child = build_tree(&child_path, current_depth + 1, max_depth, is_root_scan, files_scanned, running_total, on_progress);
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
        name: name.clone(),
        path: path.to_string_lossy().to_string(),
        size: total_size,
        is_dir: true,
        is_cleanable: is_cleanable(&name),
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
