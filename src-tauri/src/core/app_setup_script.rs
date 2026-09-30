//! Application setup script: a shell script the user sets in Application
//! settings. It runs once per distinct script, tracked by the SHA-256 of the
//! script stored in the app DB, so editing the script makes it run again. With
//! `always_run` on, it also runs on every app startup.

use std::process::{Command, Stdio};
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::Database;
use crate::lock_ext::LockExt;

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

/// Held for the whole run. Serializes runs, so a save made mid-run is
/// re-evaluated against the hash the run records.
static RUN_LOCK: Mutex<()> = Mutex::new(());

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
    // A saved script whose hash has not been recorded yet is about to run.
    running: RUN_LOCK.try_lock().is_err() || is_changed(&script, last_hash.as_deref()),
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
/// exited successfully; output goes to the app log.
fn execute(script: &str) -> bool {
  let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
  let output = Command::new("sh")
    .args(["-c", script])
    .current_dir(&home)
    .env("PATH", crate::binary_paths::get_extended_path())
    .stdin(Stdio::null())
    .output();
  match output {
    Ok(output) => {
      log::info!(
        "App setup script exited with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
      );
      output.status.success()
    }
    Err(e) => {
      log::warn!("Failed to start app setup script: {}", e);
      false
    }
  }
}

/// Runs the script when `trigger` calls for it. Returns whether it ran.
pub fn run_if_needed(db: &Mutex<Database>, trigger: Trigger) -> Result<bool, String> {
  let _run = RUN_LOCK.lock_or_recover();
  let Some(script) = script_to_run(&db.lock_or_recover(), trigger)? else {
    return Ok(false);
  };
  let status = if execute(&script) { "passed" } else { "failed" };
  let db = db.lock_or_recover();
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
  std::thread::spawn(move || {
    let result = Database::new(db_path.into())
      .map_err(|e| e.to_string())
      .and_then(|db| run_if_needed(&Mutex::new(db), trigger));
    if let Err(e) = result {
      log::warn!("App setup script failed: {}", e);
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn test_db(dir: &TempDir) -> Mutex<Database> {
    let db = Database::new(dir.path().join("treq.db")).unwrap();
    db.init().unwrap();
    Mutex::new(db)
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
    save(&db.lock().unwrap(), &script, false).unwrap();

    assert!(run_if_needed(&db, Trigger::Startup).unwrap());

    assert_eq!(run_count(&dir), 1);
    let status = get_status(&db.lock().unwrap()).unwrap();
    assert_eq!(status.last_hash, Some(script_hash(&script)));
    assert!(status.last_run_at.is_some());
    assert_eq!(status.last_status.as_deref(), Some("passed"));
  }

  #[test]
  fn skips_startup_run_when_script_unchanged() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&db.lock().unwrap(), &counting_script(&dir, "a"), false).unwrap();
    run_if_needed(&db, Trigger::Startup).unwrap();

    assert!(!run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(run_count(&dir), 1);
  }

  #[test]
  fn reruns_when_script_changes() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&db.lock().unwrap(), &counting_script(&dir, "a"), false).unwrap();
    run_if_needed(&db, Trigger::SettingsChanged).unwrap();

    save(&db.lock().unwrap(), &counting_script(&dir, "b"), false).unwrap();
    assert!(run_if_needed(&db, Trigger::SettingsChanged).unwrap());
    assert_eq!(run_count(&dir), 2);
  }

  #[test]
  fn always_run_reruns_unchanged_script_on_startup() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&db.lock().unwrap(), &counting_script(&dir, "a"), true).unwrap();
    run_if_needed(&db, Trigger::Startup).unwrap();

    assert!(run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(run_count(&dir), 2);
  }

  #[test]
  fn always_run_does_not_rerun_unchanged_script_on_settings_save() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&db.lock().unwrap(), &counting_script(&dir, "a"), true).unwrap();
    run_if_needed(&db, Trigger::Startup).unwrap();

    assert!(!run_if_needed(&db, Trigger::SettingsChanged).unwrap());
    assert_eq!(run_count(&dir), 1);
  }

  #[test]
  fn skips_blank_script() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&db.lock().unwrap(), "  \n", true).unwrap();

    assert!(!run_if_needed(&db, Trigger::Startup).unwrap());
    assert_eq!(get_status(&db.lock().unwrap()).unwrap().last_run_at, None);
  }

  #[test]
  fn records_failed_status_and_does_not_retry_same_script() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&db.lock().unwrap(), "exit 3", false).unwrap();

    assert!(run_if_needed(&db, Trigger::Startup).unwrap());
    let status = get_status(&db.lock().unwrap()).unwrap();
    assert_eq!(status.last_status.as_deref(), Some("failed"));
    assert!(!run_if_needed(&db, Trigger::Startup).unwrap());
  }

  #[test]
  fn get_status_returns_saved_settings() {
    let dir = TempDir::new().unwrap();
    let db = test_db(&dir);
    save(&db.lock().unwrap(), "echo hi", true).unwrap();

    let status = get_status(&db.lock().unwrap()).unwrap();
    assert_eq!(status.script, "echo hi");
    assert!(status.always_run);
    assert_eq!(status.last_hash, None);
  }
}
