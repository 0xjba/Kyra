use super::CleanRule;

/// Returns all cleaning rule definitions.
/// Each rule specifies paths to scan — `~` is expanded to the user's home directory at scan time.
pub fn all_rules() -> Vec<CleanRule> {
    vec![]
}
