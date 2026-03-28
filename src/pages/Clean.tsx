import { useEffect, useState } from "react";
import { ChevronRight } from "lucide-react";
import { useCleanStore } from "../stores/cleanStore";
import { checkRunningProcesses, type RunningApp } from "../lib/tauri";
import "../styles/clean.css";

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

function IdleView({ onScan }: { onScan: () => void }) {
  return (
    <div className="clean-centered">
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
        Scan your system for reclaimable space
      </div>
      <button className="clean-btn clean-btn-primary" onClick={onScan}>
        Start Scan
      </button>
    </div>
  );
}

function ScanningView() {
  return (
    <div className="clean-centered">
      <div className="clean-spinner" />
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
        Scanning files and caches...
      </div>
    </div>
  );
}

function CategorySection({
  category,
  items,
  selectedIds,
  onToggle,
}: {
  category: string;
  items: { rule_id: string; label: string; total_size: number }[];
  selectedIds: Set<string>;
  onToggle: (id: string) => void;
}) {
  const [open, setOpen] = useState(true);
  const categorySize = items.reduce((sum, item) => sum + item.total_size, 0);

  return (
    <div className="clean-category">
      <div className="clean-category-header" onClick={() => setOpen(!open)}>
        <ChevronRight
          size={12}
          color="var(--text-tertiary)"
          className={`clean-category-chevron ${open ? "open" : ""}`}
        />
        <span className="clean-category-name">{category}</span>
        <span className="clean-category-size">{formatSize(categorySize)}</span>
      </div>
      {open &&
        items.map((item) => (
          <div key={item.rule_id} className="clean-item">
            <input
              type="checkbox"
              className="clean-item-checkbox"
              checked={selectedIds.has(item.rule_id)}
              onChange={() => onToggle(item.rule_id)}
            />
            <span className="clean-item-label">{item.label}</span>
            <span className="clean-item-size">
              {formatSize(item.total_size)}
            </span>
          </div>
        ))}
    </div>
  );
}

function ResultsView() {
  const items = useCleanStore((s) => s.items);
  const [runningApps, setRunningApps] = useState<RunningApp[]>([]);

  useEffect(() => {
    const ruleIds = items.map((item) => item.rule_id);
    checkRunningProcesses(ruleIds).then(setRunningApps).catch(() => {});
  }, [items]);
  const selectedIds = useCleanStore((s) => s.selectedIds);
  const toggleItem = useCleanStore((s) => s.toggleItem);
  const selectAll = useCleanStore((s) => s.selectAll);
  const deselectAll = useCleanStore((s) => s.deselectAll);
  const clean = useCleanStore((s) => s.clean);

  // Group items by category
  const categories = new Map<
    string,
    { rule_id: string; label: string; total_size: number }[]
  >();
  for (const item of items) {
    const list = categories.get(item.category) || [];
    list.push({
      rule_id: item.rule_id,
      label: item.label,
      total_size: item.total_size,
    });
    categories.set(item.category, list);
  }

  const selectedSize = items
    .filter((item) => selectedIds.has(item.rule_id))
    .reduce((sum, item) => sum + item.total_size, 0);

  const totalSize = items.reduce((sum, item) => sum + item.total_size, 0);
  const allSelected = selectedIds.size === items.length;

  return (
    <>
      <div className="clean-header">
        <h2>Found {formatSize(totalSize)} reclaimable</h2>
        <button
          className="clean-btn"
          onClick={allSelected ? deselectAll : selectAll}
        >
          {allSelected ? "Deselect All" : "Select All"}
        </button>
      </div>

      {runningApps.length > 0 && (
        <div style={{
          padding: "10px 14px",
          background: "rgba(250, 204, 21, 0.06)",
          border: "1px solid rgba(250, 204, 21, 0.15)",
          borderRadius: 8,
          marginBottom: 12,
        }}>
          <div style={{ fontSize: 12, color: "#facc15", fontWeight: 500, marginBottom: 4 }}>
            Running Applications Detected
          </div>
          <div style={{ fontSize: 11, color: "rgba(255, 255, 255, 0.4)", lineHeight: 1.5 }}>
            {runningApps.map((app) => app.name).join(", ")} — cleaning their caches while running may cause issues. Consider closing them first or deselecting their items.
          </div>
        </div>
      )}

      <div style={{ flex: 1, overflowY: "auto" }}>
        {Array.from(categories.entries()).map(([category, categoryItems]) => (
          <CategorySection
            key={category}
            category={category}
            items={categoryItems}
            selectedIds={selectedIds}
            onToggle={toggleItem}
          />
        ))}
      </div>

      <div className="clean-footer">
        <span className="clean-footer-info">
          {selectedIds.size} of {items.length} items selected (
          {formatSize(selectedSize)})
        </span>
        <button
          className="clean-btn clean-btn-primary"
          disabled={selectedIds.size === 0}
          onClick={clean}
        >
          Clean Selected
        </button>
      </div>
    </>
  );
}

function CleaningView() {
  const progress = useCleanStore((s) => s.progress);

  const percent =
    progress && progress.items_total > 0
      ? (progress.items_done / progress.items_total) * 100
      : 0;

  return (
    <div className="clean-centered">
      <div style={{ width: "100%", maxWidth: 300 }}>
        <div
          style={{
            fontSize: 13,
            color: "var(--text-primary)",
            marginBottom: 4,
            textAlign: "center",
          }}
        >
          Cleaning...
        </div>
        <div className="clean-progress-bar-track">
          <div
            className="clean-progress-bar-fill"
            style={{ width: `${percent}%` }}
          />
        </div>
        {progress && (
          <div
            style={{
              fontSize: 11,
              color: "var(--text-tertiary)",
              textAlign: "center",
            }}
          >
            {progress.current_item} ({progress.items_done}/
            {progress.items_total})
          </div>
        )}
      </div>
    </div>
  );
}

function DoneView() {
  const result = useCleanStore((s) => s.result);
  const reset = useCleanStore((s) => s.reset);
  const scan = useCleanStore((s) => s.scan);

  return (
    <div className="clean-centered">
      <div className="clean-summary">
        <div className="clean-summary-stat">
          {result ? formatSize(result.bytes_freed) : "0 B"}
        </div>
        <div className="clean-summary-label">space freed</div>
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
          className="clean-btn"
          onClick={() => {
            reset();
            scan();
          }}
        >
          Scan Again
        </button>
      </div>
    </div>
  );
}

export default function Clean() {
  const phase = useCleanStore((s) => s.phase);
  const error = useCleanStore((s) => s.error);
  const scan = useCleanStore((s) => s.scan);

  return (
    <div className="clean-container">
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

      {phase === "idle" && <IdleView onScan={scan} />}
      {phase === "scanning" && <ScanningView />}
      {phase === "results" && <ResultsView />}
      {phase === "cleaning" && <CleaningView />}
      {phase === "done" && <DoneView />}
    </div>
  );
}
