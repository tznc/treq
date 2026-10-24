//! The remote records in this app's settings database (treq's
//! `core::remote_records`, shared with the desktop app).

use std::sync::Mutex;

use treq_lib::core::remote_records::{self as records, SavedRepository};
pub use treq_lib::core::remote_records::{
  canonicalize_remote_path, endpoint_generation, SessionEndpoint, SessionSnapshot,
  SnapshotWorkspace, UserManagedEndpoint,
};

use crate::backend;

/// The lists as last read. Pages read them while rendering, every frame
/// while a spinner animates, so they come from memory instead of a query
/// and a JSON parse. Every write goes through this module and drops them.
static HOSTS: Mutex<Option<Vec<UserManagedEndpoint>>> = Mutex::new(None);
static REPOSITORIES: Mutex<Option<Vec<SavedRepository>>> = Mutex::new(None);

fn cached<T: Clone>(
  cache: &Mutex<Option<Vec<T>>>,
  read: impl FnOnce(&treq_lib::db::Database) -> Vec<T>,
) -> Vec<T> {
  let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
  cache
    .get_or_insert_with(|| backend::get().with_db(read))
    .clone()
}

fn drop_cached<T>(cache: &Mutex<Option<Vec<T>>>) {
  *cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn user_managed_endpoints() -> Vec<UserManagedEndpoint> {
  cached(&HOSTS, records::user_managed_endpoints)
}

/// Saves `endpoint` first in the list, replacing any record with its id.
pub fn save_user_managed_endpoint(endpoint: UserManagedEndpoint) {
  backend::get().with_db(|db| records::save_user_managed_endpoint(db, endpoint));
  drop_cached(&HOSTS);
}

pub fn remove_user_managed_endpoint(id: &str) {
  backend::get().with_db(|db| records::remove_user_managed_endpoint(db, id));
  drop_cached(&HOSTS);
}

pub fn saved_repositories(endpoint_id: &str, generation: u64) -> Vec<SavedRepository> {
  // The same filter as `records::saved_repositories`, over the cached list.
  cached(&REPOSITORIES, records::all_saved_repositories)
    .into_iter()
    .filter(|r| r.endpoint_id == endpoint_id && r.endpoint_generation == generation)
    .collect()
}

/// Records a repository that probed successfully, most recent first.
pub fn upsert_saved_repository(endpoint_id: &str, generation: u64, path: &str) {
  backend::get().with_db(|db| records::upsert_saved_repository(db, endpoint_id, generation, path));
  drop_cached(&REPOSITORIES);
}

pub fn session_snapshot() -> Option<SessionSnapshot> {
  backend::get().with_db(records::session_snapshot)
}

pub fn save_session_snapshot(snapshot: Option<&SessionSnapshot>) {
  backend::get().with_db(|db| records::save_session_snapshot(db, snapshot))
}

#[cfg(test)]
mod tests {
  use super::*;

  /// What a machine-list frame paid per read before the lists were cached:
  /// a settings query plus a JSON parse.
  /// `cargo test --lib store_read_cost -- --ignored --nocapture`
  #[test]
  #[ignore]
  fn store_read_cost() {
    let dir = std::env::temp_dir().join(format!("treq-gpui-store-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = treq_lib::db::Database::new(dir.join("app.db")).unwrap();
    db.init().unwrap();
    for i in 0..4 {
      records::save_user_managed_endpoint(
        &db,
        UserManagedEndpoint {
          id: format!("user-managed-{i}"),
          display_name: format!("Host {i}"),
          hostname: format!("host{i}.lan"),
          port: 22,
          username: "dev".into(),
          host_key_fingerprint: "SHA256:abc".into(),
          auth_identity_reference: "~/.ssh/id_ed25519".into(),
          alias: None,
          created_at: "2026-01-01T00:00:00Z".into(),
        },
      );
      for r in 0..5 {
        records::upsert_saved_repository(
          &db,
          &format!("user-managed-{i}"),
          0,
          &format!("~/src/repo-{r}"),
        );
      }
    }
    let n = 2000;
    let time = |f: &dyn Fn()| {
      let start = std::time::Instant::now();
      for _ in 0..n {
        f();
      }
      start.elapsed() / n
    };
    let hosts = time(&|| {
      std::hint::black_box(records::user_managed_endpoints(&db));
    });
    let repos = time(&|| {
      std::hint::black_box(records::saved_repositories(&db, "user-managed-2", 0));
    });
    let all = records::all_saved_repositories(&db);
    let cached = time(&|| {
      std::hint::black_box(
        all
          .clone()
          .into_iter()
          .filter(|r| r.endpoint_id == "user-managed-2")
          .collect::<Vec<_>>(),
      );
    });
    // A machine list of 4 hosts reads the hosts twice and each host's
    // repositories once per frame.
    eprintln!(
      "hosts query {hosts:?}, repositories query {repos:?}, cached repositories {cached:?}; \
       4-host machine-list frame: {:?} from the database",
      hosts * 2 + repos * 4
    );
    let _ = std::fs::remove_dir_all(dir);
  }
}
