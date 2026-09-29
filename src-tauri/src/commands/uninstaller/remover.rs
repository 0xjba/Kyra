use std::fs;
use std::path::Path;
use std::process::Command;

use super::{brew, is_shared_home_dot_path, KeptItem, UninstallProgress, UninstallResult};
use crate::commands::{data_guard, shared};
use crate::commands::utils::{canonicalize_for_safety, dir_size, is_critical_path, is_same_or_under};

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

/// Reads `CFBundleExecutable` from the app's Info.plist to get the exact
/// executable name launchd will use for the process. This is the most
/// reliable identifier for process matching — much better than the
/// display name (e.g. "Visual Studio Code" ships an executable called
/// "Code", and "zoom.us" vs display "Zoom").
fn read_bundle_executable_name(app_path: &str) -> Option<String> {
    let plist_path = Path::new(app_path).join("Contents/Info.plist");
    let plist = plist::Value::from_file(&plist_path).ok()?;
    let dict = plist.as_dictionary()?;
    let exec = dict.get("CFBundleExecutable")?.as_string()?;
    if exec.is_empty() {
        None
    } else {
        Some(exec.to_string())
    }
}

/// Check whether any process with exactly the given name is currently
/// running under this user. Relies on sysinfo rather than shelling out to
/// pgrep so we avoid an extra fork per check.
fn is_process_running(name: &str) -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes()
        .values()
        .any(|p| p.name().to_string_lossy() == name)
}

/// Best-effort force-quit of an app before its bundle is removed. Runs
/// three escalating steps and stops at the first one that leaves no
/// matching process behind:
///
/// 1. `launchctl`-style graceful SIGTERM via the `kill` binary.
/// 2. SIGKILL if the process is still alive after a short grace period.
/// 3. AppleScript `tell application "X" to quit` as a final fallback.
///
/// If `CFBundleExecutable` is readable, that exact name is used for
/// matching (avoids false positives on display names like "zoom.us" vs
/// "Zoom"). Returns `true` if the process is confirmed gone afterward.
fn try_force_quit(app_path: &str, display_name: &str, dry_run: bool) -> bool {
    if dry_run {
        return true;
    }

    let target = read_bundle_executable_name(app_path).unwrap_or_else(|| display_name.to_string());
    if target.is_empty() {
        return true;
    }

    if !is_process_running(&target) {
        return true;
    }

    shared::log_operation("UNINSTALL", &target, "force quit: SIGTERM");
    let _ = Command::new("/usr/bin/pkill").args(["-x", &target]).output();
    std::thread::sleep(std::time::Duration::from_millis(2000));

    if !is_process_running(&target) {
        return true;
    }

    shared::log_operation("UNINSTALL", &target, "force quit: SIGKILL");
    let _ = Command::new("/usr/bin/pkill")
        .args(["-9", "-x", &target])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(2000));

    if !is_process_running(&target) {
        return true;
    }

    shared::log_operation("UNINSTALL", &target, "force quit: AppleScript");
    let escaped = applescript_escape(display_name);
    let script = format!("tell application \"{}\" to quit", escaped);
    let _ = Command::new("osascript").arg("-e").arg(&script).output();
    std::thread::sleep(std::time::Duration::from_millis(2000));

    !is_process_running(&target)
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

/// Returns true if the given path is a macOS-managed app container stub
/// that `containermanagerd` protects via the `com.apple.provenance` xattr.
/// These directories are located under `~/Library/Containers/<bundle>/`
/// and are marked by the presence of a
/// `.com.apple.containermanagerd.metadata.plist` file. Removing them via
/// `rm -rf` fails by design — even with admin privileges — because the
/// xattr triggers a kernel-level block.
///
/// User data inside the container will already have been deleted via
/// per-file entries by the time we reach the directory itself, so the
/// leftover stub is harmless and should not be reported as an error.
fn is_protected_container_stub(path: &str) -> bool {
    if !path.contains("/Library/Containers/") {
        return false;
    }
    let p = Path::new(path);
    if !p.is_dir() {
        return false;
    }
    p.join(".com.apple.containermanagerd.metadata.plist").exists()
}

/// Inspect the app's `Info.plist` to see if it declares a need for the
/// Local Network permission — either via `NSLocalNetworkUsageDescription`
/// (explicit usage string) or by registering Bonjour services with
/// `NSBonjourServices`. On macOS 15+ this permission is tracked in
/// `/private/var/db/tcc.db` under a name that may not be cleared when the
/// bundle is removed, so the user can end up with a zombie permission
/// entry in System Settings → Privacy & Security → Local Network.
///
/// Must run *before* the bundle is deleted — once `Info.plist` is gone we
/// have no way to recover these keys.
fn declares_local_network_usage(app_path: &str) -> bool {
    let plist_path = Path::new(app_path).join("Contents/Info.plist");
    let plist = match plist::Value::from_file(&plist_path) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let dict = match plist.as_dictionary() {
        Some(d) => d,
        None => return false,
    };
    if dict.contains_key("NSLocalNetworkUsageDescription") {
        return true;
    }
    if dict.contains_key("NSBonjourServices") {
        return true;
    }
    false
}

/// Scan `/Library/SystemExtensions` for any `.systemextension` bundle whose
/// on-disk path contains the app's bundle identifier. System extensions
/// (network filters, camera sensors, endpoint security agents, etc.) are
/// activated through the SystemExtension framework and cannot be removed
/// by deleting the .app alone — they stay active until the user manually
/// approves a deactivation request in System Settings → Privacy & Security.
///
/// This returns `true` if an orphaned extension is likely to remain after
/// the uninstall so the caller can surface a warning. The bundle id is
/// validated against a strict charset to prevent glob/path injection.
fn has_system_extensions(bundle_id: &str) -> bool {
    if bundle_id.is_empty() {
        return false;
    }
    let valid = bundle_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
    if !valid {
        return false;
    }

    let root = Path::new("/Library/SystemExtensions");
    if !root.is_dir() {
        return false;
    }

    // walkdir-style manual recursion with a depth cap of 3 to mirror the
    // reference behavior (staging/<uuid>/<bundle>.systemextension). Anything
    // deeper isn't relevant and we want to bound the work.
    fn walk(dir: &Path, depth: usize, bundle_id: &str) -> bool {
        if depth == 0 {
            return false;
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return false,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.ends_with(".systemextension") {
                if path.to_string_lossy().contains(bundle_id) {
                    return true;
                }
            }

            if path.is_dir() && walk(&path, depth - 1, bundle_id) {
                return true;
            }
        }
        false
    }

    walk(root, 3, bundle_id)
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

/// Post-batch Launch Services rebuild: `lsregister -gc` (garbage collect)
/// then `lsregister -r -f -domain local -domain user -domain system`
/// (force re-scan). Falls back to a lighter rebuild (local + user only) if
/// the full rebuild times out. Failures are non-fatal.
fn try_lsregister_rebuild() {
    if !Path::new(LSREGISTER).exists() {
        return;
    }
    // Phase 1: garbage-collect (10s timeout)
    let _ = Command::new(LSREGISTER)
        .arg("-gc")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(10);
            loop {
                match child.try_wait()? {
                    Some(_) => return Ok(()),
                    None if start.elapsed() > timeout => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(());
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
        });

    // Phase 2: full rebuild (15s timeout, fallback to lighter version)
    let full_rebuild = Command::new(LSREGISTER)
        .args(["-r", "-f", "-domain", "local", "-domain", "user", "-domain", "system"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(15);
            loop {
                match child.try_wait()? {
                    Some(status) => return Ok(status.success()),
                    None if start.elapsed() > timeout => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(false);
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
        });

    // If full rebuild timed out or failed, try lighter version (local + user only)
    if let Ok(false) = full_rebuild {
        shared::log_operation("UNINSTALL", LSREGISTER, "full rebuild timed out, trying lighter version");
        let _ = Command::new(LSREGISTER)
            .args(["-r", "-f", "-domain", "local", "-domain", "user"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                let start = std::time::Instant::now();
                let timeout = std::time::Duration::from_secs(10);
                loop {
                    match child.try_wait()? {
                        Some(_) => return Ok(()),
                        None if start.elapsed() > timeout => {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Ok(());
                        }
                        None => std::thread::sleep(std::time::Duration::from_millis(100)),
                    }
                }
            });
    }

    shared::log_operation("UNINSTALL", LSREGISTER, "post-batch rebuild");
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
        if path.eq_ignore_ascii_case(protected) {
            return false;
        }
        // Special case: allow /Applications/*.app but block /Applications itself
        let apps_prefix = "/Applications/";
        if *protected == "/Applications"
            && path
                .get(..apps_prefix.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(apps_prefix))
        {
            let remainder = &path[apps_prefix.len()..];
            if remainder.contains('/') {
                // It's a path inside an app bundle — allow
                continue;
            }
            if !remainder.ends_with(".app") {
                return false;
            }
            continue;
        }
        if is_same_or_under(path, protected) {
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
        if is_same_or_under(&canonical_str, protected) {
            return false;
        }
    }

    if is_critical_path(Path::new(path)) || is_critical_path(&canonical) {
        return false;
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
        let canonical_home = fs::canonicalize(&home).unwrap_or_else(|_| home.clone());
        for h in [&home, &canonical_home] {
            if is_shared_home_dot_path(Path::new(path), h) || is_shared_home_dot_path(&canonical, h) {
                return false;
            }
        }
    }

    true
}

/// Remove the app from the Dock persistent apps list.
fn try_remove_from_dock(bundle_id: &str) {
    let dock_plist = match dirs::home_dir() {
        Some(h) => h.join("Library/Preferences/com.apple.dock.plist"),
        None => return,
    };

    if !dock_plist.exists() {
        return;
    }

    // Read number of persistent-apps
    let count_output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg("Print :persistent-apps")
        .arg(dock_plist.to_string_lossy().as_ref())
        .output();

    if let Ok(o) = count_output {
        let text = String::from_utf8_lossy(&o.stdout);
        let count = text.matches("Dict {").count();

        // Iterate backwards to safely remove by index
        for i in (0..count).rev() {
            let check_cmd = format!(
                "Print :persistent-apps:{}:tile-data:bundle-identifier",
                i
            );
            if let Ok(co) = Command::new("/usr/libexec/PlistBuddy")
                .arg("-c")
                .arg(&check_cmd)
                .arg(dock_plist.to_string_lossy().as_ref())
                .output()
            {
                let bid = String::from_utf8_lossy(&co.stdout).trim().to_string();
                if bid.eq_ignore_ascii_case(bundle_id) {
                    let delete_cmd = format!("Delete :persistent-apps:{}", i);
                    let _ = Command::new("/usr/libexec/PlistBuddy")
                        .arg("-c")
                        .arg(&delete_cmd)
                        .arg(dock_plist.to_string_lossy().as_ref())
                        .output();
                }
            }
        }
    }

    let _ = Command::new("killall").arg("Dock").output();
}

/// Running totals of an uninstall's file-deletion pass.
struct Tally {
    bytes_freed: u64,
    items_removed: usize,
    errors: Vec<String>,
    deleted_paths: Vec<String>,
    kept: Vec<KeptItem>,
}

/// Deletes `all_paths` one by one after the safety and browser-data checks.
fn delete_listed_paths<F>(
    all_paths: &[&str],
    bundle_id: &str,
    dry_run: bool,
    permanent: bool,
    t: &mut Tally,
    on_progress: &mut F,
) where
    F: FnMut(&UninstallProgress),
{
    let items_total = all_paths.len();

    for (i, path_str) in all_paths.iter().enumerate() {
        let path = Path::new(path_str);

        // Safety check
        if !is_safe_path(path_str) {
            t.errors.push(format!("Skipped protected path: {}", path_str));
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed: t.bytes_freed,
            });
            continue;
        }

        // Browser profiles survive unless this is that exact browser's
        // uninstall; wallets, messages, VMs and other data that outlives
        // the app are kept and reported.
        if let Err(refusal) = data_guard::check(path, data_guard::DeleteContext::Uninstall { bundle_id }) {
            shared::log_operation("UNINSTALL", path_str, &format!("SKIPPED: {}", refusal));
            if refusal.category == data_guard::Category::Browser {
                t.errors.push(format!("Skipped {}", refusal));
            } else if !t.kept.iter().any(|k| k.path == refusal.path) {
                t.kept.push(KeptItem::from_refusal(&refusal));
            }
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed: t.bytes_freed,
            });
            continue;
        }

        if !path.exists() {
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed: t.bytes_freed,
            });
            continue;
        }

        // macOS-managed container stubs can't be removed via rm -rf because
        // containermanagerd protects them with the com.apple.provenance
        // xattr. Any user data inside will have been deleted via per-file
        // entries before we reach this point, so we skip the stub itself
        // silently — attempting would just trigger a pointless admin prompt
        // that still ends in failure.
        if is_protected_container_stub(path_str) {
            shared::log_operation(
                "UNINSTALL",
                path_str,
                "SKIPPED: protected container stub",
            );
            on_progress(&UninstallProgress {
                current_item: path_str.to_string(),
                items_done: i + 1,
                items_total,
                bytes_freed: t.bytes_freed,
            });
            continue;
        }

        let size = if path.is_dir() {
            dir_size(path)
        } else {
            path.metadata().map(|m| m.len()).unwrap_or(0)
        };

        if dry_run {
            t.bytes_freed += size;
            t.items_removed += 1;
            t.deleted_paths.push(path_str.to_string());
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
                    t.bytes_freed += size;
                    t.items_removed += 1;
                    t.deleted_paths.push(path_str.to_string());
                    let action = if permanent { "DELETED" } else { "TRASHED" };
                    shared::log_operation("UNINSTALL", path_str, action);
                }
                Err(e) => {
                    // If permission denied, retry with admin privileges (osascript prompt)
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        shared::log_operation("UNINSTALL", path_str, "ESCALATING: requesting admin privileges");
                        match privileged_delete(path_str, permanent) {
                            Ok(()) => {
                                t.bytes_freed += size;
                                t.items_removed += 1;
                                t.deleted_paths.push(path_str.to_string());
                                let action = if permanent { "DELETED (admin)" } else { "TRASHED (admin)" };
                                shared::log_operation("UNINSTALL", path_str, action);
                            }
                            Err(priv_e) => {
                                shared::log_operation("UNINSTALL", path_str, &format!("ERROR: {}", priv_e));
                                t.errors.push(format!("{}: {}", path_str, priv_e));
                            }
                        }
                    } else {
                        shared::log_operation("UNINSTALL", path_str, &format!("ERROR: {}", e));
                        t.errors.push(format!("{}: {}", path_str, e));
                    }
                }
            }
        }

        on_progress(&UninstallProgress {
            current_item: path_str.to_string(),
            items_done: i + 1,
            items_total,
            bytes_freed: t.bytes_freed,
        });
    }

}

/// Removes the app bundle and selected associated files.
/// Calls `on_progress` after each item is processed.
///
/// If `brew_cask` is Some, the cask is uninstalled first with
/// `brew uninstall --cask` (never `--zap`, which bypasses the data guard).
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
    let bytes_freed: u64 = 0;
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

    // Inspect Info.plist *before* deletion so we can warn about macOS 15+
    // Local Network permissions that may outlive the bundle.
    let has_local_network = declares_local_network_usage(app_path);

    // Force-quit the app if it's running. brew uninstall, file deletion,
    // and launchctl unload all fail (or leave zombies) if the bundle's
    // process is still alive. Uses CFBundleExecutable for exact matching
    // so display-name variants (e.g. "Zoom" vs "zoom.us", "Visual Studio
    // Code" vs "Code") don't cause false negatives.
    try_force_quit(app_path, &app_display_name, dry_run);

    // If this app is a Homebrew cask, let brew handle the payload + zap
    // stanzas first. The file-deletion loop below still runs afterwards to
    // clean up anything brew didn't know about (user caches, orphaned
    // launch agents, etc.).
    // Calculate app size for brew timeout scaling
    let app_size_bytes = {
        let p = Path::new(app_path);
        if p.is_dir() {
            crate::commands::utils::dir_size(p)
        } else {
            p.metadata().map(|m| m.len()).unwrap_or(0)
        }
    };

    let brew_handled_app = if let Some(cask) = brew_cask.as_deref() {
        match brew::uninstall_cask_with_size(cask, dry_run, app_size_bytes) {
            Ok(log_line) => {
                shared::log_operation(
                    "UNINSTALL",
                    app_path,
                    &format!("brew uninstall {}: {}", cask, log_line.lines().next().unwrap_or("ok")),
                );
                // Clean up orphaned brew dependencies in background (30s timeout)
                std::thread::spawn(|| {
                    if let Some(brew_bin) = brew::brew_binary() {
                        if let Ok(mut child) = std::process::Command::new(brew_bin)
                            .arg("autoremove")
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                        {
                            let start = std::time::Instant::now();
                            let timeout = std::time::Duration::from_secs(30);
                            loop {
                                match child.try_wait() {
                                    Ok(Some(_)) => break,
                                    Ok(None) => {
                                        if start.elapsed() > timeout {
                                            let _ = child.kill();
                                            let _ = child.wait();
                                            break;
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(200));
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                    }
                });

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
                    &format!("brew uninstall {} failed: {}", cask, e),
                );

                // 3-way cask-state fallback after brew failure:
                //   - cask gone (not in `brew list --cask`): brew partially
                //     succeeded — the cask record is removed but the .app
                //     bundle may remain on disk. Proceed with manual deletion.
                //   - cask still installed: brew failed entirely. Don't touch
                //     the .app to avoid a state mismatch where brew still
                //     reports it as installed. Surface a suggestion instead.
                //   - unknown (brew list failed): surface a suggestion.
                let cask_still_registered = brew::is_cask_installed(cask);

                if !cask_still_registered {
                    // Cask record is gone — brew partially cleaned up.
                    // Fall through to the normal file-deletion loop which
                    // will handle the .app bundle manually.
                    shared::log_operation(
                        "UNINSTALL",
                        app_path,
                        &format!("brew cask {} no longer registered, will delete .app manually", cask),
                    );
                    errors.push(format!("brew uninstall partially succeeded: {}", e));
                    false
                } else {
                    // Cask is still registered (or state unknown) — don't
                    // delete the .app behind brew's back. Suggest the user
                    // runs `brew uninstall` manually.
                    shared::log_operation(
                        "UNINSTALL",
                        app_path,
                        &format!("brew cask {} still registered, skipping manual .app delete", cask),
                    );
                    errors.push(format!(
                        "brew uninstall failed: {}. Run `brew uninstall --cask {}` manually.",
                        e, cask
                    ));
                    // Return true to prevent the file-deletion loop from
                    // removing the .app bundle while brew still owns it.
                    true
                }
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

    let mut tally = Tally { bytes_freed, items_removed, errors, deleted_paths, kept: Vec::new() };
    delete_listed_paths(&all_paths, bundle_id, dry_run, permanent, &mut tally, &mut on_progress);
    let Tally { mut bytes_freed, items_removed, mut errors, mut deleted_paths, kept } = tally;

    // Verify deletions — adjust bytes_freed for files that survived
    if !dry_run && !deleted_paths.is_empty() {
        let mut surviving_bytes: u64 = 0;
        deleted_paths.retain(|p| {
            let path = std::path::Path::new(p);
            if path.exists() {
                // File survived deletion — don't count its bytes
                let size = if path.is_dir() { dir_size(path) } else { path.metadata().map(|m| m.len()).unwrap_or(0) };
                surviving_bytes += size;
                false
            } else {
                true
            }
        });
        if surviving_bytes > 0 {
            bytes_freed = bytes_freed.saturating_sub(surviving_bytes);
        }
    }

    // Flush cfprefsd's preference cache so it doesn't re-create the
    // bundle's .plist after we just removed it. Only runs if the file
    // loop actually did work (skip for pure dry-run which did nothing
    // on-disk anyway).
    if !dry_run && !bundle_id.is_empty() {
        try_defaults_delete(bundle_id);
    }

    // Remove the app from the Dock if it was pinned there
    if !dry_run && !bundle_id.is_empty() {
        try_remove_from_dock(bundle_id);
    }

    // Compact and rebuild the LaunchServices database so stale entries
    // (Spotlight results, "Open with…" menus, default app mappings) are
    // flushed immediately rather than lingering for days. Per-file
    // lsregister -u handled individual bundles above; this final pass
    // garbage-collects and forces a full re-scan.
    if !dry_run && items_removed > 0 {
        try_lsregister_rebuild();
    }

    // Warn about Local Network permissions. TCC.db entries for this
    // permission are keyed by bundle id and survive bundle removal on
    // macOS 15+, leaving a stale entry in System Settings.
    if has_local_network {
        let name_for_msg = if app_display_name.is_empty() {
            bundle_id
        } else {
            app_display_name.as_str()
        };
        let msg = format!(
            "Warning: {} requested Local Network access. On macOS 15+ this permission may remain listed in System Settings → Privacy & Security → Local Network.",
            name_for_msg
        );
        shared::log_operation("UNINSTALL", bundle_id, "local network permission advisory");
        errors.push(msg);
    }

    // Detect orphaned system extensions that the SystemExtension framework
    // still owns. These cannot be removed by file deletion — the user must
    // approve a deactivation in System Settings. Surface the situation as a
    // non-fatal advisory so the caller can show it to the user.
    if !bundle_id.is_empty() && has_system_extensions(bundle_id) {
        let name_for_msg = if app_display_name.is_empty() {
            bundle_id
        } else {
            app_display_name.as_str()
        };
        let msg = format!(
            "Warning: {} installed a system extension that may remain active. Open System Settings → General → Login Items & Extensions to review it.",
            name_for_msg
        );
        shared::log_operation("UNINSTALL", bundle_id, "system extension detected (advisory)");
        errors.push(msg);
    }

    UninstallResult {
        items_removed,
        bytes_freed,
        errors,
        deleted_paths,
        kept,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{mkdir, s, write_file};
    use std::os::unix::fs::symlink;

    fn uninstall_files(paths: &[String], bundle_id: &str) -> Tally {
        let refs: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();
        let mut t = Tally { bytes_freed: 0, items_removed: 0, errors: vec![], deleted_paths: vec![], kept: vec![] };
        delete_listed_paths(&refs, bundle_id, false, true, &mut t, &mut |_| {});
        t
    }

    #[test]
    fn uninstalling_look_alike_apps_never_touches_browser_profiles() {
        use crate::commands::browser_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        use crate::commands::uninstaller::associated::find_associated_in;
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let hostile: Vec<String> = fx.protected.keys().chain(fx.protected_dirs.iter()).map(|p| s(p)).collect();

        for (bundle_id, name) in [
            ("com.google.Keystone", "Google"),
            ("com.google.Chrome.helper", "Google Chrome Helper"),
            ("company.thebrowser.something-else", "Arc"),
            ("company.thebrowser.dia.helper", "Dia Helper"),
            ("org.mozilla.nightly", "Firefox Nightly"),
            ("com.microsoft.edgemac.Beta", "Microsoft Edge Beta"),
            ("com.apple.SafariTechnologyPreview", "Safari Technology Preview"),
            ("com.kagi.kagimacOS.RC", "Orion RC"),
            ("net.imput.helium.beta", "Helium Beta"),
        ] {
            let found: Vec<String> = find_associated_in(bundle_id, name, "", &fx.home, false)
                .into_iter()
                .map(|f| f.path)
                .collect();
            for p in &found {
                for file in fx.protected.keys() {
                    assert!(!file.starts_with(p), "uninstalling {bundle_id} offers {p}");
                }
            }
            let mut all = found;
            all.extend(hostile.iter().cloned());
            let t = uninstall_files(&all, bundle_id);
            fx.assert_profiles_intact();
            assert!(t.errors.iter().any(|e| e.contains(crate::commands::browser_guard::ERROR_CODE)));
        }
    }

    #[test]
    fn uninstalling_apps_never_deletes_data_that_outlives_them() {
        use crate::commands::data_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        use crate::commands::uninstaller::associated::discover_in;
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let hostile: Vec<String> = fixtures::PROTECTED
            .iter()
            .map(|rel| s(&fx.path(rel)))
            .chain(fx.protected_dirs.iter().map(|p| s(p)))
            .collect();

        for (bundle_id, name, must_keep) in [
            ("org.whispersystems.signal-desktop", "Signal", &["Library/Application Support/Signal"][..]),
            ("org.whispersystems.signal-desktop.beta", "Signal Beta", &["Library/Application Support/Signal"]),
            ("org.electrum.electrum", "Electrum", &[".electrum"]),
            ("com.ledger.live", "Ledger Live", &["Library/Application Support/Ledger Live"]),
            ("com.bitwarden.desktop", "Bitwarden", &["Library/Application Support/Bitwarden"]),
            ("ru.keepcoder.Telegram", "Telegram", &["Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram"]),
            ("com.tinyspeck.slackmacgap", "Slack", &["Library/Application Support/Slack"]),
            ("com.hnc.Discord", "Discord", &["Library/Application Support/discord"]),
            ("com.utmapp.UTM", "UTM", &["Library/Containers/com.utmapp.UTM"]),
            ("com.valvesoftware.steam", "Steam", &["Library/Application Support/Steam"]),
            ("com.google.android.studio", "Android Studio", &[".android/avd"]),
            ("com.huawei.deveco.studio", "DevEco Studio", &[]),
            ("com.xcodesorg.xcodesapp", "Xcodes", &[]),
            ("com.getdropbox.dropbox", "Dropbox", &[]),
            ("md.obsidian", "Obsidian", &[]),
            ("com.example.community", "Community", &[]),
        ] {
            let found = discover_in(bundle_id, name, "", &fx.home, false);
            for f in &found.files {
                for file in fx.protected.keys() {
                    assert!(!file.starts_with(&f.path), "uninstalling {name} offers {} which holds {}", f.path, s(file));
                }
            }
            let kept: Vec<&str> = found.kept.iter().map(|k| k.path.as_str()).collect();
            for rel in must_keep {
                assert!(kept.contains(&s(&fx.path(rel)).as_str()), "{name}: {rel} not reported kept: {kept:#?}");
            }
            assert!(found.kept.iter().all(|k| k.message.starts_with("Kept your ")));

            let mut all: Vec<String> = found.files.iter().map(|f| f.path.clone()).collect();
            all.extend(hostile.iter().cloned());
            let t = uninstall_files(&all, bundle_id);
            fx.assert_intact();
            assert!(!t.kept.is_empty(), "{name}: refusals must be reported as kept");
        }
    }

    #[test]
    fn uninstalling_an_electron_app_still_removes_its_own_data() {
        use crate::commands::test_support::{canon, workspace_tempdir};
        use crate::commands::uninstaller::associated::discover_in;
        let dir = workspace_tempdir();
        let home = canon(&dir).join("home");
        let data = home.join("Library/Application Support/Postman");
        write_file(&data.join("Local State"), 10);
        write_file(&data.join("Preferences"), 10);
        write_file(&data.join("IndexedDB/x/000003.log"), 10);
        let found = discover_in("com.postmanlabs.mac", "Postman", "", &home, false);
        assert_eq!(found.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec![s(&data)]);
        assert!(found.kept.is_empty());
    }

    #[test]
    fn uninstalling_a_browser_removes_only_its_own_data() {
        use crate::commands::browser_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let own = [
            fx.home.join("Library/Application Support/Arc"),
            fx.home.join("Library/Saved Application State/company.thebrowser.Browser.savedState"),
            fx.home.join("Library/Preferences/company.thebrowser.Browser.plist"),
            fx.home.join("Library/Group Containers/S6N382Y83G.company.thebrowser.Browser"),
        ];
        let others = [
            fx.home.join("Library/Application Support/Dia"),
            fx.home.join("Library/Application Support/Google/Chrome"),
            fx.home.join("Library/Saved Application State/com.google.Chrome.savedState"),
        ];
        let paths: Vec<String> = own.iter().chain(others.iter()).map(|p| s(p)).collect();

        let t = uninstall_files(&paths, "company.thebrowser.Browser");

        for p in &own {
            assert!(!p.exists(), "{}", p.display());
        }
        for p in &others {
            assert!(p.exists(), "{}", p.display());
        }
        assert_eq!(t.items_removed, 4, "{:?}", t.errors);
    }

    #[test]
    fn system_and_root_paths_are_not_safe() {
        for p in [
            "/",
            "/System",
            "/System/Applications/Mail.app",
            "/usr/bin/python3",
            "/USR/BIN/python3",
            "/Library",
            "/Library/Frameworks/Foo.framework",
            "/Users",
            "/Volumes",
            "/etc/hosts",
        ] {
            assert!(!is_safe_path(p), "{p}");
        }
    }

    #[test]
    fn applications_folder_allows_only_app_bundles() {
        assert!(!is_safe_path("/Applications"));
        assert!(!is_safe_path("/Applications/"));
        assert!(!is_safe_path("/Applications/Utilities"));
        assert!(!is_safe_path("/applications/Utilities"));
        assert!(!is_safe_path("/Applications/readme.txt"));
        assert!(is_safe_path("/Applications/NotInstalledKyraTest.app"));
        assert!(is_safe_path("/Applications/NotInstalledKyraTest.app/Contents/Info.plist"));
    }

    #[test]
    fn home_and_home_folders_are_not_safe() {
        let home = dirs::home_dir().unwrap();
        let h = s(&home);
        assert!(!is_safe_path(&h));
        assert!(!is_safe_path(&format!("{h}/")));
        for d in ["Desktop", "Documents", "Downloads", "Library", "Pictures", "Music", "Movies", "Public"] {
            assert!(!is_safe_path(&format!("{h}/{d}")), "{d}");
            assert!(!is_safe_path(&format!("{h}/{}", d.to_lowercase())), "{d}");
        }
        assert!(is_safe_path(&format!("{h}/Library/Caches/com.example.kyra-test")));
        assert!(is_safe_path(&format!("{h}/Library/Preferences/com.example.kyra-test.plist")));
    }

    #[test]
    fn shared_home_dot_entries_are_not_safe() {
        let h = s(&dirs::home_dir().unwrap());
        for rel in [
            ".ssh", ".SSH", ".ssh/", "./.ssh", ".ssh/id_ed25519", ".gnupg", ".config", ".local",
            ".local/share", ".cache", ".aws", ".aws/credentials", ".kube", ".docker", ".npm",
            ".cargo", ".rustup", ".gitconfig", ".zshrc", ".bashrc", ".bash_profile", ".profile",
            ".zprofile", ".Trash", ".vscode", ".git", ".go",
        ] {
            assert!(!is_safe_path(&format!("{h}/{rel}")), "{rel}");
        }
        assert!(is_safe_path(&format!("{h}/.config/kyra-test-app")));
        assert!(is_safe_path(&format!("{h}/.kyra-test-app")));
    }

    #[test]
    fn dry_run_uninstall_refuses_shared_home_dot_entries() {
        let h = s(&dirs::home_dir().unwrap());
        let paths: Vec<String> = [".ssh", ".config", ".zshrc", ".aws/credentials"]
            .iter()
            .map(|rel| format!("{h}/{rel}"))
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Foo.app");
        write_file(&app.join("Contents/MacOS/foo"), 10);
        let result = remove_app_and_files(&s(&app), &paths, "", None, true, false, |_| {});
        assert_eq!(result.deleted_paths, vec![s(&app)]);
        for p in &paths {
            assert!(result.errors.iter().any(|e| e.contains(p.as_str())), "{p}");
        }
    }

    #[test]
    fn malformed_and_traversal_paths_are_not_safe() {
        assert!(!is_safe_path(""));
        assert!(!is_safe_path("/Applications/Foo.app/../../System"));
        assert!(!is_safe_path("/Users/x/Library/Caches/../../../../usr"));
        assert!(!is_safe_path("/tmp/evil\n/System"));
    }

    #[test]
    fn symlinks_are_judged_by_where_they_point() {
        let dir = tempfile::tempdir().unwrap();
        let to_system = dir.path().join("Evil.app");
        symlink("/System/Library", &to_system).unwrap();
        assert!(!is_safe_path(&s(&to_system)));

        let to_apps = dir.path().join("apps");
        symlink("/Applications", &to_apps).unwrap();
        assert!(!is_safe_path(&s(&to_apps)));

        let to_home = dir.path().join("home");
        symlink(dirs::home_dir().unwrap(), &to_home).unwrap();
        assert!(!is_safe_path(&s(&to_home)));

        let ok = dir.path().join("cache");
        mkdir(&ok);
        assert!(is_safe_path(&s(&ok)));
    }

    #[test]
    fn dry_run_uninstall_skips_protected_paths_and_deletes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Foo.app");
        write_file(&app.join("Contents/MacOS/foo"), 4_000);
        let support = dir.path().join("support/com.example.foo");
        write_file(&support.join("data.bin"), 2_000);
        let home = s(&dirs::home_dir().unwrap());

        let files = vec![
            s(&support),
            "/".to_string(),
            "/System/Library".to_string(),
            home.clone(),
            format!("{home}/Documents"),
            s(&dir.path().join("already-gone")),
        ];
        let mut progress = Vec::new();
        let result = remove_app_and_files(&s(&app), &files, "", None, true, true, |p| {
            progress.push(p.items_done)
        });

        assert_eq!(result.items_removed, 2);
        assert_eq!(result.deleted_paths, vec![s(&support), s(&app)]);
        assert!(result.bytes_freed >= 6_000);
        assert_eq!(
            result.errors.iter().filter(|e| e.starts_with("Skipped protected path")).count(),
            4,
            "{:?}",
            result.errors
        );
        assert_eq!(progress, (1..=files.len() + 1).collect::<Vec<_>>());
        assert!(app.join("Contents/MacOS/foo").exists());
        assert!(support.join("data.bin").exists());
    }

    #[test]
    fn local_network_usage_is_read_from_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Net.app");
        mkdir(&app.join("Contents"));
        let plist_path = app.join("Contents/Info.plist");

        let mut dict = plist::Dictionary::new();
        dict.insert("CFBundleExecutable".into(), plist::Value::String("NetBin".into()));
        plist::Value::Dictionary(dict.clone()).to_file_xml(&plist_path).unwrap();
        assert!(!declares_local_network_usage(&s(&app)));
        assert_eq!(read_bundle_executable_name(&s(&app)).as_deref(), Some("NetBin"));

        dict.insert("NSBonjourServices".into(), plist::Value::Array(vec![]));
        plist::Value::Dictionary(dict).to_file_xml(&plist_path).unwrap();
        assert!(declares_local_network_usage(&s(&app)));

        let result = remove_app_and_files(&s(&app), &[], "", None, true, true, |_| {});
        assert!(result.errors.iter().any(|e| e.contains("Local Network")));
        assert!(app.exists());

        assert!(!declares_local_network_usage(&s(&dir.path().join("Missing.app"))));
        assert_eq!(read_bundle_executable_name(&s(&dir.path().join("Missing.app"))), None);
    }

    #[test]
    fn container_stub_needs_containers_path_and_marker() {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("Library/Containers/com.example.foo");
        mkdir(&stub);
        assert!(!is_protected_container_stub(&s(&stub)));
        write_file(&stub.join(".com.apple.containermanagerd.metadata.plist"), 1);
        assert!(is_protected_container_stub(&s(&stub)));

        let elsewhere = dir.path().join("Other/com.example.foo");
        write_file(&elsewhere.join(".com.apple.containermanagerd.metadata.plist"), 1);
        assert!(!is_protected_container_stub(&s(&elsewhere)));
    }

    #[test]
    fn launchd_plist_detection() {
        assert!(is_launchd_plist("/Library/LaunchDaemons/com.foo.helper.plist"));
        assert!(is_launchd_plist("/Users/x/Library/LaunchAgents/com.foo.plist"));
        assert!(is_launchd_plist("/Library/PrivilegedHelperTools/com.foo.plist"));
        assert!(!is_launchd_plist("/Library/LaunchDaemons/com.foo.helper"));
        assert!(!is_launchd_plist("/Users/x/Library/Preferences/com.foo.plist"));
    }

    #[test]
    fn shell_and_applescript_escaping_neutralise_quotes() {
        assert_eq!(shell_escape("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_escape("/tmp/it's"), "'/tmp/it'\\''s'");
        assert_eq!(shell_escape("/tmp/$(rm -rf ~)"), "'/tmp/$(rm -rf ~)'");
        assert_eq!(applescript_escape(r#"My "App""#), r#"My \"App\""#);
        assert_eq!(applescript_escape(r"a\b"), r"a\\b");
    }

    #[test]
    fn system_apps_are_recognised() {
        assert!(is_system_app("/System/Applications/Mail.app"));
        assert!(!is_system_app("/Applications/Mail.app"));
    }

    #[test]
    fn invalid_bundle_ids_never_reach_system_extension_lookup() {
        assert!(!has_system_extensions(""));
        assert!(!has_system_extensions("../../etc"));
        assert!(!has_system_extensions("com.foo;rm"));
    }
}
