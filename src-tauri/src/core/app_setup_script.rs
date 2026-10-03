//! Application setup script: a shell script the user sets in Application
//! settings. It runs only when asked: from the Run button in settings, or on
//! every app startup when `always_run` is on. Each run's output is stored as
//! log records next to the app DB, so the Logs tab can show it.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::Utc;
use jj_lib::lock::FileLock;
use serde::{Deserialize, Serialize};

use crate::core::checks_logs::{
  infer_level, make_log_line, now_timestamp, query_repo_logs, strip_ansi, LogQuery, LogRecordView,
  LogWriter,
};
use crate::db::Database;

pub const SCRIPT_KEY: &str = "app_setup_script";
pub const ALWAYS_RUN_KEY: &str = "app_setup_script_always_run";
pub const LAST_RUN_ID_KEY: &str = "app_setup_script_last_run_id";
pub const LAST_RUN_AT_KEY: &str = "app_setup_script_last_run_at";
pub const LAST_STATUS_KEY: &str = "app_setup_script_last_status";

/// Job and step on every stored record; the run id tells runs apart.
const LOG_JOB_ID: &str = "setup";
const LOG_STEP_NAME: &str = "Application setup script";

/// What asked for the run. A manual run always runs the saved script; a
/// startup run only when `always_run` is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
  Startup,
  Manual,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct AppSetupScriptStatus {
  pub script: String,
  pub always_run: bool,
  pub last_run_at: Option<String>,
  /// "passed" | "failed"
  pub last_status: Option<String>,
  pub running: bool,
}

/// Spawned runs that have not finished. Counted from before the thread starts,
/// so a status read right after a run request already sees the run.
static PENDING_RUNS: AtomicUsize = AtomicUsize::new(0);

/// A script still running after this long is killed and recorded as failed,
/// so a hung script cannot hold the run claim and block every later run.
const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub fn get_status(db: &Database) -> Result<AppSetupScriptStatus, String> {
  let get = |key: &str| db.get_setting(key).map_err(|e| e.to_string());
  Ok(AppSetupScriptStatus {
    script: get(SCRIPT_KEY)?.unwrap_or_default(),
    always_run: get(ALWAYS_RUN_KEY)?.as_deref() == Some("true"),
    last_run_at: get(LAST_RUN_AT_KEY)?,
    last_status: get(LAST_STATUS_KEY)?,
    running: PENDING_RUNS.load(Ordering::SeqCst) > 0,
  })
}

pub fn save(db: &Database, script: &str, always_run: bool) -> Result<(), String> {
  db.set_setting(SCRIPT_KEY, script)
    .and_then(|_| db.set_setting(ALWAYS_RUN_KEY, if always_run { "true" } else { "false" }))
    .map_err(|e| e.to_string())
}

/// The script to run for `trigger`, or `None` when it should not run.
fn script_to_run(db: &Database, trigger: Trigger) -> Result<Option<String>, String> {
  let status = get_status(db)?;
  if status.script.trim().is_empty() {
    return Ok(None);
  }
  let run = trigger == Trigger::Manual || status.always_run;
  Ok(run.then_some(status.script))
}

/// The directory whose `.treq/` holds the run logs for the app DB at `db_path`.
fn logs_root(db_path: &Path) -> String {
  db_path
    .with_extension("setup-script-logs")
    .to_string_lossy()
    .into_owned()
}

/// Output lines of every recorded run, newest run first.
pub fn query_logs(db_path: &Path, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
  query_repo_logs(&logs_root(db_path), query)
}

/// Sends one run's output to the app log and, when the store opened, to the
/// run's log records.
#[derive(Clone)]
struct RunLog {
  writer: Option<LogWriter>,
  run_id: i64,
}

impl RunLog {
  fn open(db_path: &Path, run_id: i64) -> Self {
    let writer = LogWriter::create(&logs_root(db_path), run_id, LOG_JOB_ID)
      .inspect_err(|e| log::warn!("App setup script output will not be stored: {}", e))
      .ok();
    Self { writer, run_id }
  }

  fn write(&self, stream: &str, level: &str, message: &str) {
    log::info!("App setup script {}: {}", stream, message);
    if let Some(writer) = &self.writer {
      let _ = writer.write_line(&make_log_line(
        now_timestamp(),
        self.run_id,
        LOG_JOB_ID,
        0,
        LOG_STEP_NAME,
        stream,
        level,
        message,
      ));
    }
  }

  fn output(&self, stream: &'static str, pipe: impl Read + Send + 'static) {
    let log = self.clone();
    std::thread::spawn(move || {
      for line in BufReader::new(pipe).lines().map_while(Result::ok) {
        let message = strip_ansi(&line);
        log.write(stream, infer_level(&message), &message);
      }
    });
  }
}

/// Runs the script with `sh -c` from the home directory. Returns whether it
/// exited successfully within `timeout`.
fn execute(script: &str, timeout: Duration, log: &RunLog) -> bool {
  let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
  let mut command = Command::new("sh");
  command
    .args(["-c", script])
    .current_dir(&home)
    .env("PATH", crate::binary_paths::get_extended_path())
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  // Its own process group, so a timeout also kills what the script started.
  #[cfg(unix)]
  std::os::unix::process::CommandExt::process_group(&mut command, 0);
  let mut child = match command.spawn() {
    Ok(child) => child,
    Err(e) => {
      log.write("stderr", "error", &format!("Failed to start: {}", e));
      return false;
    }
  };
  // Not joined: a background job the script starts can hold the pipes open.
  if let Some(stdout) = child.stdout.take() {
    log.output("stdout", stdout);
  }
  if let Some(stderr) = child.stderr.take() {
    log.output("stderr", stderr);
  }

  let deadline = Instant::now() + timeout;
  loop {
    match child.try_wait() {
      Ok(Some(status)) => {
        log::info!("App setup script exited with {}", status);
        return status.success();
      }
      Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
      Ok(None) => {
        let message = format!("Timed out after {:?} and was killed.", timeout);
        log.write("stderr", "error", &message);
        kill_process_group(&mut child);
        let _ = child.wait();
        return false;
      }
      Err(e) => {
        log.write(
          "stderr",
          "error",
          &format!("Failed waiting for the script: {}", e),
        );
        return false;
      }
    }
  }
}

#[cfg(unix)]
fn kill_process_group(child: &mut Child) {
  // `process_group(0)` made the script's pid its group id.
  unsafe {
    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
  }
}

#[cfg(not(unix))]
fn kill_process_group(child: &mut Child) {
  let _ = child.kill();
}

/// The lock file that serializes runs across every Treq process and thread
/// sharing the app DB at `db_path`.
fn claim_path(db_path: &Path) -> PathBuf {
  db_path.with_extension("setup-script.lock")
}

/// Runs the script when `trigger` calls for it. Returns whether it ran.
///
/// The claim is held for the whole run, so runs from several instances or
/// clicks never overlap and each records its own run id and result.
pub fn run_if_needed(db_path: &Path, trigger: Trigger) -> Result<bool, String> {
  let _claim = FileLock::lock(claim_path(db_path)).map_err(|e| e.to_string())?;
  let db = Database::new(db_path.to_path_buf()).map_err(|e| e.to_string())?;
  let Some(script) = script_to_run(&db, trigger)? else {
    return Ok(false);
  };
  let run_id = db
    .get_setting(LAST_RUN_ID_KEY)
    .map_err(|e| e.to_string())?
    .and_then(|id| id.parse::<i64>().ok())
    .unwrap_or(0)
    + 1;
  db.set_setting(LAST_RUN_ID_KEY, &run_id.to_string())
    .map_err(|e| e.to_string())?;

  let status = if execute(&script, RUN_TIMEOUT, &RunLog::open(db_path, run_id)) {
    "passed"
  } else {
    "failed"
  };
  db.set_setting(LAST_RUN_AT_KEY, &Utc::now().to_rfc3339())
    .and_then(|_| db.set_setting(LAST_STATUS_KEY, status))
    .map_err(|e| e.to_string())?;
  Ok(true)
}

/// The app DB path set at startup.
pub fn app_db_path() -> Result<PathBuf, String> {
  std::env::var("TREQ_APP_DB_PATH")
    .map(PathBuf::from)
    .map_err(|_| "TREQ_APP_DB_PATH is not set".to_string())
}

/// Runs `run_if_needed` on a background thread with its own connection to the
/// app DB, so a long script never blocks startup or the settings page.
pub fn spawn_run_if_needed(trigger: Trigger) {
  match app_db_path() {
    Ok(db_path) => {
      spawn_run(db_path, trigger);
    }
    Err(e) => log::warn!("Skipping app setup script: {}", e),
  }
}

fn spawn_run(db_path: PathBuf, trigger: Trigger) -> JoinHandle<()> {
  PENDING_RUNS.fetch_add(1, Ordering::SeqCst);
  std::thread::spawn(move || {
    // Decrements on every exit, including a panic in the run.
    let _pending = PendingRun;
    if let Err(e) = run_if_needed(&db_path, trigger) {
      log::warn!("App setup script failed: {}", e);
    }
  })
}

struct PendingRun;

impl Drop for PendingRun {
  fn drop(&mut self) {
    PENDING_RUNS.fetch_sub(1, Ordering::SeqCst);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  /// Creates the app DB in `dir` and returns its path.
  fn test_db(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("treq.db");
    open(&path).init().unwrap();
    path
  }

  fn open(db_path: &Path) -> Database {
    Database::new(db_path.to_path_buf()).unwrap()
  }

  /// A script that appends a line to `runs.log` in `dir`, so tests can count runs.
  fn counting_script(dir: &TempDir, marker: &str) -> String {
    format!(
      "echo {} >> '{}'",
      marker,
      dir.path().join("runs.log").display()
    )
  }

  fn run_count(dir: &TempDir) -> usize {
    std::fs::read_to_string(dir.path().join("runs.log"))
      .map(|s| s.lines().count())
      .unwrap_or(0)
  }

  /// A log that only goes to the app log.
  fn no_store() -> RunLog {
    RunLog {
      writer: None,
      run_id: 0,
    }
  }

  #[test]
  fn manual_run_runs_saved_script_and_records_result() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), false).unwrap();

    assert!(run_if_needed(&db, Trigger::Manual).unwrap());

    assert_eq!(run_count(&dir), 1);
    let status = get_status(&open(&db)).unwrap();
    assert!(status.last_run_at.is_some());
    assert_eq!(status.last_status.as_deref(), Some("passed"));
  }

  #[test]
  fn manual_run_reruns_unchanged_script() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), false).unwrap();
    run_if_needed(&db, Trigger::Manual).unwrap();

    assert!(run_if_needed(&db, Trigger::Manual).unwrap());
    assert_eq!(run_count(&dir), 2);
  }

  #[test]
  fn startup_skips_script_unless_always_run() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), false).unwrap();

    assert!(!run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(run_count(&dir), 0);
    assert_eq!(get_status(&open(&db)).unwrap().last_run_at, None);
  }

  #[test]
  fn always_run_runs_script_on_every_startup() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), true).unwrap();
    run_if_needed(&db, Trigger::Startup).unwrap();

    assert!(run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(run_count(&dir), 2);
  }

  #[test]
  fn skips_blank_script() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "  \n", true).unwrap();

    assert!(!run_if_needed(&db, Trigger::Manual).unwrap());
    assert_eq!(get_status(&open(&db)).unwrap().last_run_at, None);
  }

  #[test]
  fn records_failed_status() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "exit 3", false).unwrap();

    assert!(run_if_needed(&db, Trigger::Manual).unwrap());
    let status = get_status(&open(&db)).unwrap();
    assert_eq!(status.last_status.as_deref(), Some("failed"));
  }

  #[test]
  fn stores_each_runs_output_as_log_records() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "echo first; echo 'error: broke' 1>&2", false).unwrap();
    run_if_needed(&db, Trigger::Manual).unwrap();
    save(&open(&db), "echo second", false).unwrap();
    run_if_needed(&db, Trigger::Manual).unwrap();

    // Output is read on detached threads, so it can land after the run returns.
    let deadline = Instant::now() + Duration::from_secs(10);
    let records = loop {
      let records = query_logs(&db, &LogQuery::default()).unwrap();
      if records.len() == 3 || Instant::now() > deadline {
        break records;
      }
      std::thread::sleep(Duration::from_millis(50));
    };

    let lines: Vec<_> = records
      .iter()
      .map(|r| {
        (
          r.run_id,
          r.stream.as_str(),
          r.severity_text.as_str(),
          r.body.as_str(),
        )
      })
      .collect();
    assert_eq!(lines[0], (2, "stdout", "INFO", "second"));
    assert!(lines.contains(&(1, "stdout", "INFO", "first")));
    assert!(lines.contains(&(1, "stderr", "ERROR", "error: broke")));
    assert_eq!(lines.len(), 3);
  }

  #[test]
  fn waits_for_another_instance_holding_the_run_claim() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), false).unwrap();
    // Stands in for another Treq process that is mid-run on the same DB.
    let claim = FileLock::lock(claim_path(&db)).unwrap();

    let waiting = std::thread::spawn({
      let db = db.clone();
      move || run_if_needed(&db, Trigger::Manual)
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(run_count(&dir), 0);

    drop(claim);
    assert!(waiting.join().unwrap().unwrap());
    assert_eq!(run_count(&dir), 1);
  }

  #[test]
  fn running_tracks_spawned_runs_only() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "echo hi", false).unwrap();
    assert!(!get_status(&open(&db)).unwrap().running);

    // The run cannot open its DB and records nothing, yet running still ends.
    spawn_run(dir.path().join("missing/treq.db"), Trigger::Manual)
      .join()
      .unwrap();
    assert!(!get_status(&open(&db)).unwrap().running);
  }

  #[test]
  fn execute_kills_a_script_that_outlives_the_timeout() {
    let started = Instant::now();
    assert!(!execute(
      "sleep 30",
      Duration::from_millis(200),
      &no_store()
    ));
    assert!(started.elapsed() < Duration::from_secs(10));
  }

  #[test]
  fn execute_does_not_wait_for_background_jobs() {
    // The job inherits the script's stdout and holds it open after `sh` exits.
    let started = Instant::now();
    assert!(execute("sleep 30 &", Duration::from_secs(60), &no_store()));
    assert!(started.elapsed() < Duration::from_secs(10));
  }

  #[test]
  fn get_status_returns_saved_settings() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "echo hi", true).unwrap();

    let status = get_status(&open(&db)).unwrap();
    assert_eq!(status.script, "echo hi");
    assert!(status.always_run);
    assert_eq!(status.last_run_at, None);
  }
}
