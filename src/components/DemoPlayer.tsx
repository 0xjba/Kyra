import { useState, useEffect, useRef } from "react";
import "../styles/demo-player.css";
// App icons
import iconGarageBand from "../assets/app-icons/garageband.png";
import iconIMovie from "../assets/app-icons/imovie.png";
import iconXcode from "../assets/app-icons/xcode.png";
import iconNumbers from "../assets/app-icons/numbers.png";
import iconPages from "../assets/app-icons/pages.png";
import iconKeynote from "../assets/app-icons/keynote.png";
import iconMaps from "../assets/app-icons/maps.png";
import iconNews from "../assets/app-icons/news.png";
import iconStocks from "../assets/app-icons/stocks.png";
import iconChess from "../assets/app-icons/chess.png";
import wallpaper from "../assets/gradient_wallpaper.png";

// ── Mock Data ──────────────────────────────────────────

const CLEAN_CATEGORIES = [
  { name: "System Caches", size: "28.4 GB", color: "#FD4841" },
  { name: "Browsers", size: "12.1 GB", color: "#FD8C34" },
  { name: "Developer Tools", size: "18.7 GB", color: "#2AC852" },
  { name: "App Leftovers", size: "10.8 GB", color: "#3A7BFF" },
];

const CLEAN_DETAIL_ITEMS = [
  { name: "com.apple.cache", size: "12.1 GB" },
  { name: "CloudKit", size: "9.8 GB" },
  { name: "Spotlight", size: "6.5 GB" },
];

const UNINSTALL_APPS = [
  { name: "GarageBand", size: "1.8 GB", icon: iconGarageBand },
  { name: "iMovie", size: "3.5 GB", icon: iconIMovie },
  { name: "Xcode", size: "12.3 GB", icon: iconXcode },
  { name: "Numbers", size: "0.4 GB", icon: iconNumbers },
  { name: "Pages", size: "0.5 GB", icon: iconPages },
  { name: "Keynote", size: "0.7 GB", icon: iconKeynote },
  { name: "Maps", size: "0.1 GB", icon: iconMaps },
  { name: "News", size: "0.1 GB", icon: iconNews },
  { name: "Stocks", size: "0.1 GB", icon: iconStocks },
  { name: "Chess", size: "0.1 GB", icon: iconChess },
];

const INSTALLER_FILES = [
  { name: "Docker.dmg", size: "0.9 GB", date: "Mar 12, 2026" },
  { name: "Xcode_16.pkg", size: "7.2 GB", date: "Feb 3, 2026" },
  { name: "macOS_Sequoia.dmg", size: "13.1 GB", date: "Jan 18, 2026" },
  { name: "Figma-2025.pkg", size: "0.4 GB", date: "Dec 8, 2025" },
  { name: "VSCode.dmg", size: "0.2 GB", date: "Nov 22, 2025" },
  { name: "Slack-4.38.dmg", size: "0.3 GB", date: "Oct 14, 2024" },
  { name: "Spotify.dmg", size: "0.1 GB", date: "Sep 5, 2024" },
];

const PRUNE_GROUPS = [
  {
    type: "node_modules",
    count: 12,
    totalSize: "48.2 GB",
    color: "#2AC852",
    expanded: true,
    items: [
      { project: "my-app", path: "~/Dev/my-app/node_modules", size: "1.2 GB" },
      { project: "dashboard", path: "~/Dev/dashboard/node_modules", size: "0.8 GB" },
    ],
  },
  {
    type: "target",
    count: 5,
    totalSize: "31.5 GB",
    color: "#FD4841",
    expanded: false,
    items: [],
  },
  {
    type: "DerivedData",
    count: 3,
    totalSize: "37.5 GB",
    color: "#FD8C34",
    expanded: false,
    items: [],
  },
];

const STORAGE_TOTAL = 500;

const STORAGE_SEGMENTS = [
  { label: "System", color: "#3A7BFF", ratio: 0.32 },
  { label: "Apps", color: "#2AC852", ratio: 0.24 },
  { label: "Dev", color: "#FD8C34", ratio: 0.22 },
  { label: "Media", color: "#8E5CF6", ratio: 0.14 },
  { label: "Other", color: "#8E8E93", ratio: 0.08 },
];

// ── Timeline ───────────────────────────────────────────

interface Step {
  scene: string;
  phase: string;
  duration: number;
  cx?: number;
  cy?: number;
  storage?: number;
}

const TIMELINE: Step[] = [
  // Clean
  { scene: "clean", phase: "show", duration: 1400, cx: 30, cy: 55 },
  { scene: "clean", phase: "cursor-btn", duration: 600, cx: 80, cy: 94 },
  { scene: "clean", phase: "click", duration: 300, cx: 80, cy: 94 },
  { scene: "clean", phase: "progress", duration: 2800, cx: 50, cy: 52 },
  { scene: "clean", phase: "done", duration: 1600, cx: 50, cy: 52, storage: 179 },

  // Uninstall
  { scene: "uninstall", phase: "show", duration: 1200, cx: 50, cy: 50 },
  { scene: "uninstall", phase: "cursor-card", duration: 500, cx: 50, cy: 52 },
  { scene: "uninstall", phase: "select", duration: 400, cx: 50, cy: 52 },
  { scene: "uninstall", phase: "cursor-btn", duration: 500, cx: 80, cy: 94 },
  { scene: "uninstall", phase: "click", duration: 300, cx: 80, cy: 94 },
  { scene: "uninstall", phase: "progress", duration: 2000, cx: 50, cy: 52 },
  { scene: "uninstall", phase: "done", duration: 1400, cx: 50, cy: 52, storage: 162 },

  // Installers
  { scene: "installers", phase: "show", duration: 1200, cx: 50, cy: 45 },
  { scene: "installers", phase: "cursor-check", duration: 500, cx: 6, cy: 40 },
  { scene: "installers", phase: "select", duration: 600, cx: 6, cy: 58 },
  { scene: "installers", phase: "cursor-btn", duration: 500, cx: 80, cy: 94 },
  { scene: "installers", phase: "click", duration: 300, cx: 80, cy: 94 },
  { scene: "installers", phase: "progress", duration: 2000, cx: 50, cy: 52 },
  { scene: "installers", phase: "done", duration: 1400, cx: 50, cy: 52, storage: 141 },

  // Prune
  { scene: "prune", phase: "show", duration: 1400, cx: 50, cy: 50 },
  { scene: "prune", phase: "cursor-btn", duration: 600, cx: 80, cy: 94 },
  { scene: "prune", phase: "click", duration: 300, cx: 80, cy: 94 },
  { scene: "prune", phase: "progress", duration: 2400, cx: 50, cy: 52 },
  { scene: "prune", phase: "done", duration: 1600, cx: 50, cy: 52, storage: 101 },

  // Summary
  { scene: "summary", phase: "show", duration: 3500, cx: 50, cy: 50 },
];


// ── Lucide-style SVG icons (miniaturized) ──────────────

const IconMonitor = () => (
  <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <rect x="2" y="3" width="20" height="14" rx="2" ry="2" /><path d="M8 21h8M12 17v4" />
  </svg>
);
const IconGlobe = () => (
  <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="10" /><path d="M2 12h20M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z" />
  </svg>
);
const IconCode = () => (
  <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="16 18 22 12 16 6" /><polyline points="8 6 2 12 8 18" />
  </svg>
);
const IconArchive = () => (
  <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <rect x="2" y="3" width="20" height="5" rx="1" /><path d="M2 8v11a2 2 0 002 2h16a2 2 0 002-2V8M10 12h4" />
  </svg>
);
const IconFolder = () => (
  <svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M14.5 13.5h-13a1 1 0 01-1-1v-8a1 1 0 011-1h4l2 2h7a1 1 0 011 1v6a1 1 0 01-1 1z" />
  </svg>
);
const IconSearch = () => (
  <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="11" cy="11" r="8" /><path d="M21 21l-4.35-4.35" />
  </svg>
);
const IconChevron = ({ expanded }: { expanded: boolean }) => (
  <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ transform: expanded ? "rotate(90deg)" : "none", transition: "transform 0.15s" }}>
    <path d="M9 6l6 6-6 6" />
  </svg>
);

const CAT_ICONS = [IconMonitor, IconGlobe, IconCode, IconArchive];

// ── Component ──────────────────────────────────────────

export const DEMO_STORAGE_TOTAL = STORAGE_TOTAL;
export const DEMO_STORAGE_SEGMENTS = STORAGE_SEGMENTS;

export default function DemoPlayer({ onStorageChange }: { onStorageChange?: (used: number) => void }) {
  const [idx, setIdx] = useState(0);
  const [storageUsed, setStorageUsed] = useState(249);
  const [progressPct, setProgressPct] = useState(0);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const step = TIMELINE[idx];
  const isClicking = step.phase === "click";

  useEffect(() => {
    if (step.phase === "progress") {
      requestAnimationFrame(() => setProgressPct(100));
    } else {
      setProgressPct(0);
    }

    timeoutRef.current = setTimeout(() => {
      const next = idx + 1;
      if (next < TIMELINE.length) {
        const nextStep = TIMELINE[next];
        if (nextStep.storage !== undefined) {
          setStorageUsed(nextStep.storage);
          onStorageChange?.(nextStep.storage);
        }
        setIdx(next);
      } else {
        setStorageUsed(249);
        onStorageChange?.(249);
        setIdx(0);
      }
    }, step.duration);

    return () => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, [idx, step.duration, step.phase]);

  // ── Storage bar — matches clean-storage-bar ──
  const freeGB = STORAGE_TOTAL - storageUsed;
  const freePct = (freeGB / STORAGE_TOTAL) * 100;

  const renderStorageBar = () => (
    <div className="dm-storage-bar">
      <div className="dm-storage-track">
        {STORAGE_SEGMENTS.map((seg) => {
          const pct = (seg.ratio * storageUsed / STORAGE_TOTAL) * 100;
          return (
            <div
              key={seg.label}
              className="dm-storage-seg"
              style={{
                width: `${pct}%`,
                background: `linear-gradient(90deg, color-mix(in srgb, ${seg.color}, white 25%) 0%, ${seg.color} 100%)`,
              }}
            />
          );
        })}
        <div className="dm-storage-seg dm-storage-free" style={{ width: `${freePct}%` }} />
      </div>
      <div className="dm-storage-legend">
        {STORAGE_SEGMENTS.map((seg) => (
          <div key={seg.label} className="dm-storage-legend-item">
            <span className="dm-storage-legend-dot" style={{ background: seg.color }} />
            <span className="dm-storage-legend-label">{seg.label}</span>
          </div>
        ))}
      </div>
    </div>
  );

  // ── Progress ring — matches *-ring-wrap (120px in real, scaled down) ──
  const renderProgressRing = (pct: number, freed: string, status: string, isDone: boolean) => {
    const size = 72;
    const stroke = 4;
    const radius = (size - stroke) / 2;
    const circumference = 2 * Math.PI * radius;
    const offset = circumference - (pct / 100) * circumference;

    return (
      <div className="dm-ring-centered">
        <div className="dm-ring-wrap" style={{ width: size, height: size }}>
          <svg className="dm-ring-svg" width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
            <defs>
              <linearGradient id="dm-ring-grad" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0%" stopColor={isDone ? "rgba(255,255,255,0.5)" : "rgba(255,255,255,0.35)"} />
                <stop offset="50%" stopColor={isDone ? "rgba(255,255,255,0.28)" : "rgba(255,255,255,0.18)"} />
                <stop offset="100%" stopColor={isDone ? "rgba(255,255,255,0.45)" : "rgba(255,255,255,0.30)"} />
              </linearGradient>
            </defs>
            <circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke="rgba(255,255,255,0.06)" strokeWidth={stroke} />
            <circle
              cx={size / 2} cy={size / 2} r={radius}
              fill="none"
              stroke="url(#dm-ring-grad)"
              strokeWidth={stroke}
              strokeLinecap="round"
              strokeDasharray={circumference}
              strokeDashoffset={offset}
              style={{ transition: "stroke-dashoffset 2.2s ease-in-out" }}
            />
          </svg>
          <div className="dm-ring-inner">
            {isDone ? (
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.7)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className="dm-ring-check">
                <path d="M20 6L9 17l-5-5" />
              </svg>
            ) : (
              <span className="dm-ring-pct">{pct}%</span>
            )}
          </div>
        </div>
        <div className="dm-ring-freed">{freed}</div>
        <div className="dm-ring-status">{status}</div>
      </div>
    );
  };

  // ── Clean — split panel with categories + detail ──
  const renderClean = () => {
    const phase = step.phase;

    if (phase === "progress" || phase === "done") {
      const isDone = phase === "done";
      return (
        <div className="dm-scene dm-scene-enter" key="clean-prog">
          {renderProgressRing(
            isDone ? 100 : Math.round(progressPct),
            isDone ? "70 GB reclaimed" : `${((progressPct / 100) * 70).toFixed(1)} GB reclaimed`,
            isDone ? "4 items cleaned" : "Removing System Caches...",
            isDone
          )}
        </div>
      );
    }

    return (
      <div className="dm-scene dm-scene-enter" key="clean-show">
        <div className="dm-summary-bar">
          <div className="dm-summary-left">
            <span className="dm-summary-title">Clean</span>
            <span className="dm-summary-size">70 GB</span>
            <span className="dm-summary-context">4 categories</span>
          </div>
          <button className="dm-btn-ghost">Select All</button>
        </div>

        {renderStorageBar()}

        <div className="dm-clean-split">
          {/* Left: category list */}
          <div className="dm-clean-left">
            {CLEAN_CATEGORIES.map((cat, i) => {
              const CatIcon = CAT_ICONS[i];
              return (
                <div key={i} className={`dm-clean-cat ${i === 0 ? "active" : ""}`}>
                  <div className="dm-checkbox checked" />
                  <div className="dm-clean-cat-icon"><CatIcon /></div>
                  <span className="dm-clean-cat-name">{cat.name}</span>
                  <span className="dm-clean-cat-size">{cat.size}</span>
                </div>
              );
            })}
          </div>
          {/* Right: detail panel */}
          <div className="dm-clean-right">
            <div className="dm-clean-detail-card">
              <div className="dm-clean-detail-icon"><IconMonitor /></div>
              <div className="dm-clean-detail-title">System Caches</div>
              <div className="dm-clean-detail-desc">Temporary system files and caches</div>
            </div>
            <div className="dm-clean-detail-list">
              {CLEAN_DETAIL_ITEMS.map((item, i) => (
                <div key={i} className="dm-clean-detail-item">
                  <div className="dm-checkbox checked" />
                  <span className="dm-clean-item-name">{item.name}</span>
                  <span className="dm-clean-item-size">{item.size}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        <div className="dm-module-footer">
          <span className="dm-module-footer-info">4 of 4 items selected</span>
          <button className={`dm-btn-primary ${phase === "cursor-btn" || isClicking ? "hover" : ""}`}>
            Clean 70 GB
          </button>
        </div>
      </div>
    );
  };

  // ── Uninstall — 5-column card grid ──
  const renderUninstall = () => {
    const phase = step.phase;
    const isSelected = phase === "select" || phase === "cursor-btn" || phase === "click";

    if (phase === "progress" || phase === "done") {
      const isDone = phase === "done";
      return (
        <div className="dm-scene dm-scene-enter" key="uninst-prog">
          {renderProgressRing(
            isDone ? 100 : Math.round(progressPct),
            isDone ? "17.6 GB reclaimed" : `${((progressPct / 100) * 17.6).toFixed(1)} GB reclaimed`,
            isDone ? "1 app removed" : "Removing Xcode...",
            isDone
          )}
        </div>
      );
    }

    return (
      <div className="dm-scene dm-scene-enter" key="uninst-show">
        <div className="dm-summary-bar">
          <div className="dm-summary-left">
            <span className="dm-summary-title">Uninstall</span>
            <span className="dm-summary-context">10 apps · 19.5 GB</span>
          </div>
          <button className="dm-btn-ghost">Select All</button>
        </div>

        {/* Search row */}
        <div className="dm-search-row">
          <div className="dm-search-box">
            <IconSearch />
            <span className="dm-search-placeholder">Search apps...</span>
          </div>
          <div className="dm-sort-controls">
            <span className="dm-sort-label">Sort</span>
            <span className="dm-sort-btn active">Size</span>
            <span className="dm-sort-btn">Name</span>
          </div>
        </div>

        {/* Card grid */}
        <div className="dm-uninstall-grid">
          {UNINSTALL_APPS.map((app, i) => (
            <div key={i} className={`dm-uninstall-card ${i === 2 && isSelected ? "selected" : ""}`}>
              {i === 2 && isSelected && <div className="dm-uninstall-card-check">✓</div>}
              <img src={app.icon} alt={app.name} className="dm-uninstall-card-icon-img" />
              <div className="dm-uninstall-card-name">{app.name}</div>
              <div className="dm-uninstall-card-size">{app.size}</div>
            </div>
          ))}
        </div>

        <div className="dm-module-footer">
          <span className="dm-module-footer-info">{isSelected ? "1" : "0"} of 10 apps selected</span>
          <button className={`dm-btn-primary ${phase === "cursor-btn" || isClicking ? "hover" : ""}`}>
            Uninstall{isSelected ? " 12.3 GB" : ""}
          </button>
        </div>
      </div>
    );
  };

  // ── Installers — grouped card with file rows ──
  const renderInstallers = () => {
    const phase = step.phase;
    const allSelected = phase === "select" || phase === "cursor-btn" || phase === "click";

    if (phase === "progress" || phase === "done") {
      const isDone = phase === "done";
      return (
        <div className="dm-scene dm-scene-enter" key="inst-prog">
          {renderProgressRing(
            isDone ? 100 : Math.round(progressPct),
            isDone ? "21.2 GB reclaimed" : `${((progressPct / 100) * 21.2).toFixed(1)} GB reclaimed`,
            isDone ? "3 items removed" : "Removing macOS_Sonoma.dmg...",
            isDone
          )}
        </div>
      );
    }

    return (
      <div className="dm-scene dm-scene-enter" key="inst-show">
        <div className="dm-summary-bar">
          <div className="dm-summary-left">
            <span className="dm-summary-title">Installers</span>
            <span className="dm-summary-size">22.3 GB</span>
            <span className="dm-summary-context">7 files</span>
          </div>
          <button className="dm-btn-ghost">{allSelected ? "Deselect All" : "Select All"}</button>
        </div>

        {/* Search row */}
        <div className="dm-search-row">
          <div className="dm-search-box">
            <IconSearch />
            <span className="dm-search-placeholder">Search files...</span>
          </div>
          <div className="dm-sort-controls">
            <span className="dm-sort-label">Sort</span>
            <span className="dm-sort-btn active">Size</span>
            <span className="dm-sort-btn">Name</span>
            <span className="dm-sort-btn">Date</span>
          </div>
        </div>

        <div className="dm-inst-card">
          {INSTALLER_FILES.map((file, i) => (
            <div key={i} className="dm-inst-row">
              <div className={`dm-checkbox ${allSelected ? "checked" : ""}`} />
              <div className="dm-inst-row-info">
                <div className="dm-inst-row-name">{file.name}</div>
                <div className="dm-inst-row-meta">{file.date}</div>
              </div>
              <div className="dm-inst-row-size">{file.size}</div>
            </div>
          ))}
        </div>

        <div className="dm-module-footer">
          <span className="dm-module-footer-info">{allSelected ? "7" : "0"} of 7 selected</span>
          <button className={`dm-btn-primary ${phase === "cursor-btn" || isClicking ? "hover" : ""}`}>
            Delete{allSelected ? " 22.3 GB" : ""}
          </button>
        </div>
      </div>
    );
  };

  // ── Prune — collapsible category groups ──
  const renderPrune = () => {
    const phase = step.phase;

    if (phase === "progress" || phase === "done") {
      const isDone = phase === "done";
      return (
        <div className="dm-scene dm-scene-enter" key="prune-prog">
          {renderProgressRing(
            isDone ? 100 : Math.round(progressPct),
            isDone ? "40 GB reclaimed" : `${((progressPct / 100) * 40).toFixed(1)} GB reclaimed`,
            isDone ? "4 items removed" : "Removing node_modules/...",
            isDone
          )}
        </div>
      );
    }

    return (
      <div className="dm-scene dm-scene-enter" key="prune-show">
        <div className="dm-summary-bar">
          <div className="dm-summary-left">
            <span className="dm-summary-title">Prune</span>
            <span className="dm-summary-size">117 GB</span>
            <span className="dm-summary-context">20 items · 3 categories</span>
          </div>
          <button className="dm-btn-ghost">Select All</button>
        </div>

        {renderStorageBar()}

        {/* Search row */}
        <div className="dm-search-row">
          <div className="dm-search-box">
            <IconSearch />
            <span className="dm-search-placeholder">Search projects...</span>
          </div>
          <div className="dm-sort-controls">
            <span className="dm-sort-label">Sort</span>
            <span className="dm-sort-btn active">Size</span>
            <span className="dm-sort-btn">Name</span>
          </div>
        </div>

        <div className="dm-prune-list">
          {PRUNE_GROUPS.map((group, gi) => (
            <div key={gi} className="dm-prune-group">
              <div className="dm-prune-cat-header">
                <IconChevron expanded={group.expanded} />
                <div className="dm-checkbox checked" />
                <span className="dm-prune-cat-name">{group.type}</span>
                <span className="dm-prune-cat-meta">{group.count} items · {group.totalSize}</span>
                <span className="dm-prune-cat-dot" style={{ background: group.color }} />
              </div>
              {group.expanded && (
                <div className="dm-prune-items">
                  {group.items.map((item, ii) => (
                    <div key={ii} className="dm-prune-item-row">
                      <div className="dm-checkbox checked" />
                      <IconFolder />
                      <div className="dm-prune-item-info">
                        <div className="dm-prune-item-project">{item.project}</div>
                        <div className="dm-prune-item-path">{item.path}</div>
                      </div>
                      <span className="dm-prune-item-size">{item.size}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>

        <div className="dm-module-footer">
          <span className="dm-module-footer-info">20 of 20 items selected</span>
          <button className={`dm-btn-primary ${phase === "cursor-btn" || isClicking ? "hover" : ""}`}>
            Prune 117 GB
          </button>
        </div>
      </div>
    );
  };

  // ── Summary screen ──
  const renderSummary = () => {
    const size = 72;
    const radius = (size - 4) / 2;
    const circumference = 2 * Math.PI * radius;

    return (
      <div className="dm-scene dm-scene-enter" key="summary">
        <div className="dm-ring-centered">
          <div className="dm-ring-wrap" style={{ width: size, height: size }}>
            <svg className="dm-ring-svg" width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
              <defs>
                <linearGradient id="dm-ring-done" x1="0" y1="0" x2="1" y2="1">
                  <stop offset="0%" stopColor="rgba(255,255,255,0.5)" />
                  <stop offset="50%" stopColor="rgba(255,255,255,0.28)" />
                  <stop offset="100%" stopColor="rgba(255,255,255,0.45)" />
                </linearGradient>
              </defs>
              <circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke="rgba(255,255,255,0.06)" strokeWidth={4} />
              <circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke="url(#dm-ring-done)" strokeWidth={4} strokeLinecap="round" strokeDasharray={circumference} strokeDashoffset={0} />
            </svg>
            <div className="dm-ring-inner">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.7)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className="dm-ring-check">
                <path d="M20 6L9 17l-5-5" />
              </svg>
            </div>
          </div>
          <div className="dm-summary-amount">148 GB freed</div>
          <div className="dm-summary-detail">249 GB → 101 GB used</div>
        </div>
      </div>
    );
  };

  const sceneRenderers: Record<string, () => React.ReactNode> = {
    clean: renderClean,
    uninstall: renderUninstall,
    installers: renderInstallers,
    prune: renderPrune,
    summary: renderSummary,
  };


  return (
    <div className="demo-player">
      {/* MacBook CSS frame */}
      <div className="dm-macbook">
        <div className="dm-macbook-screen">
          <div className="dm-macbook-notch" />
          <div className="dm-macbook-wallpaper" style={{ backgroundImage: `url(${wallpaper})` }}>
            <div className="dm-window">
              <div className="dm-titlebar">
                <div className="dm-traffic-pill">
                  <div className="dm-tl red" />
                  <div className="dm-tl yellow" />
                  <div className="dm-tl green" />
                </div>
                <span className="dm-titlebar-text">Kyra</span>
              </div>
              <div className="dm-titlebar-border" />

              <div className="dm-content">
                {sceneRenderers[step.scene]()}
              </div>

              <div className="dm-accent-bar" />

              <div
                className={`dm-cursor ${isClicking ? "clicking" : ""}`}
                style={{ left: `${step.cx ?? 50}%`, top: `${step.cy ?? 50}%` }}
              >
                <svg width="12" height="16" viewBox="0 0 12 16" fill="none">
                  <path d="M1 1l0 12 3.5-3.5L7.5 15l2-1-3-5.5H11L1 1z" fill="white" stroke="black" strokeWidth="1" strokeLinejoin="round" />
                </svg>
              </div>
            </div>
          </div>
        </div>
        <div className="dm-macbook-body">
          <div className="dm-macbook-notch-bottom" />
        </div>
      </div>

    </div>
  );
}
