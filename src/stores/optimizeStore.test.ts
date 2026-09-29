import { beforeEach, describe, expect, it } from "vitest";
import { emit, invokedWith, listenerCount, onInvoke } from "../test/tauri";
import { optTask } from "../test/fixtures";
import { useOptimizeStore } from "./optimizeStore";

const initial = useOptimizeStore.getState();
const tasks = [optTask("dns"), optTask("spotlight"), optTask("fonts")];
const okResult = { tasks_run: 1, tasks_succeeded: 1, tasks_failed: 0, tasks_skipped: 0 };

beforeEach(() => {
  useOptimizeStore.setState(initial, true);
});

async function loaded() {
  onInvoke("get_optimize_tasks", () => tasks);
  await useOptimizeStore.getState().loadTasks();
}

describe("loadTasks", () => {
  it("loads tasks as ready with nothing enabled", async () => {
    await loaded();
    const s = useOptimizeStore.getState();
    expect(s.tasks).toEqual(tasks);
    expect(s.enabledIds.size).toBe(0);
    expect(Object.values(s.statuses).every((t) => t.status === "ready")).toBe(true);
  });

  it("stores the error on failure", async () => {
    onInvoke("get_optimize_tasks", () => {
      throw new Error("x");
    });
    await useOptimizeStore.getState().loadTasks();
    expect(useOptimizeStore.getState().error).toContain("x");
  });
});

describe("selection", () => {
  it("toggles, enables all and disables all", async () => {
    await loaded();
    const s = useOptimizeStore.getState();
    s.toggleTask("dns");
    expect(useOptimizeStore.getState().enabledIds).toEqual(new Set(["dns"]));
    s.toggleTask("dns");
    expect(useOptimizeStore.getState().enabledIds.size).toBe(0);
    s.enableAll();
    expect(useOptimizeStore.getState().enabledIds.size).toBe(3);
    s.disableAll();
    expect(useOptimizeStore.getState().enabledIds.size).toBe(0);
  });
});

describe("running", () => {
  beforeEach(loaded);

  it("runSelected streams statuses and unchecks finished tasks", async () => {
    useOptimizeStore.setState({ enabledIds: new Set(["dns", "fonts"]) });
    onInvoke("run_optimize_tasks", () => {
      emit("optimize-status", { task_id: "dns", status: "running", message: null });
      expect(useOptimizeStore.getState().enabledIds.has("dns")).toBe(true);
      emit("optimize-status", { task_id: "dns", status: "done", message: "flushed" });
      emit("optimize-status", { task_id: "fonts", status: "error", message: "failed" });
      return { tasks_run: 2, tasks_succeeded: 1, tasks_failed: 1, tasks_skipped: 0 };
    });

    const pending = useOptimizeStore.getState().runSelected();
    expect(useOptimizeStore.getState().running).toBe(true);
    await pending;

    const s = useOptimizeStore.getState();
    expect(invokedWith("run_optimize_tasks")).toEqual([{ taskIds: ["dns", "fonts"] }]);
    expect(s.running).toBe(false);
    expect(s.result?.tasks_failed).toBe(1);
    expect(s.statuses.dns).toEqual({ status: "done", message: "flushed" });
    expect(s.statuses.fonts).toEqual({ status: "error", message: "failed" });
    expect(s.enabledIds.size).toBe(0);
    expect(listenerCount("optimize-status")).toBe(0);
  });

  it("runSelected is a no-op with nothing enabled", async () => {
    await useOptimizeStore.getState().runSelected();
    expect(invokedWith("run_optimize_tasks")).toHaveLength(0);
    expect(useOptimizeStore.getState().running).toBe(false);
  });

  it("clears running and records the error when the run fails", async () => {
    onInvoke("run_optimize_tasks", () => {
      throw new Error("admin denied");
    });
    await useOptimizeStore.getState().runSingle("dns");
    const s = useOptimizeStore.getState();
    expect(s.running).toBe(false);
    expect(s.error).toContain("admin denied");
    expect(listenerCount("optimize-status")).toBe(0);
  });

  it("runSingle and runTaskIds send the given ids", async () => {
    onInvoke("run_optimize_tasks", () => okResult);
    await useOptimizeStore.getState().runSingle("spotlight");
    await useOptimizeStore.getState().runTaskIds(["dns", "fonts"]);
    await useOptimizeStore.getState().runTaskIds([]);
    expect(invokedWith("run_optimize_tasks")).toEqual([{ taskIds: ["spotlight"] }, { taskIds: ["dns", "fonts"] }]);
  });

  it("markSkipped and reset", () => {
    useOptimizeStore.getState().markSkipped(["dns"], "needs admin");
    expect(useOptimizeStore.getState().statuses.dns).toEqual({ status: "skipped", message: "needs admin" });
    useOptimizeStore.setState({ running: true, result: okResult, error: "e" });
    useOptimizeStore.getState().reset();
    const s = useOptimizeStore.getState();
    expect(s).toMatchObject({ running: false, result: null, error: null });
    expect(s.statuses.dns).toEqual({ status: "ready" });
  });
});
