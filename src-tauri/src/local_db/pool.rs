//! Idle SQLite connections kept open across `get_connection` calls.
//!
//! Opening a connection costs ~0.7M instructions, and under WAL the last
//! connection to close a database checkpoints it: ~4 fsyncs (wal, dir, wal,
//! db) per write despite `synchronous=NORMAL`. Keeping a connection idle per
//! database avoids both, while SQLite's autocheckpoint (every 1000 WAL pages)
//! still keeps the WAL bounded. A connection serves one caller at a time:
//! it is checked out, moved to the caller's thread, and given back on drop.

use crate::lock_ext::LockExt;
use rusqlite::Connection;
use std::fs;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(super) struct ConnectionPool {
  /// Least recently given back first.
  idle: Mutex<Vec<IdleConnection>>,
  /// Idle connections kept across all databases (each holds two files open).
  capacity: usize,
  /// Idle connections kept per database, so one busy repo cannot evict the rest.
  per_db: usize,
}

struct IdleConnection {
  db_path: PathBuf,
  identity: DbFileIdentity,
  conn: Connection,
}

/// The file a connection has open. On Unix a deleted or replaced db stays
/// readable and writable through an open connection, so a pooled connection
/// to the old file would lose every later write.
#[cfg(unix)]
type DbFileIdentity = (u64, u64);
/// Windows cannot delete or rename a file SQLite holds open, so while the
/// path exists it names the file the connection has open.
#[cfg(not(unix))]
type DbFileIdentity = ();

fn db_file_identity(db_path: &Path) -> Option<DbFileIdentity> {
  let metadata = fs::metadata(db_path).ok()?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
  }
  #[cfg(not(unix))]
  {
    let _ = metadata;
    Some(())
  }
}

/// A connection checked out of a pool, given back when dropped.
pub(crate) struct PooledConnection<'a> {
  conn: ManuallyDrop<Connection>,
  db_path: PathBuf,
  identity: Option<DbFileIdentity>,
  pool: &'a ConnectionPool,
}

impl Deref for PooledConnection<'_> {
  type Target = Connection;

  fn deref(&self) -> &Connection {
    &self.conn
  }
}

impl DerefMut for PooledConnection<'_> {
  fn deref_mut(&mut self) -> &mut Connection {
    &mut self.conn
  }
}

impl Drop for PooledConnection<'_> {
  fn drop(&mut self) {
    // SAFETY: `conn` is taken once, here, and never used again.
    let conn = unsafe { ManuallyDrop::take(&mut self.conn) };
    // A connection left inside a transaction, or whose caller panicked, is closed instead.
    if std::thread::panicking() || !conn.is_autocommit() {
      return;
    }
    let Some(identity) = self.identity else {
      return;
    };
    self.pool.give_back(IdleConnection {
      db_path: std::mem::take(&mut self.db_path),
      identity,
      conn,
    });
  }
}

impl ConnectionPool {
  pub(super) const fn new(capacity: usize, per_db: usize) -> Self {
    Self {
      idle: Mutex::new(Vec::new()),
      capacity,
      per_db,
    }
  }

  /// Hands out an idle connection to `db_path`, or one from `open`.
  pub(super) fn check_out(
    &self,
    db_path: &Path,
    open: impl FnOnce() -> Result<Connection, String>,
  ) -> Result<PooledConnection<'_>, String> {
    let identity = db_file_identity(db_path);
    let (reused, stale) = {
      let mut idle = self.idle.lock_or_recover();
      match idle.iter().rposition(|entry| entry.db_path == db_path) {
        Some(index) if identity == Some(idle[index].identity) => {
          (Some(idle.remove(index).conn), Vec::new())
        }
        Some(_) => (
          None,
          drain_where(&mut idle, |entry| entry.db_path == db_path),
        ),
        None => (None, Vec::new()),
      }
    };
    // Closing can checkpoint (and fsync), so it happens outside the lock.
    drop(stale);
    let (conn, identity) = match reused {
      Some(conn) => (conn, identity),
      None => (open()?, db_file_identity(db_path)),
    };
    Ok(PooledConnection {
      conn: ManuallyDrop::new(conn),
      db_path: db_path.to_path_buf(),
      identity,
      pool: self,
    })
  }

  fn give_back(&self, returned: IdleConnection) {
    let closed = {
      let mut idle = self.idle.lock_or_recover();
      let same_db = idle
        .iter()
        .filter(|entry| entry.db_path == returned.db_path)
        .count();
      if same_db >= self.per_db {
        Some(returned)
      } else {
        idle.push(returned);
        (idle.len() > self.capacity).then(|| idle.remove(0))
      }
    };
    drop(closed);
  }

  /// Closes the idle connections to databases under `dir`.
  pub(super) fn release_under(&self, dir: &Path) {
    let released = drain_where(&mut self.idle.lock_or_recover(), |entry| {
      entry.db_path.starts_with(dir)
    });
    drop(released);
  }

  /// Closes every idle connection, checkpointing each database it last held.
  pub(super) fn release_all(&self) {
    let released = std::mem::take(&mut *self.idle.lock_or_recover());
    drop(released);
  }

  #[cfg(test)]
  fn idle_count(&self) -> usize {
    self.idle.lock_or_recover().len()
  }
}

fn drain_where(
  idle: &mut Vec<IdleConnection>,
  matches: impl Fn(&IdleConnection) -> bool,
) -> Vec<IdleConnection> {
  let (drained, kept) = std::mem::take(idle).into_iter().partition(&matches);
  *idle = kept;
  drained
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn db_in(temp_dir: &TempDir, name: &str) -> PathBuf {
    temp_dir.path().join(name).join("local.db")
  }

  fn open(db_path: &Path) -> Result<Connection, String> {
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    Connection::open(db_path).map_err(|e| e.to_string())
  }

  fn refuse_open() -> Result<Connection, String> {
    Err("opened a new connection".to_string())
  }

  /// A TEMP table lives only on the connection that made it.
  fn mark(conn: &Connection) {
    conn.execute("CREATE TEMP TABLE marker (x)", []).unwrap();
  }

  fn is_marked(conn: &Connection) -> bool {
    conn
      .query_row(
        "SELECT COUNT(*) FROM sqlite_temp_master WHERE name = 'marker'",
        [],
        |row| row.get::<_, i64>(0),
      )
      .unwrap()
      == 1
  }

  #[test]
  fn reuses_connection_given_back() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = db_in(&temp_dir, "a");
    let pool = ConnectionPool::new(4, 2);
    mark(&pool.check_out(&db_path, || open(&db_path)).unwrap());

    let conn = pool.check_out(&db_path, refuse_open).unwrap();
    assert!(is_marked(&conn));
  }

  #[test]
  fn opens_second_connection_while_first_is_checked_out() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = db_in(&temp_dir, "a");
    let pool = ConnectionPool::new(4, 2);
    let first = pool.check_out(&db_path, || open(&db_path)).unwrap();
    mark(&first);

    let second = pool.check_out(&db_path, || open(&db_path)).unwrap();
    assert!(!is_marked(&second));
    drop(first);
    drop(second);
    assert_eq!(pool.idle_count(), 2);
  }

  #[test]
  fn closes_connection_left_in_transaction() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = db_in(&temp_dir, "a");
    let pool = ConnectionPool::new(4, 2);
    let conn = pool.check_out(&db_path, || open(&db_path)).unwrap();
    conn.execute_batch("BEGIN; CREATE TABLE t (x);").unwrap();
    drop(conn);

    assert_eq!(pool.idle_count(), 0);
  }

  #[test]
  fn keeps_connection_after_transaction_ends() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = db_in(&temp_dir, "a");
    let pool = ConnectionPool::new(4, 2);
    let mut conn = pool.check_out(&db_path, || open(&db_path)).unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute("CREATE TABLE t (x)", []).unwrap();
    tx.commit().unwrap();
    drop(conn);

    assert_eq!(pool.idle_count(), 1);
  }

  #[test]
  fn opens_new_connection_when_db_file_is_gone() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = db_in(&temp_dir, "a");
    let pool = ConnectionPool::new(4, 2);
    mark(&pool.check_out(&db_path, || open(&db_path)).unwrap());
    pool.release_under(temp_dir.path());
    fs::remove_dir_all(db_path.parent().unwrap()).unwrap();

    let conn = pool.check_out(&db_path, || open(&db_path)).unwrap();
    assert!(!is_marked(&conn));
  }

  #[cfg(unix)]
  #[test]
  fn opens_new_connection_when_db_file_is_replaced() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = db_in(&temp_dir, "a");
    let pool = ConnectionPool::new(4, 2);
    mark(&pool.check_out(&db_path, || open(&db_path)).unwrap());
    fs::remove_dir_all(db_path.parent().unwrap()).unwrap();
    drop(open(&db_path).unwrap());

    let conn = pool.check_out(&db_path, || open(&db_path)).unwrap();
    assert!(!is_marked(&conn));
    drop(conn);
    assert_eq!(pool.idle_count(), 1, "the stale connection is closed");
  }

  #[test]
  fn keeps_at_most_capacity_idle_connections() {
    let temp_dir = TempDir::new().unwrap();
    let pool = ConnectionPool::new(2, 2);
    for name in ["a", "b", "c"] {
      let db_path = db_in(&temp_dir, name);
      drop(pool.check_out(&db_path, || open(&db_path)).unwrap());
    }

    assert_eq!(pool.idle_count(), 2);
    let oldest = db_in(&temp_dir, "a");
    assert!(pool.check_out(&oldest, refuse_open).is_err());
  }

  #[test]
  fn keeps_at_most_per_db_idle_connections_for_one_db() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = db_in(&temp_dir, "a");
    let pool = ConnectionPool::new(8, 2);
    let conns: Vec<_> = (0..3)
      .map(|_| pool.check_out(&db_path, || open(&db_path)).unwrap())
      .collect();
    drop(conns);

    assert_eq!(pool.idle_count(), 2);
  }

  #[test]
  fn release_under_closes_only_connections_under_dir() {
    let temp_dir = TempDir::new().unwrap();
    let pool = ConnectionPool::new(8, 2);
    let a = db_in(&temp_dir, "a");
    let b = db_in(&temp_dir, "b");
    drop(pool.check_out(&a, || open(&a)).unwrap());
    drop(pool.check_out(&b, || open(&b)).unwrap());

    pool.release_under(&temp_dir.path().join("a"));

    assert!(pool.check_out(&a, refuse_open).is_err());
    assert!(pool.check_out(&b, refuse_open).is_ok());
  }
}
