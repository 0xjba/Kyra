pub mod associated;
pub mod brew;
pub mod discovery;
pub mod remover;

use serde::Serialize;
use std::path::{Component, Path};

/// Home dot-entries shared by many tools or holding credentials/shell
/// config. Never offered or deleted by the uninstaller, whatever the app is
/// called. Compared case-insensitively (APFS default).
const SHARED_HOME_DOT_ENTRIES: &[&str] = &[
    ".ssh", ".gnupg", ".config", ".local", ".cache", ".aws", ".azure", ".kube",
    ".docker", ".npm", ".npmrc", ".yarn", ".yarnrc", ".pnpm-store", ".cargo",
    ".rustup", ".gem", ".m2", ".gradle", ".ivy2", ".sbt", ".pyenv", ".nvm",
    ".rbenv", ".bun", ".deno", ".conda", ".gcloud", ".terraform.d", ".git",
    ".gitconfig", ".gitignore_global", ".git-credentials", ".netrc",
    ".zshrc", ".zshenv", ".zprofile", ".zlogin", ".zlogout", ".zsh_history",
    ".zsh_sessions", ".oh-my-zsh", ".bashrc", ".bash_profile", ".bash_history",
    ".bash_sessions", ".profile", ".inputrc", ".vim", ".vimrc", ".viminfo",
    ".tmux.conf", ".Trash", ".vscode", ".CFUserTextEncoding", ".DS_Store",
];

/// Credential stores: nothing inside them is ever an app leftover.
const CREDENTIAL_HOME_DOT_DIRS: &[&str] = &[".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker"];

/// Shared roots nested inside allowed dot dirs, as home-relative paths.
const SHARED_HOME_DOT_SUBPATHS: &[&[&str]] = &[
    &[".local", "share"],
    &[".local", "state"],
    &[".local", "bin"],
    &[".local", "lib"],
];

/// True if `path` is a shared/critical dot-entry directly under `home`
/// (see `SHARED_HOME_DOT_ENTRIES`), a dot-name too generic to belong to one
/// app (fewer than 3 chars after the dot), one of the shared XDG roots, or
/// anything inside a credential store.
pub(crate) fn is_shared_home_dot_path(path: &Path, home: &Path) -> bool {
    let norm = |p: &Path| -> Option<Vec<String>> {
        let mut out = Vec::new();
        for c in p.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => return None,
                c => out.push(c.as_os_str().to_string_lossy().to_lowercase()),
            }
        }
        Some(out)
    };
    let (Some(path), Some(home)) = (norm(path), norm(home)) else {
        return true;
    };
    if path.len() <= home.len() || path[..home.len()] != home[..] {
        return false;
    }
    let parts = &path[home.len()..];
    if CREDENTIAL_HOME_DOT_DIRS.iter().any(|d| d.eq_ignore_ascii_case(&parts[0])) {
        return true;
    }
    if parts.len() == 1 {
        let name = &parts[0];
        return name.starts_with('.')
            && (name.chars().count() < 4
                || SHARED_HOME_DOT_ENTRIES.iter().any(|e| e.eq_ignore_ascii_case(name)));
    }
    SHARED_HOME_DOT_SUBPATHS
        .iter()
        .any(|sub| sub.len() == parts.len() && sub.iter().zip(parts).all(|(a, b)| a == b))
}

/// Basic info about an installed application.
#[derive(Clone, Serialize)]
pub struct AppInfo {
    pub bundle_id: String,
    pub name: String,
    pub version: String,
    pub path: String,
    pub size: u64,
    pub is_system: bool,
    pub is_data_sensitive: bool,
    /// Homebrew cask token if the app was installed via `brew install --cask`,
    /// otherwise `None`. Used to drive brew-aware uninstall.
    pub brew_cask: Option<String>,
    /// True if the app declares LSBackgroundOnly or LSUIElement in its
    /// Info.plist — these are helper/agent apps that have no visible UI.
    pub is_background_only: bool,
    /// Spotlight's kMDItemLastUsedDate as Unix seconds; `None` when unknown.
    pub last_used_secs: Option<u64>,
}

/// A file or directory associated with an application.
#[derive(Clone, Serialize)]
pub struct AssociatedFile {
    pub path: String,
    pub category: String,
    pub size: u64,
    pub is_dir: bool,
}

/// Progress event emitted during uninstallation.
#[derive(Clone, Serialize)]
pub struct UninstallProgress {
    pub current_item: String,
    pub items_done: usize,
    pub items_total: usize,
    pub bytes_freed: u64,
}

/// Data the uninstaller left in place because it outlives the app
/// (wallets, message history, VMs, backups, saves, cloud folders, ...).
#[derive(Clone, Debug, Serialize)]
pub struct KeptItem {
    pub path: String,
    pub category: crate::commands::data_guard::Category,
    pub label: String,
    pub size: u64,
    pub message: String,
}

impl KeptItem {
    pub fn from_refusal(r: &crate::commands::data_guard::Refusal) -> Self {
        let shown = match dirs::home_dir() {
            Some(home) => match Path::new(&r.path).strip_prefix(&home) {
                Ok(rel) => format!("~/{}", rel.display()),
                Err(_) => r.path.clone(),
            },
            None => r.path.clone(),
        };
        let label = r.category.label().to_string();
        KeptItem {
            size: crate::commands::utils::path_size(Path::new(&r.path)),
            message: format!("Kept your {label} at {shown} — delete it manually if you're sure."),
            path: r.path.clone(),
            category: r.category,
            label,
        }
    }
}

/// Final result of an uninstall operation.
#[derive(Clone, Serialize)]
pub struct UninstallResult {
    pub items_removed: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
    pub deleted_paths: Vec<String>,
    /// Protected data belonging to the app that was deliberately not deleted.
    pub kept: Vec<KeptItem>,
}

use tauri::Emitter;

#[tauri::command]
pub async fn scan_installed_apps() -> Vec<AppInfo> {
    discovery::scan_apps()
}

#[tauri::command]
pub fn get_associated_files(
    bundle_id: String,
    app_name: String,
    app_path: String,
) -> Vec<AssociatedFile> {
    associated::find_associated(&bundle_id, &app_name, &app_path)
}

#[tauri::command]
pub async fn execute_uninstall(
    app: tauri::AppHandle,
    app_path: String,
    file_paths: Vec<String>,
    bundle_id: String,
    brew_cask: Option<String>,
    dry_run: bool,
    permanent: bool,
) -> Result<UninstallResult, String> {
    let mut result = remover::remove_app_and_files(
        &app_path,
        &file_paths,
        &bundle_id,
        brew_cask,
        dry_run,
        permanent,
        |progress| {
            let _ = app.emit("uninstall-progress", progress);
        },
    );

    // Discovery never offers protected data, so report what it held back.
    if let Some(home) = dirs::home_dir() {
        let name = Path::new(&app_path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        for k in associated::discover_in(&bundle_id, &name, &app_path, &home, false).kept {
            if !result.kept.iter().any(|x| x.path == k.path) {
                result.kept.push(k);
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_and_generic_home_dot_entries_are_denied() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        for name in [
            ".ssh", ".gnupg", ".config", ".local", ".cache", ".aws", ".kube", ".docker",
            ".npm", ".cargo", ".rustup", ".gitconfig", ".zshrc", ".bashrc", ".bash_profile",
            ".profile", ".zprofile", ".Trash", ".vscode", ".git", ".SSH", ".Config",
            ".go", ".js", ".x",
        ] {
            assert!(is_shared_home_dot_path(&h.join(name), h), "{name}");
        }
        for rel in [".ssh/id_ed25519", ".aws/credentials", ".kube/config", ".local/share", ".local/bin", "./.config", ".config/"] {
            assert!(is_shared_home_dot_path(&h.join(rel), h), "{rel}");
        }
        assert!(is_shared_home_dot_path(&h.join("x/../.ssh"), h));
    }

    #[test]
    fn app_specific_home_dot_entries_stay_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        for rel in [
            ".foobar", ".foobarrc", ".zed", ".config/foo-bar", ".local/share/foo-bar",
            ".vscode/extensions", "Library/Caches/com.example.foo", "foo",
        ] {
            assert!(!is_shared_home_dot_path(&h.join(rel), h), "{rel}");
        }
        assert!(!is_shared_home_dot_path(Path::new("/tmp/.ssh"), h));
        assert!(!is_shared_home_dot_path(h, h));
    }
}
