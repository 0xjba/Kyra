use std::path::{Path, PathBuf};

pub(crate) fn paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (home.join(".cargo/registry/cache"), "registry cache"),
        (home.join(".cargo/registry/src"), "registry source"),
        (home.join(".rustup/tmp"), "rustup tmp"),
    ]
}
