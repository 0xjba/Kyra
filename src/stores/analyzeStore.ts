import { create } from "zustand";
import {
  analyzePath,
  revealInFinder,
  findLargeFiles,
  listenAnalyzeProgress,
  type DirNode,
  type AnalyzeScanProgress,
  type LargeFile,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useSettingsStore } from "./settingsStore";

type AnalyzePhase = "idle" | "scanning" | "ready";
type AnalyzeTab = "tree" | "large-files";

interface ScanCache {
  root: DirNode;
  timestamp: number;
}

interface AnalyzeStore {
  phase: AnalyzePhase;
  scanPath: string;
  root: DirNode | null;
  breadcrumb: DirNode[];
  current: DirNode | null;
  progress: AnalyzeScanProgress | null;
  viewMode: "treemap" | "list";
  activeTab: AnalyzeTab;
  error: string | null;
  scanCache: Record<string, ScanCache>;

  // Large files
  largeFiles: LargeFile[];
  largeFilesLoading: boolean;
  largeFilesKey: string | null;
  largeFilesCapped: boolean;

  setScanPath: (path: string) => void;
  scan: () => Promise<void>;
  drillInto: (node: DirNode) => void;
  drillUp: () => void;
  drillToIndex: (index: number) => void;
  drillToRoot: () => void;
  setViewMode: (mode: "treemap" | "list") => void;
  setActiveTab: (tab: AnalyzeTab) => void;
  reveal: (path: string) => void;
  reset: () => void;
  removeNodeByPath: (path: string, freedSize: number) => void;
  loadLargeFiles: () => Promise<void>;
  ensureLargeFiles: () => Promise<void>;
  removeLargeFile: (path: string) => void;
}

const LARGE_FILES_CAP = 50;

function largeFilesKeyFor(path: string, thresholdMb: number): string {
  return `${path}|${thresholdMb}`;
}

function currentThresholdMb(): number {
  return useSettingsStore.getState().settings.large_file_threshold_mb || 100;
}

let largeFilesRequest = 0;
let pendingLargeFilesKey: string | null = null;

function removeChildByPath(node: DirNode, targetPath: string, freedSize: number): DirNode {
  return {
    ...node,
    size: node.size - freedSize,
    children: node.children
      .filter((c) => c.path !== targetPath)
      .map((c) => {
        if (targetPath.startsWith(c.path + "/")) {
          return removeChildByPath(c, targetPath, freedSize);
        }
        return c;
      }),
  };
}

const CACHE_MAX_AGE = 5 * 60 * 1000; // 5 minutes

export const useAnalyzeStore = create<AnalyzeStore>((set, get) => ({
  phase: "idle",
  scanPath: "/",
  root: null,
  breadcrumb: [],
  current: null,
  progress: null,
  viewMode: "treemap",
  activeTab: "tree",
  error: null,
  scanCache: {},
  largeFiles: [],
  largeFilesLoading: false,
  largeFilesKey: null,
  largeFilesCapped: false,

  setScanPath: (path: string) => {
    set({ scanPath: path });
  },

  scan: async () => {
    const { scanPath, scanCache } = get();

    // Check cache
    const cached = scanCache[scanPath];
    if (cached && Date.now() - cached.timestamp < CACHE_MAX_AGE) {
      set({ phase: "ready", root: cached.root, current: cached.root, breadcrumb: [], error: null });
      return;
    }

    set({ phase: "scanning", root: null, current: null, breadcrumb: [], progress: null, error: null });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenAnalyzeProgress((progress) => {
        set({ progress });
      });

      // Use scan depth from settings, fallback to 8
      const depth = useSettingsStore.getState().settings.analyze_scan_depth || 8;
      const root = await analyzePath(scanPath, depth);
      set({
        phase: "ready",
        root,
        current: root,
        scanCache: {
          ...get().scanCache,
          [scanPath]: { root, timestamp: Date.now() },
        },
      });
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

  drillToIndex: (index: number) => {
    const { breadcrumb } = get();
    if (index >= breadcrumb.length) return;
    set({ current: breadcrumb[index], breadcrumb: breadcrumb.slice(0, index) });
  },

  drillToRoot: () => {
    const { root } = get();
    if (root) {
      set({ current: root, breadcrumb: [] });
    }
  },

  setViewMode: (mode: "treemap" | "list") => {
    set({ viewMode: mode });
  },

  setActiveTab: (tab: AnalyzeTab) => {
    set({ activeTab: tab });
    if (tab === "large-files") get().ensureLargeFiles();
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
      activeTab: "tree",
    });
  },

  removeNodeByPath: (path: string, freedSize: number) => {
    const { root, current, breadcrumb, scanPath, scanCache } = get();
    if (!root) return;

    const newRoot = removeChildByPath(root, path, freedSize);

    // Rebuild breadcrumb + current from newRoot
    let nav: DirNode = newRoot;
    const newBreadcrumb: DirNode[] = [];
    for (const crumb of breadcrumb) {
      newBreadcrumb.push(nav);
      const next = nav.children.find((c) => c.path === crumb.path);
      if (!next) break;
      nav = next;
    }
    // Find current in the rebuilt tree
    let newCurrent = nav;
    if (current && current.path !== nav.path) {
      const found = nav.children.find((c) => c.path === current.path);
      if (found) {
        newBreadcrumb.push(nav);
        newCurrent = found;
      }
    }

    // Invalidate cache for this scan path
    const newCache = { ...scanCache };
    delete newCache[scanPath];

    set({ root: newRoot, current: newCurrent, breadcrumb: newBreadcrumb, scanCache: newCache });
  },

  loadLargeFiles: async () => {
    const request = ++largeFilesRequest;
    const scanPath = get().scanPath;
    const threshold = currentThresholdMb();
    pendingLargeFilesKey = largeFilesKeyFor(scanPath, threshold);
    set({ largeFilesLoading: true });
    try {
      const files = await findLargeFiles(threshold, scanPath || undefined);
      if (request !== largeFilesRequest) return;
      set({
        largeFiles: [...files].sort((a, b) => b.size - a.size),
        largeFilesLoading: false,
        largeFilesKey: largeFilesKeyFor(scanPath, threshold),
        largeFilesCapped: files.length >= LARGE_FILES_CAP,
      });
    } catch {
      if (request !== largeFilesRequest) return;
      set({ largeFiles: [], largeFilesLoading: false, largeFilesKey: null, largeFilesCapped: false });
    }
  },

  ensureLargeFiles: async () => {
    const threshold = currentThresholdMb();
    const { scanPath, largeFilesKey, largeFilesLoading } = get();
    const key = largeFilesKeyFor(scanPath, threshold);
    if (key === (largeFilesLoading ? pendingLargeFilesKey : largeFilesKey)) return;
    await get().loadLargeFiles();
  },

  removeLargeFile: (path: string) => {
    set({
      largeFiles: get().largeFiles.filter((f) => f.path !== path && !f.path.startsWith(path + "/")),
    });
  },
}));
