import { create } from "zustand";

interface NavigationStore {
  /** When set, the TitleBar back button calls this instead of navigating to "/" */
  backOverride: (() => void) | null;
  setBackOverride: (fn: (() => void) | null) => void;
}

export const useNavigationStore = create<NavigationStore>((set) => ({
  backOverride: null,
  setBackOverride: (fn) => set({ backOverride: fn }),
}));
