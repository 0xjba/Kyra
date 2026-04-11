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
    "/usr/bin",
    "/usr/sbin",
    "/etc",
    "/var/db",
    "/Applications",
    "/Library/Frameworks",
];

/// Returns true if a path is safe to delete (not in the protected list).
pub fn is_safe_path(path: &str) -> bool {
    for protected in PROTECTED_PATHS {
        if path == *protected || path.starts_with(&format!("{}/", protected)) {
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

#[tauri::command]
pub async fn run_brew_cleanup() -> Result<String, String> {
    use std::path::Path;
    use std::process::Command;

    let brew_path = if Path::new("/opt/homebrew/bin/brew").exists() {
        "/opt/homebrew/bin/brew"
    } else if Path::new("/usr/local/bin/brew").exists() {
        "/usr/local/bin/brew"
    } else {
        return Err("Homebrew not installed".into());
    };

    let mut results = Vec::new();

    match Command::new(brew_path)
        .args(["cleanup", "--prune=all"])
        .output()
    {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            results.push(format!(
                "brew cleanup: {}",
                if output.status.success() {
                    "done"
                } else {
                    stderr.trim()
                }
            ));
        }
        Err(e) => results.push(format!("brew cleanup failed: {}", e)),
    }

    match Command::new(brew_path).arg("autoremove").output() {
        Ok(output) => {
            results.push(format!(
                "brew autoremove: {}",
                if output.status.success() {
                    "done"
                } else {
                    "failed"
                }
            ));
        }
        Err(e) => results.push(format!("brew autoremove failed: {}", e)),
    }

    Ok(results.join("\n"))
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
            &["chrome_cache"],
        ),
        ("Firefox", &["firefox", "Firefox"], &["firefox_cache"]),
        (
            "Microsoft Edge",
            &["Microsoft Edge", "Microsoft Edge Helper"],
            &["edge_cache"],
        ),
        (
            "Brave Browser",
            &["Brave Browser", "Brave Browser Helper"],
            &["brave_cache"],
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
