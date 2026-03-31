use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
