use super::InstallerFile;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

const INSTALLER_EXTENSIONS: &[&str] = &["dmg", "pkg", "iso", "xip"];

fn is_installer_extension(ext: &str) -> bool {
    INSTALLER_EXTENSIONS.contains(&ext)
}

fn is_app_bundle(name: &str) -> bool {
    name.ends_with(".app")
}

fn scan_directory(dir: &Path, check_app_bundles: bool) -> Vec<InstallerFile> {
    let mut results = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return results,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        if name.starts_with('.') {
            continue;
        }

        let is_installer = if let Some(ext) = path.extension() {
            is_installer_extension(&ext.to_string_lossy().to_lowercase())
        } else {
            false
        };

        let is_app = check_app_bundles && path.is_dir() && is_app_bundle(&name);

        if !is_installer && !is_app {
            continue;
        }

        let extension = if is_app {
            "app".to_string()
        } else {
            path.extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };

        let size = if path.is_dir() {
            dir_size(&path)
        } else {
            fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        };

        let modified_secs = fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
            .unwrap_or(0);

        results.push(InstallerFile {
            name,
            path: path.to_string_lossy().to_string(),
            extension,
            size,
            modified_secs,
        });
    }

    results
}

fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_symlink() {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    total += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    total
}

pub fn scan_for_installers() -> Vec<InstallerFile> {
    let mut all = Vec::new();

    if let Some(home) = dirs::home_dir() {
        let downloads = home.join("Downloads");
        if downloads.exists() {
            all.extend(scan_directory(&downloads, true));
        }

        let desktop = home.join("Desktop");
        if desktop.exists() {
            all.extend(scan_directory(&desktop, false));
        }
    }

    let tmp = Path::new("/tmp");
    if tmp.exists() {
        all.extend(scan_directory(tmp, false));
    }

    all.sort_by(|a, b| b.size.cmp(&a.size));
    all.dedup_by(|a, b| a.path == b.path);
    all
}
