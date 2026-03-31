import { useEffect, useState, useRef, useMemo, useCallback } from "react";
import { useOptimizeStore } from "../stores/optimizeStore";
import { ShieldAlert } from "lucide-react";
import type { OptTask } from "../lib/tauri";
import "../styles/optimize.css";

type Category = "safe" | "restart" | "admin";
type Filter = "all" | "safe" | "restart" | "admin";

function categorize(task: OptTask): Category {
  if (task.needs_admin) return "admin";
  if (task.warning) return "restart";
  return "safe";
}

const CATEGORY_META: Record<Category, { label: string; dot: string }> = {
  safe: { label: "Safe to run", dot: "green" },
  restart: { label: "Needs restart", dot: "amber" },
  admin: { label: "Admin required", dot: "red" },
};

/* ── Admin Warning Dialog ──────────────────────────────── */

function AdminWarningDialog({
  visible,
  adminCount,
  onContinue,
  onCancel,
}: {
  visible: boolean;
  adminCount: number;
  onContinue: () => void;
  onCancel: () => void;
}) {
  if (!visible) return null;

  return (
    <div className="opt-dialog-overlay" onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}>
      <div className="opt-dialog">
        <div className="opt-dialog-icon">
          <ShieldAlert size={28} strokeWidth={1.6} />
        </div>
        <div className="opt-dialog-title">Admin Access Required</div>
        <div className="opt-dialog-desc">
          You selected {adminCount} admin task{adminCount !== 1 ? "s" : ""}. Each will prompt for your password. This may take a few minutes.
        </div>
        <div className="opt-dialog-buttons">
          <button className="btn" onClick={onCancel}>Cancel</button>
          <button className="btn btn-primary" onClick={onContinue}>Continue</button>
        </div>
      </div>
    </div>
  );
}

/* ── Task Card ──────────────────────────────────────── */

function TaskCard({
  task,
  status,
  checked,
  globalRunning,
  onToggle,
  onRun,
}: {
  task: OptTask;
  status: { status: string; message?: string };
  checked: boolean;
  globalRunning: boolean;
  onToggle: () => void;
  onRun: () => void;
}) {
  const actualStatus = status.status;
  const [displayStatus, setDisplayStatus] = useState(actualStatus);
  const runStartRef = useRef<number>(0);

  useEffect(() => {
    if (actualStatus === "running") {
      runStartRef.current = Date.now();
      setDisplayStatus("running");
    } else if (actualStatus === "done" || actualStatus === "error" || actualStatus === "skipped") {
      // Ensure spinner shows for at least 800ms
      const elapsed = Date.now() - runStartRef.current;
      const remaining = Math.max(0, 800 - elapsed);
      if (remaining > 0) {
        const timer = setTimeout(() => setDisplayStatus(actualStatus), remaining);
        return () => clearTimeout(timer);
      } else {
        setDisplayStatus(actualStatus);
      }
    } else {
      setDisplayStatus(actualStatus);
    }
  }, [actualStatus]);

  const [showCheckmark, setShowCheckmark] = useState(false);
  const [cardPulse, setCardPulse] = useState(false);

  const isRunning = displayStatus === "running";
  const isDone = displayStatus === "done";
  const isError = displayStatus === "error";
  const isSkipped = displayStatus === "skipped";
  const isFinished = isDone || isError || isSkipped;

  // Trigger checkmark animation and card pulse when done
  useEffect(() => {
    if (isDone) {
      setShowCheckmark(true);
      setCardPulse(true);
      const pulseTimer = setTimeout(() => setCardPulse(false), 1200);
      const checkTimer = setTimeout(() => setShowCheckmark(false), 1800);
      return () => { clearTimeout(pulseTimer); clearTimeout(checkTimer); };
    }
  }, [isDone]);

  // Friendly fallback messages per task
  const TASK_RESULTS: Record<string, string> = {
    dns_flush: "DNS resolver cache cleared",
    cache_refresh: "Thumbnail and preview caches refreshed",
    saved_state: "Saved window state data removed",
    launch_services: "Launch Services database rebuilt",
    icon_cache: "App icon cache refreshed",
    sqlite_vacuum: "Databases compacted",
    plist_repair: "Preference files validated",
    font_cache: "Font caches cleared",
    memory_purge: "Inactive memory returned to system",
    network_flush: "Network stack flushed and renewed",
    disk_permissions: "Disk permissions repaired",
    bluetooth_reset: "Bluetooth module reset",
    spotlight_rebuild: "Spotlight re-indexing started",
    dock_refresh: "Dock refreshed and reloaded",
    firewall_enable: "Firewall enabled",
  };

  const resultMessage = isDone
    ? TASK_RESULTS[task.id] || status.message?.trim() || "Completed successfully"
    : null;

  return (
    <div className={`opt-card${cardPulse ? " opt-card-pulse" : ""}`}>
      <input
        type="checkbox"
        className="checkbox"
        checked={checked}
        onChange={onToggle}
        disabled={globalRunning || isFinished}
      />
      <div className="opt-card-info">
        <div className="opt-card-name-row">
          <span className="opt-card-name">{task.name}</span>
          {task.needs_admin && <span className="opt-admin-badge">admin</span>}
        </div>
        <div className={`opt-card-desc${isDone ? " opt-card-desc-done" : ""}`}>
          {isDone && resultMessage ? resultMessage : task.description}
        </div>
        {isError && status.message ? (
          <div className="opt-card-error">{status.message}</div>
        ) : isSkipped && status.message ? (
          <div className="opt-card-skip">{status.message}</div>
        ) : task.warning && !isDone ? (
          <div className="opt-card-warning">{"\u26A0"} {task.warning}</div>
        ) : null}
      </div>

      <div className="opt-card-action">
        {isRunning ? (
          <button className="btn opt-run-btn running" disabled>
            <span className="opt-btn-spinner" />
          </button>
        ) : isDone && showCheckmark ? (
          <button className="btn opt-run-btn checkmark" disabled>
            <svg className="opt-checkmark" width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path className="opt-checkmark-path" d="M3 7L6 10L11 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        ) : isDone ? (
          <button className="btn opt-run-btn done" disabled>Done</button>
        ) : isError ? (
          <button className="btn opt-run-btn failed" disabled>Failed</button>
        ) : isSkipped ? (
          <button className="btn opt-run-btn skipped" disabled>Skipped</button>
        ) : (
          <button
            className="btn opt-run-btn"
            onClick={onRun}
            disabled={globalRunning}
          >
            Run
          </button>
        )}
      </div>
    </div>
  );
}

/* ── Optimize Page ──────────────────────────────────── */

export default function Optimize() {
  const tasks = useOptimizeStore((s) => s.tasks);
  const statuses = useOptimizeStore((s) => s.statuses);
  const running = useOptimizeStore((s) => s.running);
  const error = useOptimizeStore((s) => s.error);
  const enabledIds = useOptimizeStore((s) => s.enabledIds);
  const loadTasks = useOptimizeStore((s) => s.loadTasks);
  const runSingle = useOptimizeStore((s) => s.runSingle);
  const runTaskIds = useOptimizeStore((s) => s.runTaskIds);
  const toggleTask = useOptimizeStore((s) => s.toggleTask);
  const markSkipped = useOptimizeStore((s) => s.markSkipped);

  const [filter, setFilter] = useState<Filter>("all");
  const [showAdminDialog, setShowAdminDialog] = useState(false);
  const [pendingAdminIds, setPendingAdminIds] = useState<string[]>([]);
  const [executionDone, setExecutionDone] = useState(false);

  useEffect(() => {
    if (tasks.length === 0) {
      loadTasks();
    }
  }, [tasks.length, loadTasks]);

  // Group tasks by category
  const grouped = useMemo(() => {
    const groups: Record<Category, OptTask[]> = { safe: [], restart: [], admin: [] };
    for (const task of tasks) {
      groups[categorize(task)].push(task);
    }
    return groups;
  }, [tasks]);

  // Summary counts (only after execution completes)
  const summary = useMemo(() => {
    if (!executionDone) return null;
    let completed = 0, skipped = 0, failed = 0;
    for (const s of Object.values(statuses)) {
      if (s.status === "done") completed++;
      else if (s.status === "skipped") skipped++;
      else if (s.status === "error") failed++;
    }
    if (completed === 0 && skipped === 0 && failed === 0) return null;
    return { completed, skipped, failed };
  }, [statuses, executionDone]);

  // Header context text
  const headerContext = summary
    ? `${summary.completed} completed${summary.skipped > 0 ? `, ${summary.skipped} skipped` : ""}${summary.failed > 0 ? `, ${summary.failed} failed` : ""}`
    : `${tasks.length} tasks available`;

  // Count selected
  const selectedCount = enabledIds.size;

  // Categories to show based on filter
  const visibleCategories: Category[] = filter === "all"
    ? (["safe", "restart", "admin"] as Category[]).filter((c) => grouped[c].length > 0)
    : grouped[filter].length > 0
      ? [filter]
      : [];

  // Run Selected handler — phased execution
  const handleRunSelected = useCallback(async () => {
    const selectedTasks = tasks.filter((t) => enabledIds.has(t.id));
    if (selectedTasks.length === 0) return;

    const nonAdminIds = selectedTasks.filter((t) => !t.needs_admin).map((t) => t.id);
    const adminIds = selectedTasks.filter((t) => t.needs_admin).map((t) => t.id);

    setExecutionDone(false);

    // Phase 1: Run non-admin tasks
    if (nonAdminIds.length > 0) {
      await runTaskIds(nonAdminIds);
    }

    // Phase 2: Handle admin tasks
    if (adminIds.length > 0) {
      setPendingAdminIds(adminIds);
      setShowAdminDialog(true);
    } else {
      setExecutionDone(true);
    }
  }, [tasks, enabledIds, runTaskIds]);

  // Admin dialog: Continue
  const handleAdminContinue = useCallback(async () => {
    setShowAdminDialog(false);
    const ids = pendingAdminIds;
    setPendingAdminIds([]);
    await runTaskIds(ids);
    setExecutionDone(true);
  }, [pendingAdminIds, runTaskIds]);

  // Admin dialog: Cancel
  const handleAdminCancel = useCallback(() => {
    setShowAdminDialog(false);
    markSkipped(pendingAdminIds, "Cancelled by user");
    setPendingAdminIds([]);
    setExecutionDone(true);
  }, [pendingAdminIds, markSkipped]);

  return (
    <div className="opt-container">
      {error && <div className="opt-error">{error}</div>}

      {/* Header */}
      <div className="opt-header">
        <div className="opt-header-left">
          <span className="opt-header-title">Optimize</span>
          <span className="opt-header-context">{headerContext}</span>
        </div>
      </div>

      {/* Filter chips */}
      <div className="opt-filters">
        <button
          className={`opt-filter-chip${filter === "all" ? " active" : ""}`}
          onClick={() => setFilter("all")}
        >
          All
        </button>
        <button
          className={`opt-filter-chip${filter === "safe" ? " active" : ""}`}
          onClick={() => setFilter("safe")}
        >
          <span className="opt-filter-dot green" />
          Safe
        </button>
        <button
          className={`opt-filter-chip${filter === "restart" ? " active" : ""}`}
          onClick={() => setFilter("restart")}
        >
          <span className="opt-filter-dot amber" />
          Needs restart
        </button>
        <button
          className={`opt-filter-chip${filter === "admin" ? " active" : ""}`}
          onClick={() => setFilter("admin")}
        >
          <span className="opt-filter-dot red" />
          Admin
        </button>
      </div>

      {/* Summary bar */}
      {summary && (
        <div className="opt-summary">
          <span className="opt-summary-stat">
            <span className="opt-summary-dot green" />
            {summary.completed} completed
          </span>
          {summary.skipped > 0 && (
            <span className="opt-summary-stat">
              <span className="opt-summary-dot amber" />
              {summary.skipped} skipped
            </span>
          )}
          {summary.failed > 0 && (
            <span className="opt-summary-stat">
              <span className="opt-summary-dot red" />
              {summary.failed} failed
            </span>
          )}
        </div>
      )}

      {/* Task list */}
      <div className="opt-task-list">
        {visibleCategories.map((cat) => (
          <div key={cat} className="opt-category-group">
            <div className="opt-category-header">
              <span className={`opt-category-dot ${CATEGORY_META[cat].dot}`} />
              <span className="opt-category-name">{CATEGORY_META[cat].label}</span>
              <span className="opt-category-count">{"\u00B7"} {grouped[cat].length} task{grouped[cat].length !== 1 ? "s" : ""}</span>
            </div>
            <div className="opt-cards">
              {grouped[cat].map((task) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  status={statuses[task.id] || { status: "ready" }}
                  checked={enabledIds.has(task.id)}
                  globalRunning={running}
                  onToggle={() => toggleTask(task.id)}
                  onRun={() => runSingle(task.id)}
                />
              ))}
            </div>
          </div>
        ))}
      </div>

      {/* Footer */}
      <div className="opt-footer">
        <span className="opt-footer-count">
          {selectedCount} of {tasks.length} tasks selected
        </span>
        <button
          className="btn btn-primary"
          onClick={handleRunSelected}
          disabled={running || selectedCount === 0}
        >
          Run Selected
        </button>
      </div>

      {/* Admin warning dialog */}
      <AdminWarningDialog
        visible={showAdminDialog}
        adminCount={pendingAdminIds.length}
        onContinue={handleAdminContinue}
        onCancel={handleAdminCancel}
      />
    </div>
  );
}
