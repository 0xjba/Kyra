use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join("Library/Caches/pip"), "pip cache (Library)"),
        (home.join(".cache/pip"), "pip cache (dotcache)"),
        (home.join(".conda/pkgs"), "conda packages"),
        (home.join(".cache/conda"), "conda cache"),
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
        category: "python".into(),
        display_name: "Python".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.9,
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
