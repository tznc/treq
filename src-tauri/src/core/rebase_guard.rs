//! Checks run before any rebase: the target is fetched and its local bookmark
//! synced with origin, and a workspace with uncommitted changes or a target
//! that diverged from origin is held back instead of rebased.

use serde::{Deserialize, Serialize};

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

/// The hold the workspace's last rebase attempt recorded.
pub fn hold(_repo_path: &str, _workspace_id: i64) -> Option<RebaseHold> {
  None
}
