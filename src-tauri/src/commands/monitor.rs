use serde::Serialize;
use sysinfo::System;

#[derive(Serialize)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub disk_total: u64,
    pub disk_free: u64,
}

#[tauri::command]
pub fn get_system_stats() -> SystemStats {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_usage = sys.global_cpu_usage();

    let memory_total = sys.total_memory();
    let memory_used = sys.used_memory();
    let memory_percent = if memory_total > 0 {
        (memory_used as f32 / memory_total as f32) * 100.0
    } else {
        0.0
    };

    let mut disk_total: u64 = 0;
    let mut disk_free: u64 = 0;
    for disk in sysinfo::Disks::new_with_refreshed_list().iter() {
        if disk.mount_point() == std::path::Path::new("/") {
            disk_total = disk.total_space();
            disk_free = disk.available_space();
            break;
        }
    }

    SystemStats {
        cpu_usage,
        memory_total,
        memory_used,
        memory_percent,
        disk_total,
        disk_free,
    }
}
