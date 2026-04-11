use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Roots where Homebrew stores cask payloads on macOS.
const CASKROOMS: &[&str] = &["/opt/homebrew/Caskroom", "/usr/local/Caskroom"];

/// Validates that a candidate cask token looks like a real Homebrew cask
/// name: starts with a lowercase alphanumeric and contains only lowercase
/// alphanumerics, hyphens, and at symbols. This prevents an attacker-
/// controlled Caskroom path (unlikely but worth defending against) from
/// injecting arbitrary tokens into a later `brew uninstall` call.
fn is_valid_cask_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let mut chars = token.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '@')
}

/// Extracts the cask token from a path that lives inside a Caskroom.
/// Given `/opt/homebrew/Caskroom/<token>/<version>/Foo.app`, returns
/// `<token>` if it looks valid; otherwise returns `None`.
fn extract_cask_token(path: &Path) -> Option<String> {
    let path_str = path.to_string_lossy();
    for room in CASKROOMS {
        let prefix = format!("{}/", room);
        if let Some(rest) = path_str.strip_prefix(prefix.as_str()) {
            let token = rest.split('/').next().unwrap_or("");
            if is_valid_cask_token(token) {
                return Some(token.to_string());
            }
            return None;
        }
    }
    None
}

/// Stage 1: Resolve every symlink on `app_path` and check whether the
/// resulting real path lives under a Caskroom. This is fast and entirely
/// deterministic — no false positives, no shell invocations.
fn detect_via_canonical(app_path: &Path) -> Option<String> {
    let resolved = fs::canonicalize(app_path).ok()?;
    extract_cask_token(&resolved)
}

/// Stage 2: Look up the app bundle name inside each Caskroom directly.
/// Walks `/opt/homebrew/Caskroom/*/*/` (cask token → version) and checks
/// whether any of them contains a bundle matching `bundle_name`. Only
/// succeeds when exactly one cask token matches, to avoid uninstalling
/// the wrong package if two casks share the same bundle name.
fn detect_via_caskroom_search(bundle_name: &str) -> Option<String> {
    if bundle_name.is_empty() {
        return None;
    }

    let mut tokens: Vec<String> = Vec::new();
    for room in CASKROOMS {
        let room_path = Path::new(room);
        if !room_path.is_dir() {
            continue;
        }
        let cask_dirs = match fs::read_dir(room_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for cask_entry in cask_dirs.filter_map(|e| e.ok()) {
            let cask_dir = cask_entry.path();
            if !cask_dir.is_dir() {
                continue;
            }
            let token = match cask_dir.file_name().and_then(|n| n.to_str()) {
                Some(t) if is_valid_cask_token(t) => t.to_string(),
                _ => continue,
            };
            let version_dirs = match fs::read_dir(&cask_dir) {
                Ok(d) => d,
                Err(_) => continue,
            };
            for version_entry in version_dirs.filter_map(|e| e.ok()) {
                let version_dir = version_entry.path();
                if !version_dir.is_dir() {
                    continue;
                }
                if version_dir.join(bundle_name).exists() && !tokens.contains(&token) {
                    tokens.push(token.clone());
                }
            }
        }
    }

    if tokens.len() == 1 {
        tokens.pop()
    } else {
        None
    }
}

/// Stage 3: If `app_path` is itself a symlink, read its immediate target
/// (without recursing further) and check whether that target is inside a
/// Caskroom. Catches dangling symlinks where the Caskroom payload has
/// already been partially removed but the /Applications entry remains.
fn detect_via_symlink_target(app_path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(app_path).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let target = fs::read_link(app_path).ok()?;
    let absolute: PathBuf = if target.is_absolute() {
        target
    } else {
        app_path.parent()?.join(target)
    };
    extract_cask_token(&absolute)
}

/// Returns true if Homebrew is installed and usable.
pub fn is_homebrew_available() -> bool {
    Path::new("/opt/homebrew/bin/brew").exists() || Path::new("/usr/local/bin/brew").exists()
}

/// Looks up the Homebrew cask token that manages `app_path`, or `None`
/// if the app was not installed via Homebrew. Runs three detection
/// stages in order from fastest/most-deterministic to slowest:
///
/// 1. Canonicalize the path and check whether it resolves into a Caskroom.
/// 2. Search Caskroom subdirectories for a bundle with the same name.
/// 3. If the path is a symlink, follow one level and recheck.
pub fn detect_cask(app_path: &str) -> Option<String> {
    let path = Path::new(app_path);
    if !path.exists() && fs::symlink_metadata(path).is_err() {
        return None;
    }
    if !is_homebrew_available() {
        return None;
    }

    if let Some(token) = detect_via_canonical(path) {
        return Some(token);
    }

    let bundle_name = path.file_name()?.to_string_lossy().to_string();
    if let Some(token) = detect_via_caskroom_search(&bundle_name) {
        return Some(token);
    }

    if let Some(token) = detect_via_symlink_target(path) {
        return Some(token);
    }

    None
}

/// Returns true if the given cask token is currently recorded as installed
/// by `brew list --cask`. Used as a final sanity check before invoking
/// `brew uninstall` so we never pass a stale or hand-constructed token.
#[allow(dead_code)]
pub fn is_cask_installed(cask: &str) -> bool {
    if !is_valid_cask_token(cask) || !is_homebrew_available() {
        return false;
    }

    let brew = if Path::new("/opt/homebrew/bin/brew").exists() {
        "/opt/homebrew/bin/brew"
    } else {
        "/usr/local/bin/brew"
    };

    let output = match Command::new(brew)
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .args(["list", "--cask"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(|line| line.trim() == cask)
}
