import { create } from "zustand";
import {
  guardianAccount,
  guardianCancelSubscription,
  guardianCheckoutCreate,
  guardianRestoreStart,
  guardianRestoreVerify,
  guardianCheckLicense,
  guardianGetDeviceId,
  getDeviceName,
  guardianPatrolStatus,
  guardianPatrolNow,
  guardianReviewClean,
  guardianReviewDismiss,
  guardianSetPatrol,
  listenPatrolStarted,
  listenPatrolFinished,
  listenPatrolStatus,
  type Account,
  type GuardianCleanResult,
  type LicenseStatus,
  type CheckoutSession,
  type PatrolRun,
  type PatrolStatus,
} from "../lib/tauri";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { errorText } from "../utils/pawtrolAccount";

const HISTORY_LIMIT = 20;
export const CHECKOUT_POLL_MS = 5_000;
const CHECKOUT_POLL_LIMIT = (10 * 60 * 1000) / CHECKOUT_POLL_MS;
const OPEN_FAILED = "Couldn't open checkout in your browser. Try again.";

function isLicenseError(msg: string): boolean {
  return msg.includes("No active license") || msg.includes("License expired") || msg.includes("403");
}

let patrolGeneration = 0;
let patrolSubscription: Promise<void> | null = null;
let patrolUnlisten: UnlistenFn[] = [];
let checkoutTimer: ReturnType<typeof setInterval> | null = null;

interface GuardianStore {
  license: LicenseStatus;
  deviceId: string;
  deviceName: string;
  subscribeError: string | null;
  checkoutPolling: boolean;

  /** undefined until loaded; null when the worker has no account for this Mac. */
  account: Account | null | undefined;
  accountError: string | null;

  patrolStatus: PatrolStatus | null;
  patrolError: string | null;
  /** Review ids with a clean or dismiss in flight. */
  reviewBusy: string[];
  reviewResult: GuardianCleanResult | null;
  reviewError: string | null;

  checkLicense: () => Promise<void>;
  /** Creates a hosted checkout for this Mac, opens it and polls until the license turns active. */
  subscribe: (email: string) => Promise<boolean>;
  startCheckoutPoll: () => void;
  stopCheckoutPoll: () => void;
  restoreStart: (email: string) => Promise<void>;
  /** Resolves with whether this Mac is now licensed; rejects with a friendly message. */
  restoreVerify: (email: string, code: string) => Promise<boolean>;
  loadAccount: () => Promise<void>;
  cancelSubscription: () => Promise<boolean>;

  subscribePatrol: () => Promise<void>;
  unsubscribePatrol: () => void;
  loadPatrolStatus: () => Promise<void>;
  patrolNow: () => Promise<void>;
  reviewClean: (ids: string[]) => Promise<void>;
  reviewDismiss: (ids: string[]) => Promise<void>;
  setPatrol: (enabled: boolean, autoClean: boolean) => Promise<void>;
}

export const useGuardianStore = create<GuardianStore>((set, get) => {
  const failed = (e: unknown): string => {
    const msg = String(e);
    if (isLicenseError(msg)) set({ license: { active: false, expires: null } });
    return msg;
  };

  const patchStatus = (patch: (s: PatrolStatus) => Partial<PatrolStatus>) => {
    const s = get().patrolStatus;
    if (s) set({ patrolStatus: { ...s, ...patch(s) } });
  };

  const applyRun = (run: PatrolRun) => {
    patchStatus((s) => ({
      running: false,
      last_patrol_at: run.finished_at,
      freed_last: run.freed,
      freed_total: s.freed_total + run.freed,
      history: [run, ...s.history.filter((h) => h.started_at !== run.started_at)].slice(0, HISTORY_LIMIT),
    }));
  };

  const withoutItems = (ids: string[]) => {
    patchStatus((s) => ({ pending_review: s.pending_review.filter((i) => !ids.includes(i.id)) }));
  };

  const runReview = async (ids: string[], action: () => Promise<void>) => {
    if (ids.length === 0) return;
    set((s) => ({ reviewBusy: [...s.reviewBusy, ...ids], reviewError: null }));
    try {
      await action();
      withoutItems(ids);
    } catch (e) {
      set({ reviewError: failed(e) });
    } finally {
      set((s) => ({ reviewBusy: s.reviewBusy.filter((id) => !ids.includes(id)) }));
    }
    await get().loadPatrolStatus();
  };

  return {
    license: { active: false, expires: null },
    deviceId: "",
    deviceName: "",
    subscribeError: null,
    checkoutPolling: false,

    account: undefined,
    accountError: null,

    patrolStatus: null,
    patrolError: null,
    reviewBusy: [],
    reviewResult: null,
    reviewError: null,

    checkLicense: async () => {
      try {
        let deviceId = get().deviceId;
        if (!deviceId) {
          deviceId = await guardianGetDeviceId();
          set({ deviceId });
        }
        const license = await guardianCheckLicense(deviceId);
        set({ license });
        if (license.active) get().stopCheckoutPoll();
      } catch {
        // Non-critical: the cached license stays in place.
      }

      if (!get().deviceName) {
        try {
          const name = await getDeviceName();
          set({ deviceName: name });
        } catch {}
      }
    },

    subscribe: async (email) => {
      set({ subscribeError: null });
      let session: CheckoutSession;
      try {
        session = await guardianCheckoutCreate(email.trim());
      } catch (e) {
        set({ subscribeError: errorText(e) });
        return false;
      }
      if (!session.opened_by_app) {
        try {
          await openUrl(session.short_url);
        } catch {
          set({ subscribeError: OPEN_FAILED });
          return false;
        }
      }
      get().startCheckoutPoll();
      return true;
    },

    startCheckoutPoll: () => {
      get().stopCheckoutPoll();
      let ticks = 0;
      set({ checkoutPolling: true });
      checkoutTimer = setInterval(async () => {
        ticks++;
        await get().checkLicense();
        if (get().license.active || ticks >= CHECKOUT_POLL_LIMIT) get().stopCheckoutPoll();
      }, CHECKOUT_POLL_MS);
    },

    stopCheckoutPoll: () => {
      if (checkoutTimer) clearInterval(checkoutTimer);
      checkoutTimer = null;
      if (get().checkoutPolling) set({ checkoutPolling: false });
    },

    restoreStart: async (email) => {
      try {
        await guardianRestoreStart(email.trim());
      } catch (e) {
        throw new Error(errorText(e));
      }
    },

    restoreVerify: async (email, code) => {
      let license: LicenseStatus;
      try {
        license = await guardianRestoreVerify(email.trim(), code);
      } catch (e) {
        throw new Error(errorText(e));
      }
      if (!license.active) return false;
      set({ license, account: undefined });
      get().stopCheckoutPoll();
      return true;
    },

    loadAccount: async () => {
      try {
        set({ account: await guardianAccount(), accountError: null });
      } catch (e) {
        set({ accountError: errorText(e) });
      }
    },

    cancelSubscription: async () => {
      set({ accountError: null });
      try {
        set({ account: await guardianCancelSubscription() });
      } catch (e) {
        set({ accountError: errorText(e) });
        return false;
      }
      await get().checkLicense();
      return true;
    },

    subscribePatrol: () => {
      if (patrolSubscription) return patrolSubscription;
      const gen = patrolGeneration;
      patrolSubscription = (async () => {
        const fns = await Promise.all([
          listenPatrolStarted(() => patchStatus(() => ({ running: true }))),
          listenPatrolFinished((run) => {
            applyRun(run);
            get().loadPatrolStatus();
          }),
          listenPatrolStatus((status) => set({ patrolStatus: status })),
        ]);
        if (gen !== patrolGeneration) {
          fns.forEach((f) => f());
          return;
        }
        patrolUnlisten = fns;
      })();
      return patrolSubscription;
    },

    unsubscribePatrol: () => {
      patrolGeneration++;
      patrolUnlisten.forEach((f) => f());
      patrolUnlisten = [];
      patrolSubscription = null;
    },

    loadPatrolStatus: async () => {
      get().subscribePatrol().catch(() => {});
      try {
        set({ patrolStatus: await guardianPatrolStatus() });
      } catch (e) {
        failed(e);
      }
    },

    patrolNow: async () => {
      if (get().patrolStatus?.running) return;
      set({ patrolError: null });
      patchStatus(() => ({ running: true }));
      try {
        applyRun(await guardianPatrolNow());
      } catch (e) {
        set({ patrolError: failed(e) });
        patchStatus(() => ({ running: false }));
      }
      await get().loadPatrolStatus();
    },

    reviewClean: (ids) =>
      runReview(ids, async () => {
        set({ reviewResult: null });
        const result = await guardianReviewClean(ids);
        set({
          reviewResult: result,
          reviewError: result.errors.length > 0
            ? `${result.errors.length} ${result.errors.length === 1 ? "item" : "items"} couldn't be removed`
            : null,
        });
      }),

    reviewDismiss: (ids) => runReview(ids, () => guardianReviewDismiss(ids)),

    setPatrol: async (enabled, autoClean) => {
      patchStatus(() => ({ enabled, auto_clean: autoClean }));
      try {
        await guardianSetPatrol(enabled, autoClean);
      } catch (e) {
        set({ patrolError: failed(e) });
      }
      await get().loadPatrolStatus();
    },
  };
});
