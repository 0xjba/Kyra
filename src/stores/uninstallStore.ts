import { create } from "zustand";
import {
  scanInstalledApps,
  getAssociatedFiles,
  executeUninstall,
  listenUninstallProgress,
  type AppInfo,
  type AssociatedFile,
  type UninstallProgress,
  type UninstallResult,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";

type UninstallPhase = "idle" | "scanning" | "list" | "removing" | "done";

interface UninstallStore {
  phase: UninstallPhase;
  apps: AppInfo[];
  search: string;
  selectedApp: AppInfo | null;
  associatedFiles: AssociatedFile[];
  loadingFiles: boolean;
  selectedFilePaths: Set<string>;
  progress: UninstallProgress | null;
  result: UninstallResult | null;
  error: string | null;

  scanApps: () => Promise<void>;
  setSearch: (query: string) => void;
  selectApp: (app: AppInfo) => Promise<void>;
  deselectApp: () => void;
  toggleFile: (path: string) => void;
  selectAllFiles: () => void;
  deselectAllFiles: () => void;
  uninstall: () => Promise<void>;
  reset: () => void;
}

export const useUninstallStore = create<UninstallStore>((set, get) => ({
  phase: "idle",
  apps: [],
  search: "",
  selectedApp: null,
  associatedFiles: [],
  loadingFiles: false,
  selectedFilePaths: new Set(),
  progress: null,
  result: null,
  error: null,

  scanApps: async () => {
    set({ phase: "scanning", apps: [], error: null });
    try {
      const apps = await scanInstalledApps();
      set({ phase: "list", apps });
    } catch (e) {
      set({ phase: "idle", error: String(e) });
    }
  },

  setSearch: (query: string) => {
    set({ search: query });
  },

  selectApp: async (app: AppInfo) => {
    set({ selectedApp: app, associatedFiles: [], loadingFiles: true, selectedFilePaths: new Set(), error: null });
    try {
      const files = await getAssociatedFiles(app.bundle_id, app.name, app.path);
      const allPaths = new Set(files.map((f) => f.path));
      set({ associatedFiles: files, loadingFiles: false, selectedFilePaths: allPaths });
    } catch (e) {
      set({ loadingFiles: false, error: String(e) });
    }
  },

  deselectApp: () => {
    set({ selectedApp: null, associatedFiles: [], selectedFilePaths: new Set(), error: null });
  },

  toggleFile: (path: string) => {
    const { selectedFilePaths } = get();
    const next = new Set(selectedFilePaths);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    set({ selectedFilePaths: next });
  },

  selectAllFiles: () => {
    const allPaths = new Set(get().associatedFiles.map((f) => f.path));
    set({ selectedFilePaths: allPaths });
  },

  deselectAllFiles: () => {
    set({ selectedFilePaths: new Set() });
  },

  uninstall: async () => {
    const { selectedApp, selectedFilePaths } = get();
    if (!selectedApp) return;

    set({ phase: "removing", progress: null, result: null });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenUninstallProgress((progress) => {
        set({ progress });
      });

      const filePaths = Array.from(selectedFilePaths);
      const result = await executeUninstall(selectedApp.path, filePaths, false);
      set({ phase: "done", result });
    } catch (e) {
      set({ phase: "list", error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  reset: () => {
    set({
      phase: "idle",
      apps: [],
      search: "",
      selectedApp: null,
      associatedFiles: [],
      loadingFiles: false,
      selectedFilePaths: new Set(),
      progress: null,
      result: null,
      error: null,
    });
  },
}));
