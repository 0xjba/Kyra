use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Temp dir under `target/` rather than `$TMPDIR`: `$TMPDIR` resolves into
/// `/private/var`, which several delete guards (correctly) refuse outright.
pub fn workspace_tempdir() -> tempfile::TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/kyra-test-tmp");
    fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new().prefix("t").tempdir_in(base).unwrap()
}

/// Canonical path of a temp dir, so string comparisons match what the
/// guards compute after `canonicalize`.
pub fn canon(dir: &tempfile::TempDir) -> PathBuf {
    fs::canonicalize(dir.path()).unwrap()
}

pub fn write_file(path: &Path, len: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, vec![b'x'; len]).unwrap();
}

pub fn mkdir(path: &Path) {
    fs::create_dir_all(path).unwrap();
}

pub fn set_age_days(path: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 86_400);
    let f = fs::File::open(path).unwrap();
    f.set_times(fs::FileTimes::new().set_modified(t).set_accessed(t))
        .unwrap();
}

/// Marks `path` as downloaded from the internet, like a browser does.
pub fn set_quarantine(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let value = b"0083;66f00000;Safari;";
    let rc = unsafe {
        libc::setxattr(
            c_path.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    assert_eq!(rc, 0, "setxattr failed on {}", path.display());
}

pub fn s(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
