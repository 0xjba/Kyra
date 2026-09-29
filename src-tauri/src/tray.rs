use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "pawtrol";

pub const STATUS_ON_DUTY: &str = "Pawtrol · on duty";
pub const STATUS_OFF: &str = "Pawtrol · off";

const ITEM_OPEN: &str = "tray-open";
const ITEM_QUIT: &str = "tray-quit";
// macOS scales tray icons to 18pt tall, so a 36px bitmap stays sharp on Retina.
const TRAY_ICON_PX: u32 = 36;

static VISIBLE: AtomicBool = AtomicBool::new(false);
// Status set by the patrol engine wins over the default licensed/off text.
static CUSTOM_STATUS: Mutex<Option<String>> = Mutex::new(None);

struct TrayStatusItem(MenuItem<Wry>);

/// True while the menu-bar icon is shown, i.e. while Pawtrol is licensed.
pub fn is_tray_visible() -> bool {
    VISIBLE.load(Ordering::SeqCst)
}

/// Updates the disabled status line at the top of the tray menu.
pub fn set_tray_status(app: &AppHandle, text: &str) {
    *CUSTOM_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.to_string());
    if let Some(item) = app.try_state::<TrayStatusItem>() {
        let _ = item.0.set_text(text);
    }
}

fn current_status(licensed: bool) -> String {
    CUSTOM_STATUS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| if licensed { STATUS_ON_DUTY } else { STATUS_OFF }.to_string())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "tray-status", current_status(true), false, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, ITEM_OPEN, "Open Kyra", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, ITEM_QUIT, "Quit Kyra", true, None::<&str>)?,
        ],
    )?;
    app.manage(TrayStatusItem(status));

    let icon = Image::new(include_bytes!("../icons/tray/trayTemplate@2x.rgba"), TRAY_ICON_PX, TRAY_ICON_PX);
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Kyra")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            ITEM_OPEN => show_main_window(app),
            ITEM_QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// Called by the frontend whenever the Pawtrol license state changes.
#[tauri::command]
pub fn set_tray_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    VISIBLE.store(visible, Ordering::SeqCst);
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_visible(visible).map_err(|e| e.to_string())?,
        None if visible => build_tray(&app).map_err(|e| e.to_string())?,
        None => return Ok(()),
    }
    if let Some(item) = app.try_state::<TrayStatusItem>() {
        let _ = item.0.set_text(current_status(visible));
    }
    Ok(())
}
