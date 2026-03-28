use std::fs;
use std::path::Path;

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
                    total += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    total
}

/// Get the size of a path — file size for files, recursive size for directories.
pub fn path_size(path: &Path) -> u64 {
    if path.is_dir() {
        dir_size(path)
    } else {
        fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}
