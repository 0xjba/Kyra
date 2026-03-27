mod commands;

use commands::monitor::SystemMonitor;
use std::sync::Mutex;
use sysinfo::System;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(SystemMonitor(Mutex::new(System::new_all())))
        .setup(|app| {
            let show_item = MenuItemBuilder::with_id("show", "Show Kyra").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&quit_item)
                .build()?;

            TrayIconBuilder::with_id("main-tray")
                .tooltip("Kyra")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::monitor::get_system_stats,
            commands::monitor::start_stats_stream,
            commands::cleaner::scan_for_cleanables,
            commands::cleaner::execute_clean,
            commands::optimizer::get_optimize_tasks,
            commands::optimizer::run_optimize_tasks,
            commands::uninstaller::scan_installed_apps,
            commands::uninstaller::get_associated_files,
            commands::uninstaller::execute_uninstall,
            commands::analyzer::analyze_path,
            commands::analyzer::reveal_in_finder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kyra");
}
