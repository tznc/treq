//! Versioned schema migrations for a repository's `.treq/local.db`.
//!
//! `schema_migrations` records each migration applied. Opening a db runs every
//! migration not recorded there, oldest first, recording each as it lands, so
//! a failure part way resumes from the step that failed on the next open.
//!
//! Each migration lives in `migrations/<unix seconds>_<name>.rs`, and that
//! timestamp is its version, as in Rails, sqlx and Supabase: two branches
//! adding migrations at once cannot claim the same number. Applied versions
//! are rows in `schema_migrations`, so a migration merged after a newer one
//! still runs.
//!
//! The first migration is the baseline: the schema as it stood when
//! migrations were introduced. It checks the file's actual tables and columns rather than its
//! version, because databases from before versioning sit at 0 in any of the
//! shapes earlier releases left. Later migrations can assume the baseline.

use rusqlite::Connection;

#[path = "migrations/1793162598_baseline.rs"]
mod baseline;

pub(super) struct Migration {
  /// The `user_version` this migration brings a database to.
  pub version: i64,
  /// Run inside a transaction with its version bump. The baseline manages its
  /// own transactions around table rebuilds, so it cannot.
  pub transactional: bool,
  pub apply: fn(&Connection) -> Result<(), String>,
}

/// Every migration, oldest first. Add one as `migrations/<date +%s>_<name>.rs`
/// with a `mod` above and an entry here; never edit a released one.
pub(super) const MIGRATIONS: &[Migration] = &[Migration {
  version: 1_793_162_598,
  transactional: false,
  apply: baseline::apply,
}];

/// Versions recorded as applied: the rows of `schema_migrations`, plus, for
/// files from releases that recorded only the last version in
/// `user_version`, every known migration at or below it.
pub(super) fn applied_versions(conn: &Connection) -> Result<Vec<i64>, String> {
  let read = |e: rusqlite::Error| format!("Failed to read local db migrations: {}", e);
  let legacy: i64 = conn
    .query_row("PRAGMA user_version", [], |row| row.get(0))
    .map_err(read)?;
  let mut versions: Vec<i64> = MIGRATIONS
    .iter()
    .map(|m| m.version)
    .filter(|v| *v <= legacy)
    .collect();
  let has_table: bool = conn
    .query_row(
      "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
      [],
      |row| row.get(0),
    )
    .map_err(read)?;
  if has_table {
    let mut stmt = conn
      .prepare("SELECT version FROM schema_migrations")
      .map_err(read)?;
    for version in stmt.query_map([], |row| row.get(0)).map_err(read)? {
      versions.push(version.map_err(read)?);
    }
  }
  versions.sort_unstable();
  versions.dedup();
  Ok(versions)
}

/// Whether every one of `migrations` is applied. The common case on every
/// open, so it costs two small reads.
pub(super) fn is_current(conn: &Connection, migrations: &[Migration]) -> Result<bool, String> {
  let applied = applied_versions(conn)?;
  Ok(
    migrations
      .iter()
      .all(|m| applied.binary_search(&m.version).is_ok()),
  )
}

/// Applies, oldest first, every migration not yet recorded, including one
/// older than migrations already applied (merged after them).
pub(super) fn run(conn: &Connection, migrations: &[Migration]) -> Result<(), String> {
  let applied = applied_versions(conn)?;
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS schema_migrations (
         version INTEGER PRIMARY KEY,
         applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
       )",
      [],
    )
    .map_err(|e| format!("Failed to create schema_migrations: {}", e))?;
  for migration in migrations
    .iter()
    .filter(|m| applied.binary_search(&m.version).is_err())
  {
    let record = |conn: &Connection| {
      conn
        .execute(
          "INSERT OR IGNORE INTO schema_migrations (version) VALUES (?1)",
          [migration.version],
        )
        .map(|_| ())
        .map_err(|e| format!("Failed to record migration {}: {}", migration.version, e))
    };
    if migration.transactional {
      let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to start migration {}: {}", migration.version, e))?;
      (migration.apply)(&tx)?;
      record(&tx)?;
      tx.commit()
        .map_err(|e| format!("Failed to commit migration {}: {}", migration.version, e))?;
    } else {
      (migration.apply)(conn)?;
      record(conn)?;
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::cell::RefCell;

  thread_local! {
    static RAN: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
  }

  fn ran() -> Vec<i64> {
    RAN.with(|a| a.borrow().clone())
  }

  fn create(conn: &Connection, version: i64, table: &str) -> Result<(), String> {
    RAN.with(|a| a.borrow_mut().push(version));
    conn
      .execute(&format!("CREATE TABLE {table} (id INTEGER)"), [])
      .map(|_| ())
      .map_err(|e| e.to_string())
  }

  // Versions past 2038, which no longer fit `PRAGMA user_version`.
  const A: i64 = 2_200_000_000;
  const B: i64 = 2_200_000_100;
  const C: i64 = 2_200_000_200;

  fn step_a(conn: &Connection) -> Result<(), String> {
    create(conn, A, "a")
  }

  fn step_b(conn: &Connection) -> Result<(), String> {
    create(conn, B, "b")
  }

  fn failing_step_c(conn: &Connection) -> Result<(), String> {
    create(conn, C, "c")?;
    Err("step c broke".into())
  }

  fn migration(version: i64, apply: fn(&Connection) -> Result<(), String>) -> Migration {
    Migration {
      version,
      transactional: true,
      apply,
    }
  }

  fn fresh_db() -> Connection {
    RAN.with(|a| a.borrow_mut().clear());
    Connection::open_in_memory().unwrap()
  }

  fn table_exists(conn: &Connection, name: &str) -> bool {
    conn
      .query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
      )
      .unwrap()
      > 0
  }

  #[test]
  fn records_versions_past_2038() {
    let conn = fresh_db();
    let steps = [migration(A, step_a), migration(B, step_b)];
    run(&conn, &steps).unwrap();
    assert_eq!(ran(), [A, B]);
    assert_eq!(applied_versions(&conn).unwrap(), [A, B]);
    assert!(is_current(&conn, &steps).unwrap());
  }

  #[test]
  fn an_up_to_date_database_runs_nothing() {
    let conn = fresh_db();
    let steps = [migration(A, step_a)];
    run(&conn, &steps).unwrap();
    RAN.with(|a| a.borrow_mut().clear());
    run(&conn, &steps).unwrap();
    assert!(ran().is_empty());
  }

  #[test]
  fn a_migration_merged_after_a_newer_one_still_runs() {
    // Branch one adds B, branch two adds A; B's branch merges and ships first.
    let conn = fresh_db();
    run(&conn, &[migration(B, step_b)]).unwrap();
    let steps = [migration(A, step_a), migration(B, step_b)];
    assert!(!is_current(&conn, &steps).unwrap());
    run(&conn, &steps).unwrap();
    assert_eq!(ran(), [B, A]);
    assert_eq!(applied_versions(&conn).unwrap(), [A, B]);
  }

  #[test]
  fn a_database_from_a_newer_release_keeps_its_unknown_migrations() {
    let conn = fresh_db();
    run(&conn, &[migration(A, step_a), migration(B, step_b)]).unwrap();
    // An older app only knows A: nothing runs and B's record stays.
    let older = [migration(A, step_a)];
    assert!(is_current(&conn, &older).unwrap());
    run(&conn, &older).unwrap();
    assert_eq!(applied_versions(&conn).unwrap(), [A, B]);
  }

  #[test]
  fn a_failed_migration_rolls_back_and_is_not_recorded() {
    let conn = fresh_db();
    let steps = [migration(A, step_a), migration(C, failing_step_c)];
    let err = run(&conn, &steps).unwrap_err();
    assert!(err.contains("step c broke"), "{err}");
    assert_eq!(applied_versions(&conn).unwrap(), [A]);
    assert!(table_exists(&conn, "a"));
    assert!(!table_exists(&conn, "c"));

    // The next open resumes at the failed step.
    RAN.with(|a| a.borrow_mut().clear());
    run(&conn, &steps).unwrap_err();
    assert_eq!(ran(), [C]);
  }

  #[test]
  fn user_version_from_earlier_releases_counts_as_applied() {
    // Releases before the table recorded the last migration in
    // `user_version`: 2 for the pre-timestamp baseline, then the baseline's
    // timestamp. Only migrations at or below it count as applied.
    let baseline = MIGRATIONS[0].version;
    for (user_version, applied) in [(0, vec![]), (2, vec![]), (baseline, vec![baseline])] {
      let conn = fresh_db();
      conn
        .pragma_update(None, "user_version", user_version)
        .unwrap();
      assert_eq!(applied_versions(&conn).unwrap(), applied, "{user_version}");
    }
  }

  #[test]
  fn each_migration_file_is_named_by_its_unix_timestamp_version() {
    // `migrations/<unix seconds>_<name>.rs`, as Rails, sqlx and Supabase name
    // theirs: branches adding migrations in parallel cannot pick the same one.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/local_db/migrations");
    let mut stamps: Vec<i64> = std::fs::read_dir(&dir)
      .expect("migrations dir")
      .map(|entry| {
        let name = entry
          .expect("entry")
          .file_name()
          .to_string_lossy()
          .into_owned();
        let (stamp, rest) = name
          .split_once('_')
          .unwrap_or_else(|| panic!("{name}: no `_`"));
        assert!(rest.ends_with(".rs"), "{name}: not a .rs file");
        stamp
          .parse()
          .unwrap_or_else(|_| panic!("{name}: prefix is not a unix timestamp"))
      })
      .collect();
    stamps.sort_unstable();
    let versions: Vec<i64> = MIGRATIONS.iter().map(|m| m.version).collect();
    assert_eq!(stamps, versions);
    for version in versions {
      assert!(
        version >= 1_700_000_000,
        "{version} is not a unix timestamp"
      );
    }
  }

  #[test]
  fn migrations_are_in_strictly_increasing_version_order() {
    assert!(MIGRATIONS.windows(2).all(|w| w[0].version < w[1].version));
  }
}
