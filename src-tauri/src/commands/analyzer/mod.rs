pub mod scanner;

use serde::Serialize;
use std::path::Path;
use tauri::Emitter;

/// A node in the directory size tree.
#[derive(Clone, Serialize)]
pub struct DirNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_cleanable: bool,
    pub children: Vec<DirNode>,
}

/// Progress event emitted during scan.
#[derive(Clone, Serialize)]
pub struct ScanProgress {
    pub current_path: String,
    pub files_scanned: usize,
    pub total_size: u64,
}

/// A large file found by Spotlight search.
#[derive(Clone, Serialize)]
pub struct LargeFile {
    pub name: String,
    pub path: String,
    pub size: u64,
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

#[tauri::command]
pub async fn delete_analyzed_item(path: String, permanent: bool) -> Result<u64, String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err("Path does not exist".into());
    }

    // Safety: don't delete system paths
    let protected = ["/System", "/bin", "/sbin", "/usr", "/etc", "/var", "/Applications", "/Library"];
    for prot in &protected {
        if path == *prot || path.starts_with(&format!("{}/", prot)) {
            return Err(format!("Cannot delete protected path: {}", path));
        }
    }

    // Don't delete home directory
    if let Some(home) = dirs::home_dir() {
        if path == home.to_string_lossy() {
            return Err("Cannot delete home directory".into());
        }
    }

    let size = if p.is_dir() {
        crate::commands::utils::dir_size(p)
    } else {
        p.metadata().map(|m| m.len()).unwrap_or(0)
    };

    if permanent {
        if p.is_dir() {
            std::fs::remove_dir_all(p).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_file(p).map_err(|e| e.to_string())?;
        }
    } else {
        trash::delete(p).map_err(|e| e.to_string())?;
    }

    crate::commands::shared::log_operation(
        "ANALYZE_DELETE",
        &path,
        if permanent { "DELETED" } else { "TRASHED" },
    );

    Ok(size)
}

#[tauri::command]
pub async fn find_large_files(min_size_mb: u64) -> Vec<LargeFile> {
    use std::process::Command;

    let min_bytes = min_size_mb * 1024 * 1024;
    let home = dirs::home_dir().unwrap_or_default();
    let query = format!("kMDItemFSSize >= {}", min_bytes);

    let output = Command::new("mdfind")
        .args(["-onlyin", &home.to_string_lossy(), &query])
        .output();

    let mut files: Vec<LargeFile> = Vec::new();

    if let Ok(o) = output {
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines().take(100) {
            let path = Path::new(line);
            if !path.exists() || path.is_dir() {
                continue;
            }
            // Skip source code files
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let skip_exts = [
                    "go", "js", "ts", "py", "rs", "swift", "java", "c", "cpp", "h",
                    "rb", "php", "sql", "lock",
                ];
                if skip_exts.contains(&ext) {
                    continue;
                }
            }
            if let Ok(meta) = path.metadata() {
                files.push(LargeFile {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path: line.to_string(),
                    size: meta.len(),
                });
            }
        }
    }

    files.sort_by(|a, b| b.size.cmp(&a.size));
    files.truncate(20);
    files
}
