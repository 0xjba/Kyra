# Kyra Guardian — Design Spec

**Date**: 2026-09-27
**Status**: Approved
**Author**: Eleven Tribes

---

## Overview

Guardian is an intelligent background auto-cleaning feature for Kyra. It monitors disk usage passively, detects when free space drops below a threshold, probes software-specific waste categories using the Jev AI decision model, and notifies the user with a scored cleanup summary and one-tap "Clean Now" button. Guardian is a paid feature at $0.99/month.

## Goals

1. Detect low disk situations before the user notices
2. Identify cleanable waste across 20+ software categories (not just files)
3. Present an intelligent, confidence-scored recommendation
4. Let the user approve cleanup with one tap (never auto-clean silently)
5. Generate subscription revenue to fund Jev API costs

## Non-Goals

- Silent auto-deletion without user consent
- Replacing the existing manual scan modules (Clean, Prune, Installers)
- Real-time continuous monitoring (too resource-heavy)

---

## 1. Architecture

### System Components

```
┌─────────────────────────────────────────────────┐
│  Kyra App (Tauri + Rust)                        │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐ │
│  │ Disk     │→ │ Software │→ │ Jev Scorer    │ │
│  │ Monitor  │  │ Probes   │  │ (via CF proxy)│ │
│  └──────────┘  └──────────┘  └───────┬───────┘ │
│       │                              │          │
│       │         ┌────────────────────┘          │
│       │         ▼                               │
│  ┌──────────────────┐  ┌──────────────────────┐│
│  │ Notification     │  │ Cleanup Executor     ││
│  │ (one-tap approve)│→ │ (reuse existing)     ││
│  └──────────────────┘  └──────────────────────┘│
└─────────────────────────────────────────────────┘
          │
          ▼
┌─────────────────────────────────────────────────┐
│  Cloudflare Worker (free tier)                   │
│  - Proxies Jev API calls (holds API key)         │
│  - Validates device license via KV store          │
│  - Rate limits: 10 req/device/hour                │
└─────────────────────────────────────────────────┘
          │
          ▼
┌─────────────────────────────────────────────────┐
│  TypeSafe Jev API                                │
│  - Score questions as batch (~15-25 per scan)     │
│  - ~$0.000005/question → ~$0.001/scan             │
└─────────────────────────────────────────────────┘
```

### Cloudflare Worker Responsibilities

- **Jev Proxy**: Receives batch of score questions from Kyra, forwards to Jev API, returns results. API key stored as Worker secret (never in app).
- **License Validation**: `GET /license?device_id=xxx` checks KV store for active subscription.
- **Rate Limiting**: Max 10 Jev requests per device per hour (prevents abuse).
- **Endpoints**:
  - `POST /jev/score` — proxied Jev batch request (requires valid license)
  - `GET /license` — check subscription status
  - `POST /webhook/razorpay` — handles Razorpay subscription webhooks

### Why Cloudflare Workers (Free Tier)

- 100,000 requests/day free = supports ~50,000 active users
- KV store: 1,000 writes/day free (subscription changes are infrequent)
- KV reads: 100,000/day free (license checks)
- Zero server cost at indie scale

---

## 2. Software Probes

Each probe is a Rust module that discovers and measures one software category. Probes return aggregate stats only (no file paths sent to Jev).

### Probe Interface

```rust
pub struct ProbeReport {
    pub category: String,          // e.g. "docker", "xcode", "homebrew"
    pub display_name: String,      // e.g. "Docker", "Xcode DerivedData"
    pub total_bytes: u64,
    pub cleanable_bytes: u64,
    pub item_count: u32,
    pub last_used: Option<u64>,    // unix timestamp
    pub confidence: f32,           // 0.0-1.0, probe's own confidence
    pub details: ProbeDetails,     // category-specific metadata
}

pub enum ProbeDetails {
    Docker {
        dangling_images: u32,
        stopped_containers: u32,
        unused_volumes: u32,
        build_cache_bytes: u64,
    },
    Xcode {
        derived_data_bytes: u64,
        archives_bytes: u64,
        simulators_bytes: u64,
        old_sdks: u32,
    },
    Homebrew {
        outdated_casks: u32,
        cache_bytes: u64,
        old_versions: u32,
    },
    Generic {
        description: String,
    },
}
```

### Probe Categories (25 total)

**Dev Tools:**
1. Docker — dangling images, stopped containers, unused volumes, build cache
2. Homebrew — outdated formula cache, old versions, cask downloads
3. Xcode — DerivedData, archives, old simulators, device support files
4. Node.js — npm/yarn/pnpm caches, global node_modules
5. Rust — cargo registry cache, target dirs (via existing Prune rules)
6. Python — pip cache, virtualenvs, __pycache__ dirs
7. Go — GOMODCACHE, old build cache
8. Ruby — gem cache, old gem versions
9. CocoaPods — pod cache
10. Gradle/Maven — .gradle/caches, .m2/repository

**IDE Caches:**
11. VS Code — extensions cache, workspace storage
12. JetBrains — IntelliJ/WebStorm/PyCharm caches, logs, indices
13. Android Studio — AVD images, SDK components, build caches
14. Sublime Text — cache, index, session files

**AI & ML:**
15. Hugging Face — model cache (~/.cache/huggingface)
16. Ollama — downloaded models (~/.ollama/models)
17. CoreML — compiled model caches

**Apps:**
18. Slack — cache, service worker data
19. Discord — cache, code cache
20. Spotify — offline cache
21. Chrome/Safari/Firefox — browser caches, service workers (per-browser)
22. Electron apps — shared Electron cache, app-specific cache

**System:**
23. System logs — /var/log, ~/Library/Logs
24. Mail downloads — ~/Library/Mail Downloads
25. Trash — ~/.Trash size report

### Probe Execution

- All probes run in parallel via `tokio::spawn`
- Each probe has a 5-second timeout (won't block on slow I/O)
- Probes that fail return `None` (graceful degradation)
- Total probe phase: <10 seconds on most machines

---

## 3. Guardian Engine Flow

### Trigger: Threshold-Based Monitoring

```
Disk Monitor (existing SystemStats)
    → Polls every 5 minutes (configurable)
    → If disk_free < threshold (default 10 GB):
        → Run Software Probes
        → Send probe results to Jev via CF Worker
        → Build notification from Jev scores
        → Send macOS notification with "Clean Now" button
```

### Jev Scoring (Approach B: Per-Category Batch)

For each probe that found cleanable data, send one Jev Score question:

```
Batch request (~15-25 questions):
[
  "Docker has 4 dangling images, 2 stopped containers idle 30+ days, 
   3.2GB build cache. How valuable to clean? Score 0-100.",
  
  "Homebrew cache 1.8GB with 12 outdated versions. 
   How valuable to clean? Score 0-100.",
  
  "Xcode DerivedData 5.1GB, last build 14 days ago, 
   3 old simulators totaling 8GB. How valuable to clean? Score 0-100.",
  ...
]
```

**What goes to Jev**: Category name, aggregate sizes, counts, age metrics.
**What never goes to Jev**: File paths, filenames, directory names, user data.

### Jev Response Processing

Each Jev score comes back as a float 0-100 with confidence:

```rust
pub struct JevResult {
    pub category: String,
    pub score: f32,           // 0-100 cleanup value
    pub confidence: f32,      // 0.0-1.0
}
```

### Notification

macOS notification via `tauri-plugin-notification`:
- **Title**: "Kyra Guardian"
- **Body**: "Found 12.4 GB to clean across Docker, Xcode, and 3 more categories"
- **Action**: "Clean Now" button opens Kyra to Guardian results view

### Guardian Results View (in-app)

When user taps notification or opens Kyra:
- Show sorted list of categories by Jev score (highest first)
- Each row: category icon, name, cleanable size, Jev score badge
- Global "Clean All Recommended" button (items with score > 60)
- Individual toggle per category
- Uses existing cleanup executor infrastructure (safe_remove_dir_all, trash, etc.)

---

## 4. Subscription & Payment

### Payment Provider: Razorpay

**Why Razorpay:**
- India-native, instant signup (no invite gate like Stripe India)
- 2% domestic / 3% international card fees (no fixed per-transaction fee)
- Built-in Subscriptions API (no extra percentage)
- Settles in INR to Indian bank account
- Supports global card payments (Visa, Mastercard, Amex)

**Fee on $0.99/mo international:**
- ~3% = ~$0.03 per transaction
- You keep: ~$0.96

**Compared to alternatives:**

| Provider | Fee on $0.99 | You Keep | Signup |
|---|---|---|---|
| Razorpay | ~$0.03 (3%) | $0.96 | Instant |
| Stripe India | ~$0.05 (5%) | $0.94 | Invite-only |
| Lemon Squeezy | ~$0.55 (55%) | $0.44 | Instant |
| Paddle | ~$0.55 (55%) | $0.44 | Instant |

### License Architecture

```
Purchase Flow:
  User clicks "Upgrade" → Razorpay Checkout (hosted page)
  → Razorpay creates subscription → webhook fires
  → CF Worker stores device_id → active in KV (35-day TTL)

Validation Flow:
  Kyra startup / periodic check → GET /license?device_id=xxx
  → CF Worker checks KV → returns { active: true/false }
  → Kyra enables/disables Guardian

Cancellation Flow:
  subscription.cancelled webhook → CF Worker removes KV entry
  → Next license check returns inactive → Guardian disabled
```

### License Details

- **Device ID**: Generated on first launch, stored in app settings. SHA-256 hash of hardware UUID (privacy-preserving).
- **KV TTL**: 35 days (covers Razorpay's billing retry window for failed payments).
- **Grace period**: Service continues during payment retry (up to ~7 days).
- **Offline**: License cached locally with expiry timestamp. Works offline for up to 35 days.

### Tax Compliance

At indie scale ($0.99/mo), most jurisdictions have revenue thresholds before sales tax obligations apply (e.g., EU VAT threshold ~€10,000/year). No automated tax handling needed initially. Revisit when revenue crosses ~$10K/year in any single jurisdiction.

### Fallback Plan

Apply for Stripe India invite in parallel. CF Worker architecture is payment-provider-agnostic — only the webhook handler changes. Nothing in Kyra's app code changes.

### Price Tiers (Future)

- $0.99/mo — Guardian only (launch tier)
- $2.99/mo — Guardian + future AI features (e.g., "Ask Kyra")
- $9.99/year — Annual discount

Start with $0.99/mo. Add tiers when more paid features exist.

---

## 5. Settings & Configuration

### New Settings Fields

Added to existing `AppSettings` struct:

```rust
// Guardian settings
pub guardian_enabled: bool,           // default: false (requires license)
pub guardian_threshold_gb: u32,       // default: 10 (same as low_disk_threshold_gb)
pub guardian_check_interval_min: u32, // default: 5
pub guardian_auto_score: bool,        // default: true (auto-run Jev on trigger)
pub guardian_min_score: u32,          // default: 60 (min Jev score for recommendation)

// License
pub device_id: String,               // generated on first launch
pub license_active: bool,            // cached license status
pub license_expires: u64,            // unix timestamp of cached license expiry
```

### Tauri Commands (New)

```rust
// Guardian
commands::guardian::check_guardian_status  // returns license + last scan info
commands::guardian::run_guardian_scan      // manual trigger for probes + Jev
commands::guardian::get_probe_results     // returns cached probe results
commands::guardian::execute_guardian_clean // clean selected categories

// License
commands::guardian::check_license         // validate via CF Worker
commands::guardian::get_device_id         // returns/generates device_id
```

---

## 6. Frontend

### Guardian Tab

New route in the app: `/guardian`

**States:**
1. **Locked** (no license): Upgrade CTA with feature description, "Start Free Trial" or "$0.99/mo" button → opens Razorpay Checkout
2. **Idle** (licensed, no alerts): "Guardian is watching" with last scan time, manual "Scan Now" button
3. **Alert** (low disk detected): Category list sorted by Jev score, each with toggle, "Clean All Recommended" CTA
4. **Cleaning**: Progress bar (reuses existing clean progress UI pattern)
5. **Success**: Summary of freed space (reuses existing success state pattern)

### Design Consistency

Per project memory rules:
- Glass primary buttons for CTAs (Start Scan style)
- Same scrollbar, checkbox, footer, empty state patterns as other modules
- 12px uniform dashboard padding
- Locked-in font sizes and glassmorphism values
- macOS color palette (no Tailwind colors)

---

## 7. Rust Module Structure

```
src-tauri/src/commands/
├── guardian/
│   ├── mod.rs           // module exports, Tauri commands
│   ├── monitor.rs       // disk threshold monitoring loop
│   ├── probes/
│   │   ├── mod.rs       // ProbeReport struct, probe runner
│   │   ├── docker.rs
│   │   ├── homebrew.rs
│   │   ├── xcode.rs
│   │   ├── node.rs
│   │   ├── rust_lang.rs
│   │   ├── python.rs
│   │   ├── ide.rs       // VS Code, JetBrains, Android Studio, Sublime
│   │   ├── ai_ml.rs     // Hugging Face, Ollama, CoreML
│   │   ├── apps.rs      // Slack, Discord, Spotify, browsers
│   │   └── system.rs    // logs, mail downloads, trash
│   ├── scorer.rs        // Jev API client (via CF Worker)
│   ├── executor.rs      // cleanup execution (delegates to existing executors)
│   └── license.rs       // license check, device ID, caching
```

### New Dependencies

```toml
# Cargo.toml additions
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
uuid = { version = "1", features = ["v4"] }
```

`reqwest` for HTTP calls to CF Worker. `uuid` for device ID generation.

---

## 8. Cloudflare Worker Spec

### Endpoints

```
POST /jev/score
  Headers: X-Device-ID, X-License-Key
  Body: { questions: string[] }
  → Validates license in KV
  → Checks rate limit (10 req/device/hour)
  → Forwards to Jev API
  → Returns: { scores: { score: number, confidence: number }[] }

GET /license?device_id=xxx
  → Checks KV for device_id key
  → Returns: { active: boolean, expires: number }

POST /webhook/razorpay
  Headers: X-Razorpay-Signature (for verification)
  Body: Razorpay webhook payload
  Events handled:
    subscription.charged → KV put(device_id, { active: true }, ttl: 35 days)
    subscription.cancelled → KV delete(device_id)
    subscription.paused → KV put(device_id, { active: false })
```

### KV Schema

```
Key: device:{device_id}
Value: { "active": true, "plan": "guardian", "subscription_id": "sub_xxx" }
TTL: 35 days (auto-refreshed on each successful charge)
```

### Rate Limit

Stored in KV as `rate:{device_id}:{hour}` with 1-hour TTL, incremented per request.

---

## 9. Security Considerations

- **No file paths to Jev**: Only aggregate stats (sizes, counts, ages) leave the device
- **API key server-side**: Jev API key stored as CF Worker secret, never in app binary
- **License tampering**: Device ID is SHA-256 of hardware UUID — hard to spoof. License check hits server, cached locally with expiry.
- **Webhook verification**: Razorpay webhooks verified via X-Razorpay-Signature header using webhook secret
- **Rate limiting**: Prevents abuse of Jev API quota
- **Existing safety**: Guardian reuses `safe_remove_dir_all()` and all existing deletion safeguards from the cleaner module

---

## 10. Cost Analysis

### Per-User Monthly Cost

| Item | Cost |
|---|---|
| Jev API (~20 questions/scan, ~6 scans/mo) | ~$0.006 |
| CF Worker requests (~50 req/user/mo) | $0 (free tier) |
| **Total cost per user** | **~$0.006/mo** |

### Revenue Per User

| Item | Amount |
|---|---|
| Subscription | $0.99 |
| Razorpay fee (3% intl) | -$0.03 |
| Jev cost | -$0.006 |
| **Net per user/month** | **~$0.95** |

### Break-Even

CF Worker free tier supports ~50,000 users. Beyond that, Workers Paid ($5/mo) supports 10M requests. Jev costs scale linearly but remain negligible (~$0.006/user/mo).

---

## 11. Implementation Order

1. **Phase 1**: Rust module scaffold + software probes (no Jev, no payment)
2. **Phase 2**: CF Worker + Jev integration (scoring works)
3. **Phase 3**: Razorpay subscription + license system
4. **Phase 4**: Frontend Guardian tab
5. **Phase 5**: Notification integration + background monitoring loop
6. **Phase 6**: Testing, polish, release

---

## Revision 2026-09-29: Autonomous patrol

Pawtrol (user-facing name of Guardian) is an autonomous agent, not a manual scan screen.

**Behavior**
- **Patrol triggers:** once a day when the Mac is idle and on AC power, plus immediately when free space drops below the low-disk threshold (with a cooldown). "Patrol now" is a secondary manual action.
- **Safe items** (score > 70 and not user data): auto-cleaned when the "Auto-clean safe items" setting is on (default on), respecting Move to Trash. When it's off, a notification offers one-tap approval.
- **Risky items** (score 40–70, or any user-data category): never touched automatically. They're added to a "Needs your review" list and announced with a notification.
- **User-data categories** (Docker VM, AI models, Xcode Archives, Spotify offline, Zoom data): score capped at 55, labeled with what would be lost, explicit confirmation required.
- **After each patrol:** one summary notification, e.g. "Pawtrol freed 2.3 GB · 2 items need review".

This supersedes the original non-goal "Silent auto-deletion without user consent". Consent is now given once, through the default-on setting, and only for regenerable caches.

**Presence**
- For Pro users, closing the window hides it and Kyra keeps running with a menu-bar icon: status, last patrol, Patrol now, Open Kyra, Quit.
- Launch at login is enabled when Pro activates.

**UI**
- The Pawtrol page is a dashboard: on-duty status, last and next patrol, space freed, the needs-review list, and an activity log.
- The titlebar popover shows the same status at a glance.

**Contract (Rust ⇄ frontend)**
- **Commands:**
  - `guardian_patrol_status() -> PatrolStatus`
  - `guardian_patrol_now() -> PatrolRun`
  - `guardian_review_clean(ids) -> CleanResult`
  - `guardian_review_dismiss(ids)` (snooze 30 days)
  - `guardian_set_patrol(enabled, auto_clean)`
- **Events:** `patrol-started` {trigger}, `patrol-finished` PatrolRun, `patrol-status` PatrolStatus.
- **Types:**
  - `PatrolStatus` { enabled, auto_clean, running, last_patrol_at, next_patrol_at, freed_total, freed_last, pending_review: ReviewItem[], history: PatrolRun[] (last 20) }
  - `ReviewItem` { id, name, details, size, score, user_data, data_loss?, found_at }
  - `PatrolRun` { started_at, finished_at, trigger: "schedule"|"low_disk"|"manual", cleaned: {name,size}[], freed, review_count, error? }

---

## Revision 2026-09-29: Subscription, restore and account (Razorpay)

**Identity:** no passwords and no signup form. Accounts are keyed by the buyer's email, and a device holds a license bound to its `device_id`.

**Subscribe:**
1. The app asks for an email.
2. The Worker creates a Razorpay Subscription (plan from env `RAZORPAY_PLAN_ID`) with `notes: {device_id, email}` and returns its hosted `short_url`.
3. The app opens `short_url` in the browser.
4. The Razorpay webhook (`subscription.activated` / `charged` / `cancelled` / `halted` / `completed`) updates the license for the device, plus `account:{email}` = {subscription_id, status, current_end, devices[]}.

**Restore on a new Mac:**
1. `POST /restore/start {email}` emails a 6-digit code: valid 10 minutes, 5 attempts, 3 sends per hour per email. It always returns 200 so account existence can't be probed.
2. `POST /restore/verify {email, code, device_id}` binds the device (max 3; the oldest is evicted) and writes its license.

**Manage from the app:**
- `GET /account?device_id=` returns {email, status, current_end, cancel_at_period_end, devices_count}.
- `POST /subscription/cancel {device_id}` cancels at the end of the billing cycle via the Razorpay API.
- Card updates use Razorpay's own customer flow, linked from the account screen when Razorpay provides a URL.

**Email:** Resend HTTP API (`RESEND_API_KEY`, `MAIL_FROM`).

**Secrets (wrangler secret):** `RAZORPAY_KEY_ID`, `RAZORPAY_KEY_SECRET`, `RAZORPAY_WEBHOOK_SECRET`, `RAZORPAY_PLAN_ID`, `RESEND_API_KEY`, `MAIL_FROM`, `JEV_API_KEY`.

**App commands (Rust → Worker):**
- `guardian_checkout_create(email) -> {short_url}`
- `guardian_restore_start(email)`
- `guardian_restore_verify(email, code) -> LicenseStatus`
- `guardian_account() -> Account | null`
- `guardian_cancel_subscription() -> Account`
