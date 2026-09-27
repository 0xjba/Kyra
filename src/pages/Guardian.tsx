import { useEffect, useState } from "react";
import { Shield, Check, RotateCcw } from "lucide-react";
import { useGuardianStore } from "../stores/guardianStore";
import { useSettingsStore } from "../stores/settingsStore";
import { formatSize } from "../utils/format";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import "../styles/guardian.css";

/* ── Score badge helpers ── */
function scoreTier(score: number): "high" | "mid" | "low" {
  if (score > 70) return "high";
  if (score >= 40) return "mid";
  return "low";
}

function ScoreBadge({ score }: { score: number }) {
  const tier = scoreTier(score);
  return (
    <span className={`guardian-score-badge guardian-score-${tier}`}>
      {Math.round(score)}
    </span>
  );
}

/* ── Idle ── */
function IdleView({ onScan }: { onScan: () => void }) {
  return (
    <div className="centered">
      <div className="guardian-idle-icon">
        <Shield size={26} strokeWidth={1.5} />
      </div>

      <div className="guardian-idle-title">Guardian</div>
      <div className="guardian-idle-desc">
        Analyzes your system and scores reclaimable data by confidence
        and safety, so you can auto-clean what's truly safe to remove.
      </div>

      <button className="btn btn-primary" onClick={onScan}>
        Scan Now
      </button>
    </div>
  );
}

/* ── Scanning / Scoring ── */
function ScanningView({ label }: { label: string }) {
  return (
    <div className="centered">
      <div className="guardian-pulse">
        <Shield size={22} strokeWidth={1.5} />
      </div>
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>{label}</div>
    </div>
  );
}

/* ── Category row ── */
function CategoryRow({
  category,
  displayName,
  cleanableBytes,
  score,
  details,
  selected,
  onToggle,
}: {
  category: string;
  displayName: string;
  cleanableBytes: number;
  score: number;
  details: string;
  selected: boolean;
  onToggle: (category: string) => void;
}) {
  return (
    <div className="guardian-row" onClick={() => onToggle(category)}>
      <input
        type="checkbox"
        className="checkbox"
        checked={selected}
        onChange={() => onToggle(category)}
        onClick={(e) => e.stopPropagation()}
      />
      <div className="guardian-row-main">
        <div className="guardian-row-top">
          <span className="guardian-row-name">{displayName}</span>
          <ScoreBadge score={score} />
          <span className="guardian-row-size">{formatSize(cleanableBytes)}</span>
        </div>
        <div className="guardian-row-details">{details}</div>
      </div>
    </div>
  );
}

/* ── Results ── */
function ResultsView() {
  const scores = useGuardianStore((s) => s.scores);
  const selected = useGuardianStore((s) => s.selected);
  const totalCleanable = useGuardianStore((s) => s.totalCleanable);
  const toggleCategory = useGuardianStore((s) => s.toggleCategory);
  const selectAll = useGuardianStore((s) => s.selectAll);
  const deselectAll = useGuardianStore((s) => s.deselectAll);
  const clean = useGuardianStore((s) => s.clean);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  const [showConfirm, setShowConfirm] = useState(false);

  if (scores.length === 0) {
    return (
      <div className="centered">
        <div className="guardian-empty-icon">
          <Check size={26} strokeWidth={1.5} />
        </div>
        <div className="guardian-empty-title">All clean</div>
        <div className="guardian-empty-desc">
          No reclaimable data was found. Your system is already in great shape.
        </div>
        <button className="btn" onClick={() => useGuardianStore.getState().scan()} style={{ marginTop: 8 }}>
          Scan Again
        </button>
      </div>
    );
  }

  const sorted = [...scores].sort((a, b) => b.score - a.score);
  const allSelected = scores.length > 0 && scores.every((s) => selected.has(s.category));
  const selectedSize = sorted
    .filter((s) => selected.has(s.category))
    .reduce((sum, s) => sum + s.cleanable_bytes, 0);

  return (
    <>
      <div className="guardian-summary-bar">
        <div className="guardian-summary-left">
          <span className="guardian-summary-title">Guardian</span>
          <span className="guardian-summary-size">{formatSize(totalCleanable)}</span>
          <span className="guardian-summary-context">
            reclaimable across {sorted.length} categories
          </span>
        </div>
        <button className="btn" style={{ minWidth: 90 }} onClick={allSelected ? deselectAll : selectAll}>
          {allSelected ? "Deselect All" : "Select All"}
        </button>
      </div>

      <div className="guardian-list">
        {sorted.map((s) => (
          <CategoryRow
            key={s.category}
            category={s.category}
            displayName={s.display_name}
            cleanableBytes={s.cleanable_bytes}
            score={s.score}
            details={s.details}
            selected={selected.has(s.category)}
            onToggle={toggleCategory}
          />
        ))}
      </div>

      <div className="module-footer guardian-footer">
        <span className="module-footer-info">
          {selected.size} of {sorted.length} categories selected
        </span>
        <button
          className="btn btn-primary"
          style={{ minWidth: 140 }}
          disabled={selected.size === 0}
          onClick={() => setShowConfirm(true)}
        >
          Clean Selected {selectedSize > 0 ? formatSize(selectedSize) : ""}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Clean ${selected.size} categories (${formatSize(selectedSize)})?`}
        onConfirm={() => { setShowConfirm(false); clean(!useTrash); }}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

/* ── Cleaning ── */
function CleaningView() {
  const progress = useGuardianStore((s) => s.progress);

  const total = progress?.categories_total ?? 0;
  const done = progress?.categories_done ?? 0;
  const percent = total > 0 ? Math.round((done / total) * 100) : 0;

  const ringSize = 120;
  const strokeWidth = 6;
  const radius = (ringSize - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference - (percent / 100) * circumference;

  return (
    <div className="centered">
      <div className="guardian-ring-wrap">
        <svg
          className="guardian-ring-svg"
          width={ringSize}
          height={ringSize}
          viewBox={`0 0 ${ringSize} ${ringSize}`}
        >
          <defs>
            <linearGradient id="guardian-ring-glass" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.35)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.18)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.30)" />
            </linearGradient>
          </defs>
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none" stroke="rgba(255, 255, 255, 0.06)" strokeWidth={strokeWidth}
          />
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none" stroke="url(#guardian-ring-glass)"
            strokeWidth={strokeWidth} strokeLinecap="round"
            strokeDasharray={circumference} strokeDashoffset={dashOffset}
            className="guardian-ring-fill"
          />
        </svg>
        <span className="guardian-ring-percent">{percent}%</span>
      </div>

      <div className="guardian-ring-freed">
        {progress ? formatSize(progress.bytes_freed) : "0 B"} reclaimed
      </div>

      <div className="guardian-ring-current">
        {progress?.current_category
          ? `Cleaning ${progress.current_category}…`
          : "Starting…"}
      </div>
    </div>
  );
}

/* ── Success ── */
function SuccessView() {
  const cleanResult = useGuardianStore((s) => s.cleanResult);
  const reset = useGuardianStore((s) => s.reset);

  const ringSize = 120;
  const strokeWidth = 6;
  const radius = (ringSize - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;

  return (
    <div className="centered guardian-done">
      <div className="guardian-ring-wrap">
        <svg
          className="guardian-ring-svg"
          width={ringSize}
          height={ringSize}
          viewBox={`0 0 ${ringSize} ${ringSize}`}
        >
          <defs>
            <linearGradient id="guardian-ring-glass-done" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.5)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.28)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.45)" />
            </linearGradient>
            <filter id="guardian-ring-glow">
              <feGaussianBlur stdDeviation="3" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none" stroke="rgba(255, 255, 255, 0.06)" strokeWidth={strokeWidth}
          />
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none" stroke="url(#guardian-ring-glass-done)"
            strokeWidth={strokeWidth} strokeLinecap="round"
            strokeDasharray={circumference} strokeDashoffset={0}
            className="guardian-ring-fill"
            filter="url(#guardian-ring-glow)"
          />
        </svg>
        <Check size={32} strokeWidth={2.5} className="guardian-ring-check" />
      </div>

      <div className="guardian-ring-freed">
        {cleanResult ? formatSize(cleanResult.bytes_freed) : "0 B"} reclaimed
      </div>

      <div className="guardian-ring-current">
        {cleanResult ? cleanResult.categories_cleaned : 0} categories cleaned
      </div>

      {cleanResult && cleanResult.errors.length > 0 && (
        <div style={{
          fontSize: 11,
          color: "rgba(255,255,255,0.4)",
          marginTop: 4,
        }}>
          {cleanResult.errors.length} item{cleanResult.errors.length !== 1 ? "s" : ""} couldn't be removed
        </div>
      )}

      <button className="btn" onClick={reset} style={{ marginTop: 12 }}>
        Done
      </button>
    </div>
  );
}

/* ── Error ── */
function ErrorView() {
  const error = useGuardianStore((s) => s.error);
  const reset = useGuardianStore((s) => s.reset);

  return (
    <div className="centered">
      <div className="guardian-empty-icon">
        <RotateCcw size={24} strokeWidth={1.5} />
      </div>
      <div className="guardian-empty-title">Something went wrong</div>
      <div className="guardian-empty-desc">{error || "An unexpected error occurred."}</div>
      <button className="btn btn-primary" onClick={reset} style={{ marginTop: 8 }}>
        Try Again
      </button>
    </div>
  );
}

/* ── Main ── */
export default function Guardian() {
  const phase = useGuardianStore((s) => s.phase);
  const scan = useGuardianStore((s) => s.scan);
  const checkLicense = useGuardianStore((s) => s.checkLicense);

  useEffect(() => {
    checkLicense();
  }, [checkLicense]);

  return (
    <div className="guardian-container">
      {phase === "idle" && <IdleView onScan={scan} />}
      {phase === "scanning" && <ScanningView label="Scanning your system…" />}
      {phase === "scoring" && <ScanningView label="Analyzing…" />}
      {phase === "results" && <ResultsView />}
      {phase === "cleaning" && <CleaningView />}
      {phase === "success" && <SuccessView />}
      {phase === "error" && <ErrorView />}
    </div>
  );
}
