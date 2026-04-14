pub mod executor;
pub mod rules;
pub mod scanner;

use serde::{Deserialize, Serialize};

/// A cleaning rule definition — pure data describing what to scan.
#[derive(Clone, Serialize)]
pub struct CleanRule {
    pub id: String,
    pub category: String,
    pub label: String,
    pub paths: Vec<String>,
    pub max_age_days: Option<u32>,
}

/// A single found path with its size.
#[derive(Clone, Serialize, Deserialize)]
pub struct PathInfo {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
}

/// Result of scanning a single rule — what was found and how big it is.
#[derive(Clone, Serialize, Deserialize)]
pub struct ScanItem {
    pub rule_id: String,
    pub category: String,
    pub label: String,
    pub paths: Vec<PathInfo>,
    pub total_size: u64,
}

/// Progress event emitted during cleaning.
#[derive(Clone, Serialize)]
pub struct CleanProgress {
    pub current_item: String,
    pub items_done: usize,
    pub items_total: usize,
    pub bytes_freed: u64,
}

/// Final result of a clean operation.
#[derive(Clone, Serialize)]
pub struct CleanResult {
    pub items_cleaned: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
    pub cleaned_ids: Vec<String>,
}

/// Paths that must never be deleted.
const PROTECTED_PATHS: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr",
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/etc",
    "/var/db",
    "/private",
    "/Applications",
    "/Library/Frameworks",
    "/Library/Extensions",
];

/// Returns true if a path is safe to delete (not in the protected list).
///
/// Performs textual validation, blocks directory traversal (`..`) and control
/// characters, and additionally resolves symlinks so that a user-writable path
/// pointing into a protected system location is rejected.
pub fn is_safe_path(path: &str) -> bool {
    let canonical = match crate::commands::utils::canonicalize_for_safety(path) {
        Some(p) => p,
        None => return false,
    };
    let canonical_str = canonical.to_string_lossy();

    for protected in PROTECTED_PATHS {
        let prefix = format!("{}/", protected);
        if path == *protected || path.starts_with(&prefix) {
            return false;
        }
        if canonical_str == *protected || canonical_str.starts_with(&prefix) {
            return false;
        }
    }
    true
}

#[derive(Clone, Serialize)]
pub struct RunningApp {
    pub name: String,
    pub rule_ids: Vec<String>,
}

use tauri::Emitter;

#[tauri::command]
pub async fn scan_for_cleanables() -> Vec<ScanItem> {
    let rules = rules::all_rules();
    let mut results = scanner::scan_rules(&rules);
    results.extend(scanner::scan_orphaned_data());
    results
}

#[tauri::command]
pub async fn execute_clean(
    app: tauri::AppHandle,
    items: Vec<ScanItem>,
    dry_run: bool,
    permanent: bool,
) -> Result<CleanResult, String> {
    if items.is_empty() {
        return Ok(CleanResult {
            items_cleaned: 0,
            bytes_freed: 0,
            errors: vec![],
            cleaned_ids: vec![],
        });
    }

    let result = tokio::task::spawn_blocking(move || {
        executor::execute_clean_items(&items, dry_run, permanent, |progress| {
            let _ = app.emit("clean-progress", progress);
        })
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?;

    Ok(result)
}

/// Returns true if a CLI tool is available at the PATH.
fn tool_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run a native tool with a timeout and return whether it succeeded.
fn run_tool_with_timeout(tool: &str, args: &[&str], timeout_secs: u64) -> bool {
    let child = std::process::Command::new(tool)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => return false,
    };
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(_) => return false,
        }
    }
}

/// Run native package manager cache cleanup commands if the tools are installed.
fn run_native_cache_cleanup() -> Vec<String> {
    let mut results = Vec::new();

    // npm
    if tool_exists("npm") {
        if run_tool_with_timeout("npm", &["cache", "clean", "--force"], 30) {
            results.push("npm cache cleaned".into());
        }
    }

    // pnpm — detect Corepack shim (non-functional stub) before running
    if tool_exists("pnpm") {
        let pnpm_usable = std::process::Command::new("pnpm")
            .arg("--version")
            .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if pnpm_usable {
            if run_tool_with_timeout("pnpm", &["store", "prune"], 30) {
                results.push("pnpm cache cleaned".into());
            }
        }
    }

    // yarn
    if tool_exists("yarn") {
        if run_tool_with_timeout("yarn", &["cache", "clean"], 30) {
            results.push("yarn cache cleaned".into());
        }
    }

    // pip3 — detect macOS CLT stub that triggers an install dialog
    if tool_exists("pip3") {
        let pip3_usable = std::process::Command::new("pip3")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if pip3_usable {
            if run_tool_with_timeout("pip3", &["cache", "purge"], 30) {
                results.push("pip3 cache cleaned".into());
            }
        }
    }

    // go
    if tool_exists("go") {
        if run_tool_with_timeout("go", &["clean", "-cache"], 30) {
            results.push("go cache cleaned".into());
        }
    }

    // bun
    if tool_exists("bun") {
        if run_tool_with_timeout("bun", &["pm", "cache", "rm"], 30) {
            results.push("bun cache cleaned".into());
        }
    }

    // mise
    if tool_exists("mise") {
        if run_tool_with_timeout("mise", &["cache", "clear"], 30) {
            results.push("mise cache cleaned".into());
        }
    }

    // nix
    if tool_exists("nix-collect-garbage") {
        if run_tool_with_timeout("nix-collect-garbage", &["--delete-older-than", "30d"], 30) {
            results.push("nix garbage collected".into());
        }
    }

    results
}

#[tauri::command]
pub async fn run_brew_cleanup() -> Result<String, String> {
    use std::path::Path;

    let brew_path = if Path::new("/opt/homebrew/bin/brew").exists() {
        "/opt/homebrew/bin/brew"
    } else if Path::new("/usr/local/bin/brew").exists() {
        "/usr/local/bin/brew"
    } else {
        return Err("Homebrew not installed".into());
    };

    let mut results = Vec::new();

    // ── 7-day skip: avoid repeated heavy operations ──────────────
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".cache/kyra");
    let stamp_file = cache_dir.join("brew_last_cleanup");
    if stamp_file.exists() {
        if let Ok(meta) = stamp_file.metadata() {
            if let Ok(modified) = meta.modified() {
                let age = std::time::SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                let days = age.as_secs() / 86400;
                if days < 7 {
                    results.push(format!("brew: cleaned {}d ago, skipped", days));
                    results.extend(run_native_cache_cleanup());
                    return Ok(results.join("\n"));
                }
            }
        }
    }

    // ── Small-cache skip: skip cleanup if cache < 50 MB ──────────
    let brew_cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join("Library/Caches/Homebrew");
    let skip_cleanup = if brew_cache_dir.is_dir() {
        let size = crate::commands::utils::dir_size(&brew_cache_dir);
        size < 50 * 1024 * 1024 // 50 MB
    } else {
        true
    };

    // ── Parallel execution with 120s timeout ─────────────────────
    let brew_cleanup = brew_path.to_string();
    let brew_autoremove = brew_path.to_string();
    let timeout = std::time::Duration::from_secs(120);

    let cleanup_handle = if !skip_cleanup {
        let path = brew_cleanup.clone();
        Some(std::thread::spawn(move || {
            run_command_with_timeout(&path, &["cleanup", "--prune=30"], timeout)
        }))
    } else {
        None
    };

    let autoremove_handle = {
        let path = brew_autoremove.clone();
        std::thread::spawn(move || {
            run_command_with_timeout(&path, &["autoremove"], timeout)
        })
    };

    // Collect results
    if skip_cleanup {
        results.push("brew cleanup: skipped (cache small)".into());
    } else if let Some(handle) = cleanup_handle {
        match handle.join() {
            Ok(Ok(true)) => results.push("brew cleanup: done".into()),
            Ok(Ok(false)) => results.push("brew cleanup: timed out".into()),
            Ok(Err(e)) => results.push(format!("brew cleanup failed: {}", e)),
            Err(_) => results.push("brew cleanup: thread error".into()),
        }
    }

    match autoremove_handle.join() {
        Ok(Ok(true)) => results.push("brew autoremove: done".into()),
        Ok(Ok(false)) => results.push("brew autoremove: timed out".into()),
        Ok(Err(e)) => results.push(format!("brew autoremove failed: {}", e)),
        Err(_) => results.push("brew autoremove: thread error".into()),
    }

    // Update timestamp on success
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::write(&stamp_file, "");

    // Also clean native package manager caches
    results.extend(run_native_cache_cleanup());

    Ok(results.join("\n"))
}

/// Run a command with a timeout, returning Ok(true) on success, Ok(false) on
/// timeout, Err on spawn failure.
fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<bool, String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[tauri::command]
pub fn check_running_processes(rule_ids: Vec<String>) -> Vec<RunningApp> {
    use sysinfo::System;

    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    // (display_name, exact_process_names, guarded_rule_ids)
    //
    // Process names are matched exactly against the executable name reported
    // by sysinfo. Substring matching is intentionally avoided because it
    // produces false positives (e.g. "Safari" matching "SafariBookmarksSyncAgent",
    // "Mail" matching "MailCompositionService") and false negatives when the
    // executable name differs from the display name (e.g. Docker's backend
    // process is "com.docker.backend", not "Docker").
    let app_rules: Vec<(&str, &[&str], &[&str])> = vec![
        ("Safari", &["Safari"], &["safari_cache"]),
        (
            "Google Chrome",
            &["Google Chrome", "Google Chrome Helper"],
            &["chrome_cache", "browser_chrome_old_framework"],
        ),
        ("Firefox", &["firefox", "Firefox"], &["firefox_cache"]),
        (
            "Microsoft Edge",
            &["Microsoft Edge", "Microsoft Edge Helper"],
            &["edge_cache", "browser_edge_old_framework", "browser_edge_updater_old_versions"],
        ),
        (
            "Brave Browser",
            &["Brave Browser", "Brave Browser Helper"],
            &["brave_cache", "browser_brave_old_framework"],
        ),
        ("Arc", &["Arc"], &["arc_cache"]),
        ("Opera", &["Opera"], &["opera_cache"]),
        ("Vivaldi", &["Vivaldi"], &["vivaldi_cache"]),
        ("Discord", &["Discord", "Discord Helper"], &["comm_discord"]),
        ("Slack", &["Slack", "Slack Helper"], &["comm_slack"]),
        (
            "Spotify",
            &["Spotify", "Spotify Helper"],
            &["media_spotify"],
        ),
        ("Zoom", &["zoom.us", "CptHost"], &["comm_zoom"]),
        (
            "Microsoft Teams",
            &["Microsoft Teams", "MSTeams", "Teams"],
            &["comm_teams"],
        ),
        (
            "Visual Studio Code",
            &["Code", "Code Helper"],
            &["dev_vscode_cache"],
        ),
        ("Telegram", &["Telegram"], &["comm_telegram"]),
        ("WhatsApp", &["WhatsApp"], &["comm_whatsapp"]),
        ("WeChat", &["WeChat"], &["comm_wechat"]),
        ("Skype", &["Skype"], &["comm_skype"]),
        ("Signal", &["Signal"], &["comm_signal"]),
        ("Figma", &["Figma"], &["design_figma"]),
        ("Sketch", &["Sketch"], &["design_sketch"]),
        ("Steam", &["Steam", "steam_osx"], &["game_steam"]),
        ("OBS", &["OBS", "obs"], &["media_obs"]),
        ("IINA", &["IINA"], &["media_iina"]),
        ("VLC", &["VLC"], &["media_vlc"]),
        ("Notion", &["Notion", "Notion Helper"], &["notes_notion"]),
        ("Obsidian", &["Obsidian"], &["notes_obsidian"]),
        ("Mail", &["Mail"], &["system_mail_downloads"]),
        (
            "Docker",
            &[
                "Docker Desktop",
                "Docker",
                "com.docker.backend",
                "com.docker.build",
            ],
            &["dev_docker_buildx"],
        ),
    ];

    let process_names: Vec<String> = sys
        .processes()
        .values()
        .map(|p| p.name().to_string_lossy().to_string())
        .collect();

    let mut running: Vec<RunningApp> = Vec::new();

    for (app_name, proc_names, rules) in &app_rules {
        let matching_rules: Vec<String> = rules
            .iter()
            .filter(|r| rule_ids.contains(&r.to_string()))
            .map(|r| r.to_string())
            .collect();

        if matching_rules.is_empty() {
            continue;
        }

        let is_running = process_names
            .iter()
            .any(|pn| proc_names.iter().any(|target| pn == target));

        if is_running {
            running.push(RunningApp {
                name: app_name.to_string(),
                rule_ids: matching_rules,
            });
        }
    }

    running
}
