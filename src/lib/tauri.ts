import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface SystemStats {
  cpu_usage: number;
  memory_total: number;
  memory_used: number;
  memory_percent: number;
  disk_total: number;
  disk_free: number;
}

export async function getSystemStats(): Promise<SystemStats> {
  return invoke<SystemStats>("get_system_stats");
}

// ── Clean Module Types ──────────────────────────────────

export interface PathInfo {
  path: string;
  size: number;
  is_dir: boolean;
}

export interface ScanItem {
  rule_id: string;
  category: string;
  label: string;
  paths: PathInfo[];
  total_size: number;
}

export interface CleanProgress {
  current_item: string;
  items_done: number;
  items_total: number;
  bytes_freed: number;
}

export interface CleanResult {
  items_cleaned: number;
  bytes_freed: number;
  errors: string[];
}

// ── Clean Module Commands ───────────────────────────────

export async function scanForCleanables(): Promise<ScanItem[]> {
  return invoke<ScanItem[]>("scan_for_cleanables");
}

export async function executeClean(
  ruleIds: string[],
  dryRun: boolean
): Promise<CleanResult> {
  return invoke<CleanResult>("execute_clean", {
    ruleIds,
    dryRun,
  });
}

export async function listenCleanProgress(
  callback: (progress: CleanProgress) => void
): Promise<UnlistenFn> {
  return listen<CleanProgress>("clean-progress", (event) => {
    callback(event.payload);
  });
}
