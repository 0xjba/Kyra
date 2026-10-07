import { describe, expect, it } from "vitest";
import type { ScanItem } from "../lib/tauri";
import { cleanSelection, uniqueSize } from "./cleanSelection";

const item = (rule_id: string, total_size: number, category = "Caches"): ScanItem => ({
  rule_id,
  category,
  label: rule_id,
  paths: [],
  total_size,
});

describe("cleanSelection", () => {
  it("never reports more selected than selectable when rule ids repeat", () => {
    const items = [item("a", 10), item("a", 5), item("b", 20), item("c", 30)];
    const s = cleanSelection(items, new Set(["a", "b", "c"]));
    expect(s.selectableIds.size).toBe(3);
    expect(s.selectedCount).toBe(3);
    expect(s.allSelected).toBe(true);
  });

  it("ignores zero-size items and stale ids in the selection", () => {
    const items = [item("a", 10), item("empty", 0)];
    const s = cleanSelection(items, new Set(["a", "empty", "gone"]));
    expect(s.selectableIds).toEqual(new Set(["a"]));
    expect(s.selectedCount).toBe(1);
    expect(s.selectedCount).toBeLessThanOrEqual(s.selectableIds.size);
    expect(s.nonZeroItems.map((i) => i.rule_id)).toEqual(["a"]);
  });

  it("sums total and selected sizes over non-empty items", () => {
    const items = [item("a", 10), item("b", 20), item("z", 0)];
    const s = cleanSelection(items, new Set(["b"]));
    expect(s.totalSize).toBe(30);
    expect(s.selectedSize).toBe(20);
    expect(s.allSelected).toBe(false);
  });

  it("is not all-selected when nothing is selectable", () => {
    const s = cleanSelection([item("z", 0)], new Set(["z"]));
    expect(s.allSelected).toBe(false);
    expect(s.selectedCount).toBe(0);
  });

  it("counts a folder inside a selected folder once", () => {
    const caches = withPaths("user_caches", [["/h/Library/Caches", 5000]]);
    const pip = withPaths("dev_pip_cache", [["/h/Library/Caches/pip", 1500]]);
    const warp = withPaths("shell_warp_cache", [["/h/Library/Caches/SentryCrash/Warp", 100]]);
    const lookalike = withPaths("caches_old", [["/h/Library/Caches Old", 70]]);
    const npm = withPaths("dev_npm_cache", [["/h/.npm/_cacache", 1500]]);
    const items = [caches, pip, warp, lookalike, npm];

    const all = cleanSelection(items, new Set(items.map((i) => i.rule_id)));
    expect(all.totalSize).toBe(5000 + 70 + 1500);
    expect(all.selectedSize).toBe(5000 + 70 + 1500);

    // Without the parent, each child counts on its own.
    const children = cleanSelection(items, new Set(["dev_pip_cache", "shell_warp_cache"]));
    expect(children.selectedSize).toBe(1600);
    expect(children.totalSize).toBe(all.totalSize);
  });

  it("counts a folder found by two rules once", () => {
    const a = withPaths("sys_coredevice_cache", [["/h/C/Caches", 700]]);
    const b = withPaths("dynamic_container_caches", [["/h/C/Caches", 700], ["/h/D/tmp", 9]]);
    expect(uniqueSize([a, b])).toBe(709);
  });

  it("keeps pseudo-paths and path-less items as they are", () => {
    const snaps = withPaths("special_tm_local_snapshots", [["tmutil://com.apple.TimeMachine.1.local", 300]]);
    expect(uniqueSize([snaps, item("legacy", 40)])).toBe(340);
  });
});

function withPaths(rule_id: string, paths: [string, number][]): ScanItem {
  return {
    rule_id,
    category: "Caches",
    label: rule_id,
    paths: paths.map(([path, size]) => ({ path, size, is_dir: true })),
    total_size: paths.reduce((s, [, size]) => s + size, 0),
  };
}
