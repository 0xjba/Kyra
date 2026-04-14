use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use super::{DirNode, ScanProgress};

/// Directories treated as atomic — never recurse into them.
const FOLDED_DIRS: &[&str] = &[
    // VCS
    ".git", ".svn", ".hg",
    // JavaScript/Node
    "node_modules", ".npm", ".yarn", ".pnpm-store", ".next", ".nuxt",
    "bower_components", ".vite", ".turbo", ".parcel-cache", ".nx", ".rush",
    "tnpm", ".tnpm", ".bun", ".deno",
    // npm internals
    "_npx", "_cacache", "_logs", "_locks", "_quick", "_libvips",
    "_prebuilds", "_update-notifier-last-checked",
    // Python
    "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache",
    "venv", ".venv", "virtualenv", ".tox", "site-packages", ".eggs",
    "*.egg-info", ".pyenv", ".poetry", ".pip", ".pipx", ".nox",
    // Ruby/Go/PHP/Java/Rust
    "vendor", ".bundle", "gems", ".rbenv", "target", ".gradle",
    ".m2", ".ivy2", "out", "pkg", "composer.phar", ".composer", ".cargo",
    // Build outputs
    "build", "dist", ".output", "coverage", ".coverage",
    // IDE
    ".idea", ".vscode", ".vs", ".fleet",
    // Cache directories
    ".cache", "__MACOSX", ".DS_Store", ".Trash", "Caches",
    ".Spotlight-V100", ".fseventsd", ".DocumentRevisions-V100",
    ".TemporaryItems", "$RECYCLE.BIN", ".temp", ".tmp", "_temp", "_tmp",
    ".Homebrew", ".rustup", ".sdkman", ".nvm",
    "CachedData", "CachedExtensions", "GPUCache", "Cache",
    // macOS
    "Application Scripts", "Saved Application State",
    // iCloud
    "Mobile Documents",
    // Containers
    ".docker", ".containerd",
    // Mobile development
    "Pods", "DerivedData", ".build", "xcuserdata", "Carthage", ".dart_tool",
    // Web frameworks
    ".angular", ".svelte-kit", ".astro", ".solid",
    // Databases
    ".mysql", ".postgres", "mongodb",
    // Infrastructure
    ".terraform", ".vagrant",
    // Temp
    "tmp", "temp",
    // Other
    ".cxx", ".expo", "zig-out", ".zig-cache", "obj", "bin",
    ".Trashes",
];

/// Directories that are safe to delete (build artifacts, caches).
const CLEANABLE_NAMES: &[&str] = &[
    "node_modules", "target", "build", "dist", "venv", ".venv",
    "__pycache__", ".pytest_cache", "Pods", "DerivedData",
    ".next", ".nuxt", ".gradle", ".turbo", "coverage",
    ".mypy_cache", ".nox", ".output", ".ruff_cache", ".tox",
    "Cache", "Caches", "CachedData", "GPUCache",
];

/// Top-level directories to skip when scanning from `/`.
const SKIP_DIRS: &[&str] = &[
    "dev", "cores", "System", "sbin", "bin", "etc", "var",
    "Volumes", "Network", ".vol", ".Spotlight-V100", ".fseventsd",
    "private",
    // VM and container mounts
    "OrbStack", "Colima", "Parallels", "VMware Fusion", "VirtualBox VMs",
    "Rancher Desktop", ".lima", ".colima", ".orbstack",
    // macOS system
    ".DocumentRevisions-V100", ".TemporaryItems", ".MobileBackups",
    // Network
    "home", "net",
];

/// Directories to skip at ALL depth levels (network/virtual mounts, etc.).
const DEFAULT_SKIP_DIRS: &[&str] = &[
    "nfs",
    "PHD",
    "Permissions",
    // Virtualization/Container mounts (NFS, network filesystems)
    "OrbStack",
    "Colima",
    "Parallels",
    "VMware Fusion",
    "VirtualBox VMs",
    "Rancher Desktop",
    ".lima",
    ".colima",
    ".orbstack",
];

fn is_folded(name: &str) -> bool {
    FOLDED_DIRS.iter().any(|&d| d == name)
}

/// Check if a directory should be folded based on its name AND parent path.
/// Handles npm/tnpm cache structures where internal directories should be
/// treated as atomic.
fn should_fold_with_path(name: &str, path: &Path) -> bool {
    if is_folded(name) {
        return true;
    }

    // npm/tnpm cache: fold internal directories
    let path_str = path.to_string_lossy();
    if path_str.contains("/.npm/") || path_str.contains("/.tnpm/") {
        if let Some(parent_name) = path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) {
            if parent_name == ".npm" || parent_name == ".tnpm" || parent_name.starts_with('_') {
                return true;
            }
        }
        // Single-char directory names inside npm/tnpm caches
        if name.len() == 1 {
            return true;
        }
    }

    false
}

/// Check if a directory should be skipped at any depth level.
fn should_skip_default(name: &str) -> bool {
    DEFAULT_SKIP_DIRS.iter().any(|&d| d == name)
}

/// Check if a path is inside a folded directory (used to filter mdfind results).
pub fn is_in_folded_dir(path: &str) -> bool {
    for segment in path.split('/') {
        if !segment.is_empty() && FOLDED_DIRS.iter().any(|&d| d == segment) {
            return true;
        }
    }
    false
}

fn is_cleanable(name: &str, path: &Path) -> bool {
    if !CLEANABLE_NAMES.iter().any(|&d| d == name) {
        return false;
    }
    // Don't mark as cleanable if under ~/Library/Caches or ~/Library/Logs
    // (those are managed by the Clean module)
    if let Some(home) = dirs::home_dir() {
        let caches_dir = home.join("Library/Caches");
        let logs_dir = home.join("Library/Logs");
        if path.starts_with(&caches_dir) || path.starts_with(&logs_dir) {
            return false;
        }
    }
    true
}

fn should_skip_root_child(name: &str) -> bool {
    SKIP_DIRS.iter().any(|&d| d == name)
}

/// Physical size of a single file (blocks * 512), matching `du`.
fn file_physical_size(meta: &fs::Metadata) -> u64 {
    let logical = meta.len();
    let physical = meta.blocks() * 512;
    if physical > 0 && physical < logical {
        physical
    } else {
        logical
    }
}

/// Extract last access time (atime) from metadata via the Unix stat.
fn get_last_access(meta: &fs::Metadata) -> Option<u64> {
    let atime = meta.atime();
    if atime > 0 { Some(atime as u64) } else { None }
}

/// Concurrency limit for parallel directory sizing.
fn concurrency_limit() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (cpus * 4).min(64).max(4)
}

/// Shared progress state accessible from multiple threads.
struct SharedProgress<F> {
    files_scanned: AtomicUsize,
    dirs_scanned: AtomicUsize,
    total_size: AtomicU64,
    estimated_total: AtomicUsize,
    callback: Mutex<F>,
}

impl<F: FnMut(&ScanProgress)> SharedProgress<F> {
    fn new(callback: F) -> Self {
        Self {
            files_scanned: AtomicUsize::new(0),
            dirs_scanned: AtomicUsize::new(0),
            total_size: AtomicU64::new(0),
            estimated_total: AtomicUsize::new(0),
            callback: Mutex::new(callback),
        }
    }

    #[allow(dead_code)]
    fn set_estimated_total(&self, total: usize) {
        self.estimated_total.store(total, Ordering::Relaxed);
    }

    fn add_file(&self, size: u64) {
        let count = self.files_scanned.fetch_add(1, Ordering::Relaxed) + 1;
        self.total_size.fetch_add(size, Ordering::Relaxed);

        if count % 500 == 0 {
            self.emit_progress("");
        }
    }

    fn add_dir(&self) {
        self.dirs_scanned.fetch_add(1, Ordering::Relaxed);
    }

    fn add_size(&self, size: u64) {
        self.total_size.fetch_add(size, Ordering::Relaxed);
    }

    fn emit_progress(&self, current_path: &str) {
        if let Ok(mut cb) = self.callback.lock() {
            let est = self.estimated_total.load(Ordering::Relaxed);
            cb(&ScanProgress {
                current_path: current_path.to_string(),
                files_scanned: self.files_scanned.load(Ordering::Relaxed),
                dirs_scanned: self.dirs_scanned.load(Ordering::Relaxed),
                total_size: self.total_size.load(Ordering::Relaxed),
                estimated_total: if est > 0 { Some(est) } else { None },
            });
        }
    }

    fn files_count(&self) -> usize {
        self.files_scanned.load(Ordering::Relaxed)
    }
}

/// Recursively scans a directory and builds a size-annotated tree.
/// `max_depth` limits how deep the tree goes (0 = just the root).
/// Calls `on_progress` periodically during the scan.
pub fn scan_directory<F>(
    root_path: &str,
    max_depth: usize,
    on_progress: F,
) -> DirNode
where
    F: FnMut(&ScanProgress) + Send,
{
    let path = Path::new(root_path);
    let is_root = root_path == "/";
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    let progress = Arc::new(SharedProgress::new(on_progress));

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(root_path)
        .to_string();

    let mut root = build_tree(path, 0, max_depth, is_root, &progress, deadline);
    root.name = name;
    root.path = root_path.to_string();
    root
}

fn build_tree<F>(
    path: &Path,
    current_depth: usize,
    max_depth: usize,
    is_root_scan: bool,
    progress: &Arc<SharedProgress<F>>,
    deadline: std::time::Instant,
) -> DirNode
where
    F: FnMut(&ScanProgress) + Send,
{
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    // Handle symlinks: count lstat size (the link itself) instead of 0
    if path.is_symlink() {
        let size = path.symlink_metadata()
            .map(|m| file_physical_size(&m))
            .unwrap_or(0);
        let is_dir = path.metadata().map(|m| m.is_dir()).unwrap_or(false);
        progress.add_file(size);
        return DirNode {
            name: format!("{} \u{2192}", name), // arrow suffix like reference
            path: path.to_string_lossy().to_string(),
            size,
            is_dir,
            is_cleanable: false,
            children: vec![],
            last_access: path.symlink_metadata().ok().and_then(|m| get_last_access(&m)),
        };
    }

    if path.is_file() {
        let meta = path.metadata().ok();
        let size = meta.as_ref().map(|m| file_physical_size(m)).unwrap_or(0);
        let last_access = meta.as_ref().and_then(|m| get_last_access(m));
        progress.add_file(size);

        return DirNode {
            name,
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: false,
            is_cleanable: false,
            children: vec![],
            last_access,
        };
    }

    // Check scan deadline
    if progress.files_count() % 1000 == 0 && std::time::Instant::now() > deadline {
        return DirNode {
            name,
            path: path.to_string_lossy().to_string(),
            size: 0,
            is_dir: true,
            is_cleanable: false,
            children: vec![],
            last_access: None,
        };
    }

    // Skip directories that should be skipped at any depth level
    if should_skip_default(&name) {
        return DirNode {
            name,
            path: path.to_string_lossy().to_string(),
            size: 0,
            is_dir: true,
            is_cleanable: false,
            children: vec![],
            last_access: None,
        };
    }

    // Directory — check if it should be folded (treated as atomic leaf)
    if current_depth > 0 && should_fold_with_path(&name, path) {
        // For folded dirs, try `du -skP` first with a timeout, fall back to manual walk
        let size = dir_size_du_fallback(path);
        progress.add_size(size);
        progress.add_dir();
        progress.emit_progress(&path.to_string_lossy());

        return DirNode {
            name: name.clone(),
            path: path.to_string_lossy().to_string(),
            size,
            is_dir: true,
            is_cleanable: is_cleanable(&name, path),
            children: vec![],
            last_access: None,
        };
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            return DirNode {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                size: 0,
                is_dir: true,
                is_cleanable: is_cleanable(&name, path),
                children: vec![],
                last_access: None,
            };
        }
    };

    // Collect all entries upfront so we can process directories in parallel
    let mut file_entries: Vec<PathBuf> = Vec::new();
    let mut dir_entries: Vec<PathBuf> = Vec::new();

    // Also track symlinks separately for size counting
    let mut symlink_entries: Vec<PathBuf> = Vec::new();

    for entry in entries.filter_map(|e| e.ok()) {
        let child_path = entry.path();

        // Skip system directories when scanning from /
        if is_root_scan && current_depth == 0 {
            if let Some(child_name) = child_path.file_name().and_then(|n| n.to_str()) {
                if should_skip_root_child(child_name) {
                    continue;
                }
            }
        }

        if child_path.is_symlink() {
            symlink_entries.push(child_path);
            continue;
        }

        if child_path.is_dir() {
            dir_entries.push(child_path);
        } else {
            file_entries.push(child_path);
        }
    }

    let mut children = Vec::new();
    let mut total_size: u64 = 0;

    // Process symlinks: count lstat size to avoid double-counting targets
    for link_path in &symlink_entries {
        let meta = match link_path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = file_physical_size(&meta);
        let is_dir = link_path.metadata().map(|m| m.is_dir()).unwrap_or(false);
        progress.add_file(size);
        total_size += size;

        if current_depth < max_depth {
            let fname = link_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            children.push(DirNode {
                name: format!("{} \u{2192}", fname),
                path: link_path.to_string_lossy().to_string(),
                size,
                is_dir,
                is_cleanable: false,
                children: vec![],
                last_access: get_last_access(&meta),
            });
        }
    }

    // Process files sequentially (fast, no I/O contention benefit)
    for file_path in &file_entries {
        let meta = file_path.metadata().ok();
        let size = meta.as_ref().map(|m| file_physical_size(m)).unwrap_or(0);
        progress.add_file(size);
        total_size += size;

        if current_depth < max_depth {
            let fname = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            children.push(DirNode {
                name: fname,
                path: file_path.to_string_lossy().to_string(),
                size,
                is_dir: false,
                is_cleanable: false,
                children: vec![],
                last_access: meta.as_ref().and_then(|m| get_last_access(m)),
            });
        }
    }

    if current_depth < max_depth {
        // Process child directories — use thread::scope for concurrency
        // Each child builds its subtree in parallel
        let child_nodes: Vec<DirNode> = if dir_entries.len() > 1 {
            parallel_build_children(&dir_entries, current_depth, max_depth, is_root_scan, progress, deadline)
        } else {
            // Single or no directory — no point spawning threads
            dir_entries.iter().map(|dir_path| {
                build_tree(dir_path, current_depth + 1, max_depth, is_root_scan, progress, deadline)
            }).collect()
        };

        for child in child_nodes {
            total_size += child.size;
            children.push(child);
        }

        // Sort children by size descending
        children.sort_by(|a, b| b.size.cmp(&a.size));
    } else {
        // At max depth, just calculate total size without building children
        // Use concurrent sizing for directories
        if dir_entries.len() > 1 {
            let dir_sizes = parallel_flat_size(&dir_entries, progress);
            total_size += dir_sizes;
        } else {
            for dir_path in &dir_entries {
                total_size += flat_size(dir_path, progress);
            }
        }
    }

    progress.add_dir();

    DirNode {
        name: name.clone(),
        path: path.to_string_lossy().to_string(),
        size: total_size,
        is_dir: true,
        is_cleanable: is_cleanable(&name, path),
        children,
        last_access: None,
    }
}

/// Build child DirNodes for multiple directories concurrently using thread::scope.
fn parallel_build_children<F>(
    dir_entries: &[PathBuf],
    current_depth: usize,
    max_depth: usize,
    is_root_scan: bool,
    progress: &Arc<SharedProgress<F>>,
    deadline: std::time::Instant,
) -> Vec<DirNode>
where
    F: FnMut(&ScanProgress) + Send,
{
    let limit = concurrency_limit();
    // Use a channel as a counting semaphore to bound concurrency
    let (sem_tx, sem_rx) = std::sync::mpsc::sync_channel::<()>(limit);
    // Pre-fill the semaphore
    for _ in 0..limit {
        let _ = sem_tx.send(());
    }

    let results: Mutex<Vec<(usize, DirNode)>> = Mutex::new(Vec::with_capacity(dir_entries.len()));

    std::thread::scope(|s| {
        for (idx, dir_path) in dir_entries.iter().enumerate() {
            // Try to acquire a semaphore slot (non-blocking first to avoid deadlock
            // under deep recursion — same pattern as the reference implementation)
            let got_slot = sem_rx.try_recv().is_ok();

            let progress = Arc::clone(progress);
            let results = &results;
            let sem_tx = sem_tx.clone();
            let dir_path = dir_path.clone();

            if got_slot {
                s.spawn(move || {
                    let node = build_tree(&dir_path, current_depth + 1, max_depth, is_root_scan, &progress, deadline);
                    results.lock().unwrap().push((idx, node));
                    // Release semaphore slot
                    let _ = sem_tx.send(());
                });
            } else {
                // Fallback: run synchronously to avoid deadlock under high fan-out
                let node = build_tree(&dir_path, current_depth + 1, max_depth, is_root_scan, &progress, deadline);
                results.lock().unwrap().push((idx, node));
            }
        }
    });

    let mut res = results.into_inner().unwrap();
    res.sort_by_key(|(idx, _)| *idx);
    res.into_iter().map(|(_, node)| node).collect()
}

/// Calculate flat sizes of multiple directories concurrently.
fn parallel_flat_size<F>(
    dir_entries: &[PathBuf],
    progress: &Arc<SharedProgress<F>>,
) -> u64
where
    F: FnMut(&ScanProgress) + Send,
{
    let total = AtomicU64::new(0);
    let limit = concurrency_limit();
    let (sem_tx, sem_rx) = std::sync::mpsc::sync_channel::<()>(limit);
    for _ in 0..limit {
        let _ = sem_tx.send(());
    }

    std::thread::scope(|s| {
        for dir_path in dir_entries {
            let got_slot = sem_rx.try_recv().is_ok();
            let progress = Arc::clone(progress);
            let total = &total;
            let sem_tx = sem_tx.clone();
            let dir_path = dir_path.clone();

            if got_slot {
                s.spawn(move || {
                    let size = flat_size(&dir_path, &progress);
                    total.fetch_add(size, Ordering::Relaxed);
                    let _ = sem_tx.send(());
                });
            } else {
                let size = flat_size(&dir_path, &progress);
                total.fetch_add(size, Ordering::Relaxed);
            }
        }
    });

    total.load(Ordering::Relaxed)
}

/// Concurrent directory size calculation for folded directories.
/// Uses a work-queue with multiple worker threads to walk the directory
/// tree in parallel, bounded by concurrency_limit().
fn dir_size_concurrent(root: &Path) -> u64 {
    let total = AtomicU64::new(0);

    // Work queue: directories left to process
    let queue: Arc<(Mutex<VecDeque<PathBuf>>, Condvar)> = Arc::new((
        Mutex::new(VecDeque::from([root.to_path_buf()])),
        Condvar::new(),
    ));
    // Track how many tasks are in-flight so workers know when to stop
    let in_flight = Arc::new(AtomicUsize::new(1)); // 1 for the initial root entry

    let num_workers = concurrency_limit().min(8);

    std::thread::scope(|s| {
        for _ in 0..num_workers {
            let queue = Arc::clone(&queue);
            let in_flight = Arc::clone(&in_flight);
            let total = &total;

            s.spawn(move || {
                loop {
                    let dir = {
                        let (lock, cvar) = &*queue;
                        let mut q = lock.lock().unwrap();

                        // Wait for work or termination
                        while q.is_empty() {
                            if in_flight.load(Ordering::Acquire) == 0 {
                                return;
                            }
                            // Wait with a short timeout to recheck in_flight
                            let (guard, _) = cvar.wait_timeout(q, std::time::Duration::from_millis(5)).unwrap();
                            q = guard;
                        }

                        q.pop_front().unwrap()
                    };

                    // Process this directory
                    let mut local_size: u64 = 0;
                    let mut subdirs: Vec<PathBuf> = Vec::new();

                    if let Ok(entries) = fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.is_symlink() {
                                continue;
                            }
                            if p.is_dir() {
                                subdirs.push(p);
                            } else {
                                local_size += fs::metadata(&p)
                                    .map(|m| file_physical_size(&m))
                                    .unwrap_or(0);
                            }
                        }
                    }

                    if local_size > 0 {
                        total.fetch_add(local_size, Ordering::Relaxed);
                    }

                    // Enqueue subdirectories
                    if !subdirs.is_empty() {
                        let (lock, cvar) = &*queue;
                        let mut q = lock.lock().unwrap();
                        in_flight.fetch_add(subdirs.len(), Ordering::AcqRel);
                        q.extend(subdirs);
                        cvar.notify_all();
                    }

                    // Mark this directory as done
                    let prev = in_flight.fetch_sub(1, Ordering::AcqRel);
                    if prev == 1 {
                        // Last task done — wake everyone so they can exit
                        let (_, cvar) = &*queue;
                        cvar.notify_all();
                    }
                }
            });
        }
    });

    total.load(Ordering::Relaxed)
}

/// Size a folded directory: try `du -skP` first (fast, kernel-level),
/// fall back to manual concurrent walk on failure or timeout.
fn dir_size_du_fallback(path: &Path) -> u64 {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let path_str = path.to_string_lossy();
    let mut child = match Command::new("du")
        .args(["-skP", &*path_str])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return dir_size_concurrent(path),
    };

    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                if let Ok(output) = child.wait_with_output() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    if let Some(kb) = text.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                        if kb > 0 {
                            return kb * 1024;
                        }
                    }
                }
                return dir_size_concurrent(path);
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return dir_size_concurrent(path);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return dir_size_concurrent(path),
        }
    }
}

/// Calculates size without building child nodes (used at max depth).
fn flat_size<F>(path: &Path, progress: &Arc<SharedProgress<F>>) -> u64
where
    F: FnMut(&ScanProgress) + Send,
{
    if path.is_symlink() {
        // Count symlink's own size (lstat) instead of 0
        let size = path.symlink_metadata().map(|m| file_physical_size(&m)).unwrap_or(0);
        progress.add_file(size);
        return size;
    }
    if path.is_file() {
        let size = path.metadata().map(|m| file_physical_size(&m)).unwrap_or(0);
        progress.add_file(size);
        return size;
    }
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            flat_size(&p, progress)
        })
        .sum()
}
