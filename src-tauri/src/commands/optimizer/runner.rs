use std::process::Command;

use super::{OptResult, OptTask, OptTaskStatus};

/// Runs a shell command and returns (success, output_message).
fn run_shell(command: &str, needs_admin: bool) -> (bool, String) {
    let result = if needs_admin {
        // Use osascript to show native macOS password dialog for admin tasks.
        // Escape backslashes and double-quotes for AppleScript string.
        let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            escaped
        );
        Command::new("osascript").arg("-e").arg(&script).output()
    } else {
        Command::new("sh").arg("-c").arg(command).output()
    };

    match result {
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

        let (success, message) = run_shell(&task.command, task.needs_admin);

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
