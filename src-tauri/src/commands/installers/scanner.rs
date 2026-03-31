use super::InstallerFile;
use crate::commands::utils::dir_size;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

const INSTALLER_EXTENSIONS: &[&str] = &["dmg", "pkg", "iso", "xip", "mpkg"];

fn is_installer_extension(ext: &str) -> bool {
    INSTALLER_EXTENSIONS.contains(&ext)
}

fn is_app_bundle(name: &str) -> bool {
    name.ends_with(".app")
}

fn scan_directory(dir: &Path, check_app_bundles: bool, max_depth: usize) -> Vec<InstallerFile> {
    scan_directory_recursive(dir, check_app_bundles, max_depth, 0)
}

fn scan_directory_recursive(
    dir: &Path,
    check_app_bundles: bool,
    max_depth: usize,
    current_depth: usize,
) -> Vec<InstallerFile> {
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

        if is_installer || is_app {
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
        } else if path.is_dir() && current_depth < max_depth {
            results.extend(scan_directory_recursive(
                &path,
                check_app_bundles,
                max_depth,
                current_depth + 1,
            ));
        }
    }

    results
}

pub fn scan_for_installers() -> Vec<InstallerFile> {
    let mut all = Vec::new();

    if let Some(home) = dirs::home_dir() {
        // Original locations
        let downloads = home.join("Downloads");
        if downloads.exists() {
            all.extend(scan_directory(&downloads, true, 0));
        }

        let desktop = home.join("Desktop");
        if desktop.exists() {
            all.extend(scan_directory(&desktop, false, 0));
        }

        // New locations — installers only (no .app bundles), top-level
        let documents = home.join("Documents");
        if documents.exists() {
            all.extend(scan_directory(&documents, false, 2));
        }

        let public = home.join("Public");
        if public.exists() {
            all.extend(scan_directory(&public, false, 0));
        }

        // Homebrew cached downloads
        let homebrew_cache = home.join("Library/Caches/Homebrew/downloads");
        if homebrew_cache.exists() {
            all.extend(scan_directory(&homebrew_cache, false, 0));
        }

        // Mail attachment downloads
        let mail_downloads = home.join("Library/Mail Downloads");
        if mail_downloads.exists() {
            all.extend(scan_directory(&mail_downloads, false, 1));
        }
    }

    // Shared location for multi-user installs
    let users_shared = Path::new("/Users/Shared");
    if users_shared.exists() {
        all.extend(scan_directory(users_shared, false, 0));
    }

    let tmp = Path::new("/tmp");
    if tmp.exists() {
        all.extend(scan_directory(tmp, false, 0));
    }

    all.sort_by(|a, b| b.size.cmp(&a.size));
    all.dedup_by(|a, b| a.path == b.path);
    all
}
