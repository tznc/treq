//! Versioned schema migrations for a repository's `.treq/local.db`.
//!
//! `PRAGMA user_version` records the last migration applied. Opening a db runs
//! every migration above it, in order, and records each version as it lands,
//! so a failure part way resumes from the step that failed on the next open.
//!
//! Each migration lives in `migrations/<unix seconds>_<name>.rs`, and that
//! timestamp is its version, as in Rails, sqlx and Supabase: two branches
//! adding migrations at once cannot claim the same number. `user_version` is
//! a signed 32-bit integer, which holds unix seconds until 2038.
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

/// The version a fully migrated database is at.
pub(super) const LATEST: i64 = MIGRATIONS[MIGRATIONS.len() - 1].version;

pub(super) fn version(conn: &Connection) -> Result<i64, String> {
  conn
    .query_row("PRAGMA user_version", [], |row| row.get(0))
    .map_err(|e| format!("Failed to read local db schema version: {}", e))
}

/// Applies every migration above the database's version, in order.
pub(super) fn run(conn: &Connection, migrations: &[Migration]) -> Result<(), String> {
  let current = version(conn)?;
  for migration in migrations.iter().filter(|m| m.version > current) {
    let record = |conn: &Connection| {
      conn
        .pragma_update(None, "user_version", migration.version)
        .map_err(|e| format!("Failed to record local db schema version: {}", e))
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
    static APPLIED: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
  }

  fn applied() -> Vec<i64> {
    APPLIED.with(|a| a.borrow().clone())
  }

  fn log(version: i64) {
    APPLIED.with(|a| a.borrow_mut().push(version));
  }

  fn step3(conn: &Connection) -> Result<(), String> {
    log(3);
    conn
      .execute("CREATE TABLE three (id INTEGER)", [])
      .map(|_| ())
      .map_err(|e| e.to_string())
  }

  fn step4(conn: &Connection) -> Result<(), String> {
    log(4);
    conn
      .execute("CREATE TABLE four (id INTEGER)", [])
      .map(|_| ())
      .map_err(|e| e.to_string())
  }

  fn failing_step5(conn: &Connection) -> Result<(), String> {
    log(5);
    conn
      .execute("CREATE TABLE five (id INTEGER)", [])
      .map_err(|e| e.to_string())?;
    Err("step 5 broke".into())
  }

  const STEPS: &[Migration] = &[
    Migration {
      version: 3,
      transactional: true,
      apply: step3,
    },
    Migration {
      version: 4,
      transactional: true,
      apply: step4,
    },
  ];

  fn db_at(version: i64) -> Connection {
    APPLIED.with(|a| a.borrow_mut().clear());
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "user_version", version).unwrap();
    conn
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
  fn applies_only_the_migrations_above_the_current_version_in_order() {
    let conn = db_at(3);
    run(&conn, STEPS).unwrap();
    assert_eq!(applied(), [4]);
    assert_eq!(version(&conn).unwrap(), 4);

    let conn = db_at(0);
    run(&conn, STEPS).unwrap();
    assert_eq!(applied(), [3, 4]);
  }

  #[test]
  fn an_up_to_date_database_runs_nothing() {
    let conn = db_at(4);
    run(&conn, STEPS).unwrap();
    assert!(applied().is_empty());
  }

  #[test]
  fn a_database_from_a_newer_release_is_left_alone() {
    let conn = db_at(9);
    run(&conn, STEPS).unwrap();
    assert!(applied().is_empty());
    assert_eq!(version(&conn).unwrap(), 9);
  }

  #[test]
  fn a_failed_migration_rolls_back_and_keeps_the_last_good_version() {
    let steps = [
      Migration {
        version: 3,
        transactional: true,
        apply: step3,
      },
      Migration {
        version: 5,
        transactional: true,
        apply: failing_step5,
      },
    ];
    let conn = db_at(0);
    let err = run(&conn, &steps).unwrap_err();
    assert!(err.contains("step 5 broke"), "{err}");
    assert_eq!(version(&conn).unwrap(), 3);
    assert!(table_exists(&conn, "three"));
    assert!(!table_exists(&conn, "five"));

    // The next open resumes at the failed step.
    APPLIED.with(|a| a.borrow_mut().clear());
    run(&conn, &steps).unwrap_err();
    assert_eq!(applied(), [5]);
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
      // After 2023, and within `PRAGMA user_version`'s signed 32 bits.
      assert!(
        (1_700_000_000..=i64::from(i32::MAX)).contains(&version),
        "{version}"
      );
    }
  }

  #[test]
  fn a_database_at_the_old_baseline_version_upgrades() {
    // Releases before timestamped versions recorded the baseline as 2.
    let conn = db_at(2);
    assert!(version(&conn).unwrap() < LATEST);
  }

  #[test]
  fn migrations_are_in_strictly_increasing_version_order() {
    assert!(MIGRATIONS.windows(2).all(|w| w[0].version < w[1].version));
    assert_eq!(LATEST, MIGRATIONS.last().unwrap().version);
  }
}
