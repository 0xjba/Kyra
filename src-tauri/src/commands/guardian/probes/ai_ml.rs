use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details: Vec<String> = Vec::new();

    let paths = [
        (home.join(".cache/huggingface"), "Hugging Face models"),
        (home.join(".ollama/models"), "Ollama models"),
        (home.join("Library/Caches/com.apple.coreml"), "CoreML cache"),
        (home.join(".cache/lm-studio"), "LM Studio models"),
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
        category: "ai_ml".into(),
        display_name: "AI & ML Models".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.7,
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
