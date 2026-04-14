use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

/// Return the physical allocated size of a file, in bytes.
///
/// Uses `st_blocks * 512` so that sparse files (APFS clones, disk images,
/// VM storage) report their true on-disk footprint rather than the
/// logical length, matching `du` and Finder's "Size on disk".
fn physical_size(meta: &fs::Metadata) -> u64 {
    meta.blocks().saturating_mul(512)
}

/// Calculate the total size of a directory recursively, skipping symlinks.
pub fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_symlink() {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    total += fs::metadata(&p).map(|m| physical_size(&m)).unwrap_or(0);
                }
            }
        }
    }
    total
}

/// Directory basenames that hold live user-data (PWA offline storage,
/// localStorage, IndexedDB, etc.) and must NEVER be walked into for
/// size accounting or deletion by the cleaner. Chromium-based apps
/// (Chrome, Edge, Brave, VS Code, Slack, Discord, Teams, Signal,
/// Cursor, Claude Desktop, …) all use these exact directory names
/// under their profile root. Clearing them would log users out of
/// PWAs, destroy offline data, and wipe extension state.
pub const PROTECTED_USER_DATA_COMPONENTS: &[&str] = &[
    "Service Worker",
    "IndexedDB",
    "Local Storage",
    "Session Storage",
    "databases",
    "Local Extension Settings",
    "Sync Extension Settings",
    "Extension State",
    "Extension Rules",
    "Extension Scripts",
    "File System",
];

/// Returns true if the given directory name is a protected user-data
/// component that the cleaner must never walk into or delete.
pub fn is_protected_user_data_component(name: &str) -> bool {
    PROTECTED_USER_DATA_COMPONENTS.iter().any(|p| *p == name)
}

/// Calculate the total size of deletable files in a directory recursively.
/// Skips symlinks, files the current user cannot delete, and any
/// subdirectory whose name is a protected user-data component (see
/// `PROTECTED_USER_DATA_COMPONENTS`). This matches the behavior of the
/// cleaner executor, so scan sizes reflect what will actually be freed.
pub fn deletable_dir_size(path: &Path) -> u64 {
    let uid = unsafe { libc::getuid() };
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // Check if we can write to the parent directory (needed to delete entries)
        let dir_writable = fs::metadata(&dir)
            .map(|m| {
                let mode = m.permissions().mode();
                let owner = m.uid();
                if owner == uid {
                    mode & 0o200 != 0 // owner write
                } else {
                    mode & 0o002 != 0 // other write
                }
            })
            .unwrap_or(false);

        if !dir_writable {
            continue; // Can't delete anything in this directory
        }

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_symlink() {
                    continue;
                }
                if p.is_dir() {
                    // Skip protected user-data subdirs (PWA / SW / IndexedDB)
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        if is_protected_user_data_component(name) {
                            continue;
                        }
                    }
                    stack.push(p);
                } else {
                    total += fs::metadata(&p).map(|m| physical_size(&m)).unwrap_or(0);
                }
            }
        }
    }
    total
}

/// Validate a path string and return its canonical form if it's well-formed.
///
/// Rejects:
/// - Empty strings
/// - Paths containing control characters (NUL, newlines, etc.)
/// - Paths containing `..` traversal components
///
/// If the path exists on disk, returns its canonical form (symlinks resolved)
/// so callers can detect symlink-escape attacks where a user-writable path
/// points into a protected system location. If the path does not exist,
/// returns the cleaned input path unchanged — non-existent paths can't be
/// deleted anyway, so symlink resolution isn't needed.
///
/// Returns `None` if the path is malformed or cannot be canonicalized.
pub fn canonicalize_for_safety(path: &str) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    if path.chars().any(|c| c.is_control()) {
        return None;
    }
    let p = Path::new(path);
    for component in p.components() {
        if matches!(component, Component::ParentDir) {
            return None;
        }
    }
    if p.exists() {
        fs::canonicalize(p).ok()
    } else {
        Some(p.to_path_buf())
    }
}

/// Get the size of a path — file size for files, recursive size for directories.
pub fn path_size(path: &Path) -> u64 {
    if path.is_dir() {
        dir_size(path)
    } else {
        fs::metadata(path).map(|m| physical_size(&m)).unwrap_or(0)
    }
}

/// Deduplicate paths by inode — same file via different paths counted once.
pub fn dedup_paths_by_inode(paths: &[String]) -> Vec<String> {
    use std::collections::HashSet;

    let mut seen_inodes: HashSet<(u64, u64)> = HashSet::new(); // (dev, inode)
    let mut unique = Vec::new();

    for path in paths {
        if let Ok(meta) = fs::metadata(path) {
            let key = (meta.dev(), meta.ino());
            if seen_inodes.insert(key) {
                unique.push(path.clone());
            }
        } else {
            unique.push(path.clone()); // Keep paths we can't stat
        }
    }

    unique
}
