import { beforeEach, describe, expect, it } from "vitest";
import { invokedWith, onInvoke } from "../test/tauri";
import type { AppSettings } from "../lib/tauri";
import { useSettingsStore } from "./settingsStore";

const initial = useSettingsStore.getState();
const defaults = initial.settings;

beforeEach(() => {
  useSettingsStore.setState(initial, true);
  onInvoke("save_settings", () => undefined);
});

describe("defaults", () => {
  it("turns Pawtrol patrol and auto-clean on", () => {
    expect(defaults).toMatchObject({ pawtrol_enabled: true, pawtrol_auto_clean: true, pawtrol_login_prompted: false });
  });
});

describe("load", () => {
  it("merges stored settings over defaults", async () => {
    onInvoke("load_settings", () => ({ use_trash: true, whitelist: ["/keep"] }));
    await useSettingsStore.getState().load();
    const s = useSettingsStore.getState();
    expect(s.loaded).toBe(true);
    expect(s.settings).toEqual({ ...defaults, use_trash: true, whitelist: ["/keep"] });
  });

  it("marks loaded with defaults when loading fails", async () => {
    onInvoke("load_settings", () => {
      throw new Error("corrupt");
    });
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState()).toMatchObject({ loaded: true, settings: defaults });
  });
});

type Setter = (s: ReturnType<typeof useSettingsStore.getState>) => Promise<void>;

describe("setters persist via saveSettings", () => {
  it.each<[string, Setter, Partial<AppSettings>]>([
    ["setUseTrash", (s) => s.setUseTrash(true), { use_trash: true }],
    ["setLargeFileThreshold", (s) => s.setLargeFileThreshold(500), { large_file_threshold_mb: 500 }],
    ["setAnalyzeScanDepth", (s) => s.setAnalyzeScanDepth(12), { analyze_scan_depth: 12 }],
    ["setLaunchAtLogin", (s) => s.setLaunchAtLogin(true), { launch_at_login: true }],
    ["setCheckForUpdates", (s) => s.setCheckForUpdates(false), { check_for_updates: false }],
    ["setNotificationsEnabled", (s) => s.setNotificationsEnabled(false), { notifications_enabled: false }],
    ["setLowDiskThreshold", (s) => s.setLowDiskThreshold(25), { low_disk_threshold_gb: 25 }],
    ["setOnboardingCompleted", (s) => s.setOnboardingCompleted(true), { onboarding_completed: true }],
    ["setPawtrolEnabled", (s) => s.setPawtrolEnabled(false), { pawtrol_enabled: false }],
    ["setPawtrolAutoClean", (s) => s.setPawtrolAutoClean(false), { pawtrol_auto_clean: false }],
    ["setPawtrolLoginPrompted", (s) => s.setPawtrolLoginPrompted(true), { pawtrol_login_prompted: true }],
  ])("%s", async (_name, call, patch) => {
    await call(useSettingsStore.getState());
    const expected = { ...defaults, ...patch };
    expect(useSettingsStore.getState().settings).toEqual(expected);
    expect(invokedWith("save_settings")).toEqual([{ settings: expected }]);
  });

  it("keeps earlier changes when saving later ones", async () => {
    await useSettingsStore.getState().setUseTrash(true);
    await useSettingsStore.getState().setLowDiskThreshold(5);
    expect(invokedWith("save_settings")[1]).toEqual({
      settings: { ...defaults, use_trash: true, low_disk_threshold_gb: 5 },
    });
  });
});

describe("whitelist", () => {
  it("adds and removes paths after the backend confirms", async () => {
    onInvoke("add_to_whitelist", () => undefined);
    onInvoke("remove_from_whitelist", () => undefined);
    await useSettingsStore.getState().addWhitelist("/a");
    await useSettingsStore.getState().addWhitelist("/b");
    await useSettingsStore.getState().removeWhitelist("/a");
    expect(useSettingsStore.getState().settings.whitelist).toEqual(["/b"]);
    expect(invokedWith("add_to_whitelist")).toEqual([{ path: "/a" }, { path: "/b" }]);
  });

  it("leaves state unchanged when the backend rejects", async () => {
    onInvoke("add_to_whitelist", () => {
      throw new Error("invalid path");
    });
    await expect(useSettingsStore.getState().addWhitelist("/nope")).rejects.toThrow("invalid path");
    expect(useSettingsStore.getState().settings.whitelist).toEqual([]);
  });
});
