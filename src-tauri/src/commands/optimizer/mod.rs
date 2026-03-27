pub mod runner;
pub mod tasks;

use serde::Serialize;

/// An optimization task definition.
#[derive(Clone, Serialize)]
pub struct OptTask {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: String,
    pub needs_admin: bool,
    pub warning: Option<String>,
}

/// Status update for a single task during execution.
#[derive(Clone, Serialize)]
pub struct OptTaskStatus {
    pub task_id: String,
    pub status: String, // "running" | "done" | "error" | "skipped"
    pub message: Option<String>,
}

/// Final result of running optimization tasks.
#[derive(Clone, Serialize)]
pub struct OptResult {
    pub tasks_run: usize,
    pub tasks_succeeded: usize,
    pub tasks_failed: usize,
    pub tasks_skipped: usize,
}

use tauri::Emitter;

#[tauri::command]
pub fn get_optimize_tasks() -> Vec<OptTask> {
    tasks::all_tasks()
}

#[tauri::command]
pub fn run_optimize_tasks(
    app: tauri::AppHandle,
    task_ids: Vec<String>,
) -> Result<OptResult, String> {
    let all_tasks = tasks::all_tasks();
    let selected: Vec<OptTask> = all_tasks
        .into_iter()
        .filter(|t| task_ids.contains(&t.id))
        .collect();

    if selected.is_empty() {
        return Ok(OptResult {
            tasks_run: 0,
            tasks_succeeded: 0,
            tasks_failed: 0,
            tasks_skipped: 0,
        });
    }

    let result = runner::run_tasks(&selected, |status| {
        let _ = app.emit("optimize-status", status);
    });

    Ok(result)
}
