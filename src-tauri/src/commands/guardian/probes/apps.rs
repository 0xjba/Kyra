use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join("Library/Application Support/Slack/Cache"), "Slack cache"),
        (
            home.join("Library/Application Support/Slack/Service Worker/CacheStorage"),
            "Slack SW",
        ),
        (
            home.join("Library/Application Support/discord/Cache"),
            "Discord cache",
        ),
        (
            home.join("Library/Application Support/discord/Code Cache"),
            "Discord code cache",
        ),
        (home.join("Library/Caches/com.spotify.client"), "Spotify cache"),
        (
            home.join("Library/Application Support/Spotify/PersistentCache"),
            "Spotify offline",
        ),
        (
            home.join("Library/Caches/Google/Chrome/Default/Cache"),
            "Chrome cache",
        ),
        (home.join("Library/Caches/Firefox/Profiles"), "Firefox cache"),
        (home.join("Library/Caches/com.apple.Safari"), "Safari cache"),
        (
            home.join("Library/Application Support/zoom.us/data"),
            "Zoom data",
        ),
        (
            home.join("Library/Application Support/Microsoft/Teams/Cache"),
            "Teams cache",
        ),
    ];

    for (path, label) in &paths {
        if path.exists() {
            let p = path.clone();
            let size = tokio::task::spawn_blocking(move || dir_size(&p))
                .await
                .unwrap_or(0);
            if size > 0 {
                cleanable += size;
                items += 1;
                details.push(format!("{} {}", label, format_mb(size)));
            }
        }
    }

    if cleanable == 0 {
        return None;
    }

    Some(ProbeReport {
        category: "apps".into(),
        display_name: "App Caches".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.85,
        details: details.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}
