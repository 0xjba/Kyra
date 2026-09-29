use std::path::{Path, PathBuf};

pub(crate) fn paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    // Mail Downloads stays with the cleaner's age-gated rule: attachments
    // edited in place live there, so Pawtrol never clears it on its own.
    vec![(home.join("Library/Logs"), "User logs")]
}

pub(crate) fn trash_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(home.join(".Trash"), "Trash")]
}
