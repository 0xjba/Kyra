use super::types::{GuardianCleanProgress, GuardianCleanResult};
use crate::commands::shared;
use tauri::Emitter;

pub async fn execute_guardian_clean(
    app: &tauri::AppHandle,
    categories: &[String],
    permanent: bool,
) -> Result<GuardianCleanResult, String> {
    let total = categories.len();
    let mut bytes_freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut cleaned = 0;

    for (i, category) in categories.iter().enumerate() {
        let _ = app.emit(
            "guardian-clean-progress",
            GuardianCleanProgress {
                current_category: category.clone(),
                categories_done: i,
                categories_total: total,
                bytes_freed,
            },
        );

        match clean_category(category, permanent).await {
            Ok(freed) => {
                bytes_freed += freed;
                cleaned += 1;
            }
            Err(e) => {
                errors.push(format!("{}: {}", category, e));
            }
        }
    }

    let _ = app.emit(
        "guardian-clean-progress",
        GuardianCleanProgress {
            current_category: String::new(),
            categories_done: total,
            categories_total: total,
            bytes_freed,
        },
    );

    shared::log_operation(
        "GUARDIAN_CLEAN",
        "guardian",
        &format!("cleaned {} categories, freed {} bytes", cleaned, bytes_freed),
    );

    Ok(GuardianCleanResult {
        categories_cleaned: cleaned,
        bytes_freed,
        errors,
    })
}

async fn clean_category(category: &str, permanent: bool) -> Result<u64, String> {
    let paths = get_cleanable_paths(category);
    let mut freed: u64 = 0;

    for path_str in &paths {
        let path = std::path::Path::new(path_str);
        if !path.exists() {
            continue;
        }
        if !crate::commands::cleaner::is_safe_path(path_str) {
            continue;
        }

        let size = crate::commands::utils::dir_size(path);

        let result = if permanent {
            if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        } else {
            trash::delete(path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        };

        match result {
            Ok(_) => freed += size,
            Err(e) => {
                shared::log_operation("GUARDIAN_CLEAN_ERR", path_str, &e.to_string());
            }
        }
    }

    Ok(freed)
}

fn get_cleanable_paths(category: &str) -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let h = home.to_string_lossy();

    match category {
        "docker" => vec![
            format!("{}/.docker/buildx/cache", h),
            format!("{}/Library/Containers/com.docker.docker/Data/vms", h),
        ],
        "homebrew" => vec![format!("{}/Library/Caches/Homebrew", h)],
        "xcode" => vec![
            format!("{}/Library/Developer/Xcode/DerivedData", h),
            format!("{}/Library/Developer/Xcode/Archives", h),
            format!("{}/Library/Developer/CoreSimulator/Caches", h),
        ],
        "node" => vec![
            format!("{}/.npm/_cacache", h),
            format!("{}/Library/Caches/Yarn", h),
            format!("{}/Library/pnpm/store", h),
            format!("{}/.bun/install/cache", h),
        ],
        "rust" => vec![
            format!("{}/.cargo/registry/cache", h),
            format!("{}/.cargo/registry/src", h),
            format!("{}/.rustup/tmp", h),
        ],
        "python" => vec![
            format!("{}/Library/Caches/pip", h),
            format!("{}/.cache/pip", h),
            format!("{}/.conda/pkgs", h),
            format!("{}/.cache/conda", h),
        ],
        "ide" => vec![
            format!("{}/Library/Application Support/Code/Cache", h),
            format!("{}/Library/Application Support/Code/CachedData", h),
            format!("{}/Library/Application Support/Code/CachedExtensions", h),
            format!("{}/Library/Caches/JetBrains", h),
            format!("{}/Library/Application Support/Sublime Text/Cache", h),
        ],
        "ai_ml" => vec![
            format!("{}/.cache/huggingface", h),
            format!("{}/.ollama/models", h),
            format!("{}/Library/Caches/com.apple.coreml", h),
            format!("{}/.cache/lm-studio", h),
        ],
        "apps" => vec![
            format!("{}/Library/Application Support/Slack/Cache", h),
            format!(
                "{}/Library/Application Support/Slack/Service Worker/CacheStorage",
                h
            ),
            format!("{}/Library/Application Support/discord/Cache", h),
            format!("{}/Library/Application Support/discord/Code Cache", h),
            format!("{}/Library/Caches/com.spotify.client", h),
            format!("{}/Library/Application Support/Spotify/PersistentCache", h),
            format!("{}/Library/Caches/Google/Chrome/Default/Cache", h),
            format!("{}/Library/Caches/Firefox/Profiles", h),
            format!("{}/Library/Caches/com.apple.Safari", h),
            format!("{}/Library/Application Support/zoom.us/data", h),
            format!("{}/Library/Application Support/Microsoft/Teams/Cache", h),
        ],
        "system" => vec![
            format!("{}/Library/Logs", h),
            format!("{}/Library/Mail Downloads", h),
            format!("{}/.Trash", h),
        ],
        _ => vec![],
    }
}
