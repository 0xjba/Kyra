import { useEffect, useState, useRef, useMemo, useCallback } from "react";
import { useOptimizeStore } from "../stores/optimizeStore";
import { LockKeyhole, Copy, Check, Sparkles } from "lucide-react";
import type { OptTask } from "../lib/tauri";
import { askAiOptimize } from "../utils/askAi";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import "../styles/optimize.css";

type Kind = "safe" | "restart" | "admin";

const CAT_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];

const KIND_META: Record<Kind, { color: string; label: string }> = {
  safe: { color: "#2AC852", label: "Safe to run" },
  restart: { color: "#FDB022", label: "Needs restart" },
  admin: { color: "#FD4841", label: "Admin required" },
};

const GROUPS: { name: string; ids: string[] }[] = [
  { name: "Network", ids: ["dns_flush", "network_flush", "bluetooth_reset", "prevent_network_dsstore"] },
  { name: "Caches", ids: ["cache_refresh", "icon_cache", "dock_refresh", "saved_state", "font_cache"] },
  { name: "Databases & Indexes", ids: ["sqlite_vacuum", "launch_services", "spotlight_rebuild"] },
  {
    name: "System",
    ids: ["memory_purge", "periodic_maintenance", "disk_verify", "disk_permissions", "plist_repair", "shared_file_list_repair"],
  },
  {
    name: "Security & Privacy",
    ids: ["firewall_enable", "quarantine_cleanup", "login_items_audit", "launch_agents_cleanup", "notification_cleanup", "coreduet_cleanup"],
  },
];

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
  quarantine_cleanup: "Gatekeeper quarantine records cleared",
  prevent_network_dsstore: "Network .DS_Store creation disabled",
  launch_agents_cleanup: "Stale Launch Agents removed",
  periodic_maintenance: "Periodic maintenance scripts executed",
  shared_file_list_repair: "Shared file lists repaired",
  notification_cleanup: "Old notification records cleaned",
  disk_verify: "Disk filesystem verified OK",
  coreduet_cleanup: "Usage history records trimmed",
  login_items_audit: "Login items audited",
};

function kindOf(task: OptTask): Kind {
  if (task.needs_admin) return "admin";
  if (task.warning) return "restart";
  return "safe";
}

const RING_LEN = 364.4;

function TaskRow({
  task,
  status,
  first,
  globalRunning,
  onRun,
}: {
  task: OptTask;
  status: { status: string; message?: string };
  first: boolean;
  globalRunning: boolean;
  onRun: () => void;
}) {
  const actualStatus = status.status;
  const [displayStatus, setDisplayStatus] = useState(actualStatus);
  const runStartRef = useRef<number>(0);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (actualStatus === "running") {
      runStartRef.current = Date.now();
      setDisplayStatus("running");
    } else if (actualStatus === "done" || actualStatus === "error" || actualStatus === "skipped") {
      const remaining = Math.max(0, 900 - (Date.now() - runStartRef.current));
      if (remaining > 0) {
        const timer = setTimeout(() => setDisplayStatus(actualStatus), remaining);
        return () => clearTimeout(timer);
      }
      setDisplayStatus(actualStatus);
    } else {
      setDisplayStatus(actualStatus);
    }
  }, [actualStatus]);

  const kind = kindOf(task);
  const isRunning = displayStatus === "running";
  const isDone = displayStatus === "done";
  const isError = displayStatus === "error";
  const isSkipped = displayStatus === "skipped";
  const isFinished = isDone || isError || isSkipped;

  const btnState = isDone ? " done" : isError ? " failed" : isSkipped ? " skipped" : isRunning ? " running" : "";
  const btnLabel = isDone
    ? "Done"
    : isError
      ? "Failed"
      : isSkipped
        ? "Skipped"
        : isRunning
          ? "Running…"
          : kind === "admin"
            ? "Run…"
            : "Run";

  return (
    <div className={`opt-task${first ? "" : " divided"}`}>
      <div className="opt-task-prog" style={{ width: isRunning ? "100%" : "0%" }} />
      <div className="opt-task-info">
        <div className="opt-task-name-row">
          <span className="opt-task-name">{task.name}</span>
          <span
            className="opt-task-dot"
            style={{ background: KIND_META[kind].color }}
            title={task.warning ? `${KIND_META[kind].label}: ${task.warning}` : KIND_META[kind].label}
          />
        </div>
        <div
          className={`opt-task-desc${isError ? " error" : isSkipped ? " skipped" : ""}`}
          title={isError && status.message ? status.message : undefined}
        >
          {isDone ? (
            TASK_RESULTS[task.id] || status.message?.trim() || task.description
          ) : isError && status.message ? (
            <>
              <span className="opt-task-error-text">{status.message}</span>
              <button
                className="opt-task-copy"
                onClick={(e) => {
                  e.stopPropagation();
                  navigator.clipboard.writeText(status.message!);
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1500);
                }}
              >
                {copied ? <Check size={11} /> : <Copy size={11} />}
              </button>
            </>
          ) : isSkipped && status.message ? (
            status.message
          ) : (
            task.description
          )}
        </div>
      </div>
      <button
        className="opt-task-ai"
        title="Ask AI about this task"
        onClick={() => askAiOptimize([task], new Set([task.id]))}
      >
        <Sparkles size={13} strokeWidth={1.8} />
      </button>
      <button
        className={`opt-task-btn${btnState}`}
        onClick={onRun}
        disabled={globalRunning && !isRunning && !isFinished}
      >
        {isDone && <Check size={11} strokeWidth={3} />}
        {btnLabel}
      </button>
    </div>
  );
}

export default function Optimize() {
  const tasks = useOptimizeStore((s) => s.tasks);
  const statuses = useOptimizeStore((s) => s.statuses);
  const running = useOptimizeStore((s) => s.running);
  const error = useOptimizeStore((s) => s.error);
  const loadTasks = useOptimizeStore((s) => s.loadTasks);
  const runSingle = useOptimizeStore((s) => s.runSingle);
  const runTaskIds = useOptimizeStore((s) => s.runTaskIds);

  const [adminTask, setAdminTask] = useState<OptTask | null>(null);
  const [frame, setFrame] = useState(0);

  useEffect(() => {
    if (tasks.length === 0) loadTasks();
  }, [tasks.length, loadTasks]);

  useEffect(() => {
    const t = setInterval(() => setFrame((f) => (f + 1) % CAT_FRAMES.length), 220);
    return () => clearInterval(t);
  }, []);

  const groups = useMemo(() => {
    const byId = new Map(tasks.map((t) => [t.id, t]));
    const used = new Set<string>();
    const out: { name: string; tasks: OptTask[] }[] = [];
    for (const g of GROUPS) {
      const list = g.ids.map((id) => byId.get(id)).filter((t): t is OptTask => !!t);
      list.forEach((t) => used.add(t.id));
      if (list.length) out.push({ name: g.name, tasks: list });
    }
    const rest = tasks.filter((t) => !used.has(t.id));
    if (rest.length) out.push({ name: "Other", tasks: rest });
    return out;
  }, [tasks]);

  const statusOf = useCallback(
    (id: string) => statuses[id]?.status ?? "ready",
    [statuses],
  );

  const total = tasks.length;
  const doneCount = tasks.filter((t) => statusOf(t.id) === "done").length;
  const safeLeftIds = tasks
    .filter((t) => kindOf(t) === "safe" && statusOf(t.id) !== "done")
    .map((t) => t.id);
  const safeLeft = safeLeftIds.length;

  const legend = (Object.keys(KIND_META) as Kind[]).map((k) => ({
    ...KIND_META[k],
    n: tasks.filter((t) => kindOf(t) === k).length,
  }));

  const heroTitle = total > 0 && doneCount === total ? "Fully tuned" : running ? "Tuning up…" : "Tune-up";
  const heroSub = running
    ? "Kyra is working through the list. You can keep using your Mac."
    : safeLeft
      ? `${safeLeft} task${safeLeft !== 1 ? "s" : ""} can run right now without a password or restart.`
      : total > 0
        ? "Safe tasks are done. The rest need a restart or your password."
        : "Loading tasks…";
  const allLabel = running
    ? "Running…"
    : safeLeft
      ? `Run ${safeLeft} safe task${safeLeft !== 1 ? "s" : ""}`
      : "All safe tasks done";

  const ringOff = RING_LEN * (1 - (total ? doneCount / total : 0));

  const handleRunAll = () => {
    if (running || safeLeft === 0) return;
    runTaskIds(safeLeftIds);
  };

  const handleRun = (task: OptTask) => {
    const st = statusOf(task.id);
    if (running || st === "running" || st === "done" || st === "error" || st === "skipped") return;
    if (task.needs_admin) setAdminTask(task);
    else runSingle(task.id);
  };

  const handleAdminContinue = () => {
    const t = adminTask;
    setAdminTask(null);
    if (t) runSingle(t.id);
  };

  return (
    <div className="opt-layout">
      <div className="opt-panel">
        <div className="opt-ring">
          <svg width="132" height="132" viewBox="0 0 132 132" className="opt-ring-svg">
            <circle cx="66" cy="66" r="58" fill="none" className="opt-ring-track" strokeWidth="9" />
            <circle
              cx="66"
              cy="66"
              r="58"
              fill="none"
              stroke="url(#optg)"
              strokeWidth="9"
              strokeLinecap="round"
              strokeDasharray={RING_LEN}
              strokeDashoffset={ringOff}
              className="opt-ring-fill"
            />
            <defs>
              <linearGradient id="optg" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0" stopColor="#22B8F0" />
                <stop offset="1" stopColor="#2AC852" />
              </linearGradient>
            </defs>
          </svg>
          <div className="opt-ring-center">
            <span className="opt-ring-num">{doneCount}</span>
            <span className="opt-ring-sub">of {total} done</span>
          </div>
        </div>
        <div className="opt-hero-title">{heroTitle}</div>
        <div className="opt-hero-sub">{heroSub}</div>
        <button className="opt-run-all" onClick={handleRunAll} disabled={running || safeLeft === 0}>
          {allLabel}
        </button>
        <div className="opt-legend">
          {legend.map((l) => (
            <div key={l.label} className="opt-legend-row">
              <span className="opt-legend-dot" style={{ background: l.color }} />
              <span className="opt-legend-label">{l.label}</span>
              <span className="opt-legend-n">{l.n}</span>
            </div>
          ))}
        </div>
        <img src={CAT_FRAMES[frame]} className="opt-cat" alt="" draggable={false} />
      </div>

      <div className="opt-list">
        {error && <div className="opt-error">{error}</div>}
        {groups.map((g) => (
          <div key={g.name} className="opt-group">
            <div className="opt-group-name">{g.name}</div>
            <div className="opt-group-card">
              {g.tasks.map((task, i) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  first={i === 0}
                  status={statuses[task.id] || { status: "ready" }}
                  globalRunning={running}
                  onRun={() => handleRun(task)}
                />
              ))}
            </div>
          </div>
        ))}
      </div>

      {adminTask && (
        <div className="opt-admin-overlay" onClick={() => setAdminTask(null)}>
          <div className="opt-admin-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="opt-admin-icon">
              <LockKeyhole size={22} strokeWidth={2} />
            </div>
            <div className="opt-admin-title">Admin access required</div>
            <div className="opt-admin-text">
              {adminTask.name} needs your administrator password. macOS will ask for it next.
              {adminTask.warning ? ` ${adminTask.warning}.` : ""}
            </div>
            <div className="opt-admin-btns">
              <button className="opt-admin-btn cancel" onClick={() => setAdminTask(null)}>
                Cancel
              </button>
              <button className="opt-admin-btn continue" onClick={handleAdminContinue}>
                Continue
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
