import { describe, expect, it } from "vitest";
import type { ScanItem } from "../lib/tauri";
import { cleanSelection } from "./cleanSelection";

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
});
