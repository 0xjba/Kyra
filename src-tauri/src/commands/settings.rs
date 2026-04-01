use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub dry_run: bool,
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default)]
    pub use_trash: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dry_run: false,
            whitelist: Vec::new(),
            use_trash: false,
        }
    }
}

fn settings_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("com.kyra.app");
    let _ = fs::create_dir_all(&path);
    path.push("settings.json");
    path
}

/// Internal load — callable from other modules without `#[tauri::command]`.
pub fn load_settings_internal() -> Result<AppSettings, String> {
    let path = settings_path();
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).map_err(|e| e.to_string()),
        Err(_) => Ok(AppSettings::default()),
    }
}

/// Internal save — callable from other modules without `#[tauri::command]`.
pub fn save_settings_internal(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path();
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_settings() -> AppSettings {
    load_settings_internal().unwrap_or_default()
}

#[tauri::command]
pub fn save_settings(settings: AppSettings) -> Result<(), String> {
    save_settings_internal(&settings)
}

#[tauri::command]
pub fn add_to_whitelist(path: String) -> Result<(), String> {
    let mut settings = load_settings_internal()?;
    if !settings.whitelist.contains(&path) {
        settings.whitelist.push(path);
        save_settings_internal(&settings)?;
    }
    Ok(())
}

#[tauri::command]
pub fn remove_from_whitelist(path: String) -> Result<(), String> {
    let mut settings = load_settings_internal()?;
    settings.whitelist.retain(|p| p != &path);
    save_settings_internal(&settings)?;
    Ok(())
}

#[tauri::command]
pub fn pick_folder() -> Result<Option<String>, String> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("POSIX path of (choose folder with prompt \"Select folder to scan\")")
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            Ok(None)
        } else {
            Ok(Some(path))
        }
    } else {
        // User cancelled
        Ok(None)
    }
}

// ── Lifetime Stats ──────────────────────────────────────────

fn stats_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("com.kyra.app");
    let _ = fs::create_dir_all(&path);
    path.push("stats.json");
    path
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LifetimeStats {
    #[serde(default)]
    pub total_bytes_freed: u64,
}

/// In-memory cache so we don't read the file on every tick.
static CACHED_BYTES_FREED: AtomicU64 = AtomicU64::new(0);
static STATS_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn ensure_stats_loaded() {
    if !STATS_LOADED.swap(true, Ordering::SeqCst) {
        if let Ok(content) = fs::read_to_string(stats_path()) {
            if let Ok(stats) = serde_json::from_str::<LifetimeStats>(&content) {
                CACHED_BYTES_FREED.store(stats.total_bytes_freed, Ordering::SeqCst);
            }
        }
    }
}

#[tauri::command]
pub fn get_total_bytes_freed() -> u64 {
    ensure_stats_loaded();
    CACHED_BYTES_FREED.load(Ordering::SeqCst)
}

#[tauri::command]
pub fn add_bytes_freed(bytes: u64) -> Result<u64, String> {
    ensure_stats_loaded();
    let new_total = CACHED_BYTES_FREED.fetch_add(bytes, Ordering::SeqCst) + bytes;
    let stats = LifetimeStats { total_bytes_freed: new_total };
    let json = serde_json::to_string_pretty(&stats).map_err(|e| e.to_string())?;
    fs::write(stats_path(), json).map_err(|e| e.to_string())?;
    Ok(new_total)
}
