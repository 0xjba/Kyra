use crate::commands::guardian::types::ProbeReport;
use crate::commands::utils::dir_size;
use std::process::Command;

pub async fn probe() -> Option<ProbeReport> {
    let home = dirs::home_dir()?;
    let docker_dir = home.join(".docker");
    if !docker_dir.exists() {
        return None;
    }

    let mut cleanable: u64 = 0;
    let mut items: u32 = 0;
    let mut details_parts: Vec<String> = Vec::new();

    // Build cache
    let buildx_cache = docker_dir.join("buildx/cache");
    if buildx_cache.exists() {
        let size = tokio::task::spawn_blocking(move || dir_size(&buildx_cache))
            .await
            .unwrap_or(0);
        if size > 0 {
            cleanable += size;
            items += 1;
            details_parts.push(format!("build cache {}", format_mb(size)));
        }
    }

    // Docker Desktop VM disk
    let vm_dir = home.join("Library/Containers/com.docker.docker/Data/vms");
    if vm_dir.exists() {
        let vm_dir_c = vm_dir.clone();
        let size = tokio::task::spawn_blocking(move || dir_size(&vm_dir_c))
            .await
            .unwrap_or(0);
        if size > 1_073_741_824 {
            // Only count as cleanable if > 1 GB; estimate half is reclaimable
            cleanable += size / 2;
            items += 1;
            details_parts.push(format!("VM disk {}", format_mb(size)));
        }
    }

    // Docker system info via CLI (dangling images, stopped containers)
    if let Ok(output) = Command::new("docker")
        .args(["system", "df", "--format", "{{.Type}}\t{{.Reclaimable}}"])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("Build Cache") || line.contains("Images") {
                    details_parts.push(format!("docker reports: {}", line.trim()));
                }
            }
        }
    }

    // Count dangling images
    if let Ok(output) = Command::new("docker")
        .args(["images", "-f", "dangling=true", "-q"])
        .output()
    {
        if output.status.success() {
            let count = String::from_utf8_lossy(&output.stdout).lines().count() as u32;
            if count > 0 {
                items += count;
                details_parts.push(format!("{} dangling images", count));
            }
        }
    }

    // Count stopped containers
    if let Ok(output) = Command::new("docker")
        .args(["ps", "-f", "status=exited", "-q"])
        .output()
    {
        if output.status.success() {
            let count = String::from_utf8_lossy(&output.stdout).lines().count() as u32;
            if count > 0 {
                items += count;
                details_parts.push(format!("{} stopped containers", count));
            }
        }
    }

    if cleanable == 0 && items == 0 {
        return None;
    }

    Some(ProbeReport {
        category: "docker".into(),
        display_name: "Docker".into(),
        total_bytes: cleanable,
        cleanable_bytes: cleanable,
        item_count: items,
        last_used_secs: None,
        confidence: 0.8,
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
