import { useEffect } from "react";
import { useOptimizeStore } from "../stores/optimizeStore";
import "../styles/optimize.css";

function TaskRow({
  task,
  enabled,
  status,
  running,
  onToggle,
  onRun,
}: {
  task: { id: string; name: string; description: string; needs_admin: boolean; warning: string | null };
  enabled: boolean;
  status: { status: string; message?: string };
  running: boolean;
  onToggle: () => void;
  onRun: () => void;
}) {
  const statusClass = `optimize-status optimize-status-${status.status}`;

  return (
    <div className="optimize-task">
      <label className="optimize-toggle">
        <input
          type="checkbox"
          checked={enabled}
          onChange={onToggle}
          disabled={running}
        />
        <span className="optimize-toggle-track" />
      </label>

      <div className="optimize-task-info">
        <div className="optimize-task-name">
          {task.name}
          {task.needs_admin && <span className="optimize-admin-badge">admin</span>}
        </div>
        <div className="optimize-task-desc">{task.description}</div>
        {task.warning && (
          <div className="optimize-task-warning">{task.warning}</div>
        )}
        {status.status === "error" && status.message && (
          <div style={{ fontSize: 10, color: "var(--red)", opacity: 0.7, marginTop: 2 }}>
            {status.message}
          </div>
        )}
      </div>

      <div className={statusClass}>
        <div className="optimize-status-dot" />
        <span style={{ color: "var(--text-tertiary)" }}>
          {status.status === "ready"
            ? ""
            : status.status === "running"
              ? "Running"
              : status.status === "done"
                ? "Done"
                : status.status === "error"
                  ? "Failed"
                  : "Skipped"}
        </span>
      </div>

      <button
        className="optimize-run-btn"
        onClick={onRun}
        disabled={running || status.status === "running"}
      >
        Run
      </button>
    </div>
  );
}

export default function Optimize() {
  const tasks = useOptimizeStore((s) => s.tasks);
  const enabledIds = useOptimizeStore((s) => s.enabledIds);
  const statuses = useOptimizeStore((s) => s.statuses);
  const running = useOptimizeStore((s) => s.running);
  const result = useOptimizeStore((s) => s.result);
  const error = useOptimizeStore((s) => s.error);
  const loadTasks = useOptimizeStore((s) => s.loadTasks);
  const toggleTask = useOptimizeStore((s) => s.toggleTask);
  const enableAll = useOptimizeStore((s) => s.enableAll);
  const disableAll = useOptimizeStore((s) => s.disableAll);
  const runSelected = useOptimizeStore((s) => s.runSelected);
  const runSingle = useOptimizeStore((s) => s.runSingle);
  const reset = useOptimizeStore((s) => s.reset);

  useEffect(() => {
    if (tasks.length === 0) {
      loadTasks();
    }
  }, [tasks.length, loadTasks]);

  const allEnabled = enabledIds.size === tasks.length;

  return (
    <div className="optimize-container">
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

      <div className="optimize-header">
        <h2>Optimization Tasks</h2>
        <div className="optimize-actions">
          <button
            className="optimize-btn"
            onClick={allEnabled ? disableAll : enableAll}
            disabled={running}
          >
            {allEnabled ? "Deselect All" : "Select All"}
          </button>
          {result && !running && (
            <button className="optimize-btn" onClick={reset}>
              Reset
            </button>
          )}
          <button
            className="optimize-btn optimize-btn-primary"
            onClick={runSelected}
            disabled={running || enabledIds.size === 0}
          >
            {running ? "Running..." : "Run Selected"}
          </button>
        </div>
      </div>

      <div className="optimize-task-list">
        {tasks.map((task) => (
          <TaskRow
            key={task.id}
            task={task}
            enabled={enabledIds.has(task.id)}
            status={statuses[task.id] || { status: "ready" }}
            running={running}
            onToggle={() => toggleTask(task.id)}
            onRun={() => runSingle(task.id)}
          />
        ))}
      </div>

      {result && !running && (
        <div className="optimize-summary">
          <span className="optimize-summary-stat">
            {result.tasks_succeeded} succeeded
          </span>
          {result.tasks_failed > 0 && (
            <span className="optimize-summary-stat" style={{ color: "var(--red)" }}>
              {result.tasks_failed} failed
            </span>
          )}
          {result.tasks_skipped > 0 && (
            <span className="optimize-summary-stat">
              {result.tasks_skipped} skipped
            </span>
          )}
        </div>
      )}
    </div>
  );
}
