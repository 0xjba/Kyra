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
  setUseTrash: (useTrash: boolean) => Promise<void>;
  setLargeFileThreshold: (mb: number) => Promise<void>;
  setAnalyzeScanDepth: (depth: number) => Promise<void>;
  setLaunchAtLogin: (enabled: boolean) => Promise<void>;
  setCheckForUpdates: (enabled: boolean) => Promise<void>;
  setNotificationsEnabled: (enabled: boolean) => Promise<void>;
  setLowDiskThreshold: (gb: number) => Promise<void>;
  setOnboardingCompleted: (completed: boolean) => Promise<void>;
  addWhitelist: (path: string) => Promise<void>;
  removeWhitelist: (path: string) => Promise<void>;
}

const DEFAULT_SETTINGS: AppSettings = {
  dry_run: false,
  whitelist: [],
  use_trash: false,
  large_file_threshold_mb: 100,
  analyze_scan_depth: 8,
  launch_at_login: false,
  check_for_updates: true,
  notifications_enabled: true,
  low_disk_threshold_gb: 10,
  onboarding_completed: false,
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

  setLaunchAtLogin: async (enabled: boolean) => {
    const settings = { ...get().settings, launch_at_login: enabled };
    set({ settings });
    await saveSettings(settings);
  },

  setCheckForUpdates: async (enabled: boolean) => {
    const settings = { ...get().settings, check_for_updates: enabled };
    set({ settings });
    await saveSettings(settings);
  },

  setNotificationsEnabled: async (enabled: boolean) => {
    const settings = { ...get().settings, notifications_enabled: enabled };
    set({ settings });
    await saveSettings(settings);
  },

  setLowDiskThreshold: async (gb: number) => {
    const settings = { ...get().settings, low_disk_threshold_gb: gb };
    set({ settings });
    await saveSettings(settings);
  },

  setOnboardingCompleted: async (completed: boolean) => {
    const settings = { ...get().settings, onboarding_completed: completed };
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
