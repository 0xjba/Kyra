use super::types::ProbeReport;
use crate::commands::data_guard::{self, DeleteContext};
use crate::commands::utils::dir_size;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod ai_ml;
mod apps;
mod docker;
mod homebrew;
mod ide;
mod node;
mod python;
mod rust_lang;
mod system;
mod xcode;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

type PathsFn = fn(&Path) -> Vec<(PathBuf, &'static str)>;

pub(crate) struct CategoryDef {
    pub id: &'static str,
    pub display_name: &'static str,
    pub confidence: f32,
    /// Set when cleaning deletes something the user can't regenerate by just using the app again.
    pub data_loss: Option<&'static str>,
    paths: PathsFn,
}

const fn cat(
    id: &'static str,
    display_name: &'static str,
    confidence: f32,
    paths: PathsFn,
) -> CategoryDef {
    CategoryDef {
        id,
        display_name,
        confidence,
        data_loss: None,
        paths,
    }
}

const fn user_data(
    id: &'static str,
    display_name: &'static str,
    confidence: f32,
    data_loss: &'static str,
    paths: PathsFn,
) -> CategoryDef {
    CategoryDef {
        id,
        display_name,
        confidence,
        data_loss: Some(data_loss),
        paths,
    }
}

pub(crate) const CATEGORIES: &[CategoryDef] = &[
    cat("docker", "Docker", 0.8, docker::cache_paths),
    user_data(
        "docker_vm",
        "Docker Data",
        0.8,
        "Deletes all Docker images, containers and volumes",
        docker::vm_paths,
    ),
    cat("homebrew", "Homebrew", 0.9, homebrew::paths),
    cat("xcode", "Xcode", 0.9, xcode::cache_paths),
    user_data(
        "xcode_archives",
        "Xcode Archives",
        0.9,
        "Deletes Xcode archives and their debug symbols",
        xcode::archive_paths,
    ),
    cat("node", "Node.js", 0.9, node::paths),
    cat("rust", "Rust / Cargo", 0.85, rust_lang::paths),
    cat("python", "Python", 0.9, python::paths),
    cat("ide", "IDE Caches", 0.85, ide::paths),
    user_data(
        "ai_ml",
        "AI & ML Models",
        0.7,
        "Deletes downloaded AI models (Ollama, Hugging Face, LM Studio)",
        ai_ml::model_paths,
    ),
    cat("ai_cache", "CoreML Cache", 0.85, ai_ml::cache_paths),
    cat("apps", "App Caches", 0.85, apps::cache_paths),
    user_data(
        "app_data",
        "App Data",
        0.85,
        "Deletes Spotify offline downloads and Zoom data",
        apps::data_paths,
    ),
    cat("system", "System & Logs", 0.8, system::paths),
    user_data(
        TRASH,
        "Trash",
        0.9,
        "Permanently empties your Trash",
        system::trash_paths,
    ),
];

/// Emptied in place and always permanently: moving the Trash into the Trash would free nothing.
pub(crate) const TRASH: &str = "trash";

pub(crate) fn definition(category: &str) -> Option<&'static CategoryDef> {
    CATEGORIES.iter().find(|d| d.id == category)
}

pub(crate) fn data_loss(category: &str) -> Option<&'static str> {
    definition(category).and_then(|d| d.data_loss)
}

/// Every path a category's probe measures; the clean engine may only delete from this set.
pub(crate) fn paths_for(category: &str, home: &Path) -> Vec<PathBuf> {
    match definition(category) {
        Some(def) => (def.paths)(home).into_iter().map(|(p, _)| p).collect(),
        None => Vec::new(),
    }
}

/// Guard context for cleaning `category`: data-loss categories reach
/// protected data only once the user approved them; everything else, and
/// every autonomous clean, gets the general rules.
pub(crate) fn guard_context(category: &str, approved: bool) -> DeleteContext<'static> {
    if approved && data_loss(category).is_some() {
        DeleteContext::Confirmed
    } else {
        DeleteContext::General
    }
}

/// True if Pawtrol may clean `category` without asking: it deletes nothing
/// the user can't regenerate, and none of its paths reach protected data.
pub(crate) fn autonomous_safe(category: &str) -> bool {
    let Some(def) = definition(category) else {
        return false;
    };
    def.data_loss.is_none()
        && (def.paths)(Path::new("/Users/pawtrol")).iter().all(|(p, _)| {
            matches!(
                data_guard::static_verdict(p),
                data_guard::Verdict::NotProtected | data_guard::Verdict::CacheLeaf
            )
        })
}

/// Bytes the clean engine would free by deleting `path`, or None when it would skip it.
/// Probes and the engine both measure through this so the UI size matches what gets deleted.
pub(crate) fn deletable_size(path: &Path) -> Option<u64> {
    deletable_size_in(path, DeleteContext::General)
}

pub(crate) fn deletable_size_in(path: &Path, ctx: DeleteContext) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        return None;
    }
    if !crate::commands::cleaner::is_safe_path(&path.to_string_lossy()) {
        return None;
    }
    if data_guard::check(path, ctx).is_err() {
        return None;
    }
    Some(if meta.is_dir() {
        dir_size(path)
    } else {
        meta.blocks().saturating_mul(512)
    })
}

pub(crate) fn measure(home: &Path, def: &CategoryDef) -> Option<ProbeReport> {
    let mut total: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let ctx = guard_context(def.id, true);
    for (path, label) in (def.paths)(home) {
        let size = deletable_size_in(&path, ctx).unwrap_or(0);
        if size > 0 {
            total += size;
            items += 1;
            details.push(format!("{} {}", label, format_mb(size)));
        }
    }

    if total == 0 {
        return None;
    }

    Some(ProbeReport {
        category: def.id.into(),
        display_name: def.display_name.into(),
        total_bytes: total,
        cleanable_bytes: total,
        item_count: items,
        last_used_secs: None,
        confidence: def.confidence,
        details: details.join(", "),
        user_data: def.data_loss.is_some(),
        data_loss: def.data_loss.map(String::from),
    })
}

fn probe_blocking(home: &Path, def: &CategoryDef) -> Option<ProbeReport> {
    let mut report = measure(home, def)?;
    if def.id == "homebrew" {
        let outdated = homebrew::outdated_count();
        if outdated > 0 {
            report.item_count += outdated;
            report.details = format!("{}, {} outdated packages", report.details, outdated);
        }
    }
    Some(report)
}

pub async fn run_all_probes() -> Vec<ProbeReport> {
    match dirs::home_dir() {
        Some(home) => run_all_probes_in(home).await,
        None => Vec::new(),
    }
}

pub(crate) async fn run_all_probes_in(home: PathBuf) -> Vec<ProbeReport> {
    let handles: Vec<_> = CATEGORIES
        .iter()
        .map(|def| {
            let home = home.clone();
            let task = tokio::task::spawn_blocking(move || probe_blocking(&home, def));
            tokio::spawn(tokio::time::timeout(PROBE_TIMEOUT, task))
        })
        .collect();

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(Ok(Ok(Some(report)))) = handle.await {
            results.push(report);
        }
    }
    results.sort_by(|a, b| b.cleanable_bytes.cmp(&a.cleanable_bytes));
    results
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::guardian::test_support::{block_on, TestDir};

    fn def(id: &str) -> &'static CategoryDef {
        definition(id).unwrap()
    }

    #[test]
    fn empty_home_yields_no_reports() {
        let home = TestDir::new("probes-empty");
        assert!(block_on(run_all_probes_in(home.path().to_path_buf())).is_empty());
    }

    #[test]
    fn category_ids_are_unique() {
        let mut ids: Vec<&str> = CATEGORIES.iter().map(|d| d.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATEGORIES.len());
    }

    #[test]
    fn every_category_has_paths_inside_home() {
        let home = Path::new("/Users/test");
        for d in CATEGORIES {
            let paths = paths_for(d.id, home);
            assert!(!paths.is_empty(), "{} has no paths", d.id);
            for p in paths {
                assert!(
                    p.starts_with(home) && p != home,
                    "{}: {}",
                    d.id,
                    p.display()
                );
                assert!(!p
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)));
            }
        }
        assert!(paths_for("unknown", home).is_empty());
        assert!(paths_for("", home).is_empty());
    }

    #[test]
    fn no_path_is_shared_or_nested_across_categories() {
        let home = Path::new("/Users/test");
        let all: Vec<(&str, PathBuf)> = CATEGORIES
            .iter()
            .flat_map(|d| paths_for(d.id, home).into_iter().map(move |p| (d.id, p)))
            .collect();
        for (i, (ca, a)) in all.iter().enumerate() {
            for (cb, b) in &all[i + 1..] {
                assert!(
                    !a.starts_with(b) && !b.starts_with(a),
                    "{ca}:{} overlaps {cb}:{}",
                    a.display(),
                    b.display()
                );
            }
        }
    }

    #[test]
    fn user_data_categories_are_flagged_with_their_loss() {
        let flagged: Vec<(&str, &str)> = CATEGORIES
            .iter()
            .filter_map(|d| d.data_loss.map(|l| (d.id, l)))
            .collect();
        assert_eq!(
            flagged,
            vec![
                (
                    "docker_vm",
                    "Deletes all Docker images, containers and volumes"
                ),
                (
                    "xcode_archives",
                    "Deletes Xcode archives and their debug symbols"
                ),
                (
                    "ai_ml",
                    "Deletes downloaded AI models (Ollama, Hugging Face, LM Studio)"
                ),
                (
                    "app_data",
                    "Deletes Spotify offline downloads and Zoom data"
                ),
                ("trash", "Permanently empties your Trash"),
            ]
        );
        assert_eq!(data_loss("docker_vm"), def("docker_vm").data_loss);
        assert_eq!(data_loss("docker"), None);
        assert_eq!(data_loss("nope"), None);
    }

    #[test]
    fn user_data_paths_are_not_in_cache_categories() {
        let home = Path::new("/Users/test");
        let rel = |cat: &str| -> Vec<String> {
            paths_for(cat, home)
                .iter()
                .map(|p| p.strip_prefix(home).unwrap().display().to_string())
                .collect()
        };
        assert_eq!(
            rel("docker_vm"),
            ["Library/Containers/com.docker.docker/Data/vms"]
        );
        assert_eq!(rel("xcode_archives"), ["Library/Developer/Xcode/Archives"]);
        assert!(rel("xcode").iter().all(|p| !p.contains("Archives")));
        assert!(rel("ai_ml").contains(&".ollama/models".to_string()));
        assert!(rel("ai_ml").contains(&".cache/huggingface".to_string()));
        assert!(rel("ai_cache").iter().all(|p| p.contains("Caches")));
        assert!(rel("apps")
            .iter()
            .all(|p| !p.contains("Spotify/PersistentCache") && !p.contains("zoom.us")));
        assert_eq!(rel("app_data").len(), 2);
        assert_eq!(rel("trash"), [".Trash"]);
        assert!(rel("system").iter().all(|p| !p.contains("Trash")));
    }

    #[test]
    fn reports_are_built_per_category_and_sorted_by_size() {
        let home = TestDir::new("probes-build");
        home.write(".npm/_cacache/a", 4096);
        home.write("Library/Caches/Yarn/b", 4096);
        home.write(".cargo/registry/cache/c", 64 * 1024);
        home.write(".cache/huggingface/m", 16 * 1024);
        home.write("Library/Logs/x.log", 12 * 1024);
        // Present but empty: must not produce a report or an item.
        std::fs::create_dir_all(home.path().join("Library/Caches/pip")).unwrap();

        let reports = block_on(run_all_probes_in(home.path().to_path_buf()));
        let cats: Vec<&str> = reports.iter().map(|r| r.category.as_str()).collect();
        assert_eq!(cats, vec!["rust", "ai_ml", "system", "node"]);

        let node = reports.iter().find(|r| r.category == "node").unwrap();
        let expected = dir_size(&home.path().join(".npm/_cacache"))
            + dir_size(&home.path().join("Library/Caches/Yarn"));
        assert_eq!(node.cleanable_bytes, expected);
        assert_eq!(node.total_bytes, expected);
        assert_eq!(node.item_count, 2);
        assert_eq!(node.display_name, "Node.js");
        assert!(node.details.contains("npm cache") && node.details.contains("Yarn cache"));
        assert!(node.last_used_secs.is_none());
        assert!(!node.user_data && node.data_loss.is_none());

        let ai = reports.iter().find(|r| r.category == "ai_ml").unwrap();
        assert!(ai.user_data);
        assert_eq!(ai.data_loss.as_deref(), def("ai_ml").data_loss);

        let rust = &reports[0];
        assert_eq!(rust.display_name, "Rust / Cargo");
        assert_eq!(rust.item_count, 1);
        for r in &reports {
            assert!(r.cleanable_bytes > 0);
            assert!(r.confidence > 0.0 && r.confidence <= 1.0);
        }
    }

    #[test]
    fn every_category_reports_under_its_id() {
        let home = TestDir::new("probes-names");
        for d in CATEGORIES {
            let first = &paths_for(d.id, home.path())[0];
            let rel = first.strip_prefix(home.path()).unwrap();
            home.write(&format!("{}/f", rel.display()), 4096);
        }
        let reports = block_on(run_all_probes_in(home.path().to_path_buf()));
        let mut got: Vec<&str> = reports.iter().map(|r| r.category.as_str()).collect();
        got.sort();
        let mut want: Vec<&str> = CATEGORIES.iter().map(|d| d.id).collect();
        want.sort();
        assert_eq!(got, want);
        for r in &reports {
            assert_eq!(
                r.user_data,
                data_loss(&r.category).is_some(),
                "{}",
                r.category
            );
        }
    }

    #[test]
    fn docker_vm_is_reported_in_full_and_separately_from_build_cache() {
        let home = TestDir::new("probes-docker");
        home.write(".docker/buildx/cache/layer", 8192);
        home.write(
            "Library/Containers/com.docker.docker/Data/vms/0/disk.raw",
            64 * 1024,
        );

        let cache = measure(home.path(), def("docker")).unwrap();
        assert_eq!(
            cache.cleanable_bytes,
            dir_size(&home.path().join(".docker/buildx/cache"))
        );
        assert!(!cache.user_data);

        let vm = measure(home.path(), def("docker_vm")).unwrap();
        assert_eq!(
            vm.cleanable_bytes,
            dir_size(
                &home
                    .path()
                    .join("Library/Containers/com.docker.docker/Data/vms")
            )
        );
        assert_eq!(vm.display_name, "Docker Data");
        assert!(vm.user_data);
        assert!(vm.details.starts_with("VM disk"));
    }

    #[test]
    fn docker_absent_is_none() {
        let home = TestDir::new("probes-nodocker");
        assert!(measure(home.path(), def("docker")).is_none());
        assert!(measure(home.path(), def("docker_vm")).is_none());
    }

    #[test]
    fn symlinked_root_is_not_measured() {
        let home = TestDir::new("probes-symlink");
        home.write("Documents/keep/data.bin", 4096);
        std::fs::create_dir_all(home.path().join(".cargo/registry")).unwrap();
        std::os::unix::fs::symlink(
            home.path().join("Documents/keep"),
            home.path().join(".cargo/registry/cache"),
        )
        .unwrap();
        assert!(measure(home.path(), def("rust")).is_none());
        assert_eq!(
            deletable_size(&home.path().join(".cargo/registry/cache")),
            None
        );
    }
}
