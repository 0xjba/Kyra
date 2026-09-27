# Guardian Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Kyra Guardian — an intelligent background auto-cleaning feature with software probes, Jev AI scoring, license validation, and a full frontend tab.

**Architecture:** Rust guardian module with modular software probes that scan 25 categories, a scorer that batches questions to Jev via Cloudflare Worker proxy, a disk monitor that triggers scans when free space drops below threshold, and a React frontend tab showing probe results with one-tap cleanup. Cloudflare Worker handles Jev API proxying, license validation, and Razorpay webhooks.

**Tech Stack:** Rust (Tauri 2), reqwest (HTTP), tokio (async), React 19, Zustand, TypeScript, Cloudflare Workers (Wrangler)

---

### Task 1: Add Rust Dependencies

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add reqwest and sha2 to Cargo.toml**

Add after the `zip` dependency line:

```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
sha2 = "0.10"
```

`reqwest` for HTTP calls to CF Worker. `sha2` for device ID hashing.

- [ ] **Step 2: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: `Finished` with no errors (warnings OK)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: add reqwest and sha2 dependencies for Guardian"
```

---

### Task 2: Guardian Module Scaffold + Probe Types

**Files:**
- Create: `src-tauri/src/commands/guardian/mod.rs`
- Create: `src-tauri/src/commands/guardian/types.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: Create types.rs with ProbeReport and related structs**

```rust
// src-tauri/src/commands/guardian/types.rs
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
```

- [ ] **Step 2: Create guardian/mod.rs with Tauri commands**

```rust
// src-tauri/src/commands/guardian/mod.rs
pub mod types;
pub mod probes;
pub mod scorer;
pub mod license;
pub mod engine;

use types::*;
use tauri::Emitter;

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
pub async fn guardian_score(probes: Vec<ProbeReport>, device_id: String) -> Result<GuardianResult, String> {
    let scores = scorer::score_probes(&probes, &device_id).await?;
    let total_cleanable: u64 = scores.iter().map(|s| s.cleanable_bytes).sum();
    Ok(GuardianResult { scores, total_cleanable })
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
```

- [ ] **Step 3: Register guardian module in commands/mod.rs**

Add `pub mod guardian;` to `src-tauri/src/commands/mod.rs`.

- [ ] **Step 4: Create stub files so it compiles**

Create these stub files:

`src-tauri/src/commands/guardian/probes/mod.rs`:
```rust
use super::types::ProbeReport;

mod docker;
mod homebrew;
mod xcode;
mod node;
mod rust_lang;
mod python;
mod ide;
mod ai_ml;
mod apps;
mod system;

pub async fn run_all_probes() -> Vec<ProbeReport> {
    let handles: Vec<tokio::task::JoinHandle<Option<ProbeReport>>> = vec![
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), docker::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), homebrew::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), xcode::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), node::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), rust_lang::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), python::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), ide::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), ai_ml::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), apps::probe()).then(|r| async { r.ok().flatten() })),
        tokio::spawn(tokio::time::timeout(std::time::Duration::from_secs(5), system::probe()).then(|r| async { r.ok().flatten() })),
    ];

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(Some(report)) = handle.await {
            if report.cleanable_bytes > 0 {
                results.push(report);
            }
        }
    }
    results.sort_by(|a, b| b.cleanable_bytes.cmp(&a.cleanable_bytes));
    results
}
```

`src-tauri/src/commands/guardian/scorer.rs`:
```rust
use super::types::{ProbeReport, ScoredProbe};

const WORKER_URL: &str = "https://kyra-guardian.workers.dev";

pub async fn score_probes(probes: &[ProbeReport], device_id: &str) -> Result<Vec<ScoredProbe>, String> {
    let questions: Vec<String> = probes.iter().map(|p| {
        format!(
            "{} has {} cleanable ({} items). {}. Score cleanup value 0-100.",
            p.display_name,
            format_bytes(p.cleanable_bytes),
            p.item_count,
            p.details
        )
    }).collect();

    match call_jev_api(&questions, device_id).await {
        Ok(scores) => {
            Ok(probes.iter().zip(scores.iter()).map(|(p, s)| ScoredProbe {
                category: p.category.clone(),
                display_name: p.display_name.clone(),
                cleanable_bytes: p.cleanable_bytes,
                score: s.score,
                confidence: s.confidence,
                details: p.details.clone(),
            }).collect())
        }
        Err(_) => {
            Ok(probes.iter().map(|p| {
                let score = heuristic_score(p);
                ScoredProbe {
                    category: p.category.clone(),
                    display_name: p.display_name.clone(),
                    cleanable_bytes: p.cleanable_bytes,
                    score,
                    confidence: 0.6,
                    details: p.details.clone(),
                }
            }).collect())
        }
    }
}

#[derive(serde::Deserialize)]
struct JevResponse {
    scores: Vec<JevScoreItem>,
}

#[derive(serde::Deserialize)]
struct JevScoreItem {
    score: f32,
    confidence: f32,
}

struct ScoreResult {
    score: f32,
    confidence: f32,
}

async fn call_jev_api(questions: &[String], device_id: &str) -> Result<Vec<ScoreResult>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/jev/score", WORKER_URL))
        .header("X-Device-ID", device_id)
        .json(&serde_json::json!({ "questions": questions }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("Jev API returned {}", resp.status()));
    }

    let body: JevResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body.scores.iter().map(|s| ScoreResult { score: s.score, confidence: s.confidence }).collect())
}

fn heuristic_score(probe: &ProbeReport) -> f32 {
    let size_score = match probe.cleanable_bytes {
        0..=104_857_600 => 30.0,
        104_857_601..=1_073_741_824 => 60.0,
        _ => 85.0,
    };
    let age_bonus = match probe.last_used_secs {
        Some(secs) if secs > 30 * 86400 => 15.0,
        Some(secs) if secs > 7 * 86400 => 5.0,
        _ => 0.0,
    };
    (size_score + age_bonus).min(100.0)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}
```

`src-tauri/src/commands/guardian/license.rs`:
```rust
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
    use sha2::{Sha256, Digest};
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
        .unwrap_or_else(|| format!("kyra-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()));

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
        Ok(content) => serde_json::from_str(&content).unwrap_or(LicenseStatus { active: false, expires: None }),
        Err(_) => LicenseStatus { active: false, expires: None },
    }
}
```

`src-tauri/src/commands/guardian/engine.rs`:
```rust
use super::types::{GuardianCleanProgress, GuardianCleanResult};
use crate::commands::shared;
use tauri::Emitter;

pub async fn execute_guardian_clean(
    app: &tauri::AppHandle,
    categories: &[String],
    permanent: bool,
) -> Result<GuardianCleanResult, String> {
    let total = categories.len();
    let mut bytes_freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut cleaned = 0;

    for (i, category) in categories.iter().enumerate() {
        let _ = app.emit("guardian-clean-progress", GuardianCleanProgress {
            current_category: category.clone(),
            categories_done: i,
            categories_total: total,
            bytes_freed,
        });

        match clean_category(category, permanent).await {
            Ok(freed) => {
                bytes_freed += freed;
                cleaned += 1;
            }
            Err(e) => {
                errors.push(format!("{}: {}", category, e));
            }
        }
    }

    let _ = app.emit("guardian-clean-progress", GuardianCleanProgress {
        current_category: String::new(),
        categories_done: total,
        categories_total: total,
        bytes_freed,
    });

    shared::log_operation("GUARDIAN_CLEAN", "guardian", &format!("cleaned {} categories, freed {} bytes", cleaned, bytes_freed));

    Ok(GuardianCleanResult {
        categories_cleaned: cleaned,
        bytes_freed,
        errors,
    })
}

async fn clean_category(category: &str, permanent: bool) -> Result<u64, String> {
    let paths = get_cleanable_paths(category);
    let mut freed: u64 = 0;

    for path_str in &paths {
        let path = std::path::Path::new(path_str);
        if !path.exists() {
            continue;
        }
        if !crate::commands::cleaner::is_safe_path(path_str) {
            continue;
        }

        let size = crate::commands::utils::dir_size(path);

        let result = if permanent {
            if path.is_dir() {
                std::fs::remove_dir_all(path).map(|_| ())
            } else {
                std::fs::remove_file(path).map(|_| ())
            }
        } else {
            trash::delete(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        };

        match result {
            Ok(_) => freed += size,
            Err(e) => {
                shared::log_operation("GUARDIAN_CLEAN_ERR", path_str, &e.to_string());
            }
        }
    }

    Ok(freed)
}

fn get_cleanable_paths(category: &str) -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let h = home.to_string_lossy();

    match category {
        "docker" => vec![
            format!("{}/.docker/buildx/cache", h),
            format!("{}/Library/Containers/com.docker.docker/Data/vms", h),
        ],
        "homebrew" => vec![
            format!("{}/Library/Caches/Homebrew", h),
        ],
        "xcode" => vec![
            format!("{}/Library/Developer/Xcode/DerivedData", h),
            format!("{}/Library/Developer/Xcode/Archives", h),
            format!("{}/Library/Developer/CoreSimulator/Caches", h),
        ],
        "node" => vec![
            format!("{}/.npm/_cacache", h),
            format!("{}/Library/Caches/Yarn", h),
            format!("{}/Library/pnpm/store", h),
        ],
        "rust" => vec![
            format!("{}/.cargo/registry/cache", h),
        ],
        "python" => vec![
            format!("{}/Library/Caches/pip", h),
            format!("{}/.cache/pip", h),
        ],
        "go" => vec![
            format!("{}/Library/Caches/go-build", h),
        ],
        "vscode" => vec![
            format!("{}/Library/Application Support/Code/Cache", h),
            format!("{}/Library/Application Support/Code/CachedData", h),
            format!("{}/Library/Application Support/Code/CachedExtensions", h),
        ],
        "jetbrains" => vec![
            format!("{}/Library/Caches/JetBrains", h),
        ],
        "android_studio" => vec![
            format!("{}/.android/avd", h),
            format!("{}/Library/Caches/Google/AndroidStudio*", h),
        ],
        "huggingface" => vec![
            format!("{}/.cache/huggingface", h),
        ],
        "ollama" => vec![
            format!("{}/.ollama/models", h),
        ],
        "slack" => vec![
            format!("{}/Library/Application Support/Slack/Cache", h),
            format!("{}/Library/Application Support/Slack/Service Worker/CacheStorage", h),
        ],
        "discord" => vec![
            format!("{}/Library/Application Support/discord/Cache", h),
            format!("{}/Library/Application Support/discord/Code Cache", h),
        ],
        "spotify" => vec![
            format!("{}/Library/Caches/com.spotify.client", h),
            format!("{}/Library/Application Support/Spotify/PersistentCache", h),
        ],
        "browsers" => vec![
            format!("{}/Library/Caches/Google/Chrome", h),
            format!("{}/Library/Caches/Firefox", h),
            format!("{}/Library/Caches/com.apple.Safari", h),
        ],
        "system_logs" => vec![
            format!("{}/Library/Logs", h),
        ],
        "mail" => vec![
            format!("{}/Library/Mail Downloads", h),
        ],
        "trash" => vec![
            format!("{}/.Trash", h),
        ],
        _ => vec![],
    }
}
```

- [ ] **Step 5: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: `Finished` with no errors

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/guardian/
git add src-tauri/src/commands/mod.rs
git commit -m "feat(guardian): add module scaffold with types, scorer, license, and engine"
```

---

### Task 3: Software Probe — Docker

**Files:**
- Create: `src-tauri/src/commands/guardian/probes/docker.rs`

- [ ] **Step 1: Implement Docker probe**

```rust
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;
use std::path::Path;
use std::process::Command;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let docker_dir = home.join(".docker");
    if !docker_dir.exists() {
        return None;
    }

    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details_parts: Vec<String> = Vec::new();

    // Build cache
    let buildx_cache = docker_dir.join("buildx/cache");
    if buildx_cache.exists() {
        let size = tokio::task::spawn_blocking(move || dir_size(&buildx_cache)).await.unwrap_or(0);
        if size > 0 {
            cleanable += size;
            items += 1;
            details_parts.push(format!("build cache {}", format_mb(size)));
        }
    }

    // Docker Desktop VM disk
    let vm_dir = home.join("Library/Containers/com.docker.docker/Data/vms");
    if vm_dir.exists() {
        let vm_dir_c = vm_dir.clone();
        let size = tokio::task::spawn_blocking(move || dir_size(&vm_dir_c)).await.unwrap_or(0);
        if size > 1_073_741_824 {
            // Only count as cleanable if > 1 GB
            cleanable += size / 2; // estimate half is reclaimable
            items += 1;
            details_parts.push(format!("VM disk {}", format_mb(size)));
        }
    }

    // Docker system info via CLI (dangling images, stopped containers)
    if let Ok(output) = Command::new("docker").args(["system", "df", "--format", "{{.Type}}\t{{.Reclaimable}}"]).output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("Build Cache") || line.contains("Images") {
                    details_parts.push(format!("docker reports: {}", line.trim()));
                }
            }
        }
    }

    // Count dangling images
    if let Ok(output) = Command::new("docker").args(["images", "-f", "dangling=true", "-q"]).output() {
        if output.status.success() {
            let count = String::from_utf8_lossy(&output.stdout).lines().count() as u32;
            if count > 0 {
                items += count;
                details_parts.push(format!("{} dangling images", count));
            }
        }
    }

    // Count stopped containers
    if let Ok(output) = Command::new("docker").args(["ps", "-f", "status=exited", "-q"]).output() {
        if output.status.success() {
            let count = String::from_utf8_lossy(&output.stdout).lines().count() as u32;
            if count > 0 {
                items += count;
                details_parts.push(format!("{} stopped containers", count));
            }
        }
    }

    if cleanable == 0 && items == 0 {
        return None;
    }

    Some(ProbeReport {
        category: "docker".into(),
        display_name: "Docker".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.8,
        details: details_parts.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands/guardian/probes/docker.rs
git commit -m "feat(guardian): add Docker software probe"
```

---

### Task 4: Software Probes — Homebrew, Xcode, Node, Rust, Python

**Files:**
- Create: `src-tauri/src/commands/guardian/probes/homebrew.rs`
- Create: `src-tauri/src/commands/guardian/probes/xcode.rs`
- Create: `src-tauri/src/commands/guardian/probes/node.rs`
- Create: `src-tauri/src/commands/guardian/probes/rust_lang.rs`
- Create: `src-tauri/src/commands/guardian/probes/python.rs`

- [ ] **Step 1: Implement Homebrew probe**

```rust
// src-tauri/src/commands/guardian/probes/homebrew.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;
use std::path::Path;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let cache_dir = home.join("Library/Caches/Homebrew");
    if !cache_dir.exists() {
        return None;
    }

    let cache_dir_c = cache_dir.clone();
    let cache_size = tokio::task::spawn_blocking(move || dir_size(&cache_dir_c)).await.unwrap_or(0);

    let mut details_parts: Vec<String> = Vec::new();
    let mut items: u32 = 0;

    if cache_size > 0 {
        details_parts.push(format!("cache {}", format_mb(cache_size)));
        items += 1;
    }

    // Count outdated formulae
    let brew_path = if Path::new("/opt/homebrew/bin/brew").exists() {
        Some("/opt/homebrew/bin/brew")
    } else if Path::new("/usr/local/bin/brew").exists() {
        Some("/usr/local/bin/brew")
    } else {
        None
    };

    if let Some(brew) = brew_path {
        if let Ok(output) = std::process::Command::new(brew).args(["outdated", "--quiet"]).output() {
            if output.status.success() {
                let count = String::from_utf8_lossy(&output.stdout).lines().filter(|l| !l.is_empty()).count() as u32;
                if count > 0 {
                    items += count;
                    details_parts.push(format!("{} outdated packages", count));
                }
            }
        }
    }

    if cache_size == 0 {
        return None;
    }

    Some(ProbeReport {
        category: "homebrew".into(),
        display_name: "Homebrew".into(),
        total_bytes: cache_size,
        cleanable_bytes: cache_size,
        item_count: items,
        last_used_secs: None,
        confidence: 0.9,
        details: details_parts.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 2: Implement Xcode probe**

```rust
// src-tauri/src/commands/guardian/probes/xcode.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join("Library/Developer/Xcode/DerivedData"), "DerivedData"),
        (home.join("Library/Developer/Xcode/Archives"), "Archives"),
        (home.join("Library/Developer/CoreSimulator/Caches"), "Simulator caches"),
        (home.join("Library/Developer/Xcode/iOS DeviceSupport"), "Device support"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "xcode".into(),
        display_name: "Xcode".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.9,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 3: Implement Node probe**

```rust
// src-tauri/src/commands/guardian/probes/node.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join(".npm/_cacache"), "npm cache"),
        (home.join("Library/Caches/Yarn"), "Yarn cache"),
        (home.join("Library/pnpm/store"), "pnpm store"),
        (home.join(".bun/install/cache"), "Bun cache"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "node".into(),
        display_name: "Node.js".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.9,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 4: Implement Rust probe**

```rust
// src-tauri/src/commands/guardian/probes/rust_lang.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join(".cargo/registry/cache"), "registry cache"),
        (home.join(".cargo/registry/src"), "registry source"),
        (home.join(".rustup/tmp"), "rustup tmp"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "rust".into(),
        display_name: "Rust / Cargo".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.85,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 5: Implement Python probe**

```rust
// src-tauri/src/commands/guardian/probes/python.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join("Library/Caches/pip"), "pip cache (Library)"),
        (home.join(".cache/pip"), "pip cache (dotcache)"),
        (home.join(".conda/pkgs"), "conda packages"),
        (home.join(".cache/conda"), "conda cache"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "python".into(),
        display_name: "Python".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.9,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 6: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/guardian/probes/
git commit -m "feat(guardian): add Homebrew, Xcode, Node, Rust, Python probes"
```

---

### Task 5: Software Probes — IDE, AI/ML, Apps, System

**Files:**
- Create: `src-tauri/src/commands/guardian/probes/ide.rs`
- Create: `src-tauri/src/commands/guardian/probes/ai_ml.rs`
- Create: `src-tauri/src/commands/guardian/probes/apps.rs`
- Create: `src-tauri/src/commands/guardian/probes/system.rs`

- [ ] **Step 1: Implement IDE probe (VS Code, JetBrains, Android Studio, Sublime)**

```rust
// src-tauri/src/commands/guardian/probes/ide.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join("Library/Application Support/Code/Cache"), "VS Code cache"),
        (home.join("Library/Application Support/Code/CachedData"), "VS Code cached data"),
        (home.join("Library/Application Support/Code/CachedExtensions"), "VS Code cached extensions"),
        (home.join("Library/Caches/JetBrains"), "JetBrains caches"),
        (home.join("Library/Caches/Google/AndroidStudio2024.1"), "Android Studio cache"),
        (home.join("Library/Application Support/Sublime Text/Cache"), "Sublime cache"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "ide".into(),
        display_name: "IDE Caches".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.85,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 2: Implement AI/ML probe (Hugging Face, Ollama, CoreML)**

```rust
// src-tauri/src/commands/guardian/probes/ai_ml.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join(".cache/huggingface"), "Hugging Face models"),
        (home.join(".ollama/models"), "Ollama models"),
        (home.join("Library/Caches/com.apple.coreml"), "CoreML cache"),
        (home.join(".cache/lm-studio"), "LM Studio models"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "ai_ml".into(),
        display_name: "AI & ML Models".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.7,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 3: Implement Apps probe (Slack, Discord, Spotify, browsers)**

```rust
// src-tauri/src/commands/guardian/probes/apps.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join("Library/Application Support/Slack/Cache"), "Slack cache"),
        (home.join("Library/Application Support/Slack/Service Worker/CacheStorage"), "Slack SW"),
        (home.join("Library/Application Support/discord/Cache"), "Discord cache"),
        (home.join("Library/Application Support/discord/Code Cache"), "Discord code cache"),
        (home.join("Library/Caches/com.spotify.client"), "Spotify cache"),
        (home.join("Library/Application Support/Spotify/PersistentCache"), "Spotify offline"),
        (home.join("Library/Caches/Google/Chrome/Default/Cache"), "Chrome cache"),
        (home.join("Library/Caches/Firefox/Profiles"), "Firefox cache"),
        (home.join("Library/Caches/com.apple.Safari"), "Safari cache"),
        (home.join("Library/Application Support/zoom.us/data"), "Zoom data"),
        (home.join("Library/Application Support/Microsoft/Teams/Cache"), "Teams cache"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "apps".into(),
        display_name: "App Caches".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.85,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 4: Implement System probe (logs, mail, trash)**

```rust
// src-tauri/src/commands/guardian/probes/system.rs
use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join("Library/Logs"), "User logs"),
        (home.join("Library/Mail Downloads"), "Mail downloads"),
        (home.join(".Trash"), "Trash"),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p)).await.unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 { return None; }

    Some(ProbeReport {
        category: "system".into(),
        display_name: "System & Logs".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.8,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
    else { format!("{:.0} MB", bytes as f64 / 1_048_576.0) }
}
```

- [ ] **Step 5: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/guardian/probes/
git commit -m "feat(guardian): add IDE, AI/ML, Apps, and System probes"
```

---

### Task 6: Register Guardian Commands in Tauri

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add guardian commands to invoke_handler**

In `src-tauri/src/lib.rs`, add these lines to the `invoke_handler` macro, after the last existing command (`commands::cleaner::run_brew_cleanup`):

```rust
commands::guardian::guardian_run_probes,
commands::guardian::guardian_score,
commands::guardian::guardian_clean,
commands::guardian::guardian_check_license,
commands::guardian::guardian_get_device_id,
```

- [ ] **Step 2: Verify compilation**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(guardian): register Tauri commands"
```

---

### Task 7: Frontend — Types and Tauri Bindings

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add Guardian types and commands to tauri.ts**

Append to `src/lib/tauri.ts`:

```typescript
// ── Guardian Module Types ──────────────────────────────

export interface ProbeReport {
  category: string;
  display_name: string;
  total_bytes: number;
  cleanable_bytes: number;
  item_count: number;
  last_used_secs: number | null;
  confidence: number;
  details: string;
}

export interface GuardianScanResult {
  probes: ProbeReport[];
  total_cleanable: number;
  scan_duration_ms: number;
}

export interface ScoredProbe {
  category: string;
  display_name: string;
  cleanable_bytes: number;
  score: number;
  confidence: number;
  details: string;
}

export interface GuardianResult {
  scores: ScoredProbe[];
  total_cleanable: number;
}

export interface GuardianCleanProgress {
  current_category: string;
  categories_done: number;
  categories_total: number;
  bytes_freed: number;
}

export interface GuardianCleanResult {
  categories_cleaned: number;
  bytes_freed: number;
  errors: string[];
}

export interface LicenseStatus {
  active: boolean;
  expires: number | null;
}

// ── Guardian Module Commands ───────────────────────────

export async function guardianRunProbes(): Promise<GuardianScanResult> {
  return invoke<GuardianScanResult>("guardian_run_probes");
}

export async function guardianScore(
  probes: ProbeReport[],
  deviceId: string
): Promise<GuardianResult> {
  return invoke<GuardianResult>("guardian_score", { probes, deviceId });
}

export async function guardianClean(
  categories: string[],
  permanent: boolean
): Promise<GuardianCleanResult> {
  return invoke<GuardianCleanResult>("guardian_clean", { categories, permanent });
}

export async function guardianCheckLicense(
  deviceId: string
): Promise<LicenseStatus> {
  return invoke<LicenseStatus>("guardian_check_license", { deviceId });
}

export async function guardianGetDeviceId(): Promise<string> {
  return invoke<string>("guardian_get_device_id");
}

export async function listenGuardianCleanProgress(
  callback: (progress: GuardianCleanProgress) => void
): Promise<UnlistenFn> {
  return listen<GuardianCleanProgress>("guardian-clean-progress", (event) => {
    callback(event.payload);
  });
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(guardian): add frontend Tauri bindings"
```

---

### Task 8: Frontend — Guardian Store

**Files:**
- Create: `src/stores/guardianStore.ts`

- [ ] **Step 1: Create guardian Zustand store**

```typescript
// src/stores/guardianStore.ts
import { create } from "zustand";
import {
  guardianRunProbes,
  guardianScore,
  guardianClean,
  guardianCheckLicense,
  guardianGetDeviceId,
  addBytesFreed,
  type ProbeReport,
  type ScoredProbe,
  type GuardianCleanResult,
  type LicenseStatus,
} from "../lib/tauri";

type Phase = "idle" | "scanning" | "scoring" | "results" | "cleaning" | "success" | "error";

interface GuardianStore {
  phase: Phase;
  probes: ProbeReport[];
  scores: ScoredProbe[];
  selected: Set<string>;
  totalCleanable: number;
  scanDurationMs: number;
  cleanResult: GuardianCleanResult | null;
  error: string | null;
  license: LicenseStatus;
  deviceId: string;

  scan: () => Promise<void>;
  toggleCategory: (category: string) => void;
  selectAll: () => void;
  deselectAll: () => void;
  clean: (permanent: boolean) => Promise<void>;
  checkLicense: () => Promise<void>;
  reset: () => void;
}

export const useGuardianStore = create<GuardianStore>((set, get) => ({
  phase: "idle",
  probes: [],
  scores: [],
  selected: new Set<string>(),
  totalCleanable: 0,
  scanDurationMs: 0,
  cleanResult: null,
  error: null,
  license: { active: false, expires: null },
  deviceId: "",

  scan: async () => {
    try {
      set({ phase: "scanning", error: null, probes: [], scores: [] });

      const result = await guardianRunProbes();
      set({ probes: result.probes, scanDurationMs: result.scan_duration_ms });

      if (result.probes.length === 0) {
        set({ phase: "results", totalCleanable: 0, scores: [] });
        return;
      }

      set({ phase: "scoring" });

      const deviceId = get().deviceId || await guardianGetDeviceId();
      const scored = await guardianScore(result.probes, deviceId);

      const allCategories = new Set(scored.scores.filter(s => s.score >= 60).map(s => s.category));
      set({
        phase: "results",
        scores: scored.scores,
        totalCleanable: scored.total_cleanable,
        selected: allCategories,
        deviceId,
      });
    } catch (e) {
      set({ phase: "error", error: String(e) });
    }
  },

  toggleCategory: (category: string) => {
    const selected = new Set(get().selected);
    if (selected.has(category)) {
      selected.delete(category);
    } else {
      selected.add(category);
    }
    set({ selected });
  },

  selectAll: () => {
    const all = new Set(get().scores.map(s => s.category));
    set({ selected: all });
  },

  deselectAll: () => {
    set({ selected: new Set() });
  },

  clean: async (permanent: boolean) => {
    const { selected } = get();
    if (selected.size === 0) return;

    try {
      set({ phase: "cleaning" });
      const result = await guardianClean([...selected], permanent);
      if (result.bytes_freed > 0) {
        await addBytesFreed(result.bytes_freed);
      }
      set({ phase: "success", cleanResult: result });
    } catch (e) {
      set({ phase: "error", error: String(e) });
    }
  },

  checkLicense: async () => {
    try {
      const deviceId = await guardianGetDeviceId();
      const license = await guardianCheckLicense(deviceId);
      set({ license, deviceId });
    } catch {
      set({ license: { active: false, expires: null } });
    }
  },

  reset: () => {
    set({
      phase: "idle",
      probes: [],
      scores: [],
      selected: new Set(),
      totalCleanable: 0,
      cleanResult: null,
      error: null,
    });
  },
}));
```

- [ ] **Step 2: Commit**

```bash
git add src/stores/guardianStore.ts
git commit -m "feat(guardian): add Zustand store"
```

---

### Task 9: Frontend — Guardian Page

**Files:**
- Create: `src/pages/Guardian.tsx`
- Create: `src/styles/guardian.css`

- [ ] **Step 1: Create Guardian.tsx**

Build the Guardian page component with all states: idle, scanning, scoring, results, cleaning, success, error. Follow existing module patterns (Clean.tsx) for layout, glassmorphism, buttons, scrollbar, footer.

The page should show:
- "Scan Now" CTA button (idle state)
- Scanning animation with progress (scanning state)
- Scored category list with toggles (results state)
- "Clean Selected" glass button (results state)
- Progress bar during cleaning
- Success summary with bytes freed

Use the project's macOS color palette, glass primary buttons, and consistent footer/scrollbar patterns from the design memories.

- [ ] **Step 2: Create guardian.css**

Style matching other module pages (clean.css pattern): glass cards, category rows with score badges, consistent spacing.

- [ ] **Step 3: Commit**

```bash
git add src/pages/Guardian.tsx src/styles/guardian.css
git commit -m "feat(guardian): add Guardian page and styles"
```

---

### Task 10: Frontend — Route and Navigation

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/pages/Home.tsx`

- [ ] **Step 1: Add Guardian route to App.tsx**

Add lazy import at top:
```tsx
const Guardian = lazy(() => import("./pages/Guardian"));
```

Add route in Routes:
```tsx
<Route path="/guardian" element={<Guardian />} />
```

- [ ] **Step 2: Add Guardian card to Home.tsx bento grid**

Add a Guardian module card to the dashboard. Use the `Shield` icon from lucide-react. The card should show "Guardian" with description "AI-powered auto-clean" and route to `/guardian`.

- [ ] **Step 3: Verify the app compiles and runs**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Then start dev server and verify the Guardian page loads.

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx src/pages/Home.tsx
git commit -m "feat(guardian): add route and dashboard card"
```

---

### Task 11: Cloudflare Worker

**Files:**
- Create: `worker/wrangler.toml`
- Create: `worker/src/index.ts`
- Create: `worker/package.json`

- [ ] **Step 1: Create CF Worker project structure**

Create `worker/` directory at project root with Wrangler config, entry point handling `/jev/score`, `/license`, and `/webhook/razorpay` endpoints. The worker should:
- Store/retrieve license status from KV
- Rate limit Jev requests per device
- Forward Jev API calls with server-side API key
- Verify Razorpay webhook signatures

- [ ] **Step 2: Commit**

```bash
git add worker/
git commit -m "feat(guardian): add Cloudflare Worker for Jev proxy and license management"
```

---

### Task 12: Final Integration and Compilation Check

- [ ] **Step 1: Run full cargo build**

Run: `cd src-tauri && cargo build 2>&1 | tail -20`
Expected: successful compilation

- [ ] **Step 2: Run npm build check**

Run: `cd /Users/0xjba/Projects/personal-projects/kyra && npx tsc --noEmit 2>&1 | tail -20`
Expected: no type errors

- [ ] **Step 3: Final commit with all remaining changes**

```bash
git add -A
git commit -m "feat(guardian): complete Guardian implementation with probes, scoring, license, and UI"
```
