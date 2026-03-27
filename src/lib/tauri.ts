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

export interface DetailedStats {
  cpu_usage: number;
  cpu_cores: number[];
  memory_total: number;
  memory_used: number;
  memory_percent: number;
  disk_total: number;
  disk_used: number;
  disk_free: number;
  disk_percent: number;
  net_upload: number;
  net_download: number;
}

export async function startStatsStream(): Promise<void> {
  return invoke<void>("start_stats_stream");
}

export async function listenStatsTick(
  callback: (stats: DetailedStats) => void
): Promise<UnlistenFn> {
  return listen<DetailedStats>("system-stats-tick", (event) => {
    callback(event.payload);
  });
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

// ── Optimize Module Types ───────────────────────────────

export interface OptTask {
  id: string;
  name: string;
  description: string;
  command: string;
  needs_admin: boolean;
  warning: string | null;
}

export interface OptTaskStatus {
  task_id: string;
  status: "running" | "done" | "error" | "skipped";
  message: string | null;
}

export interface OptResult {
  tasks_run: number;
  tasks_succeeded: number;
  tasks_failed: number;
  tasks_skipped: number;
}

// ── Optimize Module Commands ────────────────────────────

export async function getOptimizeTasks(): Promise<OptTask[]> {
  return invoke<OptTask[]>("get_optimize_tasks");
}

export async function runOptimizeTasks(
  taskIds: string[]
): Promise<OptResult> {
  return invoke<OptResult>("run_optimize_tasks", { taskIds });
}

export async function listenOptimizeStatus(
  callback: (status: OptTaskStatus) => void
): Promise<UnlistenFn> {
  return listen<OptTaskStatus>("optimize-status", (event) => {
    callback(event.payload);
  });
}

// ── Uninstall Module Types ────────────────────────────────

export interface AppInfo {
  bundle_id: string;
  name: string;
  version: string;
  path: string;
  size: number;
}

export interface AssociatedFile {
  path: string;
  category: string;
  size: number;
  is_dir: boolean;
}

export interface UninstallProgress {
  current_item: string;
  items_done: number;
  items_total: number;
  bytes_freed: number;
}

export interface UninstallResult {
  items_removed: number;
  bytes_freed: number;
  errors: string[];
}

// ── Uninstall Module Commands ─────────────────────────────

export async function scanInstalledApps(): Promise<AppInfo[]> {
  return invoke<AppInfo[]>("scan_installed_apps");
}

export async function getAssociatedFiles(
  bundleId: string,
  appName: string,
  appPath: string
): Promise<AssociatedFile[]> {
  return invoke<AssociatedFile[]>("get_associated_files", {
    bundleId,
    appName,
    appPath,
  });
}

export async function executeUninstall(
  appPath: string,
  filePaths: string[],
  dryRun: boolean
): Promise<UninstallResult> {
  return invoke<UninstallResult>("execute_uninstall", {
    appPath,
    filePaths,
    dryRun,
  });
}

export async function listenUninstallProgress(
  callback: (progress: UninstallProgress) => void
): Promise<UnlistenFn> {
  return listen<UninstallProgress>("uninstall-progress", (event) => {
    callback(event.payload);
  });
}

// ── Analyze Module Types ──────────────────────────────────

export interface DirNode {
  name: string;
  path: string;
  size: number;
  is_dir: boolean;
  children: DirNode[];
}

export interface AnalyzeScanProgress {
  current_path: string;
  files_scanned: number;
  total_size: number;
}

// ── Analyze Module Commands ───────────────────────────────

export async function analyzePath(
  path: string,
  depth: number
): Promise<DirNode> {
  return invoke<DirNode>("analyze_path", { path, depth });
}

export async function revealInFinder(path: string): Promise<void> {
  return invoke<void>("reveal_in_finder", { path });
}

export async function listenAnalyzeProgress(
  callback: (progress: AnalyzeScanProgress) => void
): Promise<UnlistenFn> {
  return listen<AnalyzeScanProgress>("analyze-progress", (event) => {
    callback(event.payload);
  });
}
