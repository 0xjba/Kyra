import { create } from "zustand";
import {
  analyzePath,
  revealInFinder,
  listenAnalyzeProgress,
  type DirNode,
  type AnalyzeScanProgress,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";

type AnalyzePhase = "idle" | "scanning" | "ready";

interface AnalyzeStore {
  phase: AnalyzePhase;
  scanPath: string;
  root: DirNode | null;
  breadcrumb: DirNode[];
  current: DirNode | null;
  progress: AnalyzeScanProgress | null;
  viewMode: "sunburst" | "list";
  error: string | null;

  setScanPath: (path: string) => void;
  scan: () => Promise<void>;
  drillInto: (node: DirNode) => void;
  drillUp: () => void;
  drillToRoot: () => void;
  setViewMode: (mode: "sunburst" | "list") => void;
  reveal: (path: string) => void;
  reset: () => void;
}

export const useAnalyzeStore = create<AnalyzeStore>((set, get) => ({
  phase: "idle",
  scanPath: "/",
  root: null,
  breadcrumb: [],
  current: null,
  progress: null,
  viewMode: "sunburst",
  error: null,

  setScanPath: (path: string) => {
    set({ scanPath: path });
  },

  scan: async () => {
    const { scanPath } = get();
    set({ phase: "scanning", root: null, current: null, breadcrumb: [], progress: null, error: null });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenAnalyzeProgress((progress) => {
        set({ progress });
      });

      const root = await analyzePath(scanPath, 8);
      set({ phase: "ready", root, current: root });
    } catch (e) {
      set({ phase: "idle", error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  drillInto: (node: DirNode) => {
    if (!node.is_dir || node.children.length === 0) return;
    const { current, breadcrumb } = get();
    if (current) {
      set({ current: node, breadcrumb: [...breadcrumb, current] });
    }
  },

  drillUp: () => {
    const { breadcrumb } = get();
    if (breadcrumb.length === 0) return;
    const parent = breadcrumb[breadcrumb.length - 1];
    set({ current: parent, breadcrumb: breadcrumb.slice(0, -1) });
  },

  drillToRoot: () => {
    const { root } = get();
    if (root) {
      set({ current: root, breadcrumb: [] });
    }
  },

  setViewMode: (mode: "sunburst" | "list") => {
    set({ viewMode: mode });
  },

  reveal: (path: string) => {
    revealInFinder(path).catch(() => {});
  },

  reset: () => {
    set({
      phase: "idle",
      root: null,
      breadcrumb: [],
      current: null,
      progress: null,
      error: null,
    });
  },
}));
