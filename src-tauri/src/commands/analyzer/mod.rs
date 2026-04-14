pub mod cache;
pub mod scanner;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

/// L1 in-memory scan cache — avoids re-scanning within 5 minutes.
static SCAN_CACHE: std::sync::LazyLock<Mutex<HashMap<String, (Instant, DirNode)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Overview size cache — stores measured sizes for overview entries.
/// TTL: 7 days (matching reference), keyed by absolute path.
static OVERVIEW_SIZE_CACHE: std::sync::LazyLock<Mutex<HashMap<String, (Instant, u64)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Singleflight dedup: tracks in-flight scans so concurrent callers
/// of the same path get the same result instead of spawning duplicate work.
static IN_FLIGHT_SCANS: std::sync::LazyLock<Mutex<HashMap<String, Arc<Mutex<Option<Result<DirNode, String>>>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// How long overview sizes remain valid in memory.
const OVERVIEW_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Timeout for individual `du` processes.
const DU_TIMEOUT: Duration = Duration::from_secs(30);

/// Overview snapshot file path (persistent JSON).
const OVERVIEW_SNAPSHOT_FILE: &str = "overview_sizes.json";

/// An entry in the disk overview — a top-level system folder with its measured size.
#[derive(Clone, Serialize)]
pub struct OverviewEntry {
    /// Display name (e.g. "Home", "Applications", "iOS Backups").
    pub name: String,
    /// Absolute path on disk.
    pub path: String,
    /// Measured size in bytes. 0 if measurement failed.
    pub size: u64,
    /// Whether this is a directory (always true for overview entries).
    pub is_dir: bool,
    /// Category hint for the frontend (e.g. "system", "user", "insight").
    pub category: String,
}

/// Full overview result returned to the frontend.
#[derive(Clone, Serialize)]
pub struct OverviewResult {
    /// The overview entries sorted by size descending.
    pub entries: Vec<OverviewEntry>,
    /// Sum of all successfully measured entry sizes.
    pub total_size: u64,
    /// Free disk space on the boot volume (bytes).
    pub disk_free: u64,
    /// Total disk capacity of the boot volume (bytes).
    pub disk_total: u64,
}

/// A node in the directory size tree.
#[derive(Clone, Serialize, Deserialize)]
pub struct DirNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_cleanable: bool,
    pub children: Vec<DirNode>,
    /// Last access time as seconds since UNIX epoch, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_access: Option<u64>,
}

/// Progress event emitted during scan.
#[derive(Clone, Serialize)]
pub struct ScanProgress {
    pub current_path: String,
    pub files_scanned: usize,
    pub dirs_scanned: usize,
    pub total_size: u64,
    /// Estimated total files (from previous scan cache), if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_total: Option<usize>,
}

/// A large file found by Spotlight search.
#[derive(Clone, Serialize)]
pub struct LargeFile {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// Progress event emitted during batch delete.
#[derive(Clone, Serialize)]
pub struct DeleteProgress {
    pub current: usize,
    pub total: usize,
    pub path: String,
    pub done: bool,
}

#[tauri::command]
pub async fn analyze_path(app: tauri::AppHandle, path: String, depth: usize) -> Result<DirNode, String> {
    let cache_key = format!("{}:{}", path, depth);

    // L1: in-memory cache (5-minute TTL)
    if let Ok(cache) = SCAN_CACHE.lock() {
        if let Some((ts, node)) = cache.get(&cache_key) {
            if ts.elapsed() < std::time::Duration::from_secs(300) {
                return Ok(node.clone());
            }
        }
    }

    // L2: persistent disk cache (7-day TTL with mod-time freshness)
    if let Some(node) = cache::load_from_disk(&path, depth) {
        // Promote to L1
        if let Ok(mut cache) = SCAN_CACHE.lock() {
            cache.insert(cache_key.clone(), (std::time::Instant::now(), node.clone()));
        }
        return Ok(node);
    }

    // L3: stale cache — return immediately for fast first-paint, then
    // spawn a background thread to do a fresh scan and update both caches.
    if let Some(stale_node) = cache::load_stale(&path, depth) {
        let stale_clone = stale_node.clone();
        // Promote stale data to L1 so UI has something right away
        if let Ok(mut cache) = SCAN_CACHE.lock() {
            cache.insert(cache_key.clone(), (std::time::Instant::now(), stale_clone));
        }
        // Background refresh
        let bg_path = path.clone();
        let bg_key = cache_key.clone();
        let bg_app = app.clone();
        std::thread::spawn(move || {
            let fresh = scanner::scan_directory(&bg_path, depth, |progress| {
                let _ = bg_app.emit("analyze-progress", progress);
            });
            // Update L1
            if let Ok(mut cache) = SCAN_CACHE.lock() {
                cache.insert(bg_key, (std::time::Instant::now(), fresh.clone()));
            }
            // Update L2
            cache::save_to_disk(&bg_path, depth, &fresh);
        });
        return Ok(stale_node);
    }

    // Singleflight dedup: check if another caller is already scanning this path.
    // If so, wait for their result instead of spawning a duplicate scan.
    let flight_key = cache_key.clone();
    let existing_slot = {
        let flights = IN_FLIGHT_SCANS.lock().map_err(|e| e.to_string())?;
        flights.get(&flight_key).map(Arc::clone)
    };

    if let Some(slot) = existing_slot {
        // Another scan is in progress — poll for its result
        loop {
            {
                let guard = slot.lock().map_err(|e| e.to_string())?;
                if let Some(ref result) = *guard {
                    return result.clone();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    // No in-flight scan — register ourselves
    let result_slot = Arc::new(Mutex::new(None));
    {
        let mut flights = IN_FLIGHT_SCANS.lock().map_err(|e| e.to_string())?;
        flights.insert(flight_key.clone(), Arc::clone(&result_slot));
    }

    // Gap 7: peek at cached total files for progress estimation
    let estimated_total = cache::peek_total_files(&path, depth);

    // Cache miss — perform full scan
    let path_clone = path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut first_emit = true;
        scanner::scan_directory(&path_clone, depth, |progress| {
            // On first progress event, include estimated total from previous scan
            if first_emit {
                if let Some(est) = estimated_total {
                    let mut enriched = progress.clone();
                    enriched.estimated_total = Some(est);
                    let _ = app.emit("analyze-progress", &enriched);
                    first_emit = false;
                    return;
                }
            }
            first_emit = false;
            let _ = app.emit("analyze-progress", progress);
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    // Store in L1 (bounded to 32 entries max)
    if let Ok(mut cache) = SCAN_CACHE.lock() {
        cache.insert(cache_key, (std::time::Instant::now(), result.clone()));
        cache.retain(|_, (ts, _)| ts.elapsed() < std::time::Duration::from_secs(300));
        while cache.len() > 32 {
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, (ts, _))| *ts)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            } else {
                break;
            }
        }
    }

    // Store in L2 (disk)
    let disk_path = path.clone();
    let disk_node = result.clone();
    std::thread::spawn(move || {
        cache::save_to_disk(&disk_path, depth, &disk_node);
    });

    // Publish result to any waiting callers, then remove from in-flight map
    {
        let mut guard = result_slot.lock().map_err(|e| e.to_string())?;
        *guard = Some(Ok(result.clone()));
    }
    if let Ok(mut flights) = IN_FLIGHT_SCANS.lock() {
        flights.remove(&flight_key);
    }

    Ok(result)
}

#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("Path does not exist".to_string());
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_analyzed_item(path: String, permanent: bool) -> Result<u64, String> {
    // Canonicalize to prevent symlink traversal
    let canonical = std::fs::canonicalize(&path)
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let path = canonical.to_string_lossy().to_string();
    let p = Path::new(&path);
    if !p.exists() {
        return Err("Path does not exist".into());
    }

    // Safety: don't delete system paths
    let protected = ["/System", "/bin", "/sbin", "/usr", "/etc", "/var", "/Applications", "/Library"];
    for prot in &protected {
        if path == *prot || path.starts_with(&format!("{}/", prot)) {
            return Err(format!("Cannot delete protected path: {}", path));
        }
    }

    // Don't delete home directory
    if let Some(home) = dirs::home_dir() {
        if path == home.to_string_lossy() {
            return Err("Cannot delete home directory".into());
        }
    }

    let size = if p.is_dir() {
        let s = crate::commands::utils::dir_size(p);
        if s == 0 {
            // Fallback to du if metadata traversal returns 0 (30s timeout)
            let du_result = std::process::Command::new("du")
                .args(["-sk", &path])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
                .and_then(|mut child| {
                    let start = std::time::Instant::now();
                    let timeout = std::time::Duration::from_secs(30);
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => break child.wait_with_output().ok(),
                            Ok(None) => {
                                if start.elapsed() > timeout {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    break None;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                            Err(_) => break None,
                        }
                    }
                });
            du_result
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .split_whitespace()
                        .next()?
                        .parse::<u64>()
                        .ok()
                        .map(|kb| kb * 1024)
                })
                .unwrap_or(0)
        } else {
            s
        }
    } else {
        p.metadata().map(|m| m.len()).unwrap_or(0)
    };

    if permanent {
        if p.is_dir() {
            std::fs::remove_dir_all(p).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_file(p).map_err(|e| e.to_string())?;
        }
    } else {
        trash::delete(p).map_err(|e| e.to_string())?;
    }

    crate::commands::shared::log_operation(
        "ANALYZE_DELETE",
        &path,
        if permanent { "DELETED" } else { "TRASHED" },
    );

    // Invalidate L1 in-memory cache entries containing this path
    if let Ok(mut mem_cache) = SCAN_CACHE.lock() {
        mem_cache.retain(|_, (_, node)| {
            node.path != path && !path.starts_with(&format!("{}/", node.path))
                && !node.path.starts_with(&format!("{}/", path))
        });
    }

    // Invalidate L2 disk cache entries (async to avoid blocking)
    let deleted = path.clone();
    std::thread::spawn(move || {
        cache::invalidate_path(&deleted);
    });

    Ok(size)
}

#[tauri::command]
pub async fn find_large_files(min_size_mb: u64, search_path: Option<String>) -> Vec<LargeFile> {
    let min_bytes = min_size_mb * 1024 * 1024;
    let scope = search_path
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().to_string_lossy().to_string());
    let query = format!("kMDItemFSSize >= {}", min_bytes);

    let mut files: Vec<LargeFile> = Vec::new();

    let output = {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut child = match std::process::Command::new("mdfind")
            .args(["-onlyin", &scope, &query])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return files,
        };

        let timeout = Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break child.wait_with_output().ok(),
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break None,
            }
        }
    };

    if let Some(o) = output {
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines().take(200) {
            let path = Path::new(line);
            if !path.exists() || path.is_dir() {
                continue;
            }
            // Skip source code and data files (matching reference ~40+ extensions)
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                let skip_exts = [
                    "go", "js", "ts", "tsx", "jsx", "json", "md", "txt",
                    "yml", "yaml", "xml", "html", "css", "scss", "sass", "less",
                    "py", "rb", "java", "kt", "rs", "swift", "m", "mm",
                    "c", "cpp", "h", "hpp", "cs", "sql", "db", "lock",
                    "gradle", "mjs", "cjs", "coffee", "dart", "svelte", "vue",
                    "nim", "hx",
                ];
                if skip_exts.contains(&ext_lower.as_str()) {
                    continue;
                }
            }
            // Skip files inside folded directories
            if scanner::is_in_folded_dir(line) {
                continue;
            }
            if let Ok(meta) = path.symlink_metadata() {
                if meta.file_type().is_symlink() {
                    continue;
                }
                files.push(LargeFile {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path: line.to_string(),
                    size: meta.len(),
                });
            }
        }
    }

    files.sort_by(|a, b| b.size.cmp(&a.size));
    files.truncate(20);
    files
}

// ---------------------------------------------------------------------------
// Overview / disk analysis mode
// ---------------------------------------------------------------------------

/// Build the list of overview entry descriptors (path + display name + category).
/// Only includes paths that actually exist on disk.
fn create_overview_entries() -> Vec<(String, String, String)> {
    let home = dirs::home_dir().unwrap_or_default();
    let home_str = home.to_string_lossy().to_string();
    let mut entries: Vec<(String, String, String)> = Vec::new();

    // ---- Core user/system directories ----
    if !home_str.is_empty() {
        entries.push(("Home".into(), home_str.clone(), "user".into()));

        let user_library = home.join("Library");
        if user_library.exists() {
            entries.push((
                "App Library".into(),
                user_library.to_string_lossy().to_string(),
                "user".into(),
            ));
        }
    }

    let system_dirs: &[(&str, &str)] = &[
        ("Applications", "/Applications"),
        ("System Library", "/Library"),
    ];
    for (name, path) in system_dirs {
        if Path::new(path).exists() {
            entries.push((name.to_string(), path.to_string(), "system".into()));
        }
    }

    // ---- Insight entries (hidden space accumulators, only if they exist) ----
    if !home_str.is_empty() {
        let insight_paths: Vec<(&str, PathBuf)> = vec![
            (
                "iOS Backups",
                home.join("Library/Application Support/MobileSync/Backup"),
            ),
            ("Old Downloads (90d+)", home.join("Downloads")),
            ("System Logs", home.join("Library/Logs")),
            ("Homebrew Cache", home.join("Library/Caches/Homebrew")),
            (
                "Xcode DerivedData",
                home.join("Library/Developer/Xcode/DerivedData"),
            ),
            (
                "Xcode Simulators",
                home.join("Library/Developer/CoreSimulator/Devices"),
            ),
            (
                "Xcode Archives",
                home.join("Library/Developer/Xcode/Archives"),
            ),
            (
                "Spotify Cache",
                home.join("Library/Application Support/Spotify/PersistentCache"),
            ),
            ("JetBrains Cache", home.join("Library/Caches/JetBrains")),
            (
                "Docker Data",
                home.join("Library/Containers/com.docker.docker/Data"),
            ),
            ("pip Cache", home.join("Library/Caches/pip")),
            ("Gradle Cache", home.join(".gradle/caches")),
            ("CocoaPods Cache", home.join("Library/Caches/CocoaPods")),
        ];
        for (name, path) in insight_paths {
            if path.is_dir() {
                entries.push((
                    name.to_string(),
                    path.to_string_lossy().to_string(),
                    "insight".into(),
                ));
            }
        }
    }

    entries
}

/// Get the overview snapshot file path.
fn overview_snapshot_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = home.join("Library/Caches/com.kyra.app");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(OVERVIEW_SNAPSHOT_FILE))
}

/// Load overview size snapshot from disk (persistent JSON).
fn load_overview_snapshot() -> HashMap<String, (u64, u64)> {
    let path = match overview_snapshot_path() {
        Some(p) => p,
        None => return HashMap::new(),
    };
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    // Format: { "path": { "size": N, "updated": epoch_secs } }
    let parsed: HashMap<String, serde_json::Value> = match serde_json::from_slice(&data) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let mut result = HashMap::new();
    for (k, v) in parsed {
        let size = v.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
        let updated = v.get("updated").and_then(|s| s.as_u64()).unwrap_or(0);
        if size > 0 {
            result.insert(k, (size, updated));
        }
    }
    result
}

/// Save a single overview size entry to the persistent snapshot.
fn store_overview_size(path: &str, size: u64) {
    if path.is_empty() || size == 0 {
        return;
    }
    let snapshot_path = match overview_snapshot_path() {
        Some(p) => p,
        None => return,
    };
    let mut snapshots = load_overview_snapshot();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    snapshots.insert(path.to_string(), (size, now));

    // Serialize
    let mut map = serde_json::Map::new();
    for (k, (s, u)) in &snapshots {
        let mut entry = serde_json::Map::new();
        entry.insert("size".to_string(), serde_json::Value::Number((*s).into()));
        entry.insert("updated".to_string(), serde_json::Value::Number((*u).into()));
        map.insert(k.clone(), serde_json::Value::Object(entry));
    }
    let data = match serde_json::to_vec_pretty(&map) {
        Ok(d) => d,
        Err(_) => return,
    };
    let tmp_path = snapshot_path.with_extension("tmp");
    if std::fs::write(&tmp_path, &data).is_ok() {
        let _ = std::fs::rename(&tmp_path, &snapshot_path);
    }
}

/// Load a stored overview size if it exists and is within the TTL.
fn load_stored_overview_size(path: &str) -> Option<u64> {
    let snapshots = load_overview_snapshot();
    let (size, updated) = snapshots.get(path)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(*updated) < OVERVIEW_CACHE_TTL.as_secs() && *size > 0 {
        Some(*size)
    } else {
        None
    }
}

/// Measure directory size using `du -skP` with an optional exclude path.
/// When measuring Home, we subtract ~/Library to avoid double-counting since
/// App Library is listed separately.
fn measure_dir_size(path: &str) -> u64 {
    // Check in-memory overview cache first
    if let Ok(cache) = OVERVIEW_SIZE_CACHE.lock() {
        if let Some((ts, size)) = cache.get(path) {
            if ts.elapsed() < OVERVIEW_CACHE_TTL {
                return *size;
            }
        }
    }

    // Check persistent snapshot
    if let Some(size) = load_stored_overview_size(path) {
        if let Ok(mut cache) = OVERVIEW_SIZE_CACHE.lock() {
            cache.insert(path.to_string(), (Instant::now(), size));
        }
        return size;
    }

    let size = measure_dir_size_uncached(path);

    // Store in memory cache and persistent snapshot
    if size > 0 {
        if let Ok(mut cache) = OVERVIEW_SIZE_CACHE.lock() {
            cache.insert(path.to_string(), (Instant::now(), size));
        }
        store_overview_size(path, size);
    }

    size
}

/// Run `du -skP` on a path with a timeout. Returns size in bytes.
fn run_du(path: &str) -> Option<u64> {
    let mut child = std::process::Command::new("du")
        .args(["-skP", path])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().ok()?;
                let text = String::from_utf8_lossy(&output.stdout);
                let kb: u64 = text.split_whitespace().next()?.parse().ok()?;
                return if kb > 0 { Some(kb * 1024) } else { None };
            }
            Ok(None) => {
                if start.elapsed() > DU_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }
}

/// Measure directory size without caching.
/// For Home, excludes ~/Library to prevent double-counting.
/// For "Old Downloads (90d+)", counts only files older than 90 days.
fn measure_dir_size_uncached(path: &str) -> u64 {
    let home = dirs::home_dir().unwrap_or_default();
    let home_str = home.to_string_lossy().to_string();

    // Special case: Old Downloads — only count files older than 90 days
    if !home_str.is_empty() && path == home.join("Downloads").to_string_lossy() {
        return measure_old_downloads(path, 90);
    }

    // Special case: Home — subtract ~/Library size
    if !home_str.is_empty() && path == home_str {
        let total = match run_du(path) {
            Some(s) => s,
            None => return crate::commands::utils::dir_size(Path::new(path)),
        };
        let library_path = home.join("Library");
        let library_size = run_du(&library_path.to_string_lossy()).unwrap_or(0);
        return total.saturating_sub(library_size);
    }

    // Normal case
    run_du(path).unwrap_or_else(|| crate::commands::utils::dir_size(Path::new(path)))
}

/// Calculate total size of files in a directory older than `days_old` days.
fn measure_old_downloads(dir: &str, days_old: i64) -> u64 {
    use std::time::SystemTime;

    let cutoff = SystemTime::now()
        - Duration::from_secs((days_old as u64) * 24 * 60 * 60);

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let mut total: u64 = 0;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Skip hidden files
        if name_str.starts_with('.') {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if mtime < cutoff {
            if meta.is_dir() {
                let child_path = entry.path();
                total += run_du(&child_path.to_string_lossy()).unwrap_or(0);
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Get disk space info for the boot volume.
fn get_disk_info() -> (u64, u64) {
    #[cfg(target_os = "macos")]
    {
        use std::mem::MaybeUninit;
        let mut stat = MaybeUninit::<libc::statfs>::uninit();
        let path = std::ffi::CString::new("/").unwrap();
        let ret = unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) };
        if ret == 0 {
            let stat = unsafe { stat.assume_init() };
            let block_size = stat.f_bsize as u64;
            let total = stat.f_blocks as u64 * block_size;
            let free = stat.f_bavail as u64 * block_size;
            return (free, total);
        }
    }
    (0, 0)
}

#[tauri::command]
pub async fn analyze_overview() -> Result<OverviewResult, String> {
    let entry_specs = create_overview_entries();

    // Check if we can serve from cache (all entries have cached sizes)
    let mut all_cached = true;
    let mut cached_entries: Vec<OverviewEntry> = Vec::with_capacity(entry_specs.len());
    for (name, path, category) in &entry_specs {
        if let Ok(cache) = OVERVIEW_SIZE_CACHE.lock() {
            if let Some((ts, size)) = cache.get(path.as_str()) {
                if ts.elapsed() < OVERVIEW_CACHE_TTL {
                    cached_entries.push(OverviewEntry {
                        name: name.clone(),
                        path: path.clone(),
                        size: *size,
                        is_dir: true,
                        category: category.clone(),
                    });
                    continue;
                }
            }
        }
        // Also try persistent snapshot
        if let Some(size) = load_stored_overview_size(path) {
            cached_entries.push(OverviewEntry {
                name: name.clone(),
                path: path.clone(),
                size,
                is_dir: true,
                category: category.clone(),
            });
            continue;
        }
        all_cached = false;
        break;
    }

    // If all cached, return immediately and spawn background refresh
    if all_cached && cached_entries.len() == entry_specs.len() {
        cached_entries.sort_by(|a, b| b.size.cmp(&a.size));
        let total_size: u64 = cached_entries.iter().map(|e| e.size).sum();
        let (disk_free, disk_total) = get_disk_info();

        // Background prefetch: refresh sizes for entries that may be stale
        let specs_for_refresh = entry_specs.clone();
        std::thread::spawn(move || {
            for (_name, path, _category) in specs_for_refresh {
                let size = measure_dir_size_uncached(&path);
                if size > 0 {
                    if let Ok(mut cache) = OVERVIEW_SIZE_CACHE.lock() {
                        cache.insert(path.clone(), (Instant::now(), size));
                    }
                    store_overview_size(&path, size);
                }
            }
        });

        return Ok(OverviewResult {
            entries: cached_entries,
            total_size,
            disk_free,
            disk_total,
        });
    }

    // Not all cached — do a full measurement
    let mut handles = Vec::with_capacity(entry_specs.len());
    for (name, path, category) in entry_specs {
        let handle = tauri::async_runtime::spawn_blocking(move || {
            let size = measure_dir_size(&path);
            OverviewEntry {
                name,
                path,
                size,
                is_dir: true,
                category,
            }
        });
        handles.push(handle);
    }

    // Collect results
    let mut entries = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                eprintln!("overview scan task failed: {}", e);
            }
        }
    }

    // Sort by size descending
    entries.sort_by(|a, b| b.size.cmp(&a.size));

    let total_size: u64 = entries.iter().map(|e| e.size).sum();
    let (disk_free, disk_total) = get_disk_info();

    Ok(OverviewResult {
        entries,
        total_size,
        disk_free,
        disk_total,
    })
}

/// Batch delete multiple analyzed items, sorting deepest-first to avoid
/// parent/child conflicts. Emits progress events during deletion.
#[tauri::command]
pub async fn delete_analyzed_items(
    app: tauri::AppHandle,
    paths: Vec<String>,
    permanent: bool,
) -> Result<u64, String> {
    if paths.is_empty() {
        return Ok(0);
    }

    // Sort paths deepest-first (most path separators first) to avoid
    // deleting a parent before its children.
    let mut sorted_paths = paths.clone();
    sorted_paths.sort_by(|a, b| {
        let depth_a = a.matches('/').count();
        let depth_b = b.matches('/').count();
        depth_b.cmp(&depth_a)
    });

    let total_count = sorted_paths.len();
    let mut total_size: u64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for (i, path_str) in sorted_paths.iter().enumerate() {
        // Emit progress
        let _ = app.emit(
            "analyze-delete-progress",
            DeleteProgress {
                current: i + 1,
                total: total_count,
                path: path_str.clone(),
                done: false,
            },
        );

        // Skip paths that no longer exist (already deleted as child of a parent)
        let p = Path::new(path_str);
        if !p.exists() {
            continue;
        }

        match delete_single_item(path_str, permanent).await {
            Ok(size) => total_size += size,
            Err(e) => errors.push(format!("{}: {}", path_str, e)),
        }
    }

    // Emit final progress
    let _ = app.emit(
        "analyze-delete-progress",
        DeleteProgress {
            current: total_count,
            total: total_count,
            path: String::new(),
            done: true,
        },
    );

    if !errors.is_empty() {
        let msg = errors.iter().take(3).cloned().collect::<Vec<_>>().join("; ");
        return Err(format!("Some deletions failed: {}", msg));
    }

    Ok(total_size)
}

/// Internal helper for deleting a single item (used by both single and batch delete).
async fn delete_single_item(path: &str, permanent: bool) -> Result<u64, String> {
    // Canonicalize to prevent symlink traversal
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let path = canonical.to_string_lossy().to_string();
    let p = Path::new(&path);

    if !p.exists() {
        return Err("Path does not exist".into());
    }

    // Safety: don't delete system paths
    let protected = ["/System", "/bin", "/sbin", "/usr", "/etc", "/var", "/Applications", "/Library"];
    for prot in &protected {
        if path == *prot || path.starts_with(&format!("{}/", prot)) {
            return Err(format!("Cannot delete protected path: {}", path));
        }
    }

    if let Some(home) = dirs::home_dir() {
        if path == home.to_string_lossy() {
            return Err("Cannot delete home directory".into());
        }
    }

    let size = if p.is_dir() {
        let s = crate::commands::utils::dir_size(p);
        if s == 0 {
            run_du(&path).unwrap_or(0)
        } else {
            s
        }
    } else {
        p.metadata().map(|m| m.len()).unwrap_or(0)
    };

    if permanent {
        if p.is_dir() {
            std::fs::remove_dir_all(p).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_file(p).map_err(|e| e.to_string())?;
        }
    } else {
        trash::delete(p).map_err(|e| e.to_string())?;
    }

    crate::commands::shared::log_operation(
        "ANALYZE_DELETE",
        &path,
        if permanent { "DELETED" } else { "TRASHED" },
    );

    // Invalidate caches
    if let Ok(mut mem_cache) = SCAN_CACHE.lock() {
        mem_cache.retain(|_, (_, node)| {
            node.path != path && !path.starts_with(&format!("{}/", node.path))
                && !node.path.starts_with(&format!("{}/", path))
        });
    }
    let deleted = path.to_string();
    std::thread::spawn(move || {
        cache::invalidate_path(&deleted);
    });

    Ok(size)
}
