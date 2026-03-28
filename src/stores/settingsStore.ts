import { create } from "zustand";
import {
  loadSettings,
  saveSettings,
  type AppSettings,
} from "../lib/tauri";

interface SettingsStore {
  settings: AppSettings;
  loaded: boolean;

  load: () => Promise<void>;
  setDryRun: (dryRun: boolean) => Promise<void>;
}

const DEFAULT_SETTINGS: AppSettings = {
  dry_run: false,
};

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  loaded: false,

  load: async () => {
    try {
      const settings = await loadSettings();
      set({ settings, loaded: true });
    } catch {
      set({ loaded: true });
    }
  },

  setDryRun: async (dryRun: boolean) => {
    const settings = { ...get().settings, dry_run: dryRun };
    set({ settings });
    await saveSettings(settings);
  },
}));
