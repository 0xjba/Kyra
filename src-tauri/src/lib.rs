mod commands;
#[cfg(target_os = "macos")]
mod glass;
pub mod tray;

use commands::monitor::{StatsStreamActive, SystemMonitor};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use sysinfo::System;
use tauri::Manager;

pub fn run() {
    // Install panic hook so crashes are written to the log file
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_else(|| "unknown".into());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".into()
        };
        commands::shared::log_operation("PANIC", &location, &payload);
        default_hook(info);
    }));

    // Log system info on startup
    log_system_info();

    tauri::Builder::default()
        .manage(SystemMonitor(Mutex::new(System::new_all())))
        .manage(StatsStreamActive(Arc::new(AtomicBool::new(false))))
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                unsafe {
                    use objc2::msg_send;
                    use objc2::runtime::AnyObject;

                    let ns_win = window.ns_window().unwrap() as *mut AnyObject;

                    if !glass::install(ns_win) {
                        use tauri::window::{Effect, EffectState, EffectsBuilder};
                        let _ = window.set_effects(
                            EffectsBuilder::new()
                                .effect(Effect::UnderWindowBackground)
                                .state(EffectState::Active)
                                .build(),
                        );
                    }

                    glass::measure_traffic_lights(ns_win);

                    // Disable fullscreen but keep green button for tiling/arrange
                    let behavior: u64 = msg_send![&*ns_win, collectionBehavior];
                    // Remove FullScreenPrimary (1 << 7), add FullScreenAuxiliary (1 << 8)
                    let new_behavior = (behavior & !(1 << 7)) | (1 << 8);
                    let _: () = msg_send![&*ns_win, setCollectionBehavior: new_behavior];
                }
            }

            commands::guardian::patrol::start_patrol_scheduler(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && keep_running_in_background() {
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
            }

            #[cfg(target_os = "macos")]
            if matches!(
                event,
                tauri::WindowEvent::Focused(_)
                    | tauri::WindowEvent::ThemeChanged(_)
                    | tauri::WindowEvent::Resized(_)
            ) {
                glass::retune();
            }
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
            commands::analyzer::delete_analyzed_items,
            commands::analyzer::find_large_files,
            commands::analyzer::analyze_overview,
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
            commands::shared::get_log_path,
            commands::shared::reveal_log_in_finder,
            commands::shared::get_app_icon,
            commands::shared::get_app_icon_by_path,
            commands::cleaner::check_running_processes,
            commands::cleaner::run_brew_cleanup,
            commands::guardian::guardian_run_probes,
            commands::guardian::guardian_score,
            commands::guardian::guardian_clean,
            commands::guardian::guardian_check_license,
            commands::guardian::guardian_get_device_id,
            commands::guardian::guardian_checkout_create,
            commands::guardian::guardian_restore_start,
            commands::guardian::guardian_restore_verify,
            commands::guardian::guardian_account,
            commands::guardian::guardian_cancel_subscription,
            commands::guardian::guardian_patrol_status,
            commands::guardian::guardian_patrol_now,
            commands::guardian::guardian_review_clean,
            commands::guardian::guardian_review_dismiss,
            commands::guardian::guardian_set_patrol,
            commands::guardian::get_device_name,
            tray::set_tray_visible,
            #[cfg(target_os = "macos")]
            glass::get_traffic_lights,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Kyra")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows: false, .. } = _event {
                tray::show_main_window(_app);
            }
        });
}

/// Pawtrol keeps Kyra alive in the menu bar after the window closes.
fn keep_running_in_background() -> bool {
    tray::is_tray_visible()
        && commands::settings::load_settings_internal()
            .map(|s| s.pawtrol_enabled)
            .unwrap_or(true)
}

fn log_system_info() {
    use std::process::Command;

    let os_version = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());

    let arch = std::env::consts::ARCH;
    let app_version = env!("CARGO_PKG_VERSION");

    commands::shared::log_operation(
        "APP_START",
        "kyra",
        &format!("v{} | macOS {} | {}", app_version, os_version, arch),
    );
}
