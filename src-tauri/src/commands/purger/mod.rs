pub mod remover;
pub mod scanner;

use serde::Serialize;
use tauri::Emitter;

#[derive(Clone, Serialize)]
pub struct ArtifactEntry {
    pub project_name: String,
    pub project_path: String,
    pub artifact_type: String,
    pub artifact_path: String,
    pub size: u64,
}

#[derive(Clone, Serialize)]
pub struct ScanProgress {
    pub current_path: String,
    pub artifacts_found: usize,
}

#[derive(Clone, Serialize)]
pub struct PurgeProgress {
    pub current_item: String,
    pub items_done: usize,
    pub items_total: usize,
    pub bytes_freed: u64,
}

#[derive(Clone, Serialize)]
pub struct PurgeResult {
    pub items_removed: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn scan_artifacts(app: tauri::AppHandle, root_path: String) -> Vec<ArtifactEntry> {
    scanner::scan_for_artifacts(&root_path, |progress| {
        let _ = app.emit("purge-scan-progress", progress);
    })
}

#[tauri::command]
pub async fn execute_purge(
    app: tauri::AppHandle,
    artifact_paths: Vec<String>,
    dry_run: bool,
) -> PurgeResult {
    remover::remove_artifacts(&artifact_paths, dry_run, |progress| {
        let _ = app.emit("purge-progress", progress);
    })
}

