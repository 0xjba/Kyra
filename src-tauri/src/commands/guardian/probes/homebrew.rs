use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;
use std::path::Path;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let cache_dir = home.join("Library/Caches/Homebrew");
    if !cache_dir.exists() {
        return None;
    }

    let cache_dir_c = cache_dir.clone();
    let cache_size = tokio::task::spawn_blocking(move || dir_size(&cache_dir_c))
        .await
        .unwrap_or(0);

    if cache_size == 0 {
        return None;
    }

    let mut details_parts: Vec<String> = Vec::new();
    let mut items: u32 = 0;

    details_parts.push(format!("cache {}", format_mb(cache_size)));
    items += 1;

    // Count outdated formulae
    let brew_path = if Path::new("/opt/homebrew/bin/brew").exists() {
        Some("/opt/homebrew/bin/brew")
    } else if Path::new("/usr/local/bin/brew").exists() {
        Some("/usr/local/bin/brew")
    } else {
        None
    };

    if let Some(brew) = brew_path {
        if let Ok(output) = std::process::Command::new(brew)
            .args(["outdated", "--quiet"])
            .output()
        {
            if output.status.success() {
                let count = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|l| !l.is_empty())
                    .count() as u32;
                if count > 0 {
                    items += count;
                    details_parts.push(format!("{} outdated packages", count));
                }
            }
        }
    }

    Some(ProbeReport {
        category: "homebrew".into(),
        display_name: "Homebrew".into(),
        total_bytes: cache_size,
        cleanable_bytes: cache_size,
        item_count: items,
        last_used_secs: None,
        confidence: 0.9,
        details: details_parts.join(", "),
    })
}

fn format_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}
