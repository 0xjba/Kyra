use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeReport {
    pub category: String,
    pub display_name: String,
    pub total_bytes: u64,
    pub cleanable_bytes: u64,
    pub item_count: u32,
    pub last_used_secs: Option<u64>,
    pub confidence: f32,
    pub details: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuardianScanResult {
    pub probes: Vec<ProbeReport>,
    pub total_cleanable: u64,
    pub scan_duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JevScore {
    pub category: String,
    pub score: f32,
    pub confidence: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuardianResult {
    pub scores: Vec<ScoredProbe>,
    pub total_cleanable: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoredProbe {
    pub category: String,
    pub display_name: String,
    pub cleanable_bytes: u64,
    pub score: f32,
    pub confidence: f32,
    pub details: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GuardianCleanProgress {
    pub current_category: String,
    pub categories_done: usize,
    pub categories_total: usize,
    pub bytes_freed: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GuardianCleanResult {
    pub categories_cleaned: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LicenseStatus {
    pub active: bool,
    pub expires: Option<u64>,
}
