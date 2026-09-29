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

/// True if `path` equals `root` or lies inside it. Case-insensitive because
/// default APFS volumes are, so `/system/Library` is `/System/Library`.
pub fn is_same_or_under(path: &str, root: &str) -> bool {
    let (p, r) = (path.as_bytes(), root.as_bytes());
    p.len() >= r.len()
        && p[..r.len()].eq_ignore_ascii_case(r)
        && (p.len() == r.len() || p[r.len()] == b'/')
}

/// Locations that are never a valid delete target in any module. Matched
/// exactly: what may be removed *inside* them is each module's own policy.
const CRITICAL_ROOTS: &[&str] = &[
    "/",
    "/Applications",
    "/Library",
    "/System",
    "/Users",
    "/Users/Shared",
    "/Volumes",
    "/bin",
    "/cores",
    "/dev",
    "/etc",
    "/opt",
    "/opt/homebrew",
    "/private",
    "/private/etc",
    "/private/tmp",
    "/private/var",
    "/sbin",
    "/tmp",
    "/usr",
    "/usr/local",
    "/var",
];

const CRITICAL_HOME_CHILDREN: &[&str] = &[
    "Applications",
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "Movies",
    "Music",
    "Pictures",
    "Public",
];

fn normalized(path: &Path) -> String {
    path.components().collect::<PathBuf>().to_string_lossy().into_owned()
}

/// True if `path` is a filesystem root, the home directory, or one of the
/// standard top-level home folders, relative to the given `home`.
pub fn is_critical_path_for_home(path: &Path, home: Option<&Path>) -> bool {
    let p = normalized(path);
    if CRITICAL_ROOTS.iter().any(|r| p.eq_ignore_ascii_case(r)) {
        return true;
    }
    if let Some(home) = home {
        let h = normalized(home);
        if p.eq_ignore_ascii_case(&h) {
            return true;
        }
        return CRITICAL_HOME_CHILDREN
            .iter()
            .any(|c| p.eq_ignore_ascii_case(&format!("{}/{}", h, c)));
    }
    false
}

/// `is_critical_path_for_home` against the real home directory, in both its
/// literal and symlink-resolved forms.
pub fn is_critical_path(path: &Path) -> bool {
    let home = dirs::home_dir();
    let canonical_home = home.as_ref().and_then(|h| fs::canonicalize(h).ok());
    is_critical_path_for_home(path, home.as_deref())
        || is_critical_path_for_home(path, canonical_home.as_deref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{mkdir, write_file};
    use std::os::unix::fs::symlink;

    #[test]
    fn dir_size_sums_nested_files_and_skips_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        write_file(&dir.path().join("a.bin"), 10_000);
        write_file(&dir.path().join("sub/deeper/b.bin"), 20_000);
        let outside = tempfile::tempdir().unwrap();
        write_file(&outside.path().join("huge.bin"), 1_000_000);
        symlink(outside.path(), dir.path().join("link_dir")).unwrap();
        symlink(outside.path().join("huge.bin"), dir.path().join("link_file")).unwrap();

        let size = dir_size(dir.path());
        assert!(size >= 30_000, "{size}");
        assert!(size < 1_000_000, "symlink target was counted: {size}");
    }

    #[test]
    fn dir_size_of_missing_or_empty_dir_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(dir.path()), 0);
        assert_eq!(dir_size(&dir.path().join("missing")), 0);
    }

    #[test]
    fn path_size_handles_files_dirs_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("f.bin");
        write_file(&f, 50_000);
        assert!(path_size(&f) >= 50_000);
        assert_eq!(path_size(dir.path()), dir_size(dir.path()));
        assert_eq!(path_size(&dir.path().join("nope")), 0);
    }

    #[test]
    fn deletable_dir_size_skips_protected_user_data_dirs() {
        let dir = tempfile::tempdir().unwrap();
        write_file(&dir.path().join("Cache/data.bin"), 10_000);
        for name in PROTECTED_USER_DATA_COMPONENTS {
            write_file(&dir.path().join(name).join("keep.bin"), 500_000);
        }
        let size = deletable_dir_size(dir.path());
        assert!(size >= 10_000);
        assert!(size < 500_000, "protected dir counted: {size}");
    }

    #[test]
    fn deletable_dir_size_skips_read_only_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        write_file(&locked.join("x.bin"), 500_000);
        write_file(&dir.path().join("y.bin"), 1_000);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let size = deletable_dir_size(dir.path());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(size >= 1_000);
        assert!(size < 500_000, "unwritable dir counted: {size}");
    }

    #[test]
    fn protected_component_names_are_exact() {
        assert!(is_protected_user_data_component("IndexedDB"));
        assert!(is_protected_user_data_component("Local Storage"));
        assert!(is_protected_user_data_component("Service Worker"));
        assert!(!is_protected_user_data_component("indexeddb"));
        assert!(!is_protected_user_data_component("Cache"));
        assert!(!is_protected_user_data_component(""));
    }

    #[test]
    fn canonicalize_rejects_malformed_input() {
        assert!(canonicalize_for_safety("").is_none());
        assert!(canonicalize_for_safety("/tmp/a\0b").is_none());
        assert!(canonicalize_for_safety("/tmp/a\nb").is_none());
        assert!(canonicalize_for_safety("/Users/x/../../System").is_none());
        assert!(canonicalize_for_safety("../etc").is_none());
        assert!(canonicalize_for_safety("/a/b/..").is_none());
    }

    #[test]
    fn canonicalize_resolves_symlinks_and_passes_missing_paths_through() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        mkdir(&real);
        let link = dir.path().join("link");
        symlink(&real, &link).unwrap();

        let resolved = canonicalize_for_safety(&link.to_string_lossy()).unwrap();
        assert_eq!(resolved, fs::canonicalize(&real).unwrap());

        let missing = dir.path().join("missing/file");
        assert_eq!(
            canonicalize_for_safety(&missing.to_string_lossy()).unwrap(),
            missing
        );
    }

    #[test]
    fn same_or_under_is_component_aware_and_case_insensitive() {
        assert!(is_same_or_under("/System", "/System"));
        assert!(is_same_or_under("/System/Library", "/System"));
        assert!(is_same_or_under("/system/library", "/System"));
        assert!(is_same_or_under("/SYSTEM", "/System"));
        assert!(is_same_or_under("/System/", "/System"));
        assert!(!is_same_or_under("/SystemX", "/System"));
        assert!(!is_same_or_under("/Sys", "/System"));
        assert!(!is_same_or_under("/Users/x/System", "/System"));
    }

    #[test]
    fn critical_paths_cover_roots_home_and_home_folders() {
        let home = Path::new("/Users/tester");
        for p in [
            "/", "/Users", "/Library", "/System", "/Applications", "/Volumes", "/private/var",
            "/usr/local", "/opt/homebrew", "/tmp", "/etc",
        ] {
            assert!(is_critical_path_for_home(Path::new(p), Some(home)), "{p}");
        }
        for p in [
            "/Users/tester",
            "/Users/tester/",
            "/users/TESTER",
            "/Users/tester/Documents",
            "/Users/tester/Documents/",
            "/Users/tester/./Library",
            "/Users/tester/downloads",
            "/Users/tester/Desktop",
            "/Users/tester/Pictures",
        ] {
            assert!(is_critical_path_for_home(Path::new(p), Some(home)), "{p}");
        }
    }

    #[test]
    fn critical_paths_do_not_cover_descendants() {
        let home = Path::new("/Users/tester");
        for p in [
            "/Users/tester/Documents/report.pdf",
            "/Users/tester/Library/Caches",
            "/Users/tester/Library/Caches/com.foo",
            "/Applications/Foo.app",
            "/Library/Caches",
            "/Users/other",
            "/Users/tester/project",
            "/usr/local/bin",
        ] {
            assert!(!is_critical_path_for_home(Path::new(p), Some(home)), "{p}");
        }
        assert!(!is_critical_path_for_home(Path::new("/Users/tester/Documents"), None));
    }

    #[test]
    fn critical_path_uses_the_real_home() {
        if let Some(home) = dirs::home_dir() {
            assert!(is_critical_path(&home));
            assert!(is_critical_path(&home.join("Documents")));
            assert!(!is_critical_path(&home.join("Documents/some-file.txt")));
        }
        assert!(is_critical_path(Path::new("/")));
    }

    #[test]
    fn dedup_by_inode_collapses_hardlinks_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        write_file(&a, 10);
        let hard = dir.path().join("hard");
        fs::hard_link(&a, &hard).unwrap();
        let soft = dir.path().join("soft");
        symlink(&a, &soft).unwrap();
        let b = dir.path().join("b");
        write_file(&b, 10);
        let missing = dir.path().join("missing").to_string_lossy().to_string();

        let input: Vec<String> = [&a, &hard, &soft, &b]
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .chain([missing.clone(), missing.clone()])
            .collect();
        let out = dedup_paths_by_inode(&input);
        assert_eq!(out, vec![input[0].clone(), input[3].clone(), missing.clone(), missing]);
    }
}
