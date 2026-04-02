import { create } from "zustand";
import {
  loadSettings,
  saveSettings,
  addToWhitelist as addToWhitelistApi,
  removeFromWhitelist as removeFromWhitelistApi,
  type AppSettings,
} from "../lib/tauri";

interface SettingsStore {
  settings: AppSettings;
  loaded: boolean;

  load: () => Promise<void>;
  setDryRun: (dryRun: boolean) => Promise<void>;
  setUseTrash: (useTrash: boolean) => Promise<void>;
  setLargeFileThreshold: (mb: number) => Promise<void>;
  setAnalyzeScanDepth: (depth: number) => Promise<void>;
  addWhitelist: (path: string) => Promise<void>;
  removeWhitelist: (path: string) => Promise<void>;
}

const DEFAULT_SETTINGS: AppSettings = {
  dry_run: false,
  whitelist: [],
  use_trash: false,
  large_file_threshold_mb: 100,
  analyze_scan_depth: 8,
};

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  loaded: false,

  load: async () => {
    try {
      const settings = await loadSettings();
      set({ settings: { ...DEFAULT_SETTINGS, ...settings }, loaded: true });
    } catch {
      set({ loaded: true });
    }
  },

  setDryRun: async (dryRun: boolean) => {
    const settings = { ...get().settings, dry_run: dryRun };
    set({ settings });
    await saveSettings(settings);
  },

  setUseTrash: async (useTrash: boolean) => {
    const settings = { ...get().settings, use_trash: useTrash };
    set({ settings });
    await saveSettings(settings);
  },

  setLargeFileThreshold: async (mb: number) => {
    const settings = { ...get().settings, large_file_threshold_mb: mb };
    set({ settings });
    await saveSettings(settings);
  },

  setAnalyzeScanDepth: async (depth: number) => {
    const settings = { ...get().settings, analyze_scan_depth: depth };
    set({ settings });
    await saveSettings(settings);
  },

  addWhitelist: async (path: string) => {
    await addToWhitelistApi(path);
    const settings = {
      ...get().settings,
      whitelist: [...get().settings.whitelist, path],
    };
    set({ settings });
  },

  removeWhitelist: async (path: string) => {
    await removeFromWhitelistApi(path);
    const settings = {
      ...get().settings,
      whitelist: get().settings.whitelist.filter((p) => p !== path),
    };
    set({ settings });
  },
}));
