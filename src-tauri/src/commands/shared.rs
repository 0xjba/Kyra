use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[tauri::command]
pub fn open_fda_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn();
}

#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    let current_exe = std::env::current_exe().unwrap_or_default();
    let exe_path = current_exe.to_string_lossy();

    // In production, the exe lives inside a .app bundle — relaunch via `open`
    if let Some(app_idx) = exe_path.find(".app/") {
        let app_bundle = &exe_path[..app_idx + 4]; // includes ".app"
        let _ = std::process::Command::new("open").arg("-n").arg(app_bundle).spawn();
        app.exit(0);
    } else {
        // Dev mode: just relaunch the binary directly
        let _ = std::process::Command::new(&current_exe).spawn();
        app.exit(0);
    }
}

#[tauri::command]
pub fn check_full_disk_access() -> bool {
    // Try multiple FDA-protected paths — any readable one confirms access
    let candidates: Vec<PathBuf> = if let Some(home) = dirs::home_dir() {
        vec![
            home.join("Library/Safari"),
            home.join("Library/Mail"),
            home.join("Library/Containers"),
            home.join("Library/Cookies"),
        ]
    } else {
        return true;
    };

    for path in &candidates {
        if path.exists() {
            return fs::read_dir(path).is_ok();
        }
    }
    // None of the protected paths exist — assume access is fine
    true
}

#[tauri::command]
pub fn check_sip_status() -> bool {
    use std::process::Command;
    let output = Command::new("csrutil").arg("status").output();
    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            text.contains("enabled")
        }
        Err(_) => true, // assume enabled if can't check
    }
}

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024; // 5 MB

fn log_path() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        let dir = home.join("Library/Logs/Kyra");
        let _ = fs::create_dir_all(&dir);
        dir.join("operations.log")
    } else {
        PathBuf::from("/tmp/kyra-operations.log")
    }
}

fn rotate_log_if_needed(path: &PathBuf) {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > MAX_LOG_SIZE {
            let rotated = path.with_extension("log.1");
            let _ = fs::rename(path, rotated);
        }
    }
}

pub fn log_operation(action: &str, path: &str, result: &str) {
    let log = log_path();
    rotate_log_if_needed(&log);

    let timestamp = chrono_timestamp();
    let line = format!("[{}] {} | {} | {}\n", timestamp, action, path, result);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let time_of_day = now % secs_per_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_days: &[u64] = if is_leap(year) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &md in month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Returns the log file path for the frontend.
#[tauri::command]
pub fn get_log_path() -> String {
    log_path().to_string_lossy().to_string()
}

/// Opens the log file's parent directory in Finder and selects the file.
#[tauri::command]
pub fn reveal_log_in_finder() {
    let path = log_path();
    if path.exists() {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn();
    }
}

/// Log a session start marker with timestamp.
pub fn log_session_start(module: &str) {
    let timestamp = chrono_timestamp();
    log_operation(
        "SESSION_START",
        module,
        &format!("--- Session started at {} ---", timestamp),
    );
}

/// Log a session end marker with summary.
pub fn log_session_end(module: &str, summary: &str) {
    let timestamp = chrono_timestamp();
    log_operation(
        "SESSION_END",
        module,
        &format!("--- {} | {} ---", summary, timestamp),
    );
}

/// Read a cached icon or return None.
fn read_cached_icon(cache_path: &std::path::Path) -> Option<String> {
    if cache_path.exists() {
        if let Ok(bytes) = fs::read(cache_path) {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Some(format!("data:image/png;base64,{}", b64));
        }
    }
    None
}

/// Encode a PNG file at cache_path into a data URI.
fn encode_cached_png(cache_path: &std::path::Path) -> Option<String> {
    let bytes = fs::read(cache_path).ok()?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:image/png;base64,{}", b64))
}

/// Fallback: use Swift/NSWorkspace to extract the app icon (works for Asset Catalog icons).
fn extract_icon_via_nsworkspace(
    app_path: &str,
    cache_path: &std::path::Path,
) -> Option<String> {
    use std::process::Command;

    let cache_str = cache_path.to_string_lossy();
    let script = format!(
        "import AppKit; let ws = NSWorkspace.shared; let img = ws.icon(forFile: \"{}\"); \
         img.size = NSSize(width: 64, height: 64); \
         let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 64, pixelsHigh: 64, \
         bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, \
         colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!; \
         NSGraphicsContext.saveGraphicsState(); \
         NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep); \
         img.draw(in: NSRect(x: 0, y: 0, width: 64, height: 64)); \
         NSGraphicsContext.restoreGraphicsState(); \
         let data = rep.representation(using: .png, properties: [:])!; \
         try! data.write(to: URL(fileURLWithPath: \"{}\"))",
        app_path, cache_str
    );

    let ok = Command::new("swift")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok && cache_path.exists() {
        encode_cached_png(cache_path)
    } else {
        None
    }
}

/// Extract an app icon by its full bundle path.
/// Tries .icns via sips first, falls back to NSWorkspace for Asset Catalog icons.
#[tauri::command]
pub async fn get_app_icon_by_path(app_path: String) -> Option<String> {
    use std::path::Path;
    use std::process::Command;

    let app = Path::new(&app_path);
    if !app.exists() {
        return None;
    }

    // Check cache
    let cache_dir = std::env::temp_dir().join("kyra-icons");
    let _ = fs::create_dir_all(&cache_dir);
    let cache_key = app_path.replace(['/', ' ', '.'], "_");
    let cache_path = cache_dir.join(format!("{}.png", cache_key));

    if let Some(cached) = read_cached_icon(&cache_path) {
        return Some(cached);
    }

    // Read CFBundleIconFile from Info.plist
    let plist_path = app.join("Contents/Info.plist");
    let plist_str = plist_path.to_string_lossy();

    let icon_output = Command::new("defaults")
        .args(["read", &plist_str, "CFBundleIconFile"])
        .output()
        .ok();

    let icon_name_opt = match &icon_output {
        Some(o) if o.status.success() => {
            let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if name.is_empty() { None } else { Some(name) }
        }
        _ => {
            // Fallback: try CFBundleIconName
            let fallback = Command::new("defaults")
                .args(["read", &plist_str, "CFBundleIconName"])
                .output()
                .ok();
            match &fallback {
                Some(o) if o.status.success() => {
                    let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if name.is_empty() { None } else { Some(name) }
                }
                _ => None,
            }
        }
    };

    // Try .icns approach first
    if let Some(mut icon_name) = icon_name_opt {
        if !icon_name.ends_with(".icns") {
            icon_name.push_str(".icns");
        }

        let icns_path = app.join("Contents/Resources").join(&icon_name);
        if icns_path.exists() {
            let ok = Command::new("sips")
                .args([
                    "-s", "format", "png",
                    "-Z", "64",
                    &icns_path.to_string_lossy(),
                    "--out",
                    &cache_path.to_string_lossy(),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok()
                .map(|s| s.success())
                .unwrap_or(false);

            if ok && cache_path.exists() {
                return encode_cached_png(&cache_path);
            }
        }
    }

    // Fallback: NSWorkspace (handles Asset Catalog icons like UTM)
    extract_icon_via_nsworkspace(&app_path, &cache_path)
}

/// Extract an app icon as a base64 data URI PNG.
/// Looks up the app in /Applications, reads CFBundleIconFile from Info.plist,
/// converts the .icns to a 64px PNG via `sips`, and returns a data URI.
#[tauri::command]
pub async fn get_app_icon(app_name: String) -> Option<String> {
    use std::path::Path;
    use std::process::Command;

    // Check cache first
    let cache_dir = std::env::temp_dir().join("kyra-icons");
    let _ = fs::create_dir_all(&cache_dir);
    let cache_key = app_name.replace(['/', ' ', '.'], "_");
    let cache_path = cache_dir.join(format!("{}.png", cache_key));

    if cache_path.exists() {
        if let Ok(bytes) = fs::read(&cache_path) {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Some(format!("data:image/png;base64,{}", b64));
        }
    }

    // Search common app locations
    let app_dirs = [
        format!("/Applications/{}.app", app_name),
        format!("/Applications/{}.app", app_name.replace(' ', "")),
        format!("/System/Applications/{}.app", app_name),
    ];

    let home = dirs::home_dir()?;
    let user_app = format!(
        "{}/Applications/{}.app",
        home.display(),
        app_name
    );

    let mut app_path: Option<String> = None;
    for candidate in app_dirs.iter().chain(std::iter::once(&user_app)) {
        if Path::new(candidate).exists() {
            app_path = Some(candidate.clone());
            break;
        }
    }
    let app_path = app_path?;

    // Read Info.plist to find icon file name
    let plist_path = format!("{}/Contents/Info.plist", app_path);
    let plist_output = Command::new("defaults")
        .args(["read", &plist_path, "CFBundleIconFile"])
        .output()
        .ok()?;

    if !plist_output.status.success() {
        return None;
    }

    let mut icon_name = String::from_utf8_lossy(&plist_output.stdout)
        .trim()
        .to_string();

    // Append .icns if missing
    if !icon_name.ends_with(".icns") {
        icon_name.push_str(".icns");
    }

    let icns_path = format!("{}/Contents/Resources/{}", app_path, icon_name);
    if !Path::new(&icns_path).exists() {
        return None;
    }

    // Convert to PNG using sips
    let sips_result = Command::new("sips")
        .args([
            "-s", "format", "png",
            &icns_path,
            "--out",
            &cache_path.to_string_lossy(),
            "--resampleWidth", "64",
        ])
        .output()
        .ok()?;

    if !sips_result.status.success() {
        return None;
    }

    // Read and encode
    let bytes = fs::read(&cache_path).ok()?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:image/png;base64,{}", b64))
}
