//! Application setup script: a shell script the user sets in Application
//! settings. It runs once per distinct script, tracked by the SHA-256 of the
//! script stored in the app DB, so editing the script makes it run again. With
//! `always_run` on, it also runs on every app startup.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::Utc;
use jj_lib::lock::FileLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::Database;

pub const SCRIPT_KEY: &str = "app_setup_script";
pub const ALWAYS_RUN_KEY: &str = "app_setup_script_always_run";
pub const LAST_HASH_KEY: &str = "app_setup_script_last_hash";
pub const LAST_RUN_AT_KEY: &str = "app_setup_script_last_run_at";
pub const LAST_STATUS_KEY: &str = "app_setup_script_last_status";

/// What asked for the run. `always_run` only applies at startup; a settings
/// save runs the script only when its hash changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
  Startup,
  SettingsChanged,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct AppSetupScriptStatus {
  pub script: String,
  pub always_run: bool,
  pub last_hash: Option<String>,
  pub last_run_at: Option<String>,
  /// "passed" | "failed"
  pub last_status: Option<String>,
  pub running: bool,
}

/// Spawned runs that have not finished. Counted from before the thread starts,
/// so a status read right after a save already sees the run.
static PENDING_RUNS: AtomicUsize = AtomicUsize::new(0);

/// A script still running after this long is killed and recorded as failed,
/// so a hung script cannot hold the run claim and block every later run.
const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub fn script_hash(script: &str) -> String {
  Sha256::digest(script.as_bytes())
    .iter()
    .map(|byte| format!("{:02x}", byte))
    .collect()
}

pub fn get_status(db: &Database) -> Result<AppSetupScriptStatus, String> {
  let get = |key: &str| db.get_setting(key).map_err(|e| e.to_string());
  let script = get(SCRIPT_KEY)?.unwrap_or_default();
  let last_hash = get(LAST_HASH_KEY)?;
  Ok(AppSetupScriptStatus {
    running: PENDING_RUNS.load(Ordering::SeqCst) > 0,
    always_run: get(ALWAYS_RUN_KEY)?.as_deref() == Some("true"),
    last_run_at: get(LAST_RUN_AT_KEY)?,
    last_status: get(LAST_STATUS_KEY)?,
    script,
    last_hash,
  })
}

fn is_changed(script: &str, last_hash: Option<&str>) -> bool {
  !script.trim().is_empty() && last_hash != Some(script_hash(script).as_str())
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
  let changed = is_changed(&status.script, status.last_hash.as_deref());
  let always = trigger == Trigger::Startup && status.always_run;
  Ok((changed || always).then_some(status.script))
}

/// Runs the script with `sh -c` from the home directory. Returns whether it
/// exited successfully within `timeout`; output goes to the app log.
fn execute(script: &str, timeout: Duration) -> bool {
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
      log::warn!("Failed to start app setup script: {}", e);
      return false;
    }
  };
  // Not joined: a background job the script starts can hold the pipes open.
  if let Some(stdout) = child.stdout.take() {
    log_lines("stdout", stdout);
  }
  if let Some(stderr) = child.stderr.take() {
    log_lines("stderr", stderr);
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
        log::warn!("App setup script timed out after {:?}; killing it", timeout);
        kill_process_group(&mut child);
        let _ = child.wait();
        return false;
      }
      Err(e) => {
        log::warn!("Failed waiting for app setup script: {}", e);
        return false;
      }
    }
  }
}

fn log_lines(stream: &'static str, pipe: impl Read + Send + 'static) {
  std::thread::spawn(move || {
    for line in BufReader::new(pipe).lines().map_while(Result::ok) {
      log::info!("App setup script {}: {}", stream, line);
    }
  });
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
/// The claim is held from the hash check until the run is recorded, so a
/// script runs once even when several instances start together, and a save
/// made mid-run is re-evaluated against the hash the run records.
pub fn run_if_needed(db_path: &Path, trigger: Trigger) -> Result<bool, String> {
  let _claim = FileLock::lock(claim_path(db_path)).map_err(|e| e.to_string())?;
  let db = Database::new(db_path.to_path_buf()).map_err(|e| e.to_string())?;
  let Some(script) = script_to_run(&db, trigger)? else {
    return Ok(false);
  };
  let status = if execute(&script, RUN_TIMEOUT) {
    "passed"
  } else {
    "failed"
  };
  db.set_setting(LAST_HASH_KEY, &script_hash(&script))
    .and_then(|_| db.set_setting(LAST_RUN_AT_KEY, &Utc::now().to_rfc3339()))
    .and_then(|_| db.set_setting(LAST_STATUS_KEY, status))
    .map_err(|e| e.to_string())?;
  Ok(true)
}

/// Runs `run_if_needed` on a background thread with its own connection to the
/// app DB, so a long script never blocks startup or the settings save.
pub fn spawn_run_if_needed(trigger: Trigger) {
  let Ok(db_path) = std::env::var("TREQ_APP_DB_PATH") else {
    log::warn!("TREQ_APP_DB_PATH unset; skipping app setup script");
    return;
  };
  spawn_run(db_path.into(), trigger);
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

  #[test]
  fn runs_script_on_first_startup_and_records_hash_and_timestamp() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    let script = counting_script(&dir, "a");
    save(&open(&db), &script, false).unwrap();

    assert!(run_if_needed(&db, Trigger::Startup).unwrap());

    assert_eq!(run_count(&dir), 1);
    let status = get_status(&open(&db)).unwrap();
    assert_eq!(status.last_hash, Some(script_hash(&script)));
    assert!(status.last_run_at.is_some());
    assert_eq!(status.last_status.as_deref(), Some("passed"));
  }

  #[test]
  fn skips_startup_run_when_script_unchanged() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), false).unwrap();
    run_if_needed(&db, Trigger::Startup).unwrap();

    assert!(!run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(run_count(&dir), 1);
  }

  #[test]
  fn reruns_when_script_changes() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), false).unwrap();
    run_if_needed(&db, Trigger::SettingsChanged).unwrap();

    save(&open(&db), &counting_script(&dir, "b"), false).unwrap();
    assert!(run_if_needed(&db, Trigger::SettingsChanged).unwrap());
    assert_eq!(run_count(&dir), 2);
  }

  #[test]
  fn always_run_reruns_unchanged_script_on_startup() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), true).unwrap();
    run_if_needed(&db, Trigger::Startup).unwrap();

    assert!(run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(run_count(&dir), 2);
  }

  #[test]
  fn always_run_does_not_rerun_unchanged_script_on_settings_save() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), &counting_script(&dir, "a"), true).unwrap();
    run_if_needed(&db, Trigger::Startup).unwrap();

    assert!(!run_if_needed(&db, Trigger::SettingsChanged).unwrap());
    assert_eq!(run_count(&dir), 1);
  }

  #[test]
  fn skips_blank_script() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "  \n", true).unwrap();

    assert!(!run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(get_status(&open(&db)).unwrap().last_run_at, None);
  }

  #[test]
  fn records_failed_status_and_does_not_retry_same_script() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "exit 3", false).unwrap();

    assert!(run_if_needed(&db, Trigger::Startup).unwrap());
    let status = get_status(&open(&db)).unwrap();
    assert_eq!(status.last_status.as_deref(), Some("failed"));
    assert!(!run_if_needed(&db, Trigger::Startup).unwrap());
  }

  #[test]
  fn waits_for_another_instance_holding_the_run_claim() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    let script = counting_script(&dir, "a");
    save(&open(&db), &script, false).unwrap();
    // Stands in for another Treq process that is mid-run on the same DB.
    let claim = FileLock::lock(claim_path(&db)).unwrap();

    let waiting = std::thread::spawn({
      let db = db.clone();
      move || run_if_needed(&db, Trigger::Startup)
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(run_count(&dir), 0);

    // The other process records its run, then releases the claim.
    open(&db)
      .set_setting(LAST_HASH_KEY, &script_hash(&script))
      .unwrap();
    drop(claim);
    assert!(!waiting.join().unwrap().unwrap());
    assert_eq!(run_count(&dir), 0);
  }

  #[test]
  fn running_tracks_spawned_runs_only() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&open(&db), "echo hi", false).unwrap();
    // Not running until spawned: a skipped spawn would never clear the flag.
    assert!(!get_status(&open(&db)).unwrap().running);

    // The run cannot open its DB and records nothing, yet running still ends.
    spawn_run(dir.path().join("missing/treq.db"), Trigger::Startup)
      .join()
      .unwrap();
    assert!(!get_status(&open(&db)).unwrap().running);
  }

  #[test]
  fn execute_kills_a_script_that_outlives_the_timeout() {
    let started = Instant::now();
    assert!(!execute("sleep 30", Duration::from_millis(200)));
    assert!(started.elapsed() < Duration::from_secs(10));
  }

  #[test]
  fn execute_does_not_wait_for_background_jobs() {
    // The job inherits the script's stdout and holds it open after `sh` exits.
    let started = Instant::now();
    assert!(execute("sleep 30 &", Duration::from_secs(60)));
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
    assert_eq!(status.last_hash, None);
  }
}
