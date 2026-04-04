import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { ChevronRight, Search, Folder, FolderOpen, Package, Check } from "lucide-react";
import { usePruneStore } from "../stores/pruneStore";
import { formatSize } from "../utils/format";
import { revealInFinder, pickFolder, getSystemStats, type ArtifactEntry } from "../lib/tauri";
import { pickEquivalenceCard, type EquivalenceCard } from "../utils/equivalenceCards";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import "../styles/prune.css";

/* ── Category colors for storage bar (mapped to backend artifact_type strings) ── */
const TYPE_COLORS: Record<string, string> = {
  "Node.js": "#2AC852",          // --green
  "Build Output": "#FD8C34",     // --orange
  "Rust": "#FD4841",             // --red
  "Python": "#FDD225",           // --yellow
  "Next.js": "#3A7BFF",          // --blue
  "Nuxt.js": "#5A67F2",          // --indigo
  "Xcode Build": "#FF5DA2",      // --pink
  "CocoaPods": "#A2845E",        // --brown
  "Swift": "#13D1BB",            // --teal
  "Gradle": "#22B8F0",           // --cyan
  "Python Virtual Env": "#8E5CF6", // --purple
  "Test Coverage": "#8E8E93",    // --neutral-500
  "Vendor Deps": "#747474",      // --neutral-600
  "Turbo Cache": "#13D1BB",      // --teal
  "Parcel Cache": "#FD8C34",     // --orange
  "Angular Cache": "#FD4841",    // --red
  "SvelteKit": "#FD8C34",        // --orange
  "Astro Cache": "#8E5CF6",      // --purple
  "Pytest Cache": "#FDD225",     // --yellow
  "Mypy Cache": "#FDD225",       // --yellow
  "Ruff Cache": "#FDD225",       // --yellow
  "C#/.NET Build": "#5A67F2",    // --indigo
  "C++ Build": "#22B8F0",        // --cyan
  "Expo Cache": "#3A7BFF",       // --blue
  "Dart Tool": "#22B8F0",        // --cyan
  "Nitro/Nuxt Output": "#5A67F2", // --indigo
  "Tox Env": "#FDD225",          // --yellow
  "Nox Env": "#FDD225",          // --yellow
  "Maven": "#FD8C34",             // --orange
  "Elixir": "#8E5CF6",            // --purple
  "Elixir Deps": "#8E5CF6",       // --purple
  "Haskell": "#FF5DA2",           // --pink
  "OCaml": "#FDD225",             // --yellow
  "Ruby Bundler": "#FD4841",      // --red
  "CMake Build": "#22B8F0",       // --cyan
  "Bun Cache": "#2AC852",         // --green
};

function getTypeColor(type: string): string {
  return TYPE_COLORS[type] || "#8E8E93";
}


/* ── Path shortener ── */
function shortenPath(fullPath: string, projectName: string): string {
  const home = fullPath.indexOf("/Users/");
  if (home >= 0) {
    const afterHome = fullPath.substring(home);
    const parts = afterHome.split("/");
    // e.g. /Users/name/Projects/foo/node_modules → Projects/foo
    if (parts.length > 3) {
      return parts.slice(3, -1).join("/");
    }
  }
  // Fallback: show parent dir
  const idx = fullPath.lastIndexOf("/" + projectName);
  if (idx >= 0) {
    const parent = fullPath.substring(0, idx).split("/").pop();
    return parent ? `${parent}/${projectName}` : projectName;
  }
  return projectName;
}

/* ── Quick-pick path chips ── */
const DEFAULT_PATHS = ["~/", "~/Projects", "~/Developer", "~/Code", "~/dev", "~/src"];
const RECENT_PATHS_KEY = "kyra_prune_recent_paths";

function loadRecentPaths(): string[] {
  try {
    const stored = localStorage.getItem(RECENT_PATHS_KEY);
    return stored ? JSON.parse(stored) : [];
  } catch { return []; }
}

function saveRecentPath(path: string) {
  const recent = loadRecentPaths().filter((p) => p !== path);
  recent.unshift(path);
  // Keep max 6
  localStorage.setItem(RECENT_PATHS_KEY, JSON.stringify(recent.slice(0, 6)));
}

/** Shorten a path for chip display — show last folder with .../ prefix if long */
function chipLabel(path: string): string {
  if (DEFAULT_PATHS.includes(path)) return path;
  // Remove trailing slash
  const clean = path.replace(/\/+$/, "");
  const parts = clean.split("/").filter(Boolean);
  if (parts.length <= 2) return path.startsWith("/") ? `/${parts.join("/")}` : parts.join("/");
  const last = parts[parts.length - 1];
  return `.../${last}`;
}

/* ── What gets detected ── */
const DETECTED_TYPES = [
  { label: "node_modules", color: "#2AC852" },
  { label: "target", color: "#FD4841" },
  { label: "build / dist", color: "#FD8C34" },
  { label: "DerivedData", color: "#FF5DA2" },
  { label: "venv", color: "#8E5CF6" },
  { label: ".gradle", color: "#22B8F0" },
];

/* ── Idle View ── */
function IdleView() {
  const rootPath = usePruneStore((s) => s.rootPath);
  const setRootPath = usePruneStore((s) => s.setRootPath);
  const scan = usePruneStore((s) => s.scan);
  const error = usePruneStore((s) => s.error);

  // Merge recent paths with defaults (deduplicated, max 6)
  const recentPaths = useMemo(() => loadRecentPaths(), []);
  const quickPaths = useMemo(() => {
    const merged = [...recentPaths];
    for (const p of DEFAULT_PATHS) {
      if (!merged.includes(p)) merged.push(p);
    }
    return merged.slice(0, 6);
  }, [recentPaths]);

  const handleScan = useCallback(() => {
    saveRecentPath(rootPath);
    scan();
  }, [rootPath, scan]);

  const handlePickFolder = useCallback(async () => {
    const selected = await pickFolder();
    if (selected) {
      setRootPath(selected);
    }
  }, [setRootPath]);

  return (
    <div className="centered">
      {/* Feature icon — matches dashboard Package icon */}
      <div className="prune-idle-icon">
        <Package size={26} strokeWidth={1.5} />
      </div>

      <div className="prune-idle-title">Scan for Developer Artifacts</div>
      <div className="prune-idle-desc">
        Find node_modules, target, dist, and other build outputs
        inside your project folders. Typically recovers 2–10 GB.
      </div>

      {/* Path input with clickable folder icon */}
      <div className="prune-path-row">
        <div className="prune-path-input-wrap">
          <FolderOpen
            size={14}
            className="prune-path-icon clickable"
            onClick={handlePickFolder}
          />
          <input
            type="text"
            className="prune-path-input"
            value={rootPath}
            onChange={(e) => setRootPath(e.target.value)}
            placeholder="Browse or type a path to scan..."
          />
        </div>
        <button className="btn btn-primary" onClick={handleScan} disabled={!rootPath.trim()}>
          Scan
        </button>
      </div>

      {/* Quick-pick chips */}
      <div className="prune-quick-picks">
        {quickPaths.map((p) => (
          <button
            key={p}
            className={`prune-pick-chip${rootPath === p ? " active" : ""}`}
            onClick={() => setRootPath(p)}
            title={p}
          >
            {chipLabel(p)}
          </button>
        ))}
      </div>

      {error && <div className="prune-error">{error}</div>}

      {/* What gets detected */}
      <div className="prune-detected-section">
        <span className="prune-detected-label">WHAT GETS DETECTED</span>
        <div className="prune-detected-types">
          {DETECTED_TYPES.map((t) => (
            <span key={t.label} className="prune-detected-chip">
              <span className="prune-detected-dot" style={{ backgroundColor: t.color }} />
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
    <div className="centered">
      <div className="spinner" />
      <div className="prune-scanning-text">Scanning for artifacts...</div>
    </div>
  );
}

/* ── Storage Bar ── */
function StorageBar({
  categories,
  diskTotal,
  diskFree,
}: {
  categories: [string, ArtifactEntry[]][];
  diskTotal: number;
  diskFree: number;
}) {
  // Build segments from grouped categories
  type Seg = { name: string; size: number; color: string };
  const scannedSegments: Seg[] = [];

  for (const [type, items] of categories) {
    const size = (items as any[]).reduce((s: number, a: any) => s + a.size, 0);
    if (size > 0) {
      scannedSegments.push({ name: type, size, color: getTypeColor(type) });
    }
  }

  const scannedSize = scannedSegments.reduce((s, seg) => s + seg.size, 0);
  if (diskTotal === 0) return null;

  // "Other" = used space that wasn't scanned
  const diskUsed = diskTotal - diskFree;
  const otherSize = Math.max(0, diskUsed - scannedSize);

  const allSegments: Seg[] = [...scannedSegments];
  if (otherSize > 0) allSegments.push({ name: "Other", size: otherSize, color: "var(--text-quaternary)" });
  if (diskFree > 0) allSegments.push({ name: "Free", size: diskFree, color: "rgba(255,255,255,0.06)" });

  return (
    <div className="prune-storage-bar">
      <div className="prune-storage-track">
        {allSegments.map((seg, i) => {
          const pct = (seg.size / diskTotal) * 100;
          const isContext = seg.name === "Free" || seg.name === "Other";
          const minPct = isContext ? 0.5 : 1;
          return (
            <div
              key={seg.name}
              className="prune-storage-segment"
              style={{
                width: `${Math.max(pct, minPct)}%`,
                background: seg.name === "Free"
                  ? "rgba(255,255,255,0.06)"
                  : seg.name === "Other"
                    ? "rgba(255,255,255,0.12)"
                    : `linear-gradient(90deg, color-mix(in srgb, ${seg.color}, white 25%) 0%, ${seg.color} 100%)`,
                borderRadius:
                  i === 0 && i === allSegments.length - 1
                    ? "4px"
                    : i === 0
                      ? "4px 0 0 4px"
                      : i === allSegments.length - 1
                        ? "0 4px 4px 0"
                        : "0",
              }}
              title={`${seg.name}: ${formatSize(seg.size)}`}
            />
          );
        })}
      </div>
      <div className="prune-storage-legend">
        {allSegments.map((seg) => (
          <div key={seg.name} className="prune-storage-legend-item">
            <span
              className="prune-storage-legend-dot"
              style={{ backgroundColor: seg.name === "Free" ? "rgba(255,255,255,0.06)" : seg.name === "Other" ? "rgba(255,255,255,0.12)" : seg.color }}
            />
            <span className="prune-storage-legend-label">{seg.name}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ── Category Group ── */
function CategoryGroup({
  type,
  items,
  selectedPaths,
  onToggle,
  onToggleAll,
}: {
  type: string;
  items: any[];
  selectedPaths: Set<string>;
  onToggle: (path: string) => void;
  onToggleAll: (paths: string[], select: boolean) => void;
}) {
  const [expanded, setExpanded] = useState(true);

  const groupSize = items.reduce((s: number, a: any) => s + a.size, 0);
  const groupPaths = items.map((a: any) => a.artifact_path);
  const selectedInGroup = groupPaths.filter((p: string) => selectedPaths.has(p)).length;
  const allInGroupSelected = selectedInGroup === items.length;
  const someSelected = selectedInGroup > 0 && !allInGroupSelected;

  const color = getTypeColor(type);

  return (
    <div className="prune-category-group">
      <div className="prune-category-header" onClick={() => setExpanded(!expanded)}>
        <ChevronRight
          size={14}
          className={`prune-category-chevron ${expanded ? "expanded" : ""}`}
        />
        <input
          type="checkbox"
          className={`checkbox ${someSelected ? "partial" : ""}`}
          checked={allInGroupSelected}
          onClick={(e) => e.stopPropagation()}
          onChange={() => onToggleAll(groupPaths, !allInGroupSelected)}
        />
        <span className="prune-category-name">{type}</span>
        <span className="prune-category-meta">
          {items.length} item{items.length !== 1 ? "s" : ""} · {formatSize(groupSize)}
        </span>
        <span className="prune-category-color-dot" style={{ backgroundColor: color }} />
      </div>

      {expanded && (
        <div className="prune-category-items">
          {items.map((artifact: any) => (
            <label key={artifact.artifact_path} className="prune-artifact-row">
              <input
                type="checkbox"
                className="checkbox"
                checked={selectedPaths.has(artifact.artifact_path)}
                onChange={() => onToggle(artifact.artifact_path)}
              />
              <Folder size={14} className="prune-artifact-icon" />
              <div className="prune-artifact-info">
                <div className="prune-artifact-project">
                  {artifact.project_name}
                </div>
                <div
                  className="prune-artifact-path clickable"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    revealInFinder(artifact.artifact_path);
                  }}
                  title="Open in Finder"
                >
                  {shortenPath(artifact.artifact_path, artifact.project_name)}
                </div>
              </div>
              <div className="prune-artifact-size">{formatSize(artifact.size)}</div>
            </label>
          ))}
        </div>
      )}
    </div>
  );
}

/* ── Quick Filter Buttons ── */
type QuickFilter = "all" | "node_modules" | "large";

function QuickFilters({
  active,
  onChange,
  counts,
}: {
  active: QuickFilter;
  onChange: (f: QuickFilter) => void;
  counts: { nodeModules: number; large: number };
}) {
  return (
    <div className="prune-quick-filters">
      <button
        className={`prune-filter-btn ${active === "all" ? "active" : ""}`}
        onClick={() => onChange("all")}
      >
        All
      </button>
      <button
        className={`prune-filter-btn ${active === "node_modules" ? "active" : ""}`}
        onClick={() => onChange("node_modules")}
      >
        All node_modules
        {counts.nodeModules > 0 && <span className="prune-filter-count">{counts.nodeModules}</span>}
      </button>
      <button
        className={`prune-filter-btn ${active === "large" ? "active" : ""}`}
        onClick={() => onChange("large")}
      >
        All &gt;500 MB
        {counts.large > 0 && <span className="prune-filter-count">{counts.large}</span>}
      </button>
    </div>
  );
}

/* ── Sort Options ── */
type SortMode = "size" | "name";

/* ── List View ── */
function ListView() {
  const artifacts = usePruneStore((s) => s.artifacts);
  const selectedPaths = usePruneStore((s) => s.selectedPaths);
  const selectAll = usePruneStore((s) => s.selectAll);
  const deselectAll = usePruneStore((s) => s.deselectAll);
  const toggleSelect = usePruneStore((s) => s.toggleSelect);
  const reset = usePruneStore((s) => s.reset);
  const prune = usePruneStore((s) => s.prune);

  const [searchQuery, setSearchQuery] = useState("");
  const [quickFilter, setQuickFilter] = useState<QuickFilter>("all");
  const [sortMode, setSortMode] = useState<SortMode>("size");
  const [showConfirm, setShowConfirm] = useState(false);
  const [diskTotal, setDiskTotal] = useState(0);
  const [diskFree, setDiskFree] = useState(0);

  useEffect(() => {
    getSystemStats().then((s) => { setDiskTotal(s.disk_total); setDiskFree(s.disk_free); }).catch(() => {});
  }, []);

  const allSize = artifacts.reduce((sum, a) => sum + a.size, 0);
  const allSelected = artifacts.length > 0 && selectedPaths.size === artifacts.length;

  // Filter counts
  const filterCounts = useMemo(() => ({
    nodeModules: artifacts.filter((a) => a.artifact_type === "Node.js").length,
    large: artifacts.filter((a) => a.size >= 500 * 1024 * 1024).length,
  }), [artifacts]);

  // Apply filters
  const filtered = useMemo(() => {
    let list = artifacts;

    // Quick filter
    if (quickFilter === "node_modules") {
      list = list.filter((a) => a.artifact_type === "Node.js");
    } else if (quickFilter === "large") {
      list = list.filter((a) => a.size >= 500 * 1024 * 1024);
    }

    // Search
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      list = list.filter(
        (a) =>
          a.project_name.toLowerCase().includes(q) ||
          a.artifact_path.toLowerCase().includes(q) ||
          a.artifact_type.toLowerCase().includes(q)
      );
    }

    return list;
  }, [artifacts, quickFilter, searchQuery]);

  // Group by artifact_type and sort
  const grouped = useMemo(() => {
    const map = new Map<string, typeof filtered>();
    for (const a of filtered) {
      const existing = map.get(a.artifact_type) || [];
      existing.push(a);
      map.set(a.artifact_type, existing);
    }

    // Sort items within each group
    for (const [, items] of map) {
      items.sort((a, b) =>
        sortMode === "size" ? b.size - a.size : a.project_name.localeCompare(b.project_name)
      );
    }

    // Sort groups by selected sort mode
    return Array.from(map.entries()).sort((a, b) => {
      if (sortMode === "size") {
        const sizeA = a[1].reduce((s, i) => s + i.size, 0);
        const sizeB = b[1].reduce((s, i) => s + i.size, 0);
        return sizeB - sizeA;
      }
      return a[0].localeCompare(b[0]);
    });
  }, [filtered, sortMode]);

  // Stable storage bar data (always from full artifacts, sorted by size, unaffected by filters/sort)
  const storageBarCategories = useMemo(() => {
    const map = new Map<string, typeof artifacts>();
    for (const a of artifacts) {
      const existing = map.get(a.artifact_type) || [];
      existing.push(a);
      map.set(a.artifact_type, existing);
    }
    return Array.from(map.entries()).sort((a, b) => {
      const sizeA = a[1].reduce((s, i) => s + i.size, 0);
      const sizeB = b[1].reduce((s, i) => s + i.size, 0);
      return sizeB - sizeA;
    });
  }, [artifacts]);

  // Unique category count
  const categoryCount = new Set(artifacts.map((a) => a.artifact_type)).size;

  // Selected size
  const selectedSize = artifacts
    .filter((a) => selectedPaths.has(a.artifact_path))
    .reduce((sum, a) => sum + a.size, 0);

  // Toggle all in a group
  const toggleGroupAll = useCallback(
    (paths: string[], select: boolean) => {
      for (const p of paths) {
        const isSelected = usePruneStore.getState().selectedPaths.has(p);
        if (select && !isSelected) toggleSelect(p);
        if (!select && isSelected) toggleSelect(p);
      }
    },
    [toggleSelect]
  );

  if (artifacts.length === 0) {
    return (
      <div className="centered">
        <div className="prune-empty-icon">
          <Check size={26} strokeWidth={1.5} />
        </div>
        <div className="prune-idle-title">Nothing to prune</div>
        <div className="prune-idle-desc">
          No developer artifacts were found in this directory. Your projects are already clean.
        </div>
        <button className="btn" onClick={reset} style={{ marginTop: 8 }}>New Scan</button>
      </div>
    );
  }

  return (
    <>
      {/* Header */}
      <div className="prune-list-header">
        <div className="prune-list-summary">
          <span className="prune-list-title">Prune</span>
          <span className="prune-list-size">{formatSize(allSize)}</span>
          <span className="prune-list-context">
            items found across {categoryCount} categor{categoryCount !== 1 ? "ies" : "y"}
          </span>
        </div>
        <div className="prune-list-actions">
          <button
            className="btn"
            style={{ minWidth: 90 }}
            onClick={allSelected ? deselectAll : selectAll}
          >
            {allSelected ? "Deselect All" : "Select All"}
          </button>
          <button className="btn" onClick={reset}>
            New Scan
          </button>
        </div>
      </div>

      {/* Storage Bar */}
      <StorageBar categories={storageBarCategories} diskTotal={diskTotal} diskFree={diskFree} />

      {/* Quick Filters */}
      <QuickFilters active={quickFilter} onChange={setQuickFilter} counts={filterCounts} />

      {/* Search + Sort */}
      <div className="prune-search-row">
        <div className="prune-search-box">
          <Search size={13} className="prune-search-icon" />
          <input
            type="text"
            className="prune-search-input"
            placeholder="Search projects..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>
        <div className="prune-sort-controls">
          <span className="prune-sort-label">Sort</span>
          <button
            className={`prune-sort-btn ${sortMode === "size" ? "active" : ""}`}
            onClick={() => setSortMode("size")}
          >
            Size
          </button>
          <button
            className={`prune-sort-btn ${sortMode === "name" ? "active" : ""}`}
            onClick={() => setSortMode("name")}
          >
            Name
          </button>
        </div>
      </div>

      {/* Grouped List */}
      <div className="prune-artifact-list">
        {grouped.map(([type, items]) => (
          <CategoryGroup
            key={type}
            type={type}
            items={items}
            selectedPaths={selectedPaths}
            onToggle={toggleSelect}
            onToggleAll={toggleGroupAll}
          />
        ))}
      </div>

      {/* Footer */}
      <div className="module-footer">
        <span className="module-footer-info">
          {selectedPaths.size} of {artifacts.length} items selected
        </span>
        <button
          className="btn btn-primary"
          style={{ minWidth: 120 }}
          disabled={selectedPaths.size === 0}
          onClick={() => setShowConfirm(true)}
        >
          Prune {selectedPaths.size > 0 ? formatSize(selectedSize) : ""}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Prune ${selectedPaths.size} artifact${selectedPaths.size > 1 ? "s" : ""} (${formatSize(selectedSize)})?`}
        onConfirm={() => { setShowConfirm(false); prune(); }}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

/* ── Confetti Particle ── */
const CONFETTI_COLORS = [
  "rgba(255, 255, 255, 0.6)",
  "rgba(255, 255, 255, 0.4)",
  "rgba(255, 255, 255, 0.3)",
  "rgba(253, 72, 65, 0.35)",    // red muted
  "rgba(42, 200, 82, 0.35)",    // green muted
  "rgba(58, 123, 255, 0.3)",    // blue muted
  "rgba(253, 210, 37, 0.3)",    // yellow muted
  "rgba(142, 92, 246, 0.3)",    // purple muted
];

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  rotation: number;
  rotationSpeed: number;
  size: number;
  color: string;
  opacity: number;
  life: number;
  maxLife: number;
}

function Confetti({ active }: { active: boolean }) {
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

    // Create 20 particles from center-ish
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
        opacity: 1,
        life: 0,
        maxLife: 1600 + Math.random() * 1800,  // 1.6–3.4s
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

        const progress = p.life / p.maxLife;
        p.vy += 0.03;  // gravity
        p.x += p.vx;
        p.y += p.vy;
        p.rotation += p.rotationSpeed;
        p.opacity = 1 - Math.pow(progress, 2);

        ctx!.save();
        ctx!.translate(p.x, p.y);
        ctx!.rotate((p.rotation * Math.PI) / 180);
        ctx!.globalAlpha = p.opacity;
        ctx!.fillStyle = p.color;
        ctx!.fillRect(-p.size / 2, -p.size / 2, p.size, p.size * 0.6);
        ctx!.restore();
      }

      if (alive > 0) {
        animId = requestAnimationFrame(animate);
      }
    }

    animId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animId);
  }, [active]);

  if (!active) return null;
  return <canvas ref={canvasRef} className="prune-confetti" />;
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

/* ── Pruning View ── */
function PruningView() {
  const phase = usePruneStore((s) => s.phase);
  const progress = usePruneStore((s) => s.progress);
  const artifacts = usePruneStore((s) => s.artifacts);
  const selectedPaths = usePruneStore((s) => s.selectedPaths);
  const dismissDone = usePruneStore((s) => s.dismissDone);
  const isDone = phase === "done";

  // Staggered animation state
  const [showCard, setShowCard] = useState(false);
  const [showChips, setShowChips] = useState(false);
  const [showDone, setShowDone] = useState(false);
  const [showConfetti, setShowConfetti] = useState(false);
  const [chipsExpanded, setChipsExpanded] = useState(false);

  // Pick equivalence card once when done
  const cardRef = useRef<EquivalenceCard | null>(null);
  if (isDone && !cardRef.current && progress) {
    cardRef.current = pickEquivalenceCard(progress.bytes_freed);
  }

  // Reset animation state when leaving done
  useEffect(() => {
    if (!isDone) {
      setShowCard(false);
      setShowChips(false);
      setShowDone(false);
      setShowConfetti(false);
      setChipsExpanded(false);
      cardRef.current = null;
      return;
    }

    // Staggered reveal
    const t1 = setTimeout(() => { setShowConfetti(true); setShowCard(true); }, 500);
    const t2 = setTimeout(() => setShowChips(true), 800);
    const t3 = setTimeout(() => setShowDone(true), 1050);

    return () => { clearTimeout(t1); clearTimeout(t2); clearTimeout(t3); };
  }, [isDone]);

  const percent =
    isDone
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

  // Extract folder name from current_item path for display
  const currentLabel = progress?.current_item
    ? `Removing ${progress.current_item.split("/").filter(Boolean).pop() || progress.current_item}...`
    : "Starting...";

  // Breakdown chips — group selected items by artifact_type
  const breakdownChips = useMemo(() => {
    if (!isDone) return [];
    const selected = artifacts.filter((a) => selectedPaths.has(a.artifact_path));
    const map = new Map<string, { count: number; size: number }>();
    for (const a of selected) {
      const existing = map.get(a.artifact_type) || { count: 0, size: 0 };
      existing.count++;
      existing.size += a.size;
      map.set(a.artifact_type, existing);
    }
    return Array.from(map.entries())
      .sort((a, b) => b[1].size - a[1].size)
      .map(([type, { count, size }]) => ({ type, count, size }));
  }, [isDone, artifacts, selectedPaths]);

  const visibleChips = chipsExpanded ? breakdownChips : breakdownChips.slice(0, 3);
  const hiddenCount = breakdownChips.length - 3;

  const card = cardRef.current;

  return (
    <div className={`centered${isDone ? " prune-done" : ""}`}>
      <Confetti active={showConfetti} />

      {/* Circular progress ring */}
      <div className="prune-ring-wrap">
        <svg
          className="prune-ring-svg"
          width={ringSize}
          height={ringSize}
          viewBox={`0 0 ${ringSize} ${ringSize}`}
        >
          <defs>
            <linearGradient id="ring-glass" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.35)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.18)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.30)" />
            </linearGradient>
            <linearGradient id="ring-glass-done" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.5)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.28)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.45)" />
            </linearGradient>
            <filter id="ring-glow">
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
            stroke={isDone ? "url(#ring-glass-done)" : "url(#ring-glass)"}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={dashOffset}
            className="prune-ring-fill"
            filter={isDone ? "url(#ring-glow)" : undefined}
          />
        </svg>
        {isDone ? (
          <Check size={32} strokeWidth={2.5} className="prune-ring-check" />
        ) : (
          <span className="prune-ring-percent">{percent}%</span>
        )}
      </div>

      {/* Status text */}
      <div className="prune-ring-freed">
        {progress ? formatSize(progress.bytes_freed) : "0 B"} reclaimed
      </div>

      <div className="prune-ring-current">
        {isDone
          ? `${progress ? progress.items_total : 0} items removed`
          : currentLabel}
      </div>

      {/* Layer 2: Equivalence card (slides up) */}
      {isDone && card && (
        <div className={`prune-equiv-card${showCard ? " visible" : ""}`}>
          <div className="prune-equiv-icon">
            {card.isMilestone ? <SsdIcon /> : <span className="prune-equiv-emoji">{card.emoji}</span>}
          </div>
          <div className="prune-equiv-text">
            <div className="prune-equiv-title">{card.title}</div>
            <div className="prune-equiv-desc">{card.description}</div>
          </div>
        </div>
      )}

      {/* Layer 3: Breakdown chips (slides up) */}
      {isDone && breakdownChips.length > 0 && (
        <div className={`prune-breakdown-chips${showChips ? " visible" : ""}`}>
          {visibleChips.map((chip) => (
            <span key={chip.type} className="prune-breakdown-chip">
              {chip.count} {chip.type.toLowerCase()} · {formatSize(chip.size)}
            </span>
          ))}
          {hiddenCount > 0 && !chipsExpanded && (
            <button
              className="prune-breakdown-chip prune-breakdown-more"
              onClick={() => setChipsExpanded(true)}
            >
              +{hiddenCount} more
            </button>
          )}
        </div>
      )}

      {/* Done button (slides up) */}
      {isDone && (
        <button
          className={`btn prune-done-btn${showDone ? " visible" : ""}`}
          onClick={dismissDone}
        >
          Done
        </button>
      )}
    </div>
  );
}

/* ── Main ── */
export default function Prune() {
  const phase = usePruneStore((s) => s.phase);

  return (
    <div className="prune-container">
      {phase === "idle" && <IdleView />}
      {phase === "scanning" && <ScanningView />}
      {phase === "list" && <ListView />}
      {(phase === "pruning" || phase === "done") && <PruningView />}
    </div>
  );
}
