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
    pub deleted_paths: Vec<String>,
}

#[tauri::command]
pub async fn scan_installers() -> Vec<InstallerFile> {
    scanner::scan_for_installers()
}

#[tauri::command]
pub async fn delete_installers(
    app: tauri::AppHandle,
    file_paths: Vec<String>,
    dry_run: bool,
    permanent: bool,
) -> InstallerResult {
    tokio::task::spawn_blocking(move || {
        remover::remove_installer_files(&file_paths, dry_run, permanent, |progress| {
            let _ = app.emit("installer-progress", progress);
        })
    })
    .await
    .unwrap_or_else(|e| InstallerResult {
        items_removed: 0,
        bytes_freed: 0,
        errors: vec![format!("Task panicked: {}", e)],
        deleted_paths: Vec::new(),
    })
}
