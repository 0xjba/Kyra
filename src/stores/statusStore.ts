import { create } from "zustand";
import {
  startStatsStream,
  stopStatsStream,
  listenStatsTick,
  type DetailedStats,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";

interface NetworkPoint {
  upload: number;
  download: number;
}

const MAX_HISTORY = 60;

interface StatusStore {
  stats: DetailedStats | null;
  networkHistory: NetworkPoint[];
  streaming: boolean;
  unlisten: UnlistenFn | null;

  startStream: () => Promise<void>;
  stopStream: () => void;
}

export const useStatusStore = create<StatusStore>((set, get) => ({
  stats: null,
  networkHistory: [],
  streaming: false,
  unlisten: null,

  startStream: async () => {
    if (get().streaming) return;

    const unlisten = await listenStatsTick((stats) => {
      set((state) => {
        const upload = isFinite(stats.net_upload) ? stats.net_upload : 0;
        const download = isFinite(stats.net_download) ? stats.net_download : 0;
        const point: NetworkPoint = { upload, download };
        const history = [...state.networkHistory, point];
        if (history.length > MAX_HISTORY) {
          history.shift();
        }
        return { stats, networkHistory: history };
      });
    });

    set({ streaming: true, unlisten });
    try {
      await startStatsStream();
    } catch (err) {
      console.error("[statusStore] startStatsStream failed:", err);
    }
  },

  stopStream: () => {
    const { unlisten } = get();
    if (unlisten) unlisten();
    // Stop the Rust background thread too
    stopStatsStream().catch(() => {});
    set({ streaming: false, unlisten: null });
  },
}));
