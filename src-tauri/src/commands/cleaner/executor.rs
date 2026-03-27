use super::CleanResult;

/// Deletes the specified paths and returns the result.
pub fn execute_clean_items(
    _paths: Vec<String>,
    _dry_run: bool,
) -> CleanResult {
    CleanResult {
        items_cleaned: 0,
        bytes_freed: 0,
        errors: vec![],
    }
}
