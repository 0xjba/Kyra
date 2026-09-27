import { create } from "zustand";
import {
  guardianRunProbes,
  guardianScore,
  guardianClean,
  guardianCheckLicense,
  guardianGetDeviceId,
  listenGuardianCleanProgress,
  addBytesFreed,
  type ProbeReport,
  type ScoredProbe,
  type GuardianCleanProgress,
  type GuardianCleanResult,
  type LicenseStatus,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";

type GuardianPhase =
  | "idle"
  | "scanning"
  | "scoring"
  | "results"
  | "cleaning"
  | "success"
  | "error";

const AUTO_SELECT_THRESHOLD = 60;

interface GuardianStore {
  phase: GuardianPhase;
  probes: ProbeReport[];
  scores: ScoredProbe[];
  selected: Set<string>;
  totalCleanable: number;
  scanDurationMs: number;
  progress: GuardianCleanProgress | null;
  cleanResult: GuardianCleanResult | null;
  error: string | null;
  license: LicenseStatus;
  deviceId: string;

  scan: () => Promise<void>;
  toggleCategory: (category: string) => void;
  selectAll: () => void;
  deselectAll: () => void;
  clean: (permanent: boolean) => Promise<void>;
  checkLicense: () => Promise<void>;
  reset: () => void;
}

export const useGuardianStore = create<GuardianStore>((set, get) => ({
  phase: "idle",
  probes: [],
  scores: [],
  selected: new Set(),
  totalCleanable: 0,
  scanDurationMs: 0,
  progress: null,
  cleanResult: null,
  error: null,
  license: { active: false, expires: null },
  deviceId: "",

  scan: async () => {
    set({ phase: "scanning", error: null, cleanResult: null });
    try {
      const scanResult = await guardianRunProbes();
      set({
        phase: "scoring",
        probes: scanResult.probes,
        totalCleanable: scanResult.total_cleanable,
        scanDurationMs: scanResult.scan_duration_ms,
      });

      let deviceId = get().deviceId;
      if (!deviceId) {
        try {
          deviceId = await guardianGetDeviceId();
          set({ deviceId });
        } catch {
          deviceId = "";
        }
      }

      const guardianResult = await guardianScore(scanResult.probes, deviceId);
      const autoSelected = new Set(
        guardianResult.scores
          .filter((s) => s.score >= AUTO_SELECT_THRESHOLD)
          .map((s) => s.category)
      );

      set({
        phase: "results",
        scores: guardianResult.scores,
        totalCleanable: guardianResult.total_cleanable,
        selected: autoSelected,
      });
    } catch (e) {
      set({ phase: "error", error: String(e) });
    }
  },

  toggleCategory: (category: string) => {
    const { selected } = get();
    const next = new Set(selected);
    if (next.has(category)) {
      next.delete(category);
    } else {
      next.add(category);
    }
    set({ selected: next });
  },

  selectAll: () => {
    const allCategories = new Set(get().scores.map((s) => s.category));
    set({ selected: allCategories });
  },

  deselectAll: () => {
    set({ selected: new Set() });
  },

  clean: async (permanent: boolean) => {
    const { selected } = get();
    const categories = Array.from(selected);
    if (categories.length === 0) return;

    set({ phase: "cleaning", progress: null, cleanResult: null, error: null });

    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listenGuardianCleanProgress((progress) => {
        set({ progress });
      });

      const result = await guardianClean(categories, permanent);
      if (result.bytes_freed > 0) {
        addBytesFreed(result.bytes_freed).catch(() => {});
      }
      set({ phase: "success", cleanResult: result });
    } catch (e) {
      set({ phase: "error", error: String(e) });
    } finally {
      if (unlisten) unlisten();
    }
  },

  checkLicense: async () => {
    try {
      let deviceId = get().deviceId;
      if (!deviceId) {
        deviceId = await guardianGetDeviceId();
        set({ deviceId });
      }
      const license = await guardianCheckLicense(deviceId);
      set({ license });
    } catch {
      // Silently fail — license check is non-critical for scanning
    }
  },

  reset: () => {
    set({
      phase: "idle",
      probes: [],
      scores: [],
      selected: new Set(),
      totalCleanable: 0,
      scanDurationMs: 0,
      progress: null,
      cleanResult: null,
      error: null,
    });
  },
}));
