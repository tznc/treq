//! Keeps a workspace opened from a pull request in step with the PR: its title
//! and body are copied in on every PR poll until someone edits them locally.
//!
//! `workspace_pr_sync` holds the values last copied from the PR. A field keeps
//! syncing while the workspace's value still equals that copy. An edit from
//! the app, an agent's `treq set -d` or the CLI makes them differ, and that
//! field stops syncing until [`resync`] copies the PR in again.

use crate::github::PrInfo;
use crate::local_db::{self, PrSyncRecord, Workspace};
use std::collections::HashMap;

/// The description workspaces opened from a PR got before the body was copied.
pub fn legacy_placeholder(number: u64) -> String {
  format!("From GitHub PR #{number}")
}

/// The PR body as a workspace description: GitHub's CRLFs folded, ends trimmed.
pub fn pr_description(pr: &PrInfo) -> String {
  pr.body.replace("\r\n", "\n").trim().to_string()
}

/// What one poll changes for one workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
  /// The title to write, when it syncs and the PR's differs.
  pub title: Option<String>,
  /// The description to write, when it syncs and the PR body differs.
  pub description: Option<String>,
  pub record: PrSyncRecord,
}

/// Which fields of a workspace follow its PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncState {
  pub pr_number: u64,
  pub title: bool,
  pub description: bool,
}

fn current_description(workspace: &Workspace) -> &str {
  workspace.description.as_deref().unwrap_or("")
}

/// The record a workspace syncs against: its own, or for a workspace made
/// before syncing existed, one inferred from the untouched placeholder.
fn baseline(
  workspace: &Workspace,
  record: Option<&PrSyncRecord>,
  pr: &PrInfo,
) -> Option<PrSyncRecord> {
  match record {
    Some(record) => (record.pr_number == pr.number).then(|| record.clone()),
    None if current_description(workspace) == legacy_placeholder(pr.number) => Some(PrSyncRecord {
      pr_number: pr.number,
      title: (workspace.title == pr.title).then(|| pr.title.clone()),
      description: Some(legacy_placeholder(pr.number)),
    }),
    None => None,
  }
}

/// The changes `pr` makes to `workspace`, or `None` when there are none.
pub fn plan(workspace: &Workspace, record: Option<&PrSyncRecord>, pr: &PrInfo) -> Option<SyncPlan> {
  let base = baseline(workspace, record, pr)?;
  let body = pr_description(pr);
  let synced = state(workspace, &base);
  let title = (synced.title && workspace.title != pr.title).then(|| pr.title.clone());
  let description = (synced.description && current_description(workspace) != body).then_some(body);
  let next = PrSyncRecord {
    pr_number: base.pr_number,
    title: if synced.title {
      Some(pr.title.clone())
    } else {
      base.title
    },
    description: match &description {
      Some(body) => Some(body.clone()),
      None => base.description,
    },
  };
  if title.is_none() && description.is_none() && record == Some(&next) {
    return None;
  }
  Some(SyncPlan {
    title,
    description,
    record: next,
  })
}

/// Which fields of `workspace` still follow `record`.
pub fn state(workspace: &Workspace, record: &PrSyncRecord) -> SyncState {
  SyncState {
    pr_number: record.pr_number,
    title: record.title.as_deref() == Some(workspace.title.as_str()),
    description: record.description.as_deref() == Some(current_description(workspace)),
  }
}

fn apply(repo_path: &str, workspace_id: i64, plan: &SyncPlan) -> Result<(), String> {
  if let Some(title) = &plan.title {
    local_db::update_workspace_title(repo_path, workspace_id, title)?;
  }
  if let Some(description) = &plan.description {
    local_db::update_workspace_description(repo_path, workspace_id, description)?;
  }
  local_db::set_workspace_pr_sync(repo_path, workspace_id, &plan.record)
}

/// Copies each PR in `prs` (by head branch) into the workspaces that follow
/// it. Returns the ids of the workspaces whose title or description changed.
pub fn sync_repo(
  repo_path: &str,
  prs: &HashMap<String, Option<PrInfo>>,
) -> Result<Vec<i64>, String> {
  if prs.values().all(Option::is_none) {
    return Ok(Vec::new());
  }
  let records = local_db::list_workspace_pr_syncs(repo_path)?;
  let mut changed = Vec::new();
  for workspace in local_db::get_workspaces(repo_path)? {
    if workspace.archived {
      continue;
    }
    let Some(Some(pr)) = prs.get(&workspace.branch_name) else {
      continue;
    };
    let Some(plan) = plan(&workspace, records.get(&workspace.id), pr) else {
      continue;
    };
    apply(repo_path, workspace.id, &plan)?;
    if plan.title.is_some() || plan.description.is_some() {
      changed.push(workspace.id);
    }
  }
  Ok(changed)
}

/// Starts syncing a workspace just opened from PR `pr_number`, from the title
/// and description it was made with.
pub fn record_created(
  repo_path: &str,
  workspace: &Workspace,
  pr_number: u64,
) -> Result<(), String> {
  let record = PrSyncRecord {
    pr_number,
    title: Some(workspace.title.clone()),
    description: Some(current_description(workspace).to_string()),
  };
  local_db::set_workspace_pr_sync(repo_path, workspace.id, &record)
}

/// The sync state of a workspace, or `None` when it was not opened from a PR.
pub fn sync_state(repo_path: &str, workspace: &Workspace) -> Option<SyncState> {
  let record = local_db::get_workspace_pr_sync(repo_path, workspace.id)
    .map_err(|err| log::warn!("pr-sync: reading workspace {}: {err}", workspace.id))
    .ok()??;
  Some(state(workspace, &record))
}

/// Copies `pr`'s title and body over local edits and syncs both again.
pub fn resync(repo_path: &str, workspace_id: i64, pr: &PrInfo) -> Result<(), String> {
  let body = pr_description(pr);
  apply(
    repo_path,
    workspace_id,
    &SyncPlan {
      title: Some(pr.title.clone()),
      description: Some(body.clone()),
      record: PrSyncRecord {
        pr_number: pr.number,
        title: Some(pr.title.clone()),
        description: Some(body),
      },
    },
  )
}

/// [`resync`] with the PR the poller last read for the workspace's branch.
pub fn resync_workspace(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| "Workspace not found".to_string())?;
  let pr = crate::pr_status::global()
    .list_cached(repo_path)
    .remove(&workspace.branch_name)
    .flatten()
    .ok_or_else(|| "The pull request hasn't loaded yet. Try again in a minute.".to_string())?;
  resync(repo_path, workspace_id, &pr)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn pr(number: u64, title: &str, body: &str) -> PrInfo {
    PrInfo {
      number,
      title: title.into(),
      state: "OPEN".into(),
      url: format!("https://github.com/o/r/pull/{number}"),
      head_ref_name: "feat/x".into(),
      base_ref_name: "main".into(),
      merge_state_status: None,
      is_draft: false,
      body: body.into(),
    }
  }

  fn workspace(title: &str, description: Option<&str>) -> Workspace {
    Workspace {
      id: 1,
      repo_path: "/repo".into(),
      workspace_name: "feat-x".into(),
      workspace_path: "feat-x".into(),
      branch_name: "feat/x".into(),
      created_at: String::new(),
      refreshed_at: None,
      metadata: None,
      target_branch: None,
      title: title.into(),
      description: description.map(str::to_string),
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  fn record(number: u64, title: Option<&str>, description: Option<&str>) -> PrSyncRecord {
    PrSyncRecord {
      pr_number: number,
      title: title.map(str::to_string),
      description: description.map(str::to_string),
    }
  }

  #[test]
  fn plan_copies_changed_body_and_title_into_untouched_workspace() {
    let ws = workspace("Old", Some("old body"));
    let rec = record(7, Some("Old"), Some("old body"));
    let plan = plan(&ws, Some(&rec), &pr(7, "New", "new body")).unwrap();
    assert_eq!(plan.title.as_deref(), Some("New"));
    assert_eq!(plan.description.as_deref(), Some("new body"));
    assert_eq!(plan.record, record(7, Some("New"), Some("new body")));
  }

  #[test]
  fn plan_returns_none_when_workspace_matches_pr() {
    let ws = workspace("T", Some("b"));
    let rec = record(7, Some("T"), Some("b"));
    assert_eq!(plan(&ws, Some(&rec), &pr(7, "T", "b")), None);
  }

  #[test]
  fn plan_skips_locally_edited_description() {
    let ws = workspace("T", Some("my notes"));
    let rec = record(7, Some("T"), Some("old body"));
    let plan = plan(&ws, Some(&rec), &pr(7, "T2", "new body")).unwrap();
    assert_eq!(plan.description, None);
    assert_eq!(plan.title.as_deref(), Some("T2"));
    assert_eq!(plan.record.description.as_deref(), Some("old body"));
  }

  #[test]
  fn plan_skips_locally_edited_title() {
    let ws = workspace("Mine", Some("b"));
    let rec = record(7, Some("T"), Some("b"));
    let plan = plan(&ws, Some(&rec), &pr(7, "T2", "b2")).unwrap();
    assert_eq!(plan.title, None);
    assert_eq!(plan.description.as_deref(), Some("b2"));
  }

  #[test]
  fn plan_fills_empty_description_recorded_at_creation() {
    let ws = workspace("T", None);
    let rec = record(7, Some("T"), Some(""));
    let plan = plan(&ws, Some(&rec), &pr(7, "T", "## Summary\r\nbody\n")).unwrap();
    assert_eq!(plan.description.as_deref(), Some("## Summary\nbody"));
  }

  #[test]
  fn plan_ignores_workspace_without_record_or_placeholder() {
    let ws = workspace("T", Some("hand written"));
    assert_eq!(plan(&ws, None, &pr(7, "T", "body")), None);
  }

  #[test]
  fn plan_ignores_record_for_another_pr() {
    let ws = workspace("T", Some("b"));
    let rec = record(6, Some("T"), Some("b"));
    assert_eq!(plan(&ws, Some(&rec), &pr(7, "T2", "b2")), None);
  }

  #[test]
  fn plan_backfills_legacy_placeholder() {
    let ws = workspace("T", Some("From GitHub PR #7"));
    let plan = plan(&ws, None, &pr(7, "T", "body")).unwrap();
    assert_eq!(plan.description.as_deref(), Some("body"));
    assert_eq!(plan.title, None);
    assert_eq!(plan.record, record(7, Some("T"), Some("body")));
  }

  #[test]
  fn plan_backfill_leaves_edited_title_alone() {
    let ws = workspace("Mine", Some("From GitHub PR #7"));
    let plan = plan(&ws, None, &pr(7, "T", "body")).unwrap();
    assert_eq!(plan.title, None);
    assert_eq!(plan.record.title, None);
  }

  #[test]
  fn plan_backfill_ignores_placeholder_for_another_pr() {
    let ws = workspace("T", Some("From GitHub PR #6"));
    assert_eq!(plan(&ws, None, &pr(7, "T", "body")), None);
  }

  #[test]
  fn state_reports_fields_that_match_record() {
    let ws = workspace("T", Some("edited"));
    let st = state(&ws, &record(7, Some("T"), Some("body")));
    assert_eq!(
      st,
      SyncState {
        pr_number: 7,
        title: true,
        description: false
      }
    );
  }

  fn repo_with_workspace(description: Option<&str>) -> (TempDir, String, i64) {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap().to_string();
    local_db::init_local_db(&repo).unwrap();
    let id = local_db::add_workspace(
      &repo,
      "feat-x".into(),
      "feat-x".into(),
      "feat/x".into(),
      description.map(str::to_string),
      None,
      None,
    )
    .unwrap();
    local_db::update_workspace_title(&repo, id, "T").unwrap();
    (dir, repo, id)
  }

  fn load(repo: &str, id: i64) -> Workspace {
    local_db::get_workspace_by_id(repo, id).unwrap().unwrap()
  }

  #[test]
  fn sync_repo_copies_pr_into_created_workspace_until_edited() {
    let (_dir, repo, id) = repo_with_workspace(None);
    record_created(&repo, &load(&repo, id), 7).unwrap();
    let prs = HashMap::from([("feat/x".to_string(), Some(pr(7, "T", "first")))]);

    assert_eq!(sync_repo(&repo, &prs).unwrap(), vec![id]);
    assert_eq!(load(&repo, id).description.as_deref(), Some("first"));

    local_db::update_workspace_description(&repo, id, "mine").unwrap();
    let prs = HashMap::from([("feat/x".to_string(), Some(pr(7, "T", "second")))]);
    assert_eq!(sync_repo(&repo, &prs).unwrap(), Vec::<i64>::new());
    assert_eq!(load(&repo, id).description.as_deref(), Some("mine"));
  }

  #[test]
  fn sync_repo_backfills_placeholder_workspace() {
    let (_dir, repo, id) = repo_with_workspace(Some("From GitHub PR #7"));
    let prs = HashMap::from([("feat/x".to_string(), Some(pr(7, "T", "body")))]);
    assert_eq!(sync_repo(&repo, &prs).unwrap(), vec![id]);
    assert_eq!(load(&repo, id).description.as_deref(), Some("body"));
    assert_eq!(
      sync_state(&repo, &load(&repo, id)).map(|s| s.description),
      Some(true)
    );
  }

  #[test]
  fn sync_state_is_none_for_workspace_not_from_pr() {
    let (_dir, repo, id) = repo_with_workspace(Some("notes"));
    assert_eq!(sync_state(&repo, &load(&repo, id)), None);
  }

  #[test]
  fn resync_overwrites_local_edits_and_resumes_sync() {
    let (_dir, repo, id) = repo_with_workspace(Some("body"));
    record_created(&repo, &load(&repo, id), 7).unwrap();
    local_db::update_workspace_description(&repo, id, "mine").unwrap();
    local_db::update_workspace_title(&repo, id, "Mine").unwrap();

    resync(&repo, id, &pr(7, "PR title", "PR body")).unwrap();

    let ws = load(&repo, id);
    assert_eq!(ws.title, "PR title");
    assert_eq!(ws.description.as_deref(), Some("PR body"));
    let st = sync_state(&repo, &ws).unwrap();
    assert!(st.title && st.description);
  }
}
