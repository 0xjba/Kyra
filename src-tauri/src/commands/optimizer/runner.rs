use std::process::Command;

use super::{OptResult, OptTask, OptTaskStatus};

/// Runs a shell command and returns (success, output_message).
fn run_shell(command: &str) -> (bool, String) {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                (true, stdout)
            } else {
                let msg = if stderr.is_empty() { stdout } else { stderr };
                (false, msg.trim().to_string())
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Runs the given optimization tasks sequentially.
/// Calls `on_status` with status updates for each task.
pub fn run_tasks<F>(tasks: &[OptTask], mut on_status: F) -> OptResult
where
    F: FnMut(&OptTaskStatus),
{
    let mut succeeded: usize = 0;
    let mut failed: usize = 0;
    let skipped: usize = 0;

    for task in tasks {
        on_status(&OptTaskStatus {
            task_id: task.id.clone(),
            status: "running".into(),
            message: None,
        });

        let (success, message) = run_shell(&task.command);

        if success {
            succeeded += 1;
            on_status(&OptTaskStatus {
                task_id: task.id.clone(),
                status: "done".into(),
                message: None,
            });
        } else {
            failed += 1;
            on_status(&OptTaskStatus {
                task_id: task.id.clone(),
                status: "error".into(),
                message: Some(message),
            });
        }
    }

    OptResult {
        tasks_run: succeeded + failed + skipped,
        tasks_succeeded: succeeded,
        tasks_failed: failed,
        tasks_skipped: skipped,
    }
}
