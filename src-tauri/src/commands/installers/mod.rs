pub mod remover;
pub mod scanner;

use serde::Serialize;
use tauri::Emitter;

#[derive(Clone, Serialize)]
pub struct InstallerFile {
    pub name: String,
    pub path: String,
    pub extension: String,
    pub size: u64,
    pub modified_secs: u64,
}

#[derive(Clone, Serialize)]
pub struct InstallerProgress {
    pub current_item: String,
    pub items_done: usize,
    pub items_total: usize,
    pub bytes_freed: u64,
}

#[derive(Clone, Serialize)]
pub struct InstallerResult {
    pub items_removed: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn scan_installers() -> Vec<InstallerFile> {
    scanner::scan_for_installers()
}

#[tauri::command]
pub fn delete_installers(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
    dry_run: bool,
) -> InstallerResult {
    remover::remove_installer_files(&file_paths, dry_run, |progress| {
        let _ = app.emit("installer-progress", progress);
    })
}
