use super::types::ProbeReport;
use std::time::Duration;

mod ai_ml;
mod apps;
mod docker;
mod homebrew;
mod ide;
mod node;
mod python;
mod rust_lang;
mod system;
mod xcode;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

async fn with_timeout<F>(fut: F) -> Option<ProbeReport>
where
    F: std::future::Future<Output = Option<ProbeReport>>,
{
    match tokio::time::timeout(PROBE_TIMEOUT, fut).await {
        Ok(report) => report,
        Err(_) => None,
    }
}

pub async fn run_all_probes() -> Vec<ProbeReport> {
    let handles: Vec<tokio::task::JoinHandle<Option<ProbeReport>>> = vec![
        tokio::spawn(with_timeout(docker::probe())),
        tokio::spawn(with_timeout(homebrew::probe())),
        tokio::spawn(with_timeout(xcode::probe())),
        tokio::spawn(with_timeout(node::probe())),
        tokio::spawn(with_timeout(rust_lang::probe())),
        tokio::spawn(with_timeout(python::probe())),
        tokio::spawn(with_timeout(ide::probe())),
        tokio::spawn(with_timeout(ai_ml::probe())),
        tokio::spawn(with_timeout(apps::probe())),
        tokio::spawn(with_timeout(system::probe())),
    ];

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(Some(report)) = handle.await {
            if report.cleanable_bytes > 0 {
                results.push(report);
            }
        }
    }
    results.sort_by(|a, b| b.cleanable_bytes.cmp(&a.cleanable_bytes));
    results
}
