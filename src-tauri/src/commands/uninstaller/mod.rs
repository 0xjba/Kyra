pub mod associated;
pub mod discovery;
pub mod remover;

use serde::Serialize;

/// Basic info about an installed application.
#[derive(Clone, Serialize)]
pub struct AppInfo {
    pub bundle_id: String,
    pub name: String,
    pub version: String,
    pub path: String,
    pub size: u64,
    pub is_system: bool,
    pub is_data_sensitive: bool,
}

/// A file or directory associated with an application.
#[derive(Clone, Serialize)]
pub struct AssociatedFile {
    pub path: String,
    pub category: String,
    pub size: u64,
    pub is_dir: bool,
}

/// Progress event emitted during uninstallation.
#[derive(Clone, Serialize)]
pub struct UninstallProgress {
    pub current_item: String,
    pub items_done: usize,
    pub items_total: usize,
    pub bytes_freed: u64,
}

/// Final result of an uninstall operation.
#[derive(Clone, Serialize)]
pub struct UninstallResult {
    pub items_removed: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

use tauri::Emitter;

#[tauri::command]
pub async fn scan_installed_apps() -> Vec<AppInfo> {
    discovery::scan_apps()
}

#[tauri::command]
pub fn get_associated_files(
    bundle_id: String,
    app_name: String,
    app_path: String,
) -> Vec<AssociatedFile> {
    associated::find_associated(&bundle_id, &app_name, &app_path)
}

#[tauri::command]
pub async fn execute_uninstall(
    app: tauri::AppHandle,
    app_path: String,
    file_paths: Vec<String>,
    dry_run: bool,
    permanent: bool,
) -> Result<UninstallResult, String> {
    let result = remover::remove_app_and_files(&app_path, &file_paths, dry_run, permanent, |progress| {
        let _ = app.emit("uninstall-progress", progress);
    });

    Ok(result)
}
