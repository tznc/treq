pub mod agent_chat;
pub mod agent_cli;
pub mod agent_supervisor;
pub mod app;
pub mod auto_update;
pub mod browser_review;
pub mod changes;
pub mod checks;
pub mod checks_logs;
pub mod commits;
pub mod feature_preview;
pub mod files;
pub mod github_issues;
pub mod idempotency_store;
pub mod prerequisites;
pub mod pty_remote_supervisor;
pub mod remote;
pub mod remote_control_plane;
pub mod remote_device_key;
pub mod remote_local_keys;
pub mod remote_provider;
pub mod remote_provider_sprites;
pub mod remote_pty;
pub mod remote_ssh_config;
pub mod remote_ssh_transport;
pub mod remote_ssh_ws_stream;
pub mod repo;
pub mod resolve;
pub mod sessions;
pub mod skills;
pub mod stash;
pub mod submodules;
pub mod supporting_repos;
pub mod workspaces;
use crate::lock_ext::LockExt;
pub use agent_chat::*;
pub use agent_cli::*;
pub use app::*;
pub use auto_update::*;
pub use browser_review::*;
pub use changes::*;
pub use checks::*;
pub use checks_logs::*;
pub use commits::*;
pub use files::*;
pub use prerequisites::*;
pub use repo::*;
pub use resolve::*;
pub use skills::*;
pub use stash::*;
use std::path::{Path, PathBuf};
pub use submodules::*;
pub use workspaces::*;

pub const DEFAULT_CONFLICT_MARKER_STYLE: &str = "git";

pub fn resolve_conflict_marker_style_from_db(db: &crate::db::Database) -> String {
  db.get_setting("conflict_marker_style")
    .ok()
    .flatten()
    .and_then(|value| {
      let trimmed = value.trim();
      if trimmed.is_empty() {
        None
      } else {
        Some(trimmed.to_string())
      }
    })
    .unwrap_or_else(|| DEFAULT_CONFLICT_MARKER_STYLE.to_string())
}

pub fn resolve_conflict_marker_style(db: &std::sync::Mutex<crate::db::Database>) -> String {
  resolve_conflict_marker_style_from_db(&db.lock_or_recover())
}

/// File name of the app database inside the app data dir.
pub const APP_DB_FILE_NAME: &str = "treq.db";

/// The app database inside the app data dir, falling back to `<repo>/.treq/treq.db`.
pub fn resolve_app_db_path(repo_path: &str) -> PathBuf {
  resolve_app_db_path_with(repo_path, |key| std::env::var(key).ok())
}

/// `resolve_app_db_path` reading env vars through `env`, so tests need not mutate the process env.
fn resolve_app_db_path_with(repo_path: &str, env: impl Fn(&str) -> Option<String>) -> PathBuf {
  app_data_dir_with(env)
    .unwrap_or_else(|| Path::new(repo_path).join(".treq"))
    .join(APP_DB_FILE_NAME)
}

#[cfg(test)]
thread_local! {
  /// Per-thread app data dir for tests; the env var would leak across parallel tests.
  pub(crate) static TEST_APP_DATA_DIR: std::cell::RefCell<Option<PathBuf>> =
    const { std::cell::RefCell::new(None) };
}

/// Points this test thread's app data dir at `dir` until dropped.
#[cfg(test)]
pub(crate) struct AppDataDirGuard;

#[cfg(test)]
impl AppDataDirGuard {
  pub(crate) fn set(dir: &Path) -> Self {
    TEST_APP_DATA_DIR.with(|d| *d.borrow_mut() = Some(dir.to_path_buf()));
    Self
  }
}

#[cfg(test)]
impl Drop for AppDataDirGuard {
  fn drop(&mut self) {
    TEST_APP_DATA_DIR.with(|d| *d.borrow_mut() = None);
  }
}

/// The app data dir from `TREQ_APP_DATA_DIR`, or a test's per-thread override.
pub(crate) fn app_data_dir() -> Option<PathBuf> {
  app_data_dir_with(|key| std::env::var(key).ok())
}

fn app_data_dir_with(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
  #[cfg(test)]
  if let Some(dir) = TEST_APP_DATA_DIR.with(|dir| dir.borrow().clone()) {
    return Some(dir);
  }
  env("TREQ_APP_DATA_DIR")
    .map(|dir| dir.trim().to_string())
    .filter(|dir| !dir.is_empty())
    .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
  use super::resolve_app_db_path_with;
  use std::path::Path;

  #[test]
  fn resolve_app_db_path_uses_app_data_dir() {
    let env = |key: &str| (key == "TREQ_APP_DATA_DIR").then(|| "/tmp/app-data".to_string());

    let resolved = resolve_app_db_path_with("/repo/path", env);
    assert_eq!(resolved, Path::new("/tmp/app-data").join("treq.db"));
  }

  #[test]
  fn resolve_app_db_path_falls_back_to_repo_dir() {
    let resolved = resolve_app_db_path_with("/repo/path", |_| None);
    assert_eq!(resolved, Path::new("/repo/path/.treq/treq.db"));
  }
}
