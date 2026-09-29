use super::types::*;
use super::engine::CleanMode;
use super::{engine, license, probes, scorer};
use crate::commands::settings::{
    self, AppSettings, DataAction, PawtrolFrequency, ReviewAction, SafeAction,
};
use crate::commands::shared;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const MINUTE: u64 = 60_000;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
const GIB: u64 = 1 << 30;

pub(crate) const SAFE_SCORE: f32 = 70.0;
pub(crate) const REVIEW_SCORE: f32 = 40.0;
pub(crate) const MIN_ITEM_BYTES: u64 = 100 * 1024 * 1024;
const HISTORY_LEN: usize = 20;
const SNOOZE_MS: u64 = 30 * DAY;

const IDLE_MS: u64 = 5 * MINUTE;
// Without an idle moment a scheduled run still happens once it is two intervals late.
const OVERDUE_INTERVALS: u64 = 2;
const LOW_DISK_COOLDOWN_MS: u64 = 6 * HOUR;
const CRITICAL_COOLDOWN_MS: u64 = 2 * HOUR;
// The worker allows 10 scoring calls per hour; scheduled patrols stay far below that.
const SCHEDULED_MIN_GAP_MS: u64 = HOUR;
const FIRST_TICK: Duration = Duration::from_secs(120);
const TICK: Duration = Duration::from_secs(600);

pub const EVENT_STARTED: &str = "patrol-started";
pub const EVENT_FINISHED: &str = "patrol-finished";
pub const EVENT_STATUS: &str = "patrol-status";

const STATE_FILE: &str = "pawtrol_state.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct PatrolState {
    pub last_patrol_at: Option<u64>,
    pub freed_total: u64,
    pub freed_last: u64,
    /// Newest first.
    pub history: Vec<PatrolRun>,
    pub pending_review: Vec<ReviewItem>,
    /// Category id → snoozed until (ms).
    pub snoozed: HashMap<String, u64>,
    pub last_low_disk_trigger_at: Option<u64>,
    pub last_scheduled_at: Option<u64>,
    pub last_critical_trigger_at: Option<u64>,
}

static STATE_LOCK: Mutex<()> = Mutex::new(());

fn state_path() -> PathBuf {
    license::app_data_dir().join(STATE_FILE)
}

pub(crate) fn load_state(path: &Path) -> PatrolState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, state: &PatrolState) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Load-modify-save under one lock so a dismiss during a patrol is not overwritten.
pub(crate) fn update_state<T>(
    path: &Path,
    f: impl FnOnce(&mut PatrolState) -> T,
) -> Result<T, String> {
    let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load_state(path);
    let out = f(&mut state);
    save_state(path, &state)?;
    Ok(out)
}

impl PatrolRules {
    pub fn from_settings(s: &AppSettings) -> Self {
        PatrolRules {
            enabled: s.pawtrol_enabled,
            frequency: s.pawtrol_frequency,
            low_gb: s.low_disk_threshold_gb,
            critical_gb: s.pawtrol_critical_gb,
            safe_action: s.pawtrol_safe_action,
            review_action: s.pawtrol_review_action,
            data_action: s.pawtrol_data_action,
        }
    }

    /// Writes the rules into `s`, clamping the critical tier below the low tier.
    pub fn apply(self, s: &mut AppSettings) {
        s.pawtrol_enabled = self.enabled;
        s.pawtrol_frequency = self.frequency;
        s.low_disk_threshold_gb = self.low_gb;
        s.pawtrol_critical_gb = self.critical_gb;
        s.pawtrol_safe_action = self.safe_action;
        s.pawtrol_review_action = self.review_action;
        s.pawtrol_data_action = self.data_action;
        s.normalize();
    }
}

impl Default for PatrolRules {
    fn default() -> Self {
        PatrolRules::from_settings(&AppSettings::default())
    }
}

pub(crate) fn interval_ms(frequency: PawtrolFrequency) -> Option<u64> {
    match frequency {
        PawtrolFrequency::SixHours => Some(6 * HOUR),
        PawtrolFrequency::Daily => Some(DAY),
        PawtrolFrequency::Weekly => Some(7 * DAY),
        PawtrolFrequency::LowDiskOnly => None,
    }
}

#[derive(Debug, Default)]
pub(crate) struct Partition {
    pub safe: Vec<ScoredProbe>,
    pub review: Vec<ScoredProbe>,
}

fn is_snoozed(snoozed: &HashMap<String, u64>, id: &str, now: u64) -> bool {
    snoozed.get(id).is_some_and(|until| *until > now)
}

fn is_user_data(s: &ScoredProbe) -> bool {
    s.user_data || probes::data_loss(&s.category).is_some()
}

pub(crate) fn partition(
    scores: Vec<ScoredProbe>,
    snoozed: &HashMap<String, u64>,
    now: u64,
    min_bytes: u64,
) -> Partition {
    let mut out = Partition::default();
    for s in scores {
        if probes::definition(&s.category).is_none()
            || s.cleanable_bytes < min_bytes
            || is_snoozed(snoozed, &s.category, now)
        {
            continue;
        }
        // Anything that could reach data the guard protects needs the user.
        let needs_review = is_user_data(&s) || !probes::autonomous_safe(&s.category);
        if s.score > SAFE_SCORE && !needs_review {
            out.safe.push(s);
        } else if needs_review || s.score >= REVIEW_SCORE {
            out.review.push(s);
        }
    }
    out
}

#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub clean: Vec<ScoredProbe>,
    /// (item, safe)
    pub queue: Vec<(ScoredProbe, bool)>,
}

/// A critical run cleans safe items even when the rules say ask.
pub(crate) fn plan(part: Partition, rules: &PatrolRules, trigger: PatrolTrigger) -> Plan {
    let mut out = Plan::default();
    if rules.safe_action == SafeAction::Auto || trigger == PatrolTrigger::Critical {
        out.clean = part.safe;
    } else {
        out.queue.extend(part.safe.into_iter().map(|s| (s, true)));
    }
    out.queue.extend(
        part.review
            .into_iter()
            .filter(|s| !(is_user_data(s) && rules.data_action == DataAction::Ignore))
            .map(|s| (s, false)),
    );
    out
}

pub(crate) fn is_quiet(item: &ReviewItem, rules: &PatrolRules) -> bool {
    if item.safe {
        false
    } else if item.user_data {
        rules.data_action != DataAction::Notify
    } else {
        rules.review_action == ReviewAction::Quiet
    }
}

fn review_item(s: &ScoredProbe, found_at: u64, safe: bool) -> ReviewItem {
    let data_loss = probes::data_loss(&s.category)
        .map(String::from)
        .or_else(|| s.data_loss.clone());
    ReviewItem {
        id: s.category.clone(),
        name: s.display_name.clone(),
        details: s.details.clone(),
        size: s.cleanable_bytes,
        score: s.score,
        user_data: is_user_data(s),
        data_loss,
        found_at,
        safe,
    }
}

#[derive(Default)]
pub(crate) struct CleanBatch {
    pub cleaned: Vec<CleanedItem>,
    pub cleaned_ids: Vec<String>,
    pub freed: u64,
    pub errors: Vec<String>,
    pub path_errors: Vec<(String, String)>,
}

/// One category at a time so each cleaned item carries its own freed size.
fn clean_each(home: &Path, items: &[(String, String)], permanent: bool, mode: CleanMode) -> CleanBatch {
    let mut batch = CleanBatch::default();
    for (id, name) in items {
        let out = engine::clean_categories_with(home, std::slice::from_ref(id), permanent, mode, |_| {});
        batch.errors.extend(out.result.errors);
        batch.path_errors.extend(out.path_errors);
        if out.result.categories_cleaned > 0 {
            batch.cleaned_ids.push(id.clone());
        }
        if out.result.bytes_freed > 0 {
            batch.freed += out.result.bytes_freed;
            batch.cleaned.push(CleanedItem {
                name: name.clone(),
                size: out.result.bytes_freed,
            });
        }
    }
    batch.cleaned.sort_by(|a, b| b.size.cmp(&a.size));
    batch
}

async fn clean_blocking(
    home: &Path,
    items: Vec<(String, String)>,
    permanent: bool,
    mode: CleanMode,
) -> Result<CleanBatch, String> {
    let home = home.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || clean_each(&home, &items, permanent, mode))
        .await
        .map_err(|e| format!("Clean task failed: {}", e))
}

pub(crate) struct PatrolCtx {
    pub state_path: PathBuf,
    pub home: PathBuf,
    pub clock: Box<dyn Fn() -> u64 + Send + Sync>,
    pub rules: PatrolRules,
    pub permanent: bool,
    pub min_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Notice {
    pub title: String,
    pub body: String,
    pub urgent: bool,
}

pub(crate) struct PatrolOutcome {
    pub run: PatrolRun,
    pub notice: Option<Notice>,
    pub path_errors: Vec<(String, String)>,
}

struct Found {
    batch: CleanBatch,
    pending: Vec<(ScoredProbe, bool)>,
}

async fn scan_and_clean<S, F>(
    ctx: &PatrolCtx,
    trigger: PatrolTrigger,
    license: impl Future<Output = Result<(), String>>,
    score: S,
) -> Result<Found, String>
where
    S: FnOnce(Vec<ProbeReport>) -> F,
    F: Future<Output = Result<Vec<ScoredProbe>, String>>,
{
    license.await?;
    let reports = probes::run_all_probes_in(ctx.home.clone()).await;
    let scores = score(reports).await?;
    let snoozed = load_state(&ctx.state_path).snoozed;
    let part = partition(scores, &snoozed, (ctx.clock)(), ctx.min_bytes);
    let plan = plan(part, &ctx.rules, trigger);

    let batch = if plan.clean.is_empty() {
        CleanBatch::default()
    } else {
        let items = plan
            .clean
            .iter()
            .map(|s| (s.category.clone(), s.display_name.clone()))
            .collect();
        clean_blocking(&ctx.home, items, ctx.permanent, CleanMode::Autonomous).await?
    };
    Ok(Found {
        batch,
        pending: plan.queue,
    })
}

pub(crate) async fn patrol_with<S, F>(
    ctx: &PatrolCtx,
    trigger: PatrolTrigger,
    license: impl Future<Output = Result<(), String>>,
    score: S,
) -> PatrolOutcome
where
    S: FnOnce(Vec<ProbeReport>) -> F,
    F: Future<Output = Result<Vec<ScoredProbe>, String>>,
{
    let started_at = (ctx.clock)();
    let found = scan_and_clean(ctx, trigger, license, score).await;
    let finished_at = (ctx.clock)();
    let mut run = PatrolRun {
        started_at,
        finished_at,
        trigger,
        cleaned: Vec::new(),
        freed: 0,
        review_count: 0,
        error: None,
    };

    let found = match found {
        Ok(found) => found,
        Err(e) => {
            run.error = Some(e);
            let _ = update_state(&ctx.state_path, |state| {
                run.review_count = state.pending_review.len();
                push_history(state, run.clone());
            });
            return PatrolOutcome {
                run,
                notice: None,
                path_errors: Vec::new(),
            };
        }
    };

    run.cleaned = found.batch.cleaned;
    run.freed = found.batch.freed;

    let merged = update_state(&ctx.state_path, |state| {
        let now = finished_at;
        state.snoozed.retain(|_, until| *until > now);
        let previous: HashMap<String, u64> = state
            .pending_review
            .iter()
            .map(|i| (i.id.clone(), i.found_at))
            .collect();
        let mut pending: Vec<ReviewItem> = found
            .pending
            .iter()
            .filter(|(s, _)| !is_snoozed(&state.snoozed, &s.category, now))
            .map(|(s, safe)| {
                let found_at = previous.get(&s.category).copied().unwrap_or(now);
                review_item(s, found_at, *safe)
            })
            .collect();
        pending.sort_by(|a, b| b.size.cmp(&a.size));
        let new_ids: HashSet<String> = pending
            .iter()
            .filter(|i| !previous.contains_key(&i.id))
            .map(|i| i.id.clone())
            .collect();

        state.pending_review = pending.clone();
        state.last_patrol_at = Some(finished_at);
        state.freed_last = run.freed;
        state.freed_total = state.freed_total.saturating_add(run.freed);
        run.review_count = pending.iter().filter(|i| !i.safe).count();
        push_history(state, run.clone());
        (pending, new_ids)
    });

    let notice = match &merged {
        Ok((pending, _)) if trigger == PatrolTrigger::Critical => critical_notice(&run, pending),
        Ok((pending, new_ids)) => {
            let announced: Vec<ReviewItem> = pending
                .iter()
                .filter(|i| !is_quiet(i, &ctx.rules))
                .cloned()
                .collect();
            build_notice(&run, &announced, new_ids)
        }
        Err(e) => {
            run.error = Some(format!("Couldn't save Pawtrol's progress: {}", e));
            None
        }
    };

    PatrolOutcome {
        run,
        notice,
        path_errors: found.batch.path_errors,
    }
}

fn push_history(state: &mut PatrolState, run: PatrolRun) {
    state.history.insert(0, run);
    state.history.truncate(HISTORY_LEN);
}

pub(crate) struct ReviewCleanOutcome {
    pub result: GuardianCleanResult,
    pub path_errors: Vec<(String, String)>,
}

/// Only ids already in the review list are cleaned; the UI's confirmation covers user-data items.
pub(crate) async fn review_clean_with(
    state_path: &Path,
    home: &Path,
    ids: &[String],
    permanent: bool,
    license: impl Future<Output = Result<(), String>>,
) -> Result<ReviewCleanOutcome, String> {
    license.await?;
    let pending = load_state(state_path).pending_review;
    let mut items: Vec<(String, String)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for id in ids {
        if items.iter().any(|(known, _)| known == id) {
            continue;
        }
        match pending.iter().find(|i| &i.id == id) {
            Some(item) => items.push((item.id.clone(), item.name.clone())),
            None => errors.push(format!("{}: not in the review list", id)),
        }
    }

    let batch = clean_blocking(home, items, permanent, CleanMode::Approved).await?;
    errors.extend(batch.errors);

    update_state(state_path, |state| {
        state
            .pending_review
            .retain(|i| !batch.cleaned_ids.contains(&i.id));
        state.freed_total = state.freed_total.saturating_add(batch.freed);
    })?;

    Ok(ReviewCleanOutcome {
        result: GuardianCleanResult {
            categories_cleaned: batch.cleaned_ids.len(),
            bytes_freed: batch.freed,
            errors,
        },
        path_errors: batch.path_errors,
    })
}

pub(crate) fn dismiss_with(state_path: &Path, ids: &[String], now: u64) -> Result<(), String> {
    update_state(state_path, |state| {
        for id in ids {
            if probes::definition(id).is_some() {
                state.snoozed.insert(id.clone(), now + SNOOZE_MS);
            }
        }
        state.pending_review.retain(|i| !ids.contains(&i.id));
    })
}

/// Mirrors formatSize in src/utils/format.ts so notifications match the UI.
pub(crate) fn format_size(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= 1000.0 * MIB {
        format!("{:.1} GB", b / (1024.0 * MIB))
    } else if b >= 1000.0 * 1024.0 {
        format!("{:.1} MB", b / MIB)
    } else if b >= 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Unchanged findings are not re-announced every day; only a clean or something new notifies.
pub(crate) fn build_notice(
    run: &PatrolRun,
    pending: &[ReviewItem],
    new_ids: &HashSet<String>,
) -> Option<Notice> {
    if run.error.is_some() {
        return None;
    }
    let review: Vec<&ReviewItem> = pending.iter().filter(|i| !i.safe).collect();

    if run.freed > 0 {
        let names: Vec<&str> = run
            .cleaned
            .iter()
            .take(3)
            .map(|c| c.name.as_str())
            .collect();
        let mut parts: Vec<String> = Vec::new();
        if !names.is_empty() {
            parts.push(names.join(", "));
        }
        match review.len() {
            0 => {}
            1 => parts.push("1 needs your review".into()),
            n => parts.push(format!("{} need your review", n)),
        }
        return Some(Notice {
            title: format!("Pawtrol freed {}", format_size(run.freed)),
            body: parts.join(" · "),
            urgent: false,
        });
    }

    if !pending.iter().any(|i| new_ids.contains(&i.id)) {
        return None;
    }

    let safe_total: u64 = pending.iter().filter(|i| i.safe).map(|i| i.size).sum();
    if safe_total > 0 {
        return Some(Notice {
            title: format!("Pawtrol found {} safe to clean", format_size(safe_total)),
            body: "Open Kyra to clean it in one tap".into(),
            urgent: false,
        });
    }

    let biggest = review.iter().max_by_key(|i| i.size)?;
    let title = match review.len() {
        1 => "1 item needs your review".to_string(),
        n => format!("{} items need your review", n),
    };
    Some(Notice {
        title,
        body: format!("{} · {}", biggest.name, format_size(biggest.size)),
        urgent: false,
    })
}

/// Sent after every critical run, whatever the review and data rules say.
pub(crate) fn critical_notice(run: &PatrolRun, pending: &[ReviewItem]) -> Option<Notice> {
    if run.error.is_some() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if run.freed > 0 {
        parts.push(format!("Freed {}", format_size(run.freed)));
    }
    if let Some(biggest) = pending.iter().max_by_key(|i| i.size) {
        let count = match pending.len() {
            1 => "1 item needs you".to_string(),
            n => format!("{} items need you", n),
        };
        parts.push(count);
        parts.push(format!("biggest: {} {}", biggest.name, format_size(biggest.size)));
    }
    if parts.is_empty() {
        parts.push("Nothing left that Kyra can clean on its own".into());
    }
    Some(Notice {
        title: "Your Mac is almost out of space".into(),
        body: parts.join(" · "),
        urgent: true,
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Conditions {
    pub now: u64,
    pub last_patrol_at: Option<u64>,
    pub last_scheduled_at: Option<u64>,
    pub last_low_disk_trigger_at: Option<u64>,
    pub last_critical_trigger_at: Option<u64>,
    pub idle_ms: Option<u64>,
    pub on_ac: bool,
    pub free_bytes: Option<u64>,
    pub rules: PatrolRules,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Run(PatrolTrigger),
    Wait(&'static str),
}

pub(crate) fn decide(c: &Conditions) -> Decision {
    let ago = |t: u64| c.now.saturating_sub(t);
    let below = |gb: u64| c.free_bytes.is_some_and(|f| f < gb.saturating_mul(GIB));
    let cooled = |last: Option<u64>, cooldown: u64| last.map_or(true, |t| ago(t) >= cooldown);

    if !c.rules.enabled {
        return Decision::Wait("paused");
    }
    if c.last_scheduled_at
        .is_some_and(|t| ago(t) < SCHEDULED_MIN_GAP_MS)
    {
        return Decision::Wait("ran within the last hour");
    }

    if below(c.rules.critical_gb as u64) && cooled(c.last_critical_trigger_at, CRITICAL_COOLDOWN_MS) {
        return Decision::Run(PatrolTrigger::Critical);
    }
    if below(c.rules.low_gb) && cooled(c.last_low_disk_trigger_at, LOW_DISK_COOLDOWN_MS) {
        return Decision::Run(PatrolTrigger::LowDisk);
    }

    let Some(interval) = interval_ms(c.rules.frequency) else {
        return Decision::Wait("scheduled runs are off");
    };
    let since = c.last_patrol_at.map(ago);
    if since.is_some_and(|s| s < interval) {
        return Decision::Wait("not due yet");
    }
    if !c.on_ac {
        return Decision::Wait("on battery");
    }
    if c.idle_ms.is_some_and(|i| i >= IDLE_MS) {
        return Decision::Run(PatrolTrigger::Schedule);
    }
    if since.is_some_and(|s| s >= interval.saturating_mul(1 + OVERDUE_INTERVALS)) {
        return Decision::Run(PatrolTrigger::Schedule);
    }
    Decision::Wait("Mac is in use")
}

/// A critical run also restarts the low-disk cooldown so the two tiers don't fire back to back.
pub(crate) fn record_trigger(state: &mut PatrolState, trigger: PatrolTrigger, now: u64) {
    state.last_scheduled_at = Some(now);
    match trigger {
        PatrolTrigger::Critical => {
            state.last_critical_trigger_at = Some(now);
            state.last_low_disk_trigger_at = Some(now);
        }
        PatrolTrigger::LowDisk => state.last_low_disk_trigger_at = Some(now),
        PatrolTrigger::Schedule | PatrolTrigger::Manual => {}
    }
}

pub(crate) fn next_patrol_estimate(state: &PatrolState, now: u64, interval: u64) -> u64 {
    let due = state.last_patrol_at.map_or(now, |t| t + interval);
    let gap = state
        .last_scheduled_at
        .map_or(0, |t| t + SCHEDULED_MIN_GAP_MS);
    due.max(gap).max(now)
}

pub(crate) fn parse_hid_idle_ms(ioreg: &str) -> Option<u64> {
    ioreg
        .lines()
        .find(|l| l.contains("\"HIDIdleTime\""))
        .and_then(|l| l.split('=').nth(1))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|ns| ns / 1_000_000)
}

pub(crate) fn parse_on_ac(pmset: &str) -> bool {
    pmset.contains("AC Power")
}

fn command_stdout(cmd: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn hid_idle_ms() -> Option<u64> {
    command_stdout("ioreg", &["-c", "IOHIDSystem", "-d", "4"]).and_then(|s| parse_hid_idle_ms(&s))
}

fn on_ac_power() -> bool {
    command_stdout("pmset", &["-g", "batt"]).is_some_and(|s| parse_on_ac(&s))
}

fn free_disk_bytes() -> Option<u64> {
    sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .find(|d| d.mount_point() == Path::new("/"))
        .map(|d| d.available_space())
}

pub(crate) fn status_from(
    state: PatrolState,
    rules: PatrolRules,
    running: bool,
    now: u64,
) -> PatrolStatus {
    let next_patrol_at = interval_ms(rules.frequency)
        .filter(|_| rules.enabled)
        .map(|interval| next_patrol_estimate(&state, now, interval));
    PatrolStatus {
        enabled: rules.enabled,
        auto_clean: rules.safe_action == SafeAction::Auto,
        running,
        last_patrol_at: state.last_patrol_at,
        next_patrol_at,
        freed_total: state.freed_total,
        freed_last: state.freed_last,
        pending_review: state.pending_review,
        history: state.history,
        rules,
    }
}

pub(crate) fn relative(ms_ago: u64) -> String {
    match ms_ago {
        t if t < MINUTE => "just now".into(),
        t if t < HOUR => format!("{}m ago", t / MINUTE),
        t if t < DAY => format!("{}h ago", t / HOUR),
        t => format!("{}d ago", t / DAY),
    }
}

pub(crate) fn freed_today(state: &PatrolState, now: u64, day_of: impl Fn(u64) -> i64) -> u64 {
    let today = day_of(now);
    state
        .history
        .iter()
        .filter(|r| day_of(r.finished_at) == today)
        .map(|r| r.freed)
        .sum()
}

pub(crate) fn tray_text(
    state: &PatrolState,
    enabled: bool,
    running: bool,
    now: u64,
    freed_today: u64,
) -> String {
    if running {
        return "Pawtrol · checking…".into();
    }
    if !enabled {
        return crate::tray::STATUS_OFF.into();
    }
    let review = state.pending_review.len();
    if review > 0 {
        return format!("Pawtrol · {} to review", review);
    }
    if freed_today > 0 {
        return format!("Pawtrol · freed {} today", format_size(freed_today));
    }
    match state.last_patrol_at {
        Some(t) => format!("Pawtrol · last run {}", relative(now.saturating_sub(t))),
        None => crate::tray::STATUS_ON_DUTY.into(),
    }
}

fn local_day(ms: u64) -> i64 {
    let t = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    tm.tm_year as i64 * 1000 + tm.tm_yday as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Busy {
    Patrol,
    ReviewClean,
}

pub(crate) struct BusyLock(Mutex<Option<Busy>>);

pub(crate) struct BusyGuard<'a>(&'a BusyLock);

impl BusyLock {
    pub const fn new() -> Self {
        BusyLock(Mutex::new(None))
    }

    pub fn try_acquire(&self, kind: Busy) -> Result<BusyGuard<'_>, String> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match *slot {
            Some(Busy::Patrol) => Err("Pawtrol is already running".into()),
            Some(Busy::ReviewClean) => Err("Pawtrol is busy cleaning".into()),
            None => {
                *slot = Some(kind);
                Ok(BusyGuard(self))
            }
        }
    }

    pub fn current(&self) -> Option<Busy> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        *self.0 .0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

static BUSY: BusyLock = BusyLock::new();

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn current_settings() -> AppSettings {
    settings::load_settings_internal().unwrap_or_default()
}

fn home_dir() -> Result<PathBuf, String> {
    // Without a home dir every target would resolve against "/", so refuse outright.
    dirs::home_dir()
        .filter(|h| h.is_absolute() && h != Path::new("/"))
        .ok_or_else(|| "Could not resolve home directory".to_string())
}

pub fn current_status() -> PatrolStatus {
    status_from(
        load_state(&state_path()),
        PatrolRules::from_settings(&current_settings()),
        BUSY.current() == Some(Busy::Patrol),
        now_ms(),
    )
}

fn refresh_tray(app: &AppHandle) {
    let s = current_settings();
    let state = load_state(&state_path());
    let now = now_ms();
    let text = tray_text(
        &state,
        s.pawtrol_enabled,
        BUSY.current() == Some(Busy::Patrol),
        now,
        freed_today(&state, now, local_day),
    );
    crate::tray::set_tray_status(app, &text);
}

pub fn emit_status(app: &AppHandle) {
    let _ = app.emit(EVENT_STATUS, current_status());
    refresh_tray(app);
}

fn log_path_errors(path_errors: &[(String, String)]) {
    for (path, err) in path_errors {
        shared::log_operation("PAWTROL_CLEAN_ERR", path, err);
    }
}

fn notify(app: &AppHandle, notice: &Notice) {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    if !current_settings().notifications_enabled {
        return;
    }
    let n = app.notification();
    if !matches!(n.permission_state(), Ok(PermissionState::Granted)) {
        return;
    }
    let mut builder = n.builder().title(&notice.title).body(&notice.body);
    if notice.urgent {
        builder = builder.sound("Sosumi");
    }
    let _ = builder.show();
}

pub async fn run_patrol(app: &AppHandle, trigger: PatrolTrigger) -> Result<PatrolRun, String> {
    let settings = current_settings();
    let ctx = PatrolCtx {
        state_path: state_path(),
        home: home_dir()?,
        clock: Box::new(now_ms),
        rules: PatrolRules::from_settings(&settings),
        permanent: !settings.use_trash,
        min_bytes: MIN_ITEM_BYTES,
    };
    let guard = BUSY.try_acquire(Busy::Patrol)?;
    let _ = app.emit(EVENT_STARTED, PatrolStarted { trigger });
    emit_status(app);

    let device_id = license::get_or_create_device_id();
    let outcome = patrol_with(
        &ctx,
        trigger,
        license::require_license(),
        move |reports| async move { scorer::score_probes(&reports, &device_id).await },
    )
    .await;
    drop(guard);

    let run = outcome.run;
    log_path_errors(&outcome.path_errors);
    shared::log_operation(
        "PAWTROL",
        &format!("{:?}", trigger),
        &match &run.error {
            Some(e) => format!("error: {}", e),
            None => format!(
                "cleaned {} items, freed {} bytes, {} to review",
                run.cleaned.len(),
                run.freed,
                run.review_count
            ),
        },
    );
    if run.freed > 0 {
        let _ = settings::add_bytes_freed(run.freed);
    }

    let _ = app.emit(EVENT_FINISHED, &run);
    emit_status(app);
    if trigger != PatrolTrigger::Manual {
        if let Some(notice) = &outcome.notice {
            notify(app, notice);
        }
    }
    Ok(run)
}

pub async fn review_clean(
    app: &AppHandle,
    ids: Vec<String>,
) -> Result<GuardianCleanResult, String> {
    let settings = current_settings();
    let home = home_dir()?;
    let guard = BUSY.try_acquire(Busy::ReviewClean)?;
    let out = review_clean_with(
        &state_path(),
        &home,
        &ids,
        !settings.use_trash,
        license::require_license(),
    )
    .await;
    drop(guard);
    let out = out?;

    log_path_errors(&out.path_errors);
    shared::log_operation(
        "PAWTROL_REVIEW_CLEAN",
        "guardian",
        &format!(
            "cleaned {} items, freed {} bytes",
            out.result.categories_cleaned, out.result.bytes_freed
        ),
    );
    if out.result.bytes_freed > 0 {
        let _ = settings::add_bytes_freed(out.result.bytes_freed);
    }
    emit_status(app);
    Ok(out.result)
}

pub fn dismiss(app: &AppHandle, ids: Vec<String>) -> Result<(), String> {
    dismiss_with(&state_path(), &ids, now_ms())?;
    emit_status(app);
    Ok(())
}

pub fn set_rules(app: &AppHandle, rules: PatrolRules) -> Result<PatrolRules, String> {
    let mut s = settings::load_settings_internal()?;
    rules.apply(&mut s);
    settings::save_settings_internal(&s)?;
    emit_status(app);
    Ok(PatrolRules::from_settings(&s))
}

pub fn set_patrol(app: &AppHandle, enabled: bool, auto_clean: bool) -> Result<(), String> {
    let s = settings::load_settings_internal()?;
    let rules = PatrolRules {
        enabled,
        safe_action: if auto_clean { SafeAction::Auto } else { SafeAction::Ask },
        ..PatrolRules::from_settings(&s)
    };
    set_rules(app, rules).map(|_| ())
}

async fn scheduler_tick(app: &AppHandle) {
    refresh_tray(app);
    let s = current_settings();
    if !s.pawtrol_enabled || !license::cached_license_active() || BUSY.current().is_some() {
        return;
    }
    let Ok((idle_ms, on_ac, free_bytes)) =
        tauri::async_runtime::spawn_blocking(|| (hid_idle_ms(), on_ac_power(), free_disk_bytes()))
            .await
    else {
        return;
    };

    let path = state_path();
    let state = load_state(&path);
    let now = now_ms();
    let decision = decide(&Conditions {
        now,
        last_patrol_at: state.last_patrol_at,
        last_scheduled_at: state.last_scheduled_at,
        last_low_disk_trigger_at: state.last_low_disk_trigger_at,
        last_critical_trigger_at: state.last_critical_trigger_at,
        idle_ms,
        on_ac,
        free_bytes,
        rules: PatrolRules::from_settings(&s),
    });
    if let Decision::Run(trigger) = decision {
        let _ = update_state(&path, |st| record_trigger(st, trigger, now));
        let _ = run_patrol(app, trigger).await;
    }
}

/// Starts the background patrol loop.
pub fn start_patrol_scheduler(app: AppHandle) {
    refresh_tray(&app);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_TICK).await;
        loop {
            scheduler_tick(&app).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::guardian::license::{cache_license, require_license_with};
    use crate::commands::guardian::test_support::{block_on, client, dead_url, TestDir};

    const GB: u64 = 1 << 30;
    const T0: u64 = 1_900_000_000_000;

    fn scored(category: &str, bytes: u64, score: f32) -> ScoredProbe {
        let data_loss = probes::data_loss(category);
        ScoredProbe {
            category: category.into(),
            display_name: probes::definition(category)
                .map(|d| d.display_name.to_string())
                .unwrap_or_else(|| category.into()),
            cleanable_bytes: bytes,
            score,
            confidence: 0.9,
            details: "d".into(),
            user_data: data_loss.is_some(),
            data_loss: data_loss.map(String::from),
        }
    }

    fn ids(v: &[ScoredProbe]) -> Vec<&str> {
        v.iter().map(|s| s.category.as_str()).collect()
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn partition_sends_anything_reaching_protected_data_to_review() {
        use crate::commands::data_guard::{static_verdict, Verdict};
        let scores = probes::CATEGORIES.iter().map(|d| scored(d.id, MIN_ITEM_BYTES, 99.0)).collect();
        let p = partition(scores, &HashMap::new(), T0, 0);
        assert!(!p.safe.is_empty());
        for s in &p.safe {
            assert!(probes::autonomous_safe(&s.category), "{}", s.category);
            assert!(probes::data_loss(&s.category).is_none());
            for path in probes::paths_for(&s.category, Path::new("/Users/tester")) {
                assert!(
                    matches!(static_verdict(&path), Verdict::NotProtected | Verdict::CacheLeaf),
                    "{} auto-cleans {}",
                    s.category,
                    path.display()
                );
            }
        }
        for d in probes::CATEGORIES.iter().filter(|d| d.data_loss.is_some()) {
            assert!(p.review.iter().any(|s| s.category == d.id), "{}", d.id);
        }
    }

    #[test]
    fn autonomous_patrol_never_touches_protected_user_data() {
        use crate::commands::data_guard::fixtures;
        use crate::commands::test_support::{canon, workspace_tempdir};
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        let data = TestDir::new("patrol-guard-data");
        let ctx = PatrolCtx {
            state_path: data.path().join(STATE_FILE),
            home: fx.home.clone(),
            clock: Box::new(|| T0),
            rules: PatrolRules::default(),
            permanent: true,
            min_bytes: 0,
        };
        let out = block_on(patrol_with(&ctx, PatrolTrigger::Schedule, async { Ok(()) }, |reports| async move {
            Ok(reports.iter().map(|r| scored(&r.category, r.cleanable_bytes, 99.0)).collect())
        }));

        assert!(out.run.error.is_none(), "{:?}", out.run.error);
        fx.assert_intact();
        assert!(!fx.path("Library/Application Support/Slack/Cache").exists(), "caches are still auto-cleaned");
        let pending: Vec<String> = load_state(&ctx.state_path).pending_review.into_iter().map(|i| i.id).collect();
        assert!(pending.contains(&"docker_vm".to_string()), "{pending:?}");
        assert!(pending.contains(&"xcode_archives".to_string()), "{pending:?}");
    }

    #[test]
    fn partition_rules() {
        let mut spoofed = scored("docker_vm", 9 * GB, 99.0);
        spoofed.user_data = false;
        let scores = vec![
            scored("node", 2 * GB, 90.0),
            scored("rust", 2 * GB, 70.0),
            scored("python", 2 * GB, 70.1),
            scored("ide", 2 * GB, 40.0),
            scored("homebrew", 2 * GB, 39.9),
            spoofed,
            scored("ai_ml", 2 * GB, 10.0),
            scored("trash", 2 * GB, 95.0),
            scored("xcode", MIN_ITEM_BYTES - 1, 99.0),
            scored("app_data", MIN_ITEM_BYTES - 1, 55.0),
            scored("docker", 2 * GB, 90.0),
            scored("apps", 2 * GB, 90.0),
            scored("not_a_category", 2 * GB, 99.0),
        ];
        let snoozed = HashMap::from([("docker".to_string(), T0 + 1), ("apps".to_string(), T0 - 1)]);
        let p = partition(scores, &snoozed, T0, MIN_ITEM_BYTES);
        assert_eq!(ids(&p.safe), ["node", "python", "apps"]);
        assert_eq!(
            ids(&p.review),
            ["rust", "ide", "docker_vm", "ai_ml", "trash"]
        );
    }

    fn rules_with(auto_clean: bool) -> PatrolRules {
        PatrolRules {
            safe_action: if auto_clean { SafeAction::Auto } else { SafeAction::Ask },
            ..PatrolRules::default()
        }
    }

    struct Env {
        home: TestDir,
        data: TestDir,
    }

    impl Env {
        fn new(name: &str) -> Self {
            let env = Env {
                home: TestDir::new(&format!("{}-home", name)),
                data: TestDir::new(&format!("{}-data", name)),
            };
            env.home.write(".npm/_cacache/blob", 64 * 1024);
            env.home.write(
                "Library/Containers/com.docker.docker/Data/vms/0/disk.raw",
                64 * 1024,
            );
            env.home.write(".Trash/old.dmg", 64 * 1024);
            env.home.write("Library/Logs/tiny.log", 10);
            env
        }

        fn state_path(&self) -> PathBuf {
            self.data.path().join(STATE_FILE)
        }

        fn ctx(&self, auto_clean: bool, now: u64) -> PatrolCtx {
            self.ctx_with(rules_with(auto_clean), now)
        }

        fn ctx_with(&self, rules: PatrolRules, now: u64) -> PatrolCtx {
            PatrolCtx {
                state_path: self.state_path(),
                home: self.home.path().to_path_buf(),
                clock: Box::new(move || now),
                rules,
                permanent: true,
                min_bytes: 16 * 1024,
            }
        }

        fn npm(&self) -> PathBuf {
            self.home.path().join(".npm/_cacache/blob")
        }

        fn vm(&self) -> PathBuf {
            self.home
                .path()
                .join("Library/Containers/com.docker.docker/Data/vms/0/disk.raw")
        }

        fn trash(&self) -> PathBuf {
            self.home.path().join(".Trash/old.dmg")
        }

        fn patrol(&self, auto_clean: bool, now: u64) -> PatrolOutcome {
            self.patrol_with(rules_with(auto_clean), PatrolTrigger::Schedule, now)
        }

        fn patrol_with(&self, rules: PatrolRules, trigger: PatrolTrigger, now: u64) -> PatrolOutcome {
            self.patrol_scored(rules, trigger, now, |_| 90.0)
        }

        fn patrol_scored(
            &self,
            rules: PatrolRules,
            trigger: PatrolTrigger,
            now: u64,
            score: fn(&str) -> f32,
        ) -> PatrolOutcome {
            block_on(patrol_with(
                &self.ctx_with(rules, now),
                trigger,
                async { Ok(()) },
                move |reports| async move {
                    Ok(reports
                        .iter()
                        .map(|r| scored(&r.category, r.cleanable_bytes, score(&r.category)))
                        .collect())
                },
            ))
        }

        fn pending_ids(&self) -> Vec<String> {
            let mut ids: Vec<String> = load_state(&self.state_path())
                .pending_review
                .into_iter()
                .map(|i| i.id)
                .collect();
            ids.sort();
            ids
        }
    }

    fn rules(review_action: ReviewAction, data_action: DataAction) -> PatrolRules {
        PatrolRules {
            review_action,
            data_action,
            ..PatrolRules::default()
        }
    }

    fn node_mid_score(category: &str) -> f32 {
        if category == "node" { 55.0 } else { 90.0 }
    }

    #[test]
    fn critical_run_cleans_safe_items_even_when_set_to_ask() {
        let env = Env::new("patrol-critical");
        env.home.write(".Trash/big.dmg", 256 * 1024);
        let ask_quiet = PatrolRules {
            safe_action: SafeAction::Ask,
            ..rules(ReviewAction::Quiet, DataAction::Quiet)
        };
        let out = env.patrol_with(ask_quiet, PatrolTrigger::Critical, T0);

        assert!(!env.npm().exists(), "critical runs clean safe items");
        assert!(env.vm().exists() && env.trash().exists());
        assert_eq!(out.run.trigger, PatrolTrigger::Critical);
        assert_eq!(env.pending_ids(), ["docker_vm", "trash"]);

        let state = load_state(&env.state_path());
        let trash = state.pending_review.iter().find(|i| i.id == "trash").unwrap();
        let notice = out.notice.unwrap();
        assert!(notice.urgent);
        assert_eq!(notice.title, "Your Mac is almost out of space");
        assert_eq!(
            notice.body,
            format!(
                "Freed {} · 2 items need you · biggest: Trash {}",
                format_size(out.run.freed),
                format_size(trash.size)
            )
        );

        let again = env.patrol_with(ask_quiet, PatrolTrigger::Critical, T0 + 2 * HOUR);
        assert!(again.notice.is_some_and(|n| n.urgent), "critical runs always notify");
    }

    #[test]
    fn critical_run_ignores_data_the_user_ignores() {
        let env = Env::new("patrol-critical-ignore");
        let out = env.patrol_with(
            rules(ReviewAction::Notify, DataAction::Ignore),
            PatrolTrigger::Critical,
            T0,
        );
        assert!(env.pending_ids().is_empty());
        assert_eq!(
            out.notice.unwrap().body,
            format!("Freed {}", format_size(out.run.freed))
        );
    }

    #[test]
    fn critical_notice_texts() {
        let n = critical_notice(&run_with(&[]), &[]).unwrap();
        assert_eq!(n.title, "Your Mac is almost out of space");
        assert_eq!(n.body, "Nothing left that Kyra can clean on its own");
        let one = vec![item("ai_ml", 12 * GB, false)];
        let n = critical_notice(&run_with(&[]), &one).unwrap();
        assert_eq!(n.body, "1 item needs you · biggest: AI & ML Models 12.0 GB");
        let mut failed = run_with(&[("Xcode", GB)]);
        failed.error = Some("x".into());
        assert!(critical_notice(&failed, &one).is_none());
    }

    #[test]
    fn review_and_data_actions_decide_what_is_announced() {
        let title = |review, data| {
            let env = Env::new("patrol-actions");
            let out = env.patrol_scored(rules(review, data), PatrolTrigger::Schedule, T0, node_mid_score);
            assert!(env.npm().exists(), "mid-score items are never auto-cleaned");
            assert_eq!(env.pending_ids(), ["docker_vm", "node", "trash"]);
            let state = load_state(&env.state_path());
            assert!(state.pending_review.iter().all(|i| !i.safe));
            out.notice.map(|n| n.title)
        };
        use DataAction as D;
        use ReviewAction as R;
        assert_eq!(title(R::Notify, D::Notify).as_deref(), Some("3 items need your review"));
        assert_eq!(title(R::Quiet, D::Notify).as_deref(), Some("2 items need your review"));
        assert_eq!(title(R::Notify, D::Quiet).as_deref(), Some("1 item needs your review"));
        assert_eq!(title(R::Quiet, D::Quiet), None);
    }

    #[test]
    fn quiet_items_are_left_out_of_the_freed_summary() {
        let env = Env::new("patrol-quiet-freed");
        let out = env.patrol_with(rules(ReviewAction::Notify, DataAction::Quiet), PatrolTrigger::Schedule, T0);
        assert!(!env.npm().exists());
        assert_eq!(out.notice.unwrap().body, "Node.js");
        assert_eq!(out.run.review_count, 2, "quiet items are still queued");
    }

    #[test]
    fn ignored_data_leaves_the_queue_on_the_next_run() {
        let env = Env::new("patrol-ignore");
        env.patrol(false, T0);
        assert_eq!(env.pending_ids(), ["docker_vm", "node", "trash"]);

        let ignore = PatrolRules {
            safe_action: SafeAction::Ask,
            ..rules(ReviewAction::Notify, DataAction::Ignore)
        };
        let out = env.patrol_with(ignore, PatrolTrigger::Schedule, T0 + DAY);
        assert_eq!(env.pending_ids(), ["node"]);
        assert_eq!(out.run.review_count, 0);
        assert!(out.notice.is_none());
        assert!(env.vm().exists() && env.trash().exists());
    }

    #[test]
    fn plan_follows_the_safe_action() {
        let part = || Partition {
            safe: vec![scored("node", GB, 90.0)],
            review: vec![scored("ide", GB, 50.0), scored("docker_vm", GB, 55.0)],
        };
        let ask = rules_with(false);
        let p = plan(part(), &PatrolRules::default(), PatrolTrigger::Schedule);
        assert_eq!(ids(&p.clean), ["node"]);
        assert_eq!(p.queue.iter().map(|(s, safe)| (s.category.as_str(), *safe)).collect::<Vec<_>>(), [("ide", false), ("docker_vm", false)]);

        let p = plan(part(), &ask, PatrolTrigger::Schedule);
        assert!(p.clean.is_empty());
        assert_eq!(p.queue.iter().map(|(s, safe)| (s.category.as_str(), *safe)).collect::<Vec<_>>(), [("node", true), ("ide", false), ("docker_vm", false)]);

        for trigger in [PatrolTrigger::LowDisk, PatrolTrigger::Manual] {
            assert!(plan(part(), &ask, trigger).clean.is_empty(), "{trigger:?}");
        }
        assert_eq!(ids(&plan(part(), &ask, PatrolTrigger::Critical).clean), ["node"]);

        let ignore = PatrolRules { data_action: DataAction::Ignore, ..ask };
        let p = plan(part(), &ignore, PatrolTrigger::Schedule);
        assert_eq!(p.queue.iter().map(|(s, _)| s.category.as_str()).collect::<Vec<_>>(), ["node", "ide"]);
    }

    #[test]
    fn rules_serialize_for_the_frontend_and_clamp_on_apply() {
        assert_eq!(
            serde_json::to_string(&PatrolRules::default()).unwrap(),
            r#"{"enabled":true,"frequency":"daily","low_gb":10,"critical_gb":3,"safe_action":"auto","review_action":"notify","data_action":"notify"}"#
        );
        let parsed: PatrolRules = serde_json::from_str(
            r#"{"enabled":false,"frequency":"low_disk_only","low_gb":5,"critical_gb":9,"safe_action":"ask","review_action":"quiet","data_action":"ignore"}"#,
        )
        .unwrap();
        let mut s = AppSettings::default();
        parsed.apply(&mut s);
        let back = PatrolRules::from_settings(&s);
        assert_eq!(back, PatrolRules { critical_gb: 4, ..parsed });
        assert!(serde_json::from_str::<PatrolRules>(r#"{"enabled":true,"frequency":"hourly","low_gb":10,"critical_gb":3,"safe_action":"auto","review_action":"notify","data_action":"notify"}"#).is_err());
        assert_eq!(serde_json::to_value(PatrolTrigger::Critical).unwrap(), "critical");
    }

    #[test]
    fn auto_clean_on_cleans_only_safe_items() {
        let env = Env::new("patrol-auto");
        let out = env.patrol(true, T0);

        assert!(!env.npm().exists());
        assert!(env.vm().exists(), "user data must never be auto-cleaned");
        assert!(env.trash().exists(), "the Trash must never be auto-emptied");
        assert!(env.home.path().join("Library/Logs/tiny.log").exists());

        let run = &out.run;
        assert!(run.error.is_none());
        assert_eq!(run.trigger, PatrolTrigger::Schedule);
        assert_eq!(run.cleaned.len(), 1);
        assert_eq!(run.cleaned[0].name, "Node.js");
        assert!(run.freed >= 64 * 1024);
        assert_eq!(run.review_count, 2);

        let state = load_state(&env.state_path());
        assert_eq!(state.last_patrol_at, Some(T0));
        assert_eq!(state.freed_total, run.freed);
        assert_eq!(state.freed_last, run.freed);
        assert_eq!(state.history, vec![run.clone()]);
        let mut pending: Vec<&str> = state.pending_review.iter().map(|i| i.id.as_str()).collect();
        pending.sort();
        assert_eq!(pending, ["docker_vm", "trash"]);
        for item in &state.pending_review {
            assert!(item.user_data && !item.safe && item.data_loss.is_some());
            assert_eq!(item.found_at, T0);
        }
        let trash = state
            .pending_review
            .iter()
            .find(|i| i.id == "trash")
            .unwrap();
        assert_eq!(
            trash.data_loss.as_deref(),
            Some("Permanently empties your Trash")
        );

        let notice = out.notice.unwrap();
        assert!(notice.title.starts_with("Pawtrol freed "));
        assert_eq!(notice.body, "Node.js · 2 need your review");
    }

    #[test]
    fn auto_clean_off_queues_safe_items_for_one_tap() {
        let env = Env::new("patrol-manual");
        let out = env.patrol(false, T0);

        assert!(env.npm().exists() && env.vm().exists() && env.trash().exists());
        assert_eq!(out.run.freed, 0);
        assert!(out.run.cleaned.is_empty());
        assert_eq!(out.run.review_count, 2);

        let state = load_state(&env.state_path());
        let node = state
            .pending_review
            .iter()
            .find(|i| i.id == "node")
            .unwrap();
        assert!(node.safe && !node.user_data);
        assert_eq!(state.pending_review.len(), 3);
        assert_eq!(
            out.notice.unwrap(),
            Notice {
                title: format!("Pawtrol found {} safe to clean", format_size(node.size)),
                body: "Open Kyra to clean it in one tap".into(),
                urgent: false,
            }
        );

        let again = env.patrol(false, T0 + DAY);
        assert!(
            again.notice.is_none(),
            "unchanged findings are not re-announced"
        );
        let state = load_state(&env.state_path());
        assert!(state.pending_review.iter().all(|i| i.found_at == T0));
        assert_eq!(state.history.len(), 2);
        assert_eq!(state.history[0].started_at, T0 + DAY);
    }

    #[test]
    fn dismissed_items_are_snoozed_for_30_days() {
        let env = Env::new("patrol-snooze");
        env.patrol(false, T0);
        dismiss_with(&env.state_path(), &strings(&["docker_vm", "bogus"]), T0).unwrap();

        let state = load_state(&env.state_path());
        assert!(state.pending_review.iter().all(|i| i.id != "docker_vm"));
        assert_eq!(state.snoozed.get("docker_vm"), Some(&(T0 + SNOOZE_MS)));
        assert!(!state.snoozed.contains_key("bogus"));

        env.patrol(false, T0 + 29 * DAY);
        let state = load_state(&env.state_path());
        assert!(state.pending_review.iter().all(|i| i.id != "docker_vm"));

        let out = env.patrol(false, T0 + 31 * DAY);
        let state = load_state(&env.state_path());
        assert!(state.pending_review.iter().any(|i| i.id == "docker_vm"));
        assert!(!state.snoozed.contains_key("docker_vm"));
        assert!(out.notice.is_some(), "a returning item counts as new");
        assert!(env.vm().exists());
    }

    #[test]
    fn inactive_license_skips_the_patrol_and_records_the_error() {
        let env = Env::new("patrol-nolicense");
        let mut scored_called = false;
        let out = block_on(patrol_with(
            &env.ctx(true, T0),
            PatrolTrigger::LowDisk,
            require_license_with(&client(), &dead_url(), "d", env.data.path()),
            |_| {
                scored_called = true;
                async { Ok(Vec::new()) }
            },
        ));
        assert!(!scored_called);
        assert!(env.npm().exists() && env.vm().exists() && env.trash().exists());
        assert_eq!(out.run.error.as_deref(), Some("No active license"));
        assert!(out.notice.is_none());

        let state = load_state(&env.state_path());
        assert_eq!(state.last_patrol_at, None);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].trigger, PatrolTrigger::LowDisk);
    }

    #[test]
    fn scoring_license_rejection_cleans_nothing() {
        let env = Env::new("patrol-403");
        let out = block_on(patrol_with(
            &env.ctx(true, T0),
            PatrolTrigger::Schedule,
            async { Ok(()) },
            |_| async { Err("License expired".to_string()) },
        ));
        assert!(env.npm().exists());
        assert_eq!(out.run.error.as_deref(), Some("License expired"));
        assert!(load_state(&env.state_path()).pending_review.is_empty());
    }

    #[test]
    fn review_clean_only_cleans_known_ids() {
        let env = Env::new("review-clean");
        env.patrol(false, T0);
        let before = load_state(&env.state_path());
        assert!(before.pending_review.iter().all(|i| i.id != "system"));

        let out = block_on(review_clean_with(
            &env.state_path(),
            env.home.path(),
            &strings(&["docker_vm", "docker_vm", "system", "../Documents", ""]),
            true,
            async { Ok(()) },
        ))
        .unwrap();

        assert!(!env.vm().exists(), "confirmed user data is cleaned");
        assert!(env.npm().exists() && env.trash().exists());
        assert!(env.home.path().join("Library/Logs/tiny.log").exists());
        assert_eq!(out.result.categories_cleaned, 1);
        assert!(out.result.bytes_freed >= 64 * 1024);
        assert_eq!(out.result.errors.len(), 3);

        let state = load_state(&env.state_path());
        assert!(state.pending_review.iter().all(|i| i.id != "docker_vm"));
        assert_eq!(state.pending_review.len(), 2);
        assert_eq!(state.freed_total, out.result.bytes_freed);
    }

    #[test]
    fn review_clean_without_license_deletes_nothing() {
        let env = Env::new("review-nolicense");
        env.patrol(false, T0);
        let res = block_on(review_clean_with(
            &env.state_path(),
            env.home.path(),
            &strings(&["node", "docker_vm", "trash"]),
            true,
            require_license_with(&client(), &dead_url(), "d", env.data.path()),
        ));
        assert_eq!(res.err().as_deref(), Some("No active license"));
        assert!(env.npm().exists() && env.vm().exists() && env.trash().exists());
        assert_eq!(load_state(&env.state_path()).pending_review.len(), 3);
    }

    #[test]
    fn active_cached_license_lets_the_patrol_run() {
        let env = Env::new("patrol-licensed");
        cache_license(
            env.data.path(),
            &LicenseStatus {
                active: true,
                expires: None,
            },
        );
        let out = block_on(patrol_with(
            &env.ctx(true, T0),
            PatrolTrigger::Manual,
            require_license_with(&client(), &dead_url(), "d", env.data.path()),
            |reports| async move {
                Ok(reports
                    .iter()
                    .map(|r| scored(&r.category, r.cleanable_bytes, 90.0))
                    .collect())
            },
        ));
        assert!(out.run.error.is_none());
        assert!(!env.npm().exists());
    }

    #[test]
    fn state_round_trips_and_tolerates_corruption() {
        let dir = TestDir::new("patrol-state");
        let path = dir.path().join("nested").join(STATE_FILE);
        assert_eq!(load_state(&path), PatrolState::default());

        let run = PatrolRun {
            started_at: 1,
            finished_at: 2,
            trigger: PatrolTrigger::LowDisk,
            cleaned: vec![CleanedItem {
                name: "Node.js".into(),
                size: 5,
            }],
            freed: 5,
            review_count: 1,
            error: None,
        };
        let state = PatrolState {
            last_patrol_at: Some(2),
            freed_total: 10,
            freed_last: 5,
            history: vec![run],
            pending_review: vec![review_item(&scored("ai_ml", GB, 55.0), 3, false)],
            snoozed: HashMap::from([("docker_vm".to_string(), 9)]),
            last_low_disk_trigger_at: Some(4),
            last_scheduled_at: Some(5),
            last_critical_trigger_at: Some(6),
        };
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path), state);

        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"trigger\": \"low_disk\""));

        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_state(&path), PatrolState::default());
        std::fs::write(&path, r#"{"freed_total": 7}"#).unwrap();
        assert_eq!(load_state(&path).freed_total, 7);

        update_state(&path, |s| s.freed_total += 1).unwrap();
        assert_eq!(load_state(&path).freed_total, 8);
    }

    #[test]
    fn history_keeps_the_last_20_newest_first() {
        let env = Env::new("patrol-history");
        for i in 0..25 {
            env.patrol(false, T0 + i * DAY);
        }
        let state = load_state(&env.state_path());
        assert_eq!(state.history.len(), HISTORY_LEN);
        assert_eq!(state.history[0].started_at, T0 + 24 * DAY);
        assert_eq!(state.history[19].started_at, T0 + 5 * DAY);
    }

    fn cond() -> Conditions {
        Conditions {
            now: T0,
            last_patrol_at: Some(T0 - 25 * HOUR),
            last_scheduled_at: None,
            last_low_disk_trigger_at: None,
            idle_ms: Some(10 * MINUTE),
            on_ac: true,
            free_bytes: Some(100 * GB),
            ..Default::default()
        }
    }

    #[test]
    fn scheduler_runs_daily_when_idle_on_ac() {
        let run = Decision::Run(PatrolTrigger::Schedule);
        assert_eq!(decide(&cond()), run);
        assert_eq!(
            decide(&Conditions {
                last_patrol_at: None,
                ..cond()
            }),
            run
        );
        assert_eq!(
            decide(&Conditions {
                last_patrol_at: Some(T0 - 23 * HOUR),
                ..cond()
            }),
            Decision::Wait("not due yet")
        );
        assert_eq!(
            decide(&Conditions {
                on_ac: false,
                ..cond()
            }),
            Decision::Wait("on battery")
        );
        assert_eq!(
            decide(&Conditions {
                idle_ms: Some(4 * MINUTE),
                ..cond()
            }),
            Decision::Wait("Mac is in use")
        );
        assert_eq!(
            decide(&Conditions {
                idle_ms: None,
                ..cond()
            }),
            Decision::Wait("Mac is in use")
        );
        assert_eq!(
            decide(&Conditions {
                idle_ms: Some(5 * MINUTE),
                ..cond()
            }),
            run
        );
    }

    #[test]
    fn overdue_patrol_runs_without_idle_but_needs_ac() {
        let busy = Conditions {
            idle_ms: Some(0),
            last_patrol_at: Some(T0 - 72 * HOUR),
            ..cond()
        };
        assert_eq!(decide(&busy), Decision::Run(PatrolTrigger::Schedule));
        assert_eq!(
            decide(&Conditions {
                on_ac: false,
                ..busy
            }),
            Decision::Wait("on battery")
        );
        assert_eq!(
            decide(&Conditions {
                last_patrol_at: Some(T0 - 71 * HOUR),
                ..busy
            }),
            Decision::Wait("Mac is in use")
        );
        assert_eq!(
            decide(&Conditions {
                last_patrol_at: None,
                ..busy
            }),
            Decision::Wait("Mac is in use")
        );
    }

    #[test]
    fn low_disk_runs_immediately_with_a_cooldown() {
        let low = Conditions {
            free_bytes: Some(5 * GB),
            last_patrol_at: Some(T0 - HOUR * 2),
            idle_ms: Some(0),
            on_ac: false,
            ..cond()
        };
        assert_eq!(decide(&low), Decision::Run(PatrolTrigger::LowDisk));
        assert_eq!(
            decide(&Conditions {
                last_low_disk_trigger_at: Some(T0 - 5 * HOUR),
                ..low
            }),
            Decision::Wait("not due yet")
        );
        assert_eq!(
            decide(&Conditions {
                last_low_disk_trigger_at: Some(T0 - 6 * HOUR),
                ..low
            }),
            Decision::Run(PatrolTrigger::LowDisk)
        );
        assert_eq!(
            decide(&Conditions {
                free_bytes: Some(10 * GB),
                ..low
            }),
            Decision::Wait("not due yet")
        );
        assert_eq!(
            decide(&Conditions {
                free_bytes: None,
                ..low
            }),
            Decision::Wait("not due yet")
        );
        assert_eq!(
            decide(&Conditions {
                rules: PatrolRules { low_gb: 0, critical_gb: 0, ..PatrolRules::default() },
                free_bytes: Some(0),
                ..low
            }),
            Decision::Wait("not due yet")
        );
    }

    #[test]
    fn scheduled_patrols_are_at_most_hourly() {
        let limited = Decision::Wait("ran within the last hour");
        let recent = Some(T0 - 59 * MINUTE);
        assert_eq!(
            decide(&Conditions {
                last_scheduled_at: recent,
                ..cond()
            }),
            limited
        );
        assert_eq!(
            decide(&Conditions {
                last_scheduled_at: recent,
                free_bytes: Some(0),
                ..cond()
            }),
            limited
        );
        assert_eq!(
            decide(&Conditions {
                last_scheduled_at: Some(T0 - HOUR),
                ..cond()
            }),
            Decision::Run(PatrolTrigger::Schedule)
        );
    }

    fn with_rules(rules: PatrolRules, c: Conditions) -> Conditions {
        Conditions { rules, ..c }
    }

    fn every(frequency: PawtrolFrequency) -> PatrolRules {
        PatrolRules { frequency, ..PatrolRules::default() }
    }

    #[test]
    fn frequency_sets_the_schedule_interval() {
        use PawtrolFrequency as F;
        let run = Decision::Run(PatrolTrigger::Schedule);
        let since = |f, ago: u64| decide(&with_rules(every(f), Conditions { last_patrol_at: Some(T0 - ago), ..cond() }));
        for (f, interval) in [(F::SixHours, 6 * HOUR), (F::Daily, DAY), (F::Weekly, 7 * DAY)] {
            assert_eq!(interval_ms(f), Some(interval));
            assert_eq!(since(f, interval), run, "{f:?}");
            assert_eq!(since(f, interval - 1), Decision::Wait("not due yet"), "{f:?}");

            let busy = |ago: u64| {
                decide(&with_rules(every(f), Conditions { idle_ms: Some(0), last_patrol_at: Some(T0 - ago), ..cond() }))
            };
            assert_eq!(busy(3 * interval), run, "overdue {f:?}");
            assert_eq!(busy(3 * interval - 1), Decision::Wait("Mac is in use"), "{f:?}");
        }

        assert_eq!(interval_ms(F::LowDiskOnly), None);
        assert_eq!(since(F::LowDiskOnly, 365 * DAY), Decision::Wait("scheduled runs are off"));
        let low = Conditions { free_bytes: Some(5 * GB), ..cond() };
        assert_eq!(decide(&with_rules(every(F::LowDiskOnly), low)), Decision::Run(PatrolTrigger::LowDisk));

        let st = status_from(PatrolState::default(), every(F::LowDiskOnly), false, T0);
        assert_eq!(st.next_patrol_at, None);
        let s = PatrolState { last_patrol_at: Some(T0 - HOUR), ..Default::default() };
        let st = status_from(s, every(F::Weekly), false, T0);
        assert_eq!(st.next_patrol_at, Some(T0 - HOUR + 7 * DAY));
    }

    #[test]
    fn paused_rules_never_run() {
        let paused = PatrolRules { enabled: false, ..PatrolRules::default() };
        for free in [0, 2 * GB, 5 * GB, 100 * GB] {
            let c = with_rules(paused, Conditions { free_bytes: Some(free), last_patrol_at: None, ..cond() });
            assert_eq!(decide(&c), Decision::Wait("paused"));
        }
    }

    #[test]
    fn critical_tier_bypasses_idle_and_power() {
        let critical = Conditions {
            free_bytes: Some(2 * GB),
            idle_ms: Some(0),
            on_ac: false,
            last_patrol_at: Some(T0 - 30 * MINUTE),
            last_low_disk_trigger_at: Some(T0 - HOUR),
            ..cond()
        };
        assert_eq!(decide(&critical), Decision::Run(PatrolTrigger::Critical));
        assert_eq!(
            decide(&with_rules(every(PawtrolFrequency::LowDiskOnly), critical)),
            Decision::Run(PatrolTrigger::Critical)
        );
        assert_eq!(
            decide(&Conditions { free_bytes: Some(3 * GB), ..critical }),
            Decision::Wait("not due yet"),
            "exactly at the critical threshold is only low"
        );
        assert_eq!(
            decide(&Conditions { free_bytes: Some(3 * GB), last_low_disk_trigger_at: None, ..critical }),
            Decision::Run(PatrolTrigger::LowDisk)
        );
        assert_eq!(
            decide(&Conditions { last_scheduled_at: Some(T0 - 59 * MINUTE), ..critical }),
            Decision::Wait("ran within the last hour"),
            "the hourly cap still applies"
        );
        assert_eq!(
            decide(&Conditions { last_critical_trigger_at: Some(T0 - 2 * HOUR + 1), ..critical }),
            Decision::Wait("not due yet")
        );
        assert_eq!(
            decide(&Conditions { last_critical_trigger_at: Some(T0 - 2 * HOUR), ..critical }),
            Decision::Run(PatrolTrigger::Critical)
        );
        let off = PatrolRules { critical_gb: 0, ..PatrolRules::default() };
        assert_eq!(
            decide(&with_rules(off, Conditions { free_bytes: Some(0), last_low_disk_trigger_at: None, ..critical })),
            Decision::Run(PatrolTrigger::LowDisk)
        );
    }

    #[test]
    fn tier_cooldowns_across_ticks() {
        let mut state = PatrolState::default();
        let tick = |state: &PatrolState, now: u64, free: u64| {
            decide(&Conditions {
                now,
                last_patrol_at: state.last_patrol_at,
                last_scheduled_at: state.last_scheduled_at,
                last_low_disk_trigger_at: state.last_low_disk_trigger_at,
                last_critical_trigger_at: state.last_critical_trigger_at,
                idle_ms: Some(0),
                on_ac: false,
                free_bytes: Some(free),
                rules: PatrolRules::default(),
            })
        };
        let fire = |state: &mut PatrolState, now: u64, free: u64| {
            let d = tick(state, now, free);
            if let Decision::Run(t) = d {
                record_trigger(state, t, now);
                state.last_patrol_at = Some(now);
            }
            d
        };

        assert_eq!(fire(&mut state, T0, 2 * GB), Decision::Run(PatrolTrigger::Critical));
        assert_eq!(state.last_critical_trigger_at, Some(T0));
        assert_eq!(state.last_low_disk_trigger_at, Some(T0), "critical also restarts the low cooldown");
        assert_eq!(fire(&mut state, T0 + 30 * MINUTE, GB), Decision::Wait("ran within the last hour"));
        assert_eq!(fire(&mut state, T0 + HOUR, 2 * GB), Decision::Wait("not due yet"));
        assert_eq!(fire(&mut state, T0 + HOUR, 5 * GB), Decision::Wait("not due yet"));
        assert_eq!(fire(&mut state, T0 + 2 * HOUR, 2 * GB), Decision::Run(PatrolTrigger::Critical));
        assert_eq!(fire(&mut state, T0 + 3 * HOUR, 5 * GB), Decision::Wait("not due yet"));
        assert_eq!(fire(&mut state, T0 + 8 * HOUR, 5 * GB), Decision::Run(PatrolTrigger::LowDisk));
        assert_eq!(state.last_critical_trigger_at, Some(T0 + 2 * HOUR));
        assert_eq!(fire(&mut state, T0 + 9 * HOUR, 2 * GB), Decision::Run(PatrolTrigger::Critical));
        assert_eq!(state.last_scheduled_at, Some(T0 + 9 * HOUR));

        let mut manual = PatrolState::default();
        record_trigger(&mut manual, PatrolTrigger::Schedule, T0);
        assert_eq!(manual.last_scheduled_at, Some(T0));
        assert_eq!((manual.last_low_disk_trigger_at, manual.last_critical_trigger_at), (None, None));
    }

    #[test]
    fn next_patrol_estimate_respects_interval_and_rate_limit() {
        let mut s = PatrolState::default();
        assert_eq!(next_patrol_estimate(&s, T0, DAY), T0);
        s.last_patrol_at = Some(T0 - HOUR);
        assert_eq!(next_patrol_estimate(&s, T0, DAY), T0 - HOUR + DAY);
        s.last_patrol_at = Some(T0 - 3 * DAY);
        assert_eq!(next_patrol_estimate(&s, T0, DAY), T0);
        s.last_scheduled_at = Some(T0 - 10 * MINUTE);
        assert_eq!(next_patrol_estimate(&s, T0, DAY), T0 + 50 * MINUTE);

        let paused = PatrolRules { enabled: false, ..PatrolRules::default() };
        let st = status_from(s.clone(), paused, false, T0);
        assert_eq!(st.next_patrol_at, None);
        assert!(!st.enabled && st.auto_clean && st.rules == paused);
        let st = status_from(s, rules_with(false), true, T0);
        assert_eq!(st.next_patrol_at, Some(T0 + 50 * MINUTE));
        assert!(st.running && st.enabled && !st.auto_clean);
    }

    fn item(id: &str, size: u64, safe: bool) -> ReviewItem {
        ReviewItem {
            safe,
            ..review_item(&scored(id, size, 55.0), T0, safe)
        }
    }

    fn run_with(cleaned: &[(&str, u64)]) -> PatrolRun {
        PatrolRun {
            started_at: T0,
            finished_at: T0,
            trigger: PatrolTrigger::Schedule,
            cleaned: cleaned
                .iter()
                .map(|(n, s)| CleanedItem {
                    name: n.to_string(),
                    size: *s,
                })
                .collect(),
            freed: cleaned.iter().map(|(_, s)| s).sum(),
            review_count: 0,
            error: None,
        }
    }

    fn all_new(items: &[ReviewItem]) -> HashSet<String> {
        items.iter().map(|i| i.id.clone()).collect()
    }

    #[test]
    fn notification_texts() {
        let cleaned = run_with(&[
            ("Xcode", GB),
            ("Node.js", GB / 2),
            ("Homebrew", GB / 4),
            ("Rust / Cargo", 1),
        ]);
        let n = build_notice(&cleaned, &[], &HashSet::new()).unwrap();
        assert_eq!(
            n.title,
            format!("Pawtrol freed {}", format_size(cleaned.freed))
        );
        assert_eq!(n.body, "Xcode, Node.js, Homebrew");

        let review = vec![
            item("docker_vm", 3 * GB, false),
            item("ai_ml", 12 * GB, false),
        ];
        let n = build_notice(&cleaned, &review, &HashSet::new()).unwrap();
        assert_eq!(n.body, "Xcode, Node.js, Homebrew · 2 need your review");

        let n = build_notice(&run_with(&[]), &review, &all_new(&review)).unwrap();
        assert_eq!(n.title, "2 items need your review");
        assert_eq!(n.body, "AI & ML Models · 12.0 GB");

        let one = vec![item("trash", 500 * 1024 * 1024, false)];
        let n = build_notice(&run_with(&[]), &one, &all_new(&one)).unwrap();
        assert_eq!(n.title, "1 item needs your review");
        assert_eq!(n.body, "Trash · 500.0 MB");

        let mixed = vec![
            item("node", 2 * GB, true),
            item("xcode", GB, true),
            item("ai_ml", GB, false),
        ];
        let n = build_notice(&run_with(&[]), &mixed, &all_new(&mixed)).unwrap();
        assert_eq!(n.title, "Pawtrol found 3.0 GB safe to clean");
        assert_eq!(n.body, "Open Kyra to clean it in one tap");

        assert!(build_notice(&run_with(&[]), &[], &HashSet::new()).is_none());
        assert!(build_notice(&run_with(&[]), &mixed, &HashSet::new()).is_none());
        let mut failed = cleaned.clone();
        failed.error = Some("x".into());
        assert!(build_notice(&failed, &review, &all_new(&review)).is_none());
    }

    #[test]
    fn format_size_matches_the_frontend() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(150 * 1024 * 1024), "150.0 MB");
        assert_eq!(format_size(1000 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_size(5 * GB / 2), "2.5 GB");
    }

    #[test]
    fn parses_idle_time_and_power_source() {
        let ioreg = "  | |   \"HIDIdleTime\" = 290584946625\n  | |   \"Other\" = 1";
        assert_eq!(parse_hid_idle_ms(ioreg), Some(290_584));
        assert_eq!(parse_hid_idle_ms("nothing here"), None);
        assert_eq!(parse_hid_idle_ms("\"HIDIdleTime\" = garbage"), None);

        assert!(parse_on_ac(
            "Now drawing from 'AC Power'\n -InternalBattery-0"
        ));
        assert!(!parse_on_ac(
            "Now drawing from 'Battery Power'\n -InternalBattery-0"
        ));
        assert!(!parse_on_ac(""));
    }

    #[test]
    fn busy_lock_blocks_concurrent_work() {
        let lock = BusyLock::new();
        let guard = lock.try_acquire(Busy::Patrol).unwrap();
        assert_eq!(lock.current(), Some(Busy::Patrol));
        assert_eq!(
            lock.try_acquire(Busy::Patrol).err().as_deref(),
            Some("Pawtrol is already running")
        );
        assert!(lock.try_acquire(Busy::ReviewClean).is_err());
        drop(guard);
        assert_eq!(lock.current(), None);
        let _clean = lock.try_acquire(Busy::ReviewClean).unwrap();
        assert_eq!(
            lock.try_acquire(Busy::Patrol).err().as_deref(),
            Some("Pawtrol is busy cleaning")
        );
    }

    #[test]
    fn tray_texts() {
        let utc_day = |ms: u64| (ms / DAY) as i64;
        let mut s = PatrolState::default();
        assert_eq!(tray_text(&s, true, true, T0, 0), "Pawtrol · checking…");
        assert_eq!(tray_text(&s, false, false, T0, 0), crate::tray::STATUS_OFF);
        assert_eq!(
            tray_text(&s, true, false, T0, 0),
            crate::tray::STATUS_ON_DUTY
        );

        s.last_patrol_at = Some(T0 - 3 * HOUR);
        assert_eq!(
            tray_text(&s, true, false, T0, 0),
            "Pawtrol · last run 3h ago"
        );

        let today = T0 - T0 % DAY + HOUR;
        s.history = vec![
            PatrolRun {
                finished_at: today,
                ..run_with(&[("Xcode", 2 * GB)])
            },
            PatrolRun {
                finished_at: today - DAY,
                ..run_with(&[("Node.js", GB)])
            },
        ];
        let freed = freed_today(&s, today + HOUR, utc_day);
        assert_eq!(freed, 2 * GB);
        assert_eq!(
            tray_text(&s, true, false, T0, freed),
            "Pawtrol · freed 2.0 GB today"
        );

        s.pending_review = vec![item("ai_ml", GB, false), item("trash", GB, false)];
        assert_eq!(
            tray_text(&s, true, false, T0, freed),
            "Pawtrol · 2 to review"
        );

        assert_eq!(relative(30_000), "just now");
        assert_eq!(relative(5 * MINUTE), "5m ago");
        assert_eq!(relative(2 * DAY + HOUR), "2d ago");
        assert_eq!(local_day(T0), local_day(T0 + 1));
    }
}
