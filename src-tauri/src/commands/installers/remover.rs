use super::{InstallerProgress, InstallerResult};
use crate::commands::shared;
use crate::commands::utils::{dir_size, is_same_or_under};
use std::fs;
use std::path::Path;
use std::process::Command;

const PROTECTED_PATHS: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/etc",
    "/var/db",
    "/Library/Frameworks",
    "/Applications",
];

const VALID_EXTENSIONS: &[&str] = &["dmg", "pkg", "iso", "xip", "mpkg", "app", "zip"];

fn is_dmg_mounted(path: &Path) -> bool {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    };
    let path_str = canonical.to_string_lossy();

    let output = match Command::new("hdiutil")
        .args(["info", "-plist"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };

    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.contains(path_str.as_ref())
}

fn is_safe_path(path: &Path, home: Option<&Path>) -> bool {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let path_str = canonical.to_string_lossy();

    if PROTECTED_PATHS.iter().any(|p| is_same_or_under(&path_str, p)) {
        return false;
    }

    let in_allowed_home_dir = home
        .map(|h| {
            canonical.starts_with(h.join("Downloads"))
                || canonical.starts_with(h.join("Desktop"))
                || canonical.starts_with(h.join("Documents"))
                || canonical.starts_with(h.join("Public"))
                || canonical.starts_with(h.join("Library/Caches/Homebrew/downloads"))
                || canonical.starts_with(h.join("Library/Mail Downloads"))
                || canonical.starts_with(h.join("Library/Downloads"))
                || canonical.starts_with(h.join("Library/Containers/com.apple.mail/Data/Library/Mail Downloads"))
                || canonical.starts_with(h.join("Downloads/Telegram Desktop"))
        })
        .unwrap_or(false);
    let in_tmp = canonical.starts_with("/tmp") || canonical.starts_with("/private/tmp");
    let in_users_shared =
        canonical.starts_with("/Users/Shared") || canonical.starts_with("/private/Users/Shared");

    if !in_allowed_home_dir && !in_tmp && !in_users_shared {
        return false;
    }

    // Same test the scanner applies: never a file the user made, an
    // encrypted vault, the only copy of an app, or protected data.
    if !super::scanner::is_offerable(&canonical, home, &super::scanner::app_dirs(home)) {
        return false;
    }

    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    VALID_EXTENSIONS.iter().any(|ext| {
        if *ext == "app" {
            name.ends_with(".app")
        } else {
            name.to_lowercase().ends_with(&format!(".{}", ext))
        }
    })
}

pub fn remove_installer_files(
    file_paths: &[String],
    dry_run: bool,
    permanent: bool,
    on_progress: impl Fn(InstallerProgress),
) -> InstallerResult {
    let home = dirs::home_dir();
    remove_installer_files_for_home(file_paths, dry_run, permanent, home.as_deref(), on_progress)
}

fn remove_installer_files_for_home(
    file_paths: &[String],
    dry_run: bool,
    permanent: bool,
    home: Option<&Path>,
    on_progress: impl Fn(InstallerProgress),
) -> InstallerResult {
    let total = file_paths.len();
    let mut items_removed: usize = 0;
    let mut bytes_freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut deleted_paths: Vec<String> = Vec::new();

    for (i, path_str) in file_paths.iter().enumerate() {
        let path = Path::new(path_str);

        on_progress(InstallerProgress {
            current_item: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path_str.clone()),
            items_done: i,
            items_total: total,
            bytes_freed,
        });

        if !is_safe_path(path, home) {
            errors.push(format!("Blocked: {}", path_str));
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) == Some("dmg") && is_dmg_mounted(path) {
            errors.push(format!(
                "Cannot delete: disk image is currently mounted: {}",
                path_str
            ));
            continue;
        }

        let size = if path.is_dir() {
            dir_size(path)
        } else {
            fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        };

        if dry_run {
            bytes_freed += size;
            items_removed += 1;
            deleted_paths.push(path_str.clone());
            continue;
        }

        let delete_result = if permanent {
            if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
        } else {
            trash::delete(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        };
        match delete_result {
            Ok(()) => {
                bytes_freed += size;
                items_removed += 1;
                deleted_paths.push(path_str.clone());
                let action = if permanent { "DELETED" } else { "TRASHED" };
                shared::log_operation("DELETE_INSTALLER", path_str, action);
            }
            Err(e) => {
                shared::log_operation("DELETE_INSTALLER", path_str, &format!("ERROR: {}", e));
                errors.push(format!("{}: {}", path_str, e));
            }
        }
    }

    on_progress(InstallerProgress {
        current_item: String::new(),
        items_done: total,
        items_total: total,
        bytes_freed,
    });

    InstallerResult {
        items_removed,
        bytes_freed,
        errors,
        deleted_paths,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{mkdir, s, set_quarantine, write_file};
    use std::os::unix::fs::symlink;

    fn downloaded(path: &Path, len: usize) {
        write_file(path, len);
        set_quarantine(path);
    }

    fn app_bundle(app: &Path, bundle_id: &str) {
        mkdir(&app.join("Contents"));
        let mut dict = plist::Dictionary::new();
        dict.insert("CFBundleIdentifier".into(), plist::Value::String(bundle_id.into()));
        plist::Value::Dictionary(dict).to_file_xml(app.join("Contents/Info.plist")).unwrap();
    }

    #[test]
    fn portable_browsers_keeping_their_profile_inside_are_refused() {
        let fh = fake_home();
        let app = fh.home.join("Downloads/Portable Browser.app");
        write_file(&app.join("Contents/Info.plist"), 10);
        write_file(&app.join("Profile/Local State"), 10);
        write_file(&app.join("Profile/Default/Login Data"), 10);
        let result = remove_installer_files_for_home(&[s(&app)], false, true, Some(&fh.home), |_| {});
        assert_eq!(result.items_removed, 0);
        assert!(app.join("Profile/Default/Login Data").exists());
    }

    struct FakeHome {
        _dir: tempfile::TempDir,
        home: std::path::PathBuf,
    }

    fn fake_home() -> FakeHome {
        let dir = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(dir.path()).unwrap().join("home");
        mkdir(&home.join("Downloads"));
        FakeHome { _dir: dir, home }
    }

    fn remove(fh: &FakeHome, paths: &[String], dry_run: bool) -> InstallerResult {
        remove_installer_files_for_home(paths, dry_run, true, Some(&fh.home), |_| {})
    }

    #[test]
    fn installers_in_allowed_folders_are_removed() {
        let fh = fake_home();
        let pkg = fh.home.join("Downloads/Tool.pkg");
        let iso = fh.home.join("Desktop/ubuntu.ISO");
        let zip = fh.home.join("Documents/sub/App.zip");
        let app = fh.home.join("Downloads/Thing.app");
        downloaded(&pkg, 1_000);
        downloaded(&iso, 2_000);
        downloaded(&zip, 3_000);
        app_bundle(&app, "com.example.thing");
        set_quarantine(&app);
        app_bundle(&fh.home.join("Applications/Thing.app"), "com.example.thing");

        let paths = vec![s(&pkg), s(&iso), s(&zip), s(&app)];
        let result = remove(&fh, &paths, false);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.items_removed, 4);
        assert!(result.bytes_freed >= 6_000);
        assert_eq!(result.deleted_paths, paths);
        for p in [&pkg, &iso, &zip, &app] {
            assert!(!p.exists(), "{}", p.display());
        }
    }

    #[test]
    fn installer_scan_on_a_fake_data_home_offers_only_downloaded_installers() {
        use crate::commands::data_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let firefox = fx.path("Downloads/Firefox.dmg");
        downloaded(&firefox, 4_000);
        let thing = fx.path("Downloads/Thing.app");
        app_bundle(&thing, "com.example.thing");
        set_quarantine(&thing);
        app_bundle(&fx.path("Applications/Thing.app"), "com.example.thing");
        let brew = fx.path(&format!("Library/Caches/Homebrew/downloads/{}--wget.pkg", "a".repeat(64)));
        write_file(&brew, 4_000);

        let mut offered: Vec<String> = crate::commands::installers::scanner::scan_for_installers_in(Some(&fx.home), false)
            .into_iter()
            .map(|f| f.path)
            .collect();
        offered.sort();
        let mut want = vec![s(&firefox), s(&thing), s(&brew)];
        want.sort();
        assert_eq!(offered, want);

        let mut hostile = offered.clone();
        for rel in fixtures::USER_FILES.iter().chain(fixtures::PROTECTED) {
            hostile.push(s(&fx.path(rel)));
        }
        hostile.push(s(&fx.path("Downloads/Vault.dmg")));
        hostile.push(s(&fx.path("Downloads/OnlyCopy.app")));
        let result = remove_installer_files_for_home(&hostile, false, true, Some(&fx.home), |_| {});
        assert_eq!(result.items_removed, 3, "{:?}", result.errors);
        fx.assert_intact();
        assert!(!firefox.exists() && !thing.exists() && !brew.exists());
    }

    #[test]
    fn dry_run_deletes_nothing() {
        let fh = fake_home();
        let pkg = fh.home.join("Downloads/Tool.pkg");
        downloaded(&pkg, 1_000);
        let result = remove(&fh, &[s(&pkg)], true);
        assert_eq!(result.items_removed, 1);
        assert_eq!(result.bytes_freed, 1_000);
        assert!(pkg.exists());
    }

    #[test]
    fn non_installer_files_are_blocked() {
        let fh = fake_home();
        let doc = fh.home.join("Downloads/taxes.pdf");
        let folder = fh.home.join("Downloads/Photos");
        write_file(&doc, 10);
        write_file(&folder.join("a.jpg"), 10);

        let result = remove(&fh, &[s(&doc), s(&folder), s(&fh.home.join("Downloads"))], false);
        assert_eq!(result.items_removed, 0);
        assert_eq!(result.errors.len(), 3);
        assert!(doc.exists());
        assert!(folder.join("a.jpg").exists());
    }

    #[test]
    fn installers_outside_allowed_folders_are_blocked() {
        let fh = fake_home();
        let in_library = fh.home.join("Library/Application Support/Foo/setup.pkg");
        let in_home_root = fh.home.join("setup.pkg");
        write_file(&in_library, 10);
        write_file(&in_home_root, 10);

        let result = remove(&fh, &[s(&in_library), s(&in_home_root)], false);
        assert_eq!(result.items_removed, 0);
        assert!(in_library.exists());
        assert!(in_home_root.exists());

        let result = remove_installer_files_for_home(&[s(&in_home_root)], false, true, None, |_| {});
        assert_eq!(result.items_removed, 0);
        assert!(in_home_root.exists());
    }

    #[test]
    fn symlink_escaping_the_allowed_folders_is_blocked() {
        let fh = fake_home();
        let secret = fh.home.join("Library/Keychains/login.pkg");
        write_file(&secret, 10);
        let link = fh.home.join("Downloads/innocent.pkg");
        symlink(&secret, &link).unwrap();

        let result = remove(&fh, &[s(&link)], false);
        assert_eq!(result.items_removed, 0);
        assert!(secret.exists());
    }

    #[test]
    fn traversal_and_missing_paths_are_blocked() {
        let fh = fake_home();
        let outside = fh.home.join("Library/x.pkg");
        write_file(&outside, 10);
        let traversal = format!("{}/Downloads/../Library/x.pkg", s(&fh.home));
        let missing = s(&fh.home.join("Downloads/gone.dmg"));

        let result = remove(&fh, &[traversal, missing], false);
        assert_eq!(result.items_removed, 0);
        assert_eq!(result.errors.len(), 2);
        assert!(outside.exists());
    }

    #[test]
    fn system_paths_are_blocked() {
        for p in ["/Applications/Safari.app", "/System/Applications/Mail.app", "/usr/bin/zip", "/"] {
            assert!(!is_safe_path(Path::new(p), dirs::home_dir().as_deref()), "{p}");
        }
    }

    #[test]
    fn progress_ends_at_total() {
        let fh = fake_home();
        let pkg = fh.home.join("Downloads/a.pkg");
        downloaded(&pkg, 10);
        let events = std::cell::RefCell::new(Vec::new());
        remove_installer_files_for_home(&[s(&pkg), "/nope.pkg".into()], true, true, Some(&fh.home), |p| {
            events.borrow_mut().push((p.items_done, p.items_total))
        });
        assert_eq!(events.into_inner(), vec![(0, 2), (1, 2), (2, 2)]);
    }
}
