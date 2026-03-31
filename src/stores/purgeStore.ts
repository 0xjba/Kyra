import { create } from "zustand";
import {
  scanArtifacts,
  executePurge,
  listenPurgeProgress,
  type ArtifactEntry,
  type PurgeProgress,
  type PurgeResult,
} from "../lib/tauri";
import { useSettingsStore } from "./settingsStore";

type PurgePhase = "idle" | "scanning" | "list" | "purging" | "done";

interface PurgeStore {
  phase: PurgePhase;
  rootPath: string;
  artifacts: ArtifactEntry[];
  selectedPaths: Set<string>;
  progress: PurgeProgress | null;
  result: PurgeResult | null;
  error: string | null;

  setRootPath: (path: string) => void;
  scan: () => Promise<void>;
  toggleSelect: (artifactPath: string) => void;
  selectAll: () => void;
  deselectAll: () => void;
  purge: () => Promise<void>;
  dismissDone: () => void;
  reset: () => void;
}

function defaultRootPath(): string {
  return "~/Projects";
}

export const usePurgeStore = create<PurgeStore>((set, get) => ({
  phase: "idle",
  rootPath: defaultRootPath(),
  artifacts: [],
  selectedPaths: new Set(),
  progress: null,
  result: null,
  error: null,

  setRootPath: (path: string) => set({ rootPath: path }),

  scan: async () => {
    set({ phase: "scanning", artifacts: [], selectedPaths: new Set(), error: null });
    try {
      const artifacts = await scanArtifacts(get().rootPath);
      const allPaths = new Set(artifacts.map((a) => a.artifact_path));
      set({ phase: "list", artifacts, selectedPaths: allPaths });
    } catch (e) {
      set({ phase: "idle", error: String(e) });
    }
  },

  toggleSelect: (artifactPath: string) => {
    set((state) => {
      const next = new Set(state.selectedPaths);
      if (next.has(artifactPath)) {
        next.delete(artifactPath);
      } else {
        next.add(artifactPath);
      }
      return { selectedPaths: next };
    });
  },

  selectAll: () => {
    set((state) => ({
      selectedPaths: new Set(state.artifacts.map((a) => a.artifact_path)),
    }));
  },

  deselectAll: () => {
    set({ selectedPaths: new Set() });
  },

  purge: async () => {
    const { selectedPaths } = get();
    if (selectedPaths.size === 0) return;

    set({ phase: "purging", progress: null, error: null });

    const unlisten = await listenPurgeProgress((progress) => {
      set({ progress });
    });

    try {
      const { dry_run: dryRun, use_trash } = useSettingsStore.getState().settings;
      const permanent = !use_trash;
      const result = await executePurge(Array.from(selectedPaths), dryRun, permanent);
      set({ phase: "done", result });
    } catch (e) {
      set({ phase: "list", error: String(e) });
    } finally {
      unlisten();
    }
  },

  dismissDone: () => {
    const { artifacts, selectedPaths } = get();
    const remaining = artifacts.filter((a) => !selectedPaths.has(a.artifact_path));
    if (remaining.length === 0) {
      set({ phase: "idle", artifacts: [], selectedPaths: new Set(), progress: null, result: null });
    } else {
      const newSelected = new Set(remaining.map((a) => a.artifact_path));
      set({ phase: "list", artifacts: remaining, selectedPaths: newSelected, progress: null, result: null });
    }
  },

  reset: () => {
    set({
      phase: "idle",
      artifacts: [],
      selectedPaths: new Set(),
      progress: null,
      result: null,
      error: null,
    });
  },
}));
