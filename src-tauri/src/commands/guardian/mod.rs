pub mod engine;
pub mod license;
pub mod probes;
pub mod scorer;
pub mod types;

use types::*;

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
