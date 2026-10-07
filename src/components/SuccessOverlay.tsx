import { useEffect, useId, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ChevronDown, Siren } from "lucide-react";
import { useGuardianStore } from "../stores/guardianStore";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import { formatSize } from "../utils/format";
import "../styles/success-overlay.css";

export interface SuccessChip {
  v: string;
  k: string;
}

export type SuccessIssueReason = "in_use" | "no_permission" | "already_gone" | "protected" | "other";

/** Something selected that wasn't freed. */
export interface SuccessIssue {
  label: string;
  path: string;
  /** Bytes left behind (ignored for "already_gone"). */
  size: number;
  reason: SuccessIssueReason;
}

const REASON_TEXT: Record<SuccessIssueReason, string> = {
  in_use: "In use",
  no_permission: "No permission",
  already_gone: "Already gone",
  protected: "Protected",
  other: "Couldn't remove",
};

interface IssueRow {
  label: string;
  reason: SuccessIssueReason;
  size: number;
  paths: string[];
}

/** One row per item and reason, failures first and largest first. */
function groupIssues(issues: SuccessIssue[]): IssueRow[] {
  const rows = new Map<string, IssueRow>();
  for (const i of issues) {
    const key = `${i.reason}\u0000${i.label}`;
    const row = rows.get(key) ?? { label: i.label, reason: i.reason, size: 0, paths: [] };
    row.size += i.size;
    row.paths.push(i.path);
    rows.set(key, row);
  }
  const gone = (r: IssueRow) => (r.reason === "already_gone" ? 1 : 0);
  return [...rows.values()].sort((a, b) => gone(a) - gone(b) || b.size - a.size);
}

function IssuesSummary({ failedBytes, issues }: { failedBytes: number; issues: SuccessIssue[] }) {
  const [open, setOpen] = useState(false);
  const listId = useId();
  if (issues.length === 0) return null;
  const failedCount = issues.filter((i) => i.reason !== "already_gone").length;
  const goneCount = issues.length - failedCount;
  const summary =
    failedCount > 0
      ? `${formatSize(failedBytes)} couldn't be removed`
      : `${goneCount} ${goneCount === 1 ? "item was" : "items were"} already gone`;
  const rows = groupIssues(issues);

  return (
    <div className="ks-issues ks-fade">
      <button
        type="button"
        className="ks-issues-toggle"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((v) => !v)}
      >
        <span className={failedCount > 0 ? "ks-issues-failed" : undefined}>{summary}</span>
        <ChevronDown size={13} strokeWidth={2.2} className={`ks-issues-chevron${open ? " ks-open" : ""}`} />
      </button>
      {open && (
        <ul id={listId} className="ks-issues-list">
          {rows.map((r) => (
            <li key={`${r.reason}-${r.label}`} className="ks-issue" title={r.paths.join("\n")}>
              <span className="ks-issue-name">
                {r.label}
                {r.paths.length > 1 && <span className="ks-issue-count"> · {r.paths.length}</span>}
              </span>
              <span className="ks-issue-size">{r.reason === "already_gone" ? "" : formatSize(r.size)}</span>
              <span className="ks-issue-reason">{REASON_TEXT[r.reason]}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

interface SuccessOverlayProps {
  headline: string;
  freedGB: number;
  detail: string;
  itemCount?: number;
  categoryCount?: number;
  lifetimeGB?: number;
  storageUsedGB?: number;
  storageTotalGB?: number;
  showPawtrolUpsell?: boolean;
  /** Bytes that were selected but couldn't be removed. */
  failedBytes?: number;
  /** What wasn't freed and why, shown behind an expandable line. */
  issues?: SuccessIssue[];
  chips?: SuccessChip[];
  upHead?: string;
  isPro?: boolean;
  onUpgrade?: () => void;
  onDone: () => void;
}

const CAT_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];
const CONFETTI_COLORS = ["#FD4841", "#FDD225", "#2AC852", "#22B8F0", "#1f5fff"];
const COUNT_DELAY = 250;
const COUNT_DURATION = 1200;

function fmtGB(gb: number): string {
  if (gb >= 0.995) return `${gb.toFixed(1)} GB`;
  return `${Math.round(gb * 1024)} MB`;
}

function SirenMark({ size, blink }: { size: number; blink: boolean }) {
  const gid = `ks-siren-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const left = blink ? "#1f5fff" : "#FD4841";
  const right = blink ? "#FD4841" : "#1f5fff";
  return (
    <Siren size={size} color={`url(#${gid})`} aria-hidden="true" className="ks-siren">
      <defs>
        <linearGradient id={gid} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="24" y2="0">
          <stop offset="0.5" stopColor={left} />
          <stop offset="0.5" stopColor={right} />
        </linearGradient>
      </defs>
    </Siren>
  );
}

export default function SuccessOverlay({
  headline,
  freedGB,
  detail,
  itemCount,
  categoryCount,
  lifetimeGB,
  storageUsedGB,
  storageTotalGB,
  showPawtrolUpsell = false,
  failedBytes = 0,
  issues,
  chips,
  upHead = "Next time, you could skip this.",
  isPro,
  onUpgrade,
  onDone,
}: SuccessOverlayProps) {
  const navigate = useNavigate();
  const licenseActive = useGuardianStore((s) => s.license.active);
  const pro = isPro ?? licenseActive;

  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const freedRef = useRef<HTMLSpanElement>(null);
  const catRef = useRef<HTMLImageElement>(null);

  const [n, setN] = useState(0);
  const [shown, setShown] = useState(false);
  const [phase2, setPhase2] = useState(false);
  const [hop, setHop] = useState(false);
  const [frame, setFrame] = useState(0);
  const [blink, setBlink] = useState(false);

  const G = Math.max(0, Number.isFinite(freedGB) ? freedGB : 0);
  const targetRef = useRef(G);
  targetRef.current = G;

  useEffect(() => {
    let raf = 0;
    const start = performance.now() + COUNT_DELAY;
    const tick = (now: number) => {
      const t = Math.max(0, Math.min((now - start) / COUNT_DURATION, 1));
      setN(targetRef.current * (1 - Math.pow(1 - t, 3)));
      if (t < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    const snap = setTimeout(() => setN(targetRef.current), COUNT_DURATION + 600);
    return () => {
      cancelAnimationFrame(raf);
      clearTimeout(snap);
    };
  }, []);

  useEffect(() => {
    let confettiRaf = 0;

    const confetti = () => {
      const root = rootRef.current;
      const cv = canvasRef.current;
      if (!root || !cv) return;
      const w = cv.offsetWidth;
      const h = cv.offsetHeight;
      if (!w || !h) return;
      const dpr = window.devicePixelRatio || 2;
      cv.width = w * dpr;
      cv.height = h * dpr;
      const ctx = cv.getContext("2d");
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      const rb = root.getBoundingClientRect();
      const anchor = freedRef.current ?? catRef.current;
      let ox = w / 2;
      let oy = h * 0.62;
      if (anchor) {
        const fb = anchor.getBoundingClientRect();
        ox = fb.left - rb.left + fb.width / 2;
        oy = fb.top - rb.top + fb.height / 2;
      }

      const ps = Array.from({ length: 60 }, () => ({
        x: ox + (Math.random() - 0.5) * 40,
        y: oy,
        vx: (Math.random() - 0.5) * 6,
        vy: -(Math.random() * 5 + 3),
        r: Math.random() * 360,
        vr: (Math.random() - 0.5) * 14,
        s: Math.random() * 5 + 3,
        c: CONFETTI_COLORS[Math.floor(Math.random() * CONFETTI_COLORS.length)],
        life: 0,
        max: 1400 + Math.random() * 1200,
      }));

      let last = performance.now();
      const tick = (now: number) => {
        const dt = Math.min(now - last, 32);
        last = now;
        ctx.clearRect(0, 0, w, h);
        let alive = 0;
        for (const p of ps) {
          p.life += dt;
          if (p.life > p.max) continue;
          alive++;
          p.vy += 0.12;
          p.vx *= 0.99;
          p.x += p.vx;
          p.y += p.vy;
          p.r += p.vr;
          ctx.save();
          ctx.translate(p.x, p.y);
          ctx.rotate((p.r * Math.PI) / 180);
          ctx.globalAlpha = 1 - Math.pow(p.life / p.max, 2);
          ctx.fillStyle = p.c;
          ctx.fillRect(-p.s / 2, -p.s / 2, p.s, p.s * 0.6);
          ctx.restore();
        }
        if (alive) confettiRaf = requestAnimationFrame(tick);
      };
      confettiRaf = requestAnimationFrame(tick);
    };

    const t0 = setTimeout(() => setShown(true), 40);
    const t1 = setTimeout(() => {
      setPhase2(true);
      setHop(true);
      confetti();
    }, 900);
    const t2 = setTimeout(() => setHop(false), 1350);
    const a = setInterval(() => setFrame((f) => (f + 1) % CAT_FRAMES.length), 220);
    const b = setInterval(() => setBlink((v) => !v), 650);

    window.dispatchEvent(new CustomEvent("kyra-success", { detail: true }));

    return () => {
      cancelAnimationFrame(confettiRaf);
      [t0, t1, t2].forEach(clearTimeout);
      clearInterval(a);
      clearInterval(b);
      window.dispatchEvent(new CustomEvent("kyra-success", { detail: false }));
    };
  }, []);

  const total = storageTotalGB && storageTotalGB > 0 ? storageTotalGB : 0;
  const hasBar = total > 0;
  const after = Math.max(0, Math.min(storageUsedGB ?? 0, total));
  const before = Math.min(total, after + G);
  const usedPct = hasBar ? (after / total) * 100 : 0;
  const freedPct = hasBar ? ((before - after) / total) * 100 : 0;
  const catLeftPct = hasBar
    ? Math.max(0, Math.min(100, phase2 ? usedPct + freedPct / 2 : usedPct - 4))
    : 50;

  const big = G >= 0.995;
  const amount = big ? n.toFixed(1) : String(Math.round(n * 1024));
  const unit = big ? "GB" : "MB";

  const chipList: SuccessChip[] =
    chips ??
    [
      itemCount !== undefined ? { v: itemCount.toLocaleString(), k: itemCount === 1 ? "item" : "items" } : null,
      categoryCount !== undefined
        ? { v: categoryCount.toLocaleString(), k: categoryCount === 1 ? "category" : "categories" }
        : null,
      lifetimeGB !== undefined ? { v: fmtGB(lifetimeGB), k: "lifetime" } : null,
    ].filter((c): c is SuccessChip => c !== null);

  const showUpsell = !pro && showPawtrolUpsell;

  const handleUpgrade = () => {
    if (onUpgrade) {
      onUpgrade();
      return;
    }
    onDone();
    navigate("/guardian");
  };

  return (
    <div
      ref={rootRef}
      className={`ks-root${shown ? " ks-shown" : ""}${phase2 ? " ks-phase2" : ""}`}
    >
      <canvas ref={canvasRef} className="ks-confetti" />

      <div className="ks-main">
        <div className="ks-headline ks-rise">{headline}</div>
        <div className="ks-hero ks-rise">
          <span className="ks-amount">{amount}</span>
          <span className="ks-unit">{unit}</span>
        </div>
        <div className="ks-detail ks-fade">{detail}</div>

        <div className={`ks-bar ks-fade${hasBar ? "" : " ks-bar-empty"}`}>
          <img
            ref={catRef}
            src={CAT_FRAMES[frame]}
            alt=""
            draggable={false}
            className="ks-cat"
            style={{ left: `${catLeftPct}%`, transform: `translateY(${hop ? "-22px" : "0px"})` }}
          />
          {hasBar && (
            <>
              <div className="ks-track">
                <span className="ks-used" style={{ width: `${usedPct}%` }} />
                <span ref={freedRef} className="ks-freed" style={{ width: `${Math.max(freedPct, 1.5)}%` }} />
              </div>
              <div className="ks-bar-labels">
                <span>{`${Math.round(phase2 ? after : before)} of ${Math.round(total)} GB used`}</span>
                <span className="ks-free-label">{`+${fmtGB(G)} free`}</span>
              </div>
            </>
          )}
        </div>

        {chipList.length > 0 && (
          <div className="ks-chips ks-fade">
            {chipList.map((c, i) => (
              <span key={`${c.k}-${i}`} className="ks-chip">
                <span className="ks-chip-v">{c.v}</span>
                {c.k}
              </span>
            ))}
          </div>
        )}

        {issues && issues.length > 0 && <IssuesSummary failedBytes={failedBytes} issues={issues} />}

        {!showUpsell && (
          <button type="button" className="ks-done-primary" onClick={onDone}>
            Done
          </button>
        )}
      </div>

      {pro && (
        <div className="ks-onduty">
          <SirenMark size={14} blink={blink} />
          Pawtrol is on duty.
        </div>
      )}

      {showUpsell && (
        <>
          <div className="ks-upsell">
            <SirenMark size={26} blink={blink} />
            <div className="ks-upsell-body">
              <div className="ks-upsell-title">{upHead}</div>
              <div className="ks-upsell-desc">
                Put Kyra on Pawtrol and she'll keep your storage in check herself, for less than a cookie a month.
              </div>
            </div>
            <button type="button" className="ks-upsell-btn" onClick={handleUpgrade}>
              Try Pawtrol
              <span className="ks-upsell-price">from $0.99/mo</span>
            </button>
          </div>
          <button type="button" className="ks-done-link" onClick={onDone}>
            Done
          </button>
        </>
      )}
    </div>
  );
}
