//! The remote records in this app's settings database (treq's
//! `core::remote_records`, shared with the desktop app).

use treq_lib::core::remote_records::{self as records, SavedRepository};
pub use treq_lib::core::remote_records::{
  canonicalize_remote_path, endpoint_generation, SessionEndpoint, SessionSnapshot,
  SnapshotWorkspace, UserManagedEndpoint,
};

use crate::backend;

pub fn user_managed_endpoints() -> Vec<UserManagedEndpoint> {
  backend::get().with_db(records::user_managed_endpoints)
}

/// Saves `endpoint` first in the list, replacing any record with its id.
pub fn save_user_managed_endpoint(endpoint: UserManagedEndpoint) {
  backend::get().with_db(|db| records::save_user_managed_endpoint(db, endpoint))
}

pub fn remove_user_managed_endpoint(id: &str) {
  backend::get().with_db(|db| records::remove_user_managed_endpoint(db, id))
}

pub fn saved_repositories(endpoint_id: &str, generation: u64) -> Vec<SavedRepository> {
  backend::get().with_db(|db| records::saved_repositories(db, endpoint_id, generation))
}

/// Records a repository that probed successfully, most recent first.
pub fn upsert_saved_repository(endpoint_id: &str, generation: u64, path: &str) {
  backend::get().with_db(|db| records::upsert_saved_repository(db, endpoint_id, generation, path))
}

pub fn session_snapshot() -> Option<SessionSnapshot> {
  backend::get().with_db(records::session_snapshot)
}

pub fn save_session_snapshot(snapshot: Option<&SessionSnapshot>) {
  backend::get().with_db(|db| records::save_session_snapshot(db, snapshot))
}
