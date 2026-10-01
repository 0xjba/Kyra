import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { openUrl } from "@tauri-apps/plugin-opener";
import { emit, flush, invokedWith, listenerCount, onInvoke } from "../test/tauri";
import { patrolRules, patrolRun, patrolStatus, reviewItem, toWireRun, toWireStatus } from "../test/fixtures";
import { useGuardianStore } from "./guardianStore";
import { useSettingsStore } from "./settingsStore";

const initial = useGuardianStore.getState();
const initialSettings = useSettingsStore.getState();
const EVENTS = ["patrol-started", "patrol-finished", "patrol-status"];

beforeEach(() => {
  useGuardianStore.setState(initial, true);
  vi.mocked(openUrl).mockReset();
  vi.mocked(openUrl).mockResolvedValue(undefined);
});

afterEach(() => {
  useGuardianStore.getState().unsubscribePatrol();
  useSettingsStore.setState(initialSettings, true);
});

describe("patrol events", () => {
  it("subscribes to each event once, however often it's asked", async () => {
    onInvoke("guardian_patrol_status", () => patrolStatus());
    const s = useGuardianStore.getState();
    await Promise.all([s.subscribePatrol(), s.subscribePatrol(), s.loadPatrolStatus()]);
    await s.loadPatrolStatus();
    for (const e of EVENTS) expect(listenerCount(e)).toBe(1);
  });

  it("unsubscribe removes the listeners and allows a fresh subscription", async () => {
    await useGuardianStore.getState().subscribePatrol();
    useGuardianStore.getState().unsubscribePatrol();
    for (const e of EVENTS) expect(listenerCount(e)).toBe(0);
    await useGuardianStore.getState().subscribePatrol();
    for (const e of EVENTS) expect(listenerCount(e)).toBe(1);
  });

  it("drops listeners that resolve after an unsubscribe", async () => {
    const pending = useGuardianStore.getState().subscribePatrol();
    useGuardianStore.getState().unsubscribePatrol();
    await pending;
    for (const e of EVENTS) expect(listenerCount(e)).toBe(0);
  });

  it("keeps status live from started, status and finished events", async () => {
    const status = patrolStatus({ freed_total: 1000, history: [patrolRun({ started_at: 1 })] });
    onInvoke("guardian_patrol_status", () => toWireStatus(status));
    await useGuardianStore.getState().loadPatrolStatus();

    emit("patrol-started", { trigger: "low_disk" });
    expect(useGuardianStore.getState().patrolStatus?.running).toBe(true);

    const run = patrolRun({ started_at: 50, finished_at: 60, trigger: "low_disk", freed: 500 });
    onInvoke("guardian_patrol_status", () => toWireStatus({ ...status, pending_review: [reviewItem("docker")] }));
    emit("patrol-finished", toWireRun(run));
    expect(useGuardianStore.getState().patrolStatus).toMatchObject({
      running: false,
      last_patrol_at: 60,
      freed_last: 500,
      freed_total: 1500,
    });
    expect(useGuardianStore.getState().patrolStatus?.history[0]).toEqual(run);
    await flush();
    expect(useGuardianStore.getState().patrolStatus?.pending_review.map((i) => i.id)).toEqual(["docker"]);

    const pushed = patrolStatus({ enabled: false });
    emit("patrol-status", toWireStatus(pushed));
    expect(useGuardianStore.getState().patrolStatus).toEqual(pushed);
  });
});

describe("patrol actions", () => {
  beforeEach(() => {
    onInvoke("guardian_patrol_status", () => patrolStatus({ freed_total: 2000 }));
  });

  it("patrolNow marks running, applies the run and refreshes", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus() });
    let seenRunning = false;
    onInvoke("guardian_patrol_now", () => {
      seenRunning = useGuardianStore.getState().patrolStatus!.running;
      return patrolRun({ trigger: "manual", freed: 10 });
    });
    await useGuardianStore.getState().patrolNow();
    expect(seenRunning).toBe(true);
    expect(invokedWith("guardian_patrol_now")).toHaveLength(1);
    expect(useGuardianStore.getState().patrolStatus).toMatchObject({ running: false, freed_total: 2000 });
  });

  it("patrolNow does nothing while a patrol is running", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus({ running: true }) });
    await useGuardianStore.getState().patrolNow();
    expect(invokedWith("guardian_patrol_now")).toHaveLength(0);
  });

  it("patrolNow surfaces errors and clears running", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus() });
    onInvoke("guardian_patrol_status", () => {
      throw new Error("offline");
    });
    onInvoke("guardian_patrol_now", () => {
      throw new Error("probe crashed");
    });
    await useGuardianStore.getState().patrolNow();
    expect(useGuardianStore.getState().patrolError).toContain("probe crashed");
    expect(useGuardianStore.getState().patrolStatus?.running).toBe(false);
  });

  it("a rejected license drops back to the paywall", async () => {
    useGuardianStore.setState({ license: { active: true, expires: null }, patrolStatus: patrolStatus() });
    onInvoke("guardian_patrol_now", () => {
      throw "No active license";
    });
    await useGuardianStore.getState().patrolNow();
    expect(useGuardianStore.getState().license.active).toBe(false);
  });

  it("reviewClean cleans the ids and removes them from the queue", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus({ pending_review: [reviewItem("a"), reviewItem("b")] }) });
    onInvoke("guardian_patrol_status", () => {
      throw new Error("offline");
    });
    onInvoke("guardian_review_clean", () => ({ categories_cleaned: 1, bytes_freed: 1000, errors: [] }));
    await useGuardianStore.getState().reviewClean(["a"]);
    expect(invokedWith("guardian_review_clean")).toEqual([{ ids: ["a"] }]);
    const s = useGuardianStore.getState();
    expect(s.patrolStatus?.pending_review.map((i) => i.id)).toEqual(["b"]);
    expect(s.reviewResult?.bytes_freed).toBe(1000);
    expect(s.reviewBusy).toEqual([]);
    expect(s.reviewError).toBeNull();
  });

  it("reviewClean reports items that couldn't be removed", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus({ pending_review: [reviewItem("a")] }) });
    onInvoke("guardian_review_clean", () => ({ categories_cleaned: 0, bytes_freed: 0, errors: ["busy"] }));
    await useGuardianStore.getState().reviewClean(["a"]);
    expect(useGuardianStore.getState().reviewError).toBe("1 item couldn't be removed");
  });

  it("reviewClean keeps the item queued when the command fails", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus({ pending_review: [reviewItem("a")] }) });
    onInvoke("guardian_patrol_status", () => {
      throw new Error("offline");
    });
    onInvoke("guardian_review_clean", () => {
      throw new Error("disk");
    });
    await useGuardianStore.getState().reviewClean(["a"]);
    expect(useGuardianStore.getState().patrolStatus?.pending_review).toHaveLength(1);
    expect(useGuardianStore.getState().reviewError).toContain("disk");
  });

  it("reviewDismiss snoozes the ids", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus({ pending_review: [reviewItem("a"), reviewItem("b")] }) });
    onInvoke("guardian_patrol_status", () => {
      throw new Error("offline");
    });
    onInvoke("guardian_review_dismiss", () => undefined);
    await useGuardianStore.getState().reviewDismiss(["a", "b"]);
    expect(invokedWith("guardian_review_dismiss")).toEqual([{ ids: ["a", "b"] }]);
    expect(useGuardianStore.getState().patrolStatus?.pending_review).toEqual([]);
  });

  it("setRules saves the full rules and applies them right away", async () => {
    useGuardianStore.setState({ patrolStatus: patrolStatus({ enabled: false }) });
    onInvoke("guardian_set_rules", () => undefined);
    const rules = patrolRules({ enabled: true, frequency: "weekly" });
    const pending = useGuardianStore.getState().setRules(rules);
    expect(useGuardianStore.getState().patrolStatus).toMatchObject({ enabled: true, rules });
    await pending;
    expect(invokedWith("guardian_set_rules")).toEqual([{ rules }]);
    expect(useGuardianStore.getState().patrolStatus?.rules).toEqual(rules);
  });

  it("setRules rolls back and reports when saving fails", async () => {
    const before = patrolStatus();
    useGuardianStore.setState({ patrolStatus: before });
    onInvoke("guardian_set_rules", () => {
      throw "disk full";
    });
    await useGuardianStore.getState().setRules({ ...before.rules, enabled: false, safe_action: "ask" });
    expect(useGuardianStore.getState().patrolStatus).toMatchObject({ enabled: true, rules: before.rules });
    expect(useGuardianStore.getState().patrolError).toBe("disk full");
  });

  it("setRules refreshes cached settings so a later settings save can't undo the rules", async () => {
    useSettingsStore.setState({ loaded: true });
    useGuardianStore.setState({ patrolStatus: patrolStatus() });
    onInvoke("guardian_set_rules", () => undefined);
    onInvoke("load_settings", () => ({ ...useSettingsStore.getState().settings, low_disk_threshold_gb: 30 }));
    await useGuardianStore.getState().setRules(patrolRules({ low_gb: 30 }));
    expect(useSettingsStore.getState().settings.low_disk_threshold_gb).toBe(30);
  });

  it("remembers the last Pawtrol tab", () => {
    useGuardianStore.getState().setPawtrolTab("rules");
    expect(useGuardianStore.getState().pawtrolTab).toBe("rules");
    expect(localStorage.getItem("kyra.pawtrol.tab")).toBe("rules");
  });
});

describe("license", () => {
  it("checkLicense stores device id, license and device name", async () => {
    onInvoke("guardian_get_device_id", () => "dev-9");
    onInvoke("guardian_check_license", () => ({ active: true, expires: 123 }));
    onInvoke("get_device_name", () => "Studio");
    await useGuardianStore.getState().checkLicense();
    expect(invokedWith("guardian_check_license")).toEqual([{ deviceId: "dev-9" }]);
    expect(useGuardianStore.getState()).toMatchObject({
      deviceId: "dev-9",
      deviceName: "Studio",
      license: { active: true, expires: 123 },
    });
  });

  it("checkLicense fails silently", async () => {
    await expect(useGuardianStore.getState().checkLicense()).resolves.toBeUndefined();
    expect(useGuardianStore.getState().license.active).toBe(false);
  });

});

const ACCOUNT = {
  email: "j***@gmail.com",
  status: "active",
  current_end: 1_932_854_400,
  cancel_at_period_end: false,
  devices_count: 2,
};

describe("subscribe", () => {
  beforeEach(() => {
    onInvoke("guardian_get_device_id", () => "dev-1");
    onInvoke("get_device_name", () => "Mac");
    onInvoke("guardian_checkout_create", () => ({ short_url: "https://pay.rev.cat/tok/kyra-abc?email=me%40example.com" }));
  });

  afterEach(() => {
    useGuardianStore.getState().stopCheckoutPoll();
  });

  it("creates checkout with the trimmed email, opens short_url and polls every 5s until active", async () => {
    vi.useFakeTimers();
    let active = false;
    onInvoke("guardian_check_license", () => ({ active, expires: null }));

    await expect(useGuardianStore.getState().subscribe("  me@example.com ")).resolves.toBe(true);
    expect(invokedWith("guardian_checkout_create")).toEqual([{ email: "me@example.com" }]);
    expect(openUrl).toHaveBeenCalledWith("https://pay.rev.cat/tok/kyra-abc?email=me%40example.com");
    expect(useGuardianStore.getState().checkoutPolling).toBe(true);

    await vi.advanceTimersByTimeAsync(4_999);
    expect(invokedWith("guardian_check_license")).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(invokedWith("guardian_check_license")).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(invokedWith("guardian_check_license")).toHaveLength(3);

    active = true;
    await vi.advanceTimersByTimeAsync(5_000);
    expect(useGuardianStore.getState().license.active).toBe(true);
    expect(useGuardianStore.getState().checkoutPolling).toBe(false);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(invokedWith("guardian_check_license")).toHaveLength(4);
  });

  it("gives up polling after 10 minutes", async () => {
    vi.useFakeTimers();
    onInvoke("guardian_check_license", () => ({ active: false, expires: null }));
    await useGuardianStore.getState().subscribe("me@example.com");
    await vi.advanceTimersByTimeAsync(10 * 60 * 1000);
    expect(invokedWith("guardian_check_license")).toHaveLength(120);
    expect(useGuardianStore.getState().checkoutPolling).toBe(false);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(invokedWith("guardian_check_license")).toHaveLength(120);
  });

  it("shows the worker's message and opens nothing when checkout can't be created", async () => {
    onInvoke("guardian_checkout_create", () => {
      throw "Enter a valid email address.";
    });
    await expect(useGuardianStore.getState().subscribe("x@y.co")).resolves.toBe(false);
    expect(useGuardianStore.getState().subscribeError).toBe("Enter a valid email address.");
    expect(openUrl).not.toHaveBeenCalled();
    expect(useGuardianStore.getState().checkoutPolling).toBe(false);
  });

  it("flags an error when the browser can't open, and clears it on retry", async () => {
    vi.mocked(openUrl).mockRejectedValueOnce(new Error("no browser"));
    await expect(useGuardianStore.getState().subscribe("x@y.co")).resolves.toBe(false);
    expect(useGuardianStore.getState().subscribeError).toMatch(/Couldn't open checkout/);
    expect(useGuardianStore.getState().checkoutPolling).toBe(false);

    await useGuardianStore.getState().subscribe("x@y.co");
    expect(useGuardianStore.getState().subscribeError).toBeNull();
  });
});

describe("restore and account", () => {
  it("restoreVerify activates the license on success", async () => {
    onInvoke("guardian_restore_verify", () => ({ active: true, expires: 99 }));
    await expect(useGuardianStore.getState().restoreVerify(" a@b.co ", "123456")).resolves.toBe(true);
    expect(invokedWith("guardian_restore_verify")).toEqual([{ email: "a@b.co", code: "123456" }]);
    expect(useGuardianStore.getState().license).toEqual({ active: true, expires: 99 });
  });

  it("restoreVerify resolves false for an inactive reply and rejects with the friendly error", async () => {
    onInvoke("guardian_restore_verify", () => ({ active: false, expires: null }));
    await expect(useGuardianStore.getState().restoreVerify("a@b.co", "123456")).resolves.toBe(false);
    expect(useGuardianStore.getState().license.active).toBe(false);

    onInvoke("guardian_restore_verify", () => {
      throw "That code didn't work. Check it and try again.";
    });
    await expect(useGuardianStore.getState().restoreVerify("a@b.co", "000000")).rejects.toThrow(
      "That code didn't work. Check it and try again.",
    );
  });

  it("loadAccount stores the account or null", async () => {
    onInvoke("guardian_account", () => ACCOUNT);
    await useGuardianStore.getState().loadAccount();
    expect(useGuardianStore.getState().account).toEqual(ACCOUNT);
    onInvoke("guardian_account", () => null);
    await useGuardianStore.getState().loadAccount();
    expect(useGuardianStore.getState().account).toBeNull();
  });

  it("manageSubscription opens the portal link unless the app already did", async () => {
    onInvoke("guardian_manage_subscription", () => ({ url: "https://billing.revenuecat.com/app1/sub1?token=t" }));
    await expect(useGuardianStore.getState().manageSubscription()).resolves.toBe(true);
    expect(openUrl).toHaveBeenCalledWith("https://billing.revenuecat.com/app1/sub1?token=t");

    vi.mocked(openUrl).mockClear();
    onInvoke("guardian_manage_subscription", () => ({ url: "http://127.0.0.1:8787/dev/mock-manage", opened_by_app: true }));
    await expect(useGuardianStore.getState().manageSubscription()).resolves.toBe(true);
    expect(openUrl).not.toHaveBeenCalled();
  });

  it("manageSubscription reports worker and opener failures", async () => {
    onInvoke("guardian_manage_subscription", () => {
      throw "Pawtrol's server had a hiccup. Try again in a minute.";
    });
    await expect(useGuardianStore.getState().manageSubscription()).resolves.toBe(false);
    expect(useGuardianStore.getState().accountError).toMatch(/hiccup/);

    onInvoke("guardian_manage_subscription", () => ({ url: "https://billing.revenuecat.com/x" }));
    vi.mocked(openUrl).mockRejectedValueOnce("not allowed");
    await expect(useGuardianStore.getState().manageSubscription()).resolves.toBe(false);
    expect(useGuardianStore.getState().accountError).toBe(
      "Couldn't open the subscription page in your browser. Try again.",
    );
  });
});
