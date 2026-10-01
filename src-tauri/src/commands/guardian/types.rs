use crate::commands::settings::{DataAction, PawtrolFrequency, ReviewAction, SafeAction};
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
    #[serde(default)]
    pub user_data: bool,
    #[serde(default)]
    pub data_loss: Option<String>,
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
    pub user_data: bool,
    pub data_loss: Option<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckoutSession {
    pub short_url: String,
    /// Set when the app already opened the page itself (local dev checkout only).
    #[serde(default)]
    pub opened_by_app: bool,
}

/// Paddle's customer portal, where the buyer cancels, resumes or changes card.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManageLink {
    pub url: String,
    /// Set when the app already opened the page itself (local dev only).
    #[serde(default)]
    pub opened_by_app: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub email: String,
    pub status: String,
    #[serde(default)]
    pub current_end: Option<u64>,
    #[serde(default)]
    pub cancel_at_period_end: bool,
    #[serde(default)]
    pub devices_count: u32,
    /// "monthly" or "yearly"; absent when the worker doesn't know.
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub management_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatrolTrigger {
    Schedule,
    LowDisk,
    Critical,
    Manual,
}

/// Mirrors the Pawtrol fields of AppSettings.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatrolRules {
    pub enabled: bool,
    pub frequency: PawtrolFrequency,
    pub low_gb: u64,
    pub critical_gb: u32,
    pub safe_action: SafeAction,
    pub review_action: ReviewAction,
    pub data_action: DataAction,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CleanedItem {
    pub name: String,
    pub size: u64,
}

/// Timestamps across the patrol types are Unix epoch milliseconds.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PatrolRun {
    pub started_at: u64,
    pub finished_at: u64,
    pub trigger: PatrolTrigger,
    #[serde(default)]
    pub cleaned: Vec<CleanedItem>,
    #[serde(default)]
    pub freed: u64,
    #[serde(default)]
    pub review_count: usize,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReviewItem {
    pub id: String,
    pub name: String,
    pub details: String,
    pub size: u64,
    pub score: f32,
    pub user_data: bool,
    #[serde(default)]
    pub data_loss: Option<String>,
    pub found_at: u64,
    /// Safe to clean without confirmation; only pending because safe items are set to ask.
    #[serde(default)]
    pub safe: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PatrolStatus {
    pub enabled: bool,
    pub auto_clean: bool,
    pub running: bool,
    pub last_patrol_at: Option<u64>,
    pub next_patrol_at: Option<u64>,
    pub freed_total: u64,
    pub freed_last: u64,
    pub pending_review: Vec<ReviewItem>,
    pub history: Vec<PatrolRun>,
    pub rules: PatrolRules,
}

#[derive(Clone, Debug, Serialize)]
pub struct PatrolStarted {
    pub trigger: PatrolTrigger,
}
