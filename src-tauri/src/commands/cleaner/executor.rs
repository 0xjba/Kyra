use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

use super::{
    is_safe_path, uses_pseudo_paths, CleanIssue, CleanProgress, CleanResult, IssueReason, PathInfo,
    ScanItem,
};
use crate::commands::{data_guard, shared};
use crate::commands::utils::{dir_size, is_protected_user_data_component, physical_size};

/// What one delete attempt achieved. Deletion carries on past a failing
/// entry, so a tree can be partly freed and still report an error.
#[derive(Debug, Default)]
struct Removal {
    /// Bytes of the files that were actually unlinked.
    freed: u64,
    /// The target itself is gone.
    removed: bool,
    /// The first error hit; later ones are usually the same cause.
    error: Option<io::Error>,
}

impl Removal {
    fn fail(&mut self, e: io::Error) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }
}

/// Adds the owner write bit to `dir` when it is a real directory owned by
/// this user that lacks it, so its entries can be unlinked. Package caches
/// (npm, Go modules, …) mark directories read-only. Never follows a symlink
/// and never touches a directory owned by someone else.
fn grant_owner_write(dir: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return false;
    };
    if !meta.file_type().is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return false;
    }
    let mode = meta.permissions().mode() & 0o7777;
    if mode & 0o200 != 0 {
        return false;
    }
    let Ok(c_path) = CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    let rc = unsafe {
        libc::fchmodat(
            libc::AT_FDCWD,
            c_path.as_ptr(),
            (mode | 0o200) as libc::mode_t,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc == 0 {
        shared::log_operation("CLEAN", &dir.to_string_lossy(), "added owner write permission to clear it");
    }
    rc == 0
}

/// Runs `op` on an entry of `dir`; on EACCES grants owner write on `dir`
/// and retries once. Callers only pass a `dir` inside an approved path.
fn with_owner_write<T>(dir: &Path, mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    match op() {
        Err(e) if e.raw_os_error() == Some(libc::EACCES) && grant_owner_write(dir) => op(),
        other => other,
    }
}

/// What a delete leaves in place inside its target: whitelisted paths and
/// Kyra's own data (its log, settings and caches).
#[derive(Clone, Copy)]
struct Keep<'a> {
    whitelist: &'a HashSet<&'a str>,
}

impl Keep<'_> {
    fn keeps(&self, path: &Path) -> bool {
        data_guard::is_own_data(path) || path.to_str().is_some_and(|p| is_whitelisted(p, self.whitelist))
    }
}

/// Removes everything inside `dir`, which must be the approved path or a
/// real directory below it. Keeps going past failures so one stubborn
/// entry can't leave the rest of the tree behind. Protected user-data
/// subdirectories and whatever `keep` names are left in place. Returns true
/// when `dir` ended up empty.
fn clear_dir(dir: &Path, keep: Keep, out: &mut Removal) -> bool {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            out.fail(e);
            return false;
        }
    };
    let mut empty = true;
    for entry in entries.flatten() {
        let child = entry.path();
        if keep.keeps(&child) {
            empty = false;
            continue;
        }
        let meta = match fs::symlink_metadata(&child) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                out.fail(e);
                empty = false;
                continue;
            }
        };
        if meta.file_type().is_dir() {
            if is_protected_user_data_component(&entry.file_name().to_string_lossy()) {
                empty = false;
                continue;
            }
            if !clear_dir(&child, keep, out) {
                empty = false;
                continue;
            }
            match with_owner_write(dir, || fs::remove_dir(&child)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    out.fail(e);
                    empty = false;
                }
            }
        } else {
            // Files and symlinks are unlinked; a symlink is never followed.
            let size = if meta.file_type().is_symlink() { 0 } else { physical_size(&meta) };
            match with_owner_write(dir, || fs::remove_file(&child)) {
                Ok(()) => out.freed += size,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    out.fail(e);
                    empty = false;
                }
            }
        }
    }
    empty
}

/// Permanently removes `path` (file, symlink or directory tree), keeping
/// protected user-data subdirectories. The parent of `path` lies outside
/// the approved path, so its permissions are never changed.
fn remove_tree(path: &Path, keep: Keep) -> Removal {
    let mut out = Removal::default();
    if keep.keeps(path) {
        return out;
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if is_protected_user_data_component(name) {
            return out;
        }
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            out.fail(e);
            return out;
        }
    };
    if !meta.file_type().is_dir() {
        // A file target is sized by length, as the scanner sizes it.
        let size = if meta.file_type().is_symlink() { 0 } else { meta.len() };
        match fs::remove_file(path) {
            Ok(()) => {
                out.freed = size;
                out.removed = true;
            }
            Err(e) => out.fail(e),
        }
        return out;
    }
    if clear_dir(path, keep, &mut out) && out.error.is_none() {
        match fs::remove_dir(path) {
            Ok(()) => out.removed = true,
            Err(e) => out.fail(e),
        }
    }
    out
}

/// Size of `path` as the scanner measures it, and whether anything inside
/// must be kept. Never follows symlinks.
fn measure_keeping(path: &Path, keep: Keep) -> (u64, bool) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return (0, false);
    };
    if meta.file_type().is_symlink() {
        return (0, false);
    }
    if !meta.file_type().is_dir() {
        return (meta.len(), false);
    }
    let (mut size, mut holds_kept) = (0, false);
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if keep.keeps(&p) {
                holds_kept = true;
                continue;
            }
            match fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_dir() => stack.push(p),
                Ok(m) if !m.file_type().is_symlink() => size += physical_size(&m),
                _ => {}
            }
        }
    }
    (size, holds_kept)
}

/// Moves `path` to the Trash. When something inside must be kept, trashes
/// around it instead of moving the whole folder.
fn trash_tree(path: &Path, keep: Keep) -> Removal {
    let mut out = Removal::default();
    if keep.keeps(path) {
        return out;
    }
    let (size, holds_kept) = measure_keeping(path, keep);
    if !holds_kept {
        match move_to_trash(path) {
            Ok(()) => {
                out.freed = size;
                out.removed = true;
            }
            Err(e) => out.fail(e),
        }
        return out;
    }
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let r = trash_tree(&entry.path(), keep);
                out.freed += r.freed;
                if let Some(e) = r.error {
                    out.fail(e);
                }
            }
        }
        Err(e) => out.fail(e),
    }
    out
}

/// Recursively delete a directory tree while preserving any subdirectory
/// whose name is a protected user-data component (Service Worker,
/// IndexedDB, Local Storage, …). If any protected subdirs are
/// preserved, the root directory itself is left in place; otherwise
/// the root is removed. Returns `Ok(true)` if the root was removed,
/// `Ok(false)` if protected content kept it alive.
fn safe_remove_dir_all(path: &Path) -> io::Result<bool> {
    let r = remove_tree(path, Keep { whitelist: &HashSet::new() });
    match r.error {
        Some(e) => Err(e),
        None => Ok(r.removed),
    }
}

/// Bytes at `path`, measured the way the scanner measures them (0 when it
/// is gone).
fn size_on_disk(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_dir() => dir_size(path),
        Ok(m) if m.file_type().is_symlink() => 0,
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn exists_no_follow(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Maps a delete error to the reason shown to the user.
fn issue_reason(err: &io::Error) -> IssueReason {
    match err.raw_os_error() {
        Some(libc::ENOENT) => IssueReason::AlreadyGone,
        Some(libc::EACCES | libc::EPERM | libc::EROFS) => IssueReason::NoPermission,
        Some(libc::EBUSY | libc::EAGAIN | libc::ENOTEMPTY | libc::ETXTBSY) => IssueReason::InUse,
        _ => match err.kind() {
            io::ErrorKind::NotFound => IssueReason::AlreadyGone,
            io::ErrorKind::PermissionDenied => IssueReason::NoPermission,
            _ => IssueReason::Other,
        },
    }
}

/// Moves `path` to the Trash, translating the Cocoa error codes the trash
/// crate surfaces into io errors.
fn move_to_trash(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(|e| {
        let kind = match &e {
            trash::Error::Os { code: 4 | 260, .. } => io::ErrorKind::NotFound,
            trash::Error::Os { code: 257 | 513, .. } => io::ErrorKind::PermissionDenied,
            _ if !exists_no_follow(path) => io::ErrorKind::NotFound,
            _ => io::ErrorKind::Other,
        };
        io::Error::new(kind, e.to_string())
    })
}

/// Delete a Time Machine in-progress backup bundle via `tmutil delete`,
/// which walks the TM catalogue so the backup database stays consistent.
/// Plain `rm -rf` leaves orphaned index entries and can corrupt subsequent
/// incremental backups, so we never fall back to filesystem deletion for
/// these paths.
fn tmutil_delete(path: &str) -> Result<(), String> {
    let output = Command::new("/usr/bin/tmutil")
        .arg("delete")
        .arg(path)
        .output()
        .map_err(|e| format!("tmutil spawn failed: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

/// Translate an `std::io::Error` from a delete operation into a short
/// human-readable diagnostic. macOS returns the same `PermissionDenied`
/// kind for SIP protection, per-file immutable flags, root ownership,
/// and access-control list restrictions, so the raw message ("Operation
/// not permitted") is rarely actionable. Where possible we map raw OS
/// error codes to a specific hint the user can act on.
fn classify_delete_error(err: &std::io::Error) -> String {
    use std::io::ErrorKind;

    let base = err.to_string();
    let hint: Option<&'static str> = match err.raw_os_error() {
        Some(1) => Some("operation not permitted — path may be SIP-protected, immutable (chflags uchg/schg), or owned by root"),
        Some(13) => Some("access denied — check the file's ACL and your user's write permission on the parent directory"),
        Some(16) => Some("file in use by a running process — quit the owning app and retry"),
        Some(30) => Some("read-only filesystem"),
        Some(66) => Some("directory not empty — a protected user-data subdir (Service Worker / IndexedDB / …) was preserved"),
        Some(35) => Some("resource temporarily unavailable — another process holds a lock"),
        _ => None,
    };

    if let Some(hint) = hint {
        return format!("{} ({})", base, hint);
    }

    match err.kind() {
        ErrorKind::PermissionDenied => format!(
            "{} (permission denied — may be SIP-protected or owned by root)",
            base
        ),
        ErrorKind::NotFound => format!("{} (already removed)", base),
        ErrorKind::ReadOnlyFilesystem => format!("{} (read-only filesystem)", base),
        _ => base,
    }
}

/// Extract the `YYYY-MM-DD-HHMMSS` date portion from a local snapshot
/// identifier like `com.apple.TimeMachine.2025-11-02-120000.local`.
/// Returns `None` if the input doesn't match the expected shape.
fn extract_snapshot_date(full_name: &str) -> Option<String> {
    let after_prefix = full_name.strip_prefix("com.apple.TimeMachine.")?;
    let without_suffix = after_prefix.strip_suffix(".local")?;
    if without_suffix.is_empty() {
        return None;
    }
    Some(without_suffix.to_string())
}

/// Delete a single APFS local Time Machine snapshot via
/// `tmutil deletelocalsnapshots <date>`. The scanner encodes snapshots
/// as `tmutil://<full-identifier>`; this helper decodes the scheme,
/// extracts the date portion, and issues the tmutil call.
fn tmutil_delete_local_snapshot(pseudo_path: &str) -> Result<(), String> {
    let identifier = pseudo_path
        .strip_prefix("tmutil://")
        .ok_or_else(|| "invalid snapshot identifier".to_string())?;
    let date = extract_snapshot_date(identifier)
        .ok_or_else(|| format!("unrecognised snapshot identifier: {}", identifier))?;

    let output = Command::new("/usr/bin/tmutil")
        .arg("deletelocalsnapshots")
        .arg(&date)
        .output()
        .map_err(|e| format!("tmutil spawn failed: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

/// Maps a `simctl_unavailable://<UDID>` pseudo-path to the simulator's
/// device directory. The UDID must look like one, since it is joined onto
/// a real path and a value like `../../..` would escape the Devices dir.
fn simctl_device_dir(home: &Path, pseudo_path: &str) -> Option<std::path::PathBuf> {
    let udid = pseudo_path.strip_prefix("simctl_unavailable://")?;
    if udid.is_empty() || !udid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None;
    }
    Some(home.join("Library/Developer/CoreSimulator/Devices").join(udid))
}

/// Returns true if `path` (or any parent) is covered by a whitelisted entry.
fn is_whitelisted(path: &str, whitelist: &HashSet<&str>) -> bool {
    // O(1) exact match
    if whitelist.contains(path) {
        return true;
    }
    // Check if any whitelisted entry is a parent directory of path
    whitelist.iter().any(|w| path.starts_with(&format!("{}/", w)))
}

/// Delete the contents of a directory without removing the directory itself.
/// Returns the bytes freed and the entries that could not be removed.
fn delete_dir_contents(dir: &Path, permanent: bool, keep: Keep) -> (u64, Vec<(std::path::PathBuf, io::Error)>) {
    let mut freed: u64 = 0;
    let mut errs = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errs.push((dir.to_path_buf(), e));
            return (freed, errs);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }

        // Defense-in-depth: never clear a protected user-data component
        // even if it somehow ends up at the top level of a container dir.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if is_protected_user_data_component(name) {
                continue;
            }
        }
        if keep.keeps(&path) {
            shared::log_operation("CLEAN", &path.to_string_lossy(), "skipped: on user whitelist or Kyra's own data");
            continue;
        }
        if let Err(refusal) = data_guard::check_general(&path) {
            shared::log_operation("CLEAN", &path.to_string_lossy(), &format!("skipped: {}", refusal));
            continue;
        }

        if permanent {
            let r = remove_tree(&path, keep);
            freed += r.freed;
            match r.error {
                None => shared::log_operation("CLEAN", &path.to_string_lossy(), "DELETED"),
                Some(e) => errs.push((path, e)),
            }
        } else {
            let r = trash_tree(&path, keep);
            freed += r.freed;
            match r.error {
                None => shared::log_operation("CLEAN", &path.to_string_lossy(), "TRASHED"),
                Some(e) => errs.push((path, e)),
            }
        }
    }

    (freed, errs)
}

/// One selected path to delete.
struct Task<'a> {
    item: usize,
    info: &'a PathInfo,
    /// Another selected path contains this one.
    nested: bool,
}

/// The order paths are deleted in. A path listed twice is deleted once,
/// by its first item. A path inside another selected path runs first,
/// deepest first, so its own rule removes it and the ancestor then clears
/// only what is left: every byte is counted once, and nothing is reported
/// "already gone" just because an overlapping rule got there first.
struct Plan<'a> {
    tasks: Vec<Task<'a>>,
    /// (item index, path) of entries dropped as exact duplicates.
    duplicates: Vec<(usize, &'a str)>,
}

fn plan_tasks(items: &[ScanItem]) -> Plan<'_> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut tasks = Vec::new();
    let mut duplicates = Vec::new();
    for (i, item) in items.iter().enumerate() {
        for info in &item.paths {
            if seen.insert(info.path.as_str()) {
                tasks.push(Task { item: i, info, nested: false });
            } else {
                duplicates.push((i, info.path.as_str()));
            }
        }
    }

    let is_real = |t: &Task| !uses_pseudo_paths(&items[t.item].rule_id);
    let real: HashSet<&str> = tasks.iter().filter(|t| is_real(t)).map(|t| t.info.path.as_str()).collect();
    for t in tasks.iter_mut() {
        if !uses_pseudo_paths(&items[t.item].rule_id) {
            t.nested = Path::new(&t.info.path)
                .ancestors()
                .skip(1)
                .any(|a| a.to_str().is_some_and(|a| real.contains(a)));
        }
    }

    let (mut ordered, rest): (Vec<Task>, Vec<Task>) = tasks.into_iter().partition(|t| t.nested);
    ordered.sort_by_key(|t| std::cmp::Reverse(Path::new(&t.info.path).components().count()));
    ordered.extend(rest);
    Plan { tasks: ordered, duplicates }
}

fn is_strictly_under(path: &str, ancestor: &str) -> bool {
    path.len() > ancestor.len() && crate::commands::utils::is_same_or_under(path, ancestor)
}

/// Collects the outcome of a clean.
struct Recorder {
    result: CleanResult,
}

impl Recorder {
    /// Records a path that was not freed. `size` is what is still on disk
    /// (or, for an already-gone path, what the scan found).
    fn issue(&mut self, item: &ScanItem, path: &str, reason: IssueReason, size: u64, message: &str) {
        self.result.errors.push(format!("{}: {}", path, message));
        let mut issue = CleanIssue {
            rule_id: item.rule_id.clone(),
            label: item.label.clone(),
            path: path.to_string(),
            size,
            reason,
        };
        if reason == IssueReason::AlreadyGone {
            self.result.already_gone.push(issue);
            return;
        }
        // An ancestor's leftovers include the failures already reported
        // beneath it (they run first); count those bytes once.
        let below: u64 = self
            .result
            .failed
            .iter()
            .filter(|f| is_strictly_under(&f.path, path))
            .map(|f| f.size)
            .sum();
        issue.size = size.saturating_sub(below);
        if issue.size == 0 && below > 0 {
            // Only kept alive by failures already listed beneath it.
            return;
        }
        self.result.bytes_failed += issue.size;
        self.result.failed.push(issue);
    }

    fn io_issue(&mut self, item: &ScanItem, path: &Path, err: &io::Error, scanned: u64) {
        let reason = issue_reason(err);
        let size = if reason == IssueReason::AlreadyGone { scanned } else { size_on_disk(path) };
        let diagnosis = classify_delete_error(err);
        shared::log_operation("CLEAN", &path.to_string_lossy(), &format!("ERROR: {}", diagnosis));
        self.issue(item, &path.to_string_lossy(), reason, size, &diagnosis);
    }
}

/// Deletes all paths for the given scan items.
/// Calls `on_progress` after each path is processed for smooth UI updates.
/// If `dry_run` is true, reports what would be deleted without actually deleting.
pub fn execute_clean_items<F>(
    items: &[ScanItem],
    dry_run: bool,
    permanent: bool,
    on_progress: F,
) -> CleanResult
where
    F: FnMut(&CleanProgress),
{
    let settings = crate::commands::settings::load_settings_internal().unwrap_or_default();
    execute_clean_items_with_whitelist(items, dry_run, permanent, &settings.whitelist, on_progress)
}

fn execute_clean_items_with_whitelist<F>(
    items: &[ScanItem],
    dry_run: bool,
    permanent: bool,
    whitelist: &[String],
    mut on_progress: F,
) -> CleanResult
where
    F: FnMut(&CleanProgress),
{
    let whitelist_set: HashSet<&str> = whitelist.iter().map(|s| s.as_str()).collect();
    let plan = plan_tasks(items);
    let mut rec = Recorder { result: CleanResult::empty() };
    let mut item_ok = vec![false; items.len()];
    let mut path_ok: HashMap<&str, bool> = HashMap::new();
    let items_total = items.len();
    let paths_total = plan.tasks.len();
    let mut paths_done: usize = 0;
    let mut simctl_bulk_ran = false;

    // Emit initial progress so the UI immediately shows "Starting..." instead
    // of being stuck at null until the first deletion completes.
    on_progress(&CleanProgress {
        current_item: plan.tasks.first().map(|t| items[t.item].label.clone()).unwrap_or_default(),
        items_done: 0,
        items_total,
        paths_done: 0,
        paths_total,
        bytes_freed: 0,
    });

    for task in &plan.tasks {
        let item = &items[task.item];
        let (freed, ok) = clean_path(
            item,
            task,
            dry_run,
            permanent,
            &whitelist_set,
            &mut simctl_bulk_ran,
            &mut rec,
        );
        rec.result.bytes_freed += freed;
        item_ok[task.item] |= ok;
        path_ok.insert(task.info.path.as_str(), ok);
        paths_done += 1;
        on_progress(&CleanProgress {
            current_item: item.label.clone(),
            items_done: task.item,
            items_total,
            paths_done,
            paths_total,
            bytes_freed: rec.result.bytes_freed,
        });
    }

    for (i, path) in &plan.duplicates {
        item_ok[*i] |= path_ok.get(path).copied().unwrap_or(false);
    }
    for (i, item) in items.iter().enumerate() {
        if item_ok[i] {
            rec.result.items_cleaned += 1;
            rec.result.cleaned_ids.push(item.rule_id.clone());
        }
    }
    rec.result
}

/// Deletes one selected path. Returns (bytes freed, whether anything of it
/// was cleaned).
fn clean_path(
    item: &ScanItem,
    task: &Task,
    dry_run: bool,
    permanent: bool,
    whitelist_set: &HashSet<&str>,
    simctl_bulk_ran: &mut bool,
    rec: &mut Recorder,
) -> (u64, bool) {
    let path_info = task.info;
    let keep = Keep { whitelist: whitelist_set };

    // Skip safe-path / whitelist checks for pseudo-URIs
    // because they are not real filesystem paths.
    if !uses_pseudo_paths(&item.rule_id) {
        if !is_safe_path(&path_info.path) {
            let reason = "skipped: protected path (SIP / system directory)";
            shared::log_operation("CLEAN", &path_info.path, reason);
            rec.issue(item, &path_info.path, IssueReason::Protected, path_info.size, reason);
            return (0, false);
        }

        if is_whitelisted(&path_info.path, whitelist_set) {
            let reason = "skipped: on user whitelist";
            shared::log_operation("CLEAN", &path_info.path, reason);
            rec.issue(item, &path_info.path, IssueReason::Protected, path_info.size, reason);
            return (0, false);
        }

        if let Err(refusal) = data_guard::check_general(Path::new(&path_info.path)) {
            let reason = format!("skipped: {}", refusal);
            shared::log_operation("CLEAN", &path_info.path, &reason);
            rec.issue(item, &path_info.path, IssueReason::Protected, path_info.size, &reason);
            return (0, false);
        }
    }

    if dry_run {
        // A nested path is already counted in its selected ancestor.
        return (if task.nested { 0 } else { path_info.size }, true);
    }

    // Time Machine failed backups must be deleted through tmutil so the
    // TM catalogue stays consistent — we never touch .inProgress dirs
    // with filesystem calls.
    if item.rule_id == "special_tm_failed_backups" {
        return match tmutil_delete(&path_info.path) {
            Ok(()) => {
                shared::log_operation("CLEAN", &path_info.path, "tmutil delete");
                (path_info.size, true)
            }
            Err(e) => {
                shared::log_operation("CLEAN", &path_info.path, &format!("tmutil delete failed: {}", e));
                rec.issue(item, &path_info.path, IssueReason::Other, path_info.size, &e);
                (0, false)
            }
        };
    }

    // APFS local snapshots are removed via tmutil deletelocalsnapshots.
    // Paths for this rule are pseudo-URIs of the form `tmutil://<id>`.
    if item.rule_id == "special_tm_local_snapshots" {
        return match tmutil_delete_local_snapshot(&path_info.path) {
            Ok(()) => {
                shared::log_operation("CLEAN", &path_info.path, "tmutil deletelocalsnapshots");
                (path_info.size, true)
            }
            Err(e) => {
                shared::log_operation(
                    "CLEAN",
                    &path_info.path,
                    &format!("tmutil deletelocalsnapshots failed: {}", e),
                );
                rec.issue(item, &path_info.path, IssueReason::Other, path_info.size, &e);
                (0, false)
            }
        };
    }

    // Unavailable Xcode simulators are cleaned via `xcrun simctl delete unavailable`
    // with fallback to manual directory deletion. Paths are pseudo-URIs
    // of the form `simctl_unavailable://<UDID>`.
    if item.rule_id == "dev_xcode_unavailable_sims" {
        // Run the bulk command once rather than per-path.
        if !*simctl_bulk_ran {
            *simctl_bulk_ran = true;
            let simctl_ok = Command::new("/usr/bin/xcrun")
                .args(["simctl", "delete", "unavailable"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if simctl_ok {
                shared::log_operation("CLEAN", "xcrun simctl delete unavailable", "success");
            } else {
                shared::log_operation(
                    "CLEAN",
                    "xcrun simctl delete unavailable",
                    "failed, falling back to manual deletion",
                );
            }
        }
        // Try manual deletion of the device directory as fallback
        let device_dir = dirs::home_dir().and_then(|home| simctl_device_dir(&home, &path_info.path));
        return match device_dir {
            Some(device_dir) if device_dir.is_dir() => match safe_remove_dir_all(&device_dir) {
                Ok(_) => {
                    shared::log_operation("CLEAN", &path_info.path, "manual device dir removal");
                    (path_info.size, true)
                }
                Err(e) => {
                    shared::log_operation("CLEAN", &path_info.path, &format!("manual removal failed: {}", e));
                    let reason = issue_reason(&e);
                    rec.issue(item, &path_info.path, reason, size_on_disk(&device_dir), &e.to_string());
                    (0, false)
                }
            },
            // Device dir already removed by simctl
            Some(_) => (path_info.size, true),
            None if path_info.path.starts_with("simctl_unavailable://") => {
                rec.issue(item, &path_info.path, IssueReason::Other, path_info.size, "invalid simulator identifier");
                (0, false)
            }
            None => (0, false),
        };
    }

    let path = Path::new(&path_info.path);

    // For directories that are top-level containers (e.g. ~/Library/Caches),
    // delete contents instead of the directory itself to avoid permission errors
    // from macOS locking the parent directory.
    if path_info.is_dir && is_container_dir(&path_info.path) {
        let (freed, errs) = delete_dir_contents(path, permanent, keep);
        for (child, e) in &errs {
            rec.io_issue(item, child, e, 0);
        }
        return (freed, freed > 0);
    }

    if !permanent {
        if !exists_no_follow(path) {
            let e = io::Error::from_raw_os_error(libc::ENOENT);
            rec.io_issue(item, path, &e, path_info.size);
            return (0, false);
        }
        let r = trash_tree(path, keep);
        match &r.error {
            None => shared::log_operation("CLEAN", &path_info.path, "TRASHED"),
            Some(e) => rec.io_issue(item, path, e, path_info.size),
        }
        return (r.freed, r.error.is_none() || r.freed > 0);
    }

    let r = remove_tree(path, keep);
    match &r.error {
        None => shared::log_operation("CLEAN", &path_info.path, "DELETED"),
        // Gone before we got to it: neither freed nor failed.
        Some(e) if issue_reason(e) == IssueReason::AlreadyGone && r.freed == 0 => {
            rec.io_issue(item, path, e, path_info.size);
        }
        Some(e) if issue_reason(e) == IssueReason::AlreadyGone => {
            shared::log_operation("CLEAN", &path_info.path, "DELETED (removed concurrently)");
        }
        Some(e) => {
            if r.freed > 0 {
                shared::log_operation(
                    "CLEAN",
                    &path_info.path,
                    &format!("partly removed: {} bytes freed", r.freed),
                );
            }
            rec.io_issue(item, path, e, path_info.size);
        }
    }
    let ok = r.error.is_none() || r.freed > 0;
    (r.freed, ok)
}

/// Returns true if the path is a well-known container directory whose contents
/// should be deleted rather than the directory itself (macOS recreates these).
pub(super) fn is_container_dir(path: &str) -> bool {
    let home = match dirs::home_dir() {
        Some(h) => h.to_string_lossy().to_string(),
        None => return false,
    };

    let containers = [
        format!("{}/Library/Caches", home),
        format!("{}/Library/Logs", home),
        "/Library/Caches".to_string(),
        "/Library/Logs".to_string(),
    ];

    containers.iter().any(|c| path == c.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::browser_guard::fixtures as browser_fixtures;
    use crate::commands::data_guard::fixtures as data_fixtures;
    use crate::commands::cleaner::PathInfo;
    use crate::commands::test_support::{canon, mkdir, s, workspace_tempdir, write_file};
    use std::os::unix::fs::symlink;

    fn item(rule_id: &str, paths: &[(&Path, u64)]) -> ScanItem {
        let paths: Vec<PathInfo> = paths
            .iter()
            .map(|(p, size)| PathInfo {
                path: s(p),
                size: *size,
                is_dir: p.is_dir(),
            })
            .collect();
        ScanItem {
            rule_id: rule_id.into(),
            category: "Test".into(),
            label: rule_id.into(),
            total_size: paths.iter().map(|p| p.size).sum(),
            paths,
        }
    }

    fn raw_item(rule_id: &str, raw_paths: &[&str], is_dir: bool) -> ScanItem {
        ScanItem {
            rule_id: rule_id.into(),
            category: "Test".into(),
            label: rule_id.into(),
            paths: raw_paths
                .iter()
                .map(|p| PathInfo { path: p.to_string(), size: 100, is_dir })
                .collect(),
            total_size: 100 * raw_paths.len() as u64,
        }
    }

    fn run(items: &[ScanItem], dry_run: bool, whitelist: &[String]) -> CleanResult {
        execute_clean_items_with_whitelist(items, dry_run, true, whitelist, |_| {})
    }

    fn rule_items_for_home(home: &Path) -> Vec<ScanItem> {
        rule_items_for_home_with(home, &[])
    }

    fn rule_items_for_home_with(home: &Path, whitelist: &[String]) -> Vec<ScanItem> {
        use crate::commands::cleaner::{rules, scanner};
        let items = rules::all_rules()
            .into_iter()
            .filter_map(|mut rule| {
                // Absolute rule paths point at the real system; only the
                // home-relative ones can be aimed at the fake home.
                rule.paths.retain(|p| p.starts_with("~/"));
                scanner::collect_rule_paths_in(&rule, whitelist, Some(home))
            })
            .collect();
        scanner::drop_guard_refused_paths(items)
    }

    fn hostile_items(fx: &browser_fixtures::FakeBrowsers) -> Vec<ScanItem> {
        let paths: Vec<PathInfo> = fx
            .protected
            .keys()
            .chain(fx.protected_dirs.iter())
            .map(|p| PathInfo { path: s(p), size: 1, is_dir: p.is_dir() })
            .collect();
        vec![ScanItem {
            rule_id: "orphan_hostile".into(),
            category: "Test".into(),
            label: "hostile".into(),
            total_size: paths.len() as u64,
            paths,
        }]
    }

    #[test]
    fn every_cleaner_rule_on_a_fake_browser_home_removes_only_cache_leaves() {
        let dir = workspace_tempdir();
        let fx = browser_fixtures::build(&canon(&dir).join("home"));
        let items = rule_items_for_home(&fx.home);

        for item in &items {
            for p in &item.paths {
                let offered = Path::new(&p.path);
                for file in fx.protected.keys() {
                    assert!(!file.starts_with(offered), "rule {} offers {} which holds {}", item.rule_id, p.path, s(file));
                }
            }
        }

        let dry = run(&items, true, &[]);
        assert!(dry.bytes_freed > 0);
        fx.assert_profiles_intact();

        let real = run(&items, false, &[]);
        assert!(real.bytes_freed > 0, "{:?}", real.errors);
        fx.assert_profiles_intact();
        for rel in [
            "Library/Application Support/Google/Chrome/Default/Cache",
            "Library/Application Support/Google/Chrome/Default/Code Cache",
            "Library/Application Support/Google/Chrome/Default/GPUCache",
            "Library/Application Support/Google/Chrome/ShaderCache",
            "Library/Application Support/Google/Chrome/component_crx_cache",
            "Library/Application Support/BraveSoftware/Brave-Browser/GrShaderCache",
            "Library/Application Support/net.imput.helium/extensions_crx_cache",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/cache2",
            "Library/Application Support/Firefox/Profiles/efgh.dev-edition-default/cache2",
            "Library/Caches/Google/Chrome",
            "Library/Caches/com.apple.Safari",
            "Library/Caches/company.thebrowser.Browser",
        ] {
            assert!(!fx.home.join(rel).exists(), "{rel} should have been cleaned");
        }
    }

    #[test]
    fn executor_refuses_every_browser_profile_path_it_is_handed() {
        let dir = workspace_tempdir();
        let fx = browser_fixtures::build(&canon(&dir).join("home"));
        let items = hostile_items(&fx);
        let total = items[0].paths.len();

        let dry = run(&items, true, &[]);
        assert_eq!(dry.bytes_freed, 0);
        let real = run(&items, false, &[]);
        fx.assert_profiles_intact();
        assert_eq!(real.bytes_freed, 0);
        assert_eq!(real.items_cleaned, 0);
        let refused = real.errors.iter().filter(|e| e.contains(crate::commands::browser_guard::ERROR_CODE)).count();
        assert_eq!(refused, total, "{:#?}", real.errors);
    }

    #[test]
    fn every_cleaner_rule_on_a_fake_data_home_removes_only_cache_leaves() {
        let dir = workspace_tempdir();
        let fx = data_fixtures::build(&canon(&dir).join("home"));
        let items = rule_items_for_home(&fx.home);

        for item in &items {
            for p in &item.paths {
                for file in fx.protected.keys() {
                    assert!(!file.starts_with(&p.path), "rule {} offers {} which holds {}", item.rule_id, p.path, s(file));
                }
            }
        }
        run(&items, true, &[]);
        fx.assert_intact();
        let real = run(&items, false, &[]);
        assert!(real.bytes_freed > 0, "{:?}", real.errors);
        fx.assert_intact();
        for rel in [
            "Library/Application Support/Signal/Cache",
            "Library/Application Support/Slack/Cache",
            "Library/Application Support/Slack/Code Cache",
            "Library/Application Support/discord/Cache",
            "Library/Application Support/Microsoft/Teams/Cache",
            "Library/Application Support/Microsoft/Teams/logs",
            "Library/Messages/StickerCache",
            "Library/Messages/Caches/Previews/Attachments",
            "Library/Containers/com.microsoft.Word/Data/tmp",
            "Library/Application Support/Blackmagic Design/DaVinci Resolve/Cache",
            "Library/Application Support/Adobe/Common/Media Cache Files",
            "Library/Application Support/Notion/Cache",
            "Library/Application Support/Steam/appcache",
            "Library/Application Support/Steam/steamapps/shadercache",
            "Library/Application Support/minecraft/logs",
            "Library/Developer/Xcode/UserData/IB Support",
            ".aws/cli/cache",
            ".kube/cache",
            ".docker/buildx/cache",
        ] {
            assert!(!fx.path(rel).exists(), "{rel} should have been cleaned");
        }
    }

    #[test]
    fn executor_refuses_every_protected_path_it_is_handed() {
        let dir = workspace_tempdir();
        let fx = data_fixtures::build(&canon(&dir).join("home"));
        let paths: Vec<PathInfo> = data_fixtures::PROTECTED
            .iter()
            .map(|rel| fx.path(rel))
            .chain(fx.protected_dirs.iter().cloned())
            .map(|p| PathInfo { path: s(&p), size: 1, is_dir: p.is_dir() })
            .collect();
        let total = paths.len();
        let items = vec![ScanItem {
            rule_id: "orphan_hostile".into(),
            category: "Test".into(),
            label: "hostile".into(),
            total_size: total as u64,
            paths,
        }];

        let real = run(&items, false, &[]);
        fx.assert_intact();
        assert_eq!(real.bytes_freed, 0);
        assert_eq!(real.items_cleaned, 0);
        assert_eq!(real.errors.len(), total, "{:#?}", real.errors);
    }

    #[test]
    fn orphan_scan_offers_only_caches_logs_and_window_state_of_gone_apps() {
        use crate::commands::cleaner::scanner;
        use crate::commands::test_support::set_age_days;
        let dir = workspace_tempdir();
        let home = canon(&dir).join("home");
        let bx = browser_fixtures::build(&home);
        let fx = data_fixtures::build(&home);
        let offer = [
            "Library/Caches/com.example.goneapp",
            "Library/Logs/com.example.goneapp",
            "Library/Saved Application State/com.example.goneapp.savedState",
        ];
        let never = [
            "Library/Application Support/com.example.goneapp",
            "Library/Containers/com.example.goneapp",
            "Library/Group Containers/ABCDE12345.com.example.goneapp",
            "Library/Preferences/com.example.goneapp.plist",
            "Library/Caches/com.example.gonewallet",
            "Library/Caches/ru.keepcoder.Telegram",
        ];
        for rel in offer.iter().chain(&never) {
            write_file(&home.join(rel).join("data.bin"), 4096);
        }
        write_file(&home.join("Library/Caches/com.example.gonewallet/keys/wallet.dat"), 4096);
        for rel in offer.iter().chain(&never) {
            set_age_days(&home.join(rel), 400);
        }

        let items = scanner::scan_orphaned_data_in(&home, &HashSet::new(), &[], &|_| false);
        let mut offered: Vec<String> = items.iter().flat_map(|i| i.paths.iter().map(|p| p.path.clone())).collect();
        offered.sort();
        let mut want: Vec<String> = offer.iter().map(|rel| s(&home.join(rel))).collect();
        want.sort();
        assert_eq!(offered, want);

        run(&items, false, &[]);
        bx.assert_profiles_intact();
        fx.assert_intact();
        for rel in never {
            assert!(home.join(rel).join("data.bin").exists(), "{rel}");
        }
    }

    #[test]
    fn permanent_clean_removes_files_and_directories() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let file = root.join("old.log");
        write_file(&file, 1_000);
        let cache = root.join("cache");
        write_file(&cache.join("a/b.bin"), 2_000);

        let on_disk = 1_000 + dir_size(&cache);
        let result = run(&[item("r1", &[(&file, 1_000), (&cache, 2_000)])], false, &[]);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.bytes_freed, on_disk, "freed is measured, not taken from the scan");
        assert!(result.failed.is_empty() && result.already_gone.is_empty());
        assert_eq!(result.items_cleaned, 1);
        assert_eq!(result.cleaned_ids, vec!["r1".to_string()]);
        assert!(!file.exists());
        assert!(!cache.exists());
        assert!(root.exists());
    }

    #[test]
    fn dry_run_counts_bytes_but_deletes_nothing() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let file = root.join("x.log");
        write_file(&file, 500);

        let result = run(&[item("r1", &[(&file, 500)])], true, &[]);

        assert_eq!(result.bytes_freed, 500);
        assert_eq!(result.items_cleaned, 1);
        assert!(file.exists());
    }

    #[test]
    fn protected_and_critical_paths_are_refused() {
        let mut paths = vec![
            "/".to_string(),
            "/System/Library/Caches".to_string(),
            "/system/library/caches".to_string(),
            "/usr/lib".to_string(),
            "/private/var/db".to_string(),
            "/Applications".to_string(),
            "/Users".to_string(),
            "/Library/Caches/../../System".to_string(),
        ];
        if let Some(home) = dirs::home_dir() {
            paths.push(s(&home));
            paths.push(s(&home.join("Documents")));
            paths.push(s(&home.join("Library")));
        }
        let refs: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();
        // dry_run keeps this harmless even if a guard regressed.
        let result = run(&[raw_item("r1", &refs, true)], true, &[]);

        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors.len(), paths.len(), "{:?}", result.errors);
        assert!(result.errors.iter().all(|e| e.contains("protected path")));
    }

    #[test]
    fn whitelisted_paths_and_their_children_are_kept() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let keep = root.join("keep");
        write_file(&keep.join("inner/data.bin"), 100);
        let keep_child = keep.join("inner");
        let near_miss = root.join("keeper.bin");
        write_file(&near_miss, 100);

        let whitelist = vec![s(&keep)];
        let result = run(
            &[item("r1", &[(&keep, 100), (&keep_child, 100), (&near_miss, 100)])],
            false,
            &whitelist,
        );

        assert!(keep_child.join("data.bin").exists());
        assert!(!near_miss.exists(), "sibling sharing a name prefix is not whitelisted");
        assert_eq!(result.bytes_freed, 100);
        assert_eq!(
            result.errors.iter().filter(|e| e.contains("whitelist")).count(),
            2
        );
    }

    #[test]
    fn protected_user_data_subdirectories_survive() {
        let dir = workspace_tempdir();
        let profile = canon(&dir).join("Profile");
        write_file(&profile.join("Cache/blob.bin"), 1_000);
        write_file(&profile.join("IndexedDB/db.bin"), 1_000);
        write_file(&profile.join("nested/Local Storage/ls.bin"), 1_000);

        run(&[item("r1", &[(&profile, 3_000)])], false, &[]);

        assert!(!profile.join("Cache").exists());
        assert!(profile.join("IndexedDB/db.bin").exists());
        assert!(profile.join("nested/Local Storage/ls.bin").exists());
    }

    #[test]
    fn protected_component_as_root_is_never_removed() {
        let dir = workspace_tempdir();
        let sw = canon(&dir).join("Service Worker");
        write_file(&sw.join("x.bin"), 10);
        assert_eq!(safe_remove_dir_all(&sw).unwrap(), false);
        assert!(sw.join("x.bin").exists());
    }

    #[test]
    fn symlinks_are_unlinked_never_followed() {
        let outside = workspace_tempdir();
        let target = canon(&outside).join("precious");
        write_file(&target.join("file.txt"), 10);

        let dir = workspace_tempdir();
        let root = canon(&dir);
        let cache = root.join("cache");
        write_file(&cache.join("junk.bin"), 10);
        symlink(&target, cache.join("link_to_precious")).unwrap();
        let top_link = root.join("linked_cache");
        symlink(&target, &top_link).unwrap();

        let result = run(&[item("r1", &[(&cache, 10), (&top_link, 10)])], false, &[]);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(target.join("file.txt").exists());
        assert!(!cache.exists());
        assert!(fs::symlink_metadata(&top_link).is_err());
    }

    #[test]
    fn missing_paths_are_reported_not_counted() {
        let dir = workspace_tempdir();
        let gone = canon(&dir).join("gone.bin");
        let result = run(&[raw_item("r1", &[&s(&gone)], false)], false, &[]);
        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors.len(), 1);
        // Already gone is neither freed nor failed.
        assert_eq!(result.bytes_failed, 0);
        assert!(result.failed.is_empty());
        assert_eq!(result.already_gone.len(), 1);
        assert_eq!(result.already_gone[0].reason, IssueReason::AlreadyGone);
        assert_eq!(result.already_gone[0].path, s(&gone));
    }

    #[test]
    fn snapshot_rule_rejects_non_snapshot_paths_without_touching_them() {
        let dir = workspace_tempdir();
        let file = canon(&dir).join("important.txt");
        write_file(&file, 10);
        let p = s(&file);
        let result = run(
            &[raw_item("special_tm_local_snapshots", &[&p, "tmutil://not-a-snapshot"], false)],
            false,
            &[],
        );
        assert!(file.exists());
        assert_eq!(result.items_cleaned, 0);
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn progress_is_reported_from_start_to_finish() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let (a, b) = (root.join("a"), root.join("b"));
        write_file(&a, 10);
        write_file(&b, 10);
        let mut events = Vec::new();
        execute_clean_items_with_whitelist(
            &[item("r1", &[(&a, 10)]), item("r2", &[(&b, 10)])],
            false,
            true,
            &[],
            |p| events.push((p.paths_done, p.paths_total, p.bytes_freed)),
        );
        assert_eq!(events.first(), Some(&(0, 2, 0)));
        assert_eq!(events.last(), Some(&(2, 2, 20)));
    }

    #[test]
    fn container_dirs_are_emptied_in_place() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        write_file(&root.join("a.bin"), 100);
        write_file(&root.join("sub/b.bin"), 100);
        write_file(&root.join("IndexedDB/keep.bin"), 100);
        let outside = workspace_tempdir();
        write_file(&canon(&outside).join("keep.txt"), 1);
        symlink(canon(&outside), root.join("link")).unwrap();

        let (freed, errs) = delete_dir_contents(&root, true, Keep { whitelist: &HashSet::new() });

        assert!(errs.is_empty(), "{errs:?}");
        assert!(freed > 0);
        assert!(root.exists());
        assert!(!root.join("a.bin").exists());
        assert!(!root.join("sub").exists());
        assert!(root.join("IndexedDB/keep.bin").exists());
        assert!(canon(&outside).join("keep.txt").exists());
    }

    #[test]
    fn container_dir_detection_is_exact() {
        assert!(is_container_dir("/Library/Caches"));
        assert!(is_container_dir("/Library/Logs"));
        assert!(!is_container_dir("/private/var/log"));
        assert!(!is_container_dir("/Library/Caches/com.foo"));
        assert!(!is_container_dir("/Library"));
        if let Some(home) = dirs::home_dir() {
            assert!(is_container_dir(&s(&home.join("Library/Caches"))));
            assert!(is_container_dir(&s(&home.join("Library/Logs"))));
            assert!(!is_container_dir(&s(&home.join("Library"))));
        }
    }

    #[test]
    fn whitelist_matching_is_component_aware() {
        let wl: HashSet<&str> = ["/a/b"].into_iter().collect();
        assert!(is_whitelisted("/a/b", &wl));
        assert!(is_whitelisted("/a/b/c", &wl));
        assert!(!is_whitelisted("/a/bc", &wl));
        assert!(!is_whitelisted("/a", &wl));
    }

    #[test]
    fn snapshot_dates_are_extracted_strictly() {
        assert_eq!(
            extract_snapshot_date("com.apple.TimeMachine.2025-11-02-120000.local").as_deref(),
            Some("2025-11-02-120000")
        );
        assert_eq!(extract_snapshot_date("com.apple.TimeMachine..local"), None);
        assert_eq!(extract_snapshot_date("com.apple.TimeMachine.2025-11-02"), None);
        assert_eq!(extract_snapshot_date("/Users/x/Documents"), None);
    }

    #[test]
    fn invalid_snapshot_pseudo_paths_fail_before_running_tmutil() {
        assert!(tmutil_delete_local_snapshot("/Users/x/Documents").is_err());
        assert!(tmutil_delete_local_snapshot("tmutil://garbage").is_err());
    }

    #[test]
    fn simulator_udids_cannot_escape_the_devices_dir() {
        let home = Path::new("/Users/tester");
        assert_eq!(
            simctl_device_dir(home, "simctl_unavailable://A1B2C3D4-0000-1111-2222-333344445555"),
            Some(home.join("Library/Developer/CoreSimulator/Devices/A1B2C3D4-0000-1111-2222-333344445555"))
        );
        for bad in [
            "simctl_unavailable://../../../../Documents",
            "simctl_unavailable://",
            "simctl_unavailable://abc/def",
            "simctl_unavailable:///etc",
            "/Users/tester/Library/Developer/CoreSimulator/Devices/ABC",
        ] {
            assert_eq!(simctl_device_dir(home, bad), None, "{bad}");
        }
    }

    #[test]
    fn delete_errors_get_actionable_hints() {
        let e = std::io::Error::from_raw_os_error(1);
        assert!(classify_delete_error(&e).contains("SIP"));
        let e = std::io::Error::from_raw_os_error(16);
        assert!(classify_delete_error(&e).contains("in use"));
        let e = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        assert!(classify_delete_error(&e).contains("already removed"));
        let e = std::io::Error::new(std::io::ErrorKind::Other, "weird");
        assert_eq!(classify_delete_error(&e), "weird");
    }

    #[test]
    fn empty_directory_tree_is_removed() {
        let dir = workspace_tempdir();
        let tree = canon(&dir).join("t");
        mkdir(&tree.join("a/b/c"));
        assert_eq!(safe_remove_dir_all(&tree).unwrap(), true);
        assert!(!tree.exists());
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn mode_of(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    /// Makes a file immutable (`chflags uchg`) so unlinking it fails with
    /// EPERM, and clears the flag again on drop so the temp dir can go.
    struct Immutable(std::path::PathBuf);

    impl Immutable {
        fn new(path: &Path) -> Self {
            Immutable(path.to_path_buf()).set(libc::UF_IMMUTABLE)
        }
        fn set(self, flags: libc::c_uint) -> Self {
            let c = CString::new(self.0.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::chflags(c.as_ptr(), flags) }, 0, "chflags {}", self.0.display());
            self
        }
    }

    impl Drop for Immutable {
        fn drop(&mut self) {
            let c = CString::new(self.0.as_os_str().as_bytes()).unwrap();
            unsafe { libc::chflags(c.as_ptr(), 0) };
        }
    }

    /// npm writes parts of `_cacache` read-only; the old recursive delete
    /// stopped at the first EACCES and left the whole cache behind.
    fn npm_like_cache(root: &Path) {
        write_file(&root.join("content-v2/sha512/ab/cd/blob1"), 50_000);
        write_file(&root.join("content-v2/sha512/ab/ef/blob2"), 50_000);
        write_file(&root.join("index-v5/aa/bb/entry"), 1_000);
        write_file(&root.join("tmp/partial"), 10);
        set_mode(&root.join("content-v2/sha512/ab/cd"), 0o555);
        set_mode(&root.join("content-v2/sha512/ab"), 0o555);
        set_mode(&root.join("index-v5/aa/bb"), 0o500);
        set_mode(&root.join("index-v5/aa/bb/entry"), 0o444);
    }

    #[test]
    fn read_only_subdirectories_inside_an_approved_path_are_cleared() {
        let dir = workspace_tempdir();
        let root = canon(&dir).join("_cacache");
        npm_like_cache(&root);
        let on_disk = dir_size(&root);

        let result = run(&[item("dev_npm_cache", &[(&root, on_disk)])], false, &[]);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(result.failed.is_empty());
        assert!(!root.exists());
        assert_eq!(result.bytes_freed, on_disk);
        assert_eq!(result.cleaned_ids, vec!["dev_npm_cache".to_string()]);
    }

    #[test]
    fn owner_write_is_never_added_outside_the_approved_path() {
        let dir = workspace_tempdir();
        let base = canon(&dir);

        // The approved path sits in a read-only parent: its contents go, but
        // removing the folder itself needs the parent, which we leave alone.
        let parent = base.join("locked-parent");
        let root = parent.join("cache");
        npm_like_cache(&root);
        set_mode(&parent, 0o555);

        // A symlink inside points at a read-only folder elsewhere.
        let outside = base.join("outside");
        write_file(&outside.join("precious.txt"), 100);
        set_mode(&outside, 0o555);
        symlink(&outside, root.join("tmp/link")).unwrap();

        let result = run(&[item("r1", &[(&root, dir_size(&root))])], false, &[]);

        assert_eq!(mode_of(&parent), 0o555, "parent of the approved path was chmodded");
        assert_eq!(mode_of(&outside), 0o555, "symlink target was chmodded");
        assert!(outside.join("precious.txt").exists());
        assert!(root.exists() && fs::read_dir(&root).unwrap().next().is_none(), "contents cleared");
        assert!(result.bytes_freed >= 101_010);
        assert_eq!(result.failed.len(), 1, "{:?}", result.failed);
        assert_eq!(result.failed[0].reason, IssueReason::NoPermission);
        assert_eq!(result.failed[0].path, s(&root));

        set_mode(&parent, 0o755);
        set_mode(&outside, 0o755);
    }

    #[test]
    fn an_unremovable_entry_no_longer_strands_the_rest_and_is_reported() {
        let dir = workspace_tempdir();
        let root = canon(&dir).join("cache");
        write_file(&root.join("a/one.bin"), 30_000);
        write_file(&root.join("b/stuck.bin"), 8_192);
        write_file(&root.join("c/two.bin"), 30_000);
        let _stuck = Immutable::new(&root.join("b/stuck.bin"));
        let before = dir_size(&root);
        let stuck_size = dir_size(&root.join("b"));

        let result = run(&[item("r1", &[(&root, before)])], false, &[]);

        assert!(!root.join("a").exists() && !root.join("c").exists(), "the rest was removed");
        assert!(root.join("b/stuck.bin").exists());
        assert_eq!(result.bytes_freed, before - stuck_size);
        assert_eq!(result.failed.len(), 1);
        let f = &result.failed[0];
        assert_eq!((f.rule_id.as_str(), f.path.as_str()), ("r1", s(&root).as_str()));
        assert_eq!(f.reason, IssueReason::NoPermission);
        assert_eq!(f.size, stuck_size);
        assert_eq!(result.bytes_failed, stuck_size);
        assert!(result.already_gone.is_empty());
        assert_eq!(result.items_cleaned, 1, "partly freed still counts as cleaned");
    }

    #[test]
    fn a_failure_inside_a_selected_parent_is_counted_once() {
        let dir = workspace_tempdir();
        let parent = canon(&dir).join("Caches");
        let child = parent.join("pip");
        write_file(&parent.join("other/x.bin"), 20_000);
        write_file(&child.join("wheels/stuck.whl"), 8_192);
        let _stuck = Immutable::new(&child.join("wheels/stuck.whl"));
        let stuck_size = dir_size(&child);

        let result = run(
            &[item("user_caches", &[(&parent, dir_size(&parent))]), item("dev_pip_cache", &[(&child, stuck_size)])],
            false,
            &[],
        );

        assert_eq!(result.bytes_failed, stuck_size);
        assert_eq!(result.failed.len(), 1, "{:?}", result.failed);
        assert_eq!(result.failed[0].rule_id, "dev_pip_cache");
        assert!(!parent.join("other").exists());
    }

    #[test]
    fn nested_and_duplicate_paths_are_freed_and_counted_once() {
        let dir = workspace_tempdir();
        let caches = canon(&dir).join("Caches");
        let pip = caches.join("pip");
        let brew = caches.join("Homebrew");
        let sentry = caches.join("SentryCrash");
        let warp = sentry.join("Warp");
        write_file(&pip.join("http/a.bin"), 40_000);
        write_file(&brew.join("downloads/b.tar.gz"), 60_000);
        write_file(&warp.join("c.json"), 5_000);
        write_file(&sentry.join("d.json"), 5_000);
        write_file(&caches.join("misc/e.bin"), 10_000);
        let other = canon(&dir).join("CoreDevice");
        write_file(&other.join("f.bin"), 7_000);
        let union = dir_size(&caches) + dir_size(&other);

        // Listed in rule order, like a real scan: the parent first, then
        // children with their own rules, and one folder under two rules.
        let items = [
            item("user_caches", &[(&caches, dir_size(&caches))]),
            item("dev_pip_cache", &[(&pip, dir_size(&pip))]),
            item("util_homebrew", &[(&brew, dir_size(&brew))]),
            item("dev_sentry_crash", &[(&sentry, dir_size(&sentry))]),
            item("shell_warp_cache", &[(&warp, dir_size(&warp))]),
            item("sys_coredevice_cache", &[(&other, dir_size(&other))]),
            item("dynamic_container_caches", &[(&other, dir_size(&other))]),
        ];
        let listed: u64 = items.iter().map(|i| i.total_size).sum();
        assert!(listed > union, "the fixture overlaps");

        let dry = run(&items, true, &[]);
        assert_eq!(dry.bytes_freed, union, "dry run counts each byte once");

        let result = run(&items, false, &[]);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(result.already_gone.is_empty(), "overlaps are not 'already gone': {:?}", result.already_gone);
        assert!(result.failed.is_empty());
        assert_eq!(result.bytes_freed, union);
        assert!(!caches.exists() && !other.exists());
        assert_eq!(result.items_cleaned, items.len());
    }

    #[test]
    fn plan_runs_nested_paths_first_and_drops_duplicates() {
        let items = [
            raw_item("user_caches", &["/h/Library/Caches"], true),
            raw_item("dev_pip_cache", &["/h/Library/Caches/pip"], true),
            raw_item("dev_sentry_crash", &["/h/Library/Caches/SentryCrash"], true),
            raw_item("shell_warp_cache", &["/h/Library/Caches/SentryCrash/Warp", "/h/Library/Caches Old"], true),
            raw_item("dup", &["/h/Library/Caches/pip"], true),
            raw_item("special_tm_local_snapshots", &["tmutil://com.apple.TimeMachine.x.local"], false),
        ];
        let plan = plan_tasks(&items);
        let order: Vec<&str> = plan.tasks.iter().map(|t| t.info.path.as_str()).collect();
        assert_eq!(
            order,
            [
                "/h/Library/Caches/SentryCrash/Warp",
                "/h/Library/Caches/pip",
                "/h/Library/Caches/SentryCrash",
                "/h/Library/Caches",
                "/h/Library/Caches Old",
                "tmutil://com.apple.TimeMachine.x.local",
            ]
        );
        assert_eq!(plan.duplicates, vec![(4, "/h/Library/Caches/pip")]);
    }

    #[test]
    fn issue_reasons_use_plain_categories() {
        let r = |code| issue_reason(&std::io::Error::from_raw_os_error(code));
        assert_eq!(r(libc::ENOENT), IssueReason::AlreadyGone);
        assert_eq!(r(libc::EACCES), IssueReason::NoPermission);
        assert_eq!(r(libc::EPERM), IssueReason::NoPermission);
        assert_eq!(r(libc::EBUSY), IssueReason::InUse);
        assert_eq!(r(libc::ENOTEMPTY), IssueReason::InUse);
        assert_eq!(r(libc::EIO), IssueReason::Other);
    }

    #[test]
    fn read_only_caches_on_a_fake_data_home_are_cleared_and_protected_data_is_untouched() {
        let dir = workspace_tempdir();
        let fx = data_fixtures::build(&canon(&dir).join("home"));
        npm_like_cache(&fx.path(".npm/_cacache"));
        write_file(&fx.path("Library/Caches/pip/http/a.bin"), 4_096);
        set_mode(&fx.path("Library/Caches/pip/http"), 0o555);
        // A read-only protected folder must keep both its data and its mode.
        let locked: Vec<std::path::PathBuf> =
            fx.protected_dirs.iter().filter(|d| d.is_dir()).take(3).cloned().collect();
        assert!(!locked.is_empty());
        for d in &locked {
            set_mode(d, 0o555);
        }

        let items = rule_items_for_home(&fx.home);
        let result = run(&items, false, &[]);

        for d in &locked {
            assert_eq!(mode_of(d), 0o555, "{} was chmodded", d.display());
            set_mode(d, 0o755);
        }
        fx.assert_intact();
        assert!(!fx.path(".npm/_cacache").exists(), "{:?}", result.errors);
        assert!(!fx.path("Library/Caches/pip").exists(), "{:?}", result.errors);
        assert!(result.already_gone.is_empty(), "{:?}", result.already_gone);
    }

    #[test]
    fn whitelisted_folders_inside_an_emptied_parent_survive_and_are_not_counted() {
        use crate::commands::cleaner::rules;
        let dir = workspace_tempdir();
        let fx = data_fixtures::build(&canon(&dir).join("home"));
        let caches = fx.path("Library/Caches");
        write_file(&caches.join("foo/keep.bin"), 300_000);
        write_file(&caches.join("bar/junk.bin"), 50_000);
        write_file(&caches.join("baz/inner/keep.bin"), 200_000);
        write_file(&caches.join("baz/junk.bin"), 20_000);
        let whitelist = vec![s(&caches.join("foo")), s(&caches.join("baz/inner"))];

        let user_caches = rules::all_rules().into_iter().find(|r| r.id == "user_caches").unwrap();
        let unfiltered = crate::commands::cleaner::scanner::collect_rule_paths_in(&user_caches, &[], Some(&fx.home)).unwrap();
        let items = vec![crate::commands::cleaner::scanner::collect_rule_paths_in(&user_caches, &whitelist, Some(&fx.home)).unwrap()];
        let kept = dir_size(&caches.join("foo")) + dir_size(&caches.join("baz/inner"));
        assert_eq!(items[0].total_size, unfiltered.total_size - kept, "whitelisted bytes are not offered");

        let result = run(&items, false, &whitelist);

        assert!(caches.join("foo/keep.bin").exists());
        assert!(caches.join("baz/inner/keep.bin").exists());
        assert!(!caches.join("bar").exists() && !caches.join("baz/junk.bin").exists());
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.bytes_freed, items[0].total_size, "freed matches the offered size");
        fx.assert_intact();
    }

    #[test]
    fn emptying_a_container_skips_whitelisted_guarded_and_kyra_children() {
        use crate::commands::cleaner::scanner;
        let dir = workspace_tempdir();
        let logs = canon(&dir).join("Library/Logs");
        write_file(&logs.join("Kyra/operations.log"), 10_000);
        write_file(&logs.join("keep_me/a.log"), 10_000);
        write_file(&logs.join("project/.git/HEAD"), 10_000);
        write_file(&logs.join("wallet.dat"), 10_000);
        write_file(&logs.join("app/old.log"), 7_000);
        write_file(&logs.join("loose.log"), 3_000);
        let whitelist = vec![s(&logs.join("keep_me"))];
        let wl: HashSet<&str> = whitelist.iter().map(|w| w.as_str()).collect();

        let offered = scanner::container_size_for_tests(&logs, &whitelist);
        assert_eq!(offered, dir_size(&logs.join("app")) + 3_000);

        let (freed, errs) = delete_dir_contents(&logs, true, Keep { whitelist: &wl });

        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(freed, offered);
        for kept in ["Kyra/operations.log", "keep_me/a.log", "project/.git/HEAD", "wallet.dat"] {
            assert!(logs.join(kept).exists(), "{kept} was removed");
        }
        assert!(!logs.join("app").exists() && !logs.join("loose.log").exists());
    }

    #[test]
    fn kyra_never_cleans_its_own_log_settings_or_caches() {
        use crate::commands::cleaner::scanner;
        use crate::commands::test_support::set_age_days;
        // A plain home, so rules that take a whole parent folder (User
        // Caches, Saved Application State) are offered and have to step
        // around Kyra's folders inside it.
        let dir = workspace_tempdir();
        let home = canon(&dir).join("home");
        let path = |rel: &str| home.join(rel);
        write_file(&path("Library/Caches/com.example.app/junk.bin"), 50_000);
        write_file(&path("Library/Logs/com.example.app/old.log"), 5_000);
        write_file(&path("Library/Saved Application State/com.example.app.savedState/w.plist"), 1_000);
        set_age_days(&path("Library/Logs/com.example.app"), 400);
        let own = [
            "Library/Logs/Kyra/operations.log",
            "Library/Application Support/com.kyra.app/settings.json",
            "Library/Application Support/com.kyra.app/license.json",
            "Library/Caches/com.kyra.app/analyzer/overview.json",
            "Library/Caches/com.eleventribes.kyra/WebKit/blob",
            "Library/WebKit/com.eleventribes.kyra/WebsiteData/x",
            "Library/Saved Application State/com.eleventribes.kyra.savedState/windows.plist",
            ".cache/kyra/brew_last_cleanup",
        ];
        for rel in own {
            write_file(&path(rel), 4_096);
        }
        // Old enough for every age-filtered rule (User Logs: 7 days).
        for rel in ["Library/Logs/Kyra", "Library/Caches/com.kyra.app", "Library/Saved Application State/com.eleventribes.kyra.savedState"] {
            set_age_days(&path(rel), 400);
        }

        let mut items = rule_items_for_home(&home);
        items.extend(scanner::scan_orphaned_data_in(&home, &HashSet::new(), &[], &|_| false));
        for item in &items {
            for p in &item.paths {
                assert!(!data_guard::is_own_data(Path::new(&p.path)), "rule {} offers {}", item.rule_id, p.path);
            }
        }

        let result = run(&items, false, &[]);
        assert!(result.bytes_freed > 0);
        for rel in own {
            assert!(path(rel).exists(), "{rel} was removed");
        }
        assert!(!path("Library/Caches/com.example.app").exists(), "{:?}", result.errors);
        let offered = |needle: &str| items.iter().flat_map(|i| &i.paths).any(|p| p.path.ends_with(needle));
        assert!(offered("Library/Caches"), "the parent folder is offered whole");

        // Handed directly, every one is refused.
        let direct: Vec<(std::path::PathBuf, u64)> = own.iter().map(|rel| (path(rel), 1)).collect();
        let refs: Vec<(&Path, u64)> = direct.iter().map(|(p, n)| (p.as_path(), *n)).collect();
        let refused = run(&[item("hostile", &refs)], false, &[]);
        assert_eq!(refused.bytes_freed, 0);
        assert_eq!(refused.failed.len(), own.len());
        assert!(refused.failed.iter().all(|f| f.reason == IssueReason::Protected));
        for rel in own {
            assert!(path(rel).exists(), "{rel} was removed");
        }
    }
}
