//! Checks run before any rebase: the target is fetched and its local bookmark
//! synced with origin, and a workspace with uncommitted changes or a target
//! that diverged from origin is held back instead of rebased.

use crate::jj;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Why a workspace's last rebase was held back, or ran with a caveat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseHold {
  /// The working copy has changes that are not committed.
  Dirty,
  /// The local target bookmark and `<bookmark>@origin` each have commits the
  /// other lacks.
  TargetDiverged { bookmark: String },
  /// The fetch failed, so the rebase used the last-fetched target.
  StaleTarget { error: String },
}

impl RebaseHold {
  /// Whether the rebase must not run. A stale target only warns.
  pub fn blocks(&self) -> bool {
    !matches!(self, RebaseHold::StaleTarget { .. })
  }

  /// One line for a manual rebase's result.
  pub fn message(&self) -> String {
    match self {
      RebaseHold::Dirty => {
        "Rebase skipped: the workspace has uncommitted changes. Commit or discard them, then rebase."
          .to_string()
      }
      RebaseHold::TargetDiverged { bookmark } => format!(
        "Rebase skipped: local '{bookmark}' and '{bookmark}@origin' have diverged. Push or reset '{bookmark}', then rebase."
      ),
      RebaseHold::StaleTarget { error } => {
        format!("Could not fetch the target, rebased onto the last-fetched one: {error}")
      }
    }
  }
}

/// The target as fetched and synced: whether it diverged, or the fetch failed.
#[derive(Debug, Clone, Default)]
pub struct TargetSync {
  pub diverged: Option<String>,
  pub fetch_error: Option<String>,
}

impl TargetSync {
  /// What a workspace rebasing onto this target is held by, `dirty` first.
  pub fn hold_for(&self, workspace_path: &str) -> Option<RebaseHold> {
    if is_dirty(workspace_path) {
      return Some(RebaseHold::Dirty);
    }
    if let Some(bookmark) = &self.diverged {
      return Some(RebaseHold::TargetDiverged {
        bookmark: bookmark.clone(),
      });
    }
    self
      .fetch_error
      .clone()
      .map(|error| RebaseHold::StaleTarget { error })
  }
}

/// Fetches origin and fast-forwards the local bookmark of `target_branch`
/// (`main` or `origin/main`) to its remote. A local bookmark with commits
/// origin lacks while also behind it is left alone and reported as diverged.
pub fn sync_target(repo_path: &str, target_branch: &str) -> TargetSync {
  let fetch_error = jj::jj_git_fetch(repo_path).err().map(|e| e.to_string());
  let bookmark = target_branch
    .strip_prefix("origin/")
    .unwrap_or(target_branch);
  let diverged = match target_state(repo_path, bookmark) {
    TargetState::Diverged => Some(bookmark.to_string()),
    TargetState::Behind => {
      let remote = format!("{bookmark}@origin");
      if let Err(e) = jj::jj_set_bookmark(repo_path, bookmark, &remote) {
        tracing::warn!("Failed to fast-forward '{bookmark}' to '{remote}': {e}");
      }
      None
    }
    TargetState::Synced => None,
  };
  TargetSync {
    diverged,
    fetch_error,
  }
}

enum TargetState {
  Synced,
  Behind,
  Diverged,
}

fn target_state(repo_path: &str, bookmark: &str) -> TargetState {
  if jj::jj_is_bookmark_conflicted(repo_path, bookmark) {
    return TargetState::Diverged;
  }
  // A bookmark missing on either side has nothing to sync.
  let Ok((ahead, behind)) = jj::jj_bookmark_ahead_behind_origin(repo_path, bookmark) else {
    return TargetState::Synced;
  };
  match (ahead > 0, behind > 0) {
    (true, true) => TargetState::Diverged,
    (false, true) => TargetState::Behind,
    _ => TargetState::Synced,
  }
}

/// Whether the working copy holds changes not yet committed.
fn is_dirty(workspace_path: &str) -> bool {
  jj::jj_is_working_copy_empty(workspace_path).is_ok_and(|empty| !empty)
}

static HOLDS: OnceLock<Mutex<HashMap<(String, i64), RebaseHold>>> = OnceLock::new();

/// Records what held the workspace's latest rebase attempt; `None` clears it.
pub fn record(repo_path: &str, workspace_id: i64, hold: Option<RebaseHold>) {
  let Ok(mut holds) = HOLDS.get_or_init(Default::default).lock() else {
    return;
  };
  let key = (repo_path.to_string(), workspace_id);
  match hold {
    Some(hold) => holds.insert(key, hold),
    None => holds.remove(&key),
  };
}

/// The hold the workspace's last rebase attempt recorded.
pub fn hold(repo_path: &str, workspace_id: i64) -> Option<RebaseHold> {
  HOLDS
    .get()?
    .lock()
    .ok()?
    .get(&(repo_path.to_string(), workspace_id))
    .cloned()
}
