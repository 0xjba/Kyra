import { usePurgeStore } from "../stores/purgeStore";
import "../styles/purge.css";

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

function IdleView() {
  const rootPath = usePurgeStore((s) => s.rootPath);
  const setRootPath = usePurgeStore((s) => s.setRootPath);
  const scan = usePurgeStore((s) => s.scan);
  const error = usePurgeStore((s) => s.error);

  return (
    <div className="purge-centered">
      <div className="purge-idle-title">Scan for Build Artifacts</div>
      <div className="purge-idle-desc">
        Find node_modules, target, dist, and other build artifacts that can be safely removed.
      </div>
      <div className="purge-path-row">
        <input
          type="text"
          className="purge-path-input"
          value={rootPath}
          onChange={(e) => setRootPath(e.target.value)}
          placeholder="~/Projects"
        />
        <button className="purge-scan-btn" onClick={scan}>
          Scan
        </button>
      </div>
      {error && <div className="purge-error">{error}</div>}
    </div>
  );
}

function ScanningView() {
  return (
    <div className="purge-centered">
      <div className="purge-spinner" />
      <div className="purge-scanning-text">Scanning for artifacts...</div>
    </div>
  );
}

function ListHeader() {
  const artifacts = usePurgeStore((s) => s.artifacts);
  const selectedPaths = usePurgeStore((s) => s.selectedPaths);
  const selectAll = usePurgeStore((s) => s.selectAll);
  const deselectAll = usePurgeStore((s) => s.deselectAll);
  const purge = usePurgeStore((s) => s.purge);
  const reset = usePurgeStore((s) => s.reset);

  const totalSize = artifacts
    .filter((a) => selectedPaths.has(a.artifact_path))
    .reduce((sum, a) => sum + a.size, 0);

  const allSelected = artifacts.length > 0 && selectedPaths.size === artifacts.length;

  const handlePurge = () => {
    if (selectedPaths.size === 0) return;
    const confirmed = window.confirm(
      `Delete ${selectedPaths.size} artifact${selectedPaths.size > 1 ? "s" : ""}? This will free ${formatSize(totalSize)}.`
    );
    if (confirmed) purge();
  };

  return (
    <div className="purge-list-header">
      <div className="purge-list-summary">
        <span className="purge-list-count">
          {artifacts.length} artifact{artifacts.length !== 1 ? "s" : ""} found
        </span>
        <span className="purge-list-size">{formatSize(totalSize)} selected</span>
      </div>
      <div className="purge-list-actions">
        <button
          className="purge-text-btn"
          onClick={allSelected ? deselectAll : selectAll}
        >
          {allSelected ? "Deselect All" : "Select All"}
        </button>
        <button className="purge-text-btn" onClick={reset}>
          New Scan
        </button>
        <button
          className="purge-delete-btn"
          disabled={selectedPaths.size === 0}
          onClick={handlePurge}
        >
          Purge Selected
        </button>
      </div>
    </div>
  );
}

function ArtifactList() {
  const artifacts = usePurgeStore((s) => s.artifacts);
  const selectedPaths = usePurgeStore((s) => s.selectedPaths);
  const toggleSelect = usePurgeStore((s) => s.toggleSelect);

  if (artifacts.length === 0) {
    return (
      <div className="purge-centered">
        <div className="purge-idle-desc">No build artifacts found in this directory.</div>
      </div>
    );
  }

  return (
    <div className="purge-artifact-list">
      {artifacts.map((artifact) => (
        <label key={artifact.artifact_path} className="purge-artifact-row">
          <input
            type="checkbox"
            checked={selectedPaths.has(artifact.artifact_path)}
            onChange={() => toggleSelect(artifact.artifact_path)}
          />
          <div className="purge-artifact-info">
            <div className="purge-artifact-project">
              {artifact.project_name}
              <span className="purge-artifact-type">{artifact.artifact_type}</span>
            </div>
            <div className="purge-artifact-path">{artifact.artifact_path}</div>
          </div>
          <div className="purge-artifact-size">{formatSize(artifact.size)}</div>
        </label>
      ))}
    </div>
  );
}

function ListView() {
  return (
    <>
      <ListHeader />
      <ArtifactList />
    </>
  );
}

function PurgingView() {
  const progress = usePurgeStore((s) => s.progress);

  const percent =
    progress && progress.items_total > 0
      ? Math.round((progress.items_done / progress.items_total) * 100)
      : 0;

  return (
    <div className="purge-centered">
      <div className="purge-spinner" />
      <div className="purge-scanning-text">Purging artifacts...</div>
      <div className="purge-progress-bar-track">
        <div className="purge-progress-bar-fill" style={{ width: `${percent}%` }} />
      </div>
      <div className="purge-progress-detail">
        {progress
          ? `${progress.items_done} / ${progress.items_total} — ${formatSize(progress.bytes_freed)} freed`
          : "Starting..."}
      </div>
    </div>
  );
}

function DoneView() {
  const result = usePurgeStore((s) => s.result);
  const reset = usePurgeStore((s) => s.reset);

  return (
    <div className="purge-centered">
      <div className="purge-done-icon">&#x2713;</div>
      <div className="purge-idle-title">Purge Complete</div>
      <div className="purge-idle-desc">
        Removed {result?.items_removed ?? 0} artifact
        {(result?.items_removed ?? 0) !== 1 ? "s" : ""},{" "}
        {formatSize(result?.bytes_freed ?? 0)} freed.
      </div>
      {result && result.errors.length > 0 && (
        <div className="purge-error">
          {result.errors.length} error{result.errors.length !== 1 ? "s" : ""}:{" "}
          {result.errors[0]}
        </div>
      )}
      <button className="purge-scan-btn" onClick={reset}>
        Scan Again
      </button>
    </div>
  );
}

export default function Purge() {
  const phase = usePurgeStore((s) => s.phase);

  return (
    <div className="purge-container">
      {phase === "idle" && <IdleView />}
      {phase === "scanning" && <ScanningView />}
      {phase === "list" && <ListView />}
      {phase === "purging" && <PurgingView />}
      {phase === "done" && <DoneView />}
    </div>
  );
}
