import type { ScanItem } from "../lib/tauri";

export interface CleanSelection {
  nonZeroItems: ScanItem[];
  selectableIds: Set<string>;
  totalSize: number;
  selectedSize: number;
  selectedCount: number;
  allSelected: boolean;
}

/** Pseudo-paths (snapshots, simulators) aren't folders and never nest. */
const isFsPath = (path: string) => path.startsWith("/");

/**
 * Bytes freed by cleaning `items` together. Rules overlap: User Caches lists
 * ~/Library/Caches while Pip lists ~/Library/Caches/pip, and one folder can
 * be found by two rules. A path is counted once, and not at all when a
 * folder containing it is also in the set, since that folder's size already
 * includes it.
 */
export function uniqueSize(items: ScanItem[]): number {
  const sizes = new Map<string, number>();
  for (const item of items) {
    if (item.total_size <= 0) continue;
    if (item.paths.length === 0) {
      sizes.set(`rule:${item.rule_id}`, item.total_size);
      continue;
    }
    for (const p of item.paths) {
      if (!sizes.has(p.path)) sizes.set(p.path, p.size);
    }
  }
  let total = 0;
  for (const [path, size] of sizes) {
    if (isFsPath(path) && hasListedAncestor(path, sizes)) continue;
    total += size;
  }
  return total;
}

function hasListedAncestor(path: string, listed: Map<string, number>): boolean {
  let i = path.lastIndexOf("/");
  while (i > 0) {
    if (listed.has(path.slice(0, i))) return true;
    i = path.lastIndexOf("/", i - 1);
  }
  return false;
}

/** Counts unique non-empty rule ids so duplicates or zero-size items can't inflate "N of M selected". */
export function cleanSelection(items: ScanItem[], selectedIds: Set<string>): CleanSelection {
  const nonZeroItems = items.filter((i) => i.total_size > 0);
  const selectableIds = new Set(nonZeroItems.map((i) => i.rule_id));
  const ids = [...selectableIds];
  return {
    nonZeroItems,
    selectableIds,
    totalSize: uniqueSize(nonZeroItems),
    selectedSize: uniqueSize(nonZeroItems.filter((i) => selectedIds.has(i.rule_id))),
    selectedCount: ids.filter((id) => selectedIds.has(id)).length,
    allSelected: ids.length > 0 && ids.every((id) => selectedIds.has(id)),
  };
}
