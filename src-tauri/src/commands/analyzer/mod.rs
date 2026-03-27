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
pub fn analyze_path(app: tauri::AppHandle, path: String, depth: usize) -> Result<DirNode, String> {
    let result = scanner::scan_directory(&path, depth, |progress| {
        let _ = app.emit("analyze-progress", progress);
    });
    Ok(result)
}

#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
