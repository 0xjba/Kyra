use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use sysinfo::{Networks, System};
use tauri::{Emitter, State};

#[derive(Clone, Serialize)]
pub struct TopProcess {
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
}

#[derive(Serialize)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub disk_total: u64,
    pub disk_free: u64,
}

pub struct StatsStreamActive(pub Arc<AtomicBool>);

#[derive(Clone, Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub upload: u64,
    pub download: u64,
}

#[derive(Clone, Serialize)]
pub struct DetailedStats {
    pub cpu_usage: f32,
    pub cpu_cores: Vec<f32>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub memory_pressure: String,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_free: u64,
    pub disk_percent: f32,
    pub net_upload: u64,
    pub net_download: u64,
    pub network_interfaces: Vec<NetworkInterface>,
    pub battery_percent: f32,
    pub battery_charging: bool,
    pub battery_time_remaining: String,
    pub battery_health: String,
    pub battery_cycle_count: i32,
    pub gpu_name: String,
    pub gpu_vram: String,
    pub thermal_pressure: String,
    pub cpu_temp: f32,
    pub gpu_temp: f32,
    pub ssd_temp: f32,
    pub top_processes: Vec<TopProcess>,
    pub uptime_secs: u64,
    pub device_name: String,
    pub os_version: String,
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

    let active_flag = active.0.clone();

    tauri::async_runtime::spawn(async move {
        let mut sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();
        let mut disks = sysinfo::Disks::new_with_refreshed_list();
        let mut prev_sent: u64 = networks.iter().map(|(_, n)| n.total_transmitted()).sum();
        let mut prev_recv: u64 = networks.iter().map(|(_, n)| n.total_received()).sum();
        // Per-interface previous values for rate calculation
        let mut prev_per_iface: std::collections::HashMap<String, (u64, u64)> = networks
            .iter()
            .map(|(name, n)| (name.to_string(), (n.total_transmitted(), n.total_received())))
            .collect();
        let mut tick_count: u32 = 0;

        // Cached slow data
        let mut cached_memory_pressure = String::from("normal");
        let mut cached_battery_health = String::from("N/A");
        let mut cached_battery_cycle_count: i32 = -1;
        let mut cached_gpu_name = String::from("Unknown");
        let mut cached_gpu_vram = String::from("N/A");
        let mut cached_thermal_pressure = String::from("nominal");
        let mut cached_cpu_temp: f32 = -1.0;
        let mut cached_gpu_temp: f32 = -1.0;
        let mut cached_ssd_temp: f32 = -1.0;
        let mut cached_top_processes: Vec<TopProcess> = Vec::new();

        // Static system info (computed once)
        let device_name = parse_device_name();
        let os_version = System::os_version().unwrap_or_default();

        // Noise interfaces to filter out
        let noise_prefixes = [
            "lo", "awdl", "utun", "llw", "bridge", "gif", "stf", "xhc", "anpi", "ap",
        ];

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            // Check if we should stop
            if !active_flag.load(Ordering::SeqCst) {
                break;
            }

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

            // Swap usage
            let swap_total = sys.total_swap();
            let swap_used = sys.used_swap();

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

            // ── Aggregate network ──
            let curr_sent: u64 = networks.iter().map(|(_, n)| n.total_transmitted()).sum();
            let curr_recv: u64 = networks.iter().map(|(_, n)| n.total_received()).sum();
            let net_upload = curr_sent.saturating_sub(prev_sent);
            let net_download = curr_recv.saturating_sub(prev_recv);
            prev_sent = curr_sent;
            prev_recv = curr_recv;

            // ── Per-interface network (top 3 active, filtering noise) ──
            let mut iface_stats: Vec<NetworkInterface> = Vec::new();
            let mut new_prev_per_iface: std::collections::HashMap<String, (u64, u64)> =
                std::collections::HashMap::new();
            for (name, data) in networks.iter() {
                let name_str = name.to_string();
                let curr_tx = data.total_transmitted();
                let curr_rx = data.total_received();
                let (prev_tx, prev_rx) = prev_per_iface
                    .get(&name_str)
                    .copied()
                    .unwrap_or((curr_tx, curr_rx));
                let ul = curr_tx.saturating_sub(prev_tx);
                let dl = curr_rx.saturating_sub(prev_rx);
                new_prev_per_iface.insert(name_str.clone(), (curr_tx, curr_rx));

                // Filter out noise interfaces
                let is_noise = noise_prefixes
                    .iter()
                    .any(|prefix| name_str.starts_with(prefix));
                if !is_noise && (ul > 0 || dl > 0) {
                    iface_stats.push(NetworkInterface {
                        name: name_str,
                        upload: ul,
                        download: dl,
                    });
                }
            }
            prev_per_iface = new_prev_per_iface;
            // Sort by total activity descending, keep top 3
            iface_stats.sort_by(|a, b| {
                (b.upload + b.download).cmp(&(a.upload + a.download))
            });
            iface_stats.truncate(3);

            // ── Battery info (pmset every tick, it's fast) ──
            let (battery_percent, battery_charging, battery_time_remaining) = parse_battery_pmset();

            // ── Battery health + cycle count (slow, every 60 ticks) ──
            if tick_count % 60 == 1 {
                let (health, cycles) = parse_battery_profiler();
                cached_battery_health = health;
                cached_battery_cycle_count = cycles;
            }

            // ── Memory pressure (slow, every 10 ticks) ──
            if tick_count % 10 == 1 {
                cached_memory_pressure = parse_memory_pressure();
            }

            // ── GPU info (very slow, every 60 ticks) ──
            if tick_count % 60 == 1 {
                let (gn, gv) = parse_gpu_info();
                cached_gpu_name = gn;
                cached_gpu_vram = gv;
            }

            // ── Thermal pressure + temperatures (every 10 ticks) ──
            if tick_count % 10 == 1 {
                cached_thermal_pressure = parse_thermal_pressure();
                let (ct, gt, st) = smc::read_temperatures();
                cached_cpu_temp = ct;
                cached_gpu_temp = gt;
                cached_ssd_temp = st;
            }

            // ── Top processes (every 2 ticks) ──
            if tick_count % 2 == 0 {
                sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
                cached_top_processes = get_top_processes(&sys);
            }

            let stats = DetailedStats {
                cpu_usage,
                cpu_cores,
                memory_total,
                memory_used,
                memory_percent,
                memory_pressure: cached_memory_pressure.clone(),
                swap_total,
                swap_used,
                disk_total,
                disk_used,
                disk_free,
                disk_percent,
                net_upload,
                net_download,
                network_interfaces: iface_stats,
                battery_percent,
                battery_charging,
                battery_time_remaining,
                battery_health: cached_battery_health.clone(),
                battery_cycle_count: cached_battery_cycle_count,
                gpu_name: cached_gpu_name.clone(),
                gpu_vram: cached_gpu_vram.clone(),
                thermal_pressure: cached_thermal_pressure.clone(),
                cpu_temp: cached_cpu_temp,
                gpu_temp: cached_gpu_temp,
                ssd_temp: cached_ssd_temp,
                top_processes: cached_top_processes.clone(),
                uptime_secs: System::uptime(),
                device_name: device_name.clone(),
                os_version: os_version.clone(),
            };

            let _ = app.emit("system-stats-tick", &stats);

            // Tray title removed for performance
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_stats_stream(active: State<'_, StatsStreamActive>) -> Result<(), ()> {
    active.0.store(false, Ordering::SeqCst);
    Ok(())
}

/// Parse GPU info from `system_profiler SPDisplaysDataType`.
fn parse_gpu_info() -> (String, String) {
    use std::process::Command;
    let output = Command::new("system_profiler")
        .args(["SPDisplaysDataType"])
        .output();
    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut name = String::from("Unknown");
            let mut vram = String::from("N/A");
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Chipset Model:") || trimmed.starts_with("Chip:") {
                    name = trimmed
                        .split(':')
                        .nth(1)
                        .unwrap_or("Unknown")
                        .trim()
                        .to_string();
                }
                if trimmed.starts_with("VRAM") || trimmed.contains("Total Number of Cores") {
                    vram = trimmed
                        .split(':')
                        .nth(1)
                        .unwrap_or("N/A")
                        .trim()
                        .to_string();
                }
            }
            (name, vram)
        }
        Err(_) => ("Unknown".into(), "N/A".into()),
    }
}

/// Parse device model name from `system_profiler SPHardwareDataType`.
fn parse_device_name() -> String {
    use std::process::Command;
    let output = Command::new("system_profiler")
        .arg("SPHardwareDataType")
        .output();
    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Model Name:") {
                    return trimmed
                        .split(':')
                        .nth(1)
                        .unwrap_or("Mac")
                        .trim()
                        .to_string();
                }
            }
            "Mac".into()
        }
        Err(_) => "Mac".into(),
    }
}

/// Parse thermal pressure from `pmset -g therm`.
fn parse_thermal_pressure() -> String {
    use std::process::Command;
    let output = Command::new("pmset")
        .args(["-g", "therm"])
        .output();
    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            if text.contains("CPU_Scheduler_Limit") && text.contains("100") {
                "nominal".into()
            } else if text.contains("CPU_Scheduler_Limit") {
                "throttled".into()
            } else {
                "nominal".into()
            }
        }
        Err(_) => "unknown".into(),
    }
}

/// ── SMC Temperature Reading via IOKit ──────────────────────────────────
/// Reads CPU, GPU, and SSD temperatures directly from the Apple SMC.
/// Works on Apple Silicon (M1-M4+) without sudo.
/// Returns (cpu_temp, gpu_temp, ssd_temp) in Celsius. -1.0 means unavailable.

mod smc {
    use std::mem::size_of;
    use std::os::raw::c_uint;

    type IOReturn = i32;
    type MachPort = c_uint;

    #[allow(non_upper_case_globals)]
    const kIOReturnSuccess: IOReturn = 0;

    // ── KeyData struct — must match Apple's SMC user-client layout exactly ──

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct KeyDataVer {
        major: u8,
        minor: u8,
        build: u8,
        reserved: u8,
        release: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct PLimitData {
        version: u16,
        length: u16,
        cpu_p_limit: u32,
        gpu_p_limit: u32,
        mem_p_limit: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct KeyInfo {
        data_size: u32,
        data_type: u32,
        data_attributes: u8,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeyData {
        key: u32,
        vers: KeyDataVer,
        p_limit_data: PLimitData,
        key_info: KeyInfo,
        result: u8,
        status: u8,
        data8: u8,
        data32: u32,
        bytes: [u8; 32],
    }

    impl Default for KeyData {
        fn default() -> Self {
            unsafe { std::mem::zeroed() }
        }
    }

    extern "C" {
        fn mach_task_self() -> MachPort;
        fn IOServiceOpen(service: MachPort, task: MachPort, typ: u32, conn: *mut MachPort) -> IOReturn;
        fn IOServiceClose(conn: MachPort) -> IOReturn;
        fn IOConnectCallStructMethod(
            conn: MachPort, selector: u32,
            input: *const KeyData, input_size: usize,
            output: *mut KeyData, output_size: *mut usize,
        ) -> IOReturn;
        fn IOServiceGetMatchingServices(
            main_port: MachPort,
            matching: *const std::ffi::c_void,
            iterator: *mut MachPort,
        ) -> IOReturn;
        fn IOServiceMatching(name: *const u8) -> *const std::ffi::c_void;
        fn IOIteratorNext(iterator: MachPort) -> MachPort;
        fn IOObjectRelease(object: MachPort) -> IOReturn;
        fn IORegistryEntryGetNameInPlane(
            entry: MachPort, plane: *const u8, name: *mut [u8; 128],
        ) -> IOReturn;
    }

    fn fourcc(s: &str) -> u32 {
        s.bytes().fold(0u32, |acc, b| (acc << 8) | b as u32)
    }

    fn fourcc_str(v: u32) -> String {
        let b = v.to_be_bytes();
        String::from_utf8_lossy(&b).to_string()
    }

    struct SmcConn(MachPort);

    impl SmcConn {
        fn open() -> Option<Self> {
            unsafe {
                let matching = IOServiceMatching(b"AppleSMC\0".as_ptr());
                if matching.is_null() { return None; }

                let mut iter: MachPort = 0;
                let kr = IOServiceGetMatchingServices(0, matching, &mut iter);
                if kr != kIOReturnSuccess || iter == 0 { return None; }

                // Find AppleSMCKeysEndpoint (Apple Silicon) or fall back to first service
                let mut fallback: MachPort = 0;
                let mut endpoint: MachPort = 0;

                loop {
                    let service = IOIteratorNext(iter);
                    if service == 0 { break; }

                    let mut name_buf = [0u8; 128];
                    let kr = IORegistryEntryGetNameInPlane(
                        service, b"IOService\0".as_ptr(), &mut name_buf,
                    );
                    if kr == kIOReturnSuccess {
                        let name = std::ffi::CStr::from_ptr(name_buf.as_ptr() as *const _)
                            .to_string_lossy();
                        if name.contains("AppleSMCKeysEndpoint") {
                            endpoint = service;
                            break;
                        }
                    }
                    if fallback == 0 {
                        fallback = service;
                    } else {
                        IOObjectRelease(service);
                    }
                }
                IOObjectRelease(iter);

                let target = if endpoint != 0 {
                    if fallback != 0 { IOObjectRelease(fallback); }
                    endpoint
                } else if fallback != 0 {
                    fallback
                } else {
                    return None;
                };

                let mut conn: MachPort = 0;
                let kr = IOServiceOpen(target, mach_task_self(), 0, &mut conn);
                IOObjectRelease(target);
                if kr != kIOReturnSuccess || conn == 0 { return None; }
                Some(SmcConn(conn))
            }
        }

        fn call(&self, input: &KeyData) -> Option<KeyData> {
            unsafe {
                let mut output = KeyData::default();
                let mut olen = size_of::<KeyData>();
                let kr = IOConnectCallStructMethod(
                    self.0, 2,
                    input, size_of::<KeyData>(),
                    &mut output, &mut olen,
                );
                if kr != kIOReturnSuccess { return None; }
                if output.result != 0 { return None; }
                Some(output)
            }
        }

        fn key_count(&self) -> u32 {
            // Step 1: get key info for #KEY
            let mut q = KeyData::default();
            q.data8 = 9;
            q.key = fourcc("#KEY");
            let info = match self.call(&q) {
                Some(o) => o.key_info,
                None => return 0,
            };
            // Step 2: read #KEY value
            let mut q2 = KeyData::default();
            q2.data8 = 5;
            q2.key = fourcc("#KEY");
            q2.key_info = info;
            match self.call(&q2) {
                Some(o) => u32::from_be_bytes(o.bytes[0..4].try_into().unwrap_or([0; 4])),
                None => 0,
            }
        }

        fn key_at_index(&self, index: u32) -> Option<u32> {
            let mut q = KeyData::default();
            q.data8 = 8;
            q.data32 = index;
            self.call(&q).map(|o| o.key)
        }

        fn key_info(&self, key: u32) -> Option<KeyInfo> {
            let mut q = KeyData::default();
            q.data8 = 9;
            q.key = key;
            self.call(&q).map(|o| o.key_info)
        }

        fn read_val(&self, key: u32, info: &KeyInfo) -> Option<[u8; 32]> {
            let mut q = KeyData::default();
            q.data8 = 5;
            q.key = key;
            q.key_info = *info;
            self.call(&q).map(|o| o.bytes)
        }
    }

    impl Drop for SmcConn {
        fn drop(&mut self) {
            unsafe { IOServiceClose(self.0); }
        }
    }

    // Category for a temperature key
    #[derive(Clone, Copy)]
    enum TempCat { Cpu, Gpu, Ssd }

    // Cached list of (key, info, category) for temperature sensors
    static TEMP_KEYS: std::sync::OnceLock<Vec<(u32, KeyInfo, TempCat)>> = std::sync::OnceLock::new();

    fn discover_temp_keys(conn: &SmcConn) -> Vec<(u32, KeyInfo, TempCat)> {
        let flt_type = fourcc("flt ");
        let count = conn.key_count();
        let mut keys = Vec::new();

        for i in 0..count {
            let key = match conn.key_at_index(i) {
                Some(k) => k,
                None => continue,
            };
            let info = match conn.key_info(key) {
                Some(i) => i,
                None => continue,
            };
            if info.data_size != 4 || info.data_type != flt_type {
                continue;
            }

            let key_str = fourcc_str(key);
            let cat = if key_str.starts_with("Tp") || key_str.starts_with("Te") {
                Some(TempCat::Cpu)
            } else if key_str.starts_with("Tg") {
                Some(TempCat::Gpu)
            } else if key_str.starts_with("Th") || key_str.starts_with("TN") {
                Some(TempCat::Ssd)
            } else {
                None
            };

            if let Some(cat) = cat {
                keys.push((key, info, cat));
            }
        }
        keys
    }

    pub fn read_temperatures() -> (f32, f32, f32) {
        let conn = match SmcConn::open() {
            Some(c) => c,
            None => return (-1.0, -1.0, -1.0),
        };

        let keys = TEMP_KEYS.get_or_init(|| discover_temp_keys(&conn));

        if keys.is_empty() { return (-1.0, -1.0, -1.0); }

        let mut cpu_temps: Vec<f32> = Vec::new();
        let mut gpu_temps: Vec<f32> = Vec::new();
        let mut ssd_temp: f32 = -1.0;

        for &(key, ref info, cat) in keys {
            let bytes = match conn.read_val(key, info) {
                Some(b) => b,
                None => continue,
            };

            let val = f32::from_le_bytes(bytes[0..4].try_into().unwrap());
            if !val.is_finite() || val <= 0.0 || val > 150.0 { continue; }

            match cat {
                TempCat::Cpu => cpu_temps.push(val),
                TempCat::Gpu => gpu_temps.push(val),
                TempCat::Ssd => { if val > ssd_temp { ssd_temp = val; } }
            }
        }

        let cpu_avg = if cpu_temps.is_empty() { -1.0 }
            else { cpu_temps.iter().sum::<f32>() / cpu_temps.len() as f32 };
        let gpu_avg = if gpu_temps.is_empty() { -1.0 }
            else { gpu_temps.iter().sum::<f32>() / gpu_temps.len() as f32 };

        (cpu_avg, gpu_avg, ssd_temp)
    }
}

/// Get top 5 processes by CPU usage.
fn get_top_processes(sys: &System) -> Vec<TopProcess> {
    let mut procs: Vec<_> = sys
        .processes()
        .values()
        .map(|p| TopProcess {
            name: p.name().to_string_lossy().to_string(),
            cpu: p.cpu_usage(),
            memory: p.memory(),
        })
        .collect();
    procs.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    procs.truncate(5);
    procs
}

/// Parse `pmset -g batt` output for battery percentage, charging status, and time remaining.
fn parse_battery_pmset() -> (f32, bool, String) {
    let output = std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output();

    let output = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return (-1.0, false, "N/A".to_string()),
    };

    // No battery line means desktop Mac
    if !output.contains("InternalBattery") {
        return (-1.0, false, "N/A".to_string());
    }

    let mut percent: f32 = -1.0;
    let mut charging = false;
    let mut time_remaining = String::from("N/A");

    for line in output.lines() {
        let line = line.trim();
        if !line.contains("InternalBattery") {
            continue;
        }
        // Parse percentage: e.g. "85%;"
        if let Some(pct_pos) = line.find('%') {
            // Walk backwards from '%' to find the start of the number
            let before = &line[..pct_pos];
            let num_str: String = before.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
            if let Ok(p) = num_str.parse::<f32>() {
                percent = p;
            }
        }
        // Charging status
        if line.contains("charging") && !line.contains("discharging") && !line.contains("not charging") {
            charging = true;
        }
        if line.contains("AC Power") || line.contains("charged") {
            charging = true;
        }
        // Time remaining
        if let Some(idx) = line.find("remaining") {
            // Format: "3:24 remaining"
            let before = line[..idx].trim();
            if let Some(time_part) = before.rsplit(';').next() {
                let t = time_part.trim();
                if !t.is_empty() && t != "(no estimate)" {
                    time_remaining = t.to_string();
                }
            }
        } else if line.contains("(no estimate)") {
            time_remaining = "Calculating...".to_string();
        } else if line.contains("not charging") || line.contains("finishing charge") {
            time_remaining = "N/A".to_string();
        }
    }

    // If on AC power / charged, override
    if output.contains("AC Power") && output.contains("charged") {
        charging = true;
        time_remaining = "Charged".to_string();
    }

    (percent, charging, time_remaining)
}

/// Parse `system_profiler SPPowerDataType` for battery health and cycle count.
fn parse_battery_profiler() -> (String, i32) {
    let output = std::process::Command::new("system_profiler")
        .arg("SPPowerDataType")
        .output();

    let output = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return ("N/A".to_string(), -1),
    };

    let mut health = String::from("N/A");
    let mut cycle_count: i32 = -1;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Condition:") {
            health = line.trim_start_matches("Condition:").trim().to_string();
        } else if line.starts_with("Cycle Count:") {
            if let Ok(c) = line
                .trim_start_matches("Cycle Count:")
                .trim()
                .parse::<i32>()
            {
                cycle_count = c;
            }
        }
    }

    (health, cycle_count)
}

/// Parse `memory_pressure` command output for system memory pressure level.
fn parse_memory_pressure() -> String {
    let output = std::process::Command::new("/usr/bin/memory_pressure")
        .args(["-Q"])
        .output();

    let output = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return "normal".to_string(),
    };

    // The output contains a line like:
    // "The system has <level> memory pressure"
    let lower = output.to_lowercase();
    if lower.contains("critical") {
        "critical".to_string()
    } else if lower.contains("warning") || lower.contains("warn") {
        "warning".to_string()
    } else {
        "normal".to_string()
    }
}
