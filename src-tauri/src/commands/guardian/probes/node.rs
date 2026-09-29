use std::path::{Path, PathBuf};

pub(crate) fn paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (home.join(".npm/_cacache"), "npm cache"),
        (home.join("Library/Caches/Yarn"), "Yarn cache"),
        (home.join("Library/pnpm/store"), "pnpm store"),
        (home.join(".bun/install/cache"), "Bun cache"),
    ]
}
