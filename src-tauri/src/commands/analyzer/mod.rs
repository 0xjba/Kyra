pub mod scanner;

use serde::Serialize;
use tauri::Emitter;

/// A node in the directory size tree.
#[derive(Clone, Serialize)]
pub struct DirNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<DirNode>,
}

/// Progress event emitted during scan.
#[derive(Clone, Serialize)]
pub struct ScanProgress {
    pub current_path: String,
    pub files_scanned: usize,
    pub total_size: u64,
}

#[tauri::command]
pub async fn analyze_path(app: tauri::AppHandle, path: String, depth: usize) -> Result<DirNode, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scanner::scan_directory(&path, depth, |progress| {
            let _ = app.emit("analyze-progress", progress);
        })
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("Path does not exist".to_string());
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
