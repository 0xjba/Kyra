mod commands;

use commands::monitor::{StatsStreamActive, SystemMonitor};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use sysinfo::System;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(SystemMonitor(Mutex::new(System::new_all())))
        .manage(StatsStreamActive(Arc::new(AtomicBool::new(false))))
        .setup(|app| {
            // Apply macOS vibrancy effect & disable fullscreen (keep tiling)
            if let Some(window) = app.get_webview_window("main") {
                use tauri::window::{Effect, EffectState, EffectsBuilder};
                let _ = window.set_effects(
                    EffectsBuilder::new()
                        .effect(Effect::UnderWindowBackground)
                        .state(EffectState::Active)
                        .build(),
                );

                // Disable fullscreen but keep green button for tiling/arrange
                #[cfg(target_os = "macos")]
                unsafe {
                    use objc2::msg_send;
                    use objc2::runtime::AnyObject;

                    let ns_win = window.ns_window().unwrap() as *mut AnyObject;
                    let behavior: u64 = msg_send![&*ns_win, collectionBehavior];
                    // Remove FullScreenPrimary (1 << 7), add FullScreenAuxiliary (1 << 8)
                    let new_behavior = (behavior & !(1 << 7)) | (1 << 8);
                    let _: () = msg_send![&*ns_win, setCollectionBehavior: new_behavior];
                }
            }

            Ok(())
        })
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
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
            commands::pruner::scan_artifacts,
            commands::pruner::execute_prune,
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
