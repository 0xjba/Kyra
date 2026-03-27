import { create } from "zustand";
import {
  scanForCleanables,
  executeClean,
  listenCleanProgress,
  type ScanItem,
  type CleanProgress,
  type CleanResult,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";

type CleanPhase = "idle" | "scanning" | "results" | "cleaning" | "done";

interface CleanStore {
  phase: CleanPhase;
  items: ScanItem[];
  selectedIds: Set<string>;
  progress: CleanProgress | null;
  result: CleanResult | null;
  error: string | null;

  scan: () => Promise<void>;
  toggleItem: (ruleId: string) => void;
  selectAll: () => void;
  deselectAll: () => void;
  clean: () => Promise<void>;
  reset: () => void;
}

export const useCleanStore = create<CleanStore>((set, get) => ({
  phase: "idle",
  items: [],
  selectedIds: new Set(),
  progress: null,
  result: null,
  error: null,

  scan: async () => {
    set({ phase: "scanning", items: [], error: null });
    try {
      const items = await scanForCleanables();
      const allIds = new Set(items.map((item) => item.rule_id));
      set({ phase: "results", items, selectedIds: allIds });
    } catch (e) {
      set({ phase: "idle", error: String(e) });
    }
  },

  toggleItem: (ruleId: string) => {
    const { selectedIds } = get();
    const next = new Set(selectedIds);
    if (next.has(ruleId)) {
      next.delete(ruleId);
    } else {
      next.add(ruleId);
    }
    set({ selectedIds: next });
  },

  selectAll: () => {
    const allIds = new Set(get().items.map((item) => item.rule_id));
    set({ selectedIds: allIds });
  },

  deselectAll: () => {
    set({ selectedIds: new Set() });
  },

  clean: async () => {
    const { selectedIds } = get();
    const ruleIds = Array.from(selectedIds);
    if (ruleIds.length === 0) return;

    set({ phase: "cleaning", progress: null, result: null });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenCleanProgress((progress) => {
        set({ progress });
      });

      const result = await executeClean(ruleIds, false);
      set({ phase: "done", result });
    } catch (e) {
      set({ phase: "results", error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  reset: () => {
    set({
      phase: "idle",
      items: [],
      selectedIds: new Set(),
      progress: null,
      result: null,
      error: null,
    });
  },
}));
