import { create } from "zustand";
import {
  scanInstalledApps,
  getAssociatedFiles,
  executeUninstall,
  listenUninstallProgress,
  addBytesFreed,
  type AppInfo,
  type AssociatedFile,
  type UninstallProgress,
  type UninstallResult,
} from "../lib/tauri";
import { useSettingsStore } from "./settingsStore";
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
  uninstall: (permanent: boolean) => Promise<void>;
  bulkUninstall: (apps: AppInfo[], permanent: boolean) => Promise<void>;
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

  uninstall: async (permanent: boolean) => {
    const { selectedApp, selectedFilePaths } = get();
    if (!selectedApp) return;

    set({ phase: "removing", progress: null, result: null });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenUninstallProgress((progress) => {
        set({ progress });
      });

      const filePaths = Array.from(selectedFilePaths);
      const dryRun = useSettingsStore.getState().settings.dry_run;
      const result = await executeUninstall(selectedApp.path, filePaths, selectedApp.bundle_id, selectedApp.brew_cask, dryRun, permanent);
      if (!dryRun && result.bytes_freed > 0) {
        addBytesFreed(result.bytes_freed).catch(() => {});
      }
      // Only remove the app from local list if its path was actually deleted
      const appWasDeleted = result.deleted_paths.includes(selectedApp.path);
      const remainingApps = appWasDeleted
        ? get().apps.filter((a) => a.path !== selectedApp.path)
        : get().apps;
      set({ phase: "done", result, apps: remainingApps, selectedApp: appWasDeleted ? null : selectedApp, associatedFiles: appWasDeleted ? [] : get().associatedFiles, selectedFilePaths: appWasDeleted ? new Set() : get().selectedFilePaths });
      // Re-scan in background to refresh app list
      scanInstalledApps().then((apps) => set({ apps })).catch(() => {});
    } catch (e) {
      set({ phase: "list", error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  bulkUninstall: async (appsToRemove: AppInfo[], permanent: boolean) => {
    if (appsToRemove.length === 0) return;

    set({ phase: "removing", progress: null, result: null, selectedApp: null });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenUninstallProgress((progress) => {
        set({ progress });
      });

      const dryRun = useSettingsStore.getState().settings.dry_run;
      let totalRemoved = 0;
      let totalFreed = 0;
      const allErrors: string[] = [];
      const allDeletedPaths: string[] = [];

      for (const app of appsToRemove) {
        // Fetch associated files for each app
        let filePaths: string[] = [];
        try {
          const files = await getAssociatedFiles(app.bundle_id, app.name, app.path);
          filePaths = files.map((f) => f.path);
        } catch {
          // Continue even if we can't get associated files
        }

        try {
          const result = await executeUninstall(app.path, filePaths, app.bundle_id, app.brew_cask, dryRun, permanent);
          totalRemoved += result.items_removed;
          totalFreed += result.bytes_freed;
          allDeletedPaths.push(...result.deleted_paths);
          allErrors.push(...result.errors);
          if (allErrors.length > 50) allErrors.length = 50;
        } catch (e) {
          allErrors.push(`${app.name}: ${String(e)}`);
        }
      }

      // Only remove apps whose path was actually deleted
      const successPaths = new Set(allDeletedPaths);
      const remainingApps = get().apps.filter((a) => !successPaths.has(a.path));
      if (!dryRun && totalFreed > 0) {
        addBytesFreed(totalFreed).catch(() => {});
      }
      set({
        phase: "done",
        result: {
          items_removed: totalRemoved,
          bytes_freed: totalFreed,
          errors: allErrors,
          deleted_paths: allDeletedPaths,
        },
        apps: remainingApps,
        selectedApp: null,
        associatedFiles: [],
        selectedFilePaths: new Set(),
      });
      // Re-scan in background to refresh app list
      scanInstalledApps().then((apps) => set({ apps })).catch(() => {});
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
