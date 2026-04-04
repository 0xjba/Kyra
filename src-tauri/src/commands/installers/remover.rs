use super::{InstallerProgress, InstallerResult};
use crate::commands::shared;
use crate::commands::utils::dir_size;
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

const VALID_EXTENSIONS: &[&str] = &["dmg", "pkg", "iso", "xip", "mpkg", "app"];

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
    let in_allowed_home_dir = home
        .as_ref()
        .map(|h| {
            canonical.starts_with(h.join("Downloads"))
                || canonical.starts_with(h.join("Desktop"))
                || canonical.starts_with(h.join("Documents"))
                || canonical.starts_with(h.join("Public"))
                || canonical.starts_with(h.join("Library/Caches/Homebrew/downloads"))
                || canonical.starts_with(h.join("Library/Mail Downloads"))
        })
        .unwrap_or(false);
    let in_tmp = canonical.starts_with("/tmp") || canonical.starts_with("/private/tmp");
    let in_users_shared =
        canonical.starts_with("/Users/Shared") || canonical.starts_with("/private/Users/Shared");

    if !in_allowed_home_dir && !in_tmp && !in_users_shared {
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
    permanent: bool,
    on_progress: impl Fn(InstallerProgress),
) -> InstallerResult {
    let total = file_paths.len();
    let mut items_removed: usize = 0;
    let mut bytes_freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut deleted_paths: Vec<String> = Vec::new();

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
            deleted_paths.push(path_str.clone());
            continue;
        }

        let delete_result = if permanent {
            if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
        } else {
            trash::delete(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        };
        match delete_result {
            Ok(()) => {
                bytes_freed += size;
                items_removed += 1;
                deleted_paths.push(path_str.clone());
                let action = if permanent { "DELETED" } else { "TRASHED" };
                shared::log_operation("DELETE_INSTALLER", path_str, action);
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
        deleted_paths,
    }
}

