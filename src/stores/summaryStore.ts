import { create } from "zustand";
import { useCleanStore } from "./cleanStore";
import { usePruneStore } from "./pruneStore";
import { useInstallersStore } from "./installersStore";
import { useOptimizeStore } from "./optimizeStore";

export interface ScanSummary {
  bytes: number;
  count: number;
  at: number;
}

export interface Summaries {
  clean?: ScanSummary;
  prune?: ScanSummary & { projects: number };
  installers?: ScanSummary;
  optimizedAt?: number;
  cleans: number;
  firstCleanAt?: number;
}

const KEY = "kyra_scan_summaries";

function load(): Summaries {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return { cleans: 0, ...JSON.parse(raw) };
  } catch {
    // Unreadable storage: start fresh
  }
  return { cleans: 0 };
}

export const useSummaryStore = create<Summaries>(() => load());

function update(patch: Partial<Summaries>) {
  useSummaryStore.setState(patch);
  try {
    localStorage.setItem(KEY, JSON.stringify(useSummaryStore.getState()));
  } catch {
    // Storage full or blocked: keep in-memory value only
  }
}

function recordCleanRun() {
  const s = useSummaryStore.getState();
  update({ cleans: s.cleans + 1, firstCleanAt: s.firstCleanAt ?? Date.now() });
}

let started = false;

/** Mirrors each module's latest scan/run into persisted summaries for the Home tiles. */
export function startSummaryTracking() {
  if (started) return;
  started = true;

  useCleanStore.subscribe((s, prev) => {
    if (s.phase === "done" && prev.phase !== "done") recordCleanRun();
    if (s.phase === "results" && (s.items !== prev.items || prev.phase !== "results")) {
      update({
        clean: {
          bytes: s.items.reduce((sum, i) => sum + i.total_size, 0),
          count: s.items.length,
          at: Date.now(),
        },
      });
    }
    if (s.phase === "idle" && prev.phase === "done") {
      update({ clean: { bytes: 0, count: 0, at: Date.now() } });
    }
  });

  usePruneStore.subscribe((s, prev) => {
    if (s.phase === "done" && prev.phase !== "done") recordCleanRun();
    if (s.phase === "list" && (s.artifacts !== prev.artifacts || prev.phase !== "list")) {
      update({
        prune: {
          bytes: s.artifacts.reduce((sum, a) => sum + a.size, 0),
          count: s.artifacts.length,
          projects: new Set(s.artifacts.map((a) => a.project_path)).size,
          at: Date.now(),
        },
      });
    }
    if (s.phase === "idle" && prev.phase === "done") {
      update({ prune: { bytes: 0, count: 0, projects: 0, at: Date.now() } });
    }
  });

  useInstallersStore.subscribe((s, prev) => {
    if (s.phase === "done" && prev.phase !== "done") recordCleanRun();
    if (s.phase === "list" && (s.files !== prev.files || prev.phase !== "list")) {
      update({
        installers: {
          bytes: s.files.reduce((sum, f) => sum + f.size, 0),
          count: s.files.length,
          at: Date.now(),
        },
      });
    }
    if (s.phase === "idle" && prev.phase === "done") {
      update({ installers: { bytes: 0, count: 0, at: Date.now() } });
    }
  });

  useOptimizeStore.subscribe((s, prev) => {
    if (prev.running && !s.running && s.result) update({ optimizedAt: Date.now() });
  });
}
