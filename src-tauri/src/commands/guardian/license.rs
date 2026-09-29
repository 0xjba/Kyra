use super::types::LicenseStatus;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn app_data_dir() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("com.kyra.app");
    path
}

fn license_cache_path(dir: &Path) -> PathBuf {
    let _ = fs::create_dir_all(dir);
    dir.join("guardian_license.json")
}

// v2: ids used to be derived from the hardware UUID, which any local process can recompute.
// The worker treats the id as a bearer secret for account/cancel, so it must be random.
const DEVICE_ID_FILE: &str = "device_id.v2";

fn device_id_path(dir: &Path) -> PathBuf {
    let _ = fs::create_dir_all(dir);
    dir.join(DEVICE_ID_FILE)
}

pub fn get_or_create_device_id() -> String {
    get_or_create_device_id_in(&app_data_dir())
}

pub(crate) fn get_or_create_device_id_in(dir: &Path) -> String {
    let path = device_id_path(dir);
    if let Ok(id) = fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = generate_device_id();
    write_private(&path, &id);
    id
}

fn write_private(path: &Path, contents: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Ok(mut f) = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
    {
        let _ = f.write_all(contents.as_bytes());
    }
}

fn generate_device_id() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    let filled = fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_ok();
    assert!(filled, "no OS randomness available for the device id");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub async fn check_license(device_id: &str) -> Result<LicenseStatus, String> {
    check_license_with(super::http(), &super::worker_url(), device_id, &app_data_dir()).await
}

pub(crate) async fn check_license_with(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
    data_dir: &Path,
) -> Result<LicenseStatus, String> {
    match check_license_remote(client, base_url, device_id).await {
        Ok(status) => {
            cache_license(data_dir, &status);
            Ok(status)
        }
        Err(_) => Ok(load_cached_license(data_dir, now_secs())),
    }
}

async fn check_license_remote(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
) -> Result<LicenseStatus, String> {
    let resp = client
        .get(format!("{}/license", base_url))
        .query(&[("device_id", device_id)])
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("License check failed: {}", resp.status()));
    }

    resp.json::<LicenseStatus>()
        .await
        .map_err(|e| e.to_string())
}

pub(crate) fn cache_license(dir: &Path, status: &LicenseStatus) {
    let path = license_cache_path(dir);
    if let Ok(json) = serde_json::to_string(status) {
        let _ = fs::write(path, json);
    }
}

/// The same license source the scan path uses: remote first, cached status when offline.
pub async fn require_license() -> Result<(), String> {
    let device_id = get_or_create_device_id();
    require_license_with(super::http(), &super::worker_url(), &device_id, &app_data_dir()).await
}

pub(crate) async fn require_license_with(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
    data_dir: &Path,
) -> Result<(), String> {
    let status = check_license_with(client, base_url, device_id, data_dir).await?;
    ensure_active(&status, now_secs())
}

// Messages match the worker's 403 bodies, which the frontend maps to its paywall.
pub(crate) fn ensure_active(status: &LicenseStatus, now: u64) -> Result<(), String> {
    match status.expires {
        Some(exp) if exp < now => Err("License expired".into()),
        _ if status.active => Ok(()),
        _ => Err("No active license".into()),
    }
}

/// Offline-only check for background gating; cleaning still goes through `require_license`.
pub(crate) fn cached_license_active_in(dir: &Path, now: u64) -> bool {
    ensure_active(&load_cached_license(dir, now), now).is_ok()
}

pub(crate) fn cached_license_active() -> bool {
    cached_license_active_in(&app_data_dir(), now_secs())
}

fn inactive() -> LicenseStatus {
    LicenseStatus {
        active: false,
        expires: None,
    }
}

fn load_cached_license(dir: &Path, now: u64) -> LicenseStatus {
    let path = license_cache_path(dir);
    let status = match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| inactive()),
        Err(_) => inactive(),
    };
    // Offline fallback must not keep an expired subscription alive.
    match status.expires {
        Some(exp) if exp < now => LicenseStatus {
            active: false,
            expires: status.expires,
        },
        _ => status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::guardian::test_support::{
        block_on, client, dead_url, serve_once, TestDir,
    };

    #[test]
    fn device_id_is_sha256_hex() {
        let id = generate_device_id();
        assert_eq!(id.len(), 64);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn device_id_is_created_and_persisted() {
        let dir = TestDir::new("devid");
        let data = dir.path().join("com.kyra.app");
        let first = get_or_create_device_id_in(&data);
        assert_eq!(fs::read_to_string(data.join(DEVICE_ID_FILE)).unwrap(), first);
        assert_eq!(get_or_create_device_id_in(&data), first);
    }

    #[test]
    fn existing_device_id_is_trimmed_and_reused() {
        let dir = TestDir::new("devid-existing");
        fs::write(dir.path().join(DEVICE_ID_FILE), "  abc123\n").unwrap();
        assert_eq!(get_or_create_device_id_in(dir.path()), "abc123");
    }

    #[test]
    fn blank_device_id_file_is_regenerated() {
        let dir = TestDir::new("devid-blank");
        fs::write(dir.path().join(DEVICE_ID_FILE), "\n").unwrap();
        let id = get_or_create_device_id_in(dir.path());
        assert_eq!(id.len(), 64);
        assert_eq!(
            fs::read_to_string(dir.path().join(DEVICE_ID_FILE)).unwrap(),
            id
        );
    }

    #[test]
    fn parses_worker_license_shapes() {
        let active: LicenseStatus =
            serde_json::from_str(r#"{"active":true,"expires":1900000000}"#).unwrap();
        assert!(active.active);
        assert_eq!(active.expires, Some(1_900_000_000));

        let missing: LicenseStatus =
            serde_json::from_str(r#"{"active":false,"expires":null}"#).unwrap();
        assert!(!missing.active);
        assert_eq!(missing.expires, None);

        let expired: LicenseStatus =
            serde_json::from_str(r#"{"active":false,"expires":1000}"#).unwrap();
        assert!(!expired.active);
        assert_eq!(expired.expires, Some(1000));
    }

    #[test]
    fn remote_active_license_is_returned_and_cached() {
        let dir = TestDir::new("lic-active");
        let (base, server) = serve_once(200, r#"{"active":true,"expires":1900000000}"#);
        let status = block_on(check_license_with(&client(), &base, "dev 1&x", dir.path())).unwrap();
        let req = server.join().unwrap();

        assert!(status.active);
        assert_eq!(status.expires, Some(1_900_000_000));
        assert_eq!(
            req.request_line,
            "GET /license?device_id=dev+1%26x HTTP/1.1"
        );
        let cached = load_cached_license(dir.path(), 1_000);
        assert!(cached.active);
    }

    #[test]
    fn remote_inactive_overwrites_cached_active() {
        let dir = TestDir::new("lic-inactive");
        cache_license(
            dir.path(),
            &LicenseStatus {
                active: true,
                expires: Some(u64::MAX),
            },
        );
        let (base, server) = serve_once(200, r#"{"active":false,"expires":1000}"#);
        let status = block_on(check_license_with(&client(), &base, "d", dir.path())).unwrap();
        server.join().unwrap();
        assert!(!status.active);
        assert!(!load_cached_license(dir.path(), 0).active);
    }

    #[test]
    fn server_error_falls_back_to_cache() {
        let dir = TestDir::new("lic-500");
        cache_license(
            dir.path(),
            &LicenseStatus {
                active: true,
                expires: Some(u64::MAX),
            },
        );
        let (base, server) = serve_once(500, r#"{"error":"boom"}"#);
        let status = block_on(check_license_with(&client(), &base, "d", dir.path())).unwrap();
        server.join().unwrap();
        assert!(status.active);
    }

    #[test]
    fn malformed_body_falls_back_to_cache() {
        let dir = TestDir::new("lic-garbage");
        let (base, server) = serve_once(200, "not json");
        let status = block_on(check_license_with(&client(), &base, "d", dir.path())).unwrap();
        server.join().unwrap();
        assert!(!status.active);
    }

    #[test]
    fn unreachable_worker_without_cache_is_inactive() {
        let dir = TestDir::new("lic-dead");
        let status = block_on(check_license_with(&client(), &dead_url(), "d", dir.path())).unwrap();
        assert!(!status.active);
        assert_eq!(status.expires, None);
    }

    #[test]
    fn cached_license_past_expiry_is_inactive() {
        let dir = TestDir::new("lic-expired-cache");
        cache_license(
            dir.path(),
            &LicenseStatus {
                active: true,
                expires: Some(1_000),
            },
        );
        let status = load_cached_license(dir.path(), 2_000);
        assert!(!status.active);
        assert_eq!(status.expires, Some(1_000));
        assert!(load_cached_license(dir.path(), 500).active);
    }

    #[test]
    fn cached_gate_needs_an_unexpired_active_cache() {
        let dir = TestDir::new("lic-gate");
        assert!(!cached_license_active_in(dir.path(), 10));
        cache_license(
            dir.path(),
            &LicenseStatus {
                active: true,
                expires: Some(100),
            },
        );
        assert!(cached_license_active_in(dir.path(), 50));
        assert!(!cached_license_active_in(dir.path(), 200));
    }

    #[test]
    fn corrupt_cache_is_inactive() {
        let dir = TestDir::new("lic-corrupt");
        fs::write(license_cache_path(dir.path()), "{").unwrap();
        assert!(!load_cached_license(dir.path(), 0).active);
    }

    #[test]
    fn ensure_active_requires_active_and_unexpired() {
        let st = |active, expires| LicenseStatus { active, expires };
        assert_eq!(ensure_active(&st(true, None), 100), Ok(()));
        assert_eq!(ensure_active(&st(true, Some(100)), 100), Ok(()));
        assert_eq!(
            ensure_active(&st(true, Some(99)), 100),
            Err("License expired".into())
        );
        assert_eq!(
            ensure_active(&st(false, Some(99)), 100),
            Err("License expired".into())
        );
        assert_eq!(
            ensure_active(&st(false, Some(200)), 100),
            Err("No active license".into())
        );
        assert_eq!(
            ensure_active(&st(false, None), 100),
            Err("No active license".into())
        );
    }
}
