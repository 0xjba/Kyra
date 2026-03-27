import { invoke } from "@tauri-apps/api/core";

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
