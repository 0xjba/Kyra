import { create } from "zustand";
import {
  scanForCleanables,
  executeClean,
  listenCleanProgress,
  type ScanItem,
  type CleanProgress,
  type CleanResult,
} from "../lib/tauri";
import { useSettingsStore } from "./settingsStore";
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
  dismissDone: () => void;
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

      const { dry_run: dryRun, use_trash } = useSettingsStore.getState().settings;
      const permanent = !use_trash;
      const selectedItems = get().items.filter((item) => ruleIds.includes(item.rule_id));
      const result = await executeClean(selectedItems, dryRun, permanent);
      set({ phase: "done", result });
    } catch (e) {
      set({ phase: "results", error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  dismissDone: () => {
    const { items, selectedIds } = get();
    // Remove cleaned items (the ones that were selected), keep the rest
    const cleanedIds = selectedIds;
    const remaining = items.filter((i) => !cleanedIds.has(i.rule_id));
    if (remaining.length === 0 || remaining.every((i) => i.total_size === 0)) {
      // Nothing left — go back to idle
      set({ phase: "idle", items: [], selectedIds: new Set(), progress: null, result: null });
    } else {
      // Go back to results with remaining items, re-select all
      const newSelected = new Set(remaining.map((i) => i.rule_id));
      set({ phase: "results", items: remaining, selectedIds: newSelected, progress: null, result: null });
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
