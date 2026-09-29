use super::InstallerFile;
use crate::commands::utils::dir_size;
use std::fs;
use std::path::{Path, PathBuf};
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

/// True if `path` carries the quarantine mark browsers, Mail and
/// AirDrop put on downloaded files. Disk images and archives the user made
/// themselves never have it.
pub(crate) fn is_quarantined(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let len = unsafe {
        libc::getxattr(
            c_path.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            libc::XATTR_NOFOLLOW,
        )
    };
    len >= 0
}

/// Encrypted disk images are private vaults, never installers.
fn is_encrypted_dmg(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 8];
    if f.read_exact(&mut head).is_ok() && &head == b"encrcdsa" {
        return true;
    }
    let mut tail = [0u8; 8];
    f.seek(SeekFrom::End(-8)).is_ok() && f.read_exact(&mut tail).is_ok() && &tail == b"cdsaencr"
}

fn bundle_id_of(app: &Path) -> Option<String> {
    let plist = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    plist
        .as_dictionary()?
        .get("CFBundleIdentifier")?
        .as_string()
        .map(|s| s.to_lowercase())
}

/// Folders where installed apps live.
pub(crate) fn app_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(h) = home {
        dirs.push(h.join("Applications"));
    }
    dirs
}

/// True if an app with the same bundle id as `app` is installed at another
/// path, so the downloaded copy is redundant.
fn installed_elsewhere(app: &Path, app_dirs: &[PathBuf]) -> bool {
    let Some(id) = bundle_id_of(app) else {
        return false;
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in app_dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "app") {
                candidates.push(p);
            } else if p.is_dir() {
                if let Ok(inner) = fs::read_dir(&p) {
                    candidates.extend(inner.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "app")));
                }
            }
        }
    }
    let same = |a: &Path, b: &Path| fs::canonicalize(a).ok() == fs::canonicalize(b).ok();
    candidates.iter().any(|c| !same(c, app) && bundle_id_of(c).as_deref() == Some(id.as_str()))
}

/// Caches of downloads, whose files are installers by nature.
fn in_download_cache(path: &Path, home: &Path) -> bool {
    [
        "Library/Caches/Homebrew/downloads",
        "Library/Mail Downloads",
        "Library/Containers/com.apple.mail/Data/Library/Mail Downloads",
    ]
    .iter()
    .any(|rel| path.starts_with(home.join(rel)))
}

/// Whether a found installer may be offered (and deleted): it was
/// downloaded rather than made by the user, is not an encrypted vault, is
/// not the only copy of an app, and holds no protected data.
pub(crate) fn is_offerable(path: &Path, home: Option<&Path>, app_dirs: &[PathBuf]) -> bool {
    if crate::commands::data_guard::check_general(path).is_err() {
        return false;
    }
    let is_app = path.extension().is_some_and(|e| e == "app") && path.is_dir();
    if is_app {
        return is_macos_installer(path) || (is_quarantined(path) && installed_elsewhere(path, app_dirs));
    }
    let is_dmg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dmg"));
    if is_dmg && is_encrypted_dmg(path) {
        return false;
    }
    home.is_some_and(|h| in_download_cache(path, h)) || is_quarantined(path)
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
    let home = dirs::home_dir();
    scan_for_installers_in(home.as_deref(), true)
}

/// `scan_for_installers` against an explicit home; `include_system` adds
/// /Users/Shared and /tmp.
pub(crate) fn scan_for_installers_in(home: Option<&Path>, include_system: bool) -> Vec<InstallerFile> {
    let mut all = Vec::new();

    if let Some(home) = home {
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

        // Library Downloads (software updates, etc.)
        let library_downloads = home.join("Library/Downloads");
        if library_downloads.exists() {
            all.extend(scan_directory(&library_downloads, false, 0));
        }

        let telegram_downloads = home.join("Downloads/Telegram Desktop");
        if telegram_downloads.exists() {
            all.extend(scan_directory(&telegram_downloads, false, 1));
        }
    }

    // Shared location for multi-user installs
    let users_shared = Path::new("/Users/Shared");
    if include_system && users_shared.exists() {
        all.extend(scan_directory(users_shared, false, 2));
    }

    let tmp = Path::new("/tmp");
    if include_system && tmp.exists() {
        all.extend(scan_directory(tmp, false, 0));
    }

    let mut seen = std::collections::HashSet::new();
    all.retain(|item| seen.insert(item.path.clone()));
    let app_dirs = app_dirs(home);
    all.retain(|item| is_offerable(Path::new(&item.path), home, &app_dirs));
    all.sort_by(|a, b| b.size.cmp(&a.size));
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{mkdir, write_file};
    use std::io::Write;
    use std::os::unix::fs::symlink;

    fn make_zip(path: &Path, entries: &[&str]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for e in entries {
            if e.ends_with('/') {
                zip.add_directory(e.trim_end_matches('/'), opts).unwrap();
            } else {
                zip.start_file(*e, opts).unwrap();
                zip.write_all(b"data").unwrap();
            }
        }
        zip.finish().unwrap();
    }

    fn names(results: &[InstallerFile]) -> Vec<String> {
        let mut v: Vec<String> = results.iter().map(|f| f.name.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn installer_extensions_are_detected_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for n in ["a.dmg", "b.pkg", "c.iso", "d.xip", "e.mpkg", "F.DMG"] {
            write_file(&r.join(n), 100);
        }
        for n in ["notes.txt", "photo.jpg", "dmg", "archive.tar.gz"] {
            write_file(&r.join(n), 100);
        }
        let results = scan_directory(r, false, 0);
        assert_eq!(names(&results), vec!["F.DMG", "a.dmg", "b.pkg", "c.iso", "d.xip", "e.mpkg"]);
        let upper = results.iter().find(|f| f.name == "F.DMG").unwrap();
        assert_eq!(upper.extension, "dmg");
        assert_eq!(upper.size, 100);
        assert!(upper.modified_secs > 0);
    }

    #[test]
    fn zips_count_only_when_they_contain_installers() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        make_zip(&r.join("app.zip"), &["Foo.app/", "Foo.app/Contents/Info.plist"]);
        make_zip(&r.join("nested.zip"), &["Foo.app/Contents/MacOS/foo"]);
        make_zip(&r.join("pkg.zip"), &["setup/Installer.PKG"]);
        make_zip(&r.join("photos.zip"), &["img/1.jpg", "img/2.jpg"]);
        write_file(&r.join("corrupt.zip"), 64);

        let results = scan_directory(r, false, 0);
        assert_eq!(names(&results), vec!["app.zip", "nested.zip", "pkg.zip"]);
        assert!(results.iter().all(|f| f.extension == "zip"));
        assert!(!is_installer_zip(&r.join("photos.zip")));
        assert!(!is_installer_zip(&r.join("corrupt.zip")));
        assert!(!is_installer_zip(&r.join("missing.zip")));
    }

    #[test]
    fn app_bundles_only_count_where_requested() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("Tool.app/Contents/Info.plist"), 50);
        write_file(&r.join("Tool.app/Contents/Resources/inner.dmg"), 50);

        let with_apps = scan_directory(r, true, 2);
        assert_eq!(names(&with_apps), vec!["Tool.app"]);
        assert_eq!(with_apps[0].extension, "app");

        let without_apps = scan_directory(r, false, 5);
        assert_eq!(names(&without_apps), vec!["inner.dmg"]);
    }

    #[test]
    fn depth_hidden_entries_and_symlinks_are_respected() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("top.pkg"), 1);
        write_file(&r.join("l1/one.pkg"), 1);
        write_file(&r.join("l1/l2/two.pkg"), 1);
        write_file(&r.join(".hidden.pkg"), 1);
        write_file(&r.join(".cache/in-hidden.pkg"), 1);
        let elsewhere = tempfile::tempdir().unwrap();
        write_file(&elsewhere.path().join("linked.pkg"), 1);
        symlink(elsewhere.path().join("linked.pkg"), r.join("link.pkg")).unwrap();
        symlink(elsewhere.path(), r.join("linkdir")).unwrap();

        assert_eq!(names(&scan_directory(r, false, 0)), vec!["top.pkg"]);
        assert_eq!(names(&scan_directory(r, false, 1)), vec!["one.pkg", "top.pkg"]);
        assert_eq!(
            names(&scan_directory(r, false, 2)),
            vec!["one.pkg", "top.pkg", "two.pkg"]
        );
    }

    #[test]
    fn brew_hash_prefix_is_stripped_only_when_valid() {
        let hash = "a".repeat(64);
        assert_eq!(strip_brew_hash_prefix(&format!("{hash}--Firefox.dmg")), "Firefox.dmg");
        let not_hex = format!("{}--x.dmg", "z".repeat(64));
        assert_eq!(strip_brew_hash_prefix(&not_hex), not_hex);
        assert_eq!(strip_brew_hash_prefix("short--x.dmg"), "short--x.dmg");
        assert_eq!(strip_brew_hash_prefix(&format!("{hash}__x.dmg")), format!("{hash}__x.dmg"));
    }

    #[test]
    fn macos_installer_detection_and_version() {
        assert!(is_macos_installer(Path::new("/Applications/Install macOS Sonoma.app")));
        assert!(!is_macos_installer(Path::new("/Applications/Sonoma.app")));
        assert!(!is_macos_installer(Path::new("/x/Install macOS Sonoma.dmg")));
        assert!(!is_protected_macos_installer(Path::new("/x/Firefox.app"), 0));

        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Install macOS Test.app");
        mkdir(&app.join("Contents"));
        let mut dict = plist::Dictionary::new();
        dict.insert("DTPlatformVersion".into(), plist::Value::String("14.2".into()));
        plist::Value::Dictionary(dict).to_file_xml(app.join("Contents/Info.plist")).unwrap();
        assert_eq!(read_installer_major_version(&app), Some(14));

        let mut dict = plist::Dictionary::new();
        dict.insert("CFBundleShortVersionString".into(), plist::Value::String("15.0.1".into()));
        plist::Value::Dictionary(dict).to_file_xml(app.join("Contents/Info.plist")).unwrap();
        assert_eq!(read_installer_major_version(&app), Some(15));

        assert_eq!(read_installer_major_version(&dir.path().join("missing.app")), None);
    }
}
