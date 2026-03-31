use std::fs;
use std::path::Path;
use std::process::Command;

use super::{UninstallProgress, UninstallResult};
use crate::commands::shared;
use crate::commands::utils::dir_size;

/// Paths that must never be deleted.
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

/// User-relative directories that must not be deleted as a whole.
const PROTECTED_HOME_DIRS: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "Pictures",
    "Music",
    "Movies",
];

/// Returns true if the app path is a system application that must not be uninstalled.
fn is_system_app(path: &str) -> bool {
    path.starts_with("/System/Applications/")
}

/// Attempt privileged deletion via osascript (triggers macOS admin password prompt).
/// Used as a fallback when normal deletion fails with Permission denied.
fn privileged_delete(path: &str, permanent: bool) -> Result<(), std::io::Error> {
    let script = if permanent {
        format!(
            "do shell script \"rm -rf {}\" with administrator privileges",
            shell_escape(path)
        )
    } else {
        // Use Finder to move to trash with admin privileges
        format!(
            "do shell script \"mv {} ~/.Trash/\" with administrator privileges",
            shell_escape(path)
        )
    };

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // User cancelled the password dialog
        if stderr.contains("User canceled") || stderr.contains("(-128)") {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Authorization cancelled by user",
            ))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                stderr.trim().to_string(),
            ))
        }
    }
}

/// Shell-escape a path for use inside an osascript do shell script string.
fn shell_escape(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// Returns true if a path is safe to delete.
/// Allows deleting individual .app bundles inside /Applications (e.g. /Applications/Foo.app)
/// but blocks deleting /Applications itself or its non-.app contents.
/// Also blocks system applications under /System/Applications/.
fn is_safe_path(path: &str) -> bool {
    if is_system_app(path) {
        return false;
    }
    // Block exact protected system paths and their children
    for protected in PROTECTED_PATHS {
        if path == *protected {
            return false;
        }
        // Special case: allow /Applications/*.app but block /Applications itself
        if *protected == "/Applications" && path.starts_with("/Applications/") {
            // Only allow .app bundles directly in /Applications
            let remainder = &path["/Applications/".len()..];
            if remainder.contains('/') {
                // It's a path inside an app bundle — allow
                continue;
            }
            if !remainder.ends_with(".app") {
                return false;
            }
            continue;
        }
        if path.starts_with(&format!("{}/", protected)) {
            return false;
        }
    }

    // Block home directory itself and key user directories
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path == home_str.as_ref() {
            return false;
        }
        for dir in PROTECTED_HOME_DIRS {
            let protected = format!("{}/{}", home_str, dir);
            if path == protected {
                return false;
            }
        }
    }

    true
}

/// Removes the app bundle and selected associated files.
/// Calls `on_progress` after each item is processed.
pub fn remove_app_and_files<F>(
    app_path: &str,
    file_paths: &[String],
    dry_run: bool,
    permanent: bool,
    mut on_progress: F,
) -> UninstallResult
where
    F: FnMut(&UninstallProgress),
{
    let mut bytes_freed: u64 = 0;
    let mut items_removed: usize = 0;
    let mut errors: Vec<String> = Vec::new();

    // Collect all paths to delete: associated files first, then the app bundle
    let mut all_paths: Vec<&str> = file_paths.iter().map(|s| s.as_str()).collect();
    all_paths.push(app_path);

    let items_total = all_paths.len();

    for (i, path_str) in all_paths.iter().enumerate() {
        let path = Path::new(path_str);

        // Safety check
        if !is_safe_path(path_str) {
            errors.push(format!("Skipped protected path: {}", path_str));
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        if !path.exists() {
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        let size = if path.is_dir() {
            dir_size(path)
        } else {
            path.metadata().map(|m| m.len()).unwrap_or(0)
        };

        if dry_run {
            bytes_freed += size;
            items_removed += 1;
        } else {
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
                    let action = if permanent { "DELETED" } else { "TRASHED" };
                    shared::log_operation("UNINSTALL", path_str, action);
                }
                Err(e) => {
                    // If permission denied, retry with admin privileges (osascript prompt)
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        shared::log_operation("UNINSTALL", path_str, "ESCALATING: requesting admin privileges");
                        match privileged_delete(path_str, permanent) {
                            Ok(()) => {
                                bytes_freed += size;
                                items_removed += 1;
                                let action = if permanent { "DELETED (admin)" } else { "TRASHED (admin)" };
                                shared::log_operation("UNINSTALL", path_str, action);
                            }
                            Err(priv_e) => {
                                shared::log_operation("UNINSTALL", path_str, &format!("ERROR: {}", priv_e));
                                errors.push(format!("{}: {}", path_str, priv_e));
                            }
                        }
                    } else {
                        shared::log_operation("UNINSTALL", path_str, &format!("ERROR: {}", e));
                        errors.push(format!("{}: {}", path_str, e));
                    }
                }
            }
        }

        on_progress(&UninstallProgress {
            current_item: path_str.to_string(),
            items_done: i + 1,
            items_total,
            bytes_freed,
        });
    }

    UninstallResult {
        items_removed,
        bytes_freed,
        errors,
    }
}

