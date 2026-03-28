import { useAnalyzeStore } from "../stores/analyzeStore";
import Sunburst from "../components/Sunburst";
import { formatSize } from "../utils/format";
import "../styles/analyze.css";

function IdleView() {
  const scanPath = useAnalyzeStore((s) => s.scanPath);
  const setScanPath = useAnalyzeStore((s) => s.setScanPath);
  const scan = useAnalyzeStore((s) => s.scan);

  return (
    <div className="analyze-centered">
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
        Scan a directory to explore disk usage
      </div>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <input
          className="analyze-path-input"
          type="text"
          value={scanPath}
          onChange={(e) => setScanPath(e.target.value)}
          placeholder="/"
          onKeyDown={(e) => e.key === "Enter" && scan()}
        />
        <button className="analyze-btn" onClick={scan}>
          Scan
        </button>
      </div>
    </div>
  );
}

function ScanningView() {
  const progress = useAnalyzeStore((s) => s.progress);

  return (
    <div className="analyze-centered">
      <div className="analyze-spinner" />
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
        Scanning...
      </div>
      {progress && (
        <div style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
          {progress.files_scanned.toLocaleString()} files scanned
        </div>
      )}
    </div>
  );
}

function Breadcrumb() {
  const breadcrumb = useAnalyzeStore((s) => s.breadcrumb);
  const current = useAnalyzeStore((s) => s.current);
  const drillToRoot = useAnalyzeStore((s) => s.drillToRoot);
  const drillToIndex = useAnalyzeStore((s) => s.drillToIndex);

  if (breadcrumb.length === 0) return null;

  return (
    <div className="analyze-breadcrumb">
      <button className="analyze-breadcrumb-item" onClick={drillToRoot}>
        {breadcrumb[0]?.name || "/"}
      </button>
      {breadcrumb.slice(1).map((node, i) => (
        <span key={node.path}>
          <span className="analyze-breadcrumb-sep">/</span>
          <button
            className="analyze-breadcrumb-item"
            onClick={() => drillToIndex(i + 1)}
          >
            {node.name}
          </button>
        </span>
      ))}
      {current && (
        <span>
          <span className="analyze-breadcrumb-sep">/</span>
          <span style={{ color: "var(--text-primary)" }}>{current.name}</span>
        </span>
      )}
    </div>
  );
}

function ListView() {
  const current = useAnalyzeStore((s) => s.current);
  const drillInto = useAnalyzeStore((s) => s.drillInto);
  const reveal = useAnalyzeStore((s) => s.reveal);

  if (!current) return null;

  const maxSize = current.children.length > 0
    ? current.children[0].size
    : 1;

  return (
    <div className="analyze-list">
      {current.children.map((child) => (
        <div
          key={child.path}
          className="analyze-list-row"
          onClick={() => {
            if (child.is_dir && child.children.length > 0) {
              drillInto(child);
            }
          }}
          onContextMenu={(e) => {
            e.preventDefault();
            reveal(child.path);
          }}
        >
          <span className="analyze-list-icon">
            {child.is_dir ? "📁" : "📄"}
          </span>
          <span className="analyze-list-name">{child.name}</span>
          <div className="analyze-list-bar-track">
            <div
              className="analyze-list-bar-fill"
              style={{ width: `${(child.size / maxSize) * 100}%` }}
            />
          </div>
          <span className="analyze-list-size">{formatSize(child.size)}</span>
        </div>
      ))}
    </div>
  );
}

function ReadyView() {
  const current = useAnalyzeStore((s) => s.current);
  const viewMode = useAnalyzeStore((s) => s.viewMode);
  const setViewMode = useAnalyzeStore((s) => s.setViewMode);
  const drillInto = useAnalyzeStore((s) => s.drillInto);
  const drillUp = useAnalyzeStore((s) => s.drillUp);
  const reveal = useAnalyzeStore((s) => s.reveal);
  const reset = useAnalyzeStore((s) => s.reset);
  const breadcrumb = useAnalyzeStore((s) => s.breadcrumb);

  if (!current) return null;

  return (
    <>
      <div className="analyze-header">
        <h2>{formatSize(current.size)}</h2>
        <div className="analyze-actions">
          <button
            className={`analyze-btn ${viewMode === "sunburst" ? "analyze-btn-active" : ""}`}
            onClick={() => setViewMode("sunburst")}
          >
            Sunburst
          </button>
          <button
            className={`analyze-btn ${viewMode === "list" ? "analyze-btn-active" : ""}`}
            onClick={() => setViewMode("list")}
          >
            List
          </button>
          <button className="analyze-btn" onClick={reset}>
            New Scan
          </button>
        </div>
      </div>

      <Breadcrumb />

      <div className="analyze-content">
        {viewMode === "sunburst" ? (
          <div
            onClick={(e) => {
              if (e.target === e.currentTarget && breadcrumb.length > 0) {
                drillUp();
              }
            }}
          >
            <Sunburst
              node={current}
              onDrillIn={drillInto}
              onReveal={reveal}
            />
          </div>
        ) : (
          <ListView />
        )}
      </div>
    </>
  );
}

export default function Analyze() {
  const phase = useAnalyzeStore((s) => s.phase);
  const error = useAnalyzeStore((s) => s.error);

  return (
    <div className="analyze-container">
      {error && (
        <div
          style={{
            fontSize: 12,
            color: "var(--red)",
            padding: "8px 12px",
            background: "rgba(248, 113, 113, 0.08)",
            borderRadius: 6,
            marginBottom: 12,
          }}
        >
          {error}
        </div>
      )}

      {phase === "idle" && <IdleView />}
      {phase === "scanning" && <ScanningView />}
      {phase === "ready" && <ReadyView />}
    </div>
  );
}
