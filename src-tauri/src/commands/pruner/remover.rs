use std::fs;
use std::path::Path;

use super::scanner::classify_artifact;
use super::{PruneProgress, PruneResult};
use crate::commands::shared;
use crate::commands::utils::{dir_size, is_same_or_under};

/// Paths that must never be deleted.
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

/// Returns true if a path is safe to delete for prune operations.
/// Canonicalizes the path to prevent traversal attacks.
fn is_safe_path(path_str: &str) -> bool {
    // Canonicalize to resolve any .. or symlinks
    let canonical = match fs::canonicalize(path_str) {
        Ok(p) => p,
        Err(_) => return false, // Can't resolve = don't delete
    };
    let path = canonical.to_string_lossy();

    if PROTECTED_PATHS.iter().any(|p| is_same_or_under(&path, p)) {
        return false;
    }

    // Block home directory itself
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path.as_ref() == home_str.as_ref() {
            return false;
        }
    }

    if crate::commands::data_guard::check_general(&canonical).is_err() {
        return false;
    }

    // Re-run the scanner's detection: a path is only removable if it is
    // still an artifact of a real project, not committed, and holds tool
    // output — whatever list the frontend sent.
    match (canonical.parent(), canonical.file_name().and_then(|n| n.to_str())) {
        (Some(project), Some(name)) => classify_artifact(project, name, &canonical).is_some(),
        _ => false,
    }
}

/// Removes selected artifact directories.
pub fn remove_artifacts<F>(
    paths: &[String],
    dry_run: bool,
    permanent: bool,
    mut on_progress: F,
) -> PruneResult
where
    F: FnMut(&PruneProgress),
{
    let mut bytes_freed: u64 = 0;
    let mut items_removed: usize = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut cleaned_paths: Vec<String> = Vec::new();
    let items_total = paths.len();

    for (i, path_str) in paths.iter().enumerate() {
        let path = Path::new(path_str);

        if !is_safe_path(path_str) {
            errors.push(format!("Skipped protected path: {}", path_str));
            on_progress(&PruneProgress {
                current_item: path_str.clone(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        if !path.exists() {
            on_progress(&PruneProgress {
                current_item: path_str.clone(),
                items_done: i + 1,
                items_total,
                bytes_freed,
            });
            continue;
        }

        let size = dir_size(path);

        if dry_run {
            bytes_freed += size;
            items_removed += 1;
            cleaned_paths.push(path_str.clone());
        } else {
            let delete_result = if permanent {
                fs::remove_dir_all(path)
            } else {
                trash::delete(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            };
            match delete_result {
                Ok(()) => {
                    bytes_freed += size;
                    items_removed += 1;
                    cleaned_paths.push(path_str.clone());
                    let action = if permanent { "DELETED" } else { "TRASHED" };
                    shared::log_operation("PRUNE", path_str, action);
                }
                Err(e) => {
                    shared::log_operation("PRUNE", path_str, &format!("ERROR: {}", e));
                    errors.push(format!("{}: {}", path_str, e));
                }
            }
        }

        on_progress(&PruneProgress {
            current_item: path_str.clone(),
            items_done: i + 1,
            items_total,
            bytes_freed,
        });
    }

    PruneResult {
        items_removed,
        bytes_freed,
        errors,
        cleaned_paths,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::pruner::scanner::is_artifact_name;
    use crate::commands::test_support::{mkdir, s, write_file};
    use std::os::unix::fs::symlink;

    #[test]
    fn artifact_named_folders_inside_browser_profiles_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("Library/Application Support/Google/Chrome/Default");
        write_file(&profile.join("Login Data"), 10);
        write_file(&profile.join("build/x.bin"), 10);
        let result = remove_artifacts(&[s(&profile.join("build"))], false, true, |_| {});
        assert_eq!(result.items_removed, 0);
        assert!(profile.join("build/x.bin").exists());
        assert!(profile.join("Login Data").exists());
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        write_file(&dir.path().join("app/package.json"), 2);
        write_file(&dir.path().join("app/node_modules/left-pad/index.js"), 4_000);
        write_file(&dir.path().join("app/src/main.js"), 100);
        dir
    }

    fn prune(paths: &[String], dry_run: bool) -> PruneResult {
        remove_artifacts(paths, dry_run, true, |_| {})
    }

    #[test]
    fn removes_artifact_dirs_permanently() {
        let dir = project();
        let nm = dir.path().join("app/node_modules");
        let result = prune(&[s(&nm)], false);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.items_removed, 1);
        assert!(result.bytes_freed >= 4_000);
        assert_eq!(result.cleaned_paths, vec![s(&nm)]);
        assert!(!nm.exists());
        assert!(dir.path().join("app/src/main.js").exists());
    }

    #[test]
    fn dry_run_leaves_artifacts_in_place() {
        let dir = project();
        let nm = dir.path().join("app/node_modules");
        let result = prune(&[s(&nm)], true);
        assert_eq!(result.items_removed, 1);
        assert!(result.bytes_freed >= 4_000);
        assert!(nm.join("left-pad/index.js").exists());
    }

    #[test]
    fn non_artifact_directories_are_refused() {
        let dir = project();
        let src = dir.path().join("app/src");
        let result = prune(&[s(&src), s(&dir.path().join("app"))], false);
        assert_eq!(result.items_removed, 0);
        assert_eq!(result.errors.len(), 2);
        assert!(src.join("main.js").exists());
    }

    #[test]
    fn traversal_out_of_an_artifact_is_refused() {
        let dir = project();
        let sneaky = format!("{}/app/node_modules/../src", s(dir.path()));
        let result = prune(&[sneaky], false);
        assert_eq!(result.items_removed, 0);
        assert!(dir.path().join("app/src/main.js").exists());
    }

    #[test]
    fn symlink_named_like_an_artifact_is_judged_by_its_target() {
        let dir = project();
        let precious = dir.path().join("precious");
        write_file(&precious.join("thesis.docx"), 10);
        let link = dir.path().join("app/build");
        symlink(&precious, &link).unwrap();

        let result = prune(&[s(&link)], false);
        assert_eq!(result.items_removed, 0);
        assert!(precious.join("thesis.docx").exists());
    }

    #[test]
    fn symlink_to_a_real_artifact_only_removes_the_link() {
        let dir = project();
        let nm = dir.path().join("app/node_modules");
        let other = dir.path().join("other");
        mkdir(&other);
        let link = other.join("node_modules");
        symlink(&nm, &link).unwrap();

        let result = prune(&[s(&link)], false);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(nm.join("left-pad/index.js").exists());
    }

    #[test]
    fn missing_and_system_paths_are_refused() {
        let dir = project();
        let paths: Vec<String> = [
            "/",
            "/usr/bin",
            "/System/Library",
            "/Applications",
            "/Applications/node_modules",
            "",
        ]
        .iter()
        .map(|p| p.to_string())
        .chain([s(&dir.path().join("app/missing/node_modules"))])
        .collect();
        // dry_run keeps this harmless even if a guard regressed.
        let result = prune(&paths, true);
        assert_eq!(result.items_removed, 0);
        assert_eq!(result.errors.len(), paths.len());
        if let Some(home) = dirs::home_dir() {
            assert!(!is_safe_path(&s(&home)));
            assert!(!is_safe_path(&s(&home.join("Documents"))));
        }
    }

    #[test]
    fn artifact_name_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["package.json", "Cargo.toml", "a.py", "App.xcodeproj/project.pbxproj"] {
            write_file(&dir.path().join(f), 2);
        }
        for f in [
            "node_modules/x/index.js",
            "target/CACHEDIR.TAG",
            "__pycache__/a.pyc",
            ".venv/pyvenv.cfg",
            "DerivedData/x",
            "foo.egg-info/PKG-INFO",
        ] {
            write_file(&dir.path().join(f), 2);
        }
        for name in ["node_modules", "target", "__pycache__", ".venv", "DerivedData", "foo.egg-info"] {
            let p = dir.path().join(name);
            assert!(is_safe_path(&s(&p)), "{name}");
        }
        for name in ["src", "Documents", "node_modules_backup", ".git", "Library"] {
            let p = dir.path().join(name);
            mkdir(&p);
            assert!(!is_safe_path(&s(&p)), "{name}");
        }
    }

    #[test]
    fn progress_reports_every_item() {
        let dir = project();
        let mut done = Vec::new();
        remove_artifacts(
            &[s(&dir.path().join("app/node_modules")), "/usr/bin".into()],
            true,
            true,
            |p| done.push((p.items_done, p.items_total)),
        );
        assert_eq!(done, vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn every_scanned_artifact_type_is_removable() {
        let missing: Vec<&str> = crate::commands::pruner::scanner::artifact_dir_names()
            .filter(|n| !is_artifact_name(n))
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    /// Each artifact name with the project markers its detection requires.
    const DETECTABLE: &[(&str, &[&str])] = &[
        ("node_modules", &["package.json"]),
        ("bower_components", &["bower.json"]),
        (".pnpm-store", &["pnpm-lock.yaml"]),
        ("dist", &["package.json"]),
        ("build", &["package.json"]),
        (".next", &["package.json"]),
        (".nuxt", &["package.json"]),
        (".output", &["package.json"]),
        (".turbo", &["package.json"]),
        (".parcel-cache", &["package.json"]),
        (".angular", &["package.json"]),
        (".svelte-kit", &["package.json"]),
        (".astro", &["package.json"]),
        (".vite", &["package.json"]),
        (".nx", &["package.json", "nx.json"]),
        (".docusaurus", &["package.json"]),
        ("coverage", &["package.json"]),
        (".nyc_output", &["package.json"]),
        (".bun", &["bun.lockb"]),
        ("target", &["Cargo.toml"]),
        ("__pycache__", &["mod.py"]),
        (".pytest_cache", &["pytest.ini"]),
        ("venv", &["requirements.txt"]),
        (".venv", &["pyproject.toml"]),
        ("virtualenv", &["setup.py"]),
        (".mypy_cache", &["mypy.ini"]),
        (".tox", &["tox.ini"]),
        (".nox", &["noxfile.py"]),
        (".ruff_cache", &["ruff.toml"]),
        (".eggs", &["setup.cfg"]),
        ("htmlcov", &["pyproject.toml"]),
        ("Pods", &["Podfile"]),
        ("Carthage", &["Cartfile"]),
        (".build", &["Package.swift"]),
        ("DerivedData", &["App.xcodeproj/project.pbxproj"]),
        (".gradle", &["settings.gradle"]),
        ("out", &["build.gradle"]),
        ("vendor", &["composer.json"]),
        (".bundle", &["Gemfile"]),
        ("obj", &["App.csproj"]),
        ("bin", &["App.csproj"]),
        (".cxx", &["CMakeLists.txt"]),
        ("CMakeFiles", &["CMakeLists.txt"]),
        (".expo", &["package.json"]),
        (".dart_tool", &["pubspec.yaml"]),
        (".zig-cache", &["build.zig"]),
        ("zig-out", &["build.zig"]),
        ("_build", &["mix.exs"]),
        ("deps", &["mix.exs"]),
        ("dist-newstyle", &["cabal.project"]),
        (".stack-work", &["stack.yaml"]),
        ("_opam", &["dune-project"]),
        (".terraform", &["main.tf"]),
    ];

    /// A file a build tool writes into an artifact of this name.
    fn tool_output(name: &str) -> &'static str {
        match name {
            "dist" => "assets/index.js",
            "out" => "production/Main.class",
            "target" => "CACHEDIR.TAG",
            "vendor" => "autoload.php",
            "coverage" => "lcov.info",
            "obj" => "project.assets.json",
            "deps" => "dep/mix.exs",
            "_build" => "dev/lib/x",
            "Carthage" => "Build/x",
            "Pods" => "Manifest.lock",
            ".bundle" => "ruby/3.3.0/x",
            "venv" | ".venv" | "virtualenv" => "pyvenv.cfg",
            _ => "Debug/blob2",
        }
    }

    #[test]
    fn every_detected_artifact_is_accepted_by_the_remover() {
        let mut names: Vec<&str> = DETECTABLE.iter().map(|(n, _)| *n).collect();
        let mut all: Vec<&str> = crate::commands::pruner::scanner::artifact_dir_names().collect();
        names.sort();
        all.sort();
        assert_eq!(names, all, "DETECTABLE must list every scanner artifact type");

        let dir = tempfile::tempdir().unwrap();
        for (i, (name, markers)) in DETECTABLE.iter().enumerate() {
            let project = dir.path().join(format!("p{i}"));
            for m in *markers {
                write_file(&project.join(m), 2);
            }
            write_file(&project.join(name).join("Debug/blob"), 1_000);
            write_file(&project.join(name).join(tool_output(name)), 10);
        }
        write_file(&dir.path().join("py/pkg.egg-info/PKG-INFO"), 1_000);

        let found: Vec<String> = crate::commands::pruner::scanner::scan_for_artifacts(&s(dir.path()), |_| {})
            .into_iter()
            .map(|e| e.artifact_path)
            .collect();
        assert_eq!(found.len(), DETECTABLE.len() + 1, "{found:#?}");

        let result = prune(&found, true);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.items_removed, found.len());
    }

    /// Minimal git index (version 2) listing `tracked`.
    fn write_git_index(repo: &Path, tracked: &[&str]) {
        let mut data = b"DIRC".to_vec();
        data.extend(2u32.to_be_bytes());
        data.extend((tracked.len() as u32).to_be_bytes());
        for path in tracked {
            let start = data.len();
            data.extend([0u8; 60]);
            data.extend((path.len() as u16).to_be_bytes());
            data.extend(path.as_bytes());
            let len = data.len() - start;
            data.resize(start + (len + 8) / 8 * 8, 0);
        }
        data.extend([0u8; 20]);
        fs::write(repo.join(".git/index"), data).unwrap();
    }

    #[test]
    fn pruning_a_fake_data_home_removes_only_real_artifacts() {
        use crate::commands::data_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let p = |rel: &str| fx.path(rel);
        write_file(&p("Projects/site/node_modules/react/index.js"), 4_000);
        write_file(&p("Projects/tool/Cargo.toml"), 10);
        write_file(&p("Projects/tool/target/CACHEDIR.TAG"), 10);
        write_file(&p("Projects/tool/target/debug/tool"), 4_000);
        write_file(&p("Projects/app/package.json"), 10);
        write_file(&p("Projects/app/build/icon.icns"), 4_000);
        write_file(&p("Projects/app/build/Release/app.node"), 4_000);
        write_git_index(&p("Projects/app"), &["build/icon.icns", "package.json", "src/main.rs"]);
        write_file(&p("Projects/notebooks/requirements.txt"), 10);

        let found: Vec<String> = crate::commands::pruner::scanner::scan_for_artifacts(&s(&fx.home), |_| {})
            .into_iter()
            .map(|e| e.artifact_path)
            .collect();
        let mut rel: Vec<String> =
            found.iter().map(|f| Path::new(f).strip_prefix(&fx.home).unwrap().display().to_string()).collect();
        rel.sort();
        assert_eq!(rel, vec!["Projects/site/node_modules", "Projects/tool/target"]);

        let mut hostile = found.clone();
        for rel in [
            "Projects/site/build",
            "Projects/site/dist",
            "Projects/rusty/target",
            "Projects/app/build",
            "Projects/notebooks/.ipynb_checkpoints",
            "Library/Mobile Documents/com~apple~CloudDocs/Projects/site/node_modules",
        ] {
            hostile.push(s(&p(rel)));
        }
        let result = remove_artifacts(&hostile, false, true, |_| {});
        assert_eq!(result.items_removed, 2, "{:?}", result.errors);
        fx.assert_intact();
        assert!(p("Projects/app/build/icon.icns").exists(), "committed build resources stay");
        assert!(!p("Projects/site/node_modules").exists());
        assert!(!p("Projects/tool/target").exists());
    }

    #[test]
    fn yarn_berry_dir_is_neither_detected_nor_removable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("app");
        write_file(&p.join("package.json"), 2);
        write_file(&p.join(".yarn/releases/yarn-4.cjs"), 1_000);
        write_file(&p.join(".yarn/patches/fix.patch"), 1_000);
        assert!(crate::commands::pruner::scanner::scan_for_artifacts(&s(dir.path()), |_| {}).is_empty());
        assert!(!is_safe_path(&s(&p.join(".yarn"))));
    }
}
