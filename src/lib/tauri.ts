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

export interface NetworkInterface {
  name: string;
  upload: number;
  download: number;
}

export interface TopProcess {
  name: string;
  cpu: number;
  memory: number;
}

export interface DetailedStats {
  cpu_usage: number;
  cpu_cores: number[];
  memory_total: number;
  memory_used: number;
  memory_percent: number;
  memory_pressure: string;
  swap_total: number;
  swap_used: number;
  disk_total: number;
  disk_used: number;
  disk_free: number;
  disk_percent: number;
  net_upload: number;
  net_download: number;
  network_interfaces: NetworkInterface[];
  battery_percent: number;
  battery_charging: boolean;
  battery_time_remaining: string;
  battery_health: string;
  battery_cycle_count: number;
  gpu_name: string;
  gpu_vram: string;
  thermal_pressure: string;
  cpu_temp: number;
  gpu_temp: number;
  ssd_temp: number;
  top_processes: TopProcess[];
  uptime_secs: number;
  device_name: string;
  os_version: string;
}

export async function startStatsStream(): Promise<void> {
  return invoke<void>("start_stats_stream");
}

export async function stopStatsStream(): Promise<void> {
  return invoke<void>("stop_stats_stream");
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
  items: ScanItem[],
  dryRun: boolean,
  permanent: boolean
): Promise<CleanResult> {
  return invoke<CleanResult>("execute_clean", {
    items,
    dryRun,
    permanent,
  });
}

export async function runBrewCleanup(): Promise<string> {
  return invoke<string>("run_brew_cleanup");
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
  is_system: boolean;
  is_data_sensitive: boolean;
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
  dryRun: boolean,
  permanent: boolean
): Promise<UninstallResult> {
  return invoke<UninstallResult>("execute_uninstall", {
    appPath,
    filePaths,
    dryRun,
    permanent,
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
  is_cleanable: boolean;
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

export async function deleteAnalyzedItem(
  path: string,
  permanent: boolean
): Promise<number> {
  return invoke<number>("delete_analyzed_item", { path, permanent });
}

export interface LargeFile {
  name: string;
  path: string;
  size: number;
}

export async function findLargeFiles(minSizeMb: number): Promise<LargeFile[]> {
  return invoke<LargeFile[]>("find_large_files", { minSizeMb });
}

// ── Purge Module Types ──────────────────────────────────

export interface ArtifactEntry {
  project_name: string;
  project_path: string;
  artifact_type: string;
  artifact_path: string;
  size: number;
}

export interface PurgeScanProgress {
  current_path: string;
  artifacts_found: number;
}

export interface PurgeProgress {
  current_item: string;
  items_done: number;
  items_total: number;
  bytes_freed: number;
}

export interface PurgeResult {
  items_removed: number;
  bytes_freed: number;
  errors: string[];
}

// ── Purge Module Commands ───────────────────────────────

export async function scanArtifacts(rootPath: string): Promise<ArtifactEntry[]> {
  return invoke<ArtifactEntry[]>("scan_artifacts", { rootPath });
}

export async function executePurge(
  artifactPaths: string[],
  dryRun: boolean,
  permanent: boolean
): Promise<PurgeResult> {
  return invoke<PurgeResult>("execute_purge", { artifactPaths, dryRun, permanent });
}

export async function listenPurgeScanProgress(
  callback: (progress: PurgeScanProgress) => void
): Promise<UnlistenFn> {
  return listen<PurgeScanProgress>("purge-scan-progress", (event) => {
    callback(event.payload);
  });
}

export async function listenPurgeProgress(
  callback: (progress: PurgeProgress) => void
): Promise<UnlistenFn> {
  return listen<PurgeProgress>("purge-progress", (event) => {
    callback(event.payload);
  });
}

// ── Installers Module Types ──────────────────────────────

export interface InstallerFile {
  name: string;
  path: string;
  extension: string;
  size: number;
  modified_secs: number;
}

export interface InstallerProgress {
  current_item: string;
  items_done: number;
  items_total: number;
  bytes_freed: number;
}

export interface InstallerResult {
  items_removed: number;
  bytes_freed: number;
  errors: string[];
}

// ── Installers Module Commands ───────────────────────────

export async function scanInstallers(): Promise<InstallerFile[]> {
  return invoke<InstallerFile[]>("scan_installers");
}

export async function deleteInstallers(
  filePaths: string[],
  dryRun: boolean,
  permanent: boolean
): Promise<InstallerResult> {
  return invoke<InstallerResult>("delete_installers", { filePaths, dryRun, permanent });
}

export async function listenInstallerProgress(
  callback: (progress: InstallerProgress) => void
): Promise<UnlistenFn> {
  return listen<InstallerProgress>("installer-progress", (event) => {
    callback(event.payload);
  });
}

// ── Settings ─────────────────────────────────────────────

export interface AppSettings {
  dry_run: boolean;
  whitelist: string[];
  use_trash: boolean;
  large_file_threshold_mb: number;
  analyze_scan_depth: number;
}

export async function loadSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("load_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke<void>("save_settings", { settings });
}

export async function getTotalBytesFreed(): Promise<number> {
  return invoke<number>("get_total_bytes_freed");
}

export async function addBytesFreed(bytes: number): Promise<number> {
  return invoke<number>("add_bytes_freed", { bytes });
}

export async function addToWhitelist(path: string): Promise<void> {
  return invoke<void>("add_to_whitelist", { path });
}

export async function removeFromWhitelist(path: string): Promise<void> {
  return invoke<void>("remove_from_whitelist", { path });
}

export async function resetLifetimeStats(): Promise<void> {
  return invoke<void>("reset_lifetime_stats");
}

export async function getStoragePath(): Promise<string> {
  return invoke<string>("get_storage_path");
}

export async function pickFolder(): Promise<string | null> {
  return invoke<string | null>("pick_folder");
}

export async function checkSipStatus(): Promise<boolean> {
  return invoke<boolean>("check_sip_status");
}

// ── Shared Utilities ─────────────────────────────────────

export async function checkFullDiskAccess(): Promise<boolean> {
  return invoke<boolean>("check_full_disk_access");
}

// ── Process Detection ────────────────────────────────────

export interface RunningApp {
  name: string;
  rule_ids: string[];
}

export async function checkRunningProcesses(
  ruleIds: string[]
): Promise<RunningApp[]> {
  return invoke<RunningApp[]>("check_running_processes", { ruleIds });
}

// ── App Icons ───────────────────────────────────────────

export async function getAppIcon(appName: string): Promise<string | null> {
  return invoke<string | null>("get_app_icon", { appName });
}

export async function getAppIconByPath(appPath: string): Promise<string | null> {
  return invoke<string | null>("get_app_icon_by_path", { appPath });
}
