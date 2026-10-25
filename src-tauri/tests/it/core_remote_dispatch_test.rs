//! Typed remote commands run through `execute_local_request`, the dispatch
//! the CLI, the SSH exec channel and `remote_dispatch_local` all share.

use crate::common::TestRepo;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use treq_lib::core::remote::{execute_local_request, TreqCommandRequest};
use treq_lib::local_db::Workspace;

/// Idempotency keys are cached per machine, so each mutation needs its own.
fn key(tag: &str) -> String {
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  let nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  format!(
    "{tag}-{nanos}-{}-{}",
    std::process::id(),
    COUNTER.fetch_add(1, Ordering::Relaxed)
  )
}

fn dispatch(request: TreqCommandRequest) -> Value {
  execute_local_request(request).expect("dispatch should succeed")
}

fn workspace_with_file(repo: &TestRepo, branch: &str, file: &str) -> (Workspace, String) {
  let ws = repo.create_workspace_simple(branch).unwrap();
  let path = repo.workspace_full_path(&ws);
  TestRepo::write_workspace_file(&path, file, &format!("{file}\n")).unwrap();
  (ws, path)
}

fn workspace_with_commit(repo: &TestRepo, branch: &str, message: &str) -> (Workspace, String) {
  let (ws, path) = workspace_with_file(repo, branch, "a.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, message).unwrap();
  (ws, path)
}

fn descriptions(repo: &TestRepo, id: i64) -> Vec<String> {
  let commits = dispatch(TreqCommandRequest::ListCommits {
    repo: repo.repo_path.clone(),
    workspace: Some(id.to_string()),
  });
  commits["commits"]
    .as_array()
    .unwrap()
    .iter()
    .map(|c| {
      c["description"]
        .as_str()
        .unwrap_or("")
        .trim_end()
        .to_string()
    })
    .collect()
}

/// The commit Git's `refs/heads/{branch}` points to, if the ref exists.
fn git_branch(repo: &TestRepo, branch: &str) -> Option<String> {
  let git = gix::open(&repo.repo_path).unwrap();
  git
    .try_find_reference(format!("refs/heads/{branch}").as_str())
    .unwrap()
    .map(|mut r| r.peel_to_id().unwrap().to_string())
}

fn list_changes(repo: &TestRepo, id: i64) -> Vec<Value> {
  dispatch(TreqCommandRequest::ListChanges {
    repo: repo.repo_path.clone(),
    workspace: Some(id.to_string()),
  })
  .as_array()
  .unwrap()
  .clone()
}

#[test]
fn undo_operation_restores_an_abandoned_commit() {
  let repo = TestRepo::new().unwrap();
  let (ws, path) = workspace_with_commit(&repo, "feat/undo", "keep this commit");
  let change_id = TestRepo::jj_change_id(&path, "@-").unwrap();

  let operation_id = dispatch(TreqCommandRequest::AbandonCommit {
    repo: repo.repo_path.clone(),
    workspace: ws.id.to_string(),
    commit: change_id,
    idempotency_key: key("abandon"),
  });
  assert!(!descriptions(&repo, ws.id).contains(&"keep this commit".to_string()));

  dispatch(TreqCommandRequest::UndoOperation {
    repo: repo.repo_path.clone(),
    workspace: Some(ws.id.to_string()),
    operation_id: operation_id.as_str().unwrap().to_string(),
  });
  assert!(descriptions(&repo, ws.id).contains(&"keep this commit".to_string()));
}

#[test]
fn undo_operation_moves_the_git_branch_back_to_the_restored_commit() {
  let repo = TestRepo::new().unwrap();
  let (ws, path) = workspace_with_commit(&repo, "feat/undo-git", "keep this commit");
  let change_id = TestRepo::jj_change_id(&path, "@-").unwrap();
  let committed = git_branch(&repo, "feat/undo-git");
  assert!(committed.is_some());

  let operation_id = dispatch(TreqCommandRequest::AbandonCommit {
    repo: repo.repo_path.clone(),
    workspace: ws.id.to_string(),
    commit: change_id,
    idempotency_key: key("abandon"),
  });
  assert_ne!(git_branch(&repo, "feat/undo-git"), committed);
  dispatch(TreqCommandRequest::UndoOperation {
    repo: repo.repo_path.clone(),
    workspace: Some(ws.id.to_string()),
    operation_id: operation_id.as_str().unwrap().to_string(),
  });

  // A branch ref left at the abandon's commit would abandon the restored
  // commit again at the next import of Git's refs.
  assert_eq!(git_branch(&repo, "feat/undo-git"), committed);
}

#[test]
fn undo_commit_returns_the_tip_commits_changes_to_the_working_copy() {
  let repo = TestRepo::new().unwrap();
  let (ws, path) = workspace_with_commit(&repo, "feat/undo-commit", "add a");

  dispatch(TreqCommandRequest::UndoCommit {
    repo: repo.repo_path.clone(),
    workspace: ws.id.to_string(),
    commit: TestRepo::jj_change_id(&path, "@-").unwrap(),
  });

  assert!(!descriptions(&repo, ws.id).contains(&"add a".to_string()));
  assert_eq!(
    std::fs::read_to_string(Path::new(&path).join("a.txt")).unwrap(),
    "a.txt\n"
  );
  let changes = list_changes(&repo, ws.id);
  assert!(
    changes.iter().any(|c| c["path"] == "a.txt"),
    "a.txt should be an uncommitted change: {changes:?}"
  );
}

#[test]
fn revert_commit_adds_a_reversing_commit() {
  let repo = TestRepo::new().unwrap();
  let (ws, path) = workspace_with_commit(&repo, "feat/revert", "add a");
  let before = descriptions(&repo, ws.id).len();

  dispatch(TreqCommandRequest::RevertCommit {
    repo: repo.repo_path.clone(),
    workspace: ws.id.to_string(),
    commit: TestRepo::jj_change_id(&path, "@-").unwrap(),
    idempotency_key: key("revert"),
  });

  assert_eq!(descriptions(&repo, ws.id).len(), before + 1);
}

#[test]
fn pull_workspace_pulls_a_pushed_branch() {
  let repo = TestRepo::with_remote().unwrap();
  let ws = repo
    .setup_workspace_with_pushed_commit("feat/pull-me", "pulled.txt", "content\n")
    .unwrap();

  let result = dispatch(TreqCommandRequest::PullWorkspace {
    repo: repo.repo_path,
    workspace: Some(ws.id.to_string()),
  });

  assert_eq!(result["success"], true, "{result}");
  assert_eq!(result["has_conflicts"], false, "{result}");
}

#[test]
fn stash_workspace_changes_moves_them_into_the_stash_list() {
  let repo = TestRepo::new().unwrap();
  let (ws, _) = workspace_with_file(&repo, "feat/stash-me", "wip.txt");

  let entry = dispatch(TreqCommandRequest::StashWorkspaceChanges {
    repo: repo.repo_path.clone(),
    workspace: Some(ws.id.to_string()),
    idempotency_key: key("stash"),
  });

  let stashes: Vec<i64> = treq_lib::core::list_stashes(&repo.repo_path)
    .unwrap()
    .iter()
    .map(|s| s.id)
    .collect();
  assert_eq!(stashes, vec![entry["id"].as_i64().unwrap()]);
  assert!(list_changes(&repo, ws.id).is_empty());
}

#[test]
fn restore_all_discards_changes_and_restore_snapshot_brings_them_back() {
  let repo = TestRepo::new().unwrap();
  let (ws, path) = workspace_with_file(&repo, "feat/discard", "draft.txt");
  let workspace = Some(ws.id.to_string());
  assert_eq!(list_changes(&repo, ws.id).len(), 1);

  let snapshot_id = dispatch(TreqCommandRequest::SnapshotWorkingCopy {
    repo: repo.repo_path.clone(),
    workspace: workspace.clone(),
  });
  dispatch(TreqCommandRequest::RestoreAll {
    repo: repo.repo_path.clone(),
    workspace: workspace.clone(),
  });
  assert!(!Path::new(&path).join("draft.txt").exists());
  assert!(list_changes(&repo, ws.id).is_empty());

  dispatch(TreqCommandRequest::RestoreSnapshot {
    repo: repo.repo_path,
    workspace,
    snapshot_id: snapshot_id.as_str().unwrap().to_string(),
  });
  assert_eq!(
    std::fs::read_to_string(Path::new(&path).join("draft.txt")).unwrap(),
    "draft.txt\n"
  );
}

#[test]
fn archive_workspace_removes_it_from_the_list_and_disk() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("feat/archive-me").unwrap();
  let path = repo.workspace_full_path(&ws);

  dispatch(TreqCommandRequest::ArchiveWorkspace {
    repo: repo.repo_path.clone(),
    workspace: ws.id.to_string(),
  });

  let listed = dispatch(TreqCommandRequest::ListWorkspaces {
    repo: repo.repo_path,
  });
  assert!(
    !listed
      .as_array()
      .unwrap()
      .iter()
      .filter_map(|w| w["id"].as_i64())
      .any(|x| x == ws.id),
    "{listed}"
  );
  assert!(!Path::new(&path).exists());
}

#[test]
fn switch_repo_branch_moves_the_home_working_copy() {
  let repo = TestRepo::new().unwrap();
  workspace_with_commit(&repo, "feat/switch-to", "branch commit");

  dispatch(TreqCommandRequest::SwitchRepoBranch {
    repo: repo.repo_path.clone(),
    bookmark: "feat/switch-to".into(),
  });

  let inspected = dispatch(TreqCommandRequest::InspectRepository {
    repo: repo.repo_path,
  });
  assert_eq!(inspected["current_branch"], "feat/switch-to");
}

#[test]
fn git_remote_info_reads_the_github_origin() {
  let repo = TestRepo::new().unwrap();
  TestRepo::run_git(
    &repo.repo_path,
    &["remote", "add", "origin", "git@github.com:acme/widgets.git"],
  )
  .unwrap();

  let info = dispatch(TreqCommandRequest::GitRemoteInfo {
    repo: repo.repo_path,
  });

  assert_eq!(
    info,
    serde_json::json!({ "owner": "acme", "repo": "widgets", "full_name": "acme/widgets" })
  );
}

#[test]
fn probe_init_inspect_and_clone_through_typed_commands() {
  let dir = tempfile::TempDir::new().unwrap();
  let source = dir.path().join("source");
  let empty = dir.path().join("empty");
  TestRepo::ensure_dir(&source).unwrap();
  TestRepo::ensure_dir(&empty).unwrap();
  let path = |p: &Path| p.to_str().unwrap().to_string();

  let missing = dispatch(TreqCommandRequest::ProbeRepo { repo: path(&empty) });
  assert_eq!(missing["is_repo"], false);
  assert_eq!(missing["needs_clone"], true);

  let initialized = dispatch(TreqCommandRequest::InitRepo {
    repo: path(&source),
    idempotency_key: key("init"),
  });
  assert!(!initialized["repository_type"].as_str().unwrap().is_empty());

  let inspected = dispatch(TreqCommandRequest::InspectRepository {
    repo: path(&source),
  });
  assert!(inspected["root"].as_str().unwrap().contains("source"));

  let destination = dir.path().join("clone");
  let cloned = dispatch(TreqCommandRequest::CloneRepo {
    repo_url: path(&source),
    destination: path(&destination),
    idempotency_key: key("clone"),
  });
  assert!(cloned["root"].as_str().unwrap().contains("clone"));

  let probe = dispatch(TreqCommandRequest::ProbeRepo {
    repo: path(&destination),
  });
  assert_eq!(probe["is_repo"], true);
}

#[test]
fn diff_files_reads_the_workspace_diffs_trees() {
  let repo = TestRepo::new().unwrap();
  let (ws, path) = workspace_with_file(&repo, "feat/diff-files", "a.txt");
  let diff = dispatch(TreqCommandRequest::WorkspaceDiff {
    repo: repo.repo_path.clone(),
    workspace: ws.id.to_string(),
  });
  let token = diff["working_copy_token"].as_str().unwrap().to_string();
  let single = dispatch(TreqCommandRequest::DiffFile {
    repo: repo.repo_path.clone(),
    workspace: Some(ws.id.to_string()),
    path: "a.txt".into(),
  });
  // Edited after the diff: the batch still reads the diff's state.
  TestRepo::write_workspace_file(&path, "a.txt", "a.txt\nmore\n").unwrap();

  let batch = dispatch(TreqCommandRequest::DiffFiles {
    repo: repo.repo_path,
    workspace: Some(ws.id.to_string()),
    paths: vec!["a.txt".into()],
    token: Some(token.clone()),
  });

  assert_eq!(batch["snapshotToken"], token.as_str());
  assert_eq!(batch["files"][0]["path"], "a.txt");
  assert_eq!(batch["files"][0]["hunks"], single);
}
