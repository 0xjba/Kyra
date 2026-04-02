import { HardDrive, FolderOpen } from "lucide-react";
import { useAnalyzeStore } from "../stores/analyzeStore";
import { useSettingsStore } from "../stores/settingsStore";
import { pickFolder } from "../lib/tauri";
import Treemap from "../components/Treemap";
import { formatSize } from "../utils/format";
import "../styles/analyze.css";

/* ── Idle ── */

const QUICK_PATHS = [
  { path: "/", label: "Macintosh HD" },
  { path: "/Users", label: "Users" },
];

// Add home dir dynamically
const home = "/Users/" + (typeof window !== "undefined" ? window.__TAURI_INTERNALS__?.metadata?.currentDir?.split("/")[2] : "");

function IdleView() {
  const scanPath = useAnalyzeStore((s) => s.scanPath);
  const setScanPath = useAnalyzeStore((s) => s.setScanPath);
  const scan = useAnalyzeStore((s) => s.scan);

  const handlePickFolder = async () => {
    const selected = await pickFolder();
    if (selected) {
      setScanPath(selected);
    }
  };

  const handleScan = () => {
    if (scanPath.trim()) scan();
  };

  return (
    <div className="centered">
      <div className="analyze-idle-icon">
        <HardDrive size={26} strokeWidth={1.5} />
      </div>

      <div className="analyze-idle-title">Analyze Disk Usage</div>
      <div className="analyze-idle-desc">
        Visualize what's taking up space on your disk.
        Drill into folders and find large files.
      </div>

      <div className="analyze-path-row">
        <div className="analyze-path-input-wrap">
          <FolderOpen
            size={14}
            className="analyze-path-icon clickable"
            onClick={handlePickFolder}
          />
          <input
            type="text"
            className="analyze-path-input"
            value={scanPath}
            onChange={(e) => setScanPath(e.target.value)}
            placeholder="Browse or type a path to scan..."
            onKeyDown={(e) => e.key === "Enter" && handleScan()}
          />
        </div>
        <button className="btn btn-primary" onClick={handleScan} disabled={!scanPath.trim()}>
          Scan
        </button>
      </div>

      <div className="analyze-quick-picks">
        {QUICK_PATHS.map((p) => (
          <button
            key={p.path}
            className={`analyze-pick-chip${scanPath === p.path ? " active" : ""}`}
            onClick={() => setScanPath(p.path)}
          >
            {p.label}
          </button>
        ))}
      </div>

      <div className="analyze-detected-section">
        <span className="analyze-detected-label">WHAT YOU'LL SEE</span>
        <div className="analyze-detected-types">
          <span className="analyze-detected-chip">
            <span className="analyze-detected-dot" style={{ backgroundColor: "var(--blue)" }} />
            Treemap
          </span>
          <span className="analyze-detected-chip">
            <span className="analyze-detected-dot" style={{ backgroundColor: "var(--green)" }} />
            List view
          </span>
          <span className="analyze-detected-chip">
            <span className="analyze-detected-dot" style={{ backgroundColor: "var(--orange)" }} />
            Large files
          </span>
        </div>
      </div>
    </div>
  );
}

/* ── Scanning ── */

function ScanningView() {
  const progress = useAnalyzeStore((s) => s.progress);

  return (
    <div className="centered">
      <div className="spinner" />
      <div className="analyze-scanning-text">Scanning directory…</div>
      {progress && (
        <div className="analyze-scan-count">
          {progress.files_scanned.toLocaleString()} files scanned
        </div>
      )}
    </div>
  );
}

/** Turn a root path into a human-friendly label */
function friendlyRootName(node: { name: string; path: string }): string {
  const name = node.name || node.path;
  if (name === "/" || name === "") return "Macintosh HD";
  return name;
}

/* ── Breadcrumb ── */

function Breadcrumb() {
  const breadcrumb = useAnalyzeStore((s) => s.breadcrumb);
  const current = useAnalyzeStore((s) => s.current);
  const reveal = useAnalyzeStore((s) => s.reveal);
  const drillToRoot = useAnalyzeStore((s) => s.drillToRoot);
  const drillToIndex = useAnalyzeStore((s) => s.drillToIndex);

  if (!current) return null;

  // Build path segments from breadcrumb + current
  const segments: { name: string; onClick?: () => void }[] = [];

  // Root entry
  if (breadcrumb.length === 0) {
    segments.push({ name: friendlyRootName(current) });
  } else {
    // We have navigation history
    segments.push({ name: friendlyRootName(breadcrumb[0]), onClick: drillToRoot });

    for (let i = 1; i < breadcrumb.length; i++) {
      const idx = i;
      segments.push({
        name: breadcrumb[i].name,
        onClick: () => drillToIndex(idx),
      });
    }
    // Current (non-clickable)
    segments.push({ name: current.name });
  }

  return (
    <div className="analyze-breadcrumb">
      {segments.map((seg, i) => (
        <span key={i}>
          {i > 0 && <span className="analyze-breadcrumb-sep">/</span>}
          {seg.onClick ? (
            <button className="analyze-breadcrumb-item" onClick={seg.onClick}>
              {seg.name}
            </button>
          ) : (
            <span className="analyze-breadcrumb-current">
              {seg.name}
            </span>
          )}
        </span>
      ))}
      <button
        className="analyze-reveal-btn"
        onClick={() => reveal(current.path)}
        title="Show in Finder"
      >
        <svg
          width="8"
          height="8"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M6 2h8v8" />
          <path d="M14 2L2 14" />
        </svg>
      </button>
    </div>
  );
}

/* ── List View ── */

function ListView() {
  const current = useAnalyzeStore((s) => s.current);
  const drillInto = useAnalyzeStore((s) => s.drillInto);
  const reveal = useAnalyzeStore((s) => s.reveal);

  if (!current) return null;

  const maxSize =
    current.children.length > 0 ? current.children[0].size : 1;
  const parentSize = current.size || 1;

  return (
    <>
      {/* Column headers */}
      <div className="analyze-list-header">
        <span className="analyze-list-header-name">NAME</span>
        <span className="analyze-list-header-size">SIZE</span>
        <span className="analyze-list-header-pct">%</span>
      </div>

      <div className="analyze-list">
        {current.children.map((child) => {
          const pct = Math.round((child.size / parentSize) * 100);
          const canDrill = child.is_dir && child.children.length > 0;

          return (
            <div
              key={child.path}
              className={`analyze-list-row ${canDrill ? "analyze-list-drillable" : ""}`}
              onClick={() => canDrill && drillInto(child)}
            >
              <span className="analyze-list-checkbox" />
              <span className="analyze-list-name">{child.name}</span>
              <div className="analyze-list-bar-track">
                <div
                  className="analyze-list-bar-fill"
                  style={{ width: `${(child.size / maxSize) * 100}%` }}
                />
              </div>
              <span className="analyze-list-size">
                {formatSize(child.size)}
              </span>
              <span className="analyze-list-pct">{pct}%</span>
              <button
                className="analyze-reveal-row-btn"
                title="Show in Finder"
                onClick={(e) => {
                  e.stopPropagation();
                  reveal(child.path);
                }}
              >
                <svg
                  width="10"
                  height="10"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M6 2h8v8" />
                  <path d="M14 2L2 14" />
                </svg>
              </button>
            </div>
          );
        })}
      </div>
    </>
  );
}

/* ── Large Files View ── */

function LargeFilesView() {
  const largeFiles = useAnalyzeStore((s) => s.largeFiles);
  const largeFilesLoading = useAnalyzeStore((s) => s.largeFilesLoading);
  const loadLargeFiles = useAnalyzeStore((s) => s.loadLargeFiles);
  const reveal = useAnalyzeStore((s) => s.reveal);
  const threshold = useSettingsStore((s) => s.settings.large_file_threshold_mb);

  if (largeFilesLoading) {
    return (
      <div className="centered">
        <div className="spinner" />
        <div className="analyze-idle-text">Searching for large files…</div>
      </div>
    );
  }

  if (largeFiles.length === 0) {
    return (
      <div className="centered">
        <div className="analyze-idle-text">No files larger than {threshold >= 1000 ? `${threshold / 1000} GB` : `${threshold} MB`} found</div>
        <button className="btn btn-primary" onClick={loadLargeFiles}>
          Scan Again
        </button>
      </div>
    );
  }

  const maxSize = largeFiles[0].size;

  return (
    <>
      <div className="analyze-list">
        {largeFiles.map((file) => (
          <div
            key={file.path}
            className="analyze-list-row"
            onContextMenu={(e) => {
              e.preventDefault();
              reveal(file.path);
            }}
            title={file.path}
          >
            <span className="analyze-list-name">{file.name}</span>
            <div className="analyze-list-bar-track">
              <div
                className="analyze-list-bar-fill"
                style={{ width: `${(file.size / maxSize) * 100}%` }}
              />
            </div>
            <span className="analyze-list-size">{formatSize(file.size)}</span>
          </div>
        ))}
      </div>

    </>
  );
}

/* ── Ready View ── */

function ReadyView() {
  const current = useAnalyzeStore((s) => s.current);
  const viewMode = useAnalyzeStore((s) => s.viewMode);
  const setViewMode = useAnalyzeStore((s) => s.setViewMode);
  const activeTab = useAnalyzeStore((s) => s.activeTab);
  const drillInto = useAnalyzeStore((s) => s.drillInto);
  const reset = useAnalyzeStore((s) => s.reset);

  if (!current) return null;

  const contextText =
    activeTab === "tree"
      ? `${formatSize(current.size)} in ${friendlyRootName(current)}`
      : "Large Files";

  return (
    <>
      {/* Header */}
      <div className="analyze-header">
        <div className="analyze-header-left">
          <span className="analyze-header-title">Analyze</span>
          <span className="analyze-header-context">{contextText}</span>
        </div>
        <div className="analyze-actions">
          {activeTab === "tree" && (
            <>
              <button
                className={`btn ${viewMode === "treemap" ? "btn-active" : ""}`}
                onClick={() => setViewMode("treemap")}
              >
                Treemap
              </button>
              <button
                className={`btn ${viewMode === "list" ? "btn-active" : ""}`}
                onClick={() => setViewMode("list")}
              >
                List
              </button>
            </>
          )}
          <button className="btn" onClick={reset}>
            New Scan
          </button>
        </div>
      </div>

      {/* Breadcrumb */}
      {activeTab === "tree" && <Breadcrumb />}

      {/* Content */}
      <div className="analyze-content">
        {activeTab === "large-files" ? (
          <LargeFilesView />
        ) : viewMode === "treemap" ? (
          <Treemap node={current} onDrillIn={drillInto} />
        ) : (
          <ListView />
        )}
      </div>

      {/* Footer hint */}
      {activeTab === "tree" && (
        <div className="analyze-footer">
          <span className="analyze-footer-hint">Click folder to drill down</span>
        </div>
      )}
    </>
  );
}

/* ── Main ── */

export default function Analyze() {
  const phase = useAnalyzeStore((s) => s.phase);
  const error = useAnalyzeStore((s) => s.error);

  return (
    <div className="analyze-container">
      {error && (
        <div className="analyze-error">{error}</div>
      )}
      {phase === "idle" && <IdleView />}
      {phase === "scanning" && <ScanningView />}
      {phase === "ready" && <ReadyView />}
    </div>
  );
}
