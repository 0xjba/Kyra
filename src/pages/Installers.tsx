import { useEffect } from "react";
import { useInstallersStore } from "../stores/installersStore";
import "../styles/installers.css";

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`;
  }
  return `${bytes} B`;
}

function formatDate(secs: number): string {
  if (secs === 0) return "\u2014";
  return new Date(secs * 1000).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

const EXT_LABELS: Record<string, string> = {
  dmg: "Disk Image",
  pkg: "Package",
  iso: "ISO Image",
  xip: "Xcode Archive",
  app: "Application",
};

function IdleView() {
  const scan = useInstallersStore((s) => s.scan);

  return (
    <div className="inst-centered">
      <div className="inst-idle-text">
        Scans <strong>Downloads</strong>, <strong>Desktop</strong>, and{" "}
        <strong>/tmp</strong> for installer files.
      </div>
      <button className="inst-scan-btn" onClick={scan}>
        Find Installers
      </button>
    </div>
  );
}

function ScanningView() {
  return (
    <div className="inst-centered">
      <div className="inst-spinner" />
      <div className="inst-scanning-text">Scanning for installers...</div>
    </div>
  );
}

function ListView() {
  const files = useInstallersStore((s) => s.files);
  const selected = useInstallersStore((s) => s.selected);
  const toggleSelect = useInstallersStore((s) => s.toggleSelect);
  const selectAll = useInstallersStore((s) => s.selectAll);
  const deselectAll = useInstallersStore((s) => s.deselectAll);
  const deleteSelected = useInstallersStore((s) => s.deleteSelected);
  const reset = useInstallersStore((s) => s.reset);

  const selectedSize = files
    .filter((f) => selected.has(f.path))
    .reduce((sum, f) => sum + f.size, 0);

  const grouped = files.reduce<Record<string, typeof files>>((acc, f) => {
    const key = f.extension;
    if (!acc[key]) acc[key] = [];
    acc[key].push(f);
    return acc;
  }, {});

  const handleDelete = () => {
    if (
      window.confirm(
        `Delete ${selected.size} installer file${selected.size === 1 ? "" : "s"} (${formatSize(selectedSize)})?`
      )
    ) {
      deleteSelected();
    }
  };

  if (files.length === 0) {
    return (
      <div className="inst-centered">
        <div className="inst-idle-text">No installer files found.</div>
        <button className="inst-scan-btn" onClick={reset}>
          Scan Again
        </button>
      </div>
    );
  }

  return (
    <div className="inst-list-container">
      <div className="inst-list-header">
        <div className="inst-list-summary">
          {files.length} file{files.length === 1 ? "" : "s"} found
          <span className="inst-list-sep">&middot;</span>
          {formatSize(selectedSize)} selected
        </div>
        <div className="inst-list-actions">
          <button className="inst-text-btn" onClick={selectAll}>
            Select All
          </button>
          <button className="inst-text-btn" onClick={deselectAll}>
            Deselect All
          </button>
          <button className="inst-text-btn" onClick={reset}>
            New Scan
          </button>
          <button
            className="inst-delete-btn"
            disabled={selected.size === 0}
            onClick={handleDelete}
          >
            Delete Selected
          </button>
        </div>
      </div>

      <div className="inst-list-scroll">
        {Object.entries(grouped).map(([ext, items]) => (
          <div key={ext} className="inst-group">
            <div className="inst-group-header">
              {EXT_LABELS[ext] || ext.toUpperCase()} ({items.length})
            </div>
            {items.map((file) => (
              <label key={file.path} className="inst-row">
                <input
                  type="checkbox"
                  checked={selected.has(file.path)}
                  onChange={() => toggleSelect(file.path)}
                />
                <div className="inst-row-info">
                  <div className="inst-row-name">{file.name}</div>
                  <div className="inst-row-meta">
                    {formatDate(file.modified_secs)}
                  </div>
                </div>
                <div className="inst-row-size">{formatSize(file.size)}</div>
              </label>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function DeletingView() {
  const progress = useInstallersStore((s) => s.progress);

  const percent =
    progress && progress.items_total > 0
      ? Math.round((progress.items_done / progress.items_total) * 100)
      : 0;

  return (
    <div className="inst-centered">
      <div className="inst-spinner" />
      <div className="inst-scanning-text">Deleting files...</div>
      {progress && (
        <>
          <div className="inst-progress-bar">
            <div
              className="inst-progress-fill"
              style={{ width: `${percent}%` }}
            />
          </div>
          <div className="inst-progress-text">
            {progress.items_done} / {progress.items_total} &middot;{" "}
            {formatSize(progress.bytes_freed)} freed
          </div>
        </>
      )}
    </div>
  );
}

function DoneView() {
  const result = useInstallersStore((s) => s.result);
  const reset = useInstallersStore((s) => s.reset);

  return (
    <div className="inst-centered">
      <div className="inst-success-icon">&check;</div>
      <div className="inst-done-stat">
        {result?.items_removed ?? 0} file{(result?.items_removed ?? 0) === 1 ? "" : "s"} deleted
      </div>
      <div className="inst-done-freed">
        {formatSize(result?.bytes_freed ?? 0)} freed
      </div>
      {result && result.errors.length > 0 && (
        <div className="inst-errors">
          {result.errors.map((err, i) => (
            <div key={i} className="inst-error-row">
              {err}
            </div>
          ))}
        </div>
      )}
      <button className="inst-scan-btn" onClick={reset}>
        Scan Again
      </button>
    </div>
  );
}

export default function Installers() {
  const phase = useInstallersStore((s) => s.phase);
  const scan = useInstallersStore((s) => s.scan);

  useEffect(() => {
    if (phase === "idle") {
      scan();
    }
  }, []);

  return (
    <div className="inst-container">
      {phase === "idle" && <IdleView />}
      {phase === "scanning" && <ScanningView />}
      {phase === "list" && <ListView />}
      {phase === "deleting" && <DeletingView />}
      {phase === "done" && <DoneView />}
    </div>
  );
}
