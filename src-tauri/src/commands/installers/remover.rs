use super::{InstallerProgress, InstallerResult};
use crate::commands::shared;
use std::fs;
use std::path::Path;

const PROTECTED_PATHS: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/etc",
    "/var/db",
    "/Library/Frameworks",
    "/Applications",
];

const VALID_EXTENSIONS: &[&str] = &["dmg", "pkg", "iso", "xip", "app"];

fn is_safe_path(path: &Path) -> bool {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let path_str = canonical.to_string_lossy();

    for protected in PROTECTED_PATHS {
        if *protected == path_str.as_ref()
            || path_str.starts_with(&format!("{}/", protected))
        {
            return false;
        }
    }

    let home = dirs::home_dir();
    let in_downloads = home
        .as_ref()
        .map(|h| canonical.starts_with(h.join("Downloads")))
        .unwrap_or(false);
    let in_desktop = home
        .as_ref()
        .map(|h| canonical.starts_with(h.join("Desktop")))
        .unwrap_or(false);
    let in_tmp = canonical.starts_with("/tmp") || canonical.starts_with("/private/tmp");

    if !in_downloads && !in_desktop && !in_tmp {
        return false;
    }

    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    VALID_EXTENSIONS.iter().any(|ext| {
        if *ext == "app" {
            name.ends_with(".app")
        } else {
            name.to_lowercase().ends_with(&format!(".{}", ext))
        }
    })
}

pub fn remove_installer_files(
    file_paths: &[String],
    dry_run: bool,
    on_progress: impl Fn(InstallerProgress),
) -> InstallerResult {
    let total = file_paths.len();
    let mut items_removed: usize = 0;
    let mut bytes_freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for (i, path_str) in file_paths.iter().enumerate() {
        let path = Path::new(path_str);

        on_progress(InstallerProgress {
            current_item: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path_str.clone()),
            items_done: i,
            items_total: total,
            bytes_freed,
        });

        if !is_safe_path(path) {
            errors.push(format!("Blocked: {}", path_str));
            continue;
        }

        let size = if path.is_dir() {
            dir_size(path)
        } else {
            fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        };

        if dry_run {
            bytes_freed += size;
            items_removed += 1;
            continue;
        }

        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };

        match result {
            Ok(()) => {
                bytes_freed += size;
                items_removed += 1;
                shared::log_operation("DELETE_INSTALLER", path_str, "OK");
            }
            Err(e) => {
                shared::log_operation("DELETE_INSTALLER", path_str, &format!("ERROR: {}", e));
                errors.push(format!("{}: {}", path_str, e));
            }
        }
    }

    on_progress(InstallerProgress {
        current_item: String::new(),
        items_done: total,
        items_total: total,
        bytes_freed,
    });

    InstallerResult {
        items_removed,
        bytes_freed,
        errors,
    }
}

fn dir_size(path: &Path) -> u64 {
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
