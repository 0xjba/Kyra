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
pub fn check_full_disk_access() -> bool {
    if let Some(home) = dirs::home_dir() {
        let test_path = home.join("Library/Mail");
        if test_path.exists() {
            return fs::read_dir(&test_path).is_ok();
        }
    }
    if let Some(home) = dirs::home_dir() {
        let test_path = home.join("Library/Safari");
        if test_path.exists() {
            return fs::read_dir(&test_path).is_ok();
        }
    }
    true
}

fn log_path() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        let dir = home.join("Library/Logs/Kyra");
        let _ = fs::create_dir_all(&dir);
        dir.join("operations.log")
    } else {
        PathBuf::from("/tmp/kyra-operations.log")
    }
}

pub fn log_operation(action: &str, path: &str, result: &str) {
    let timestamp = chrono_timestamp();
    let line = format!("[{}] {} | {} | {}\n", timestamp, action, path, result);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
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
