use super::{OptResult, OptTask, OptTaskStatus};

pub fn run_tasks<F>(_tasks: &[OptTask], _on_status: F) -> OptResult
where
    F: FnMut(&OptTaskStatus),
{
    OptResult {
        tasks_run: 0,
        tasks_succeeded: 0,
        tasks_failed: 0,
        tasks_skipped: 0,
    }
}
