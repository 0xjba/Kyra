import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Check, Clock, Hand, LockKeyhole, Radar, Siren } from "lucide-react";
import { useGuardianStore } from "../stores/guardianStore";
import { useSettingsStore } from "../stores/settingsStore";
import { formatSize } from "../utils/format";
import { agoLabel, dataLossCopy, nextPatrolLabel, patrolStatusLine, runSummary } from "../utils/patrol";
import type { PatrolRun, PatrolStatus, PatrolTrigger, ReviewItem } from "../lib/tauri";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import SubscribeSheet from "../components/SubscribeSheet";
import RestoreSheet from "../components/RestoreSheet";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import walk1 from "../assets/cat-walking/cat_walking_01.png";
import walk2 from "../assets/cat-walking/cat_walking_02.png";
import walk3 from "../assets/cat-walking/cat_walking_03.png";
import walk4 from "../assets/cat-walking/cat_walking_04.png";
import walk5 from "../assets/cat-walking/cat_walking_05.png";
import "../styles/guardian.css";

const TAIL_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];
const WALK_FRAMES = [walk1, walk2, walk3, walk4, walk5, walk4, walk3, walk2];

const PERKS = [
  "Runs daily and when space runs low",
  "Auto-cleans safe caches on its own",
  "Asks before touching your data",
];

const TEASER = [
  { name: "Freed 2.3 GB", tier: "#2AC852" },
  { name: "Docker Data", tier: "#FDB022" },
  { name: "Xcode Archives", tier: "#FDB022" },
];

const TRIGGERS: Record<PatrolTrigger, { icon: typeof Clock; label: string }> = {
  schedule: { icon: Clock, label: "Daily run" },
  low_disk: { icon: AlertTriangle, label: "Low disk" },
  manual: { icon: Hand, label: "You asked" },
};

function tierColor(score: number): string {
  if (score > 70) return "#2AC852";
  if (score >= 40) return "#FDB022";
  return "#FD4841";
}

function useFrame(length: number, ms: number) {
  const [i, setI] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setI((n) => (n + 1) % length), ms);
    return () => clearInterval(id);
  }, [length, ms]);
  return i;
}

function useNow(ms: number) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
}

function TailCat({ className }: { className: string }) {
  const i = useFrame(TAIL_FRAMES.length, 130);
  return <img src={TAIL_FRAMES[i]} className={className} alt="" draggable={false} />;
}

function WalkCat({ className }: { className: string }) {
  const i = useFrame(WALK_FRAMES.length, 130);
  return <img src={WALK_FRAMES[i]} className={className} alt="" draggable={false} />;
}

function SirenBadge() {
  const blink = useFrame(2, 650) === 1;
  const a = blink ? "#1f5fff" : "#FD4841";
  const b = blink ? "#FD4841" : "#1f5fff";
  return (
    <div className="guardian-siren-badge">
      <svg width="0" height="0" className="guardian-svg-defs" aria-hidden="true">
        <defs>
          <linearGradient id="guardian-siren-grad" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="24" y2="0">
            <stop offset="0.5" stopColor={a} />
            <stop offset="0.5" stopColor={b} />
          </linearGradient>
        </defs>
      </svg>
      <Siren size={24} strokeWidth={2} stroke="url(#guardian-siren-grad)" />
    </div>
  );
}

function LockedView({ onSubscribe, onRestore }: { onSubscribe: () => void; onRestore: () => void }) {
  const deviceName = useGuardianStore((s) => s.deviceName);

  return (
    <div className="guardian-locked">
      <div className="guardian-hero">
        <div className="guardian-hero-cat">
          <TailCat className="guardian-hero-cat-img" />
          <SirenBadge />
        </div>
        <div className="guardian-hero-body">
          <div className="guardian-hero-heading">
            <span className="guardian-hero-title">Pawtrol</span>
            <span className="guardian-pro-pill">PRO</span>
          </div>
          <div className="guardian-hero-desc">
            Pawtrol looks after your {deviceName || "Mac"} every day and when space runs low, clears what's
            safe on its own, and asks you before touching anything that matters.
          </div>
          <div className="guardian-perks">
            {PERKS.map((k) => (
              <div key={k} className="guardian-perk">
                <span className="guardian-perk-check">
                  <Check size={11} strokeWidth={3} />
                </span>
                {k}
              </div>
            ))}
          </div>
          <div className="guardian-cta-row">
            <button className="guardian-btn-subscribe" onClick={onSubscribe}>
              Subscribe
            </button>
            <div className="guardian-price">
              <span className="guardian-price-amount">$0.99</span>
              <span className="guardian-price-period">/month</span>
            </div>
          </div>
          <button className="guardian-refresh-link" onClick={onRestore}>
            Already subscribed? Restore
          </button>
        </div>
      </div>

      <div className="guardian-teaser">
        <div className="guardian-teaser-blur" aria-hidden="true">
          {TEASER.map((c) => (
            <div key={c.name} className="guardian-teaser-row">
              <span className="guardian-teaser-name">{c.name}</span>
              <span className="guardian-score-pill guardian-teaser-pill" style={{ background: c.tier }} />
              <span className="guardian-teaser-size" />
            </div>
          ))}
        </div>
        <div className="guardian-teaser-lock">
          <LockKeyhole size={14} strokeWidth={2} />
          Your patrol reports show up here
        </div>
      </div>
    </div>
  );
}

function DutyHero({ status, now }: { status: PatrolStatus | null; now: number }) {
  const setPatrol = useGuardianStore((s) => s.setPatrol);
  const patrolError = useGuardianStore((s) => s.patrolError);

  const running = status?.running ?? false;
  const paused = status ? !status.enabled : false;
  const mode = running ? "running" : paused ? "paused" : "on";

  let freedSub = "since Pawtrol started";
  if (status && status.freed_last > 0) freedSub = `${formatSize(status.freed_last)} last run`;
  else if (status?.last_patrol_at) freedSub = "nothing to clear last run";

  return (
    <section className={`guardian-duty guardian-duty-${mode}`}>
      <div className="guardian-duty-cat">
        {running ? (
          <WalkCat className="guardian-duty-cat-img" />
        ) : paused ? (
          <img src={TAIL_FRAMES[0]} className="guardian-duty-cat-img guardian-duty-cat-paused" alt="" draggable={false} />
        ) : (
          <TailCat className="guardian-duty-cat-img" />
        )}
      </div>

      <div className="guardian-duty-body">
        <div className="guardian-duty-heading">
          <span className="guardian-duty-title">{paused ? "Pawtrol is paused" : "Pawtrol is on duty"}</span>
          <span className="guardian-active-pill">Pro</span>
        </div>
        <div className="guardian-duty-status">
          <span className="guardian-duty-dot" />
          <span>{status ? patrolStatusLine(status, now) : "Checking in…"}</span>
        </div>
        <div className="guardian-duty-actions">
          {paused && status && (
            <button className="guardian-pill guardian-pill-primary" onClick={() => setPatrol(true, status.auto_clean)}>
              Resume Pawtrol
            </button>
          )}
        </div>
        {patrolError && <div className="guardian-duty-error">{patrolError}</div>}
      </div>

      <div className="guardian-duty-stat">
        <div className="guardian-duty-freed">
          <span className="guardian-duty-freed-size">{formatSize(status?.freed_total ?? 0)}</span>
          <span className="guardian-duty-freed-label">freed</span>
        </div>
        <div className="guardian-duty-freed-sub">{freedSub}</div>
      </div>
    </section>
  );
}

function ReviewRow({
  item,
  checked,
  busy,
  onToggle,
  onClean,
  onDismiss,
}: {
  item: ReviewItem;
  checked: boolean;
  busy: boolean;
  onToggle: () => void;
  onClean: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className={`guardian-review-row${busy ? " guardian-review-row-busy" : ""}`} data-review-id={item.id}>
      <input
        type="checkbox"
        className="checkbox"
        checked={checked}
        disabled={busy}
        onChange={onToggle}
        aria-label={`Select ${item.name}`}
      />
      <div className="guardian-row-main">
        <div className="guardian-row-name">{item.name}</div>
        {item.details && <div className="guardian-row-details">{item.details}</div>}
        {item.user_data && item.data_loss && (
          <div className="guardian-row-warning">
            <AlertTriangle size={11} strokeWidth={2.2} />
            <span>{item.data_loss}</span>
          </div>
        )}
      </div>
      <div className="guardian-review-side">
        <div className="guardian-review-meta">
          <span className="guardian-score-pill" style={{ background: tierColor(item.score) }}>
            {Math.round(item.score)}
          </span>
          <span className="guardian-row-size">{formatSize(item.size)}</span>
        </div>
        <div className="guardian-review-actions">
          <button className="guardian-row-btn" onClick={onDismiss} disabled={busy}>
            Not now
          </button>
          <button className="guardian-row-btn guardian-row-btn-clean" onClick={onClean} disabled={busy}>
            Clean
          </button>
        </div>
      </div>
    </div>
  );
}

function ReviewPanel({ status }: { status: PatrolStatus | null }) {
  const reviewClean = useGuardianStore((s) => s.reviewClean);
  const reviewDismiss = useGuardianStore((s) => s.reviewDismiss);
  const reviewBusy = useGuardianStore((s) => s.reviewBusy);
  const reviewError = useGuardianStore((s) => s.reviewError);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<ReviewItem[] | null>(null);

  const items = status?.pending_review ?? [];
  const { safe, risky } = useMemo(() => {
    const byScore = [...items].sort((a, b) => b.score - a.score);
    return { safe: byScore.filter((i) => i.safe), risky: byScore.filter((i) => !i.safe) };
  }, [items]);

  const selected = items.filter((i) => picked.has(i.id));
  const selectedSize = selected.reduce((sum, i) => sum + i.size, 0);
  const safeSize = safe.reduce((sum, i) => sum + i.size, 0);

  const forget = (targets: ReviewItem[]) =>
    setPicked((prev) => {
      const next = new Set(prev);
      targets.forEach((t) => next.delete(t.id));
      return next;
    });

  const clean = (targets: ReviewItem[]) => {
    forget(targets);
    reviewClean(targets.map((t) => t.id));
  };

  const requestClean = (targets: ReviewItem[]) => {
    if (targets.length === 0) return;
    if (targets.some((t) => t.user_data)) setConfirm(targets);
    else clean(targets);
  };

  const dismiss = (targets: ReviewItem[]) => {
    forget(targets);
    reviewDismiss(targets.map((t) => t.id));
  };

  const toggle = (id: string) =>
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const renderRow = (item: ReviewItem) => (
    <ReviewRow
      key={item.id}
      item={item}
      checked={picked.has(item.id)}
      busy={reviewBusy.includes(item.id)}
      onToggle={() => toggle(item.id)}
      onClean={() => requestClean([item])}
      onDismiss={() => dismiss([item])}
    />
  );

  const confirmSize = confirm?.reduce((sum, i) => sum + i.size, 0) ?? 0;
  const confirmTitle = !confirm
    ? ""
    : confirm.length === 1
      ? `Clean ${confirm[0].name}?`
      : `Clean ${confirm.length} items (${formatSize(confirmSize)})?`;
  const confirmDesc = confirm
    ? dataLossCopy(
        confirm.filter((i) => i.user_data).map((i) => i.data_loss || `${i.name} holds your own data`),
        useTrash,
      )
    : undefined;

  return (
    <section className="guardian-panel guardian-review">
      <div className="guardian-panel-head">
        <span className="guardian-panel-title">Needs your review</span>
        {items.length > 0 && <span className="guardian-count-pill">{items.length}</span>}
      </div>

      {items.length === 0 ? (
        <div className="guardian-panel-empty">
          <TailCat className="guardian-empty-cat" />
          <div className="guardian-panel-empty-title">Nothing needs you. I'll keep watching.</div>
        </div>
      ) : (
        <div className="guardian-panel-body">
          {safe.length > 0 && (
            <>
              <div className="guardian-group-head">
                <span className="guardian-group-label">
                  Safe to clean{status && !status.auto_clean ? " · waiting because auto-clean is off" : ""}
                </span>
                <button
                  className="guardian-btn-clean guardian-btn-compact"
                  onClick={() => requestClean(safe)}
                  disabled={safe.every((i) => reviewBusy.includes(i.id))}
                >
                  Clean all safe ({formatSize(safeSize)})
                </button>
              </div>
              {safe.map(renderRow)}
            </>
          )}
          {risky.length > 0 && safe.length > 0 && (
            <div className="guardian-group-head">
              <span className="guardian-group-label">Your call</span>
            </div>
          )}
          {risky.map(renderRow)}
        </div>
      )}

      {reviewError && <div className="guardian-panel-error">{reviewError}</div>}

      {selected.length > 0 && (
        <div className="guardian-panel-foot">
          <span className="guardian-sel-info">
            {selected.length} selected · {formatSize(selectedSize)}
          </span>
          <button className="guardian-btn-secondary" onClick={() => dismiss(selected)}>
            Not now
          </button>
          <button className="guardian-btn-clean guardian-btn-compact" onClick={() => requestClean(selected)}>
            Clean {formatSize(selectedSize)}
          </button>
        </div>
      )}

      <DeleteConfirmDialog
        visible={!!confirm}
        title={confirmTitle}
        description={confirmDesc}
        confirmLabel={useTrash ? "Move to Trash" : "Delete"}
        destructive
        onConfirm={() => {
          if (confirm) clean(confirm);
          setConfirm(null);
        }}
        onCancel={() => setConfirm(null)}
      />
    </section>
  );
}

function ActivityRow({ run, now }: { run: PatrolRun; now: number }) {
  const trigger = TRIGGERS[run.trigger] ?? TRIGGERS.schedule;
  const Icon = trigger.icon;
  const { title, detail } = runSummary(run);
  return (
    <div className={`guardian-activity-row${run.error ? " guardian-activity-row-error" : ""}`}>
      <span className={`guardian-activity-icon guardian-activity-${run.trigger}`} title={trigger.label}>
        <Icon size={13} strokeWidth={2.2} aria-label={trigger.label} />
      </span>
      <div className="guardian-activity-main">
        <div className="guardian-activity-title">{title}</div>
        <div className="guardian-activity-detail">{detail}</div>
      </div>
      <span className="guardian-activity-time">{agoLabel(run.finished_at || run.started_at, now)}</span>
    </div>
  );
}

function ActivityPanel({ status, now }: { status: PatrolStatus | null; now: number }) {
  const history = useMemo(
    () => [...(status?.history ?? [])].sort((a, b) => b.started_at - a.started_at),
    [status?.history],
  );

  return (
    <section className="guardian-panel guardian-activity">
      <div className="guardian-panel-head">
        <span className="guardian-panel-title">Activity</span>
      </div>
      <div className="guardian-panel-body">
        {status?.running && (
          <div className="guardian-activity-row guardian-activity-live">
            <span className="guardian-activity-icon guardian-activity-manual">
              <Radar size={13} strokeWidth={2.2} />
            </span>
            <div className="guardian-activity-main">
              <div className="guardian-activity-title">Pawtrol is checking…</div>
              <div className="guardian-activity-detail">Checking caches, logs and dev tools</div>
            </div>
          </div>
        )}
        {history.map((run) => (
          <ActivityRow key={`${run.started_at}-${run.trigger}`} run={run} now={now} />
        ))}
        {history.length === 0 && !status?.running && (
          <div className="guardian-activity-empty">
            {status?.next_patrol_at
              ? `Pawtrol hasn't run yet. First run ${nextPatrolLabel(status.next_patrol_at, now).replace(/^next /, "")}.`
              : "Pawtrol hasn't run yet."}
          </div>
        )}
      </div>
    </section>
  );
}

function Dashboard() {
  const status = useGuardianStore((s) => s.patrolStatus);
  const now = useNow(60_000);
  return (
    <div className="guardian-dash">
      <DutyHero status={status} now={now} />
      <div className="guardian-panels">
        <ReviewPanel status={status} />
        <ActivityPanel status={status} now={now} />
      </div>
    </div>
  );
}

export default function Guardian() {
  const licensed = useGuardianStore((s) => s.license.active);
  const checkLicense = useGuardianStore((s) => s.checkLicense);
  const loadPatrolStatus = useGuardianStore((s) => s.loadPatrolStatus);

  useEffect(() => {
    checkLicense();
  }, [checkLicense]);

  useEffect(() => {
    if (licensed) loadPatrolStatus();
  }, [licensed, loadPatrolStatus]);

  const [sheet, setSheet] = useState<"subscribe" | "restore" | null>(null);
  const closeSheet = useCallback(() => setSheet(null), []);

  // Sheets live outside LockedView so a restore can show its success step after the license flips.
  return (
    <div className="guardian-container">
      {licensed ? (
        <Dashboard />
      ) : (
        <LockedView onSubscribe={() => setSheet("subscribe")} onRestore={() => setSheet("restore")} />
      )}
      <SubscribeSheet open={sheet === "subscribe"} onClose={closeSheet} />
      <RestoreSheet open={sheet === "restore"} onClose={closeSheet} />
    </div>
  );
}
