import { beforeEach, describe, expect, it } from "vitest";
import { emit, invokedWith, listenerCount, onInvoke } from "../test/tauri";
import { artifact } from "../test/fixtures";
import { idleDaysByProject, usePruneStore } from "./pruneStore";
import { useSettingsStore } from "./settingsStore";

const initialPrune = usePruneStore.getState();
const initialSettings = useSettingsStore.getState();

beforeEach(() => {
  usePruneStore.setState(initialPrune, true);
  useSettingsStore.setState(initialSettings, true);
});

describe("idleDaysByProject", () => {
  it("uses the newest artifact per project and null for unknown times", () => {
    const idle = idleDaysByProject([
      artifact("web", "node_modules", 1, 40),
      artifact("web", "dist", 1, 3),
      artifact("old", "target", 1, 120),
      artifact("mystery", "build", 1, null),
    ]);
    expect(idle.get("/Users/me/Projects/web")).toBe(3);
    expect(idle.get("/Users/me/Projects/old")).toBe(120);
    expect(idle.get("/Users/me/Projects/mystery")).toBeNull();
  });
});

describe("scan", () => {
  it("scans the chosen root and preselects only projects idle 30+ days", async () => {
    const list = [
      artifact("stale", "node_modules", 100, 45),
      artifact("edge", "target", 10, 30),
      artifact("fresh", "node_modules", 50, 29),
      artifact("unknown", "dist", 5, null),
    ];
    onInvoke("scan_artifacts", () => list);
    usePruneStore.getState().setRootPath("~/Code");
    await usePruneStore.getState().scan();

    expect(invokedWith("scan_artifacts")).toEqual([{ rootPath: "~/Code" }]);
    const s = usePruneStore.getState();
    expect(s.phase).toBe("list");
    expect(s.selectedPaths).toEqual(new Set([list[0].artifact_path, list[1].artifact_path]));
  });

  it("selects everything when no project is idle enough", async () => {
    const list = [artifact("a", "node_modules", 1, 1), artifact("b", "dist", 1, null)];
    onInvoke("scan_artifacts", () => list);
    await usePruneStore.getState().scan();
    expect(usePruneStore.getState().selectedPaths).toEqual(new Set(list.map((a) => a.artifact_path)));
  });

  it("returns to idle with the error on failure", async () => {
    onInvoke("scan_artifacts", () => {
      throw new Error("bad path");
    });
    await usePruneStore.getState().scan();
    expect(usePruneStore.getState()).toMatchObject({ phase: "idle" });
    expect(usePruneStore.getState().error).toContain("bad path");
  });
});

describe("selection", () => {
  it("toggles, selects all and deselects all", () => {
    const list = [artifact("a", "node_modules", 1, 1), artifact("b", "dist", 1, 1)];
    usePruneStore.setState({ phase: "list", artifacts: list, selectedPaths: new Set() });
    const s = usePruneStore.getState();
    s.toggleSelect(list[0].artifact_path);
    expect(usePruneStore.getState().selectedPaths).toEqual(new Set([list[0].artifact_path]));
    s.toggleSelect(list[0].artifact_path);
    expect(usePruneStore.getState().selectedPaths.size).toBe(0);
    s.selectAll();
    expect(usePruneStore.getState().selectedPaths.size).toBe(2);
    s.deselectAll();
    expect(usePruneStore.getState().selectedPaths.size).toBe(0);
  });
});

describe("prune", () => {
  const list = [artifact("a", "node_modules", 100, 60), artifact("b", "dist", 20, 60), artifact("c", "target", 5, 1)];

  beforeEach(() => {
    usePruneStore.setState({ phase: "list", artifacts: list, selectedPaths: new Set([list[0].artifact_path]) });
  });

  it("deletes the selection permanently when trash is off", async () => {
    onInvoke("execute_prune", () => {
      emit("prune-progress", { current_item: "a", items_done: 1, items_total: 1, bytes_freed: 100 });
      return { items_removed: 1, bytes_freed: 100, errors: [], cleaned_paths: [list[0].artifact_path] };
    });
    onInvoke("add_bytes_freed", () => 100);
    await usePruneStore.getState().prune();

    expect(invokedWith("execute_prune")).toEqual([
      { artifactPaths: [list[0].artifact_path], dryRun: false, permanent: true },
    ]);
    expect(usePruneStore.getState().phase).toBe("done");
    expect(usePruneStore.getState().progress?.bytes_freed).toBe(100);
    expect(listenerCount("prune-progress")).toBe(0);
  });

  it("returns to the list with the error on failure", async () => {
    onInvoke("execute_prune", () => {
      throw new Error("busy");
    });
    await usePruneStore.getState().prune();
    expect(usePruneStore.getState().phase).toBe("list");
    expect(usePruneStore.getState().error).toContain("busy");
    expect(listenerCount("prune-progress")).toBe(0);
  });

  it("dismissDone keeps the rest with default selection, or goes idle when empty", () => {
    usePruneStore.setState({
      phase: "done",
      result: { items_removed: 1, bytes_freed: 100, errors: [], cleaned_paths: [list[0].artifact_path] },
    });
    usePruneStore.getState().dismissDone();
    let s = usePruneStore.getState();
    expect(s.phase).toBe("list");
    expect(s.artifacts).toHaveLength(2);
    expect(s.selectedPaths).toEqual(new Set([list[1].artifact_path]));

    usePruneStore.setState({
      phase: "done",
      result: { items_removed: 2, bytes_freed: 25, errors: [], cleaned_paths: s.artifacts.map((a) => a.artifact_path) },
    });
    usePruneStore.getState().dismissDone();
    s = usePruneStore.getState();
    expect(s).toMatchObject({ phase: "idle", artifacts: [] });
  });
});
