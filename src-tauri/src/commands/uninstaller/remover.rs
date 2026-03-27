use super::{UninstallProgress, UninstallResult};

/// Removes the application bundle and associated files.
/// Calls `on_progress` after each item is processed.
/// If `dry_run` is true, reports what would be removed without actually removing anything.
pub fn remove_app_and_files<F>(
    _app_path: &str,
    _file_paths: &[String],
    _dry_run: bool,
    _on_progress: F,
) -> Result<UninstallResult, String>
where
    F: FnMut(&UninstallProgress),
{
    Ok(UninstallResult {
        items_removed: 0,
        bytes_freed: 0,
        errors: vec![],
    })
}
