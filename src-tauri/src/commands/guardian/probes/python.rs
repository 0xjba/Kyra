use std::path::{Path, PathBuf};

pub(crate) fn paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (home.join("Library/Caches/pip"), "pip cache (Library)"),
        (home.join(".cache/pip"), "pip cache (dotcache)"),
        (home.join(".conda/pkgs"), "conda packages"),
        (home.join(".cache/conda"), "conda cache"),
    ]
}
