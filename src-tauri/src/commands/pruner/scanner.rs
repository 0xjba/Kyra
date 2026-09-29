use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{ArtifactEntry, ScanProgress};
use crate::commands::utils::dir_size;

/// Maximum depth to scan for build artifacts.
const MAX_SCAN_DEPTH: usize = 6;

/// Wraps `dir_size` with a timeout to avoid stalling on huge or slow directories.
/// Returns `Some(size)` on success, `None` on timeout.
fn dir_size_with_timeout(path: &std::path::Path, timeout_secs: u64) -> Option<u64> {
    let path = path.to_path_buf();
    let handle = std::thread::spawn(move || dir_size(&path));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if handle.is_finished() {
            return Some(handle.join().unwrap_or(0));
        }
        if std::time::Instant::now() > deadline {
            return None; // timeout — size unknown
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Known build artifact directory names and their human-readable type labels.
const ARTIFACT_DIRS: &[(&str, &str)] = &[
    // JavaScript / Node.js
    ("node_modules", "Node.js"),
    ("bower_components", "Bower"),
    (".pnpm-store", "pnpm Store"),
    ("dist", "Build Output"),
    ("build", "Build Output"),
    (".next", "Next.js"),
    (".nuxt", "Nuxt.js"),
    (".output", "Nitro/Nuxt Output"),
    (".turbo", "Turbo Cache"),
    (".parcel-cache", "Parcel Cache"),
    (".angular", "Angular Cache"),
    (".svelte-kit", "SvelteKit"),
    (".astro", "Astro Cache"),
    (".vite", "Vite Cache"),
    (".nx", "Nx Cache"),
    (".docusaurus", "Docusaurus Cache"),
    ("coverage", "Test Coverage"),
    (".nyc_output", "NYC Coverage"),
    (".bun", "Bun Cache"),
    // Rust
    ("target", "Rust"),
    // Python
    ("__pycache__", "Python"),
    (".pytest_cache", "Pytest Cache"),
    ("venv", "Python Virtual Env"),
    (".venv", "Python Virtual Env"),
    ("virtualenv", "Python Virtual Env"),
    (".mypy_cache", "Mypy Cache"),
    (".tox", "Tox Env"),
    (".nox", "Nox Env"),
    (".ruff_cache", "Ruff Cache"),
    (".eggs", "Python Eggs"),
    ("htmlcov", "Python Coverage"),
    // iOS / macOS
    ("Pods", "CocoaPods"),
    ("Carthage", "Carthage"),
    (".build", "Swift"),
    ("DerivedData", "Xcode Build"),
    // Android / JVM
    (".gradle", "Gradle"),
    ("out", "Java/Kotlin Build"),
    // PHP / Go / Ruby
    ("vendor", "Vendor Deps"),
    (".bundle", "Ruby Bundler"),
    // C# / .NET
    ("obj", "C#/.NET Build"),
    ("bin", "C#/.NET Build"),
    // C++ (CMake)
    (".cxx", "C++ Build"),
    ("CMakeFiles", "CMake Build"),
    // React Native
    (".expo", "Expo Cache"),
    // Flutter / Dart
    (".dart_tool", "Dart Tool"),
    // Zig
    (".zig-cache", "Zig Cache"),
    ("zig-out", "Zig Output"),
    // Elixir
    ("_build", "Elixir"),
    ("deps", "Elixir Deps"),
    // Haskell
    ("dist-newstyle", "Haskell"),
    (".stack-work", "Haskell"),
    // OCaml
    ("_opam", "OCaml"),
    // Infrastructure
    (".terraform", "Terraform"),
];

/// File patterns for egg-info directories (matched by suffix).
const ARTIFACT_SUFFIXES: &[(&str, &str)] = &[(".egg-info", "Python")];

const RECENT_SECS: u64 = 7 * 24 * 60 * 60;

fn modified_secs(meta: &fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Artifact modified within the last 7 days (actively in use).
fn is_recent(path: &Path) -> bool {
    fs::metadata(path)
        .ok()
        .and_then(|m| modified_secs(&m))
        .map(|m| now_secs().saturating_sub(m) < RECENT_SECS)
        .unwrap_or(false)
}

/// Single source of truth for both detection and the remover's allowlist.
pub(crate) fn is_artifact_name(name: &str) -> bool {
    ARTIFACT_DIRS.iter().any(|(n, _)| *n == name)
        || ARTIFACT_SUFFIXES.iter().any(|(s, _)| name.ends_with(s))
}

fn any_exists(dir: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| dir.join(n).exists())
}

fn any_child(dir: &Path, pred: impl Fn(&str) -> bool) -> bool {
    fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok()).take(2_000).any(|e| pred(&e.file_name().to_string_lossy())))
        .unwrap_or(false)
}

fn any_child_with_suffix(dir: &Path, suffixes: &[&str]) -> bool {
    any_child(dir, |n| suffixes.iter().any(|s| n.ends_with(s)))
}

/// Folder names people also use by hand (`build/` with deploy scripts,
/// `dist/` with release notes, `target/` with plans). These count as
/// artifacts only when they hold output a build tool actually writes.
fn holds_tool_output(name: &str, path: &Path) -> bool {
    match name {
        "build" => {
            any_exists(
                path,
                &[
                    "CMakeCache.txt", "CMakeFiles", "intermediates", "generated", "outputs", "tmp",
                    "classes", "kotlin", "config.gypi", "asset-manifest.json", "XCBuildData",
                    "Release", "Debug", "Intermediates.noindex",
                ],
            ) || any_child(path, |n| {
                n.starts_with("bdist.") || n.starts_with("lib.") || n.starts_with("temp.") || n.ends_with(".build")
            })
        }
        "dist" => {
            any_exists(path, &["assets", "index.html", "builder-effective-config.yaml", "mac", "mac-arm64", "mac-universal"])
                || any_child_with_suffix(
                    path,
                    &[".js", ".mjs", ".cjs", ".map", ".css", ".whl", ".tar.gz", ".blockmap", ".dmg"],
                )
        }
        "out" => {
            any_exists(path, &["production", "artifacts", "_next", "classes"])
                || any_child_with_suffix(path, &[".class", ".js", ".jar"])
        }
        "target" => {
            any_exists(
                path,
                &[
                    "CACHEDIR.TAG", ".rustc_info.json", "debug", "release", "classes", "test-classes",
                    "maven-status", "maven-archiver", "generated-sources", "surefire-reports",
                ],
            ) || any_child_with_suffix(path, &[".jar", ".war"])
        }
        "vendor" => any_exists(path, &["autoload.php", "composer"]),
        "coverage" => any_exists(
            path,
            &[
                "lcov.info", "lcov-report", "coverage-final.json", "coverage-summary.json", "clover.xml",
                "cobertura-coverage.xml", "coverage.xml", "index.html",
            ],
        ),
        "obj" => any_exists(path, &["project.assets.json", "Debug", "Release"]) || any_child(path, |n| n.contains(".nuget.")),
        "bin" => any_exists(path, &["Debug", "Release"]),
        "deps" => {
            fs::read_dir(path)
                .map(|es| es.filter_map(|e| e.ok()).take(2_000).any(|e| any_exists(&e.path(), &["mix.exs", ".hex"])))
                .unwrap_or(false)
        }
        "_build" => any_exists(path, &["dev", "test", "prod"]),
        "Carthage" => any_exists(path, &["Build"]),
        "Pods" => any_exists(path, &["Manifest.lock"]),
        ".bundle" => any_exists(path, &["ruby", "gems", "bundler"]),
        "venv" | ".venv" | "virtualenv" => path.join("pyvenv.cfg").is_file(),
        _ => true,
    }
}

/// Reads git's offset-encoded varint (index v4 path compression).
fn read_git_varint(data: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    let mut byte = *data.first()?;
    let mut val = (byte & 0x7f) as usize;
    while byte & 0x80 != 0 {
        i += 1;
        byte = *data.get(i)?;
        val = ((val + 1) << 7) | (byte & 0x7f) as usize;
    }
    Some((val, i + 1))
}

/// Whether any path in a git index (versions 2-4) starts with `prefix`.
/// `None` when the index can't be read completely (unknown format, split
/// index).
fn index_has_prefix(data: &[u8], prefix: &[u8]) -> Option<bool> {
    if data.len() < 12 || &data[..4] != b"DIRC" {
        return None;
    }
    let version = u32::from_be_bytes(data[4..8].try_into().ok()?);
    if !(2..=4).contains(&version) {
        return None;
    }
    let count = u32::from_be_bytes(data[8..12].try_into().ok()?) as usize;
    let matches = |p: &[u8]| p.len() >= prefix.len() && p[..prefix.len()].eq_ignore_ascii_case(prefix);
    let mut pos = 12;
    let mut prev: Vec<u8> = Vec::new();
    for _ in 0..count {
        let start = pos;
        let flags = u16::from_be_bytes([*data.get(pos + 60)?, *data.get(pos + 61)?]);
        pos += 62;
        if version >= 3 && flags & 0x4000 != 0 {
            pos += 2;
        }
        let path = if version == 4 {
            let (strip, n) = read_git_varint(data.get(pos..)?)?;
            pos += n;
            let end = pos + data.get(pos..)?.iter().position(|&b| b == 0)?;
            let mut p = prev.get(..prev.len().checked_sub(strip)?)?.to_vec();
            p.extend_from_slice(&data[pos..end]);
            pos = end + 1;
            p
        } else {
            let end = pos + data.get(pos..)?.iter().position(|&b| b == 0)?;
            let p = data[pos..end].to_vec();
            pos = start + (end - start + 8) / 8 * 8;
            p
        };
        if matches(&path) {
            return Some(true);
        }
        prev = path;
    }
    // A split index keeps most entries in a shared file we don't read.
    let trailer = 20;
    while pos + 8 + trailer <= data.len() {
        let sig = &data[pos..pos + 4];
        if sig == b"link" {
            return None;
        }
        let size = u32::from_be_bytes(data[pos + 4..pos + 8].try_into().ok()?) as usize;
        pos += 8 + size;
    }
    Some(false)
}

/// True when git tracks anything inside `artifact`: committed content is
/// the user's, whatever the folder is called.
fn git_tracks_contents(artifact: &Path) -> bool {
    let mut worktree = match artifact.parent() {
        Some(p) => p,
        None => return false,
    };
    let dot_git = loop {
        let candidate = worktree.join(".git");
        if candidate.exists() {
            break candidate;
        }
        match worktree.parent() {
            Some(p) => worktree = p,
            None => return false,
        }
    };
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let Some(target) = fs::read_to_string(&dot_git)
            .ok()
            .and_then(|t| t.trim().strip_prefix("gitdir:").map(|g| g.trim().to_string()))
        else {
            return false;
        };
        worktree.join(target)
    };
    let Some(rel) = artifact.strip_prefix(worktree).ok().and_then(|r| r.to_str()) else {
        return false;
    };
    let Some(index) = read_index_cached(&git_dir.join("index")) else {
        return false;
    };
    index_has_prefix(&index, format!("{rel}/").as_bytes()) == Some(true)
}

thread_local! {
    static INDEX_CACHE: std::cell::RefCell<Option<(std::path::PathBuf, SystemTime, u64, std::rc::Rc<Vec<u8>>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Artifacts of one scan mostly share a repository, whose index can be
/// tens of megabytes: read it once per change.
fn read_index_cached(path: &Path) -> Option<std::rc::Rc<Vec<u8>>> {
    let meta = fs::metadata(path).ok()?;
    let (mtime, len) = (meta.modified().ok()?, meta.len());
    INDEX_CACHE.with(|cache| {
        if let Some((p, m, l, data)) = cache.borrow().as_ref() {
            if p == path && *m == mtime && *l == len {
                return Some(data.clone());
            }
        }
        let data = std::rc::Rc::new(fs::read(path).ok()?);
        *cache.borrow_mut() = Some((path.to_path_buf(), mtime, len, data.clone()));
        Some(data)
    })
}

fn has_file_with_suffix(dir: &Path, suffixes: &[&str]) -> bool {
    any_child_with_suffix(dir, suffixes)
}

/// Artifact type of `path` (named `name`, inside `project`), or `None` if
/// it is not a regenerable build artifact. Single source of truth for the
/// scanner and the remover.
pub(crate) fn classify_artifact(project: &Path, name: &str, path: &Path) -> Option<&'static str> {
    let label = ARTIFACT_DIRS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
        .or_else(|| ARTIFACT_SUFFIXES.iter().find(|(s, _)| name.ends_with(s)).map(|(_, t)| *t))?;
    let has = |f: &str| project.join(f).exists();
    let any = |fs: &[&str]| fs.iter().any(|f| has(f));
    let ok = match name {
        "target" => has("pom.xml") || has("Cargo.toml"),
        "node_modules" => any(&["package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb"]),
        "Pods" => has("Podfile"),
        ".build" => has("Package.swift"),
        "build" => any(&["package.json", "build.gradle", "build.gradle.kts", "Makefile"]),
        "dist" => has("package.json"),
        ".output" | ".turbo" | ".parcel-cache" | ".angular" | ".svelte-kit" | ".astro" | ".next" | ".nuxt"
        | ".expo" | ".vite" | ".nx" | ".nyc_output" | ".docusaurus" => has("package.json"),
        "coverage" => any(&[
            "package.json", "pytest.ini", "setup.py", "setup.cfg", "pyproject.toml", "requirements.txt",
            "Cargo.toml", "go.mod",
        ]),
        // Only PHP Composer: Go and Ruby vendor dirs are deliberately vendored.
        "vendor" => has("composer.json"),
        "obj" => has_file_with_suffix(project, &[".csproj", ".sln", ".fsproj"]),
        "bin" => has_file_with_suffix(project, &[".csproj", ".fsproj", ".vbproj"]),
        ".bundle" => has("Gemfile"),
        "CMakeFiles" | ".cxx" => has("CMakeLists.txt"),
        "_build" | "deps" => has("mix.exs"),
        "dist-newstyle" => has_file_with_suffix(project, &[".cabal"]) || has("cabal.project"),
        ".stack-work" => has("stack.yaml"),
        "_opam" => has("dune-project") || has_file_with_suffix(project, &[".opam"]),
        ".bun" => any(&["package.json", "bun.lockb"]),
        "venv" | ".venv" | "virtualenv" => true,
        "__pycache__" => has_file_with_suffix(project, &[".py"]),
        ".pytest_cache" => any(&["pytest.ini", "setup.cfg", "pyproject.toml", "conftest.py"]),
        ".mypy_cache" => any(&["mypy.ini", "setup.cfg", "pyproject.toml"]),
        ".tox" => any(&["tox.ini", "pyproject.toml", "setup.py"]),
        ".nox" => any(&["noxfile.py", "pyproject.toml", "setup.py"]),
        ".ruff_cache" => any(&["pyproject.toml", "setup.py", "ruff.toml", ".ruff.toml"]),
        ".eggs" => any(&["setup.py", "setup.cfg", "pyproject.toml"]),
        "htmlcov" => any(&["setup.py", "pyproject.toml", "pytest.ini", "setup.cfg"]),
        ".gradle" => any(&["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"]),
        "out" => any(&["build.gradle", "build.gradle.kts", "pom.xml"]),
        ".dart_tool" => has("pubspec.yaml"),
        ".zig-cache" | "zig-out" => any(&["build.zig", "build.zig.zon"]),
        "bower_components" => any(&["bower.json", "package.json"]),
        ".pnpm-store" => any(&["package.json", "pnpm-lock.yaml"]),
        "Carthage" => has("Cartfile"),
        ".terraform" => has_file_with_suffix(project, &[".tf"]),
        "DerivedData" => has_file_with_suffix(project, &[".xcodeproj", ".xcworkspace"]),
        _ => true,
    };
    if !ok || !holds_tool_output(name, path) || git_tracks_contents(path) {
        return None;
    }
    if name == "target" && has("pom.xml") {
        return Some("Maven");
    }
    Some(label)
}

/// Last activity of a project: newest mtime among the root's direct entries
/// (build artifacts excluded, since builds touch them without real work),
/// plus `.git/HEAD` and `.git/index` so commits and checkouts count.
/// Depth 1 only; falls back to the root's own mtime.
fn project_last_activity(project: &Path) -> u64 {
    let mut newest = 0u64;
    if let Ok(entries) = fs::read_dir(project) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name == ".DS_Store" || is_artifact_name(&name) {
                continue;
            }
            if let Some(m) = entry.metadata().ok().and_then(|m| modified_secs(&m)) {
                newest = newest.max(m);
            }
        }
    }
    for git_file in [".git/HEAD", ".git/index"] {
        if let Some(m) = fs::metadata(project.join(git_file))
            .ok()
            .and_then(|m| modified_secs(&m))
        {
            newest = newest.max(m);
        }
    }
    if newest == 0 {
        newest = fs::metadata(project)
            .ok()
            .and_then(|m| modified_secs(&m))
            .unwrap_or(0);
    }
    newest
}

/// Scans `root` recursively for artifact directories.
/// Emits progress every 50 artifacts found.
/// When an artifact directory is found, it is NOT descended into.
pub fn scan_for_artifacts<F>(root: &str, mut on_progress: F) -> Vec<ArtifactEntry>
where
    F: FnMut(&ScanProgress),
{
    // Expand ~ to home directory
    let expanded = if root.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(&root[2..])
        } else {
            std::path::PathBuf::from(root)
        }
    } else if root == "~" {
        dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(root))
    } else {
        std::path::PathBuf::from(root)
    };

    let root_path = expanded.as_path();
    if !root_path.is_dir() {
        return Vec::new();
    }

    let mut results: Vec<ArtifactEntry> = Vec::new();
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root_path.to_path_buf(), 0)];

    while let Some((dir, current_depth)) = stack.pop() {
        if current_depth >= MAX_SCAN_DEPTH {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Skip symlinks
            if path.is_symlink() {
                continue;
            }

            if !path.is_dir() {
                continue;
            }

            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            if name.starts_with('.') && !is_artifact_name(&name) {
                continue;
            }

            let mut is_artifact = false;
            if is_artifact_name(&name) {
                if let Some(artifact_type) = classify_artifact(&dir, &name, &path) {
                    is_artifact = true;
                    if crate::commands::data_guard::check_general(&path).is_ok() {
                        let size_result = dir_size_with_timeout(&path, 15);
                        // Skip genuinely empty artifacts (Some(0)), keep timeouts (None) with size 0
                        if size_result != Some(0) {
                            results.push(ArtifactEntry {
                                project_name: dir
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                project_path: dir.to_string_lossy().to_string(),
                                artifact_type: artifact_type.to_string(),
                                artifact_path: path.to_string_lossy().to_string(),
                                size: size_result.unwrap_or(0),
                                is_recent: is_recent(&path),
                                last_modified_secs: 0,
                            });
                            if results.len() % 50 == 0 {
                                on_progress(&ScanProgress {
                                    current_path: dir.to_string_lossy().to_string(),
                                    artifacts_found: results.len(),
                                });
                            }
                        }
                    }
                }
            }

            // Only descend if this was NOT an artifact directory
            if !is_artifact {
                // Skip directories that waste time or produce false positives
                if name == ".Trash" || name == "Applications" {
                    continue;
                }
                // Skip ~/Library — it's huge and not relevant
                if name == "Library" {
                    if let Some(home) = dirs::home_dir() {
                        if dir == home {
                            continue;
                        }
                    }
                }
                // Nothing inside wallets, vaults, cloud folders and the like
                // is ever pruned, so don't walk them.
                if matches!(
                    crate::commands::data_guard::static_verdict(&path),
                    crate::commands::data_guard::Verdict::Protected { contains: false, .. }
                ) {
                    continue;
                }
                stack.push((path, current_depth + 1));
            }
        }
    }

    // Final progress emit
    on_progress(&ScanProgress {
        current_path: root.to_string(),
        artifacts_found: results.len(),
    });

    let mut activity: HashMap<String, u64> = HashMap::new();
    for entry in results.iter_mut() {
        entry.last_modified_secs = *activity
            .entry(entry.project_path.clone())
            .or_insert_with(|| project_last_activity(Path::new(&entry.project_path)));
    }

    // Sort by size descending
    results.sort_by(|a, b| b.size.cmp(&a.size));

    results
}

#[cfg(test)]
pub(crate) fn artifact_dir_names() -> impl Iterator<Item = &'static str> {
    ARTIFACT_DIRS.iter().map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{mkdir, s, set_age_days, write_file};
    use std::os::unix::fs::symlink;

    fn scan(root: &Path) -> Vec<ArtifactEntry> {
        scan_for_artifacts(&s(root), |_| {})
    }

    fn found(results: &[ArtifactEntry]) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = results
            .iter()
            .map(|e| (e.artifact_path.clone(), e.artifact_type.clone()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn detects_artifacts_only_inside_real_projects() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("web/package.json"), 2);
        write_file(&r.join("web/node_modules/x/index.js"), 1_000);
        write_file(&r.join("web/dist/app.js"), 1_000);
        write_file(&r.join("crate/Cargo.toml"), 2);
        write_file(&r.join("crate/target/debug/bin"), 1_000);
        write_file(&r.join("java/pom.xml"), 2);
        write_file(&r.join("java/target/app.jar"), 1_000);
        write_file(&r.join("py/requirements.txt"), 2);
        write_file(&r.join("py/venv/lib/site.py"), 1_000);
        write_file(&r.join("py/venv/pyvenv.cfg"), 10);
        write_file(&r.join("py/mod.py"), 2);
        write_file(&r.join("py/__pycache__/mod.pyc"), 1_000);
        write_file(&r.join("py/pkg.egg-info/PKG-INFO"), 1_000);

        // Look-alikes with no project marker next to them.
        write_file(&r.join("notes/node_modules/readme.txt"), 1_000);
        write_file(&r.join("notes/target/plan.txt"), 1_000);
        write_file(&r.join("notes/dist/report.pdf"), 1_000);
        write_file(&r.join("notes/vendor/thing.txt"), 1_000);
        write_file(&r.join("notes/build/out.txt"), 1_000);
        write_file(&r.join("notes/__pycache__/x.pyc"), 1_000);

        let got = found(&scan(r));
        let expect = |rel: &str, ty: &str| (s(&r.join(rel)), ty.to_string());
        let mut want = vec![
            expect("web/node_modules", "Node.js"),
            expect("web/dist", "Build Output"),
            expect("crate/target", "Rust"),
            expect("java/target", "Maven"),
            expect("py/venv", "Python Virtual Env"),
            expect("py/__pycache__", "Python"),
            expect("py/pkg.egg-info", "Python"),
        ];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn does_not_descend_into_artifacts_or_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("app/package.json"), 2);
        write_file(&r.join("app/node_modules/dep/package.json"), 2);
        write_file(&r.join("app/node_modules/dep/node_modules/inner/i.js"), 1_000);

        let elsewhere = tempfile::tempdir().unwrap();
        write_file(&elsewhere.path().join("proj/package.json"), 2);
        write_file(&elsewhere.path().join("proj/node_modules/a.js"), 1_000);
        symlink(elsewhere.path().join("proj"), r.join("linked-proj")).unwrap();

        let got = found(&scan(r));
        assert_eq!(got, vec![(s(&r.join("app/node_modules")), "Node.js".to_string())]);
    }

    #[test]
    fn skips_empty_artifacts_and_unlisted_hidden_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("empty/package.json"), 2);
        mkdir(&r.join("empty/node_modules"));
        write_file(&r.join(".hidden/app/package.json"), 2);
        write_file(&r.join(".hidden/app/node_modules/a.js"), 1_000);
        write_file(&r.join("site/package.json"), 2);
        write_file(&r.join("site/.next/cache.bin"), 1_000);

        let got = found(&scan(r));
        assert_eq!(got, vec![(s(&r.join("site/.next")), "Next.js".to_string())]);
    }

    #[test]
    fn respects_max_scan_depth() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let shallow = r.join("a/b/c/d/e");
        write_file(&shallow.join("package.json"), 2);
        write_file(&shallow.join("node_modules/x.js"), 1_000);
        let deep = r.join("a/b/c/d/e/f/g");
        write_file(&deep.join("package.json"), 2);
        write_file(&deep.join("node_modules/x.js"), 1_000);

        let got = found(&scan(r));
        assert_eq!(got, vec![(s(&shallow.join("node_modules")), "Node.js".to_string())]);
    }

    #[test]
    fn results_are_sorted_by_size_descending() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("small/package.json"), 2);
        write_file(&r.join("small/node_modules/a.js"), 5_000);
        write_file(&r.join("big/package.json"), 2);
        write_file(&r.join("big/node_modules/a.js"), 500_000);
        let results = scan(r);
        assert_eq!(results.len(), 2);
        assert!(results[0].size >= results[1].size);
        assert!(results[0].artifact_path.ends_with("big/node_modules"));
        assert_eq!(results[0].project_name, "big");
        assert_eq!(results[0].project_path, s(&r.join("big")));
    }

    #[test]
    fn recency_follows_artifact_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("old/package.json"), 2);
        write_file(&r.join("old/node_modules/a.js"), 1_000);
        set_age_days(&r.join("old/node_modules"), 30);
        write_file(&r.join("new/package.json"), 2);
        write_file(&r.join("new/node_modules/a.js"), 1_000);

        let results = scan(r);
        let by_project = |name: &str| results.iter().find(|e| e.project_name == name).unwrap();
        assert!(!by_project("old").is_recent);
        assert!(by_project("new").is_recent);
        assert!(!is_recent(&r.join("missing")));
    }

    #[test]
    fn last_activity_ignores_artifacts_but_counts_git() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("proj");
        write_file(&p.join("package.json"), 2);
        write_file(&p.join("src/index.js"), 2);
        write_file(&p.join("node_modules/a.js"), 1_000);
        write_file(&p.join(".git/HEAD"), 2);
        for rel in ["package.json", "src", ".git/HEAD"] {
            set_age_days(&p.join(rel), 100);
        }
        let now = now_secs();
        let hundred_days = 100 * 86_400;

        let activity = project_last_activity(&p);
        assert!(now - activity >= hundred_days - 60, "fresh node_modules counted as activity");

        let results = scan(dir.path());
        assert_eq!(results[0].last_modified_secs, activity);

        write_file(&p.join(".git/index"), 2);
        assert!(now_secs() - project_last_activity(&p) < 60, "git index write not counted");
    }

    #[test]
    fn last_activity_falls_back_to_project_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("proj");
        write_file(&p.join("node_modules/a.js"), 1);
        set_age_days(&p, 10);
        let activity = project_last_activity(&p);
        let age = now_secs() - activity;
        assert!((10 * 86_400 - 60..10 * 86_400 + 60).contains(&age), "{age}");
    }

    #[test]
    fn missing_root_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn git_index_paths_are_read_in_every_version() {
        let entry = |v: &mut Vec<u8>, extended: bool| {
            v.extend([0u8; 60]);
            v.extend((if extended { 0x4000u16 } else { 0 }).to_be_bytes());
            if extended {
                v.extend([0u8; 2]);
            }
        };
        let header = |version: u32, count: u32| {
            let mut v = b"DIRC".to_vec();
            v.extend(version.to_be_bytes());
            v.extend(count.to_be_bytes());
            v
        };

        let mut v3 = header(3, 2);
        for (path, extended) in [("a/x", true), ("build/icon.icns", false)] {
            let start = v3.len();
            entry(&mut v3, extended);
            v3.extend(path.as_bytes());
            let len = v3.len() - start;
            v3.resize(start + (len + 8) / 8 * 8, 0);
        }
        v3.extend([0u8; 20]);
        assert_eq!(index_has_prefix(&v3, b"build/"), Some(true));
        assert_eq!(index_has_prefix(&v3, b"dist/"), Some(false));

        let mut v4 = header(4, 2);
        entry(&mut v4, false);
        v4.extend([0u8]);
        v4.extend(b"build/a.txt\0");
        entry(&mut v4, false);
        v4.extend([5u8]);
        v4.extend(b"icon.icns\0");
        v4.extend([0u8; 20]);
        assert_eq!(index_has_prefix(&v4, b"build/icon"), Some(true));
        assert_eq!(index_has_prefix(&v4, b"src/"), Some(false));

        let mut split = header(2, 0);
        split.extend(b"link");
        split.extend(4u32.to_be_bytes());
        split.extend([0u8; 4]);
        split.extend([0u8; 20]);
        assert_eq!(index_has_prefix(&split, b"build/"), None);
        assert_eq!(index_has_prefix(b"garbage", b"build/"), None);
    }

    #[test]
    fn hand_made_folders_with_artifact_names_are_not_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_file(&r.join("site/package.json"), 2);
        write_file(&r.join("site/Makefile"), 2);
        write_file(&r.join("site/build/deploy.sh"), 1_000);
        write_file(&r.join("site/dist/RELEASE_NOTES.md"), 1_000);
        write_file(&r.join("site/coverage/plan.md"), 1_000);
        write_file(&r.join("rs/Cargo.toml"), 2);
        write_file(&r.join("rs/target/notes.md"), 1_000);
        write_file(&r.join("py/requirements.txt"), 2);
        write_file(&r.join("py/venv/my_script.py"), 1_000);
        write_file(&r.join("nb/a.ipynb"), 2);
        write_file(&r.join("nb/.ipynb_checkpoints/a-checkpoint.ipynb"), 1_000);
        assert!(scan(r).is_empty(), "{:#?}", found(&scan(r)));
    }

    #[test]
    fn artifact_names_and_suffixes() {
        assert!(is_artifact_name("node_modules"));
        assert!(is_artifact_name("foo.egg-info"));
        assert!(!is_artifact_name("src"));
        assert!(!is_artifact_name("node_modules2"));
    }
}
