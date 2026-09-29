import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flush, invokedWith, listenerCount, onInvoke } from "../test/tauri";
import { deferred } from "../test/fixtures";
import type { DirNode, LargeFile } from "../lib/tauri";
import { useAnalyzeStore } from "./analyzeStore";
import { useSettingsStore } from "./settingsStore";

const initial = useAnalyzeStore.getState();
const initialSettings = useSettingsStore.getState();

const node = (path: string, size: number, children: DirNode[] = []): DirNode => ({
  name: path.split("/").pop() || "/",
  path,
  size,
  is_dir: children.length > 0,
  is_cleanable: false,
  children,
});

const tree = () =>
  node("/Users/me", 1000, [
    node("/Users/me/Library", 700, [node("/Users/me/Library/Caches", 400, [node("/Users/me/Library/Caches/big", 400)]), node("/Users/me/Library/Logs", 300)]),
    node("/Users/me/Movies", 300),
  ]);

const bigFiles = (n: number): LargeFile[] =>
  Array.from({ length: n }, (_, i) => ({ name: `f${i}`, path: `/Users/me/f${i}`, size: (i + 1) * 1000 }));

function setThreshold(mb: number) {
  useSettingsStore.setState({ settings: { ...useSettingsStore.getState().settings, large_file_threshold_mb: mb } });
}

beforeEach(() => {
  useAnalyzeStore.setState(initial, true);
  useSettingsStore.setState(initialSettings, true);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("scan", () => {
  it("scans with the configured depth and caches the tree for five minutes", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(1_000_000);
    useSettingsStore.setState({ settings: { ...initialSettings.settings, analyze_scan_depth: 5 } });
    onInvoke("analyze_path", () => tree());
    useAnalyzeStore.getState().setScanPath("/Users/me");

    await useAnalyzeStore.getState().scan();
    expect(invokedWith("analyze_path")).toEqual([{ path: "/Users/me", depth: 5 }]);
    expect(useAnalyzeStore.getState().phase).toBe("ready");
    expect(useAnalyzeStore.getState().current?.path).toBe("/Users/me");
    expect(listenerCount("analyze-progress")).toBe(0);

    vi.setSystemTime(1_000_000 + 4 * 60_000);
    await useAnalyzeStore.getState().scan();
    expect(invokedWith("analyze_path")).toHaveLength(1);

    vi.setSystemTime(1_000_000 + 6 * 60_000);
    await useAnalyzeStore.getState().scan();
    expect(invokedWith("analyze_path")).toHaveLength(2);
  });

  it("returns to idle with the error on failure", async () => {
    onInvoke("analyze_path", () => {
      throw new Error("EPERM");
    });
    await useAnalyzeStore.getState().scan();
    expect(useAnalyzeStore.getState().phase).toBe("idle");
    expect(useAnalyzeStore.getState().error).toContain("EPERM");
  });
});

describe("navigation", () => {
  it("drills in, up, to an index and back to root", () => {
    const root = tree();
    useAnalyzeStore.setState({ phase: "ready", root, current: root });
    const s = useAnalyzeStore.getState();
    const lib = root.children[0];
    s.drillInto(lib);
    s.drillInto(lib.children[0]);
    expect(useAnalyzeStore.getState().breadcrumb.map((n) => n.path)).toEqual(["/Users/me", "/Users/me/Library"]);

    s.drillInto(lib.children[1]);
    expect(useAnalyzeStore.getState().current?.path).toBe("/Users/me/Library/Caches");

    s.drillUp();
    expect(useAnalyzeStore.getState().current?.path).toBe("/Users/me/Library");
    s.drillInto(lib.children[0]);
    s.drillToIndex(0);
    expect(useAnalyzeStore.getState()).toMatchObject({ current: root, breadcrumb: [] });
    s.drillInto(lib);
    s.drillToRoot();
    expect(useAnalyzeStore.getState()).toMatchObject({ current: root, breadcrumb: [] });
  });

  it("removeNodeByPath shrinks ancestors, keeps position and invalidates the cache", () => {
    const root = tree();
    useAnalyzeStore.setState({
      phase: "ready",
      scanPath: "/Users/me",
      root,
      current: root.children[0],
      breadcrumb: [root],
      scanCache: { "/Users/me": { root, timestamp: Date.now() } },
    });
    useAnalyzeStore.getState().removeNodeByPath("/Users/me/Library/Caches/big", 400);
    const s = useAnalyzeStore.getState();
    expect(s.root?.size).toBe(600);
    expect(s.current?.path).toBe("/Users/me/Library");
    expect(s.current?.size).toBe(300);
    expect(s.current?.children.find((c) => c.path.endsWith("Caches"))?.children).toEqual([]);
    expect(s.scanCache["/Users/me"]).toBeUndefined();
  });
});

describe("large files", () => {
  beforeEach(() => {
    useAnalyzeStore.setState({ scanPath: "/Users/me" });
    setThreshold(250);
  });

  it("sorts by size and passes threshold and path", async () => {
    onInvoke("find_large_files", () => bigFiles(3));
    await useAnalyzeStore.getState().loadLargeFiles();
    const s = useAnalyzeStore.getState();
    expect(invokedWith("find_large_files")).toEqual([{ minSizeMb: 250, searchPath: "/Users/me" }]);
    expect(s.largeFiles.map((f) => f.name)).toEqual(["f2", "f1", "f0"]);
    expect(s).toMatchObject({ largeFilesLoading: false, largeFilesCapped: false, largeFilesKey: "/Users/me|250" });
  });

  it("flags the result as capped at 50 files", async () => {
    onInvoke("find_large_files", () => bigFiles(49));
    await useAnalyzeStore.getState().loadLargeFiles();
    expect(useAnalyzeStore.getState().largeFilesCapped).toBe(false);
    onInvoke("find_large_files", () => bigFiles(50));
    await useAnalyzeStore.getState().loadLargeFiles();
    expect(useAnalyzeStore.getState().largeFilesCapped).toBe(true);
  });

  it("only re-queries when the path or threshold changes", async () => {
    onInvoke("find_large_files", () => bigFiles(2));
    const s = useAnalyzeStore.getState();
    await s.ensureLargeFiles();
    await s.ensureLargeFiles();
    expect(invokedWith("find_large_files")).toHaveLength(1);

    setThreshold(500);
    await s.ensureLargeFiles();
    expect(invokedWith("find_large_files")).toHaveLength(2);

    useAnalyzeStore.getState().setScanPath("/");
    await s.ensureLargeFiles();
    expect(invokedWith("find_large_files")).toHaveLength(3);
    expect(useAnalyzeStore.getState().largeFilesKey).toBe("/|500");
  });

  it("does not duplicate an in-flight query for the same key", async () => {
    const d = deferred<LargeFile[]>();
    onInvoke("find_large_files", () => d.promise);
    useAnalyzeStore.getState().setActiveTab("large-files");
    await flush();
    await useAnalyzeStore.getState().ensureLargeFiles();
    expect(invokedWith("find_large_files")).toHaveLength(1);
    d.resolve(bigFiles(1));
    await flush();
    expect(useAnalyzeStore.getState().largeFiles).toHaveLength(1);
  });

  it("ignores a stale response that lands after a newer query", async () => {
    const slow = deferred<LargeFile[]>();
    const fast = deferred<LargeFile[]>();
    const queue = [slow.promise, fast.promise];
    onInvoke("find_large_files", () => queue.shift());

    const first = useAnalyzeStore.getState().loadLargeFiles();
    setThreshold(500);
    const second = useAnalyzeStore.getState().loadLargeFiles();
    fast.resolve([{ name: "new", path: "/new", size: 9 }]);
    await second;
    slow.resolve([{ name: "old", path: "/old", size: 1 }]);
    await first;

    const s = useAnalyzeStore.getState();
    expect(s.largeFiles.map((f) => f.name)).toEqual(["new"]);
    expect(s.largeFilesKey).toBe("/Users/me|500");
  });

  it("clears results on failure so the next visit retries", async () => {
    onInvoke("find_large_files", () => {
      throw new Error("x");
    });
    await useAnalyzeStore.getState().loadLargeFiles();
    expect(useAnalyzeStore.getState()).toMatchObject({ largeFiles: [], largeFilesKey: null, largeFilesLoading: false });
  });

  it("removeLargeFile drops the path and anything under it", () => {
    useAnalyzeStore.setState({
      largeFiles: [
        { name: "a", path: "/x/a", size: 1 },
        { name: "b", path: "/x/a/b", size: 1 },
        { name: "ab", path: "/x/ab", size: 1 },
      ],
    });
    useAnalyzeStore.getState().removeLargeFile("/x/a");
    expect(useAnalyzeStore.getState().largeFiles.map((f) => f.path)).toEqual(["/x/ab"]);
  });
});
