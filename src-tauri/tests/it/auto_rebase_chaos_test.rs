//! Chaos scenarios for auto rebase, each against a real jj repo with an origin.
use crate::common::TestRepo;
use std::time::Instant;
use treq_lib::{jj, local_db};

fn on_default(repo: &TestRepo, branch: &str, file: &str) -> (local_db::Workspace, String) {
  let ws = repo
    .create_workspace_with_commit(branch, file, "work", None)
    .unwrap();
  let target = repo.default_branch().to_string();
  local_db::update_workspace_target_branch(&repo.repo_path, ws.id, &target).unwrap();
  let path = repo.workspace_full_path(&ws);
  (ws, path)
}

fn has(path: &str, rev: &str, file: &str) -> bool {
  TestRepo::jj_files_at_revision(path, rev)
    .unwrap()
    .iter()
    .any(|f| f == file)
}

fn divergent(path: &str) -> String {
  TestRepo::run_jj_cli(
    path,
    &[
      "log",
      "--no-graph",
      "-r",
      "divergent()",
      "-T",
      "change_id ++ \"\\n\"",
    ],
  )
  .unwrap()
}

fn rebase_all(repo: &TestRepo) -> treq_lib::core::SingleRebaseResult {
  treq_lib::core::check_and_rebase_workspaces(&repo.repo_path, None, None, None, "diff").unwrap()
}

#[test]
fn chaos_multiple_workspaces_all_follow_target() {
  let repo = TestRepo::with_remote().unwrap();
  let (_, a) = on_default(&repo, "feat/a", "a.txt");
  let (_, b) = on_default(&repo, "feat/b", "b.txt");
  repo.remote_commit_file("adv.txt", "x", "advance").unwrap();
  rebase_all(&repo);
  assert!(has(&a, "feat/a", "adv.txt"));
  assert!(has(&b, "feat/b", "adv.txt"));
}

#[test]
fn chaos_stacked_child_follows_parent_and_target() {
  let repo = TestRepo::with_remote().unwrap();
  let (p, _) = on_default(&repo, "feat/p", "p.txt");
  let c = repo
    .create_workspace_with_commit("feat/c", "c.txt", "c", Some("feat/p"))
    .unwrap();
  local_db::update_workspace_target_branch(&repo.repo_path, c.id, "feat/p").unwrap();
  let cp = repo.workspace_full_path(&c);
  repo.remote_commit_file("adv.txt", "x", "advance").unwrap();
  rebase_all(&repo);
  rebase_all(&repo);
  assert!(has(&cp, "feat/c", "adv.txt"));
  assert!(has(&cp, "feat/c", "p.txt"));
  let _ = p;
  assert_eq!(divergent(&cp), "");
}

#[test]
fn chaos_repeated_trigger_is_idempotent() {
  let repo = TestRepo::with_remote().unwrap();
  let (_, a) = on_default(&repo, "feat/idem", "a.txt");
  repo.remote_commit_file("adv.txt", "x", "advance").unwrap();
  rebase_all(&repo);
  let first = jj::jj_get_commit_id(&a, "feat/idem").unwrap();
  for _ in 0..5 {
    rebase_all(&repo);
  }
  assert_eq!(jj::jj_get_commit_id(&a, "feat/idem").unwrap(), first);
}

#[test]
fn chaos_missing_workspace_dir_does_not_block_others() {
  let repo = TestRepo::with_remote().unwrap();
  let (_, gone) = on_default(&repo, "feat/gone", "g.txt");
  let (_, ok) = on_default(&repo, "feat/ok", "o.txt");
  std::fs::remove_dir_all(&gone).unwrap();
  repo.remote_commit_file("adv.txt", "x", "advance").unwrap();
  let r = treq_lib::core::check_and_rebase_workspaces(&repo.repo_path, None, None, None, "diff");
  assert!(r.is_ok(), "{r:?}");
  assert!(has(&ok, "feat/ok", "adv.txt"));
}

#[test]
fn chaos_empty_workspace_moves_up() {
  let repo = TestRepo::with_remote().unwrap();
  let ws = repo.create_workspace_simple("feat/empty").unwrap();
  local_db::update_workspace_target_branch(&repo.repo_path, ws.id, repo.default_branch()).unwrap();
  let p = repo.workspace_full_path(&ws);
  repo.remote_commit_file("adv.txt", "x", "advance").unwrap();
  rebase_all(&repo);
  assert!(has(&p, "@", "adv.txt"));
}

#[test]
fn chaos_deleted_target_on_remote_leaves_workspace_untouched() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, a) = on_default(&repo, "feat/orphan", "a.txt");
  // Retarget to a branch that exists on origin, then delete it there.
  local_db::update_workspace_target_branch(&repo.repo_path, ws.id, "feature-remote").unwrap();
  let before = jj::jj_get_commit_id(&a, "feat/orphan").unwrap();
  TestRepo::run_git(
    &repo.remote_path().to_string_lossy(),
    &["update-ref", "-d", "refs/heads/feature-remote"],
  )
  .unwrap();
  let r =
    treq_lib::core::check_and_rebase_workspaces(&repo.repo_path, Some(ws.id), None, None, "diff");
  let _ = r; // error or no-op both fine
  assert_eq!(jj::jj_get_commit_id(&a, "feat/orphan").unwrap(), before);
}

#[test]
fn chaos_renamed_target_returns_error_not_panic() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, _) = on_default(&repo, "feat/renamed", "a.txt");
  local_db::update_workspace_target_branch(&repo.repo_path, ws.id, "no-such-branch").unwrap();
  let single = std::panic::catch_unwind(|| {
    treq_lib::core::check_and_rebase_workspaces(
      &repo.repo_path,
      Some(ws.id),
      None,
      Some(true),
      "diff",
    )
  });
  assert!(single.is_ok());
  let all = treq_lib::core::check_and_rebase_workspaces(&repo.repo_path, None, None, None, "diff");
  assert!(all.is_ok(), "{all:?}");
}

#[test]
fn chaos_huge_stack_rebases_in_reasonable_time() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, a) = on_default(&repo, "feat/huge", "f0.txt");
  for i in 1..40 {
    TestRepo::write_workspace_file(&a, &format!("f{i}.txt"), "x").unwrap();
    treq_lib::core::commit_workspace(&repo.repo_path, ws.id, &format!("c{i}")).unwrap();
  }
  repo.remote_commit_file("adv.txt", "x", "advance").unwrap();
  let t = Instant::now();
  rebase_all(&repo);
  let dt = t.elapsed();
  assert!(has(&a, "feat/huge", "adv.txt"));
  assert!(has(&a, "feat/huge", "f39.txt"));
  assert!(dt.as_secs() < 20, "huge stack rebase took {dt:?}");
}

#[test]
fn chaos_concurrent_batch_and_ensure_do_not_diverge() {
  let repo = TestRepo::with_remote().unwrap();
  let mut ids = Vec::new();
  for i in 0..4 {
    let (ws, _) = on_default(&repo, &format!("feat/cc{i}"), &format!("cc{i}.txt"));
    ids.push(ws.id);
  }
  repo.remote_commit_file("adv.txt", "x", "advance").unwrap();
  let rp = repo.repo_path.clone();
  std::thread::scope(|s| {
    s.spawn(|| {
      let _ = treq_lib::core::check_and_rebase_workspaces(&rp, None, None, None, "diff");
    });
    for id in &ids {
      let rp = rp.clone();
      let id = *id;
      s.spawn(move || {
        let _ = treq_lib::core::app::ensure_workspace_rebased(&rp, id, "diff");
      });
    }
    for id in &ids {
      let rp = rp.clone();
      let id = *id;
      s.spawn(move || {
        let _ =
          treq_lib::core::check_and_rebase_workspaces(&rp, Some(id), None, Some(true), "diff");
      });
    }
  });
  assert_eq!(
    divergent(&repo.repo_path),
    "",
    "divergent changes after concurrent rebases"
  );
  for i in 0..4 {
    assert!(
      !jj::jj_is_bookmark_conflicted(&repo.repo_path, &format!("feat/cc{i}")),
      "feat/cc{i} conflicted"
    );
  }
}
