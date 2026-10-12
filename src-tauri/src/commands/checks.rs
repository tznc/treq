use crate::core::checks_logs::{LogBucket, LogQuery, LogRecordView, SqlResult};
use crate::core::{JobResult, RunSummary, SetupScriptStatus, WorkflowInfo};

#[tauri::command]
pub async fn list_workflows(repo_path: String) -> Result<Vec<WorkflowInfo>, String> {
  tauri::async_runtime::spawn_blocking(move || crate::core::list_workflows_sync(&repo_path))
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn run_workflow_job(
  repo_path: String,
  filename: String,
  job_id: String,
  workspace_id: i64,
  workspace_path: String,
) -> Result<JobResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::run_workflow_job_sync(
      &repo_path,
      &filename,
      &job_id,
      workspace_id,
      &workspace_path,
    )
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn run_workflow(
  repo_path: String,
  filename: String,
  workspace_id: i64,
  workspace_path: String,
) -> Result<Vec<JobResult>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::run_workflow_sync(&repo_path, &filename, workspace_id, &workspace_path)
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn is_repo_trusted(repo_path: String) -> Result<bool, String> {
  tauri::async_runtime::spawn_blocking(move || Ok(crate::local_db::is_repo_trusted(&repo_path)))
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn trust_repo(repo_path: String) -> Result<(), String> {
  tauri::async_runtime::spawn_blocking(move || crate::local_db::trust_repo(&repo_path))
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_workflow_runs(
  repo_path: String,
  workspace_id: i64,
  filename: String,
  limit: Option<i64>,
) -> Result<Vec<RunSummary>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::list_workflow_runs_sync(&repo_path, workspace_id, &filename, limit.unwrap_or(20))
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_run_logs(
  repo_path: String,
  run_id: i64,
  job_id: String,
  levels: Option<Vec<String>>,
  search: Option<String>,
  step_index: Option<i64>,
  limit: Option<i64>,
  offset: Option<i64>,
) -> Result<Vec<LogRecordView>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let query = LogQuery {
      levels,
      search,
      step_index,
      limit,
      offset,
    };
    crate::core::get_run_logs_sync(&repo_path, run_id, &job_id, &query)
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_repo_logs(
  repo_path: String,
  levels: Option<Vec<String>>,
  search: Option<String>,
  limit: Option<i64>,
  offset: Option<i64>,
) -> Result<Vec<LogRecordView>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let query = LogQuery {
      levels,
      search,
      step_index: None,
      limit,
      offset,
    };
    crate::core::checks_logs::query_repo_logs(&repo_path, &query)
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_log_timeseries(
  repo_path: String,
  levels: Option<Vec<String>>,
  search: Option<String>,
  bucket_seconds: Option<i64>,
) -> Result<Vec<LogBucket>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let query = LogQuery {
      levels,
      search,
      step_index: None,
      limit: None,
      offset: None,
    };
    crate::core::checks_logs::query_log_timeseries(&repo_path, &query, bucket_seconds.unwrap_or(1))
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn run_logs_sql(
  repo_path: String,
  sql: String,
  max_rows: Option<i64>,
) -> Result<SqlResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::checks_logs::run_logs_sql(&repo_path, &sql, max_rows.unwrap_or(500))
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_workspace_setup_status(
  repo_path: String,
  workspace_id: i64,
) -> Result<SetupScriptStatus, String> {
  tauri::async_runtime::spawn_blocking(move || {
    crate::core::get_setup_script_status_sync(&repo_path, workspace_id)
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rerun_workspace_setup_script(
  repo_path: String,
  workspace_id: i64,
  workspace_path: String,
) -> Result<JobResult, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let config = crate::repo_config::parse_config(&repo_path)?;
    let script = config
      .setup_script
      .filter(|s| !s.trim().is_empty())
      .ok_or_else(|| "No setup_script configured in .treq/config.yaml".to_string())?;
    crate::core::run_setup_script_sync(&repo_path, workspace_id, &workspace_path, &script)
  })
  .await
  .map_err(|e| e.to_string())?
}
