mod commands;

use commands::monitor::SystemMonitor;
use std::sync::Mutex;
use sysinfo::System;

pub fn run() {
    tauri::Builder::default()
        .manage(SystemMonitor(Mutex::new(System::new_all())))
        .invoke_handler(tauri::generate_handler![
            commands::monitor::get_system_stats,
            commands::cleaner::scan_for_cleanables,
            commands::cleaner::execute_clean,
            commands::optimizer::get_optimize_tasks,
            commands::optimizer::run_optimize_tasks,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kyra");
}
