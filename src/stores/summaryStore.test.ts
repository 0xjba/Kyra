import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { onInvoke } from "../test/tauri";
import { artifact, installer, scanItem } from "../test/fixtures";

const KEY = "kyra_scan_summaries";
const T0 = new Date(2026, 0, 10, 9, 0, 0).getTime();

async function freshModules() {
  vi.resetModules();
  const summary = await import("./summaryStore");
  const { useCleanStore } = await import("./cleanStore");
  const { usePruneStore } = await import("./pruneStore");
  const { useInstallersStore } = await import("./installersStore");
  const { useOptimizeStore } = await import("./optimizeStore");
  return { ...summary, useCleanStore, usePruneStore, useInstallersStore, useOptimizeStore };
}

async function tracked() {
  const m = await freshModules();
  m.startSummaryTracking();
  return m;
}

const persisted = () => JSON.parse(localStorage.getItem(KEY) ?? "null");

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(T0);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("loading persisted summaries", () => {
  it("starts empty with a zero clean counter", async () => {
    const { useSummaryStore } = await freshModules();
    expect(useSummaryStore.getState()).toEqual({ cleans: 0 });
  });

  it("restores stored values and fills missing fields", async () => {
    localStorage.setItem(KEY, JSON.stringify({ optimizedAt: 5, clean: { bytes: 1, count: 1, at: 2 } }));
    const { useSummaryStore } = await freshModules();
    expect(useSummaryStore.getState()).toMatchObject({ cleans: 0, optimizedAt: 5, clean: { bytes: 1 } });
  });

  it("tolerates corrupt JSON", async () => {
    localStorage.setItem(KEY, "{not json");
    const { useSummaryStore } = await freshModules();
    expect(useSummaryStore.getState()).toEqual({ cleans: 0 });
  });
});

describe("clean tracking", () => {
  it("records a summary when results arrive and persists it", async () => {
    const { useCleanStore, useSummaryStore } = await tracked();
    useCleanStore.setState({ phase: "scanning" });
    useCleanStore.setState({ phase: "results", items: [scanItem("a", 100), scanItem("b", 50)] });

    expect(useSummaryStore.getState().clean).toEqual({ bytes: 150, count: 2, at: T0 });
    expect(persisted().clean).toEqual({ bytes: 150, count: 2, at: T0 });
  });

  it("does not rewrite the summary on unrelated results-phase updates", async () => {
    const { useCleanStore, useSummaryStore } = await tracked();
    useCleanStore.setState({ phase: "results", items: [scanItem("a", 100)] });
    vi.setSystemTime(T0 + 5000);
    useCleanStore.getState().deselectAll();
    expect(useSummaryStore.getState().clean?.at).toBe(T0);
  });

  it("counts a clean run on done and zeroes the tile on done -> idle", async () => {
    const { useCleanStore, useSummaryStore } = await tracked();
    useCleanStore.setState({ phase: "results", items: [scanItem("a", 100)] });
    useCleanStore.setState({ phase: "cleaning" });
    useCleanStore.setState({ phase: "done" });

    expect(useSummaryStore.getState()).toMatchObject({ cleans: 1, firstCleanAt: T0 });

    vi.setSystemTime(T0 + 1000);
    useCleanStore.setState({ phase: "idle" });
    expect(useSummaryStore.getState().clean).toEqual({ bytes: 0, count: 0, at: T0 + 1000 });
  });

  it("keeps the first clean date across later runs", async () => {
    const { useCleanStore, useSummaryStore } = await tracked();
    useCleanStore.setState({ phase: "done" });
    useCleanStore.setState({ phase: "results" });
    vi.setSystemTime(T0 + 86_400_000);
    useCleanStore.setState({ phase: "done" });
    expect(useSummaryStore.getState()).toMatchObject({ cleans: 2, firstCleanAt: T0 });
    expect(persisted()).toMatchObject({ cleans: 2, firstCleanAt: T0 });
  });

  it("subscribes only once even if started twice", async () => {
    const m = await tracked();
    m.startSummaryTracking();
    m.useCleanStore.setState({ phase: "done" });
    expect(m.useSummaryStore.getState().cleans).toBe(1);
  });
});

describe("prune tracking", () => {
  it("records bytes, count and distinct projects on list", async () => {
    const { usePruneStore, useSummaryStore } = await tracked();
    usePruneStore.setState({
      phase: "list",
      artifacts: [artifact("web", "node_modules", 300, 40), artifact("web", "dist", 20, 40), artifact("api", "target", 80, 2)],
    });
    expect(useSummaryStore.getState().prune).toEqual({ bytes: 400, count: 3, projects: 2, at: T0 });
  });

  it("counts a run and zeroes on done -> idle", async () => {
    const { usePruneStore, useSummaryStore } = await tracked();
    usePruneStore.setState({ phase: "list", artifacts: [artifact("web", "node_modules", 300, 40)] });
    usePruneStore.setState({ phase: "done" });
    usePruneStore.setState({ phase: "idle" });
    expect(useSummaryStore.getState()).toMatchObject({
      cleans: 1,
      prune: { bytes: 0, count: 0, projects: 0 },
    });
  });
});

describe("installers tracking", () => {
  it("records the list and then zero after deleting everything", async () => {
    const { useInstallersStore, useSummaryStore } = await tracked();
    const files = [installer("a.dmg", 500), installer("b.pkg", 250)];
    onInvoke("scan_installers", () => files);
    onInvoke("delete_installers", () => ({
      items_removed: 2,
      bytes_freed: 750,
      errors: [],
      deleted_paths: files.map((f) => f.path),
    }));
    onInvoke("add_bytes_freed", () => 750);

    await useInstallersStore.getState().scan();
    expect(useSummaryStore.getState().installers).toEqual({ bytes: 750, count: 2, at: T0 });

    await useInstallersStore.getState().deleteSelected();
    expect(useSummaryStore.getState().cleans).toBe(1);

    useInstallersStore.getState().dismissDone();
    expect(useInstallersStore.getState()).toMatchObject({ phase: "list", files: [] });
    expect(useSummaryStore.getState().installers).toEqual({ bytes: 0, count: 0, at: T0 });
  });

  it("zeroes on done -> idle", async () => {
    const { useInstallersStore, useSummaryStore } = await tracked();
    useInstallersStore.setState({ phase: "list", files: [installer("a.dmg", 5)] });
    useInstallersStore.setState({ phase: "done" });
    useInstallersStore.setState({ phase: "idle" });
    expect(useSummaryStore.getState().installers).toEqual({ bytes: 0, count: 0, at: T0 });
  });
});

describe("optimize tracking", () => {
  it("stamps optimizedAt when a run finishes with a result", async () => {
    const { useOptimizeStore, useSummaryStore } = await tracked();
    useOptimizeStore.setState({ running: true });
    useOptimizeStore.setState({
      running: false,
      result: { tasks_run: 1, tasks_succeeded: 1, tasks_failed: 0, tasks_skipped: 0 },
    });
    expect(useSummaryStore.getState().optimizedAt).toBe(T0);
  });

  it("ignores runs that end without a result", async () => {
    const { useOptimizeStore, useSummaryStore } = await tracked();
    useOptimizeStore.setState({ running: true });
    useOptimizeStore.setState({ running: false, error: "boom" });
    expect(useSummaryStore.getState().optimizedAt).toBeUndefined();
  });
});

describe("storage failures", () => {
  it("keeps the in-memory value when localStorage throws", async () => {
    const { useCleanStore, useSummaryStore } = await tracked();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    useCleanStore.setState({ phase: "results", items: [scanItem("a", 10)] });
    expect(useSummaryStore.getState().clean?.bytes).toBe(10);
  });
});
