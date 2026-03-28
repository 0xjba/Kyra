import { create } from "zustand";
import {
  scanInstallers,
  deleteInstallers,
  listenInstallerProgress,
  type InstallerFile,
  type InstallerProgress,
  type InstallerResult,
} from "../lib/tauri";

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
      const result = await deleteInstallers([...selected], false);
      set({ phase: "done", result });
    } catch (e) {
      set({ phase: "list", error: String(e) });
    } finally {
      unlisten();
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
