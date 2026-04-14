use super::InstallerFile;
use crate::commands::utils::dir_size;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const INSTALLER_EXTENSIONS: &[&str] = &["dmg", "pkg", "iso", "xip", "mpkg"];

/// Strip Homebrew hash prefix from filenames.
/// Homebrew cached downloads look like `<64 hex chars>--actual-name`.
/// This returns the part after `--` if the prefix matches.
fn strip_brew_hash_prefix(name: &str) -> String {
    if name.len() > 66 && &name[64..66] == "--" {
        let prefix = &name[..64];
        if prefix.chars().all(|c| c.is_ascii_hexdigit()) {
            return name[66..].to_string();
        }
    }
    name.to_string()
}

/// Minimum age (in days) before a macOS installer is eligible to surface
/// in scan results. Recent installers may still be in use by the user
/// (upgrade in progress, freshly downloaded) and accidental deletion is
/// expensive.
const MACOS_INSTALLER_MIN_AGE_DAYS: u64 = 14;

/// Return the current macOS major version number as reported by
/// `sw_vers -productVersion`. Returns `None` if the command fails or the
/// output can't be parsed. Used to decide whether a given
/// "Install macOS X.app" bundle matches the currently running OS and
/// should therefore be preserved for recovery use.
fn current_macos_major() -> Option<u32> {
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout);
    version
        .trim()
        .split('.')
        .next()?
        .parse::<u32>()
        .ok()
}

/// Read `DTPlatformVersion` (or `CFBundleShortVersionString`) from the
/// installer's Info.plist and return its major version number. Returns
/// `None` if the bundle isn't a macOS installer or we can't parse the
/// plist.
fn read_installer_major_version(app_path: &Path) -> Option<u32> {
    let plist_path = app_path.join("Contents/Info.plist");
    let plist = plist::Value::from_file(&plist_path).ok()?;
    let dict = plist.as_dictionary()?;
    let version_str = dict
        .get("DTPlatformVersion")
        .and_then(|v| v.as_string())
        .or_else(|| {
            dict.get("CFBundleShortVersionString")
                .and_then(|v| v.as_string())
        })?;
    version_str.split('.').next()?.parse::<u32>().ok()
}

/// Returns true if the given path is a "Install macOS X.app" bundle.
fn is_macos_installer(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    name.starts_with("Install macOS") && name.ends_with(".app")
}

/// Returns true if a "Install macOS X.app" bundle is currently running,
/// determined by a targeted `/usr/bin/pgrep -f` lookup. This is a narrow
/// exact-path match: we don't match substrings of other processes.
fn is_installer_running(app_path: &Path) -> bool {
    let exec = app_path
        .join("Contents/MacOS/InstallAssistant_springboard")
        .to_string_lossy()
        .to_string();
    if let Ok(output) = Command::new("/usr/bin/pgrep").arg("-f").arg(&exec).output() {
        if output.status.success() && !output.stdout.is_empty() {
            return true;
        }
    }
    // Fall back to matching against the bundle path itself. pgrep with a
    // bundle path is still narrow enough to avoid false positives.
    if let Ok(output) = Command::new("/usr/bin/pgrep")
        .arg("-f")
        .arg(app_path.to_string_lossy().as_ref())
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return true;
        }
    }
    false
}

/// Returns true if the given "Install macOS X.app" bundle should be
/// treated as *protected* and omitted from the scan results. Three gates
/// are applied, matching the reference behavior:
///
/// 1. **Version match** — if the installer's `DTPlatformVersion` major
///    matches the currently running macOS major, the user may need it
///    for recovery/reinstall and it is never surfaced.
/// 2. **Age** — installers newer than 14 days are kept (recent
///    downloads may still be needed).
/// 3. **Running** — installers whose process is currently active are
///    obviously in use.
fn is_protected_macos_installer(app_path: &Path, modified_secs: u64) -> bool {
    if !is_macos_installer(app_path) {
        return false;
    }

    // Gate 1: matches current macOS major → keep.
    if let (Some(current), Some(installer)) =
        (current_macos_major(), read_installer_major_version(app_path))
    {
        if current == installer {
            return true;
        }
    }

    // Gate 2: age < 14 days → keep.
    if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let age_secs = now.as_secs().saturating_sub(modified_secs);
        if age_secs < MACOS_INSTALLER_MIN_AGE_DAYS * 86_400 {
            return true;
        }
    }

    // Gate 3: currently running → keep.
    if is_installer_running(app_path) {
        return true;
    }

    false
}

/// Check if a ZIP file contains installer files (.app, .pkg, .mpkg).
/// Skips files over 500MB to avoid slow reads on huge archives.
fn is_installer_zip(path: &Path) -> bool {
    // Skip huge ZIPs — reading the central directory of a multi-GB archive is expensive
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > 500 * 1024 * 1024 {
            return false;
        }
    }

    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let reader = std::io::BufReader::new(file);
    let mut archive = match zip::ZipArchive::new(reader) {
        Ok(a) => a,
        Err(_) => return false,
    };

    let check_count = archive.len().min(50);
    for i in 0..check_count {
        if let Ok(entry) = archive.by_index(i) {
            let name = entry.name().to_lowercase();
            if name.ends_with(".app/") || name.ends_with(".pkg") || name.ends_with(".mpkg")
                || name.ends_with(".dmg") || name.ends_with(".xip")
                || name.contains(".app/")
            {
                return true;
            }
        }
    }

    false
}

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

        // Skip symlinks to avoid traversal outside expected directories
        if path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            continue;
        }

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

            // Macos installer safety gate — never surface recovery
            // installers, recent downloads, or running installers.
            if is_app && is_protected_macos_installer(&path, modified_secs) {
                continue;
            }

            results.push(InstallerFile {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                extension,
                size,
                modified_secs,
            });
        }

        // Check ZIP files for embedded installers
        if !is_installer && !is_app {
            if let Some(ext) = path.extension() {
                if ext.to_string_lossy().eq_ignore_ascii_case("zip") && is_installer_zip(&path) {
                    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let modified_secs = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
                        .unwrap_or(0);

                    results.push(InstallerFile {
                        name,
                        path: path.to_string_lossy().to_string(),
                        extension: "zip".to_string(),
                        size,
                        modified_secs,
                    });
                    continue;
                }
            }
        }

        if !is_installer && !is_app && path.is_dir() && current_depth < max_depth {
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
            all.extend(scan_directory(&downloads, true, 2));
        }

        let desktop = home.join("Desktop");
        if desktop.exists() {
            all.extend(scan_directory(&desktop, false, 1));
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
            let mut brew_files = scan_directory(&homebrew_cache, false, 0);
            for f in &mut brew_files {
                f.name = strip_brew_hash_prefix(&f.name);
            }
            all.extend(brew_files);
        }

        // Mail attachment downloads (containerized path for modern macOS)
        let mail_downloads_container = home.join("Library/Containers/com.apple.mail/Data/Library/Mail Downloads");
        if mail_downloads_container.exists() {
            all.extend(scan_directory(&mail_downloads_container, false, 1));
        }
        // Legacy Mail Downloads path
        let mail_downloads_legacy = home.join("Library/Mail Downloads");
        if mail_downloads_legacy.exists() {
            all.extend(scan_directory(&mail_downloads_legacy, false, 1));
        }

        // iCloud Drive Downloads
        let icloud_downloads = home.join("Library/Mobile Documents/com~apple~CloudDocs/Downloads");
        if icloud_downloads.exists() {
            all.extend(scan_directory(&icloud_downloads, false, 1));
        }

        // Library Downloads (software updates, etc.)
        let library_downloads = home.join("Library/Downloads");
        if library_downloads.exists() {
            all.extend(scan_directory(&library_downloads, false, 0));
        }

        // Telegram Desktop cached installers
        let telegram_appdata = home.join("Library/Application Support/Telegram Desktop");
        if telegram_appdata.exists() {
            all.extend(scan_directory(&telegram_appdata, false, 1));
        }

        let telegram_downloads = home.join("Downloads/Telegram Desktop");
        if telegram_downloads.exists() {
            all.extend(scan_directory(&telegram_downloads, false, 1));
        }
    }

    // Shared location for multi-user installs
    let users_shared = Path::new("/Users/Shared");
    if users_shared.exists() {
        all.extend(scan_directory(users_shared, false, 2));
    }

    let tmp = Path::new("/tmp");
    if tmp.exists() {
        all.extend(scan_directory(tmp, false, 0));
    }

    let mut seen = std::collections::HashSet::new();
    all.retain(|item| seen.insert(item.path.clone()));
    all.sort_by(|a, b| b.size.cmp(&a.size));
    all
}
