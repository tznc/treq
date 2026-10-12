use crate::core::checks_logs::{LogBucket, LogQuery, LogRecordView, SqlResult};

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
