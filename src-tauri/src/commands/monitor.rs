use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use sysinfo::{Networks, System};
use tauri::{Emitter, State};

#[derive(Serialize)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub disk_total: u64,
    pub disk_free: u64,
}

pub struct StatsStreamActive(pub AtomicBool);

#[derive(Clone, Serialize)]
pub struct DetailedStats {
    pub cpu_usage: f32,
    pub cpu_cores: Vec<f32>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_free: u64,
    pub disk_percent: f32,
    pub net_upload: u64,
    pub net_download: u64,
}

pub struct SystemMonitor(pub Mutex<System>);

#[tauri::command]
pub fn get_system_stats(monitor: State<'_, SystemMonitor>) -> SystemStats {
    let mut sys = monitor.0.lock().unwrap();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

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

#[tauri::command]
pub async fn start_stats_stream(
    app: tauri::AppHandle,
    active: State<'_, StatsStreamActive>,
) -> Result<(), ()> {
    // Prevent spawning multiple background loops
    if active.0.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    tauri::async_runtime::spawn(async move {
        let mut sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();
        let mut disks = sysinfo::Disks::new_with_refreshed_list();
        let mut prev_sent: u64 = networks.iter().map(|(_, n)| n.total_transmitted()).sum();
        let mut prev_recv: u64 = networks.iter().map(|(_, n)| n.total_received()).sum();
        let mut tick_count: u32 = 0;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            sys.refresh_cpu_usage();
            sys.refresh_memory();
            networks.refresh();

            let cpu_usage = sys.global_cpu_usage();
            let cpu_cores: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();

            let memory_total = sys.total_memory();
            let memory_used = sys.used_memory();
            let memory_percent = if memory_total > 0 {
                (memory_used as f32 / memory_total as f32) * 100.0
            } else {
                0.0
            };

            // Refresh disk info every 30 seconds instead of every tick
            tick_count += 1;
            if tick_count % 30 == 0 {
                disks.refresh();
            }

            let mut disk_total: u64 = 0;
            let mut disk_free: u64 = 0;
            for disk in disks.iter() {
                if disk.mount_point() == std::path::Path::new("/") {
                    disk_total = disk.total_space();
                    disk_free = disk.available_space();
                    break;
                }
            }
            let disk_used = disk_total.saturating_sub(disk_free);
            let disk_percent = if disk_total > 0 {
                (disk_used as f32 / disk_total as f32) * 100.0
            } else {
                0.0
            };

            let curr_sent: u64 = networks.iter().map(|(_, n)| n.total_transmitted()).sum();
            let curr_recv: u64 = networks.iter().map(|(_, n)| n.total_received()).sum();
            let net_upload = curr_sent.saturating_sub(prev_sent);
            let net_download = curr_recv.saturating_sub(prev_recv);
            prev_sent = curr_sent;
            prev_recv = curr_recv;

            let stats = DetailedStats {
                cpu_usage,
                cpu_cores,
                memory_total,
                memory_used,
                memory_percent,
                disk_total,
                disk_used,
                disk_free,
                disk_percent,
                net_upload,
                net_download,
            };

            let _ = app.emit("system-stats-tick", &stats);

            // Update tray title with compact stats
            if let Some(tray) = app.tray_by_id("main-tray") {
                let title = format!(
                    "CPU {:.0}%  Mem {:.0}%",
                    cpu_usage, memory_percent
                );
                let _ = tray.set_title(Some(&title));
            }
        }
    });

    Ok(())
}
