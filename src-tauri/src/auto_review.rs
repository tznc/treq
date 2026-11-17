//! Automatic AI review triggers.
//!
//! The repository setting `auto_review_trigger` names one jj operation that
//! should start a review on its own: a commit, a rebase, or a pull from the
//! remote. The operations themselves live in `core::workspaces`; each one
//! calls [`notify`] once it has finished, and this module decides whether the
//! repository asked for a review of that operation.
//!
//! Nothing is reviewed here. When a review is due, the registered emitter
//! hands an [`AutoReviewEvent`] to the frontend, which launches the same
//! observable agent terminal the manual Start Review button launches.

use crate::result_ext::ResultExt;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::lock_ext::LockExt;

/// A jj operation that can start a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewTrigger {
  Commit,
  Rebase,
  Pull,
}

impl ReviewTrigger {
  /// Wire value shared with the stored setting and the frontend payload.
  pub fn as_str(self) -> &'static str {
    match self {
      ReviewTrigger::Commit => "on-commit",
      ReviewTrigger::Rebase => "on-rebase",
      ReviewTrigger::Pull => "on-pull",
    }
  }
}

/// Parsed value of the per-repo `auto_review_trigger` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoReviewSetting {
  Off,
  On(ReviewTrigger),
}

/// Parses a stored `auto_review_trigger` value.
///
/// `"on-push"` is the name this setting shipped with in the commit that added
/// it, before anything fired it. It meant "after remote changes arrive", so it
/// reads as `on-pull`. Unknown values are treated as off.
pub fn parse_setting(value: Option<&str>) -> AutoReviewSetting {
  match value.map(str::trim) {
    Some("on-commit") => AutoReviewSetting::On(ReviewTrigger::Commit),
    Some("on-rebase") => AutoReviewSetting::On(ReviewTrigger::Rebase),
    Some("on-pull") | Some("on-push") => AutoReviewSetting::On(ReviewTrigger::Pull),
    _ => AutoReviewSetting::Off,
  }
}

/// Whether a stored setting asks for a review of `trigger`.
pub fn setting_fires_for(setting: AutoReviewSetting, trigger: ReviewTrigger) -> bool {
  matches!(setting, AutoReviewSetting::On(wanted) if wanted == trigger)
}

/// Sent to the frontend when a review should start. Carries everything the
/// review prompt needs, so the frontend never has to re-derive which operation
/// it is reacting to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoReviewEvent {
  pub repo_path: String,
  pub workspace_id: i64,
  pub branch_name: String,
  /// The matched trigger: `"on-commit"`, `"on-rebase"` or `"on-pull"`.
  pub trigger: String,
  /// jj operation id the workspace landed on. Also the de-duplication key.
  pub operation_id: String,
}

type Emitter = Box<dyn Fn(AutoReviewEvent) + Send + Sync>;

static EMITTER: OnceLock<Emitter> = OnceLock::new();
static LAST_FIRED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn last_fired() -> &'static Mutex<HashMap<String, String>> {
  LAST_FIRED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registers the sink that delivers events to the frontend. Called once during
/// Tauri setup; the CLI and tests leave it unset, which makes [`notify`] a
/// no-op after its checks.
pub fn set_emitter(emitter: Emitter) {
  EMITTER.set(emitter).best_effort();
}

/// Key a workspace's last fired operation is stored under.
fn dedupe_key(repo_path: &str, workspace_id: i64) -> String {
  format!("{repo_path}#{workspace_id}")
}

/// Records `operation_id` as the last one reviewed for this workspace and says
/// whether it is new. One operation only ever starts one review, so a repeated
/// call for an operation that already fired (a retried command, a poll tick
/// that found no new work) is dropped.
pub fn claim_operation(repo_path: &str, workspace_id: i64, operation_id: &str) -> bool {
  if operation_id.is_empty() {
    return false;
  }
  let key = dedupe_key(repo_path, workspace_id);
  let mut guard = last_fired().lock_or_recover();
  if guard.get(&key).map(String::as_str) == Some(operation_id) {
    return false;
  }
  guard.insert(key, operation_id.to_string());
  true
}

/// Reads the repository's `auto_review_trigger` setting. Defaults to off when
/// the setting, the database, or the repository itself is unavailable.
fn resolve_setting(repo_path: &str) -> AutoReviewSetting {
  let app_db_path = crate::core::resolve_app_db_path(repo_path);
  if !app_db_path.exists() {
    return AutoReviewSetting::Off;
  }
  let Ok(db) = crate::db::Database::new(app_db_path) else {
    return AutoReviewSetting::Off;
  };
  let stored = db
    .get_repo_setting(repo_path, "auto_review_trigger")
    .ok()
    .flatten();
  parse_setting(stored.as_deref())
}

/// Called by a jj operation once it has finished. Emits an [`AutoReviewEvent`]
/// only when this repository asked for reviews of this operation and the
/// workspace's operation id has not already started one.
///
/// Best-effort throughout: a missing setting, an unreadable workspace, or a
/// frontend that is not listening never fails the operation that ran.
pub fn notify(
  repo_path: &str,
  workspace_id: i64,
  branch_name: &str,
  workspace_path: &str,
  trigger: ReviewTrigger,
) {
  if !setting_fires_for(resolve_setting(repo_path), trigger) {
    return;
  }
  let Some(emitter) = EMITTER.get() else {
    return;
  };
  let Ok(operation_id) = crate::jj::jj_head_operation_id(workspace_path) else {
    return;
  };
  if !claim_operation(repo_path, workspace_id, &operation_id) {
    return;
  }
  emitter(AutoReviewEvent {
    repo_path: repo_path.to_string(),
    workspace_id,
    branch_name: branch_name.to_string(),
    trigger: trigger.as_str().to_string(),
    operation_id,
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_every_trigger_value() {
    assert_eq!(
      parse_setting(Some("on-commit")),
      AutoReviewSetting::On(ReviewTrigger::Commit)
    );
    assert_eq!(
      parse_setting(Some("on-rebase")),
      AutoReviewSetting::On(ReviewTrigger::Rebase)
    );
    assert_eq!(
      parse_setting(Some("on-pull")),
      AutoReviewSetting::On(ReviewTrigger::Pull)
    );
  }

  #[test]
  fn treats_legacy_on_push_as_on_pull() {
    assert_eq!(
      parse_setting(Some("on-push")),
      AutoReviewSetting::On(ReviewTrigger::Pull)
    );
  }

  #[test]
  fn unset_and_unknown_values_are_off() {
    assert_eq!(parse_setting(None), AutoReviewSetting::Off);
    assert_eq!(parse_setting(Some("")), AutoReviewSetting::Off);
    assert_eq!(parse_setting(Some("off")), AutoReviewSetting::Off);
    assert_eq!(parse_setting(Some("weekly")), AutoReviewSetting::Off);
  }

  #[test]
  fn a_setting_fires_only_for_its_own_trigger() {
    let setting = parse_setting(Some("on-commit"));
    assert!(setting_fires_for(setting, ReviewTrigger::Commit));
    assert!(!setting_fires_for(setting, ReviewTrigger::Rebase));
    assert!(!setting_fires_for(setting, ReviewTrigger::Pull));
    assert!(!setting_fires_for(
      AutoReviewSetting::Off,
      ReviewTrigger::Commit
    ));
  }

  #[test]
  fn an_operation_fires_once_per_workspace() {
    assert!(claim_operation("/repo/once", 1, "op-a"));
    assert!(!claim_operation("/repo/once", 1, "op-a"));
    assert!(claim_operation("/repo/once", 1, "op-b"));
    assert!(claim_operation("/repo/once", 1, "op-a"));
  }

  #[test]
  fn workspaces_and_repos_dedupe_independently() {
    assert!(claim_operation("/repo/split", 1, "shared-op"));
    assert!(claim_operation("/repo/split", 2, "shared-op"));
    assert!(claim_operation("/other/split", 1, "shared-op"));
  }

  #[test]
  fn an_empty_operation_id_never_fires() {
    assert!(!claim_operation("/repo/empty", 1, ""));
  }

  #[test]
  fn trigger_wire_values_match_the_setting_values() {
    assert_eq!(ReviewTrigger::Commit.as_str(), "on-commit");
    assert_eq!(ReviewTrigger::Rebase.as_str(), "on-rebase");
    assert_eq!(ReviewTrigger::Pull.as_str(), "on-pull");
  }
}
