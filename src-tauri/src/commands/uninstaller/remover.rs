use std::fs;
use std::path::Path;
use std::process::Command;

use super::{brew, UninstallProgress, UninstallResult};
use crate::commands::shared;
use crate::commands::utils::{canonicalize_for_safety, dir_size};

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

/// Returns true if a path looks like a launchd job definition we should
/// try to unload before deleting — i.e. a .plist under a LaunchAgents,
/// LaunchDaemons, or PrivilegedHelperTools directory.
fn is_launchd_plist(path: &str) -> bool {
    if !path.ends_with(".plist") {
        return false;
    }
    path.contains("/LaunchAgents/")
        || path.contains("/LaunchDaemons/")
        || path.contains("/PrivilegedHelperTools/")
}

/// Escape a string for safe inclusion inside an AppleScript double-quoted
/// literal. AppleScript escapes `\\` and `"` by prefixing with `\`.
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Best-effort removal of the app from macOS Login Items. Uses
/// `osascript` + System Events to walk the current user's login-items
/// list in reverse and delete any entry whose name matches `app_name`.
/// Iterating in reverse avoids index shifting as items are removed.
///
/// This covers apps that registered themselves via the classic
/// LSSharedFileList API. Modern SMAppService-registered helpers are
/// already picked up through the LaunchAgents sweep in discovery.
///
/// The first invocation of System Events from a new app triggers a
/// macOS Automation permission prompt; failures are logged but do not
/// block the rest of the uninstall.
fn try_remove_login_item(app_name: &str, dry_run: bool) {
    if app_name.is_empty() || dry_run {
        return;
    }
    let escaped = applescript_escape(app_name);
    let script = format!(
        "tell application \"System Events\"\n\
            try\n\
                set itemCount to count of login items\n\
                repeat with i from itemCount to 1 by -1\n\
                    try\n\
                        if name of login item i is \"{}\" then\n\
                            delete login item i\n\
                        end if\n\
                    end try\n\
                end repeat\n\
            end try\n\
        end tell",
        escaped
    );

    let _ = Command::new("osascript").arg("-e").arg(&script).output();
    shared::log_operation("UNINSTALL", app_name, "login item removed");
}

/// Best-effort `defaults delete <bundle_id>` and
/// `defaults -currentHost delete <bundle_id>` to flush cfprefsd's
/// in-memory preference cache. Without this flush, cfprefsd may
/// re-create the preference file on disk from its cached values
/// seconds after we deleted it, leaving the app's settings behind
/// for the next install to inherit.
///
/// Validates the bundle id against a strict alphanumeric/./-/_ charset
/// so no shell metacharacters can leak into the command arguments.
fn try_defaults_delete(bundle_id: &str) {
    if bundle_id.is_empty() {
        return;
    }
    let valid = bundle_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
    if !valid {
        return;
    }
    let _ = Command::new("/usr/bin/defaults")
        .args(["delete", bundle_id])
        .output();
    let _ = Command::new("/usr/bin/defaults")
        .args(["-currentHost", "delete", bundle_id])
        .output();
    shared::log_operation("UNINSTALL", bundle_id, "defaults delete");
}

/// Path to macOS's Launch Services registration tool.
const LSREGISTER: &str =
    "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister";

/// Best-effort `lsregister -u <app>` to remove the .app bundle from the
/// Launch Services database before it's deleted. Without this, stale
/// entries can linger for days in "Open with…" menus, Spotlight results,
/// and the default-application mappings. Failures are ignored — lsregister
/// is advisory; the file deletion proceeds either way.
fn try_lsregister_unregister(app_path: &str) {
    if !app_path.ends_with(".app") {
        return;
    }
    if !Path::new(LSREGISTER).exists() {
        return;
    }
    let _ = Command::new(LSREGISTER).arg("-u").arg(app_path).output();
    shared::log_operation("UNINSTALL", app_path, "lsregister -u");
}

/// Best-effort `launchctl unload` (or `bootout`) on a job plist before it
/// gets deleted. Stopping the service avoids "resource busy" errors and
/// prevents launchd from respawning the binary we just removed. Failures
/// are logged but never propagated — if unload fails we still proceed
/// with the delete, because some jobs simply aren't loaded.
///
/// LaunchDaemons live in /Library and need admin to unload; we route
/// those through osascript. User LaunchAgents unload without escalation.
fn try_launchctl_unload(path: &str) {
    if !is_launchd_plist(path) {
        return;
    }

    let needs_admin = path.starts_with("/Library/LaunchDaemons/")
        || path.starts_with("/Library/PrivilegedHelperTools/");

    if needs_admin {
        let script = format!(
            "do shell script \"/bin/launchctl unload {} 2>/dev/null || true\" with administrator privileges",
            shell_escape(path)
        );
        let _ = Command::new("osascript").arg("-e").arg(&script).output();
    } else {
        let _ = Command::new("/bin/launchctl")
            .arg("unload")
            .arg(path)
            .output();
    }

    shared::log_operation("UNINSTALL", path, "launchctl unload");
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
///
/// Rejects empty paths, control characters, and `..` traversal components.
/// Additionally resolves symlinks so that a user-writable path which points
/// into a protected system location (e.g. a symlink to /System) is blocked
/// even if the literal string looks safe.
fn is_safe_path(path: &str) -> bool {
    if is_system_app(path) {
        return false;
    }

    let canonical = match canonicalize_for_safety(path) {
        Some(p) => p,
        None => return false,
    };
    let canonical_str = canonical.to_string_lossy();

    // Block exact protected system paths and their children (literal form).
    for protected in PROTECTED_PATHS {
        if path == *protected {
            return false;
        }
        // Special case: allow /Applications/*.app but block /Applications itself
        if *protected == "/Applications" && path.starts_with("/Applications/") {
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

    // Also check the canonical (symlink-resolved) form against protected
    // system roots. The /Applications exception does not apply here — a
    // legitimate .app bundle resolves either to itself or into a Homebrew
    // Caskroom, neither of which is a protected system directory.
    for protected in PROTECTED_PATHS {
        if *protected == "/Applications" {
            continue;
        }
        let prefix = format!("{}/", protected);
        if canonical_str == *protected || canonical_str.starts_with(&prefix) {
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
///
/// If `brew_cask` is Some, the cask is uninstalled first with
/// `brew uninstall --cask --zap`, which typically removes both the bundle
/// and any caches/launch agents the cask declares in its zap stanza.
/// After that the normal file-deletion loop still runs to pick up any
/// associated files the cask didn't know about.
///
/// If `bundle_id` is non-empty, `defaults delete` is invoked after the
/// file-deletion loop so cfprefsd drops its in-memory cache of the app's
/// preferences before it can rewrite them to disk.
pub fn remove_app_and_files<F>(
    app_path: &str,
    file_paths: &[String],
    bundle_id: &str,
    brew_cask: Option<String>,
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
    let mut deleted_paths: Vec<String> = Vec::new();

    // Strip any login-items entry for this app before we start deleting
    // files. The display name is the app bundle's file stem — e.g. the
    // `/Applications/Foo.app` bundle shows up in Login Items as "Foo".
    let app_display_name = Path::new(app_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    try_remove_login_item(&app_display_name, dry_run);

    // If this app is a Homebrew cask, let brew handle the payload + zap
    // stanzas first. The file-deletion loop below still runs afterwards to
    // clean up anything brew didn't know about (user caches, orphaned
    // launch agents, etc.).
    let brew_handled_app = if let Some(cask) = brew_cask.as_deref() {
        match brew::uninstall_cask(cask, dry_run) {
            Ok(log_line) => {
                shared::log_operation(
                    "UNINSTALL",
                    app_path,
                    &format!("brew --zap {}: {}", cask, log_line.lines().next().unwrap_or("ok")),
                );
                // If the bundle is already gone, count it as removed now.
                let path = Path::new(app_path);
                let app_gone = !path.exists() && fs::symlink_metadata(path).is_err();
                if app_gone {
                    deleted_paths.push(app_path.to_string());
                    items_removed += 1;
                }
                app_gone
            }
            Err(e) => {
                shared::log_operation(
                    "UNINSTALL",
                    app_path,
                    &format!("brew --zap {} failed: {}", cask, e),
                );
                errors.push(format!("brew uninstall failed: {}", e));
                false
            }
        }
    } else {
        false
    };

    // Collect all paths to delete: associated files first, then the app bundle
    let mut all_paths: Vec<&str> = file_paths.iter().map(|s| s.as_str()).collect();
    if !brew_handled_app {
        all_paths.push(app_path);
    }

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
            deleted_paths.push(path_str.to_string());
        } else {
            // Stop any launchd service that owns this plist before we
            // delete the file, otherwise launchd may hold a reference
            // to a now-missing binary or immediately respawn it.
            try_launchctl_unload(path_str);

            // Drop the app bundle from the Launch Services database so
            // it stops showing up in "Open with…" menus and Spotlight.
            try_lsregister_unregister(path_str);

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
                    deleted_paths.push(path_str.to_string());
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
                                deleted_paths.push(path_str.to_string());
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

    // Flush cfprefsd's preference cache so it doesn't re-create the
    // bundle's .plist after we just removed it. Only runs if the file
    // loop actually did work (skip for pure dry-run which did nothing
    // on-disk anyway).
    if !dry_run && !bundle_id.is_empty() {
        try_defaults_delete(bundle_id);
    }

    UninstallResult {
        items_removed,
        bytes_freed,
        errors,
        deleted_paths,
    }
}

