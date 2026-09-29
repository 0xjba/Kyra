use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn default_large_file_threshold() -> u64 { 100 }
fn default_analyze_scan_depth() -> u32 { 8 }
fn default_true() -> bool { true }
fn default_low_disk_threshold() -> u64 { 10 }
fn default_critical_gb() -> u32 { 3 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PawtrolFrequency {
    #[serde(rename = "6h")]
    SixHours,
    #[default]
    #[serde(rename = "daily")]
    Daily,
    #[serde(rename = "weekly")]
    Weekly,
    #[serde(rename = "low_disk_only")]
    LowDiskOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeAction {
    #[default]
    Auto,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    #[default]
    Notify,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataAction {
    #[default]
    Notify,
    Quiet,
    Ignore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub dry_run: bool,
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default)]
    pub use_trash: bool,
    #[serde(default = "default_large_file_threshold")]
    pub large_file_threshold_mb: u64,
    #[serde(default = "default_analyze_scan_depth")]
    pub analyze_scan_depth: u32,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default = "default_true")]
    pub check_for_updates: bool,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default = "default_low_disk_threshold")]
    pub low_disk_threshold_gb: u64,
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default = "default_true")]
    pub pawtrol_enabled: bool,
    #[serde(default)]
    pub pawtrol_login_prompted: bool,
    #[serde(default)]
    pub pawtrol_frequency: PawtrolFrequency,
    #[serde(default = "default_critical_gb")]
    pub pawtrol_critical_gb: u32,
    #[serde(default)]
    pub pawtrol_safe_action: SafeAction,
    #[serde(default)]
    pub pawtrol_review_action: ReviewAction,
    #[serde(default)]
    pub pawtrol_data_action: DataAction,
    /// Legacy toggle, only read to migrate into `pawtrol_safe_action`.
    #[serde(default, skip_serializing)]
    pub pawtrol_auto_clean: Option<bool>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dry_run: false,
            whitelist: Vec::new(),
            use_trash: false,
            large_file_threshold_mb: default_large_file_threshold(),
            analyze_scan_depth: default_analyze_scan_depth(),
            launch_at_login: false,
            check_for_updates: default_true(),
            notifications_enabled: default_true(),
            low_disk_threshold_gb: default_low_disk_threshold(),
            onboarding_completed: false,
            pawtrol_enabled: default_true(),
            pawtrol_login_prompted: false,
            pawtrol_frequency: PawtrolFrequency::default(),
            pawtrol_critical_gb: default_critical_gb(),
            pawtrol_safe_action: SafeAction::default(),
            pawtrol_review_action: ReviewAction::default(),
            pawtrol_data_action: DataAction::default(),
            pawtrol_auto_clean: None,
        }
    }
}

impl AppSettings {
    /// The critical tier only makes sense below the low-disk tier.
    pub fn normalize(&mut self) {
        let max = self.low_disk_threshold_gb.saturating_sub(1).min(u32::MAX as u64) as u32;
        self.pawtrol_critical_gb = self.pawtrol_critical_gb.min(max);
        self.pawtrol_auto_clean = None;
    }
}

pub(crate) fn parse_settings(json: &str) -> Result<AppSettings, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let has_safe_action = value.get("pawtrol_safe_action").is_some();
    let mut settings: AppSettings = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if !has_safe_action && settings.pawtrol_auto_clean == Some(false) {
        settings.pawtrol_safe_action = SafeAction::Ask;
    }
    settings.normalize();
    Ok(settings)
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
        Ok(content) => parse_settings(&content),
        Err(_) => Ok(AppSettings::default()),
    }
}

/// Internal save — callable from other modules without `#[tauri::command]`.
pub fn save_settings_internal(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path();
    let mut settings = settings.clone();
    settings.normalize();
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_settings() -> AppSettings {
    load_settings_internal().unwrap_or_default()
}

/// Pawtrol's rules are owned by `guardian_set_rules`; the general settings
/// screen saves a whole (possibly stale) copy, so it must never overwrite them.
fn keep_rules_from(disk: &AppSettings, mut incoming: AppSettings) -> AppSettings {
    incoming.pawtrol_enabled = disk.pawtrol_enabled;
    incoming.pawtrol_frequency = disk.pawtrol_frequency.clone();
    incoming.pawtrol_critical_gb = disk.pawtrol_critical_gb;
    incoming.pawtrol_safe_action = disk.pawtrol_safe_action.clone();
    incoming.pawtrol_review_action = disk.pawtrol_review_action.clone();
    incoming.pawtrol_data_action = disk.pawtrol_data_action.clone();
    incoming
}

#[tauri::command]
pub fn save_settings(settings: AppSettings) -> Result<(), String> {
    let disk = load_settings_internal().unwrap_or_default();
    save_settings_internal(&keep_rules_from(&disk, settings))
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

#[tauri::command]
pub fn reset_lifetime_stats() -> Result<(), String> {
    CACHED_BYTES_FREED.store(0, Ordering::SeqCst);
    let stats = LifetimeStats { total_bytes_freed: 0 };
    let json = serde_json::to_string_pretty(&stats).map_err(|e| e.to_string())?;
    fs::write(stats_path(), json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_storage_path() -> String {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("com.kyra.app");
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn general_save_keeps_pawtrol_rules_from_disk() {
        let mut disk = super::AppSettings::default();
        disk.pawtrol_enabled = false;
        disk.pawtrol_frequency = super::PawtrolFrequency::Weekly;
        disk.pawtrol_critical_gb = 5;
        disk.pawtrol_safe_action = super::SafeAction::Ask;
        disk.pawtrol_review_action = super::ReviewAction::Quiet;
        disk.pawtrol_data_action = super::DataAction::Ignore;

        let mut stale = super::AppSettings::default();
        stale.use_trash = true;
        let saved = super::keep_rules_from(&disk, stale);

        assert!(saved.use_trash);
        assert!(!saved.pawtrol_enabled);
        assert_eq!(saved.pawtrol_frequency, super::PawtrolFrequency::Weekly);
        assert_eq!(saved.pawtrol_critical_gb, 5);
        assert_eq!(saved.pawtrol_safe_action, super::SafeAction::Ask);
        assert_eq!(saved.pawtrol_review_action, super::ReviewAction::Quiet);
        assert_eq!(saved.pawtrol_data_action, super::DataAction::Ignore);
    }

    use super::*;

    #[test]
    fn settings_saved_before_pawtrol_load_with_pawtrol_defaults() {
        let s = parse_settings(r#"{"dry_run":false,"launch_at_login":true}"#).unwrap();
        assert!(s.pawtrol_enabled);
        assert!(!s.pawtrol_login_prompted);
        assert!(s.launch_at_login);
        assert_eq!(s.pawtrol_frequency, PawtrolFrequency::Daily);
        assert_eq!(s.pawtrol_critical_gb, 3);
        assert_eq!(s.pawtrol_safe_action, SafeAction::Auto);
        assert_eq!(s.pawtrol_review_action, ReviewAction::Notify);
        assert_eq!(s.pawtrol_data_action, DataAction::Notify);
    }

    #[test]
    fn auto_clean_off_migrates_to_ask() {
        let old = |auto: &str| parse_settings(&format!(r#"{{"dry_run":false,"pawtrol_auto_clean":{}}}"#, auto)).unwrap();
        assert_eq!(old("false").pawtrol_safe_action, SafeAction::Ask);
        assert_eq!(old("true").pawtrol_safe_action, SafeAction::Auto);

        let chosen = parse_settings(r#"{"dry_run":false,"pawtrol_auto_clean":false,"pawtrol_safe_action":"auto"}"#).unwrap();
        assert_eq!(chosen.pawtrol_safe_action, SafeAction::Auto);

        let json = serde_json::to_string(&old("false")).unwrap();
        assert!(!json.contains("pawtrol_auto_clean"));
        assert_eq!(parse_settings(&json).unwrap().pawtrol_safe_action, SafeAction::Ask);
    }

    #[test]
    fn pawtrol_fields_round_trip() {
        let s = AppSettings {
            pawtrol_enabled: false,
            pawtrol_login_prompted: true,
            pawtrol_frequency: PawtrolFrequency::SixHours,
            pawtrol_critical_gb: 2,
            pawtrol_safe_action: SafeAction::Ask,
            pawtrol_review_action: ReviewAction::Quiet,
            pawtrol_data_action: DataAction::Ignore,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""pawtrol_frequency":"6h""#), "{json}");
        assert!(json.contains(r#""pawtrol_data_action":"ignore""#), "{json}");
        let back = parse_settings(&json).unwrap();
        assert!(!back.pawtrol_enabled && back.pawtrol_login_prompted);
        assert_eq!(back.pawtrol_frequency, PawtrolFrequency::SixHours);
        assert_eq!(back.pawtrol_critical_gb, 2);
        assert_eq!(back.pawtrol_safe_action, SafeAction::Ask);
        assert_eq!(back.pawtrol_review_action, ReviewAction::Quiet);
        assert_eq!(back.pawtrol_data_action, DataAction::Ignore);
    }

    #[test]
    fn critical_gb_is_clamped_below_the_low_threshold() {
        let parse = |low: u64, critical: u32| {
            parse_settings(&format!(
                r#"{{"dry_run":false,"low_disk_threshold_gb":{},"pawtrol_critical_gb":{}}}"#,
                low, critical
            ))
            .unwrap()
            .pawtrol_critical_gb
        };
        assert_eq!(parse(10, 3), 3);
        assert_eq!(parse(10, 10), 9);
        assert_eq!(parse(5, 50), 4);
        assert_eq!(parse(1, 3), 0);
        assert_eq!(parse(0, 3), 0);
    }

    #[test]
    fn default_settings_enable_pawtrol() {
        let s = AppSettings::default();
        assert!(s.pawtrol_enabled && !s.pawtrol_login_prompted);
        assert_eq!(s.pawtrol_safe_action, SafeAction::Auto);
        assert!(s.pawtrol_critical_gb < s.low_disk_threshold_gb as u32);
    }
}
