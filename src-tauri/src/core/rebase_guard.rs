//! Checks run before any rebase: the target is fetched and its local bookmark
//! synced with origin, and a workspace with uncommitted changes or a target
//! that diverged from origin is held back instead of rebased.

use crate::{jj, local_db};
use serde::{Deserialize, Serialize};

/// Why a workspace's last rebase was held back, or ran with a caveat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseHold {
  /// The parent workspace (the one on the target branch) has changes that
  /// are not committed.
  ParentDirty { branch: String },
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
      RebaseHold::ParentDirty { branch } => format!(
        "Rebase skipped: parent workspace '{branch}' has uncommitted changes. Commit or discard them there, then rebase."
      ),
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
  /// The branch of the parent workspace, when it has uncommitted changes.
  pub parent_dirty: Option<String>,
  pub diverged: Option<String>,
  pub fetch_error: Option<String>,
}

impl TargetSync {
  /// What a workspace rebasing onto this target is held by, a dirty parent
  /// first. The workspace's own uncommitted changes never hold it.
  pub fn hold(&self) -> Option<RebaseHold> {
    if let Some(branch) = &self.parent_dirty {
      return Some(RebaseHold::ParentDirty {
        branch: branch.clone(),
      });
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
    parent_dirty: parent_dirty(repo_path, bookmark),
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

/// `branch`, when a workspace on it has changes not yet committed: its
/// children would rebase onto a parent that is about to move.
fn parent_dirty(repo_path: &str, branch: &str) -> Option<String> {
  let parent = local_db::get_workspace_by_branch(repo_path, branch).ok()??;
  let path = crate::auto_rebase::get_full_workspace_path(&parent);
  jj::jj_is_working_copy_empty(&path)
    .is_ok_and(|empty| !empty)
    .then(|| branch.to_string())
}

/// Stores what held the workspace's latest rebase attempt with the workspace,
/// so the app sees holds the CLI's rebases record; `None` clears it.
pub fn record(repo_path: &str, workspace_id: i64, hold: Option<RebaseHold>) {
  let value = hold.and_then(|h| serde_json::to_value(h).ok());
  if let Err(e) = local_db::set_workspace_rebase_hold(repo_path, workspace_id, value) {
    tracing::warn!("Failed to store rebase hold for workspace {workspace_id}: {e}");
  }
}

/// The hold the workspace's last rebase attempt recorded.
pub fn hold(repo_path: &str, workspace_id: i64) -> Option<RebaseHold> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id).ok()??;
  serde_json::from_value(local_db::rebase_hold_of(&workspace)?).ok()
}
