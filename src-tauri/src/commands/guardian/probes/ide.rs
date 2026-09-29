use std::path::{Path, PathBuf};

pub(crate) fn paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (
            home.join("Library/Application Support/Code/Cache"),
            "VS Code cache",
        ),
        (
            home.join("Library/Application Support/Code/CachedData"),
            "VS Code cached data",
        ),
        (
            home.join("Library/Application Support/Code/CachedExtensions"),
            "VS Code cached extensions",
        ),
        (home.join("Library/Caches/JetBrains"), "JetBrains caches"),
        (
            home.join("Library/Application Support/Sublime Text/Cache"),
            "Sublime cache",
        ),
    ]
}
