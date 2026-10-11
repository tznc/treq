use crate::core::checks_logs::{
  infer_level, job_log_relative_path, make_log_line, now_timestamp, LogWriter,
};
use crate::lock_ext::LockExt;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const STEP_TIMEOUT_SECS: u64 = 60;
const MAX_CONCURRENT_JOBS: usize = 4;

// ── Internal YAML structs ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct WorkflowFile {
  name: String,
  jobs: HashMap<String, JobDef>,
}

#[derive(Deserialize)]
struct JobDef {
  name: Option<String>,
  steps: Vec<StepDef>,
}

#[derive(Deserialize)]
struct StepDef {
  name: String,
  run: String,
  #[serde(rename = "working-directory")]
  working_directory: Option<String>,
  env: Option<HashMap<String, String>>,
}

// ── Public API types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowInfo {
  pub filename: String,
  pub name: String,
  pub jobs: Vec<JobInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JobInfo {
  pub id: String,
  pub name: String,
  pub steps: Vec<StepInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StepInfo {
  pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StepResult {
  pub name: String,
  pub success: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JobResult {
  pub job_id: String,
  pub steps: Vec<StepResult>,
  pub success: bool,
}

// ── Validation helpers ───────────────────────────────────────────────────────

fn validate_filename(filename: &str) -> Result<(), String> {
  if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
    return Err(format!(
      "Invalid workflow filename '{}': must not contain path separators or '..'",
      filename
    ));
  }
  if !filename.ends_with(".yaml") && !filename.ends_with(".yml") {
    return Err(format!(
      "Invalid workflow filename '{}': must have .yaml or .yml extension",
      filename
    ));
  }
  Ok(())
}

fn validate_working_directory(wd: &str) -> Result<(), String> {
  // `Path::is_absolute()` only recognizes drive-rooted paths on Windows (e.g. `C:\foo`),
  // so a Unix-style `/foo` slips past it there. Reject a leading separator explicitly.
  if Path::new(wd).is_absolute() || wd.starts_with('/') || wd.starts_with('\\') {
    return Err(format!(
      "working-directory must be a relative path, got: '{}'",
      wd
    ));
  }
  if wd.split('/').any(|c| c == "..") || wd.split('\\').any(|c| c == "..") {
    return Err(format!(
      "working-directory must not traverse parent directories: '{}'",
      wd
    ));
  }
  Ok(())
}

// ── Public functions ─────────────────────────────────────────────────────────

pub fn list_workflows_sync(repo_path: &str) -> Result<Vec<WorkflowInfo>, String> {
  let workflows_dir = Path::new(repo_path).join(".treq").join("workflows");
  if !workflows_dir.exists() {
    return Ok(vec![]);
  }

  let mut entries: Vec<_> = std::fs::read_dir(&workflows_dir)
    .map_err(|e| format!("Failed to read workflows dir: {}", e))?
    .filter_map(|e| e.ok())
    .filter(|e| {
      let name = e.file_name();
      let name = name.to_string_lossy();
      name.ends_with(".yaml") || name.ends_with(".yml")
    })
    .collect();

  entries.sort_by_key(|e| e.file_name());

  let canonical_dir = workflows_dir
    .canonicalize()
    .map_err(|e| format!("Failed to access workflows directory: {}", e))?;

  let mut workflows = Vec::new();
  for entry in entries {
    let filename = entry.file_name().to_string_lossy().to_string();

    // Verify each file stays within the workflows directory.
    let canonical_file = match entry.path().canonicalize() {
      Ok(p) => p,
      Err(_) => continue,
    };
    if !canonical_file.starts_with(&canonical_dir) {
      continue;
    }

    let content = match std::fs::read_to_string(entry.path()) {
      Ok(c) => c,
      Err(_) => continue,
    };

    let wf: WorkflowFile = match serde_yaml::from_str(&content) {
      Ok(w) => w,
      Err(_) => continue, // skip invalid YAML files
    };

    let mut jobs: Vec<JobInfo> = wf
      .jobs
      .into_iter()
      .map(|(id, def)| JobInfo {
        id: id.clone(),
        name: def.name.unwrap_or(id),
        steps: def
          .steps
          .into_iter()
          .map(|s| StepInfo { name: s.name })
          .collect(),
      })
      .collect();
    jobs.sort_by(|a, b| a.id.cmp(&b.id));

    workflows.push(WorkflowInfo {
      filename,
      name: wf.name,
      jobs,
    });
  }

  Ok(workflows)
}

/// Runs a single job as its own run (one row in the run history).
pub fn run_workflow_job_sync(
  repo_path: &str,
  filename: &str,
  job_id: &str,
  workspace_id: i64,
  workspace_path: &str,
) -> Result<JobResult, String> {
  let run_id = crate::local_db::create_workflow_run(repo_path, workspace_id, filename)?;
  let result = run_job_in_run(
    repo_path,
    filename,
    job_id,
    workspace_id,
    workspace_path,
    run_id,
    0,
  );
  let status = match &result {
    Ok(r) if r.success => "passed",
    Ok(_) => "failed",
    Err(_) => "failed",
  };
  crate::local_db::finish_workflow_run(repo_path, run_id, status)?;
  if matches!(&result, Ok(r) if r.success) {
    maybe_autosave_on_pass(repo_path, workspace_path);
  }
  result
}

/// Executes one job's steps inside an already-open run.
fn run_job_in_run(
  repo_path: &str,
  filename: &str,
  job_id: &str,
  workspace_id: i64,
  workspace_path: &str,
  run_id: i64,
  position: i64,
) -> Result<JobResult, String> {
  if !crate::local_db::is_repo_trusted(repo_path) {
    return Err("repository_not_trusted: Trust this repository before running checks".to_string());
  }

  ensure_setup_script_complete(repo_path, workspace_id)?;

  validate_filename(filename)?;

  let workflows_dir = Path::new(repo_path).join(".treq").join("workflows");
  let file_path = workflows_dir.join(filename);

  // Verify file stays inside the workflows directory after path normalization.
  let canonical_dir = workflows_dir
    .canonicalize()
    .map_err(|e| format!("Failed to access workflows directory: {}", e))?;
  let canonical_file = file_path
    .canonicalize()
    .map_err(|_| format!("Workflow file not found: '{}'", filename))?;
  if !canonical_file.starts_with(&canonical_dir) {
    return Err(format!(
      "Workflow file '{}' is outside the workflows directory",
      filename
    ));
  }

  let content = std::fs::read_to_string(&file_path)
    .map_err(|e| format!("Failed to read '{}': {}", filename, e))?;
  let wf: WorkflowFile =
    serde_yaml::from_str(&content).map_err(|e| format!("Failed to parse '{}': {}", filename, e))?;

  let job_def = wf
    .jobs
    .into_iter()
    .find(|(id, _)| id == job_id)
    .map(|(_, def)| def)
    .ok_or_else(|| format!("Job '{}' not found in '{}'", job_id, filename))?;

  let base_dir = if Path::new(workspace_path).is_dir() {
    workspace_path.to_string()
  } else {
    repo_path.to_string()
  };

  let started_at = Utc::now().to_rfc3339();
  let result = execute_job_steps(repo_path, job_id, &job_def.steps, &base_dir, run_id)?;
  let log_path = job_log_relative_path(run_id, job_id);

  store_job_result(repo_path, run_id, position, &started_at, &log_path, &result)?;

  Ok(result)
}

/// Runs a job's steps sequentially against an already-created run, streaming
/// output into that run's job log. Shared by workflow jobs and the
/// post-workspace-create setup script, which is just a single-step job.
fn execute_job_steps(
  repo_path: &str,
  job_id: &str,
  steps: &[StepDef],
  base_dir: &str,
  run_id: i64,
) -> Result<JobResult, String> {
  let mut step_results = Vec::new();

  let writer = LogWriter::create(repo_path, run_id, job_id)?;

  for (step_index, step) in steps.iter().enumerate() {
    let cwd = if let Some(wd) = &step.working_directory {
      validate_working_directory(wd)?;
      Path::new(base_dir).join(wd).to_string_lossy().to_string()
    } else {
      base_dir.to_string()
    };

    let sink: crate::core::shell_step::LineSink = {
      let writer = writer.clone();
      let step_name = step.name.clone();
      let job_id = job_id.to_string();
      std::sync::Arc::new(move |stream, message| {
        let _ = writer.write_line(&make_log_line(
          now_timestamp(),
          run_id,
          &job_id,
          step_index as i64,
          &step_name,
          stream,
          infer_level(message),
          message,
        ));
      })
    };
    let outcome = crate::core::shell_step::run(
      &crate::core::shell_step::ShellStep {
        script: &step.run,
        cwd: Path::new(&cwd),
        env: step.env.as_ref(),
        timeout: Duration::from_secs(STEP_TIMEOUT_SECS),
      },
      sink,
    )
    .map_err(|e| format!("Failed to run step '{}': {}", step.name, e))?;

    if matches!(outcome, crate::core::shell_step::StepOutcome::TimedOut) {
      let _ = writer.write_line(&make_log_line(
        now_timestamp(),
        run_id,
        job_id,
        step_index as i64,
        &step.name,
        "stderr",
        "error",
        &format!(
          "Step timed out after {} seconds and was terminated.",
          STEP_TIMEOUT_SECS
        ),
      ));
      let _ = writer.flush();
      step_results.push(StepResult {
        name: step.name.clone(),
        success: false,
      });
      break;
    }

    let success = outcome.success();
    step_results.push(StepResult {
      name: step.name.clone(),
      success,
    });

    if !success {
      break;
    }
  }

  writer.flush()?;

  let overall_success = step_results.iter().all(|s| s.success);
  Ok(JobResult {
    job_id: job_id.to_string(),
    steps: step_results,
    success: overall_success,
  })
}

pub fn run_workflow_sync(
  repo_path: &str,
  filename: &str,
  workspace_id: i64,
  workspace_path: &str,
) -> Result<Vec<JobResult>, String> {
  let workflows = list_workflows_sync(repo_path)?;
  let wf = workflows
    .into_iter()
    .find(|w| w.filename == filename)
    .ok_or_else(|| format!("Workflow '{}' not found", filename))?;

  // All jobs share one run row so history shows one item per invocation.
  let job_ids: Vec<String> = wf.jobs.into_iter().map(|j| j.id).collect();
  let total = job_ids.len();
  let run_id = crate::local_db::create_workflow_run(repo_path, workspace_id, filename)?;

  let rp = repo_path.to_string();
  let fn_ = filename.to_string();
  let wp = workspace_path.to_string();

  let (result_tx, result_rx) = std::sync::mpsc::channel::<Result<JobResult, String>>();
  let mut job_iter = job_ids.into_iter().enumerate();
  let mut in_flight = 0usize;
  let mut results = Vec::with_capacity(total);
  let mut failure: Option<String> = None;

  loop {
    // Refill the slot pool up to MAX_CONCURRENT_JOBS.
    while in_flight < MAX_CONCURRENT_JOBS {
      match job_iter.next() {
        Some((position, job_id)) => {
          let rp = rp.clone();
          let fn_ = fn_.clone();
          let wp = wp.clone();
          let tx = result_tx.clone();
          std::thread::spawn(move || {
            let r = run_job_in_run(
              &rp,
              &fn_,
              &job_id,
              workspace_id,
              &wp,
              run_id,
              position as i64,
            );
            tx.send(r).ok();
          });
          in_flight += 1;
        }
        None => break,
      }
    }

    if in_flight == 0 {
      break;
    }

    match result_rx.recv() {
      Ok(Ok(r)) => {
        results.push(r);
        in_flight -= 1;
      }
      // Keep draining so one bad job doesn't abort unrelated jobs.
      Ok(Err(e)) => {
        failure.get_or_insert(e);
        in_flight -= 1;
      }
      Err(_) => {
        failure.get_or_insert_with(|| "Job thread disconnected unexpectedly".to_string());
        in_flight -= 1;
      }
    }
  }

  let status = if failure.is_some() || results.iter().any(|r| !r.success) {
    "failed"
  } else {
    "passed"
  };
  crate::local_db::finish_workflow_run(repo_path, run_id, status)?;

  if let Some(err) = failure {
    return Err(err);
  }

  // Restore workflow job order; completion order is nondeterministic.
  results.sort_by(|a, b| a.job_id.cmp(&b.job_id));
  if status == "passed" {
    maybe_autosave_on_pass(repo_path, workspace_path);
  }
  Ok(results)
}

/// Snapshot a passing check into an autosave commit when the working copy is dirty.
fn maybe_autosave_on_pass(repo_path: &str, workspace_path: &str) {
  if !Path::new(workspace_path).is_dir() {
    return;
  }
  let lock = crate::core::repo::commit_lock_for_repo(repo_path);
  let _guard = lock.lock_or_recover();
  let Ok(changes) = crate::jj::jj_get_changed_files(workspace_path) else {
    return;
  };
  if changes.is_empty() {
    return;
  }
  let paths: Vec<String> = changes.into_iter().map(|change| change.path).collect();
  let message = crate::jj::autosave_commit_message(&paths);
  let _ = crate::jj::jj_commit(workspace_path, &message);
}

fn store_job_result(
  repo_path: &str,
  run_id: i64,
  position: i64,
  started_at: &str,
  log_path: &str,
  result: &JobResult,
) -> Result<(), String> {
  let steps_json = serde_json::to_string(&result.steps)
    .map_err(|e| format!("Failed to serialize steps: {}", e))?;
  crate::local_db::add_workflow_job_result(
    repo_path,
    run_id,
    &result.job_id,
    position,
    if result.success { "passed" } else { "failed" },
    started_at,
    &Utc::now().to_rfc3339(),
    &steps_json,
    Some(log_path),
  )?;
  Ok(())
}

// ── Post-workspace-create setup script ──────────────────────────────────────

/// Synthetic workflow filename used to store the setup script's runs in the
/// same `workflow_runs`/`workflow_job_results` tables as regular checks, so
/// it gets run history and browsable logs for free.
const SETUP_SCRIPT_FILENAME: &str = "__setup__";
const SETUP_SCRIPT_JOB_ID: &str = "setup";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SetupScriptStatus {
  /// Whether `.treq/config.yaml` declares a `setup_script`.
  pub configured: bool,
  /// "not_configured" | "pending" | "running" | "passed" | "failed"
  pub status: String,
  pub run_id: Option<i64>,
}

/// Blocks running checks in a workspace until its setup script has finished
/// (successfully or not) — checks should never run against a workspace whose
/// setup hasn't executed. No-op when no setup script is configured.
fn ensure_setup_script_complete(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  let config = crate::repo_config::parse_config(repo_path)?;
  let Some(script) = config.setup_script else {
    return Ok(());
  };
  if script.trim().is_empty() {
    return Ok(());
  }

  let runs =
    crate::local_db::list_workflow_runs(repo_path, workspace_id, SETUP_SCRIPT_FILENAME, 1)?;
  match runs.first() {
    Some(run) if run.status != "running" => Ok(()),
    Some(_) => Err("setup_script_pending: The workspace setup script is still running".to_string()),
    None => Err("setup_script_pending: The workspace setup script has not run yet".to_string()),
  }
}

/// The repo's `.treq/config.yaml` `setup_script`, when one is set and non-blank.
pub fn configured_setup_script(repo_path: &str) -> Result<Option<String>, String> {
  let config = crate::repo_config::parse_config(repo_path)?;
  Ok(config.setup_script.filter(|s| !s.trim().is_empty()))
}

/// Runs the repo's configured setup script for a freshly created workspace.
/// Stored as a one-job run under `SETUP_SCRIPT_FILENAME` so it shows up in the
/// same run-history / log-viewer machinery as regular workflow checks.
pub fn run_setup_script_sync(
  repo_path: &str,
  workspace_id: i64,
  workspace_path: &str,
  script: &str,
) -> Result<JobResult, String> {
  let run_id =
    crate::local_db::create_workflow_run(repo_path, workspace_id, SETUP_SCRIPT_FILENAME)?;

  let base_dir = if Path::new(workspace_path).is_dir() {
    workspace_path.to_string()
  } else {
    repo_path.to_string()
  };
  let step = StepDef {
    name: "Setup".to_string(),
    run: script.to_string(),
    working_directory: None,
    env: None,
  };

  let started_at = Utc::now().to_rfc3339();
  let result = execute_job_steps(
    repo_path,
    SETUP_SCRIPT_JOB_ID,
    std::slice::from_ref(&step),
    &base_dir,
    run_id,
  );

  let status = match &result {
    Ok(r) if r.success => "passed",
    _ => "failed",
  };
  crate::local_db::finish_workflow_run(repo_path, run_id, status)?;

  let result = result?;
  let log_path = job_log_relative_path(run_id, SETUP_SCRIPT_JOB_ID);
  store_job_result(repo_path, run_id, 0, &started_at, &log_path, &result)?;

  Ok(result)
}

pub fn get_setup_script_status_sync(
  repo_path: &str,
  workspace_id: i64,
) -> Result<SetupScriptStatus, String> {
  let config = crate::repo_config::parse_config(repo_path)?;
  if config
    .setup_script
    .filter(|s| !s.trim().is_empty())
    .is_none()
  {
    return Ok(SetupScriptStatus {
      configured: false,
      status: "not_configured".to_string(),
      run_id: None,
    });
  }

  let runs =
    crate::local_db::list_workflow_runs(repo_path, workspace_id, SETUP_SCRIPT_FILENAME, 1)?;
  let (status, run_id) = match runs.first() {
    Some(run) => (run.status.clone(), Some(run.id)),
    None => ("pending".to_string(), None),
  };

  Ok(SetupScriptStatus {
    configured: true,
    status,
    run_id,
  })
}

// ── Run history and logs ─────────────────────────────────────────────────────

/// A past invocation, with its per-job outcomes, for the run-history list.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunSummary {
  pub id: i64,
  pub filename: String,
  pub status: String,
  pub started_at: String,
  pub completed_at: Option<String>,
  pub jobs: Vec<RunJobSummary>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunJobSummary {
  pub job_id: String,
  pub status: String,
  pub steps: Vec<StepResult>,
  pub has_logs: bool,
}

pub fn list_workflow_runs_sync(
  repo_path: &str,
  workspace_id: i64,
  filename: &str,
  limit: i64,
) -> Result<Vec<RunSummary>, String> {
  let runs = crate::local_db::list_workflow_runs(repo_path, workspace_id, filename, limit)?;
  let mut summaries = Vec::with_capacity(runs.len());
  for run in runs {
    let job_rows = crate::local_db::list_workflow_job_results(repo_path, run.id)?;
    let jobs = job_rows
      .into_iter()
      .map(|j| RunJobSummary {
        job_id: j.job_id,
        status: j.status,
        steps: serde_json::from_str(&j.steps_json).unwrap_or_default(),
        has_logs: j.log_path.is_some(),
      })
      .collect();
    summaries.push(RunSummary {
      id: run.id,
      filename: run.filename,
      status: run.status,
      started_at: run.started_at,
      completed_at: run.completed_at,
      jobs,
    });
  }
  Ok(summaries)
}

/// Confirm a job actually recorded logs before querying the telemetry
/// database for them — records are looked up by `run_id`/`job_id`, not by a
/// filesystem path, so there is nothing left to canonicalize.
fn ensure_logs_recorded(repo_path: &str, run_id: i64, job_id: &str) -> Result<(), String> {
  crate::local_db::get_job_log_path(repo_path, run_id, job_id)?
    .ok_or_else(|| format!("No logs recorded for job '{}' in run {}", job_id, run_id))?;
  Ok(())
}

pub fn get_run_logs_sync(
  repo_path: &str,
  run_id: i64,
  job_id: &str,
  query: &crate::core::checks_logs::LogQuery,
) -> Result<Vec<crate::core::checks_logs::LogRecordView>, String> {
  ensure_logs_recorded(repo_path, run_id, job_id)?;
  crate::core::checks_logs::query_logs(repo_path, run_id, job_id, query)
}

pub fn export_run_logs_sync(
  repo_path: &str,
  run_id: i64,
  job_id: &str,
  dest_path: &str,
) -> Result<String, String> {
  ensure_logs_recorded(repo_path, run_id, job_id)?;
  crate::core::checks_logs::export_logs(repo_path, run_id, job_id, dest_path)
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  fn make_workflow(jobs_yaml: &str) -> String {
    format!(
      "name: Test Workflow\non:\n  workflow_dispatch: {{}}\njobs:\n{}",
      jobs_yaml
    )
  }

  fn write_workflow(dir: &TempDir, filename: &str, content: &str) -> String {
    let workflows_dir = dir.path().join(".treq").join("workflows");
    fs::create_dir_all(&workflows_dir).unwrap();
    fs::write(workflows_dir.join(filename), content).unwrap();
    dir.path().to_string_lossy().to_string()
  }

  fn setup_trusted_repo(dir: &TempDir, filename: &str, content: &str) -> String {
    let repo = write_workflow(dir, filename, content);
    crate::local_db::init_local_db(&repo).unwrap();
    crate::local_db::trust_repo(&repo).unwrap();
    repo
  }

  #[test]
  fn test_list_workflows_empty_when_no_dir() {
    let dir = TempDir::new().unwrap();
    let result = list_workflows_sync(&dir.path().to_string_lossy()).unwrap();
    assert!(result.is_empty());
  }

  #[test]
  fn test_list_workflows_parses_yaml() {
    let dir = TempDir::new().unwrap();
    let content = make_workflow(
      "  greet:\n    name: Greet Job\n    steps:\n      - name: Say hi\n        run: echo hi\n",
    );
    let repo = write_workflow(&dir, "ci.yaml", &content);
    let result = list_workflows_sync(&repo).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "Test Workflow");
  }

  #[test]
  fn test_list_workflows_skips_non_yaml() {
    let dir = TempDir::new().unwrap();
    let workflows_dir = dir.path().join(".treq").join("workflows");
    fs::create_dir_all(&workflows_dir).unwrap();
    fs::write(workflows_dir.join("readme.txt"), "not yaml").unwrap();
    let result = list_workflows_sync(&dir.path().to_string_lossy()).unwrap();
    assert!(result.is_empty());
  }

  #[test]
  fn test_list_workflows_skips_invalid_yaml() {
    let dir = TempDir::new().unwrap();
    let workflows_dir = dir.path().join(".treq").join("workflows");
    fs::create_dir_all(&workflows_dir).unwrap();
    fs::write(
      workflows_dir.join("broken.yaml"),
      "this: is: not: valid: yaml: :::",
    )
    .unwrap();
    // valid workflow alongside the broken one
    let content = make_workflow("  j:\n    steps:\n      - name: s\n        run: echo x\n");
    fs::write(workflows_dir.join("valid.yaml"), &content).unwrap();
    let result = list_workflows_sync(&dir.path().to_string_lossy()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].filename, "valid.yaml");
  }

  #[test]
  fn test_list_workflows_sorted_by_filename() {
    let dir = TempDir::new().unwrap();
    let content = make_workflow("  j:\n    steps:\n      - name: s\n        run: echo x\n");
    let repo = write_workflow(&dir, "b.yaml", &content);
    write_workflow(&dir, "a.yaml", &content);
    let result = list_workflows_sync(&repo).unwrap();
    assert_eq!(result.len(), 2);
    assert!(result[0].filename < result[1].filename);
  }

  #[test]
  fn test_run_job_requires_trust() {
    let dir = TempDir::new().unwrap();
    let content =
      make_workflow("  greet:\n    steps:\n      - name: Say hi\n        run: echo hi\n");
    let repo = write_workflow(&dir, "ci.yaml", &content);
    crate::local_db::init_local_db(&repo).unwrap();
    // NOT trusting the repo
    let err = run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap_err();
    assert!(err.contains("repository_not_trusted"));
  }

  #[test]
  fn test_run_job_rejects_path_traversal_filename() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    crate::local_db::init_local_db(&repo).unwrap();
    crate::local_db::trust_repo(&repo).unwrap();
    let err = run_workflow_job_sync(&repo, "../secret.yaml", "job", 0, &repo).unwrap_err();
    assert!(err.contains("Invalid workflow filename"));
  }

  #[test]
  fn test_run_job_success() {
    let dir = TempDir::new().unwrap();
    let content =
      make_workflow("  greet:\n    steps:\n      - name: Say hi\n        run: echo hi\n");
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);
    let result = run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap();
    assert!(result.success);
    assert!(!result.steps.is_empty());
  }

  #[test]
  fn test_run_job_stops_at_first_failure() {
    let dir = TempDir::new().unwrap();
    let content = make_workflow(
            "  check:\n    steps:\n      - name: Fail\n        run: exit 1\n      - name: Skip\n        run: echo skip\n",
        );
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);
    let result = run_workflow_job_sync(&repo, "ci.yaml", "check", 0, &repo).unwrap();
    assert!(!result.success);
    assert_eq!(result.steps.len(), 1);
  }

  #[test]
  fn test_run_job_unknown_job_returns_error() {
    let dir = TempDir::new().unwrap();
    let content = make_workflow("  greet:\n    steps:\n      - name: hi\n        run: echo hi\n");
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);
    let err = run_workflow_job_sync(&repo, "ci.yaml", "nonexistent", 0, &repo).unwrap_err();
    assert!(err.contains("nonexistent"));
  }

  #[test]
  fn test_run_job_env_vars() {
    let dir = TempDir::new().unwrap();
    let content = "name: Env Test\non:\n  workflow_dispatch: {}\njobs:\n  check:\n    steps:\n      - name: Check env\n        run: test \"$MY_VAR\" = \"hello\"\n        env:\n          MY_VAR: hello\n";
    let repo = setup_trusted_repo(&dir, "env.yaml", content);
    let result = run_workflow_job_sync(&repo, "env.yaml", "check", 0, &repo).unwrap();
    assert!(result.success);
  }

  #[test]
  fn test_run_workflow_runs_all_jobs() {
    let dir = TempDir::new().unwrap();
    let content = "name: Multi\non:\n  workflow_dispatch: {}\njobs:\n  job1:\n    steps:\n      - name: s1\n        run: echo a\n  job2:\n    steps:\n      - name: s2\n        run: echo b\n";
    let repo = setup_trusted_repo(&dir, "multi.yaml", content);
    let results = run_workflow_sync(&repo, "multi.yaml", 0, &repo).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.success));
  }

  #[test]
  fn test_run_job_captures_stdout_into_logs() {
    let dir = TempDir::new().unwrap();
    let content = make_workflow(
      "  greet:\n    steps:\n      - name: Say hi\n        run: echo hello-from-step\n",
    );
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);
    run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap();

    let runs = list_workflow_runs_sync(&repo, 0, "ci.yaml", 10).unwrap();
    let logs = get_run_logs_sync(&repo, runs[0].id, "greet", &Default::default()).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].body, "hello-from-step");
  }

  #[test]
  fn test_run_job_captures_stderr_and_marks_error_level() {
    let dir = TempDir::new().unwrap();
    let content = make_workflow(
            "  boom:\n    steps:\n      - name: Fail loudly\n        run: \"echo 'error: something broke' 1>&2; exit 1\"\n",
        );
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);
    run_workflow_job_sync(&repo, "ci.yaml", "boom", 0, &repo).unwrap();

    let runs = list_workflow_runs_sync(&repo, 0, "ci.yaml", 10).unwrap();
    let logs = get_run_logs_sync(&repo, runs[0].id, "boom", &Default::default()).unwrap();
    assert_eq!(logs[0].stream, "stderr");
    assert_eq!(logs[0].severity_text, "ERROR");
  }

  #[test]
  fn test_rerunning_creates_a_separate_run() {
    let dir = TempDir::new().unwrap();
    let content =
      make_workflow("  greet:\n    steps:\n      - name: Say hi\n        run: echo hi\n");
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);
    run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap();
    run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap();

    let runs = list_workflow_runs_sync(&repo, 0, "ci.yaml", 10).unwrap();
    assert_eq!(runs.len(), 2);
    // Newest first, and the older run's logs stay reachable.
    assert!(runs[0].id > runs[1].id);
  }

  #[test]
  fn test_run_workflow_groups_all_jobs_into_one_run() {
    let dir = TempDir::new().unwrap();
    let content = "name: Multi\non:\n  workflow_dispatch: {}\njobs:\n  job1:\n    steps:\n      - name: s1\n        run: echo a\n  job2:\n    steps:\n      - name: s2\n        run: echo b\n";
    let repo = setup_trusted_repo(&dir, "multi.yaml", content);
    run_workflow_sync(&repo, "multi.yaml", 0, &repo).unwrap();

    let runs = list_workflow_runs_sync(&repo, 0, "multi.yaml", 10).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].jobs.len(), 2);
  }

  #[test]
  fn test_export_run_logs_writes_file() {
    let dir = TempDir::new().unwrap();
    let content = make_workflow(
      "  greet:\n    steps:\n      - name: Say hi\n        run: echo exported-line\n",
    );
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);
    run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap();

    let runs = list_workflow_runs_sync(&repo, 0, "ci.yaml", 10).unwrap();
    let dest = dir.path().join("out.log").to_string_lossy().to_string();
    export_run_logs_sync(&repo, runs[0].id, "greet", &dest).unwrap();
    let contents = fs::read_to_string(&dest).unwrap();
    assert!(contents.contains("exported-line"));
  }

  #[test]
  fn test_validate_filename_rejects_path_separators() {
    assert!(validate_filename("sub/dir/ci.yaml").is_err());
    assert!(validate_filename("../escape.yaml").is_err());
    assert!(validate_filename("ci.yaml").is_ok());
    assert!(validate_filename("ci.yml").is_ok());
  }

  #[test]
  fn test_validate_working_directory_rejects_absolute_and_traversal() {
    assert!(validate_working_directory("/absolute/path").is_err());
    assert!(validate_working_directory("../../etc").is_err());
    assert!(validate_working_directory("sub/dir").is_ok());
    assert!(validate_working_directory("frontend").is_ok());
  }

  // ── Post-workspace-create setup script ────────────────────────────────────

  fn write_config(dir: &TempDir, yaml: &str) -> String {
    let treq_dir = dir.path().join(".treq");
    fs::create_dir_all(&treq_dir).unwrap();
    fs::write(treq_dir.join("config.yaml"), yaml).unwrap();
    dir.path().to_string_lossy().to_string()
  }

  #[test]
  fn test_setup_status_not_configured_when_no_setup_script() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    crate::local_db::init_local_db(&repo).unwrap();

    let status = get_setup_script_status_sync(&repo, 0).unwrap();
    assert!(!status.configured);
    assert_eq!(status.status, "not_configured");
  }

  #[test]
  fn test_setup_status_pending_before_first_run() {
    let dir = TempDir::new().unwrap();
    let repo = write_config(&dir, "setup_script: \"echo hi\"\n");
    crate::local_db::init_local_db(&repo).unwrap();

    let status = get_setup_script_status_sync(&repo, 0).unwrap();
    assert!(status.configured);
    assert_eq!(status.status, "pending");
  }

  #[test]
  fn test_checks_blocked_until_setup_script_runs() {
    let dir = TempDir::new().unwrap();
    write_config(&dir, "setup_script: \"echo hi\"\n");
    let content =
      make_workflow("  greet:\n    steps:\n      - name: Say hi\n        run: echo hi\n");
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);

    let err = run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap_err();
    assert!(err.contains("setup_script_pending"));
  }

  #[test]
  fn test_checks_unblocked_after_setup_script_runs() {
    let dir = TempDir::new().unwrap();
    write_config(&dir, "setup_script: \"echo setting up\"\n");
    let content =
      make_workflow("  greet:\n    steps:\n      - name: Say hi\n        run: echo hi\n");
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);

    run_setup_script_sync(&repo, 0, &repo, "echo setting up").unwrap();

    let status = get_setup_script_status_sync(&repo, 0).unwrap();
    assert_eq!(status.status, "passed");

    let result = run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap();
    assert!(result.success);
  }

  #[test]
  fn test_checks_unblocked_even_if_setup_script_failed() {
    let dir = TempDir::new().unwrap();
    write_config(&dir, "setup_script: \"exit 1\"\n");
    let content =
      make_workflow("  greet:\n    steps:\n      - name: Say hi\n        run: echo hi\n");
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);

    run_setup_script_sync(&repo, 0, &repo, "exit 1").unwrap();

    let status = get_setup_script_status_sync(&repo, 0).unwrap();
    assert_eq!(status.status, "failed");

    // Setup having *run* (even unsuccessfully) is enough to unblock checks.
    let result = run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap();
    assert!(result.success);
  }

  #[test]
  fn test_setup_script_only_blocks_matching_workspace() {
    let dir = TempDir::new().unwrap();
    write_config(&dir, "setup_script: \"echo hi\"\n");
    let content =
      make_workflow("  greet:\n    steps:\n      - name: Say hi\n        run: echo hi\n");
    let repo = setup_trusted_repo(&dir, "ci.yaml", &content);

    run_setup_script_sync(&repo, 1, &repo, "echo hi").unwrap();

    // Workspace 1's setup ran; workspace 0's has not.
    let err = run_workflow_job_sync(&repo, "ci.yaml", "greet", 0, &repo).unwrap_err();
    assert!(err.contains("setup_script_pending"));
    let result = run_workflow_job_sync(&repo, "ci.yaml", "greet", 1, &repo).unwrap();
    assert!(result.success);
  }

  #[test]
  fn test_run_setup_script_captures_logs() {
    let dir = TempDir::new().unwrap();
    let repo = write_config(&dir, "setup_script: \"echo setup-output\"\n");
    crate::local_db::init_local_db(&repo).unwrap();

    run_setup_script_sync(&repo, 0, &repo, "echo setup-output").unwrap();

    let runs = list_workflow_runs_sync(&repo, 0, SETUP_SCRIPT_FILENAME, 10).unwrap();
    assert_eq!(runs.len(), 1);
    let logs =
      get_run_logs_sync(&repo, runs[0].id, SETUP_SCRIPT_JOB_ID, &Default::default()).unwrap();
    assert_eq!(logs[0].body, "setup-output");
  }
}
