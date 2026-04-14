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
        "spotlight_rebuild" => pre_check_spotlight(),
        "network_flush" => pre_check_network_flush(),
        "font_cache" => pre_check_browsers_closed(),
        "periodic_maintenance" => pre_check_periodic(),
        "disk_permissions" => pre_check_disk_permissions(),
        _ => PreCheckResult::Proceed,
    }
}

/// Only purge memory if pressure is elevated (warning or critical).
fn pre_check_memory_pressure() -> PreCheckResult {
    let output = Command::new("memory_pressure").arg("-Q").output();
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
/// Also checks SPAudioDataType for BT audio output and whether media apps
/// are running with a BT device connected.
fn pre_check_bluetooth() -> PreCheckResult {
    let output = Command::new("system_profiler")
        .args(["SPBluetoothDataType"])
        .output();
    let bt_connected = if let Ok(o) = &output {
        let text = String::from_utf8_lossy(&o.stdout);
        if text.contains("Connected: Yes") {
            // Look for HID device types
            let text_lower = text.to_lowercase();
            if text_lower.contains("keyboard")
                || text_lower.contains("mouse")
                || text_lower.contains("trackpad")
            {
                return PreCheckResult::Skip(
                    "Skipped: Bluetooth HID devices connected".into(),
                );
            }
            true
        } else {
            false
        }
    } else {
        false
    };

    // Cross-check SPAudioDataType for BT audio output
    if let Ok(audio_out) = Command::new("system_profiler")
        .args(["SPAudioDataType"])
        .output()
    {
        let audio_text = String::from_utf8_lossy(&audio_out.stdout);
        // Check if default output device uses Bluetooth transport
        let audio_lower = audio_text.to_lowercase();
        if audio_lower.contains("default output device: yes") {
            // Look for Bluetooth transport near the default output section
            if audio_lower.contains("transport: bluetooth")
                || audio_lower.contains("airpods")
                || audio_lower.contains("headphone")
                || audio_lower.contains("speaker")
            {
                return PreCheckResult::Skip(
                    "Skipped: Bluetooth audio output active".into(),
                );
            }
        }
    }

    // If BT is connected, check if media apps are running (audio may be streaming)
    if bt_connected {
        let media_apps = [
            "Music", "Spotify", "VLC", "QuickTime Player", "TV", "Podcasts",
            "Safari", "Google Chrome", "Chrome", "Firefox", "Arc", "IINA", "mpv",
        ];
        for app in &media_apps {
            if is_process_running(app) {
                return PreCheckResult::Skip(
                    format!("Skipped: {} is running with Bluetooth connected", app),
                );
            }
        }
    }

    PreCheckResult::Proceed
}

/// Skip font cache rebuild if any browser is running.
fn pre_check_browsers_closed() -> PreCheckResult {
    // Check for browser main processes AND helper processes
    let browser_patterns = [
        ("Safari", "Safari"),
        ("Chrome", "Google Chrome"),
        ("Firefox", "firefox"),
        ("Edge", "Microsoft Edge"),
        ("Brave", "Brave Browser"),
        ("Arc", "Arc"),
        ("Opera", "Opera"),
        ("Vivaldi", "Vivaldi"),
        ("Chromium", "Chromium"),
        ("Zen", "Zen"),
        ("Orion", "Orion"),
    ];

    let mut running: Vec<&str> = Vec::new();
    for (label, pattern) in &browser_patterns {
        if let Ok(output) = Command::new("pgrep").args(["-if", pattern]).output() {
            if output.status.success() {
                running.push(label);
            }
        }
    }

    if !running.is_empty() {
        PreCheckResult::Skip(format!(
            "Skipped: close {} first",
            running.join(", ")
        ))
    } else {
        PreCheckResult::Proceed
    }
}

/// Skip Spotlight rebuild if running on battery or if Spotlight is already fast.
/// Runs mdfind twice and only proceeds if both queries take >3s.
fn pre_check_spotlight() -> PreCheckResult {
    // First check AC power
    let output = Command::new("pmset").args(["-g", "batt"]).output();
    if let Ok(o) = output {
        let text = String::from_utf8_lossy(&o.stdout);
        if !text.contains("AC Power") {
            return PreCheckResult::Skip(
                "Skipped: Connect to AC power before rebuilding Spotlight index".into(),
            );
        }
    }

    // Check if Spotlight indexing is even enabled
    if let Ok(o) = Command::new("mdutil").args(["-s", "/"]).output() {
        let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
        if text.contains("indexing disabled") {
            return PreCheckResult::Skip(
                "Spotlight indexing is disabled".into(),
            );
        }
    }

    // Speed test: run mdfind twice with a 1-second gap to avoid OS cache bias
    let mut slow_count = 0;
    for i in 0..2 {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        let start = std::time::Instant::now();
        let _ = Command::new("mdfind")
            .arg("kMDItemFSName == 'Applications'")
            .output();
        let elapsed = start.elapsed();
        if elapsed > std::time::Duration::from_secs(3) {
            slow_count += 1;
        }
    }

    if slow_count < 2 {
        return PreCheckResult::Skip(
            "Spotlight index is responsive, rebuild not needed".into(),
        );
    }

    PreCheckResult::Proceed
}

/// Skip network flush if route and DNS lookups both work fine.
fn pre_check_network_flush() -> PreCheckResult {
    let route_ok = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let dns_ok = Command::new("dscacheutil")
        .args(["-q", "host", "-a", "name", "example.com"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if route_ok && dns_ok {
        PreCheckResult::Skip(
            "Network stack is healthy, flush not needed".into(),
        )
    } else {
        PreCheckResult::Proceed
    }
}

/// Skip periodic maintenance if scripts ran within the last 7 days.
fn pre_check_periodic() -> PreCheckResult {
    use std::path::Path;
    let daily_log = Path::new("/var/log/daily.out");
    if let Ok(meta) = fs::metadata(daily_log) {
        if let Ok(modified) = meta.modified() {
            let age = std::time::SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default();
            if age < std::time::Duration::from_secs(7 * 24 * 60 * 60) {
                return PreCheckResult::Skip(
                    "Periodic scripts ran within the last 7 days".into(),
                );
            }
        }
    }
    PreCheckResult::Proceed
}

/// Skip disk permission repair if permissions are already correct.
fn pre_check_disk_permissions() -> PreCheckResult {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return PreCheckResult::Proceed,
    };

    // Check if home dir owner matches current user
    let uid_output = Command::new("id").arg("-u").output();
    let home_stat = Command::new("stat").args(["-f", "%u", &home.to_string_lossy()]).output();

    if let (Ok(uid_o), Ok(stat_o)) = (uid_output, home_stat) {
        let uid = String::from_utf8_lossy(&uid_o.stdout).trim().to_string();
        let home_uid = String::from_utf8_lossy(&stat_o.stdout).trim().to_string();

        if uid == home_uid {
            // Check key directories are writable
            let test_dirs = ["Desktop", "Documents", "Downloads"];
            let all_writable = test_dirs.iter().all(|d| {
                let p = home.join(d);
                !p.exists() || std::fs::metadata(&p).map(|m| !m.permissions().readonly()).unwrap_or(true)
            });

            if all_writable {
                return PreCheckResult::Skip(
                    "Disk permissions are already correct".into(),
                );
            }
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

/// Validate that a path is safe for optimizer file operations.
/// Rejects empty paths, traversal components, system roots, and anything
/// outside the user's ~/Library directory.
fn is_safe_optimizer_path(path: &std::path::Path) -> bool {
    let path_str = path.to_string_lossy();
    if path_str.is_empty() { return false; }
    if path_str.contains("..") { return false; }

    // Must be under user's Library directory
    if let Some(home) = dirs::home_dir() {
        let library = home.join("Library");
        if !path.starts_with(&library) {
            return false;
        }
    } else {
        return false;
    }

    // Block system roots
    const BLOCKED: &[&str] = &["/System", "/usr", "/bin", "/sbin", "/etc", "/var"];
    for b in BLOCKED {
        if path_str.starts_with(b) { return false; }
    }

    true
}

/// Verify a file has a valid SQLite header before operating on it.
fn is_sqlite_file(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut header = [0u8; 16];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    header.starts_with(b"SQLite format 3")
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

    // Build a targeted list of known databases instead of recursive search
    let mut db_candidates: Vec<PathBuf> = Vec::new();

    // Mail: glob ~/Library/Mail/V*/MailData/Envelope Index
    let mail_dir = home.join("Library/Mail");
    if mail_dir.exists() {
        if let Ok(entries) = fs::read_dir(&mail_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with('V') {
                            let envelope = path.join("MailData/Envelope Index");
                            if envelope.exists() {
                                db_candidates.push(envelope);
                            }
                        }
                    }
                }
            }
        }
    }

    // Messages and Safari: check specific paths directly
    let specific_dbs = [
        home.join("Library/Messages/chat.db"),
        home.join("Library/Safari/History.db"),
        home.join("Library/Safari/TopSites.db"),
    ];
    for db in &specific_dbs {
        if db.exists() {
            db_candidates.push(db.clone());
        }
    }

    let mut vacuumed: usize = 0;
    let mut skipped: usize = 0;
    let mut errors: Vec<String> = Vec::new();

    for db_path in &db_candidates {
        let db_str = db_path.to_string_lossy().to_string();

        // Verify the file has a valid SQLite header
        if !is_sqlite_file(db_path) {
            skipped += 1;
            continue;
        }

        // Skip databases over 100MB
        if let Ok(meta) = fs::metadata(db_path) {
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

        // Skip if database is already compact (freelist < 5% of pages)
        let freelist_check = Command::new("sqlite3")
            .arg(&db_str)
            .arg("SELECT freelist_count, page_count FROM pragma_freelist_count(), pragma_page_count();")
            .output();

        if let Ok(o) = freelist_check {
            let text = String::from_utf8_lossy(&o.stdout);
            let parts: Vec<&str> = text.trim().split('|').collect();
            if parts.len() == 2 {
                if let (Ok(freelist), Ok(pages)) = (
                    parts[0].trim().parse::<u64>(),
                    parts[1].trim().parse::<u64>(),
                ) {
                    if pages > 0 && (freelist * 100 / pages) < 5 {
                        skipped += 1;
                        continue;
                    }
                }
            }
        }

        // VACUUM with a 20-second timeout using spawn + try_wait polling
        let vacuum_cmd = "PRAGMA busy_timeout = 20000; VACUUM;";

        let child = Command::new("sqlite3")
            .arg(&db_str)
            .arg(vacuum_cmd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        match child {
            Ok(mut proc) => {
                let deadline = std::time::Instant::now()
                    + std::time::Duration::from_secs(20);
                let status = loop {
                    match proc.try_wait() {
                        Ok(Some(s)) => break Some(s),
                        Ok(None) => {
                            if std::time::Instant::now() >= deadline {
                                let _ = proc.kill();
                                let _ = proc.wait();
                                break None;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                        Err(_) => break None,
                    }
                };

                match status {
                    Some(s) if s.success() => {
                        vacuumed += 1;
                    }
                    Some(_) => {
                        let stderr_bytes = proc.stderr.and_then(|mut e| {
                            use std::io::Read;
                            let mut buf = Vec::new();
                            e.read_to_end(&mut buf).ok().map(|_| buf)
                        }).unwrap_or_default();
                        let stderr = String::from_utf8_lossy(&stderr_bytes);
                        errors.push(format!("{}: {}", db_str, stderr.trim()));
                    }
                    None => {
                        errors.push(format!("{}: timed out after 20s", db_str));
                    }
                }
            }
            Err(e) => {
                errors.push(format!("{}: {}", db_str, e));
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

/// Custom plist repair runner.
/// Scan a directory for corrupted .plist files and remove them.
fn repair_plists_in_dir(dir: &std::path::Path, checked: &mut usize, repaired: &mut usize) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

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
        if filename.starts_with("com.apple.") || filename.starts_with(".GlobalPreferences") || filename == "loginwindow.plist" {
            continue;
        }

        *checked += 1;

        let result = Command::new("plutil")
            .arg("-lint")
            .arg(path.to_string_lossy().as_ref())
            .output();

        if let Ok(output) = result {
            if !output.status.success() {
                if !is_safe_optimizer_path(&path) { continue; }
                if fs::remove_file(&path).is_ok() {
                    *repaired += 1;
                }
            }
        }
    }
}

fn run_plist_repair() -> (bool, String) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let prefs_dir = home.join("Library/Preferences");
    let byhost_dir = prefs_dir.join("ByHost");

    let mut checked: usize = 0;
    let mut repaired: usize = 0;

    if prefs_dir.exists() {
        repair_plists_in_dir(&prefs_dir, &mut checked, &mut repaired);
    }

    if byhost_dir.exists() {
        repair_plists_in_dir(&byhost_dir, &mut checked, &mut repaired);
    }

    let msg = format!(
        "Checked {} preference files, removed {} corrupted",
        checked, repaired
    );
    (true, msg)
}

/// Custom saved-state cleaner: only removes *.savedState dirs older than 30 days.
fn run_saved_state_cleanup() -> (bool, String) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let saved_state_dir = home.join("Library/Saved Application State");
    if !saved_state_dir.exists() {
        return (true, "No Saved Application State directory found".into());
    }

    let entries = match fs::read_dir(&saved_state_dir) {
        Ok(e) => e,
        Err(e) => return (false, format!("Cannot read directory: {}", e)),
    };

    let thirty_days = std::time::Duration::from_secs(30 * 24 * 60 * 60);
    let now = std::time::SystemTime::now();
    let mut removed: usize = 0;
    let mut skipped: usize = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.ends_with(".savedState") {
            continue;
        }

        let age_ok = fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|mtime| {
                now.duration_since(mtime)
                    .unwrap_or_default()
                    > thirty_days
            })
            .unwrap_or(false);

        if !age_ok {
            skipped += 1;
            continue;
        }

        if !is_safe_optimizer_path(&path) { continue; }
        if fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }

    let msg = format!(
        "Removed {} old saved states, kept {} recent (< 30 days)",
        removed, skipped
    );
    (true, msg)
}

/// Clean the quarantine events database.
/// Checks row count first and skips if zero entries.
fn run_quarantine_cleanup() -> (bool, String) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let db_path = home.join("Library/Preferences/com.apple.LaunchServices.QuarantineEventsV2");
    if !db_path.exists() {
        return (true, "No quarantine database found".into());
    }

    // Pre-check: count rows before attempting delete
    let count_result = Command::new("sqlite3")
        .arg(db_path.to_string_lossy().as_ref())
        .arg("SELECT COUNT(*) FROM LSQuarantineEvent;")
        .output();

    if let Ok(o) = &count_result {
        if o.status.success() {
            let count_str = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if let Ok(count) = count_str.parse::<u64>() {
                if count == 0 {
                    return (true, "Quarantine database already clean".into());
                }
            }
        }
    }

    let result = Command::new("sqlite3")
        .arg(db_path.to_string_lossy().as_ref())
        .arg("DELETE FROM LSQuarantineEvent; VACUUM;")
        .output();

    match result {
        Ok(output) if output.status.success() => {
            // Report the count if we got it
            let count_msg = if let Ok(o) = &count_result {
                let c = String::from_utf8_lossy(&o.stdout).trim().to_string();
                format!(" ({} entries)", c)
            } else {
                String::new()
            };
            (true, format!("Quarantine history cleared{}", count_msg))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            (false, format!("Failed: {}", stderr.trim()))
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Remove LaunchAgent plists whose referenced binaries no longer exist.
fn run_launch_agents_cleanup() -> (bool, String) {
    use std::path::Path;

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let agents_dir = home.join("Library/LaunchAgents");
    if !agents_dir.exists() {
        return (true, "No LaunchAgents directory found".into());
    }

    let entries = match fs::read_dir(&agents_dir) {
        Ok(e) => e,
        Err(e) => return (false, format!("Cannot read LaunchAgents: {}", e)),
    };

    let mut checked: usize = 0;
    let mut removed: usize = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("plist") {
            continue;
        }

        checked += 1;

        // Try ProgramArguments first, then Program key
        let binary_path = get_plist_program(&path);

        if let Some(bin) = binary_path {
            if !bin.is_empty() && !Path::new(&bin).exists() {
                if !is_safe_optimizer_path(&path) { continue; }

                let _ = Command::new("launchctl")
                    .arg("unload")
                    .arg(path.to_string_lossy().as_ref())
                    .output();

                if fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            }
        }
    }

    (true, format!(
        "Checked {} launch agents, removed {} with missing binaries",
        checked, removed
    ))
}

/// Extract the program binary path from a LaunchAgent plist.
fn get_plist_program(plist_path: &std::path::Path) -> Option<String> {
    // Try ProgramArguments first
    let output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg("Print :ProgramArguments:0")
        .arg(plist_path.to_string_lossy().as_ref())
        .output()
        .ok()?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    // Fall back to Program key
    let output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg("Print :Program")
        .arg(plist_path.to_string_lossy().as_ref())
        .output()
        .ok()?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

/// Repair shared file lists (Finder sidebar favorites).
/// Only removes .sfl2/.sfl3 files that fail plutil validation (corrupted).
/// Skips ApplicationRecentDocuments subdirectories (user data, not cache).
/// Does NOT kill Finder — it will pick up the changes on its own.
fn run_shared_file_list_repair() -> (bool, String) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let sfl_dir = home.join("Library/Application Support/com.apple.sharedfilelist");
    if !sfl_dir.exists() {
        return (true, "No shared file lists found".into());
    }

    let mut checked: usize = 0;
    let mut repaired: usize = 0;

    fn scan_sfl_dir(dir: &std::path::Path, checked: &mut usize, repaired: &mut usize) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                // Skip ApplicationRecentDocuments subdirectories (user data)
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.contains("ApplicationRecentDocuments") {
                        continue;
                    }
                }
                scan_sfl_dir(&path, checked, repaired);
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str());
            if ext != Some("sfl2") && ext != Some("sfl3") {
                continue;
            }
            *checked += 1;

            // Only remove if plutil says the file is corrupted
            let lint = Command::new("plutil")
                .arg("-lint")
                .arg(path.to_string_lossy().as_ref())
                .output();

            let is_corrupt = match lint {
                Ok(o) => !o.status.success(),
                Err(_) => false,
            };

            if is_corrupt {
                if !is_safe_optimizer_path(&path) { continue; }
                if fs::remove_file(&path).is_ok() {
                    *repaired += 1;
                }
            }
        }
    }

    scan_sfl_dir(&sfl_dir, &mut checked, &mut repaired);

    (true, format!("Checked {} shared file lists, removed {} corrupted", checked, repaired))
}

/// Clean old notification records (>30 days, only if DB > 50MB).
fn run_notification_cleanup() -> (bool, String) {
    // Modern macOS stores the notification DB in DARWIN_USER_DIR
    let darwin_dir = match Command::new("getconf").arg("DARWIN_USER_DIR").output() {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => return (false, "Could not determine DARWIN_USER_DIR".into()),
    };

    let db_path = PathBuf::from(&darwin_dir)
        .join("com.apple.notificationcenter/db2/db");
    if !db_path.exists() {
        return (true, "No notification database found".into());
    }

    let size = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    if size < 50 * 1024 * 1024 {
        return (true, "Notification database is small, skipped".into());
    }

    let result = Command::new("sqlite3")
        .arg(db_path.to_string_lossy().as_ref())
        .arg("DELETE FROM record WHERE delivered_date < strftime('%s', 'now', '-30 days'); VACUUM;")
        .output();

    match result {
        Ok(o) if o.status.success() => {
            // Restart NotificationCenter so it picks up the cleaned DB
            let _ = Command::new("killall")
                .arg("NotificationCenter")
                .output();
            (true, "Cleaned old notification records".into())
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            (false, format!("Failed: {}", stderr.trim()))
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Clean old CoreDuet/Knowledge records (>90 days, only if combined size > 100MB).
/// Removes WAL/SHM files before vacuuming, checks combined size of all DB files.
fn run_coreduet_cleanup() -> (bool, String) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (false, "Could not determine home directory".into()),
    };

    let db_path = home.join("Library/Application Support/Knowledge/knowledgeC.db");
    if !db_path.exists() {
        return (true, "No Knowledge database found".into());
    }

    let wal_path = home.join("Library/Application Support/Knowledge/knowledgeC.db-wal");
    let shm_path = home.join("Library/Application Support/Knowledge/knowledgeC.db-shm");

    // Check combined size of DB + WAL + SHM
    let mut total_size: u64 = 0;
    for p in [&db_path, &wal_path, &shm_path] {
        if let Ok(meta) = fs::metadata(p) {
            total_size += meta.len();
        }
    }

    if total_size < 100 * 1024 * 1024 {
        return (true, format!(
            "Knowledge database is {}MB (under 100MB threshold), skipped",
            total_size / (1024 * 1024)
        ));
    }

    // Remove WAL and SHM files first (auto-regenerated by SQLite),
    // but only if knowledged is not running — deleting these while the
    // process holds the DB open in WAL mode can corrupt the database.
    if !is_process_running("knowledged") {
        for p in [&wal_path, &shm_path] {
            if p.exists() {
                let _ = fs::remove_file(p);
            }
        }
    }

    let result = Command::new("sqlite3")
        .arg(db_path.to_string_lossy().as_ref())
        .arg("DELETE FROM ZOBJECT WHERE ZCREATIONDATE < (strftime('%s', 'now') - strftime('%s', '2001-01-01') - 7776000); VACUUM;")
        .output();

    match result {
        Ok(output) if output.status.success() => {
            (true, format!(
                "Trimmed Knowledge database entries older than 90 days (was {}MB)",
                total_size / (1024 * 1024)
            ))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            (false, format!("Failed: {}", stderr.trim()))
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Rebuild LaunchServices with 3-domain fallback to 2-domain.
fn run_launch_services_rebuild() -> (bool, String) {
    let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

    if !std::path::Path::new(lsregister).exists() {
        return (false, "lsregister not found".into());
    }

    // Garbage-collect first
    let _ = Command::new(lsregister).arg("-gc").output();

    // Try 3-domain: local + user + system
    let result = Command::new(lsregister)
        .args(["-r", "-f", "-domain", "local", "-domain", "user", "-domain", "system"])
        .output();

    let success = match result {
        Ok(o) if o.status.success() => true,
        _ => {
            // Fallback to 2-domain: local + user
            match Command::new(lsregister)
                .args(["-r", "-f", "-domain", "local", "-domain", "user"])
                .output()
            {
                Ok(o) if o.status.success() => true,
                _ => false,
            }
        }
    };

    if success {
        (true, "LaunchServices database rebuilt, file associations refreshed".into())
    } else {
        (false, "Failed to rebuild LaunchServices".into())
    }
}

/// Audit login items and report broken ones.
fn run_login_items_audit() -> (bool, String) {
    use std::path::Path;

    let result = Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to get the name of every login item")
        .output();

    let names = match result {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                return (true, "No login items found".into());
            }
            text
        }
        _ => return (true, "Could not query login items".into()),
    };

    let path_result = Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to get the path of every login item")
        .output();

    let items: Vec<&str> = names.split(", ").collect();

    let mut broken: Vec<String> = Vec::new();
    if let Ok(po) = path_result {
        if po.status.success() {
            let paths_text = String::from_utf8_lossy(&po.stdout).trim().to_string();
            let paths: Vec<&str> = paths_text.split(", ").collect();
            for (i, path_str) in paths.iter().enumerate() {
                let p = path_str.trim();
                if !p.is_empty() && !Path::new(p).exists() {
                    let name = items.get(i).unwrap_or(&"Unknown");
                    broken.push(name.to_string());
                }
            }
        }
    }

    if broken.is_empty() {
        (true, format!("All {} login items are valid", items.len()))
    } else {
        (true, format!(
            "Found {} broken login item(s): {}",
            broken.len(),
            broken.join(", ")
        ))
    }
}

/// Check if a task has a custom runner, and if so, execute it.
/// Returns Some((success, message)) for custom tasks, None for standard tasks.
/// Custom QuickLook cache refresh: reset generators, clear cache command,
/// and delete the thumbnailcache directory.
fn run_cache_refresh() -> (bool, String) {
    // Reset QuickLook cache
    let _ = Command::new("sh")
        .arg("-c")
        .arg("qlmanage -r cache 2>/dev/null")
        .output();

    // Reset all QuickLook generators
    let _ = Command::new("sh")
        .arg("-c")
        .arg("qlmanage -r 2>/dev/null")
        .output();

    // Delete the thumbnailcache directory
    if let Some(home) = dirs::home_dir() {
        let thumb_cache = home.join("Library/Caches/com.apple.QuickLook.thumbnailcache");
        if thumb_cache.exists() {
            let _ = fs::remove_dir_all(&thumb_cache);
        }
    }

    (true, "QuickLook generators reset and thumbnail cache cleared".into())
}

/// Custom Bluetooth reset with SIGKILL escalation.
/// Sends SIGTERM first, waits 1 second, checks if still running, then
/// escalates to SIGKILL if needed.
fn run_bluetooth_reset() -> (bool, String) {
    // SIGTERM via pkill
    let _ = Command::new("sh")
        .arg("-c")
        .arg("pkill bluetoothd 2>/dev/null")
        .output();

    // Wait 1 second for graceful shutdown
    std::thread::sleep(std::time::Duration::from_secs(1));

    // Check if still running
    let still_running = Command::new("pgrep")
        .arg("bluetoothd")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if still_running {
        // Escalate to SIGKILL
        let _ = Command::new("sh")
            .arg("-c")
            .arg("pkill -9 bluetoothd 2>/dev/null")
            .output();
        (true, "Bluetooth daemon killed (SIGKILL escalation)".into())
    } else {
        (true, "Bluetooth daemon restarted".into())
    }
}

fn run_custom_task(task: &OptTask) -> Option<(bool, String)> {
    match task.id.as_str() {
        "cache_refresh" => Some(run_cache_refresh()),
        "bluetooth_reset" => Some(run_bluetooth_reset()),
        "sqlite_vacuum" => Some(run_sqlite_vacuum()),
        "launch_services" => Some(run_launch_services_rebuild()),
        "plist_repair" => Some(run_plist_repair()),
        "saved_state" => Some(run_saved_state_cleanup()),
        "quarantine_cleanup" => Some(run_quarantine_cleanup()),
        "launch_agents_cleanup" => Some(run_launch_agents_cleanup()),
        "shared_file_list_repair" => Some(run_shared_file_list_repair()),
        "notification_cleanup" => Some(run_notification_cleanup()),
        "coreduet_cleanup" => Some(run_coreduet_cleanup()),
        "login_items_audit" => Some(run_login_items_audit()),
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
