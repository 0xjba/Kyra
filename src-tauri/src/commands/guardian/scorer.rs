use super::probes;
use super::types::{ProbeReport, ScoredProbe};

/// Keeps user-data categories in the UI's "Review 40–70" band so they are never auto-selected (> 70).
pub(crate) const USER_DATA_SCORE_CAP: f32 = 55.0;

pub async fn score_probes(
    probes: &[ProbeReport],
    device_id: &str,
) -> Result<Vec<ScoredProbe>, String> {
    score_probes_with(super::http(), &super::worker_url(), probes, device_id).await
}

pub(crate) async fn score_probes_with(
    client: &reqwest::Client,
    base_url: &str,
    probes: &[ProbeReport],
    device_id: &str,
) -> Result<Vec<ScoredProbe>, String> {
    if probes.is_empty() {
        return Ok(Vec::new());
    }
    let questions = build_questions(probes);

    match call_jev_api(client, base_url, &questions, device_id).await {
        Ok(scores) if scores.len() == probes.len() => Ok(probes
            .iter()
            .zip(scores.iter())
            .map(|(p, s)| scored(p, s.score, s.confidence))
            .collect()),
        // The frontend maps these messages to its paywall state.
        Err(JevError::License(msg)) => Err(msg),
        _ => Ok(heuristic_scores(probes)),
    }
}

fn build_questions(probes: &[ProbeReport]) -> Vec<String> {
    probes
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
        .collect()
}

fn heuristic_scores(probes: &[ProbeReport]) -> Vec<ScoredProbe> {
    probes
        .iter()
        .map(|p| scored(p, heuristic_score(p), 0.6))
        .collect()
}

// user_data comes from the backend category table, not the incoming payload.
fn scored(p: &ProbeReport, score: f32, confidence: f32) -> ScoredProbe {
    let data_loss = probes::data_loss(&p.category);
    let score = if data_loss.is_some() {
        score.min(USER_DATA_SCORE_CAP)
    } else {
        score
    };
    ScoredProbe {
        category: p.category.clone(),
        display_name: p.display_name.clone(),
        cleanable_bytes: p.cleanable_bytes,
        score,
        confidence,
        details: p.details.clone(),
        user_data: data_loss.is_some(),
        data_loss: data_loss.map(String::from),
    }
}

#[derive(serde::Serialize)]
struct JevRequest<'a> {
    device_id: &'a str,
    questions: &'a [String],
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

#[derive(serde::Deserialize)]
struct WorkerError {
    error: String,
}

#[derive(Debug)]
enum JevError {
    License(String),
    Other,
}

async fn call_jev_api(
    client: &reqwest::Client,
    base_url: &str,
    questions: &[String],
    device_id: &str,
) -> Result<Vec<JevScoreItem>, JevError> {
    let resp = client
        .post(format!("{}/jev/score", base_url))
        .header("X-Device-ID", device_id)
        .json(&JevRequest {
            device_id,
            questions,
        })
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| JevError::Other)?;

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        let msg = resp
            .json::<WorkerError>()
            .await
            .map(|e| e.error)
            .unwrap_or_else(|_| "No active license".into());
        return Err(JevError::License(msg));
    }
    if !status.is_success() {
        return Err(JevError::Other);
    }

    let body: JevResponse = resp.json().await.map_err(|_| JevError::Other)?;
    Ok(body.scores)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::guardian::test_support::{block_on, client, dead_url, serve_once};

    // Mirrors AUTO_SELECT_THRESHOLD in src/stores/guardianStore.ts (score > 70 is auto-selected).
    const UI_AUTO_SELECT_THRESHOLD: f32 = 70.0;

    const MB: u64 = 1_048_576;
    const GB: u64 = 1_073_741_824;

    fn probe(category: &str, bytes: u64, last_used: Option<u64>) -> ProbeReport {
        ProbeReport {
            category: category.into(),
            display_name: format!("{} name", category),
            total_bytes: bytes,
            cleanable_bytes: bytes,
            item_count: 2,
            last_used_secs: last_used,
            confidence: 0.9,
            details: "a 1 MB, b 2 MB".into(),
            user_data: false,
            data_loss: None,
        }
    }

    #[test]
    fn heuristic_size_bands() {
        assert_eq!(heuristic_score(&probe("a", 0, None)), 30.0);
        assert_eq!(heuristic_score(&probe("a", 100 * MB, None)), 30.0);
        assert_eq!(heuristic_score(&probe("a", 100 * MB + 1, None)), 60.0);
        assert_eq!(heuristic_score(&probe("a", GB, None)), 60.0);
        assert_eq!(heuristic_score(&probe("a", GB + 1, None)), 85.0);
    }

    #[test]
    fn heuristic_age_bonus_and_cap() {
        let day = 86_400;
        assert_eq!(heuristic_score(&probe("a", 0, Some(7 * day))), 30.0);
        assert_eq!(heuristic_score(&probe("a", 0, Some(7 * day + 1))), 35.0);
        assert_eq!(heuristic_score(&probe("a", 0, Some(31 * day))), 45.0);
        assert_eq!(heuristic_score(&probe("a", 5 * GB, Some(31 * day))), 100.0);
    }

    #[test]
    fn heuristic_auto_selects_only_over_one_gb_without_age() {
        let over = |b: u64| heuristic_score(&probe("a", b, None)) > UI_AUTO_SELECT_THRESHOLD;
        assert!(!over(50 * MB));
        assert!(!over(GB));
        assert!(over(GB + 1));
        // Scores never land exactly on the UI boundary, so there is no ambiguous "70".
        for b in [0, MB, 200 * MB, GB, 2 * GB] {
            for age in [None, Some(8 * 86_400), Some(40 * 86_400)] {
                assert_ne!(
                    heuristic_score(&probe("a", b, age)),
                    UI_AUTO_SELECT_THRESHOLD
                );
            }
        }
    }

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(512 * 1024), "512 KB");
        assert_eq!(format_bytes(300 * MB), "300 MB");
        assert_eq!(format_bytes(3 * GB / 2), "1.5 GB");
    }

    #[test]
    fn questions_describe_each_probe_in_order() {
        let q = build_questions(&[probe("node", 2 * GB, None), probe("rust", 5 * MB, None)]);
        assert_eq!(
            q[0],
            "node name has 2.0 GB cleanable (2 items). a 1 MB, b 2 MB. Score cleanup value 0-100."
        );
        assert!(q[1].starts_with("rust name has 5 MB cleanable"));
    }

    #[test]
    fn request_body_matches_worker_contract() {
        let questions = vec!["q1".to_string()];
        let v = serde_json::to_value(JevRequest {
            device_id: "dev",
            questions: &questions,
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "device_id": "dev", "questions": ["q1"] })
        );
    }

    #[test]
    fn remote_scores_are_zipped_in_order() {
        let (base, server) = serve_once(
            200,
            r#"{"scores":[{"category":"x","score":91,"confidence":0.8},{"score":12.5,"confidence":0.4}]}"#,
        );
        let probes = [probe("node", 2 * GB, None), probe("rust", 5 * MB, None)];
        let out = block_on(score_probes_with(&client(), &base, &probes, "dev-1")).unwrap();
        let req = server.join().unwrap();

        assert_eq!(req.request_line, "POST /jev/score HTTP/1.1");
        assert_eq!(req.header("x-device-id"), Some("dev-1"));
        let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(body["device_id"], "dev-1");
        assert_eq!(body["questions"].as_array().unwrap().len(), 2);

        assert_eq!(out[0].category, "node");
        assert_eq!(out[0].score, 91.0);
        assert_eq!(out[0].confidence, 0.8);
        assert_eq!(out[1].category, "rust");
        assert_eq!(out[1].score, 12.5);
        assert_eq!(out[1].cleanable_bytes, 5 * MB);
    }

    #[test]
    fn no_license_403_surfaces_worker_message() {
        let (base, server) = serve_once(403, r#"{"error":"No active license"}"#);
        let err = block_on(score_probes_with(
            &client(),
            &base,
            &[probe("a", GB, None)],
            "d",
        ))
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(err, "No active license");
    }

    #[test]
    fn expired_license_403_surfaces_worker_message() {
        let (base, server) = serve_once(403, r#"{"error":"License expired"}"#);
        let err = block_on(score_probes_with(
            &client(),
            &base,
            &[probe("a", GB, None)],
            "d",
        ))
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(err, "License expired");
    }

    #[test]
    fn bare_403_still_maps_to_license_error() {
        let (base, server) = serve_once(403, "");
        let err = block_on(score_probes_with(
            &client(),
            &base,
            &[probe("a", GB, None)],
            "d",
        ))
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(err, "No active license");
    }

    const USER_DATA: [&str; 4] = ["docker_vm", "xcode_archives", "ai_ml", "app_data"];

    #[test]
    fn remote_scores_for_user_data_are_capped_below_auto_select() {
        let mut probes: Vec<ProbeReport> =
            USER_DATA.iter().map(|c| probe(c, 9 * GB, None)).collect();
        probes.push(probe("node", 9 * GB, None));
        let body = serde_json::json!({
            "scores": [
                { "score": 100, "confidence": 1 },
                { "score": 90, "confidence": 1 },
                { "score": 71, "confidence": 1 },
                { "score": 30, "confidence": 1 },
                { "score": 95, "confidence": 1 },
            ]
        })
        .to_string();
        let (base, server) = serve_once(200, &body);
        let out = block_on(score_probes_with(&client(), &base, &probes, "d")).unwrap();
        server.join().unwrap();

        let scores: Vec<f32> = out.iter().map(|s| s.score).collect();
        assert_eq!(scores, vec![55.0, 55.0, 55.0, 30.0, 95.0]);
        for s in &out[..4] {
            assert!(s.user_data, "{}", s.category);
            assert_eq!(s.data_loss.as_deref(), probes::data_loss(&s.category));
            assert!(s.score <= UI_AUTO_SELECT_THRESHOLD && s.score < USER_DATA_SCORE_CAP + 0.01);
        }
        assert!(!out[4].user_data);
        assert!(out[4].data_loss.is_none());
    }

    #[test]
    fn heuristic_scores_for_user_data_are_capped() {
        let day = 86_400;
        let probes: Vec<ProbeReport> = USER_DATA
            .iter()
            .map(|c| probe(c, 50 * GB, Some(90 * day)))
            .chain([probe("xcode", 50 * GB, Some(90 * day))])
            .collect();
        let out = block_on(score_probes_with(&client(), &dead_url(), &probes, "d")).unwrap();
        for s in &out[..4] {
            assert_eq!(s.score, USER_DATA_SCORE_CAP, "{}", s.category);
            assert!(s.score > 40.0 && s.score <= UI_AUTO_SELECT_THRESHOLD);
        }
        assert_eq!(out[4].score, 100.0);
    }

    #[test]
    fn cap_ignores_what_the_payload_claims() {
        let mut spoofed = probe("docker_vm", 9 * GB, None);
        spoofed.user_data = false;
        spoofed.data_loss = None;
        let mut honest_cache = probe("node", 9 * GB, None);
        honest_cache.user_data = true;
        let out = heuristic_scores(&[spoofed, honest_cache]);
        assert_eq!(out[0].score, USER_DATA_SCORE_CAP);
        assert!(out[0].user_data);
        assert_eq!(out[1].score, 85.0);
        assert!(!out[1].user_data);
    }

    #[test]
    fn low_user_data_scores_are_left_alone() {
        let out = heuristic_scores(&[probe("ai_ml", MB, None)]);
        assert_eq!(out[0].score, 30.0);
    }

    #[test]
    fn probe_report_without_new_fields_still_deserializes() {
        let p: ProbeReport = serde_json::from_str(
            r#"{"category":"docker_vm","display_name":"D","total_bytes":1,"cleanable_bytes":1,
                "item_count":1,"last_used_secs":null,"confidence":1,"details":""}"#,
        )
        .unwrap();
        assert!(!p.user_data);
        assert_eq!(heuristic_scores(&[p])[0].user_data, true);
    }

    fn assert_heuristic(out: &[ScoredProbe], probes: &[ProbeReport]) {
        assert_eq!(out.len(), probes.len());
        for (s, p) in out.iter().zip(probes) {
            assert_eq!(s.category, p.category);
            assert_eq!(s.score, heuristic_score(p));
            assert_eq!(s.confidence, 0.6);
        }
    }

    #[test]
    fn rate_limit_and_upstream_errors_fall_back_to_heuristic() {
        let probes = [probe("a", 2 * GB, None), probe("b", MB, None)];
        for (status, body) in [
            (429, r#"{"error":"Rate limit exceeded (10/hour)"}"#),
            (502, r#"{"error":"Jev API error: 500"}"#),
            (400, r#"{"error":"Missing device_id or questions"}"#),
        ] {
            let (base, server) = serve_once(status, body);
            let out = block_on(score_probes_with(&client(), &base, &probes, "d")).unwrap();
            server.join().unwrap();
            assert_heuristic(&out, &probes);
        }
    }

    #[test]
    fn malformed_or_short_response_falls_back_to_heuristic() {
        let probes = [probe("a", 2 * GB, None), probe("b", MB, None)];
        for body in [
            "nope",
            r#"{"results":[]}"#,
            r#"{"scores":[{"score":99,"confidence":1}]}"#,
        ] {
            let (base, server) = serve_once(200, body);
            let out = block_on(score_probes_with(&client(), &base, &probes, "d")).unwrap();
            server.join().unwrap();
            assert_heuristic(&out, &probes);
        }
    }

    #[test]
    fn unreachable_worker_falls_back_to_heuristic() {
        let probes = [probe("a", 2 * GB, None)];
        let out = block_on(score_probes_with(&client(), &dead_url(), &probes, "d")).unwrap();
        assert_heuristic(&out, &probes);
    }

    #[test]
    fn empty_probes_skip_network() {
        let out = block_on(score_probes_with(&client(), &dead_url(), &[], "d")).unwrap();
        assert!(out.is_empty());
    }

    // Needs a local `wrangler dev` + mock JEV with a license seeded for "e2e-dev"; see worker/README.md.
    #[test]
    #[ignore]
    fn e2e_against_local_worker() {
        let base = std::env::var("KYRA_WORKER_URL").unwrap_or("http://127.0.0.1:8787".into());
        let probes = [probe("node", 2 * GB, None), probe("rust", 5 * MB, None)];
        let out = block_on(score_probes_with(&client(), &base, &probes, "e2e-dev")).unwrap();
        assert_ne!(
            out[0].confidence, 0.6,
            "fell back to heuristic; is the worker up and seeded?"
        );
        assert_eq!(out.len(), 2);

        let err = block_on(score_probes_with(
            &client(),
            &base,
            &probes,
            "unlicensed-device",
        ))
        .unwrap_err();
        assert_eq!(err, "No active license");

        let dir = crate::commands::guardian::test_support::TestDir::new("e2e-license");
        let lic = block_on(crate::commands::guardian::license::check_license_with(
            &client(),
            &base,
            "e2e-dev",
            dir.path(),
        ))
        .unwrap();
        assert!(lic.active);
    }
}
