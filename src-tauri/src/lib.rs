mod commands;

use commands::monitor::{StatsStreamActive, SystemMonitor};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use sysinfo::System;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(SystemMonitor(Mutex::new(System::new_all())))
        .manage(StatsStreamActive(Arc::new(AtomicBool::new(false))))
        .setup(|app| {
            // Apply macOS vibrancy effect
            if let Some(window) = app.get_webview_window("main") {
                use tauri::window::{Effect, EffectState, EffectsBuilder};
                let _ = window.set_effects(
                    EffectsBuilder::new()
                        .effect(Effect::UnderWindowBackground)
                        .state(EffectState::Active)
                        .build(),
                );
            }

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
            commands::monitor::stop_stats_stream,
            commands::cleaner::scan_for_cleanables,
            commands::cleaner::execute_clean,
            commands::optimizer::get_optimize_tasks,
            commands::optimizer::run_optimize_tasks,
            commands::uninstaller::scan_installed_apps,
            commands::uninstaller::get_associated_files,
            commands::uninstaller::execute_uninstall,
            commands::analyzer::analyze_path,
            commands::analyzer::reveal_in_finder,
            commands::analyzer::delete_analyzed_item,
            commands::analyzer::find_large_files,
            commands::purger::scan_artifacts,
            commands::purger::execute_purge,
            commands::installers::scan_installers,
            commands::installers::delete_installers,
            commands::settings::load_settings,
            commands::settings::save_settings,
            commands::settings::add_to_whitelist,
            commands::settings::remove_from_whitelist,
            commands::settings::pick_folder,
            commands::settings::get_total_bytes_freed,
            commands::settings::add_bytes_freed,
            commands::settings::reset_lifetime_stats,
            commands::settings::get_storage_path,
            commands::shared::check_full_disk_access,
            commands::shared::check_sip_status,
            commands::shared::open_fda_settings,
            commands::shared::restart_app,
            commands::shared::get_app_icon,
            commands::shared::get_app_icon_by_path,
            commands::cleaner::check_running_processes,
            commands::cleaner::run_brew_cleanup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kyra");
}
