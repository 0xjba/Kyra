use std::path::{Path, PathBuf};

pub(crate) fn cache_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(home.join(".docker/buildx/cache"), "build cache")]
}

pub(crate) fn vm_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(
        home.join("Library/Containers/com.docker.docker/Data/vms"),
        "VM disk",
    )]
}
