pub mod account;
pub mod engine;
pub mod license;
pub mod patrol;
pub mod probes;
pub mod scorer;
pub mod types;

#[cfg(test)]
pub(crate) mod test_support;

use types::*;

// TODO(deploy): set to the deployed worker URL
const PROD_WORKER_URL: &str = "https://kyra-guardian.workers.dev";

pub fn worker_url() -> String {
    let override_url = if cfg!(debug_assertions) {
        std::env::var("KYRA_WORKER_URL").ok()
    } else {
        None
    };
    resolve_worker_url(override_url)
}

/// One pooled client for every Worker call.
pub(crate) fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

fn resolve_worker_url(override_url: Option<String>) -> String {
    match override_url {
        Some(url) if !url.trim().is_empty() => url.trim().trim_end_matches('/').to_string(),
        _ => PROD_WORKER_URL.to_string(),
    }
}

#[tauri::command]
pub async fn guardian_run_probes() -> Result<GuardianScanResult, String> {
    let start = std::time::Instant::now();
    let reports = probes::run_all_probes().await;
    let total_cleanable: u64 = reports.iter().map(|r| r.cleanable_bytes).sum();
    Ok(GuardianScanResult {
        probes: reports,
        total_cleanable,
        scan_duration_ms: start.elapsed().as_millis() as u64,
    })
}

#[tauri::command]
pub async fn guardian_score(
    probes: Vec<ProbeReport>,
    device_id: String,
) -> Result<GuardianResult, String> {
    let scores = scorer::score_probes(&probes, &device_id).await?;
    let total_cleanable: u64 = scores.iter().map(|s| s.cleanable_bytes).sum();
    Ok(GuardianResult {
        scores,
        total_cleanable,
    })
}

#[tauri::command]
pub async fn guardian_clean(
    app: tauri::AppHandle,
    categories: Vec<String>,
    permanent: bool,
) -> Result<GuardianCleanResult, String> {
    engine::execute_guardian_clean(&app, &categories, permanent).await
}

#[tauri::command]
pub async fn guardian_check_license(device_id: String) -> Result<LicenseStatus, String> {
    license::check_license(&device_id).await
}

#[tauri::command]
pub fn guardian_get_device_id() -> String {
    license::get_or_create_device_id()
}

#[tauri::command]
pub async fn guardian_checkout_create(email: String) -> Result<CheckoutSession, String> {
    let device_id = license::get_or_create_device_id();
    let base = worker_url();
    let mut session = account::checkout_create_with(http(), &base, &device_id, &email).await?;
    // The shipped opener allowlist has no loopback entry, so the app opens local dev checkouts itself.
    if !session.short_url.starts_with("https://") && account::is_local_dev(&base) {
        std::process::Command::new("open")
            .arg(&session.short_url)
            .spawn()
            .map_err(|e| e.to_string())?;
        session.opened_by_app = true;
    }
    Ok(session)
}

#[tauri::command]
pub async fn guardian_restore_start(email: String) -> Result<(), String> {
    account::restore_start_with(http(), &worker_url(), &email).await
}

#[tauri::command]
pub async fn guardian_restore_verify(email: String, code: String) -> Result<LicenseStatus, String> {
    let device_id = license::get_or_create_device_id();
    account::restore_verify_with(
        http(),
        &worker_url(),
        &device_id,
        &email,
        &code,
        &license::app_data_dir(),
    )
    .await
}

#[tauri::command]
pub async fn guardian_account() -> Result<Option<Account>, String> {
    let device_id = license::get_or_create_device_id();
    account::account_with(http(), &worker_url(), &device_id).await
}

#[tauri::command]
pub async fn guardian_cancel_subscription() -> Result<Account, String> {
    let device_id = license::get_or_create_device_id();
    account::cancel_subscription_with(http(), &worker_url(), &device_id).await
}

#[tauri::command]
pub fn guardian_patrol_status() -> PatrolStatus {
    patrol::current_status()
}

#[tauri::command]
pub async fn guardian_patrol_now(app: tauri::AppHandle) -> Result<PatrolRun, String> {
    patrol::run_patrol(&app, PatrolTrigger::Manual).await
}

#[tauri::command]
pub async fn guardian_review_clean(
    app: tauri::AppHandle,
    ids: Vec<String>,
) -> Result<GuardianCleanResult, String> {
    patrol::review_clean(&app, ids).await
}

#[tauri::command]
pub fn guardian_review_dismiss(app: tauri::AppHandle, ids: Vec<String>) -> Result<(), String> {
    patrol::dismiss(&app, ids)
}

#[tauri::command]
pub fn guardian_set_rules(
    app: tauri::AppHandle,
    rules: PatrolRules,
) -> Result<PatrolRules, String> {
    patrol::set_rules(&app, rules)
}

/// Kept for older callers; `guardian_set_rules` is the full form.
#[tauri::command]
pub fn guardian_set_patrol(
    app: tauri::AppHandle,
    enabled: bool,
    auto_clean: bool,
) -> Result<(), String> {
    patrol::set_patrol(&app, enabled, auto_clean)
}

#[tauri::command]
pub fn get_device_name() -> String {
    use std::process::Command;
    let output = Command::new("system_profiler")
        .arg("SPHardwareDataType")
        .output();
    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Model Name:") {
                    return trimmed
                        .split(':')
                        .nth(1)
                        .unwrap_or("Mac")
                        .trim()
                        .to_string();
                }
            }
            "Mac".into()
        }
        Err(_) => "Mac".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_url_defaults_to_production() {
        assert_eq!(resolve_worker_url(None), PROD_WORKER_URL);
        assert_eq!(resolve_worker_url(Some("   ".into())), PROD_WORKER_URL);
    }

    #[test]
    fn worker_url_override_is_trimmed() {
        assert_eq!(
            resolve_worker_url(Some(" http://127.0.0.1:8787/ ".into())),
            "http://127.0.0.1:8787"
        );
    }

    #[test]
    fn production_url_is_https() {
        assert!(PROD_WORKER_URL.starts_with("https://"));
        assert!(!PROD_WORKER_URL.ends_with('/'));
    }
}
