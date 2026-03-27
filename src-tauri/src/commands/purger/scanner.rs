use std::fs;
use std::path::Path;

use super::{ArtifactEntry, ScanProgress};

/// Known build artifact directory names and their human-readable type labels.
const ARTIFACT_DIRS: &[(&str, &str)] = &[
    ("node_modules", "Node.js"),
    ("target", "Rust"),
    ("dist", "Build output"),
    ("build", "Build output"),
    (".next", "Next.js"),
    (".nuxt", "Nuxt.js"),
    ("__pycache__", "Python"),
    (".pytest_cache", "Python"),
    ("Pods", "CocoaPods"),
    (".gradle", "Gradle"),
    (".build", "Swift"),
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
    let root_path = Path::new(root);
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

            // Skip hidden directories (except our known ones starting with .)
            if name.starts_with('.')
                && !matches!(
                    name.as_str(),
                    ".next" | ".nuxt" | ".pytest_cache" | ".gradle" | ".build"
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

                    // Validate: for "target", only match if it looks like a Rust project
                    if name == "target" && !dir.join("Cargo.toml").exists() {
                        continue;
                    }
                    if name == "build"
                        && !dir.join("package.json").exists()
                        && !dir.join("build.gradle").exists()
                        && !dir.join("build.gradle.kts").exists()
                    {
                        continue;
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

/// Recursively calculates directory size.
fn dir_size(path: &Path) -> u64 {
    if path.is_symlink() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_symlink() {
                0
            } else if p.is_dir() {
                dir_size(&p)
            } else {
                p.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}
