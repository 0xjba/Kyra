use std::fs;

use super::{ArtifactEntry, ScanProgress};
use crate::commands::utils::dir_size;

/// Known build artifact directory names and their human-readable type labels.
const ARTIFACT_DIRS: &[(&str, &str)] = &[
    // JavaScript / Node.js
    ("node_modules", "Node.js"),
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
    ("coverage", "Test Coverage"),
    (".bun", "Bun Cache"),
    // Rust
    ("target", "Rust"),
    // Python
    ("__pycache__", "Python"),
    (".pytest_cache", "Pytest Cache"),
    ("venv", "Python Virtual Env"),
    (".venv", "Python Virtual Env"),
    (".mypy_cache", "Mypy Cache"),
    (".tox", "Tox Env"),
    (".nox", "Nox Env"),
    (".ruff_cache", "Ruff Cache"),
    // iOS / macOS
    ("Pods", "CocoaPods"),
    (".build", "Swift"),
    ("DerivedData", "Xcode Build"),
    // Android / JVM
    (".gradle", "Gradle"),
    // PHP / Go / Ruby
    ("vendor", "Vendor Deps"),
    (".bundle", "Ruby Bundler"),
    // C# / .NET
    ("obj", "C#/.NET Build"),
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
];

/// File patterns for egg-info directories (matched by suffix).
const ARTIFACT_SUFFIXES: &[(&str, &str)] = &[(".egg-info", "Python")];

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
    let mut stack: Vec<std::path::PathBuf> = vec![root_path.to_path_buf()];

    while let Some(dir) = stack.pop() {
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

            // Skip hidden directories (except our known artifact dirs starting with .)
            if name.starts_with('.')
                && !matches!(
                    name.as_str(),
                    ".next"
                        | ".nuxt"
                        | ".output"
                        | ".turbo"
                        | ".parcel-cache"
                        | ".angular"
                        | ".svelte-kit"
                        | ".astro"
                        | ".pytest_cache"
                        | ".venv"
                        | ".mypy_cache"
                        | ".tox"
                        | ".nox"
                        | ".ruff_cache"
                        | ".gradle"
                        | ".build"
                        | ".cxx"
                        | ".expo"
                        | ".dart_tool"
                        | ".zig-cache"
                        | ".bun"
                        | ".bundle"
                        | ".stack-work"
                )
            {
                continue;
            }

            // Check if this directory is a known artifact
            let mut is_artifact = false;

            for (artifact_name, artifact_type) in ARTIFACT_DIRS {
                if name == *artifact_name {
                    let project_path = dir.to_string_lossy().to_string();
                    let project_name = dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    // Validate context: only match if parent has relevant project files
                    // target — could be Rust (Cargo.toml) or Maven (pom.xml)
                    if name == "target" {
                        if dir.join("pom.xml").exists() {
                            // Maven project — override the artifact_type
                            let size = dir_size(&path);
                            results.push(ArtifactEntry {
                                project_name: project_name.clone(),
                                project_path: project_path.clone(),
                                artifact_type: "Maven".to_string(),
                                artifact_path: path.to_string_lossy().to_string(),
                                size,
                            });
                            is_artifact = true;
                            if results.len() % 50 == 0 {
                                on_progress(&ScanProgress {
                                    current_path: dir.to_string_lossy().to_string(),
                                    artifacts_found: results.len(),
                                });
                            }
                            break;
                        } else if !dir.join("Cargo.toml").exists() {
                            continue;
                        }
                    }
                    if name == "build"
                        && !dir.join("package.json").exists()
                        && !dir.join("build.gradle").exists()
                        && !dir.join("build.gradle.kts").exists()
                    {
                        continue;
                    }
                    if name == "dist" && !dir.join("package.json").exists() {
                        continue;
                    }
                    // Frontend framework caches — validate with package.json
                    if matches!(
                        name.as_str(),
                        ".output"
                            | ".turbo"
                            | ".parcel-cache"
                            | ".angular"
                            | ".svelte-kit"
                            | ".astro"
                            | "coverage"
                    ) && !dir.join("package.json").exists()
                    {
                        continue;
                    }
                    // vendor — validate with composer.json, go.mod, or Gemfile
                    if name == "vendor"
                        && !dir.join("composer.json").exists()
                        && !dir.join("go.mod").exists()
                        && !dir.join("Gemfile").exists()
                    {
                        continue;
                    }
                    // obj — validate with .csproj, .sln, or .fsproj
                    if name == "obj" {
                        let has_dotnet = fs::read_dir(&dir)
                            .map(|entries| {
                                entries.filter_map(|e| e.ok()).any(|e| {
                                    let n = e.file_name();
                                    let n = n.to_string_lossy();
                                    n.ends_with(".csproj")
                                        || n.ends_with(".sln")
                                        || n.ends_with(".fsproj")
                                })
                            })
                            .unwrap_or(false);
                        if !has_dotnet {
                            continue;
                        }
                    }
                    // .bundle — validate with Gemfile
                    if name == ".bundle" && !dir.join("Gemfile").exists() {
                        continue;
                    }
                    // CMakeFiles — validate with CMakeLists.txt
                    if name == "CMakeFiles" && !dir.join("CMakeLists.txt").exists() {
                        continue;
                    }
                    // _build — validate with mix.exs (Elixir)
                    if name == "_build" && !dir.join("mix.exs").exists() {
                        continue;
                    }
                    // deps — validate with mix.exs (Elixir) to avoid false positives
                    if name == "deps" && !dir.join("mix.exs").exists() {
                        continue;
                    }
                    // dist-newstyle — validate with cabal file (Haskell)
                    if name == "dist-newstyle" {
                        let has_cabal = fs::read_dir(&dir)
                            .map(|entries| {
                                entries.filter_map(|e| e.ok()).any(|e| {
                                    let n = e.file_name();
                                    let n = n.to_string_lossy();
                                    n.ends_with(".cabal")
                                })
                            })
                            .unwrap_or(false);
                        if !has_cabal && !dir.join("cabal.project").exists() {
                            continue;
                        }
                    }
                    // .stack-work — validate with stack.yaml (Haskell)
                    if name == ".stack-work" && !dir.join("stack.yaml").exists() {
                        continue;
                    }
                    // _opam — validate with dune-project or *.opam (OCaml)
                    if name == "_opam" {
                        let has_ocaml = dir.join("dune-project").exists()
                            || fs::read_dir(&dir)
                                .map(|entries| {
                                    entries.filter_map(|e| e.ok()).any(|e| {
                                        e.file_name().to_string_lossy().ends_with(".opam")
                                    })
                                })
                                .unwrap_or(false);
                        if !has_ocaml {
                            continue;
                        }
                    }
                    // .bun — validate with package.json or bun.lockb
                    if name == ".bun" && !dir.join("package.json").exists() && !dir.join("bun.lockb").exists() {
                        continue;
                    }
                    // DerivedData — validate with .xcodeproj or .xcworkspace
                    if name == "DerivedData" {
                        let has_xcode = fs::read_dir(&dir)
                            .map(|entries| {
                                entries.filter_map(|e| e.ok()).any(|e| {
                                    let n = e.file_name();
                                    let n = n.to_string_lossy();
                                    n.ends_with(".xcodeproj")
                                        || n.ends_with(".xcworkspace")
                                })
                            })
                            .unwrap_or(false);
                        if !has_xcode {
                            continue;
                        }
                    }

                    let size = dir_size(&path);

                    results.push(ArtifactEntry {
                        project_name,
                        project_path,
                        artifact_type: artifact_type.to_string(),
                        artifact_path: path.to_string_lossy().to_string(),
                        size,
                    });

                    is_artifact = true;

                    if results.len() % 50 == 0 {
                        on_progress(&ScanProgress {
                            current_path: dir.to_string_lossy().to_string(),
                            artifacts_found: results.len(),
                        });
                    }

                    break;
                }
            }

            // Check suffix-based patterns (e.g., *.egg-info)
            if !is_artifact {
                for (suffix, artifact_type) in ARTIFACT_SUFFIXES {
                    if name.ends_with(suffix) {
                        let project_path = dir.to_string_lossy().to_string();
                        let project_name = dir
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let size = dir_size(&path);

                        results.push(ArtifactEntry {
                            project_name,
                            project_path,
                            artifact_type: artifact_type.to_string(),
                            artifact_path: path.to_string_lossy().to_string(),
                            size,
                        });

                        is_artifact = true;
                        break;
                    }
                }
            }

            // Only descend if this was NOT an artifact directory
            if !is_artifact {
                stack.push(path);
            }
        }
    }

    // Final progress emit
    on_progress(&ScanProgress {
        current_path: root.to_string(),
        artifacts_found: results.len(),
    });

    // Sort by size descending
    results.sort_by(|a, b| b.size.cmp(&a.size));

    results
}
