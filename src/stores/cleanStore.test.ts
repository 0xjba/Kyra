import { beforeEach, describe, expect, it } from "vitest";
import { emit, invokedWith, listenerCount, onInvoke } from "../test/tauri";
import { scanItem } from "../test/fixtures";
import { useCleanStore } from "./cleanStore";
import { useSettingsStore } from "./settingsStore";

const initialClean = useCleanStore.getState();
const initialSettings = useSettingsStore.getState();

const items = [scanItem("chrome", 100), scanItem("slack", 50), scanItem("xcode", 0)];
const noIssues = { bytes_failed: 0, failed: [], already_gone: [] };

beforeEach(() => {
  useCleanStore.setState(initialClean, true);
  useSettingsStore.setState(initialSettings, true);
});

describe("scan", () => {
  it("moves to results and selects every item", async () => {
    onInvoke("scan_for_cleanables", () => items);
    const pending = useCleanStore.getState().scan();
    expect(useCleanStore.getState().phase).toBe("scanning");
    await pending;
    const s = useCleanStore.getState();
    expect(s.phase).toBe("results");
    expect(s.items).toEqual(items);
    expect(s.selectedIds).toEqual(new Set(["chrome", "slack", "xcode"]));
  });

  it("leaves Orphaned Data unselected so leftovers are opt-in", async () => {
    onInvoke("scan_for_cleanables", () => [...items, scanItem("orphan_app", 80, "Orphaned Data")]);
    await useCleanStore.getState().scan();
    const selected = useCleanStore.getState().selectedIds;
    expect(selected.has("orphan_app")).toBe(false);
    expect(selected.has("chrome")).toBe(true);
  });

  it("returns to idle with the error on failure", async () => {
    onInvoke("scan_for_cleanables", () => {
      throw new Error("no access");
    });
    await useCleanStore.getState().scan();
    expect(useCleanStore.getState().phase).toBe("idle");
    expect(useCleanStore.getState().error).toContain("no access");
  });
});

describe("selection", () => {
  beforeEach(() => useCleanStore.setState({ phase: "results", items, selectedIds: new Set() }));

  it("toggles, selects all and deselects all", () => {
    const s = useCleanStore.getState();
    s.toggleItem("chrome");
    expect(useCleanStore.getState().selectedIds).toEqual(new Set(["chrome"]));
    s.toggleItem("chrome");
    expect(useCleanStore.getState().selectedIds.size).toBe(0);
    s.selectAll();
    expect(useCleanStore.getState().selectedIds).toEqual(new Set(["chrome", "slack", "xcode"]));
    s.deselectAll();
    expect(useCleanStore.getState().selectedIds.size).toBe(0);
  });
});

describe("clean", () => {
  beforeEach(() => useCleanStore.setState({ phase: "results", items, selectedIds: new Set(["chrome"]) }));

  it("does nothing without a selection", async () => {
    useCleanStore.setState({ selectedIds: new Set() });
    await useCleanStore.getState().clean();
    expect(useCleanStore.getState().phase).toBe("results");
    expect(invokedWith("execute_clean")).toHaveLength(0);
  });

  it("cleans only selected items, honours use_trash, reports progress and credits bytes", async () => {
    useSettingsStore.setState({ settings: { ...initialSettings.settings, use_trash: true } });
    const progress = { current_item: "chrome", items_done: 1, items_total: 1, paths_done: 1, paths_total: 1, bytes_freed: 100 };
    onInvoke("execute_clean", () => {
      emit("clean-progress", progress);
      return { items_cleaned: 1, bytes_freed: 100, errors: [], cleaned_ids: ["chrome"], ...noIssues };
    });
    onInvoke("add_bytes_freed", () => 100);

    await useCleanStore.getState().clean();

    const [args] = invokedWith("execute_clean");
    expect((args.items as { rule_id: string }[]).map((i) => i.rule_id)).toEqual(["chrome"]);
    expect(args).toMatchObject({ dryRun: false, permanent: false });
    expect(useCleanStore.getState()).toMatchObject({ phase: "done", progress });
    expect(invokedWith("add_bytes_freed")).toEqual([{ bytes: 100 }]);
    expect(listenerCount("clean-progress")).toBe(0);
  });

  it("goes back to results with the error on failure", async () => {
    onInvoke("execute_clean", () => {
      throw new Error("denied");
    });
    await useCleanStore.getState().clean();
    expect(useCleanStore.getState().phase).toBe("results");
    expect(useCleanStore.getState().error).toContain("denied");
    expect(listenerCount("clean-progress")).toBe(0);
  });
});

describe("dismissDone", () => {
  it("keeps uncleaned items and reselects them", () => {
    useCleanStore.setState({
      phase: "done",
      items,
      result: { items_cleaned: 1, bytes_freed: 100, errors: [], cleaned_ids: ["chrome"], ...noIssues },
    });
    useCleanStore.getState().dismissDone();
    const s = useCleanStore.getState();
    expect(s.phase).toBe("results");
    expect(s.items.map((i) => i.rule_id)).toEqual(["slack", "xcode"]);
    expect(s.selectedIds).toEqual(new Set(["slack", "xcode"]));
    expect(s.result).toBeNull();
  });

  it("goes idle when only empty items remain", () => {
    useCleanStore.setState({
      phase: "done",
      items,
      result: { items_cleaned: 2, bytes_freed: 150, errors: [], cleaned_ids: ["chrome", "slack"], ...noIssues },
    });
    useCleanStore.getState().dismissDone();
    expect(useCleanStore.getState()).toMatchObject({ phase: "idle", items: [] });
  });
});

describe("lifetime stats", () => {
  beforeEach(() => useCleanStore.setState({ phase: "results", items, selectedIds: new Set(["chrome", "slack"]) }));

  it("credits only the bytes actually freed, not what failed or was already gone", async () => {
    const result = {
      items_cleaned: 1,
      bytes_freed: 100,
      errors: ["x"],
      cleaned_ids: ["chrome"],
      bytes_failed: 50,
      failed: [{ rule_id: "slack", label: "slack", path: "/tmp/slack", size: 50, reason: "in_use" }],
      already_gone: [{ rule_id: "chrome", label: "chrome", path: "/tmp/chrome/x", size: 30, reason: "already_gone" }],
    };
    onInvoke("execute_clean", () => result);
    onInvoke("add_bytes_freed", () => 100);
    await useCleanStore.getState().clean();
    expect(invokedWith("add_bytes_freed")).toEqual([{ bytes: 100 }]);
    expect(useCleanStore.getState().result).toEqual(result);
  });
});
