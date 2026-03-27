mod commands;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::monitor::get_system_stats,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kyra");
}
