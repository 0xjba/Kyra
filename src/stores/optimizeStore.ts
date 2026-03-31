import { create } from "zustand";
import {
  getOptimizeTasks,
  runOptimizeTasks,
  listenOptimizeStatus,
  type OptTask,
  type OptTaskStatus,
  type OptResult,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";

type TaskStatusMap = Record<
  string,
  { status: "ready" | "running" | "done" | "error" | "skipped"; message?: string }
>;

interface OptimizeStore {
  tasks: OptTask[];
  enabledIds: Set<string>;
  statuses: TaskStatusMap;
  running: boolean;
  result: OptResult | null;
  error: string | null;

  loadTasks: () => Promise<void>;
  toggleTask: (id: string) => void;
  enableAll: () => void;
  disableAll: () => void;
  runSelected: () => Promise<void>;
  runSingle: (id: string) => Promise<void>;
  runTaskIds: (ids: string[]) => Promise<void>;
  markSkipped: (ids: string[], reason: string) => void;
  reset: () => void;
}

export const useOptimizeStore = create<OptimizeStore>((set, get) => ({
  tasks: [],
  enabledIds: new Set(),
  statuses: {},
  running: false,
  result: null,
  error: null,

  loadTasks: async () => {
    try {
      const tasks = await getOptimizeTasks();
      // Default: enable all non-admin tasks
      const enabledIds = new Set(
        tasks.filter((t) => !t.needs_admin).map((t) => t.id)
      );
      const statuses: TaskStatusMap = {};
      for (const t of tasks) {
        statuses[t.id] = { status: "ready" };
      }
      set({ tasks, enabledIds, statuses, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  toggleTask: (id: string) => {
    const { enabledIds } = get();
    const next = new Set(enabledIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    set({ enabledIds: next });
  },

  enableAll: () => {
    const ids = new Set(get().tasks.map((t) => t.id));
    set({ enabledIds: ids });
  },

  disableAll: () => {
    set({ enabledIds: new Set() });
  },

  runTaskIds: async (ids: string[]) => {
    if (ids.length === 0) return;

    // Reset statuses for these tasks to ready
    const newStatuses = { ...get().statuses };
    for (const id of ids) {
      newStatuses[id] = { status: "ready" };
    }
    set({ running: true, statuses: newStatuses });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenOptimizeStatus((status: OptTaskStatus) => {
        const finished = status.status === "done" || status.status === "error" || status.status === "skipped";
        set((state) => {
          const newEnabled = finished ? new Set(state.enabledIds) : state.enabledIds;
          if (finished) newEnabled.delete(status.task_id);
          return {
            statuses: {
              ...state.statuses,
              [status.task_id]: {
                status: status.status as "running" | "done" | "error" | "skipped",
                message: status.message ?? undefined,
              },
            },
            enabledIds: newEnabled,
          };
        });
      });

      const result = await runOptimizeTasks(ids);
      set({ running: false, result });
    } catch (e) {
      set({ running: false, error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  runSelected: async () => {
    const { enabledIds, statuses } = get();
    const taskIds = Array.from(enabledIds);
    if (taskIds.length === 0) return;

    const newStatuses = { ...statuses };
    for (const id of taskIds) {
      newStatuses[id] = { status: "ready" };
    }
    set({ running: true, result: null, statuses: newStatuses });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenOptimizeStatus((status: OptTaskStatus) => {
        const finished = status.status === "done" || status.status === "error" || status.status === "skipped";
        set((state) => {
          const newEnabled = finished ? new Set(state.enabledIds) : state.enabledIds;
          if (finished) newEnabled.delete(status.task_id);
          return {
            statuses: {
              ...state.statuses,
              [status.task_id]: {
                status: status.status as "running" | "done" | "error" | "skipped",
                message: status.message ?? undefined,
              },
            },
            enabledIds: newEnabled,
          };
        });
      });

      const result = await runOptimizeTasks(taskIds);
      set({ running: false, result });
    } catch (e) {
      set({ running: false, error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  runSingle: async (id: string) => {
    set((state) => ({
      running: true,
      result: null,
      statuses: { ...state.statuses, [id]: { status: "ready" } },
    }));

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenOptimizeStatus((status: OptTaskStatus) => {
        const finished = status.status === "done" || status.status === "error" || status.status === "skipped";
        set((state) => {
          const newEnabled = finished ? new Set(state.enabledIds) : state.enabledIds;
          if (finished) newEnabled.delete(status.task_id);
          return {
            statuses: {
              ...state.statuses,
              [status.task_id]: {
                status: status.status as "running" | "done" | "error" | "skipped",
                message: status.message ?? undefined,
              },
            },
            enabledIds: newEnabled,
          };
        });
      });

      const result = await runOptimizeTasks([id]);
      set({ running: false, result });
    } catch (e) {
      set({ running: false, error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  markSkipped: (ids: string[], reason: string) => {
    set((state) => {
      const newStatuses = { ...state.statuses };
      for (const id of ids) {
        newStatuses[id] = { status: "skipped", message: reason };
      }
      return { statuses: newStatuses };
    });
  },

  reset: () => {
    const { tasks } = get();
    const statuses: TaskStatusMap = {};
    for (const t of tasks) {
      statuses[t.id] = { status: "ready" };
    }
    set({ statuses, running: false, result: null, error: null });
  },
}));
