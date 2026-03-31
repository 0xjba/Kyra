import { useState, useCallback } from "react";
import { useAnalyzeStore } from "../stores/analyzeStore";
import { useSettingsStore } from "../stores/settingsStore";
import { deleteAnalyzedItem, type LargeFile } from "../lib/tauri";
import Sunburst from "../components/Sunburst";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import Toast from "../components/Toast";
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
        <button className="btn" onClick={scan}>
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
  const removeNodeByPath = useAnalyzeStore((s) => s.removeNodeByPath);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  const [deleteTarget, setDeleteTarget] = useState<{
    path: string;
    name: string;
  } | null>(null);
  const [toast, setToast] = useState<{
    message: string;
    variant: "success" | "error";
  } | null>(null);
  const [deleting, setDeleting] = useState(false);

  const handleDelete = useCallback(
    async (permanent: boolean) => {
      if (!deleteTarget || deleting) return;
      setDeleting(true);
      try {
        const freed = await deleteAnalyzedItem(deleteTarget.path, permanent);
        removeNodeByPath(deleteTarget.path, freed);
        setToast({
          message: `${permanent ? "Deleted" : "Trashed"} ${deleteTarget.name} (${formatSize(freed)})`,
          variant: "success",
        });
      } catch (e) {
        setToast({ message: String(e), variant: "error" });
      } finally {
        setDeleteTarget(null);
        setDeleting(false);
      }
    },
    [deleteTarget, deleting, removeNodeByPath]
  );

  if (!current) return null;

  const maxSize =
    current.children.length > 0 ? current.children[0].size : 1;

  return (
    <>
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
              {child.is_dir ? "\uD83D\uDCC1" : "\uD83D\uDCC4"}
            </span>
            {child.is_cleanable && (
              <span className="analyze-cleanable-dot" title="Safe to remove" />
            )}
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
            <button
              className="analyze-delete-btn"
              title="Delete"
              onClick={(e) => {
                e.stopPropagation();
                setDeleteTarget({ path: child.path, name: child.name });
              }}
            >
              <svg
                width="12"
                height="12"
                viewBox="0 0 16 16"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M2 4h12M5.333 4V2.667a1.333 1.333 0 011.334-1.334h2.666a1.333 1.333 0 011.334 1.334V4M6.667 7.333v4M9.333 7.333v4M13.333 4v9.333a1.333 1.333 0 01-1.333 1.334H4a1.333 1.333 0 01-1.333-1.334V4" />
              </svg>
            </button>
          </div>
        ))}
      </div>

      <DeleteConfirmDialog
        visible={deleteTarget !== null}
        title={`Delete "${deleteTarget?.name}"?`}
        onConfirm={() => handleDelete(!useTrash)}
        onCancel={() => setDeleteTarget(null)}
      />

      <Toast
        message={toast?.message ?? ""}
        visible={toast !== null}
        variant={toast?.variant}
        onDone={() => setToast(null)}
      />
    </>
  );
}

function LargeFilesView() {
  const largeFiles = useAnalyzeStore((s) => s.largeFiles);
  const largeFilesLoading = useAnalyzeStore((s) => s.largeFilesLoading);
  const loadLargeFiles = useAnalyzeStore((s) => s.loadLargeFiles);
  const removeLargeFile = useAnalyzeStore((s) => s.removeLargeFile);
  const reveal = useAnalyzeStore((s) => s.reveal);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  const [deleteTarget, setDeleteTarget] = useState<LargeFile | null>(null);
  const [toast, setToast] = useState<{
    message: string;
    variant: "success" | "error";
  } | null>(null);
  const [deleting, setDeleting] = useState(false);

  const handleDelete = useCallback(
    async (permanent: boolean) => {
      if (!deleteTarget || deleting) return;
      setDeleting(true);
      try {
        const freed = await deleteAnalyzedItem(deleteTarget.path, permanent);
        removeLargeFile(deleteTarget.path);
        setToast({
          message: `${permanent ? "Deleted" : "Trashed"} ${deleteTarget.name} (${formatSize(freed)})`,
          variant: "success",
        });
      } catch (e) {
        setToast({ message: String(e), variant: "error" });
      } finally {
        setDeleteTarget(null);
        setDeleting(false);
      }
    },
    [deleteTarget, deleting, removeLargeFile]
  );

  if (largeFilesLoading) {
    return (
      <div className="analyze-centered">
        <div className="analyze-spinner" />
        <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
          Searching for large files...
        </div>
      </div>
    );
  }

  if (largeFiles.length === 0) {
    return (
      <div className="analyze-centered">
        <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
          No files larger than 100 MB found
        </div>
        <button className="btn" onClick={loadLargeFiles}>
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
            <span className="analyze-list-icon">{"\uD83D\uDCC4"}</span>
            <span className="analyze-list-name">{file.name}</span>
            <div className="analyze-list-bar-track">
              <div
                className="analyze-list-bar-fill"
                style={{ width: `${(file.size / maxSize) * 100}%` }}
              />
            </div>
            <span className="analyze-list-size">
              {formatSize(file.size)}
            </span>
            <button
              className="analyze-delete-btn"
              title="Delete"
              onClick={(e) => {
                e.stopPropagation();
                setDeleteTarget(file);
              }}
            >
              <svg
                width="12"
                height="12"
                viewBox="0 0 16 16"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M2 4h12M5.333 4V2.667a1.333 1.333 0 011.334-1.334h2.666a1.333 1.333 0 011.334 1.334V4M6.667 7.333v4M9.333 7.333v4M13.333 4v9.333a1.333 1.333 0 01-1.333 1.334H4a1.333 1.333 0 01-1.333-1.334V4" />
              </svg>
            </button>
          </div>
        ))}
      </div>

      <DeleteConfirmDialog
        visible={deleteTarget !== null}
        title={`Delete "${deleteTarget?.name}"?`}
        onConfirm={() => handleDelete(!useTrash)}
        onCancel={() => setDeleteTarget(null)}
      />

      <Toast
        message={toast?.message ?? ""}
        visible={toast !== null}
        variant={toast?.variant}
        onDone={() => setToast(null)}
      />
    </>
  );
}

function ReadyView() {
  const current = useAnalyzeStore((s) => s.current);
  const viewMode = useAnalyzeStore((s) => s.viewMode);
  const setViewMode = useAnalyzeStore((s) => s.setViewMode);
  const activeTab = useAnalyzeStore((s) => s.activeTab);
  const setActiveTab = useAnalyzeStore((s) => s.setActiveTab);
  const drillInto = useAnalyzeStore((s) => s.drillInto);
  const drillUp = useAnalyzeStore((s) => s.drillUp);
  const reveal = useAnalyzeStore((s) => s.reveal);
  const reset = useAnalyzeStore((s) => s.reset);
  const breadcrumb = useAnalyzeStore((s) => s.breadcrumb);

  if (!current) return null;

  return (
    <>
      <div className="analyze-header">
        <div className="analyze-header-left">
          <span className="analyze-header-title">Analyze</span>
          <span className="analyze-header-size">{activeTab === "tree" ? formatSize(current.size) : ""}</span>
          {activeTab === "large-files" && <span className="analyze-header-context">Large Files</span>}
        </div>
        <div className="analyze-actions">
          <button
            className={`btn ${activeTab === "tree" ? "btn-active" : ""}`}
            onClick={() => setActiveTab("tree")}
          >
            Tree
          </button>
          <button
            className={`btn ${activeTab === "large-files" ? "btn-active" : ""}`}
            onClick={() => setActiveTab("large-files")}
          >
            Large Files
          </button>
          {activeTab === "tree" && (
            <>
              <span style={{ width: 1, height: 16, background: "var(--border)" }} />
              <button
                className={`btn ${viewMode === "sunburst" ? "btn-active" : ""}`}
                onClick={() => setViewMode("sunburst")}
              >
                Sunburst
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

      {activeTab === "tree" && <Breadcrumb />}

      <div className="analyze-content">
        {activeTab === "large-files" ? (
          <LargeFilesView />
        ) : viewMode === "sunburst" ? (
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
            background: "rgba(253, 72, 65, 0.08)",
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
