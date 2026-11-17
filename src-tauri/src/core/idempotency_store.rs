// ---------------------------------------------------------------------------
// Durable remote-mutation idempotency store
// ---------------------------------------------------------------------------
//
// Replaces the process-local `OnceLock<HashMap>` idempotency cache that used
// to live in `core::remote`. Remote SSH starts a fresh `treq` CLI process per
// exec channel, so a process-local cache cannot deduplicate a retry, or a
// concurrent invocation, that lands in a second process. This store persists
// idempotency records in the same repo-local SQLite database convention used
// by `local_db.rs` (`<repo>/.treq/local.db`), so a claim, its lifecycle
// state, and its result survive process exit and application restart.
//
// What is persisted: operation kind, idempotency key, a SHA-256 digest of
// the canonical request JSON (never the request body itself), lifecycle
// state, a redacted copy of the successful result, and timestamps. Request
// bodies can carry source contents, patch bodies, prompts, or credentials;
// none of that is ever written here, only its digest. Successful results are
// redacted before storage in case a result itself echoes sensitive input
// (e.g. `AgentStart`'s result includes the prompt it was given).

use crate::result_ext::ResultExt;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a `pending` claim is treated as still in-flight (worth polling)
/// rather than stale (worth verifying against observable VM state). A
/// well-behaved local mutation completes in well under this window; a
/// process that died mid-mutation (lost SSH connection, crashed CLI) leaves
/// its claim pending forever otherwise.
pub const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(20);

/// How long a completed record is retained before cleanup may remove it.
pub const DEFAULT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// Interval between polls while waiting on a fresh (non-stale) concurrent
/// claim held by another invocation.
const POLL_INTERVAL: Duration = Duration::from_millis(15);

/// Recognized lifecycle states, stored as text so the schema stays legible
/// in `sqlite3 .treq/local.db` during debugging.
const STATE_PENDING: &str = "pending";
const STATE_COMPLETED: &str = "completed";

/// Result of attempting to claim (or observe) one idempotency record.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimOutcome {
  /// No record existed, or a stale pending record was successfully
  /// reclaimed after verification: the caller should execute the mutation
  /// now, under the same idempotency key.
  Proceed,
  /// A completed record exists for this operation/key with a matching
  /// request fingerprint: the caller must not re-execute, and should return
  /// this value instead (a "replay").
  Replay(Value),
  /// A record exists for this operation/key but its request fingerprint
  /// does not match the current request: the key was reused for a
  /// different operation or request body.
  Conflict,
  /// A pending claim exists, its fingerprint matches, and it is still
  /// within the "fresh" window: another invocation is very likely executing
  /// the same mutation concurrently right now. The caller should wait
  /// briefly and re-check.
  InProgress,
  /// A pending claim exists, its fingerprint matches, but it has exceeded
  /// the staleness window: the claimant that made it is presumed gone
  /// (crashed, lost connection) without ever completing or failing it. The
  /// caller must run the mutation's observable-state verification recipe
  /// before doing anything else.
  Stale,
}

/// Outcome of resolving a `Stale` claim via the mutation's existing
/// observable-state verification recipe (the same recipe
/// `core::remote::verification_for` builds for post-reconnect retries).
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryDecision {
  /// Verification showed the mutation's effect is already observable in VM
  /// state. `observed` is what the verification read returned, used as the
  /// replay value since no original result was ever recorded.
  AlreadyApplied { observed: Value },
  /// Verification showed the mutation's effect is not present: it is safe
  /// to execute now, reusing the same idempotency key.
  VerifiedNotApplied,
  /// Verification could not determine the outcome (no recipe, or the
  /// verification read itself failed): the caller must not execute, and
  /// must surface this ambiguity rather than guess.
  Ambiguous,
}

pub struct IdempotencyStore {
  db_path: PathBuf,
}

/// The idempotency key alone is the record's identity — not
/// `operation:key` — so that reusing the same key for a *different*
/// operation lands on the same row and is caught by the operation mismatch
/// check in `claim_locked`, rather than silently creating an independent
/// record per operation.
fn cache_key(key: &str) -> String {
  key.to_string()
}

/// SHA-256 digest (hex) of the canonical JSON form of `request`. A digest,
/// never the request itself, is what gets persisted — this is how the store
/// can detect "same key, different request" without ever writing source
/// contents, patch bodies, prompts, or credentials to disk.
pub fn fingerprint(request: &Value) -> String {
  let canonical = serde_json::to_string(request).unwrap_or_default();
  let mut hasher = Sha256::new();
  hasher.update(canonical.as_bytes());
  hasher
    .finalize()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>()
}

/// Strips keys that are known to carry sensitive or bulky payloads (prompt
/// text, patch bodies, credentials, terminal output) from a value before it
/// is persisted as a completed result. Applied recursively since a result
/// may nest the sensitive field inside a record (e.g. `AgentStart`'s
/// `AgentRecord.prompt`).
pub fn redact_for_storage(value: &Value) -> Value {
  const SENSITIVE_KEYS: &[&str] = &[
    "prompt",
    "patch_base64",
    "patch",
    "content",
    "output",
    "stdout",
    "stderr",
    "log",
    "logs",
    "password",
    "token",
    "secret",
    "credential",
    "credentials",
  ];
  match value {
    Value::Object(map) => {
      let mut out = serde_json::Map::with_capacity(map.len());
      for (k, v) in map {
        if SENSITIVE_KEYS.contains(&k.as_str()) {
          out.insert(k.clone(), Value::String("<redacted>".to_string()));
        } else {
          out.insert(k.clone(), redact_for_storage(v));
        }
      }
      Value::Object(out)
    }
    Value::Array(items) => Value::Array(items.iter().map(redact_for_storage).collect()),
    other => other.clone(),
  }
}

impl IdempotencyStore {
  /// Opens (creating if needed) the durable idempotency store for `repo`,
  /// under `<repo>/.treq/local.db` — the same repo-local, `.treq`-scoped
  /// database convention `local_db.rs` uses for workspaces and sessions.
  pub fn open(repo: &str) -> Result<Self, String> {
    Self::open_at(&crate::local_db::get_local_db_path(repo))
  }

  /// Opens the store at an explicit database path. Used directly by tests
  /// (via `TempDir`) so two `IdempotencyStore` instances can share one file
  /// the way two fresh `treq` CLI processes would.
  pub fn open_at(db_path: &Path) -> Result<Self, String> {
    if let Some(parent) = db_path.parent() {
      std::fs::create_dir_all(parent)
        .map_err(|e| format!("filesystem_error: Failed to create .treq directory: {e}"))?;
    }
    let store = Self {
      db_path: db_path.to_path_buf(),
    };
    store.with_connection(|conn| store.init_schema(conn))?;
    Ok(store)
  }

  fn connection(&self) -> Result<Connection, String> {
    let conn = Connection::open(&self.db_path)
      .map_err(|e| format!("filesystem_error: Failed to open idempotency store: {e}"))?;
    // A cross-process claim race is exactly the case this store exists to
    // handle safely: two `treq` CLI processes (two exec channels) opening
    // this file at once must not fail outright on SQLITE_BUSY. A bounded
    // busy timeout lets SQLite's own file locking serialize the competing
    // writers instead.
    conn
      .busy_timeout(Duration::from_secs(5))
      .map_err(|e| format!("filesystem_error: Failed to set busy timeout: {e}"))?;
    Ok(conn)
  }

  fn with_connection<T>(
    &self,
    f: impl FnOnce(&Connection) -> Result<T, String>,
  ) -> Result<T, String> {
    let conn = self.connection()?;
    f(&conn)
  }

  fn init_schema(&self, conn: &Connection) -> Result<(), String> {
    conn
      .execute(
        "CREATE TABLE IF NOT EXISTS idempotency_records (
            cache_key TEXT PRIMARY KEY,
            operation TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            state TEXT NOT NULL,
            result_json TEXT,
            claimed_at TEXT NOT NULL,
            completed_at TEXT
        )",
        [],
      )
      .map_err(|e| format!("filesystem_error: Failed to create idempotency_records table: {e}"))?;
    conn
      .execute(
        "CREATE INDEX IF NOT EXISTS idx_idempotency_state ON idempotency_records(state)",
        [],
      )
      .map_err(|e| format!("filesystem_error: Failed to create idempotency state index: {e}"))?;
    Ok(())
  }

  /// Atomically claims `operation`/`key` for `request_fingerprint`, or
  /// reports why it cannot be claimed right now. Uses `BEGIN IMMEDIATE` so
  /// the read-then-write decision is atomic even across two OS processes
  /// racing on the same SQLite file — the second writer blocks (up to the
  /// busy timeout) rather than interleaving with the first.
  fn try_claim(
    &self,
    operation: &str,
    key: &str,
    request_fingerprint: &str,
    stale_after: Duration,
  ) -> Result<ClaimOutcome, String> {
    self.with_connection(|conn| {
      conn
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| format!("filesystem_error: {e}"))?;
      let result = self.claim_locked(conn, operation, key, request_fingerprint, stale_after);
      match &result {
        Ok(_) => conn
          .execute_batch("COMMIT")
          .map_err(|e| format!("filesystem_error: {e}"))?,
        Err(_) => {
          conn.execute_batch("ROLLBACK").best_effort();
        }
      }
      result
    })
  }

  fn claim_locked(
    &self,
    conn: &Connection,
    operation: &str,
    key: &str,
    request_fingerprint: &str,
    stale_after: Duration,
  ) -> Result<ClaimOutcome, String> {
    let cache_key = cache_key(key);
    let existing: Option<(String, String, String, Option<String>, String)> = conn
      .query_row(
        "SELECT fingerprint, state, claimed_at, result_json, operation FROM idempotency_records WHERE cache_key = ?1",
        [&cache_key],
        |row| {
          Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
          ))
        },
      )
      .optional()
      .map_err(|e| format!("filesystem_error: {e}"))?;

    let now = chrono::Utc::now();

    let Some((stored_fingerprint, state, claimed_at, result_json, stored_operation)) = existing
    else {
      conn
        .execute(
          "INSERT INTO idempotency_records (cache_key, operation, idempotency_key, fingerprint, state, claimed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
          rusqlite::params![
            cache_key,
            operation,
            key,
            request_fingerprint,
            STATE_PENDING,
            now.to_rfc3339(),
          ],
        )
        .map_err(|e| format!("filesystem_error: {e}"))?;
      return Ok(ClaimOutcome::Proceed);
    };

    if stored_operation != operation || stored_fingerprint != request_fingerprint {
      return Ok(ClaimOutcome::Conflict);
    }

    if state == STATE_COMPLETED {
      let value: Value = result_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("filesystem_error: corrupt idempotency record: {e}"))?
        .unwrap_or(Value::Null);
      return Ok(ClaimOutcome::Replay(value));
    }

    // Pending: fresh (likely still executing elsewhere) or stale (its
    // claimant is presumed gone).
    let claimed_at = chrono::DateTime::parse_from_rfc3339(&claimed_at)
      .map(|dt| dt.with_timezone(&chrono::Utc))
      .unwrap_or(now);
    let age = now.signed_duration_since(claimed_at);
    if age >= chrono::Duration::from_std(stale_after).unwrap_or(chrono::Duration::MAX) {
      Ok(ClaimOutcome::Stale)
    } else {
      Ok(ClaimOutcome::InProgress)
    }
  }

  /// Reclaims a stale pending record after `VerifiedNotApplied`, keeping the
  /// same cache key and refreshing `claimed_at` so the caller (now owning
  /// the claim) can execute the mutation. Optimistic on `claimed_at` so a
  /// third party reclaiming or completing the same record concurrently is
  /// detected rather than silently overwritten.
  fn reclaim_stale(
    &self,
    _operation: &str,
    key: &str,
    previous_claimed_at: &str,
  ) -> Result<bool, String> {
    self.with_connection(|conn| {
      let cache_key = cache_key(key);
      let updated = conn
        .execute(
          "UPDATE idempotency_records SET claimed_at = ?1 WHERE cache_key = ?2 AND state = ?3 AND claimed_at = ?4",
          rusqlite::params![
            chrono::Utc::now().to_rfc3339(),
            cache_key,
            STATE_PENDING,
            previous_claimed_at,
          ],
        )
        .map_err(|e| format!("filesystem_error: {e}"))?;
      Ok(updated == 1)
    })
  }

  fn claimed_at(&self, _operation: &str, key: &str) -> Result<Option<String>, String> {
    self.with_connection(|conn| {
      conn
        .query_row(
          "SELECT claimed_at FROM idempotency_records WHERE cache_key = ?1 AND state = ?2",
          rusqlite::params![cache_key(key), STATE_PENDING],
          |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("filesystem_error: {e}"))
    })
  }

  /// Marks `operation`/`key` completed with `result` (already redacted by
  /// the caller). Called after the mutation this claim guards actually ran.
  pub fn complete(&self, _operation: &str, key: &str, result: &Value) -> Result<(), String> {
    self.with_connection(|conn| {
      conn
        .execute(
          "UPDATE idempotency_records SET state = ?1, result_json = ?2, completed_at = ?3 WHERE cache_key = ?4",
          rusqlite::params![
            STATE_COMPLETED,
            serde_json::to_string(result).map_err(|e| e.to_string())?,
            chrono::Utc::now().to_rfc3339(),
            cache_key(key),
          ],
        )
        .map_err(|e| format!("filesystem_error: {e}"))?;
      Ok(())
    })
  }

  /// Marks a verification-confirmed "already applied" outcome as completed,
  /// so subsequent replays return `observed` without re-running verification.
  pub fn complete_from_verification(
    &self,
    operation: &str,
    key: &str,
    observed: &Value,
  ) -> Result<(), String> {
    self.complete(operation, key, observed)
  }

  /// Abandons a pending claim (the mutation it guarded failed outright, with
  /// a structured error rather than a network failure) so a later retry with
  /// the same key is free to execute again rather than being stuck pending
  /// forever.
  pub fn abandon(&self, _operation: &str, key: &str) -> Result<(), String> {
    self.with_connection(|conn| {
      conn
        .execute(
          "DELETE FROM idempotency_records WHERE cache_key = ?1 AND state = ?2",
          rusqlite::params![cache_key(key), STATE_PENDING],
        )
        .map_err(|e| format!("filesystem_error: {e}"))?;
      Ok(())
    })
  }

  /// Deletes completed records older than `retention`. Bounded retention
  /// keeps the store from growing without limit across a long-lived VM.
  pub fn cleanup(&self, retention: Duration) -> Result<usize, String> {
    self.with_connection(|conn| {
      let cutoff = (chrono::Utc::now()
        - chrono::Duration::from_std(retention).unwrap_or(chrono::Duration::MAX))
      .to_rfc3339();
      let removed = conn
        .execute(
          "DELETE FROM idempotency_records WHERE state = ?1 AND completed_at IS NOT NULL AND completed_at < ?2",
          rusqlite::params![STATE_COMPLETED, cutoff],
        )
        .map_err(|e| format!("filesystem_error: {e}"))?;
      Ok(removed)
    })
  }

  /// Claims `operation`/`key` for `request_fingerprint`, blocking (via short
  /// polling, bounded by `stale_after`) while a fresh concurrent claim is in
  /// progress, and running `recover` — the mutation's observable-state
  /// verification recipe — exactly once if the claim turns out to be stale.
  /// Returns `Ok(Some(value))` for a replay (either a stored completed
  /// result or a verification-confirmed "already applied" outcome),
  /// `Ok(None)` when the caller should now execute the mutation, and
  /// `Err` for a conflict or an unresolved ambiguity.
  #[expect(
    clippy::expect_used,
    reason = "`recover` is taken once and a resolved claim is never Stale again"
  )]
  pub fn claim_or_replay(
    &self,
    operation: &str,
    key: &str,
    request_fingerprint: &str,
    stale_after: Duration,
    recover: impl FnOnce() -> Result<RecoveryDecision, String>,
  ) -> Result<Option<Value>, String> {
    let mut recover = Some(recover);
    loop {
      match self.try_claim(operation, key, request_fingerprint, stale_after)? {
        ClaimOutcome::Proceed => return Ok(None),
        ClaimOutcome::Replay(value) => return Ok(Some(value)),
        ClaimOutcome::Conflict => {
          return Err(
            "idempotency_conflict: Idempotency key was already used for a different operation or request".to_string(),
          )
        }
        ClaimOutcome::InProgress => {
          std::thread::sleep(POLL_INTERVAL);
          continue;
        }
        ClaimOutcome::Stale => {
          let recover = recover
            .take()
            .expect("claim_or_replay only reaches Stale once per call, since a resolved claim is never Stale again");
          match recover()? {
            RecoveryDecision::AlreadyApplied { observed } => {
              self.complete_from_verification(operation, key, &observed)?;
              return Ok(Some(observed));
            }
            RecoveryDecision::VerifiedNotApplied => {
              let Some(previous_claimed_at) = self.claimed_at(operation, key)? else {
                // Someone else already resolved it between our Stale read and
                // now; loop back and re-observe its current state.
                continue;
              };
              if self.reclaim_stale(operation, key, &previous_claimed_at)? {
                return Ok(None);
              }
              // Lost the race to reclaim; re-observe.
              continue;
            }
            RecoveryDecision::Ambiguous => {
              return Err(
                "idempotency_ambiguous: Could not determine whether the mutation already applied; not executed".to_string(),
              )
            }
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn temp_db_path(dir: &TempDir) -> PathBuf {
    dir.path().join(".treq").join("local.db")
  }

  #[test]
  fn replays_completed_result_across_two_fresh_store_instances() {
    let dir = TempDir::new().unwrap();
    let db_path = temp_db_path(&dir);

    let first = IdempotencyStore::open_at(&db_path).unwrap();
    let result = first
      .claim_or_replay(
        "workspace.create",
        "key-1",
        "fp-1",
        DEFAULT_STALE_AFTER,
        || panic!("should not need recovery for a fresh claim"),
      )
      .unwrap();
    assert_eq!(result, None);
    first
      .complete("workspace.create", "key-1", &serde_json::json!({"id": 42}))
      .unwrap();
    drop(first);

    // Simulates a fresh `treq` CLI process (a new SSH exec channel) opening
    // the same repo-local database.
    let second = IdempotencyStore::open_at(&db_path).unwrap();
    let replay = second
      .claim_or_replay(
        "workspace.create",
        "key-1",
        "fp-1",
        DEFAULT_STALE_AFTER,
        || panic!("should not need recovery for a completed claim"),
      )
      .unwrap();
    assert_eq!(replay, Some(serde_json::json!({"id": 42})));
  }

  #[test]
  fn concurrent_identical_requests_execute_at_most_once() {
    let dir = TempDir::new().unwrap();
    let db_path = temp_db_path(&dir);
    let store = std::sync::Arc::new(IdempotencyStore::open_at(&db_path).unwrap());
    let execution_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut handles = vec![];
    for _ in 0..5 {
      let store = store.clone();
      let execution_count = execution_count.clone();
      handles.push(std::thread::spawn(move || {
        let claim = store
          .claim_or_replay("git.push", "key-1", "fp-1", DEFAULT_STALE_AFTER, || {
            panic!("should not need recovery while the claim is fresh")
          })
          .unwrap();
        match claim {
          None => {
            execution_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Simulate real work so the other threads observe `InProgress`.
            std::thread::sleep(Duration::from_millis(60));
            store
              .complete("git.push", "key-1", &serde_json::json!({"pushed": true}))
              .unwrap();
            serde_json::json!({"pushed": true})
          }
          Some(value) => value,
        }
      }));
    }

    let results: Vec<Value> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(execution_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    for result in results {
      assert_eq!(result, serde_json::json!({"pushed": true}));
    }
  }

  #[test]
  fn mismatched_request_reusing_key_is_a_conflict() {
    let dir = TempDir::new().unwrap();
    let store = IdempotencyStore::open_at(&temp_db_path(&dir)).unwrap();
    store
      .claim_or_replay(
        "commit.create",
        "key-1",
        "fp-a",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed"),
      )
      .unwrap();
    store
      .complete(
        "commit.create",
        "key-1",
        &serde_json::json!({"commit": "abc"}),
      )
      .unwrap();

    let error = store
      .claim_or_replay(
        "commit.create",
        "key-1",
        "fp-b",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed for a conflict"),
      )
      .unwrap_err();
    assert!(error.starts_with("idempotency_conflict:"), "{error}");
  }

  #[test]
  fn different_operation_reusing_key_is_a_conflict() {
    let dir = TempDir::new().unwrap();
    let store = IdempotencyStore::open_at(&temp_db_path(&dir)).unwrap();
    store
      .claim_or_replay(
        "commit.create",
        "key-1",
        "fp-a",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed"),
      )
      .unwrap();

    let error = store
      .claim_or_replay(
        "workspace.create",
        "key-1",
        "fp-a",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed for a conflict"),
      )
      .unwrap_err();
    assert!(error.starts_with("idempotency_conflict:"), "{error}");
  }

  #[test]
  fn stale_pending_record_recovers_as_already_applied() {
    let dir = TempDir::new().unwrap();
    let store = IdempotencyStore::open_at(&temp_db_path(&dir)).unwrap();
    store
      .claim_or_replay("workspace.create", "key-1", "fp-1", Duration::ZERO, || {
        panic!("first claim on an empty store is never stale")
      })
      .unwrap();
    // Never completed or abandoned: simulates a process that died mid-mutation.

    let result = store
      .claim_or_replay("workspace.create", "key-1", "fp-1", Duration::ZERO, || {
        Ok(RecoveryDecision::AlreadyApplied {
          observed: serde_json::json!({"branch_name": "feature-1"}),
        })
      })
      .unwrap();
    assert_eq!(
      result,
      Some(serde_json::json!({"branch_name": "feature-1"}))
    );

    // A further replay after recovery returns the verification-derived
    // value without invoking recovery again.
    let replay = store
      .claim_or_replay("workspace.create", "key-1", "fp-1", Duration::ZERO, || {
        panic!("already resolved as completed; no recovery expected")
      })
      .unwrap();
    assert_eq!(
      replay,
      Some(serde_json::json!({"branch_name": "feature-1"}))
    );
  }

  #[test]
  fn stale_pending_record_recovers_as_verified_not_applied_and_is_safe_to_execute() {
    let dir = TempDir::new().unwrap();
    let store = IdempotencyStore::open_at(&temp_db_path(&dir)).unwrap();
    store
      .claim_or_replay("workspace.create", "key-1", "fp-1", Duration::ZERO, || {
        panic!("first claim on an empty store is never stale")
      })
      .unwrap();

    let result = store
      .claim_or_replay("workspace.create", "key-1", "fp-1", Duration::ZERO, || {
        Ok(RecoveryDecision::VerifiedNotApplied)
      })
      .unwrap();
    // `None` means "safe to execute now", under the same idempotency key.
    assert_eq!(result, None);

    store
      .complete("workspace.create", "key-1", &serde_json::json!({"id": 7}))
      .unwrap();
    let replay = store
      .claim_or_replay(
        "workspace.create",
        "key-1",
        "fp-1",
        DEFAULT_STALE_AFTER,
        || panic!("already completed"),
      )
      .unwrap();
    assert_eq!(replay, Some(serde_json::json!({"id": 7})));
  }

  #[test]
  fn stale_pending_record_recovers_as_ambiguous_and_is_not_executed() {
    let dir = TempDir::new().unwrap();
    let store = IdempotencyStore::open_at(&temp_db_path(&dir)).unwrap();
    store
      .claim_or_replay("file.patch", "key-1", "fp-1", Duration::ZERO, || {
        panic!("first claim on an empty store is never stale")
      })
      .unwrap();

    let error = store
      .claim_or_replay("file.patch", "key-1", "fp-1", Duration::ZERO, || {
        Ok(RecoveryDecision::Ambiguous)
      })
      .unwrap_err();
    assert!(error.starts_with("idempotency_ambiguous:"), "{error}");

    // Left pending, so a later attempt still has to reason about staleness
    // rather than silently treating the record as resolved.
    let error_again = store
      .claim_or_replay("file.patch", "key-1", "fp-1", Duration::ZERO, || {
        Ok(RecoveryDecision::Ambiguous)
      })
      .unwrap_err();
    assert!(
      error_again.starts_with("idempotency_ambiguous:"),
      "{error_again}"
    );
  }

  #[test]
  fn restart_simulation_preserves_pending_claim_across_store_instances() {
    let dir = TempDir::new().unwrap();
    let db_path = temp_db_path(&dir);

    let before_restart = IdempotencyStore::open_at(&db_path).unwrap();
    before_restart
      .claim_or_replay("agent.start", "key-1", "fp-1", DEFAULT_STALE_AFTER, || {
        panic!("no recovery needed")
      })
      .unwrap();
    drop(before_restart);

    // A second attempt with the same key/fingerprint, from a "restarted"
    // store instance, must see the still-pending, still-fresh claim rather
    // than a clean slate.
    let after_restart = IdempotencyStore::open_at(&db_path).unwrap();
    let outcome = after_restart
      .try_claim("agent.start", "key-1", "fp-1", DEFAULT_STALE_AFTER)
      .unwrap();
    assert_eq!(outcome, ClaimOutcome::InProgress);
  }

  #[test]
  fn cleanup_removes_only_completed_records_past_retention() {
    let dir = TempDir::new().unwrap();
    let store = IdempotencyStore::open_at(&temp_db_path(&dir)).unwrap();

    store
      .claim_or_replay(
        "workspace.create",
        "old",
        "fp-1",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed"),
      )
      .unwrap();
    store
      .complete("workspace.create", "old", &serde_json::json!({"id": 1}))
      .unwrap();
    // Backdate completion so it falls outside a short retention window.
    store
      .with_connection(|conn| {
        conn
          .execute(
            "UPDATE idempotency_records SET completed_at = ?1 WHERE cache_key = 'old'",
            [(chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339()],
          )
          .unwrap();
        Ok(())
      })
      .unwrap();

    store
      .claim_or_replay(
        "workspace.create",
        "recent",
        "fp-2",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed"),
      )
      .unwrap();
    store
      .complete("workspace.create", "recent", &serde_json::json!({"id": 2}))
      .unwrap();

    store
      .claim_or_replay(
        "workspace.create",
        "still-pending",
        "fp-3",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed"),
      )
      .unwrap();

    let removed = store.cleanup(Duration::from_secs(60 * 60)).unwrap();
    assert_eq!(removed, 1);

    let replay = store
      .claim_or_replay(
        "workspace.create",
        "recent",
        "fp-2",
        DEFAULT_STALE_AFTER,
        || panic!("still within retention"),
      )
      .unwrap();
    assert_eq!(replay, Some(serde_json::json!({"id": 2})));

    // The old, cleaned-up record's key is free to be reused as a brand new
    // claim.
    let reused = store
      .claim_or_replay(
        "workspace.create",
        "old",
        "fp-new",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed for a brand new claim"),
      )
      .unwrap();
    assert_eq!(reused, None);

    // The still-pending record survives cleanup regardless of age: cleanup
    // only ever removes completed records.
    let outcome = store
      .try_claim(
        "workspace.create",
        "still-pending",
        "fp-3",
        DEFAULT_STALE_AFTER,
      )
      .unwrap();
    assert_eq!(outcome, ClaimOutcome::InProgress);
  }

  #[test]
  fn redact_for_storage_strips_prompt_and_patch_bodies() {
    let value = serde_json::json!({
      "workspace": "feature-1",
      "prompt": "do the secret thing",
      "nested": { "patch_base64": "ZGlmZg==", "id": 1 },
      "items": [{ "output": "terminal dump" }],
    });
    let redacted = redact_for_storage(&value);
    assert_eq!(redacted["prompt"], "<redacted>");
    assert_eq!(redacted["nested"]["patch_base64"], "<redacted>");
    assert_eq!(redacted["nested"]["id"], 1);
    assert_eq!(redacted["items"][0]["output"], "<redacted>");
    assert_eq!(redacted["workspace"], "feature-1");
  }

  #[test]
  fn abandon_frees_a_pending_claim_for_reuse() {
    let dir = TempDir::new().unwrap();
    let store = IdempotencyStore::open_at(&temp_db_path(&dir)).unwrap();
    store
      .claim_or_replay(
        "workspace.create",
        "key-1",
        "fp-1",
        DEFAULT_STALE_AFTER,
        || panic!("no recovery needed"),
      )
      .unwrap();
    store.abandon("workspace.create", "key-1").unwrap();

    let outcome = store
      .try_claim("workspace.create", "key-1", "fp-1", DEFAULT_STALE_AFTER)
      .unwrap();
    assert_eq!(outcome, ClaimOutcome::Proceed);
  }
}
