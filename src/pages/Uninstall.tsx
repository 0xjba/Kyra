import { useEffect } from "react";
import { ChevronLeft } from "lucide-react";
import { useUninstallStore } from "../stores/uninstallStore";
import { formatSize } from "../utils/format";
import "../styles/uninstall.css";

function shortenPath(fullPath: string): string {
  const home = fullPath.indexOf("/Library/");
  if (home !== -1) {
    return "~" + fullPath.slice(home);
  }
  return fullPath;
}

function ScanningView() {
  return (
    <div className="uninstall-centered">
      <div className="uninstall-spinner" />
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
        Scanning installed applications...
      </div>
    </div>
  );
}

function AppListView() {
  const apps = useUninstallStore((s) => s.apps);
  const search = useUninstallStore((s) => s.search);
  const setSearch = useUninstallStore((s) => s.setSearch);
  const selectApp = useUninstallStore((s) => s.selectApp);

  const filtered = search
    ? apps.filter((app) =>
        app.name.toLowerCase().includes(search.toLowerCase())
      )
    : apps;

  return (
    <>
      <div className="uninstall-header">
        <h2>{apps.length} Apps Installed</h2>
        <input
          className="uninstall-search"
          type="text"
          placeholder="Search apps..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className="uninstall-app-list">
        {filtered.map((app) => (
          <div
            key={app.path}
            className="uninstall-app-row"
            onClick={() => selectApp(app)}
          >
            <div className="uninstall-app-name">
              {app.name}
              {app.version && (
                <span className="uninstall-app-version">{app.version}</span>
              )}
            </div>
            <div className="uninstall-app-size">{formatSize(app.size)}</div>
          </div>
        ))}
      </div>
    </>
  );
}

function DetailView() {
  const selectedApp = useUninstallStore((s) => s.selectedApp);
  const associatedFiles = useUninstallStore((s) => s.associatedFiles);
  const loadingFiles = useUninstallStore((s) => s.loadingFiles);
  const selectedFilePaths = useUninstallStore((s) => s.selectedFilePaths);
  const toggleFile = useUninstallStore((s) => s.toggleFile);
  const selectAllFiles = useUninstallStore((s) => s.selectAllFiles);
  const deselectAllFiles = useUninstallStore((s) => s.deselectAllFiles);
  const deselectApp = useUninstallStore((s) => s.deselectApp);
  const uninstall = useUninstallStore((s) => s.uninstall);

  if (!selectedApp) return null;

  const categories = new Map<string, typeof associatedFiles>();
  for (const file of associatedFiles) {
    const list = categories.get(file.category) || [];
    list.push(file);
    categories.set(file.category, list);
  }

  const selectedSize = associatedFiles
    .filter((f) => selectedFilePaths.has(f.path))
    .reduce((sum, f) => sum + f.size, 0);

  const totalSize = selectedApp.size + selectedSize;
  const allSelected =
    associatedFiles.length > 0 &&
    selectedFilePaths.size === associatedFiles.length;

  return (
    <div className="uninstall-detail">
      <div className="uninstall-detail-header">
        <button className="uninstall-detail-back" onClick={deselectApp}>
          <ChevronLeft size={16} />
        </button>
        <div>
          <div className="uninstall-detail-name">{selectedApp.name}</div>
          <div className="uninstall-detail-meta">
            {selectedApp.version && `v${selectedApp.version} · `}
            {formatSize(selectedApp.size)} app bundle
            {associatedFiles.length > 0 &&
              ` · ${formatSize(selectedSize)} associated files`}
          </div>
        </div>
      </div>

      <div className="uninstall-file-list">
        {loadingFiles ? (
          <div style={{ fontSize: 12, color: "var(--text-tertiary)", padding: "12px 0" }}>
            Searching for associated files...
          </div>
        ) : associatedFiles.length === 0 ? (
          <div style={{ fontSize: 12, color: "var(--text-tertiary)", padding: "12px 0" }}>
            No associated files found. Only the app bundle will be removed.
          </div>
        ) : (
          <>
            <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 8 }}>
              <button
                className="uninstall-btn"
                style={{ fontSize: 11, padding: "3px 10px" }}
                onClick={allSelected ? deselectAllFiles : selectAllFiles}
              >
                {allSelected ? "Deselect All" : "Select All"}
              </button>
            </div>
            {Array.from(categories.entries()).map(([category, files]) => (
              <div key={category}>
                <div className="uninstall-file-category">{category}</div>
                {files.map((file) => (
                  <div key={file.path} className="uninstall-file-row">
                    <input
                      type="checkbox"
                      className="uninstall-file-checkbox"
                      checked={selectedFilePaths.has(file.path)}
                      onChange={() => toggleFile(file.path)}
                    />
                    <span className="uninstall-file-path" title={file.path}>
                      {shortenPath(file.path)}
                    </span>
                    <span className="uninstall-file-size">
                      {formatSize(file.size)}
                    </span>
                  </div>
                ))}
              </div>
            ))}
          </>
        )}
      </div>

      <div className="uninstall-detail-footer">
        <span className="uninstall-footer-info">
          Total: {formatSize(totalSize)}
        </span>
        <button
          className="uninstall-btn uninstall-btn-danger"
          onClick={() => {
            if (window.confirm(`Remove ${selectedApp.name} and ${selectedFilePaths.size} associated files? This cannot be undone.`)) {
              uninstall();
            }
          }}
        >
          Remove {selectedApp.name}
        </button>
      </div>
    </div>
  );
}

function RemovingView() {
  const progress = useUninstallStore((s) => s.progress);
  const percent =
    progress && progress.items_total > 0
      ? (progress.items_done / progress.items_total) * 100
      : 0;

  return (
    <div className="uninstall-centered">
      <div style={{ width: "100%", maxWidth: 300 }}>
        <div
          style={{
            fontSize: 13,
            color: "var(--text-primary)",
            marginBottom: 4,
            textAlign: "center",
          }}
        >
          Removing...
        </div>
        <div className="uninstall-progress-bar-track">
          <div
            className="uninstall-progress-bar-fill"
            style={{ width: `${percent}%` }}
          />
        </div>
        {progress && (
          <div
            style={{
              fontSize: 11,
              color: "var(--text-tertiary)",
              textAlign: "center",
              marginTop: 4,
            }}
          >
            {progress.items_done}/{progress.items_total}
          </div>
        )}
      </div>
    </div>
  );
}

function DoneView() {
  const result = useUninstallStore((s) => s.result);
  const reset = useUninstallStore((s) => s.reset);
  const scanApps = useUninstallStore((s) => s.scanApps);

  return (
    <div className="uninstall-centered">
      <div className="uninstall-summary-stat">
        {result ? formatSize(result.bytes_freed) : "0 B"}
      </div>
      <div className="uninstall-summary-label">space freed</div>
      {result && result.errors.length > 0 && (
        <div
          style={{
            fontSize: 11,
            color: "var(--text-tertiary)",
            marginBottom: 12,
          }}
        >
          {result.errors.length} item{result.errors.length > 1 ? "s" : ""}{" "}
          skipped due to errors
        </div>
      )}
      <button
        className="uninstall-btn"
        onClick={() => {
          reset();
          scanApps();
        }}
      >
        Back to Apps
      </button>
    </div>
  );
}

export default function Uninstall() {
  const phase = useUninstallStore((s) => s.phase);
  const error = useUninstallStore((s) => s.error);
  const selectedApp = useUninstallStore((s) => s.selectedApp);
  const scanApps = useUninstallStore((s) => s.scanApps);

  useEffect(() => {
    if (phase === "idle") {
      scanApps();
    }
  }, [phase, scanApps]);

  return (
    <div className="uninstall-container">
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

      {phase === "scanning" && <ScanningView />}
      {phase === "list" && !selectedApp && <AppListView />}
      {phase === "list" && selectedApp && <DetailView />}
      {phase === "removing" && <RemovingView />}
      {phase === "done" && <DoneView />}
    </div>
  );
}
