use super::types::{GuardianCleanProgress, GuardianCleanResult};
use super::{license, probes};
use crate::commands::shared;
use std::future::Future;
use std::path::Path;
use tauri::Emitter;

pub async fn execute_guardian_clean(
    app: &tauri::AppHandle,
    categories: &[String],
    permanent: bool,
) -> Result<GuardianCleanResult, String> {
    // Without a home dir every target would resolve against "/", so refuse outright.
    let home = dirs::home_dir()
        .filter(|h| h.is_absolute() && h != Path::new("/"))
        .ok_or("Could not resolve home directory")?;

    let outcome = clean_if_licensed(
        license::require_license(),
        &home,
        categories,
        permanent,
        |progress| {
            let _ = app.emit("guardian-clean-progress", progress);
        },
    )
    .await?;

    for (path, err) in &outcome.path_errors {
        shared::log_operation("GUARDIAN_CLEAN_ERR", path, err);
    }
    shared::log_operation(
        "GUARDIAN_CLEAN",
        "guardian",
        &format!(
            "cleaned {} categories, freed {} bytes",
            outcome.result.categories_cleaned, outcome.result.bytes_freed
        ),
    );

    Ok(outcome.result)
}

/// Nothing is touched unless `license` resolves Ok; the frontend gate alone is not trusted.
pub(crate) async fn clean_if_licensed(
    license: impl Future<Output = Result<(), String>>,
    home: &Path,
    categories: &[String],
    permanent: bool,
    on_progress: impl FnMut(GuardianCleanProgress),
) -> Result<CleanOutcome, String> {
    license.await?;
    Ok(clean_categories(home, categories, permanent, on_progress))
}

/// Whether the user approved this clean or Pawtrol runs it on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CleanMode {
    Approved,
    Autonomous,
}

pub(crate) struct CleanOutcome {
    pub result: GuardianCleanResult,
    pub path_errors: Vec<(String, String)>,
}

/// Cleans categories the user picked or approved.
pub(crate) fn clean_categories(
    home: &Path,
    categories: &[String],
    permanent: bool,
    on_progress: impl FnMut(GuardianCleanProgress),
) -> CleanOutcome {
    clean_categories_with(home, categories, permanent, CleanMode::Approved, on_progress)
}

pub(crate) fn clean_categories_with(
    home: &Path,
    categories: &[String],
    permanent: bool,
    mode: CleanMode,
    mut on_progress: impl FnMut(GuardianCleanProgress),
) -> CleanOutcome {
    let total = categories.len();
    let mut bytes_freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut path_errors: Vec<(String, String)> = Vec::new();
    let mut cleaned = 0;

    for (i, category) in categories.iter().enumerate() {
        on_progress(GuardianCleanProgress {
            current_category: category.clone(),
            categories_done: i,
            categories_total: total,
            bytes_freed,
        });

        match clean_category(home, category, permanent, mode, &mut path_errors) {
            Ok(freed) => {
                bytes_freed += freed;
                cleaned += 1;
            }
            Err(e) => {
                errors.push(format!("{}: {}", category, e));
            }
        }
    }

    on_progress(GuardianCleanProgress {
        current_category: String::new(),
        categories_done: total,
        categories_total: total,
        bytes_freed,
    });

    CleanOutcome {
        result: GuardianCleanResult {
            categories_cleaned: cleaned,
            bytes_freed,
            errors,
        },
        path_errors,
    }
}

fn clean_category(
    home: &Path,
    category: &str,
    permanent: bool,
    mode: CleanMode,
    path_errors: &mut Vec<(String, String)>,
) -> Result<u64, String> {
    if probes::definition(category).is_none() {
        return Err("unknown category".into());
    }
    if mode == CleanMode::Autonomous && !probes::autonomous_safe(category) {
        return Err("needs your approval".into());
    }
    let mut freed: u64 = 0;
    let permanent = effective_permanent(category, permanent);
    let ctx = probes::guard_context(category, mode == CleanMode::Approved);

    for path in targets(category, home) {
        let Some(size) = probes::deletable_size_in(&path, ctx) else {
            continue;
        };
        let path_str = path.to_string_lossy().to_string();

        let result = if permanent {
            if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            }
        } else {
            trash::delete(&path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        };

        match result {
            Ok(_) => freed += size,
            Err(e) => path_errors.push((path_str, e.to_string())),
        }
    }

    Ok(freed)
}

pub(crate) fn effective_permanent(category: &str, permanent: bool) -> bool {
    permanent || category == probes::TRASH
}

/// The Trash folder itself must survive, so it is emptied entry by entry.
fn targets(category: &str, home: &Path) -> Vec<std::path::PathBuf> {
    let roots = probes::paths_for(category, home);
    if category != probes::TRASH {
        return roots;
    }
    roots
        .iter()
        .filter(|root| probes::deletable_size(root).is_some())
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flat_map(|entries| entries.flatten().map(|e| e.path()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::guardian::license::{cache_license, require_license_with};
    use crate::commands::guardian::test_support::{
        block_on, client, dead_url, serve_once, TestDir,
    };
    use crate::commands::guardian::types::LicenseStatus;

    fn cats(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cleaning_every_category_spares_browser_profiles() {
        use crate::commands::browser_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let all: Vec<String> = probes::CATEGORIES.iter().map(|c| c.id.to_string()).collect();

        clean_categories(&fx.home, &all, true, |_| {});

        fx.assert_profiles_intact();
        assert!(!fx.home.join("Library/Caches/Google/Chrome/Default/Cache").exists());
        for p in fx.protected.keys().chain(fx.protected_dirs.iter()) {
            assert!(probes::deletable_size(p).is_none(), "{}", p.display());
        }
    }

    #[test]
    fn cleaning_never_reaches_protected_data_without_approval() {
        use crate::commands::data_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let all: Vec<String> = probes::CATEGORIES.iter().map(|c| c.id.to_string()).collect();

        let auto = clean_categories_with(&fx.home, &all, true, CleanMode::Autonomous, |_| {});
        fx.assert_intact();
        for d in probes::CATEGORIES.iter().filter(|d| d.data_loss.is_some()) {
            let refused = format!("{}: needs your approval", d.id);
            assert!(auto.result.errors.contains(&refused), "{:?}", auto.result.errors);
        }
        for rel in [
            "Library/Application Support/Slack/Cache",
            "Library/Application Support/discord/Cache",
            "Library/Application Support/discord/Code Cache",
            "Library/Application Support/Microsoft/Teams/Cache",
        ] {
            assert!(!fx.path(rel).exists(), "{rel} should have been cleaned");
        }

        clean_categories(&fx.home, &all, true, |_| {});
        fx.assert_intact_except(&["Library/Containers/com.docker.docker/Data/vms", "Library/Developer/Xcode/Archives"]);
        for rel in fixtures::PROTECTED {
            assert!(probes::deletable_size(&fx.path(rel)).is_none(), "{rel}");
        }
    }

    #[test]
    fn deletes_only_the_selected_categorys_paths() {
        let home = TestDir::new("clean-node");
        let npm = home.write(".npm/_cacache/blob", 8192);
        let yarn = home.write("Library/Caches/Yarn/pkg", 4096);
        let npm_config = home.write(".npm/_logs/keep.log", 10);
        let rust = home.write(".cargo/registry/cache/crate", 4096);
        let docs = home.write("Documents/important.txt", 10);

        let mut events = Vec::new();
        let out = clean_categories(home.path(), &cats(&["node"]), true, |p| events.push(p));

        assert!(!npm.exists() && !home.path().join(".npm/_cacache").exists());
        assert!(!yarn.exists());
        assert!(npm_config.exists(), "sibling of a cache root must survive");
        assert!(rust.exists(), "other categories must survive");
        assert!(docs.exists());
        assert_eq!(out.result.categories_cleaned, 1);
        assert!(out.result.bytes_freed >= 8192 + 4096);
        assert!(out.result.errors.is_empty());
        assert!(out.path_errors.is_empty());

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].current_category, "node");
        assert_eq!(events[0].categories_done, 0);
        assert_eq!(events[1].current_category, "");
        assert_eq!(events[1].categories_done, 1);
        assert_eq!(events[1].bytes_freed, out.result.bytes_freed);
    }

    #[test]
    fn arbitrary_strings_never_delete_anything() {
        let home = TestDir::new("clean-arbitrary");
        let docs = home.write("Documents/important.txt", 10);
        let npm = home.write(".npm/_cacache/blob", 10);
        let docs_str = home.path().join("Documents").to_string_lossy().to_string();

        let out = clean_categories(
            home.path(),
            &cats(&[&docs_str, "../", "", "Documents", "NODE", "node ", "*"]),
            true,
            |_| {},
        );

        assert!(docs.exists());
        assert!(npm.exists());
        assert_eq!(out.result.categories_cleaned, 0);
        assert_eq!(out.result.bytes_freed, 0);
        assert_eq!(out.result.errors.len(), 7);
        assert!(out
            .result
            .errors
            .iter()
            .all(|e| e.ends_with("unknown category")));
    }

    #[test]
    fn missing_paths_are_a_noop() {
        let home = TestDir::new("clean-missing");
        let out = clean_categories(home.path(), &cats(&["xcode", "python"]), true, |_| {});
        assert_eq!(out.result.categories_cleaned, 2);
        assert_eq!(out.result.bytes_freed, 0);
        assert!(out.result.errors.is_empty());
    }

    #[test]
    fn symlinked_cache_root_is_not_followed() {
        let home = TestDir::new("clean-symlink");
        let target = home.write("Documents/keep/data.bin", 4096);
        std::fs::create_dir_all(home.path().join(".cargo/registry")).unwrap();
        std::os::unix::fs::symlink(
            home.path().join("Documents/keep"),
            home.path().join(".cargo/registry/cache"),
        )
        .unwrap();

        let out = clean_categories(home.path(), &cats(&["rust"]), true, |_| {});
        assert!(target.exists());
        assert_eq!(out.result.bytes_freed, 0);
    }

    #[test]
    fn protected_roots_are_refused() {
        // macOS temp dirs canonicalize under /private, which is_safe_path protects.
        let tmp =
            std::env::temp_dir().join(format!("kyra-guardian-protected-{}", std::process::id()));
        let cache = tmp.join(".npm/_cacache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("blob"), b"x").unwrap();

        let canonical = std::fs::canonicalize(&tmp).unwrap();
        let out = clean_categories(&tmp, &cats(&["node"]), true, |_| {});
        let survived = cache.join("blob").exists();
        let _ = std::fs::remove_dir_all(&tmp);

        if canonical.starts_with("/private") {
            assert!(survived);
            assert_eq!(out.result.bytes_freed, 0);
        }
    }

    fn fill_every_category(home: &TestDir) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        for (i, d) in probes::CATEGORIES.iter().enumerate() {
            for (j, root) in probes::paths_for(d.id, home.path()).iter().enumerate() {
                let rel = root.strip_prefix(home.path()).unwrap();
                files.push(home.write(&format!("{}/sub/f", rel.display()), 4096 * (i + j + 1)));
            }
        }
        files
    }

    #[test]
    fn reported_size_equals_bytes_freed_for_every_category() {
        let home = TestDir::new("clean-sizes");
        fill_every_category(&home);
        let keep = home.write("Documents/important.txt", 10);

        for d in probes::CATEGORIES {
            let reported = probes::measure(home.path(), d).unwrap().cleanable_bytes;
            let out = clean_categories(home.path(), &cats(&[d.id]), true, |_| {});
            assert!(
                out.path_errors.is_empty(),
                "{}: {:?}",
                d.id,
                out.path_errors
            );
            assert_eq!(out.result.bytes_freed, reported, "{}", d.id);
            for p in probes::paths_for(d.id, home.path()) {
                if d.id == probes::TRASH {
                    assert!(p.is_dir() && std::fs::read_dir(&p).unwrap().next().is_none());
                } else {
                    assert!(!p.exists(), "{}: {} survived", d.id, p.display());
                }
            }
            assert!(probes::measure(home.path(), d).is_none());
        }
        assert!(keep.exists());
    }

    #[test]
    fn docker_build_cache_clean_leaves_the_vm_alone() {
        let home = TestDir::new("clean-docker");
        let cache = home.write(".docker/buildx/cache/layer", 8192);
        let vm = home.write(
            "Library/Containers/com.docker.docker/Data/vms/0/disk.raw",
            8192,
        );
        let out = clean_categories(home.path(), &cats(&["docker"]), true, |_| {});
        assert!(!cache.exists());
        assert!(vm.exists());
        assert!(out.result.bytes_freed > 0);
    }

    #[test]
    fn trash_is_emptied_in_place_and_always_permanently() {
        let home = TestDir::new("clean-trash");
        let a = home.write(".Trash/old.dmg", 8192);
        let b = home.write(".Trash/folder/f", 4096);
        let logs = home.write("Library/Logs/x.log", 4096);

        let out = clean_categories(home.path(), &cats(&["system"]), true, |_| {});
        assert!(
            a.exists() && b.exists(),
            "system must no longer touch the Trash"
        );
        assert!(!logs.exists());
        assert!(out.result.bytes_freed > 0);

        let out = clean_categories(home.path(), &cats(&[probes::TRASH]), true, |_| {});
        assert!(!a.exists() && !home.path().join(".Trash/folder").exists());
        assert!(home.path().join(".Trash").is_dir());
        assert!(out.result.bytes_freed >= 8192 + 4096);

        assert!(effective_permanent(probes::TRASH, false));
        assert!(!effective_permanent("node", false));
        assert!(effective_permanent("node", true));
    }

    fn assert_refused(
        home: &TestDir,
        files: &[std::path::PathBuf],
        res: Result<CleanOutcome, String>,
        msg: &str,
    ) {
        match res {
            Err(e) => assert_eq!(e, msg),
            Ok(_) => panic!("clean ran without a license"),
        }
        for f in files {
            assert!(f.exists(), "{} was deleted", f.display());
        }
        assert!(home.path().join(".npm/_cacache").exists());
    }

    #[test]
    fn clean_without_license_is_refused_and_deletes_nothing() {
        let home = TestDir::new("clean-nolicense");
        let data = TestDir::new("clean-nolicense-data");
        let files = fill_every_category(&home);
        let all: Vec<String> = probes::CATEGORIES
            .iter()
            .map(|d| d.id.to_string())
            .collect();

        let mut events = 0;
        let res = block_on(clean_if_licensed(
            require_license_with(&client(), &dead_url(), "d", data.path()),
            home.path(),
            &all,
            true,
            |_| events += 1,
        ));
        assert_eq!(events, 0);
        assert_refused(&home, &files, res, "No active license");
    }

    #[test]
    fn expired_cached_license_is_refused_offline() {
        let home = TestDir::new("clean-expired");
        let data = TestDir::new("clean-expired-data");
        let files = fill_every_category(&home);
        cache_license(
            data.path(),
            &LicenseStatus {
                active: true,
                expires: Some(1_000),
            },
        );
        let res = block_on(clean_if_licensed(
            require_license_with(&client(), &dead_url(), "d", data.path()),
            home.path(),
            &cats(&["node", "docker_vm"]),
            true,
            |_| {},
        ));
        assert_refused(&home, &files, res, "License expired");
    }

    #[test]
    fn server_rejection_wins_over_an_active_cache() {
        let home = TestDir::new("clean-revoked");
        let data = TestDir::new("clean-revoked-data");
        let files = fill_every_category(&home);
        cache_license(
            data.path(),
            &LicenseStatus {
                active: true,
                expires: Some(u64::MAX),
            },
        );
        for (body, msg) in [
            (r#"{"active":false,"expires":1000}"#, "License expired"),
            (r#"{"active":false,"expires":null}"#, "No active license"),
            (r#"{"active":true,"expires":1000}"#, "License expired"),
        ] {
            let (base, server) = serve_once(200, body);
            let res = block_on(clean_if_licensed(
                require_license_with(&client(), &base, "d", data.path()),
                home.path(),
                &cats(&["node"]),
                true,
                |_| {},
            ));
            server.join().unwrap();
            assert_refused(&home, &files, res, msg);
        }
    }

    #[test]
    fn active_license_cleans() {
        let home = TestDir::new("clean-licensed");
        let data = TestDir::new("clean-licensed-data");
        let npm = home.write(".npm/_cacache/blob", 8192);
        cache_license(
            data.path(),
            &LicenseStatus {
                active: true,
                expires: Some(u64::MAX),
            },
        );
        let out = block_on(clean_if_licensed(
            require_license_with(&client(), &dead_url(), "d", data.path()),
            home.path(),
            &cats(&["node"]),
            true,
            |_| {},
        ))
        .unwrap();
        assert!(!npm.exists());
        assert_eq!(out.result.categories_cleaned, 1);
    }
}
