use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::DirNode;

/// Disk cache entry wrapping a DirNode with metadata for freshness checks.
#[derive(Serialize, Deserialize)]
struct DiskCacheEntry {
    /// The cached scan result.
    node: DirNode,
    /// Modification time of the scanned directory when the cache was written.
    dir_mod_time_secs: u64,
    /// Timestamp when the scan was performed (seconds since UNIX epoch).
    scan_time_secs: u64,
}

/// Maximum age before a cache entry is considered expired.
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60); // 7 days

/// Maximum age for stale cache — used for fast first-paint before background refresh.
const STALE_CACHE_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60); // 3 days

/// Grace window: if the directory mod-time changed by less than this, still
/// accept the cache (macOS Finder touches directory mtimes frequently).
const MOD_TIME_GRACE: Duration = Duration::from_secs(30 * 60); // 30 minutes

/// If the cache is younger than this window, reuse it even when the directory
/// mod-time has drifted beyond the grace window. This prevents frequent full
/// rescans on volatile directories while still forcing a refresh once the
/// cache is old enough.
const REUSE_WINDOW: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// Maximum number of cache files on disk.
const MAX_DISK_ENTRIES: usize = 50;

/// Return the cache directory, creating it if necessary.
fn cache_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let dir = home.join("Library/Caches/com.kyra.app/analyzer");
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create cache dir: {}", e))?;
    Ok(dir)
}

/// Deterministic cache filename derived from the path+depth key.
fn cache_filename(path: &str, depth: usize) -> String {
    // Simple hash: use std's default hasher (SipHash) via a manual implementation
    // to avoid pulling in an external crate.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    depth.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{:016x}.json", hash)
}

fn cache_path(path: &str, depth: usize) -> Result<PathBuf, String> {
    Ok(cache_dir()?.join(cache_filename(path, depth)))
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn _epoch_secs_to_system_time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// Try to load a fresh cache entry from disk.
/// Returns `None` if the cache is missing, expired, or the directory has been
/// modified since the cache was written (outside the grace window).
pub fn load_from_disk(path: &str, depth: usize) -> Option<DirNode> {
    let file_path = cache_path(path, depth).ok()?;
    let data = fs::read(&file_path).ok()?;
    let entry: DiskCacheEntry = serde_json::from_slice(&data).ok()?;

    let now = now_epoch_secs();

    // 1. TTL check — reject entries older than 7 days.
    let scan_age_secs = now.saturating_sub(entry.scan_time_secs);
    if scan_age_secs > CACHE_TTL.as_secs() {
        let _ = fs::remove_file(&file_path);
        return None;
    }

    // 2. Mod-time freshness — compare the directory's current mtime against
    //    the mtime recorded when the cache was written.
    let dir_meta = fs::metadata(path).ok()?;
    let current_mtime = dir_meta
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if current_mtime > entry.dir_mod_time_secs {
        let drift = current_mtime - entry.dir_mod_time_secs;
        // Within the grace window — accept.
        if drift <= MOD_TIME_GRACE.as_secs() {
            return Some(entry.node);
        }
        // Outside grace, but scan is recent enough to reuse.
        if scan_age_secs <= REUSE_WINDOW.as_secs() {
            return Some(entry.node);
        }
        // Truly stale — reject.
        let _ = fs::remove_file(&file_path);
        return None;
    }

    Some(entry.node)
}

/// Load a stale cache entry from disk, ignoring freshness checks but enforcing
/// a maximum stale TTL (3 days). Used for fast first-paint before triggering
/// a background refresh.
pub fn load_stale(path: &str, depth: usize) -> Option<DirNode> {
    let file_path = cache_path(path, depth).ok()?;
    let data = fs::read(&file_path).ok()?;
    let entry: DiskCacheEntry = serde_json::from_slice(&data).ok()?;

    // Verify the directory still exists.
    if fs::metadata(path).is_err() {
        return None;
    }

    // Enforce a maximum stale age.
    let now = now_epoch_secs();
    let scan_age_secs = now.saturating_sub(entry.scan_time_secs);
    if scan_age_secs > STALE_CACHE_TTL.as_secs() {
        return None;
    }

    Some(entry.node)
}

/// Peek at the cached total file count for a path, ignoring expiration.
/// Used for initial scan progress estimates.
pub fn peek_total_files(path: &str, depth: usize) -> Option<usize> {
    let file_path = cache_path(path, depth).ok()?;
    let data = fs::read(&file_path).ok()?;
    let entry: DiskCacheEntry = serde_json::from_slice(&data).ok()?;
    // Count files in the cached tree.
    fn count_files(node: &DirNode) -> usize {
        if !node.is_dir {
            return 1;
        }
        node.children.iter().map(|c| count_files(c)).sum::<usize>()
    }
    let count = count_files(&entry.node);
    if count > 0 { Some(count) } else { None }
}

/// Persist a scan result to the disk cache.
pub fn save_to_disk(path: &str, depth: usize, node: &DirNode) {
    let file_path = match cache_path(path, depth) {
        Ok(p) => p,
        Err(_) => return,
    };

    let dir_mod_time_secs = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let entry = DiskCacheEntry {
        node: node.clone(),
        dir_mod_time_secs,
        scan_time_secs: now_epoch_secs(),
    };

    // Atomic write: write to a tmp file, then rename.
    let tmp_path = file_path.with_extension("tmp");
    let data = match serde_json::to_vec(&entry) {
        Ok(d) => d,
        Err(_) => return,
    };
    if fs::write(&tmp_path, &data).is_ok() {
        let _ = fs::rename(&tmp_path, &file_path);
    }

    // Enforce max entries — evict oldest files if needed.
    evict_if_needed();
}

/// Remove disk cache entries whose path prefix matches the given path.
/// Called after a deletion so that stale data is not served.
pub fn invalidate_path(deleted_path: &str) {
    let dir = match cache_dir() {
        Ok(d) => d,
        Err(_) => return,
    };

    // We cannot efficiently map cache filenames back to original paths without
    // reading them, so we scan all cache files and remove any whose cached
    // node path starts with or equals the deleted path.
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(data) = fs::read(&p) {
            if let Ok(cached) = serde_json::from_slice::<DiskCacheEntry>(&data) {
                // Invalidate if the deleted path is a prefix of (or equal to)
                // the cached scan root, or vice versa (the deleted item was
                // inside the cached tree).
                if cached.node.path == deleted_path
                    || cached.node.path.starts_with(&format!("{}/", deleted_path))
                    || deleted_path.starts_with(&format!("{}/", cached.node.path))
                {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
}

/// Evict oldest cache files when the count exceeds MAX_DISK_ENTRIES.
fn evict_if_needed() {
    let dir = match cache_dir() {
        Ok(d) => d,
        Err(_) => return,
    };

    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(meta) = p.metadata() {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((p, mtime));
            }
        }
    }

    if files.len() <= MAX_DISK_ENTRIES {
        return;
    }

    // Sort oldest first.
    files.sort_by_key(|(_, t)| *t);
    let to_remove = files.len() - MAX_DISK_ENTRIES;
    for (path, _) in files.into_iter().take(to_remove) {
        let _ = fs::remove_file(path);
    }
}
