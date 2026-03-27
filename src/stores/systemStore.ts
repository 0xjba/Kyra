import { create } from "zustand";
import { getSystemStats, type SystemStats } from "../lib/tauri";

interface SystemStore {
  stats: SystemStats | null;
  loading: boolean;
  fetchStats: () => Promise<void>;
}

export const useSystemStore = create<SystemStore>((set) => ({
  stats: null,
  loading: false,
  fetchStats: async () => {
    set({ loading: true });
    try {
      const stats = await getSystemStats();
      set({ stats, loading: false });
    } catch {
      set({ loading: false });
    }
  },
}));
