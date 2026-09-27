use super::types::LicenseStatus;
use std::fs;
use std::path::PathBuf;

const WORKER_URL: &str = "https://kyra-guardian.workers.dev";

fn license_cache_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("com.kyra.app");
    let _ = fs::create_dir_all(&path);
    path.push("guardian_license.json");
    path
}

fn device_id_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("com.kyra.app");
    let _ = fs::create_dir_all(&path);
    path.push("device_id");
    path
}

pub fn get_or_create_device_id() -> String {
    let path = device_id_path();
    if let Ok(id) = fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = generate_device_id();
    let _ = fs::write(&path, &id);
    id
}

fn generate_device_id() -> String {
    use sha2::{Digest, Sha256};
    use std::process::Command;

    let hw_uuid = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            text.lines()
                .find(|l| l.contains("IOPlatformUUID"))
                .and_then(|l| l.split('"').nth(3))
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| {
            format!(
                "kyra-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });

    let mut hasher = Sha256::new();
    hasher.update(hw_uuid.as_bytes());
    hasher.update(b"kyra-guardian-salt");
    format!("{:x}", hasher.finalize())
}

pub async fn check_license(device_id: &str) -> Result<LicenseStatus, String> {
    // Try server first
    match check_license_remote(device_id).await {
        Ok(status) => {
            cache_license(&status);
            Ok(status)
        }
        Err(_) => {
            // Fall back to cached license
            Ok(load_cached_license())
        }
    }
}

async fn check_license_remote(device_id: &str) -> Result<LicenseStatus, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/license", WORKER_URL))
        .query(&[("device_id", device_id)])
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("License check failed: {}", resp.status()));
    }

    resp.json::<LicenseStatus>().await.map_err(|e| e.to_string())
}

fn cache_license(status: &LicenseStatus) {
    let path = license_cache_path();
    if let Ok(json) = serde_json::to_string(status) {
        let _ = fs::write(path, json);
    }
}

fn load_cached_license() -> LicenseStatus {
    let path = license_cache_path();
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or(LicenseStatus {
            active: false,
            expires: None,
        }),
        Err(_) => LicenseStatus {
            active: false,
            expires: None,
        },
    }
}
