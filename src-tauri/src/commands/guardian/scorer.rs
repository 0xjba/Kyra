use super::types::{ProbeReport, ScoredProbe};

const WORKER_URL: &str = "https://kyra-guardian.workers.dev";

pub async fn score_probes(
    probes: &[ProbeReport],
    device_id: &str,
) -> Result<Vec<ScoredProbe>, String> {
    let questions: Vec<String> = probes
        .iter()
        .map(|p| {
            format!(
                "{} has {} cleanable ({} items). {}. Score cleanup value 0-100.",
                p.display_name,
                format_bytes(p.cleanable_bytes),
                p.item_count,
                p.details
            )
        })
        .collect();

    match call_jev_api(&questions, device_id).await {
        Ok(scores) => Ok(probes
            .iter()
            .zip(scores.iter())
            .map(|(p, s)| ScoredProbe {
                category: p.category.clone(),
                display_name: p.display_name.clone(),
                cleanable_bytes: p.cleanable_bytes,
                score: s.score,
                confidence: s.confidence,
                details: p.details.clone(),
            })
            .collect()),
        Err(_) => Ok(probes
            .iter()
            .map(|p| {
                let score = heuristic_score(p);
                ScoredProbe {
                    category: p.category.clone(),
                    display_name: p.display_name.clone(),
                    cleanable_bytes: p.cleanable_bytes,
                    score,
                    confidence: 0.6,
                    details: p.details.clone(),
                }
            })
            .collect()),
    }
}

#[derive(serde::Deserialize)]
struct JevResponse {
    scores: Vec<JevScoreItem>,
}

#[derive(serde::Deserialize)]
struct JevScoreItem {
    score: f32,
    confidence: f32,
}

struct ScoreResult {
    score: f32,
    confidence: f32,
}

async fn call_jev_api(questions: &[String], device_id: &str) -> Result<Vec<ScoreResult>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/jev/score", WORKER_URL))
        .header("X-Device-ID", device_id)
        .json(&serde_json::json!({ "questions": questions }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("Jev API returned {}", resp.status()));
    }

    let body: JevResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body
        .scores
        .iter()
        .map(|s| ScoreResult {
            score: s.score,
            confidence: s.confidence,
        })
        .collect())
}

fn heuristic_score(probe: &ProbeReport) -> f32 {
    let size_score: f32 = match probe.cleanable_bytes {
        0..=104_857_600 => 30.0,
        104_857_601..=1_073_741_824 => 60.0,
        _ => 85.0,
    };
    let age_bonus: f32 = match probe.last_used_secs {
        Some(secs) if secs > 30 * 86400 => 15.0,
        Some(secs) if secs > 7 * 86400 => 5.0,
        _ => 0.0,
    };
    (size_score + age_bonus).min(100.0)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}
