import { create } from "zustand";
import {
  scanInstallers,
  deleteInstallers,
  listenInstallerProgress,
  addBytesFreed,
  type InstallerFile,
  type InstallerProgress,
  type InstallerResult,
} from "../lib/tauri";
import { useSettingsStore } from "./settingsStore";

type InstallersPhase = "idle" | "scanning" | "list" | "deleting" | "done";

interface InstallersStore {
  phase: InstallersPhase;
  files: InstallerFile[];
  selected: Set<string>;
  progress: InstallerProgress | null;
  result: InstallerResult | null;
  error: string | null;

  scan: () => Promise<void>;
  toggleSelect: (path: string) => void;
  selectAll: () => void;
  deselectAll: () => void;
  deleteSelected: () => Promise<void>;
  dismissDone: () => void;
  reset: () => void;
}

export const useInstallersStore = create<InstallersStore>((set, get) => ({
  phase: "idle",
  files: [],
  selected: new Set(),
  progress: null,
  result: null,
  error: null,

  scan: async () => {
    set({ phase: "scanning", error: null });
    try {
      const files = await scanInstallers();
      const allPaths = new Set(files.map((f) => f.path));
      set({ phase: "list", files, selected: allPaths });
    } catch (e) {
      set({ phase: "idle", error: String(e) });
    }
  },

  toggleSelect: (path: string) => {
    const selected = new Set(get().selected);
    if (selected.has(path)) {
      selected.delete(path);
    } else {
      selected.add(path);
    }
    set({ selected });
  },

  selectAll: () => {
    set({ selected: new Set(get().files.map((f) => f.path)) });
  },

  deselectAll: () => {
    set({ selected: new Set() });
  },

  deleteSelected: async () => {
    const { selected } = get();
    if (selected.size === 0) return;

    set({ phase: "deleting", progress: null });

    const unlisten = await listenInstallerProgress((progress) => {
      set({ progress });
    });

    try {
      const { dry_run: dryRun, use_trash } = useSettingsStore.getState().settings;
      const permanent = !use_trash;
      const result = await deleteInstallers([...selected], dryRun, permanent);
      if (!dryRun && result.bytes_freed > 0) {
        addBytesFreed(result.bytes_freed).catch(() => {});
      }
      set({ phase: "done", result });
    } catch (e) {
      set({ phase: "list", error: String(e) });
    } finally {
      unlisten();
    }
  },

  dismissDone: () => {
    const { files, result } = get();
    const deletedSet = new Set(result?.deleted_paths ?? []);
    const remaining = files.filter((f) => !deletedSet.has(f.path));
    if (remaining.length === 0) {
      set({ phase: "idle", files: [], selected: new Set(), progress: null, result: null });
    } else {
      const newSelected = new Set(remaining.map((f) => f.path));
      set({ phase: "list", files: remaining, selected: newSelected, progress: null, result: null });
    }
  },

  reset: () => {
    set({
      phase: "idle",
      files: [],
      selected: new Set(),
      progress: null,
      result: null,
      error: null,
    });
  },
}));
