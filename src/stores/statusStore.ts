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
const MIN_TICK_GAP_MS = 600;

let generation = 0;
let lastTickAt = 0;

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
    const gen = ++generation;
    set({ streaming: true });

    const unlisten = await listenStatsTick((incoming) => {
      const now = Date.now();
      if (now - lastTickAt < MIN_TICK_GAP_MS) return;
      lastTickAt = now;
      set((state) => {
        const prev = state.stats;
        const stats =
          incoming.top_processes.length === 0 && prev && prev.top_processes.length > 0
            ? { ...incoming, top_processes: prev.top_processes }
            : incoming;
        const upload = isFinite(incoming.net_upload) ? incoming.net_upload : 0;
        const download = isFinite(incoming.net_download) ? incoming.net_download : 0;
        const history = [...state.networkHistory, { upload, download }];
        if (history.length > MAX_HISTORY) history.shift();
        return { stats, networkHistory: history };
      });
    });

    if (gen !== generation) {
      unlisten();
      return;
    }

    set({ unlisten });
    try {
      await startStatsStream();
    } catch (err) {
      console.error("[statusStore] startStatsStream failed:", err);
    }
  },

  stopStream: () => {
    generation++;
    const { unlisten } = get();
    if (unlisten) unlisten();
    stopStatsStream().catch(() => {});
    set({ streaming: false, unlisten: null });
  },
}));
