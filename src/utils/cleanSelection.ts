import type { ScanItem } from "../lib/tauri";

export interface CleanSelection {
  nonZeroItems: ScanItem[];
  selectableIds: Set<string>;
  totalSize: number;
  selectedSize: number;
  selectedCount: number;
  allSelected: boolean;
}

/** Counts unique non-empty rule ids so duplicates or zero-size items can't inflate "N of M selected". */
export function cleanSelection(items: ScanItem[], selectedIds: Set<string>): CleanSelection {
  const nonZeroItems = items.filter((i) => i.total_size > 0);
  const selectableIds = new Set(nonZeroItems.map((i) => i.rule_id));
  const ids = [...selectableIds];
  return {
    nonZeroItems,
    selectableIds,
    totalSize: nonZeroItems.reduce((sum, i) => sum + i.total_size, 0),
    selectedSize: nonZeroItems.filter((i) => selectedIds.has(i.rule_id)).reduce((sum, i) => sum + i.total_size, 0),
    selectedCount: ids.filter((id) => selectedIds.has(id)).length,
    allSelected: ids.length > 0 && ids.every((id) => selectedIds.has(id)),
  };
}
