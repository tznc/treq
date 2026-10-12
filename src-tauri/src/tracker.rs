//! Shared plumbing for issue-tracker integrations: branch naming and the
//! label-driven auto-kickoff ledger and poller that Linear uses.

use crate::lock_ext::LockExt;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Lowercase, hyphen-separated slug of `title`, capped at `max_len` bytes on
/// a word boundary where possible. Returns an empty string for titles with no
/// ASCII alphanumerics.
pub fn branch_slug(title: &str, max_len: usize) -> String {
  let mut slug = String::new();
  let mut pending_dash = false;
  for ch in title.chars() {
    if ch.is_ascii_alphanumeric() {
      if pending_dash && !slug.is_empty() {
        slug.push('-');
      }
      pending_dash = false;
      slug.push(ch.to_ascii_lowercase());
    } else {
      pending_dash = true;
    }
  }
  if slug.len() <= max_len {
    return slug;
  }
  let cut = &slug[..max_len];
  match cut.rfind('-') {
    Some(idx) if idx > 0 => cut[..idx].to_string(),
    _ => cut.trim_end_matches('-').to_string(),
  }
}

/// Joins a branch prefix and title slug, dropping the slug when it is empty.
pub fn branch_name(prefix: &str, title: &str) -> String {
  let slug = branch_slug(title, 48);
  if slug.is_empty() {
    prefix.to_string()
  } else {
    format!("{prefix}-{slug}")
  }
}

/// Reads a non-empty per-repo setting, trimmed.
pub fn read_setting(
  db: &crate::db::Database,
  repo_path: &str,
  key: &str,
) -> Result<Option<String>, String> {
  let value = db
    .get_repo_setting(repo_path, key)
    .map_err(|e| format!("Failed to read {key}: {e}"))?;
  Ok(
    value
      .map(|v| v.trim().to_string())
      .filter(|v| !v.is_empty()),
  )
}

pub const MAX_KICKOFF_ATTEMPTS: u32 = 3;

/// Per-repo record of which labeled items were kicked off and which keep
/// failing. Failures retry on later polls up to `MAX_KICKOFF_ATTEMPTS`.
/// Removing and re-adding the label resets an item.
#[derive(Default)]
pub struct KickoffLedger {
  handled: HashSet<String>,
  failures: HashMap<String, u32>,
}

impl KickoffLedger {
  /// Loads the ledger stored as JSON under the two repo-setting keys.
  pub fn load(
    db: &crate::db::Database,
    repo_path: &str,
    handled_key: &str,
    failures_key: &str,
  ) -> Result<Self, String> {
    let read = |key: &str| {
      db.get_repo_setting(repo_path, key)
        .map_err(|e| format!("Failed to read {key}: {e}"))
    };
    Ok(Self {
      handled: read(handled_key)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default(),
      failures: read(failures_key)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default(),
    })
  }

  pub fn save(
    &self,
    db: &crate::db::Database,
    repo_path: &str,
    handled_key: &str,
    failures_key: &str,
  ) -> Result<(), String> {
    let write = |key: &str, json: Result<String, serde_json::Error>| {
      let json = json.map_err(|e| format!("Failed to serialize {key}: {e}"))?;
      db.set_repo_setting(repo_path, key, &json)
        .map_err(|e| format!("Failed to save {key}: {e}"))
    };
    write(handled_key, serde_json::to_string(&self.handled))?;
    write(failures_key, serde_json::to_string(&self.failures))
  }

  /// Returns the labeled items still owed an attempt. Entries for items no
  /// longer labeled are dropped, which keeps the stored sets bounded.
  pub fn due(&mut self, labeled_ids: &[String]) -> Vec<String> {
    let labeled: HashSet<&str> = labeled_ids.iter().map(String::as_str).collect();
    self.handled.retain(|id| labeled.contains(id.as_str()));
    self.failures.retain(|id, _| labeled.contains(id.as_str()));
    labeled_ids
      .iter()
      .filter(|id| {
        !self.handled.contains(*id)
          && self.failures.get(*id).copied().unwrap_or(0) < MAX_KICKOFF_ATTEMPTS
      })
      .cloned()
      .collect()
  }

  pub fn record_success(&mut self, id: &str) {
    self.failures.remove(id);
    self.handled.insert(id.to_string());
  }

  pub fn record_failure(&mut self, id: &str) -> u32 {
    let attempts = self.failures.entry(id.to_string()).or_insert(0);
    *attempts += 1;
    *attempts
  }
}

const KICKOFF_POLL_INTERVAL: Duration = Duration::from_secs(60);

struct KickoffPollerInner {
  name: &'static str,
  poll: fn(&str) -> Result<(), String>,
  watched: Mutex<HashSet<String>>,
  shutdown: AtomicBool,
  loop_started: AtomicBool,
  wake: (Mutex<()>, std::sync::Condvar),
}

/// Background thread that calls `poll(repo_path)` for every watched repo
/// once per `KICKOFF_POLL_INTERVAL`. Watching a repo wakes the loop so the
/// first poll happens immediately.
pub struct KickoffPoller {
  inner: Arc<KickoffPollerInner>,
}

impl KickoffPoller {
  pub fn new(name: &'static str, poll: fn(&str) -> Result<(), String>) -> Self {
    Self {
      inner: Arc::new(KickoffPollerInner {
        name,
        poll,
        watched: Mutex::new(HashSet::new()),
        shutdown: AtomicBool::new(false),
        loop_started: AtomicBool::new(false),
        wake: (Mutex::new(()), std::sync::Condvar::new()),
      }),
    }
  }

  fn ensure_started(&self) {
    if self
      .inner
      .loop_started
      .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
      .is_err()
    {
      return;
    }
    let inner = Arc::clone(&self.inner);
    thread::Builder::new()
      .name(format!("{}-kickoff-poller", inner.name))
      .spawn(move || kickoff_background_loop(inner))
      .expect("failed to spawn kickoff poller thread");
  }

  pub fn watch_repo(&self, repo_path: &str) {
    self
      .inner
      .watched
      .lock_or_recover()
      .insert(repo_path.to_string());
    self.ensure_started();
    self.inner.wake.1.notify_one();
  }
}

fn kickoff_background_loop(inner: Arc<KickoffPollerInner>) {
  use std::time::Instant;
  let mut last_poll: Option<Instant> = None;

  loop {
    if inner.shutdown.load(Ordering::SeqCst) {
      break;
    }

    let now = Instant::now();
    let poll_due = last_poll
      .map(|t| now.duration_since(t) >= KICKOFF_POLL_INTERVAL)
      .unwrap_or(true);

    if poll_due {
      let repos: Vec<String> = inner.watched.lock_or_recover().iter().cloned().collect();
      for repo_path in repos {
        if inner.shutdown.load(Ordering::SeqCst) {
          break;
        }
        if let Err(e) = (inner.poll)(&repo_path) {
          log::warn!("{}-kickoff: failed for {repo_path}: {e}", inner.name);
        }
      }
      last_poll = Some(Instant::now());
    }

    if inner.shutdown.load(Ordering::SeqCst) {
      break;
    }

    let wait = last_poll
      .map(|t| KICKOFF_POLL_INTERVAL.saturating_sub(now.duration_since(t)))
      .unwrap_or(Duration::ZERO)
      .max(Duration::from_millis(5));

    let (lock, cvar) = &inner.wake;
    let guard = lock.lock_or_recover();
    let _ = cvar
      .wait_timeout_while(guard, wait, |_| !inner.shutdown.load(Ordering::SeqCst))
      .unwrap();
  }
}

/// Opens the app database in `TREQ_APP_DATA_DIR` for poller threads, which
/// run outside Tauri's managed state.
pub fn open_app_db() -> Result<crate::db::Database, String> {
  let app_data_dir =
    crate::core::app_data_dir().ok_or_else(|| "TREQ_APP_DATA_DIR is not set".to_string())?;
  crate::db::Database::new(app_data_dir.join(crate::core::APP_DB_FILE_NAME))
    .map_err(|e| format!("Failed to open database: {e}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
  }

  #[test]
  fn branch_slug_collapses_punctuation() {
    assert_eq!(
      branch_slug("Fix: login  bug (iOS)!", 48),
      "fix-login-bug-ios"
    );
  }

  #[test]
  fn branch_slug_truncates_on_word_boundary() {
    assert_eq!(branch_slug("alpha beta gamma", 12), "alpha-beta");
  }

  #[test]
  fn branch_slug_drops_non_ascii_only_titles() {
    assert_eq!(branch_slug("修正する", 48), "");
    assert_eq!(branch_name("ENG-1", "修正する"), "ENG-1");
  }

  #[test]
  fn branch_name_joins_prefix_and_slug() {
    assert_eq!(
      branch_name("ENG-12", "Add login support"),
      "ENG-12-add-login-support"
    );
  }

  #[test]
  fn kickoff_ledger_skips_handled_items() {
    let mut ledger = KickoffLedger::default();
    ledger.record_success("a");
    assert_eq!(ledger.due(&ids(&["a", "b"])), ids(&["b"]));
  }

  #[test]
  fn kickoff_ledger_retries_failures_until_attempt_cap() {
    let mut ledger = KickoffLedger::default();
    for _ in 1..MAX_KICKOFF_ATTEMPTS {
      ledger.record_failure("a");
      assert_eq!(ledger.due(&ids(&["a"])), ids(&["a"]));
    }
    ledger.record_failure("a");
    assert!(ledger.due(&ids(&["a"])).is_empty());
  }

  #[test]
  fn kickoff_ledger_success_clears_failure_count() {
    let mut ledger = KickoffLedger::default();
    ledger.record_failure("a");
    ledger.record_success("a");
    assert!(!ledger.failures.contains_key("a"));
  }

  #[test]
  fn kickoff_ledger_forgets_items_that_lost_the_label() {
    let mut ledger = KickoffLedger::default();
    ledger.record_success("a");
    for _ in 0..MAX_KICKOFF_ATTEMPTS {
      ledger.record_failure("b");
    }
    assert!(ledger.due(&ids(&["c"])) == ids(&["c"]));
    assert!(ledger.handled.is_empty());
    assert!(ledger.failures.is_empty());
  }

  #[test]
  fn kickoff_ledger_round_trips_through_repo_settings() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = crate::db::Database::new(dir.path().join("treq.db")).unwrap();
    db.init().unwrap();
    let mut ledger = KickoffLedger::default();
    ledger.record_success("a");
    ledger.record_failure("b");
    ledger.save(&db, "/repo", "h", "f").unwrap();

    let mut loaded = KickoffLedger::load(&db, "/repo", "h", "f").unwrap();
    assert_eq!(loaded.due(&ids(&["a", "b"])), ids(&["b"]));
    assert_eq!(loaded.record_failure("b"), 2);
  }
}
