use std::path::{Path, PathBuf};

pub(crate) fn cache_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (
            home.join("Library/Developer/Xcode/DerivedData"),
            "DerivedData",
        ),
        (
            home.join("Library/Developer/CoreSimulator/Caches"),
            "Simulator caches",
        ),
    ]
}

pub(crate) fn archive_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(home.join("Library/Developer/Xcode/Archives"), "Archives")]
}
