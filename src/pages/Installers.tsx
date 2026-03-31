import { useState, useMemo, useEffect, useRef } from "react";
import { Download, Search, Check } from "lucide-react";
import { useInstallersStore } from "../stores/installersStore";
import { formatSize } from "../utils/format";
import { pickEquivalenceCard, type EquivalenceCard } from "../utils/equivalenceCards";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import "../styles/installers.css";

/* ── Date formatter ── */
function formatDate(secs: number): string {
  if (secs === 0) return "\u2014";
  return new Date(secs * 1000).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

/* ── What gets detected ── */
const DETECTED_TYPES = [
  { label: ".dmg", color: "#3A7BFF" },
  { label: ".pkg", color: "#FD8C34" },
  { label: ".iso", color: "#8E5CF6" },
  { label: ".xip", color: "#13D1BB" },
  { label: ".app", color: "#FD4841" },
];

/* ── Sort modes ── */
type SortMode = "size" | "name" | "date";

/* ── Idle View ── */
function IdleView() {
  const scan = useInstallersStore((s) => s.scan);
  const error = useInstallersStore((s) => s.error);

  return (
    <div className="inst-centered">
      <div className="inst-idle-icon">
        <Download size={26} strokeWidth={1.5} />
      </div>

      <div className="inst-idle-title">Find installer files</div>
      <div className="inst-idle-desc">
        Scans Downloads, Desktop, Documents, and other locations for DMG, PKG,
        and installer files. Typically recovers 0.5–5 GB.
      </div>

      <button className="btn btn-primary" onClick={scan}>
        Start Scan
      </button>

      {error && <div className="inst-error">{error}</div>}

      <div className="inst-detected-section">
        <span className="inst-detected-label">WHAT GETS DETECTED</span>
        <div className="inst-detected-types">
          {DETECTED_TYPES.map((t) => (
            <span key={t.label} className="inst-detected-chip">
              <span className="inst-detected-dot" style={{ backgroundColor: t.color }} />
              {t.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

/* ── Scanning View ── */
function ScanningView() {
  return (
    <div className="inst-centered">
      <div className="inst-spinner" />
      <div className="inst-scanning-text">Scanning for installers...</div>
    </div>
  );
}

/* ── Empty State ── */
function EmptyView() {
  const reset = useInstallersStore((s) => s.reset);

  return (
    <div className="inst-centered">
      <div className="inst-empty-icon">
        <Check size={26} strokeWidth={1.5} />
      </div>
      <div className="inst-idle-title">All clear</div>
      <div className="inst-idle-desc">
        No installer files were found. Your system is already clean.
      </div>
      <button className="btn" onClick={reset} style={{ marginTop: 8 }}>
        Scan Again
      </button>
    </div>
  );
}

/* ── List View ── */
function ListView() {
  const files = useInstallersStore((s) => s.files);
  const selected = useInstallersStore((s) => s.selected);
  const toggleSelect = useInstallersStore((s) => s.toggleSelect);
  const selectAll = useInstallersStore((s) => s.selectAll);
  const deselectAll = useInstallersStore((s) => s.deselectAll);
  const deleteSelected = useInstallersStore((s) => s.deleteSelected);
  const [searchQuery, setSearchQuery] = useState("");
  const [sortMode, setSortMode] = useState<SortMode>("size");
  const [showConfirm, setShowConfirm] = useState(false);

  const totalSize = files.reduce((sum, f) => sum + f.size, 0);
  const allSelected = files.length > 0 && selected.size === files.length;

  const selectedSize = files
    .filter((f) => selected.has(f.path))
    .reduce((sum, f) => sum + f.size, 0);

  // Filter and sort
  const filtered = useMemo(() => {
    let list = [...files];

    // Search
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      list = list.filter((f) => f.name.toLowerCase().includes(q));
    }

    // Sort
    list.sort((a, b) => {
      if (sortMode === "size") return b.size - a.size;
      if (sortMode === "name") return a.name.localeCompare(b.name);
      // date — newest first
      return b.modified_secs - a.modified_secs;
    });

    return list;
  }, [files, searchQuery, sortMode]);

  if (files.length === 0) {
    return <EmptyView />;
  }

  return (
    <>
      {/* Header */}
      <div className="inst-list-header">
        <div className="inst-list-summary">
          <span className="inst-list-title">Installers</span>
          <span className="inst-list-size">{formatSize(totalSize)}</span>
          <span className="inst-list-context">
            items found across {files.length} file{files.length === 1 ? "" : "s"}
          </span>
        </div>
        <div className="inst-list-actions">
          <button
            className="btn"
            style={{ minWidth: 90 }}
            onClick={allSelected ? deselectAll : selectAll}
          >
            {allSelected ? "Deselect All" : "Select All"}
          </button>
        </div>
      </div>

      {/* Search + Sort */}
      <div className="inst-search-row">
        <div className="inst-search-box">
          <Search size={13} className="inst-search-icon" />
          <input
            type="text"
            className="inst-search-input"
            placeholder="Search files..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>
        <div className="inst-sort-controls">
          <span className="inst-sort-label">Sort</span>
          <button
            className={`inst-sort-btn ${sortMode === "size" ? "active" : ""}`}
            onClick={() => setSortMode("size")}
          >
            Size
          </button>
          <button
            className={`inst-sort-btn ${sortMode === "name" ? "active" : ""}`}
            onClick={() => setSortMode("name")}
          >
            Name
          </button>
          <button
            className={`inst-sort-btn ${sortMode === "date" ? "active" : ""}`}
            onClick={() => setSortMode("date")}
          >
            Date
          </button>
        </div>
      </div>

      {/* File List */}
      <div className="inst-file-list">
        {filtered.map((file) => (
          <label key={file.path} className="inst-row">
            <input
              type="checkbox"
              className="checkbox"
              checked={selected.has(file.path)}
              onChange={() => toggleSelect(file.path)}
            />
            <div className="inst-row-info">
              <div className="inst-row-name">{file.name}</div>
              <div className="inst-row-meta">{formatDate(file.modified_secs)}</div>
            </div>
            <div className="inst-row-size">{formatSize(file.size)}</div>
          </label>
        ))}
      </div>

      {/* Footer */}
      <div className="inst-footer">
        <span className="inst-footer-info">
          {selected.size} of {files.length} selected
        </span>
        <button
          className="btn btn-primary"
          style={{ minWidth: 120 }}
          disabled={selected.size === 0}
          onClick={() => setShowConfirm(true)}
        >
          Delete {selected.size > 0 ? formatSize(selectedSize) : ""}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Delete ${selected.size} file${selected.size === 1 ? "" : "s"} (${formatSize(selectedSize)})?`}
        onConfirm={() => { setShowConfirm(false); deleteSelected(); }}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

/* ── Confetti ── */
const CONFETTI_COLORS = [
  "rgba(255, 255, 255, 0.6)",
  "rgba(255, 255, 255, 0.4)",
  "rgba(255, 255, 255, 0.3)",
  "rgba(253, 72, 65, 0.35)",
  "rgba(42, 200, 82, 0.35)",
  "rgba(58, 123, 255, 0.3)",
  "rgba(253, 210, 37, 0.3)",
  "rgba(142, 92, 246, 0.3)",
];

interface Particle {
  x: number; y: number; vx: number; vy: number;
  rotation: number; rotationSpeed: number;
  size: number; color: string; opacity: number;
  life: number; maxLife: number;
}

function InstConfetti({ active }: { active: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!active) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    canvas.width = canvas.offsetWidth * 2;
    canvas.height = canvas.offsetHeight * 2;
    ctx.scale(2, 2);

    const w = canvas.offsetWidth;
    const h = canvas.offsetHeight;

    const particles: Particle[] = [];
    for (let i = 0; i < 20; i++) {
      particles.push({
        x: w / 2 + (Math.random() - 0.5) * 60,
        y: h * 0.3,
        vx: (Math.random() - 0.5) * 3,
        vy: -(Math.random() * 2 + 1),
        rotation: Math.random() * 360,
        rotationSpeed: (Math.random() - 0.5) * 8,
        size: Math.random() * 4 + 2,
        color: CONFETTI_COLORS[Math.floor(Math.random() * CONFETTI_COLORS.length)],
        opacity: 1, life: 0,
        maxLife: 1600 + Math.random() * 1800,
      });
    }

    let animId: number;
    let lastTime = performance.now();

    function animate(now: number) {
      const dt = Math.min(now - lastTime, 32);
      lastTime = now;
      ctx!.clearRect(0, 0, w, h);
      let alive = 0;
      for (const p of particles) {
        p.life += dt;
        if (p.life > p.maxLife) continue;
        alive++;
        p.vy += 0.03;
        p.x += p.vx;
        p.y += p.vy;
        p.rotation += p.rotationSpeed;
        p.opacity = 1 - Math.pow(p.life / p.maxLife, 2);
        ctx!.save();
        ctx!.translate(p.x, p.y);
        ctx!.rotate((p.rotation * Math.PI) / 180);
        ctx!.globalAlpha = p.opacity;
        ctx!.fillStyle = p.color;
        ctx!.fillRect(-p.size / 2, -p.size / 2, p.size, p.size * 0.6);
        ctx!.restore();
      }
      if (alive > 0) animId = requestAnimationFrame(animate);
    }

    animId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animId);
  }, [active]);

  if (!active) return null;
  return <canvas ref={canvasRef} className="inst-confetti" />;
}

/* ── SSD Icon for milestone cards ── */
function SsdIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="6" width="18" height="12" rx="2" />
      <line x1="7" y1="10" x2="7" y2="14" />
      <line x1="11" y1="10" x2="11" y2="14" />
      <line x1="15" y1="10" x2="15" y2="14" />
    </svg>
  );
}

/* ── Deleting / Done View ── */
function DeletingView() {
  const phase = useInstallersStore((s) => s.phase);
  const progress = useInstallersStore((s) => s.progress);
  const result = useInstallersStore((s) => s.result);
  const dismissDone = useInstallersStore((s) => s.dismissDone);
  const isDone = phase === "done";

  // Staggered animation state
  const [showCard, setShowCard] = useState(false);
  const [showDoneBtn, setShowDoneBtn] = useState(false);
  const [showConfetti, setShowConfetti] = useState(false);

  // Pick equivalence card once when done
  const cardRef = useRef<EquivalenceCard | null>(null);
  const bytesFreed = isDone && result ? result.bytes_freed : (progress?.bytes_freed || 0);
  if (isDone && !cardRef.current && bytesFreed > 0) {
    cardRef.current = pickEquivalenceCard(bytesFreed);
  }

  // Reset animation state when leaving done
  useEffect(() => {
    if (!isDone) {
      setShowCard(false);
      setShowDoneBtn(false);
      setShowConfetti(false);
      cardRef.current = null;
      return;
    }

    // Staggered reveal
    const t1 = setTimeout(() => { setShowConfetti(true); setShowCard(true); }, 500);
    const t2 = setTimeout(() => setShowDoneBtn(true), 1050);

    return () => { clearTimeout(t1); clearTimeout(t2); };
  }, [isDone]);

  const percent = isDone
    ? 100
    : progress && progress.items_total > 0
      ? Math.round((progress.items_done / progress.items_total) * 100)
      : 0;

  // SVG ring dimensions
  const ringSize = 120;
  const strokeWidth = 6;
  const radius = (ringSize - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference - (percent / 100) * circumference;

  // Extract file name from current_item for display
  const currentLabel = progress?.current_item
    ? `Removing ${progress.current_item}...`
    : "Starting...";

  const card = cardRef.current;

  return (
    <div className={`inst-centered${isDone ? " inst-done" : ""}`}>
      <InstConfetti active={showConfetti} />

      {/* Circular progress ring */}
      <div className="inst-ring-wrap">
        <svg
          className="inst-ring-svg"
          width={ringSize}
          height={ringSize}
          viewBox={`0 0 ${ringSize} ${ringSize}`}
        >
          <defs>
            <linearGradient id="inst-ring-glass" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.35)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.18)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.30)" />
            </linearGradient>
            <linearGradient id="inst-ring-glass-done" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.5)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.28)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.45)" />
            </linearGradient>
            <filter id="inst-ring-glow">
              <feGaussianBlur stdDeviation="3" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>
          {/* Background track */}
          <circle
            cx={ringSize / 2}
            cy={ringSize / 2}
            r={radius}
            fill="none"
            stroke="rgba(255, 255, 255, 0.06)"
            strokeWidth={strokeWidth}
          />
          {/* Filled arc — glass gradient */}
          <circle
            cx={ringSize / 2}
            cy={ringSize / 2}
            r={radius}
            fill="none"
            stroke={isDone ? "url(#inst-ring-glass-done)" : "url(#inst-ring-glass)"}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={dashOffset}
            className="inst-ring-fill"
            filter={isDone ? "url(#inst-ring-glow)" : undefined}
          />
        </svg>
        {isDone ? (
          <Check size={32} strokeWidth={2.5} className="inst-ring-check" />
        ) : (
          <span className="inst-ring-percent">{percent}%</span>
        )}
      </div>

      {/* Status text */}
      <div className="inst-ring-freed">
        {isDone && result ? formatSize(result.bytes_freed) : (progress ? formatSize(progress.bytes_freed) : "0 B")} reclaimed
      </div>

      <div className="inst-ring-current">
        {isDone
          ? `${result ? result.items_removed : 0} items removed`
          : currentLabel}
      </div>

      {/* Equivalence card */}
      {isDone && card && (
        <div className={`inst-equiv-card${showCard ? " visible" : ""}`}>
          <div className="inst-equiv-icon">
            {card.isMilestone ? <SsdIcon /> : <span className="inst-equiv-emoji">{card.emoji}</span>}
          </div>
          <div className="inst-equiv-text">
            <div className="inst-equiv-title">{card.title}</div>
            <div className="inst-equiv-desc">{card.description}</div>
          </div>
        </div>
      )}

      {/* Done button */}
      {isDone && (
        <button
          className={`btn inst-done-btn${showDoneBtn ? " visible" : ""}`}
          onClick={dismissDone}
        >
          Done
        </button>
      )}
    </div>
  );
}

/* ── Main ── */
export default function Installers() {
  const phase = useInstallersStore((s) => s.phase);

  return (
    <div className="inst-container">
      {phase === "idle" && <IdleView />}
      {phase === "scanning" && <ScanningView />}
      {phase === "list" && <ListView />}
      {(phase === "deleting" || phase === "done") && <DeletingView />}
    </div>
  );
}

