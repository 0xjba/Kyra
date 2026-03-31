use std::fs;
use std::path::PathBuf;
use std::process::Command;
use super::{OptResult, OptTask, OptTaskStatus};

/// Runs a shell command and returns (success, output_message).
fn run_shell(command: &str, needs_admin: bool) -> (bool, String) {
    let result = if needs_admin {
        // Use osascript to show native macOS password dialog for admin tasks.
        // Escape backslashes and double-quotes for AppleScript string.
        let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            escaped
        );
        Command::new("osascript").arg("-e").arg(&script).output()
    } else {
        Command::new("sh").arg("-c").arg(command).output()
    };

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                (true, stdout)
            } else {
                let msg = if stderr.is_empty() { stdout } else { stderr };
                (false, msg.trim().to_string())
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Result of a pre-check: either proceed or skip with a reason.
enum PreCheckResult {
    Proceed,
    Skip(String),
}

/// Smart pre-checks for specific tasks.
fn pre_check(task: &OptTask) -> PreCheckResult {
    match task.id.as_str() {
        "memory_purge" => pre_check_memory_pressure(),
        "bluetooth_reset" => pre_check_bluetooth(),
        "spotlight_rebuild" => pre_check_ac_power(),
        _ => PreCheckResult::Proceed,
    }
}

/// Only purge memory if pressure is elevated (warning or critical).
fn pre_check_memory_pressure() -> PreCheckResult {
    let output = Command::new("memory_pressure").output();
    if let Ok(o) = output {
        let text = String::from_utf8_lossy(&o.stdout);
        // memory_pressure reports "The system has X memory" or
        // "System-wide memory free percentage: NN%"
        // The level line contains "normal", "warn", or "critical"
        let text_lower = text.to_lowercase();
        if text_lower.contains("normal") && !text_lower.contains("warn") && !text_lower.contains("critical") {
            return PreCheckResult::Skip(
                "Memory pressure is normal, no purge needed".into(),
            );
        }
    }
    PreCheckResult::Proceed
}

/// Skip Bluetooth reset if audio or HID devices are connected.
fn pre_check_bluetooth() -> PreCheckResult {
    let output = Command::new("system_profiler")
        .args(["SPBluetoothDataType"])
        .output();
    if let Ok(o) = output {
        let text = String::from_utf8_lossy(&o.stdout);
        // Check for connected devices that are audio or input peripherals
        if text.contains("Connected: Yes") {
            // Look for audio or HID device types near "Connected: Yes"
            let text_lower = text.to_lowercase();
            if text_lower.contains("headphone")
                || text_lower.contains("airpods")
                || text_lower.contains("speaker")
                || text_lower.contains("keyboard")
                || text_lower.contains("mouse")
                || text_lower.contains("trackpad")
                || text_lower.contains("audio")
            {
                return PreCheckResult::Skip(
                    "Skipped: Bluetooth audio/HID devices connected".into(),
                );
            }
        }
    }
    PreCheckResult::Proceed
}

/// Skip Spotlight rebuild if running on battery.
fn pre_check_ac_power() -> PreCheckResult {
    let output = Command::new("pmset").args(["-g", "batt"]).output();
    if let Ok(o) = output {
        let text = String::from_utf8_lossy(&o.stdout);
        if !text.contains("AC Power") {
            return PreCheckResult::Skip(
                "Skipped: Connect to AC power before rebuilding Spotlight index".into(),
            );
        }
    }
    PreCheckResult::Proceed
}

// ---------------------------------------------------------------------------
// Custom task runners
// ---------------------------------------------------------------------------

/// Check if any of the given processes are running.
fn is_process_running(name: &str) -> bool {
    if let Ok(output) = Command::new("pgrep").arg("-x").arg(name).output() {
        output.status.success()
    } else {
        false
    }
}

/// Custom SQLite VACUUM runner with smarter checks.
fn run_sqlite_vacuum() -> (bool, String) {
    // Check if Mail, Messages, or Safari are running
    let blockers: Vec<&str> = ["Mail", "Messages", "Safari"]
        .iter()
        .filter(|&&name| is_process_running(name))
        .copied()
        .collect();

    if !blockers.is_empty() {
        return (
            false,
            format!("Close these apps first: {}", blockers.join(", ")),
        );
    }

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let search_dirs = [
        home.join("Library/Mail"),
        home.join("Library/Messages"),
        home.join("Library/Safari"),
    ];

    let mut vacuumed: usize = 0;
    let mut skipped: usize = 0;
    let mut errors: Vec<String> = Vec::new();

    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        // Find .db files recursively
        let db_files = find_db_files(dir);
        for db_path in db_files {
            let db_str = db_path.to_string_lossy().to_string();

            // Skip databases over 100MB
            if let Ok(meta) = fs::metadata(&db_path) {
                if meta.len() > 100 * 1024 * 1024 {
                    skipped += 1;
                    continue;
                }
            }

            // Run integrity check first
            let integrity = Command::new("sqlite3")
                .arg(&db_str)
                .arg("PRAGMA integrity_check;")
                .output();

            if let Ok(o) = integrity {
                let text = String::from_utf8_lossy(&o.stdout);
                if !text.trim().eq_ignore_ascii_case("ok") {
                    errors.push(format!("{}: integrity check failed", db_str));
                    continue;
                }
            } else {
                continue;
            }

            // VACUUM with a 20-second timeout using busy_timeout pragma
            let vacuum_cmd = format!(
                "PRAGMA busy_timeout = 20000; VACUUM;"
            );

            // Use a thread with timeout
            let db_str_clone = db_str.clone();
            let handle = std::thread::spawn(move || {
                Command::new("sqlite3")
                    .arg(&db_str_clone)
                    .arg(&vacuum_cmd)
                    .output()
            });

            match handle.join() {
                Ok(Ok(output)) => {
                    if output.status.success() {
                        vacuumed += 1;
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        errors.push(format!("{}: {}", db_str, stderr.trim()));
                    }
                }
                _ => {
                    errors.push(format!("{}: timed out", db_str));
                }
            }
        }
    }

    let msg = format!(
        "Vacuumed {} databases, skipped {} (>100MB){}",
        vacuumed,
        skipped,
        if errors.is_empty() {
            String::new()
        } else {
            format!(", {} errors", errors.len())
        }
    );
    (true, msg)
}

/// Recursively find *.db files under a directory.
fn find_db_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(find_db_files(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("db") {
                results.push(path);
            }
        }
    }
    results
}

/// Custom plist repair runner.
fn run_plist_repair() -> (bool, String) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let prefs_dir = home.join("Library/Preferences");
    if !prefs_dir.exists() {
        return (true, "Preferences directory not found, nothing to do".into());
    }

    let entries = match fs::read_dir(&prefs_dir) {
        Ok(e) => e,
        Err(e) => return (false, format!("Cannot read Preferences: {}", e)),
    };

    let mut checked: usize = 0;
    let mut repaired: usize = 0;

    for entry in entries.flatten() {
        let path = entry.path();

        // Only process .plist files (not directories)
        if path.is_dir() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("plist") {
            continue;
        }

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip system plists
        if filename.starts_with("com.apple.") || filename.starts_with(".GlobalPreferences") {
            continue;
        }

        checked += 1;

        // Validate with plutil -lint
        let result = Command::new("plutil")
            .arg("-lint")
            .arg(path.to_string_lossy().as_ref())
            .output();

        if let Ok(output) = result {
            if !output.status.success() {
                // Corrupted — remove it
                if fs::remove_file(&path).is_ok() {
                    repaired += 1;
                }
            }
        }
    }

    let msg = format!(
        "Checked {} preference files, removed {} corrupted",
        checked, repaired
    );
    (true, msg)
}

/// Check if a task has a custom runner, and if so, execute it.
/// Returns Some((success, message)) for custom tasks, None for standard tasks.
fn run_custom_task(task: &OptTask) -> Option<(bool, String)> {
    match task.id.as_str() {
        "sqlite_vacuum" => Some(run_sqlite_vacuum()),
        "plist_repair" => Some(run_plist_repair()),
        _ => None,
    }
}

/// Runs the given optimization tasks sequentially.
/// Calls `on_status` with status updates for each task.
pub fn run_tasks<F>(tasks: &[OptTask], mut on_status: F) -> OptResult
where
    F: FnMut(&OptTaskStatus),
{
    let mut succeeded: usize = 0;
    let mut failed: usize = 0;
    let mut skipped: usize = 0;

    for task in tasks {
        // Run smart pre-checks before executing
        match pre_check(task) {
            PreCheckResult::Skip(reason) => {
                skipped += 1;
                on_status(&OptTaskStatus {
                    task_id: task.id.clone(),
                    status: "skipped".into(),
                    message: Some(reason),
                });
                continue;
            }
            PreCheckResult::Proceed => {}
        }

        on_status(&OptTaskStatus {
            task_id: task.id.clone(),
            status: "running".into(),
            message: None,
        });

        // Use custom runner if available, otherwise fall back to shell command
        let (success, message) = if let Some(result) = run_custom_task(task) {
            result
        } else {
            run_shell(&task.command, task.needs_admin)
        };

        if success {
            succeeded += 1;
            on_status(&OptTaskStatus {
                task_id: task.id.clone(),
                status: "done".into(),
                message: if message.trim().is_empty() {
                    None
                } else {
                    Some(message)
                },
            });
        } else {
            failed += 1;
            on_status(&OptTaskStatus {
                task_id: task.id.clone(),
                status: "error".into(),
                message: Some(message),
            });
        }
    }

    OptResult {
        tasks_run: succeeded + failed + skipped,
        tasks_succeeded: succeeded,
        tasks_failed: failed,
        tasks_skipped: skipped,
    }
}
