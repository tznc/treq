//! Every history edit that moves the colocated home repo's `@` must leave Git
//! HEAD on `@-`, attached to the checked-out branch when that branch still
//! points there, with the index matching it.
//!
//! Git HEAD is the source of truth for the home repo: the next read imports it
//! and, when it differs from `@-`, checks `@` out onto it without touching
//! disk. A HEAD left behind by a jj-side rewrite therefore turns every file the
//! rewrite changed into a phantom working-copy change.
mod e2e_test_helpers;

use e2e_test_helpers::TestRepo;
use treq_lib::core::{self, MergeCommit, HOME_MOVE_ENDPOINT};
use treq_lib::jj;

fn git(repo: &TestRepo, args: &[&str]) -> String {
  TestRepo::run_git(&repo.repo_path, args)
    .unwrap_or_else(|e| panic!("git {args:?}: {e}"))
    .trim()
    .to_string()
}

/// Home repo checked out on `feature` (commits f1, f2) while `main` has moved
/// ahead with files `feature` does not have.
fn home_on_feature() -> (TestRepo, String) {
  let repo = TestRepo::new().expect("create repo");
  let main = repo.default_branch().to_string();
  git(&repo, &["add", ".gitignore"]);
  git(&repo, &["commit", "-m", "Commit init metadata"]);
  git(&repo, &["checkout", "-b", "feature"]);
  repo.commit_file("f1.txt", "f1\n", "f1").expect("commit f1");
  repo.commit_file("f2.txt", "f2\n", "f2").expect("commit f2");
  git(&repo, &["checkout", &main]);
  for i in 0..5 {
    TestRepo::write_workspace_file(&repo.repo_path, &format!("target_{i}.txt"), "t\n")
      .expect("write target file");
  }
  git(&repo, &["add", "."]);
  git(&repo, &["commit", "-m", "advance target"]);
  git(&repo, &["checkout", "feature"]);
  assert!(jj::jj_get_changed_files(&repo.repo_path)
    .expect("initial snapshot")
    .is_empty());
  (repo, main)
}

fn change_id(repo: &TestRepo, revision: &str) -> String {
  jj::jj_get_change_id(&repo.repo_path, revision).expect("resolve change id")
}

/// Asserts Git HEAD follows `@-` and returns the working-copy changes that
/// remain once the home repo has been re-read.
fn assert_git_head_follows_wc_parent(repo: &TestRepo, branch: Option<&str>) -> Vec<String> {
  let wc_parent = jj::jj_get_commit_id(&repo.repo_path, "@-").expect("resolve @-");
  let head = git(repo, &["rev-parse", "HEAD"]);
  assert!(
    head.starts_with(&wc_parent),
    "git HEAD {head} should be @- {wc_parent}"
  );
  if let Some(branch) = branch {
    assert_eq!(
      TestRepo::run_git(&repo.repo_path, &["symbolic-ref", "-q", "HEAD"])
        .unwrap_or_default()
        .trim(),
      format!("refs/heads/{branch}"),
      "git HEAD should stay attached to {branch}"
    );
  }
  assert_eq!(
    git(repo, &["diff", "--cached", "--name-only"]),
    "",
    "git index should match HEAD"
  );

  let mut changed: Vec<String> = jj::jj_get_changed_files(&repo.repo_path)
    .expect("list changes")
    .into_iter()
    .map(|change| change.path)
    .collect();
  changed.sort();
  assert_eq!(
    jj::jj_get_commit_id(&repo.repo_path, "@-").expect("resolve @- again"),
    wc_parent,
    "re-reading the home repo must not move @"
  );
  changed
}

#[test]
fn rebase_home_branch() {
  let (repo, main) = home_on_feature();
  jj::jj_rebase_home_repo_branch(&repo.repo_path, "feature", &main).expect("rebase");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn commit() {
  let (repo, _) = home_on_feature();
  TestRepo::write_workspace_file(&repo.repo_path, "c.txt", "c\n").expect("write");
  core::commit_repo(&repo.repo_path, "home commit").expect("commit");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

/// A commit made before anything has read the home repo: jj has not imported
/// the git branch yet, so the commit must import it before moving it.
#[test]
fn commit_before_first_read() {
  let repo = TestRepo::new().expect("create repo");
  let main = repo.default_branch().to_string();
  for i in 0..2 {
    TestRepo::write_workspace_file(&repo.repo_path, &format!("c{i}.txt"), "c\n").expect("write");
    core::commit_repo(&repo.repo_path, &format!("commit {i}")).expect("commit");
    assert_eq!(
      git(&repo, &["log", "-1", "--format=%s", &main]),
      format!("commit {i}"),
      "the git branch should advance with the commit"
    );
    assert!(assert_git_head_follows_wc_parent(&repo, Some(&main)).is_empty());
  }
}

#[test]
fn split() {
  let (repo, _) = home_on_feature();
  TestRepo::write_workspace_file(&repo.repo_path, "c.txt", "c\n").expect("write");
  TestRepo::write_workspace_file(&repo.repo_path, "d.txt", "d\n").expect("write");
  jj::jj_get_changed_files(&repo.repo_path).expect("snapshot");
  jj::jj_split(&repo.repo_path, "split", vec!["c.txt".to_string()]).expect("split");
  assert_eq!(
    assert_git_head_follows_wc_parent(&repo, Some("feature")),
    vec!["d.txt"]
  );
}

#[test]
fn switch_branch() {
  let (repo, main) = home_on_feature();
  core::switch_repo_branch(&repo.repo_path, &main).expect("switch");
  assert!(assert_git_head_follows_wc_parent(&repo, Some(&main)).is_empty());
}

#[test]
fn describe_tip() {
  let (repo, _) = home_on_feature();
  let tip = change_id(&repo, "@-");
  jj::jj_describe(&repo.repo_path, &tip, "renamed tip").expect("describe");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn describe_ancestor() {
  let (repo, _) = home_on_feature();
  let f1 = change_id(&repo, "@--");
  jj::jj_describe(&repo.repo_path, &f1, "renamed f1").expect("describe");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn abandon_tip() {
  let (repo, _) = home_on_feature();
  let tip = change_id(&repo, "@-");
  jj::jj_abandon(&repo.repo_path, &tip).expect("abandon");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn undo_commit() {
  let (repo, main) = home_on_feature();
  let tip = change_id(&repo, "@-");
  jj::jj_undo_commit(&repo.repo_path, &main, &tip).expect("undo commit");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn revert_commit() {
  let (repo, _) = home_on_feature();
  let f1 = change_id(&repo, "@--");
  jj::jj_revert_commit(&repo.repo_path, &f1).expect("revert");
  assert!(assert_git_head_follows_wc_parent(&repo, None).is_empty());
}

#[test]
fn move_commit_to_workspace() {
  let (repo, _) = home_on_feature();
  let ws = repo.create_workspace_simple("ws").expect("ws");
  let tip = change_id(&repo, "@-");
  let target = repo.workspace_full_path(&ws);
  jj::squash_commit_to_workspace(&repo.repo_path, &tip, &target).expect("move");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn stash_commit() {
  let (repo, _) = home_on_feature();
  let tip = change_id(&repo, "@-");
  core::stash_commit(&repo.repo_path, None, &tip).expect("stash commit");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn stash_and_apply_working_copy() {
  let (repo, _) = home_on_feature();
  TestRepo::write_workspace_file(&repo.repo_path, "s.txt", "s\n").expect("write");
  let stash = core::stash_workspace_changes(&repo.repo_path, None).expect("stash");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());

  core::apply_stash(&repo.repo_path, stash.id, HOME_MOVE_ENDPOINT).expect("apply stash");
  assert_eq!(
    assert_git_head_follows_wc_parent(&repo, Some("feature")),
    vec!["s.txt"]
  );
}

#[test]
fn restore_discarded_changes() {
  let (repo, _) = home_on_feature();
  TestRepo::write_workspace_file(&repo.repo_path, "r.txt", "r\n").expect("write");
  jj::jj_get_changed_files(&repo.repo_path).expect("snapshot disk");
  let snapshot = core::snapshot_working_copy(&repo.repo_path).expect("snapshot");
  jj::jj_restore_all(&repo.repo_path).expect("discard");
  core::restore_working_copy_snapshot(&repo.repo_path, &snapshot).expect("restore");
  assert_eq!(
    assert_git_head_follows_wc_parent(&repo, Some("feature")),
    vec!["r.txt"]
  );
}

#[test]
fn undo_operation() {
  let (repo, _) = home_on_feature();
  let tip = change_id(&repo, "@-");
  jj::jj_describe(&repo.repo_path, &tip, "renamed tip").expect("describe");
  let op = jj::jj_head_operation_id(&repo.repo_path).expect("op id");
  core::undo_repo_operation(&repo.repo_path, None, &op).expect("undo");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn shift_lineage_timestamps() {
  let (repo, main) = home_on_feature();
  jj::jj_shift_mutable_lineage_to_now(&repo.repo_path, &main).expect("shift");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn resolve_bookmark_conflict_onto_target() {
  let (repo, main) = home_on_feature();
  jj::jj_resolve_bookmark_conflict_losslessly(&repo.repo_path, "feature", &main).expect("resolve");
  assert!(assert_git_head_follows_wc_parent(&repo, None).is_empty());
}

#[test]
fn new_on_other_branch() {
  let (repo, main) = home_on_feature();
  jj::jj_new_with_parents(&repo.repo_path, std::slice::from_ref(&main)).expect("new");
  assert!(assert_git_head_follows_wc_parent(&repo, Some(&main)).is_empty());
}

#[test]
fn sync_working_copy_to_moved_bookmark() {
  let (repo, main) = home_on_feature();
  jj::jj_set_bookmark(&repo.repo_path, "feature", &main).expect("move bookmark");
  assert!(jj::jj_sync_working_copy_if_safe(&repo.repo_path, "feature").expect("sync"));
  // The sync edits the bookmark commit as `@`; the next read starts a fresh
  // `@` on top of it, after which HEAD must match `@-` as usual.
  assert!(jj::jj_get_changed_files(&repo.repo_path)
    .expect("re-read home")
    .is_empty());
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
  assert_eq!(
    git(&repo, &["rev-parse", "HEAD"]),
    git(&repo, &["rev-parse", &main])
  );
}

#[test]
fn commit_drops_autosave_ancestors() {
  let (repo, _) = home_on_feature();
  TestRepo::write_workspace_file(&repo.repo_path, "a.txt", "a\n").expect("write");
  core::commit_repo(
    &repo.repo_path,
    &format!("{} a.txt", jj::AUTOSAVE_DESCRIPTION_PREFIX),
  )
  .expect("autosave commit");
  TestRepo::write_workspace_file(&repo.repo_path, "m.txt", "m\n").expect("write");
  core::commit_repo(&repo.repo_path, "manual").expect("manual commit");
  assert_eq!(
    git(&repo, &["log", "-1", "--format=%s", "HEAD~1"]),
    "f2",
    "the autosave commit should be dropped"
  );
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn merge_workspace_into_home() {
  for strategy in [
    MergeCommit::Merge,
    MergeCommit::SquashAndMerge,
    MergeCommit::RebaseAndMerge,
  ] {
    let repo = TestRepo::new().expect("create repo");
    let main = repo.default_branch().to_string();
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "-m", "Commit init metadata"]);
    let ws = repo
      .create_workspace_with_commit("ws-branch", "ws.txt", "ws\n", None)
      .expect("workspace");
    repo
      .commit_file("home.txt", "home\n", "advance main")
      .expect("advance main");

    core::merge_workspace(&repo.repo_path, ws.id, "merge ws", strategy).expect("merge");
    assert!(assert_git_head_follows_wc_parent(&repo, Some(&main)).is_empty());
  }
}

/// A rewrite run from another workspace rebases the home `@` and updates the
/// home files on disk; Git HEAD must follow there too.
#[test]
fn sibling_workspace_rewrites_home_ancestor() {
  let (repo, _) = home_on_feature();
  let ws = repo
    .create_workspace_simple("ws-branch")
    .expect("workspace");
  let ws_path = repo.workspace_full_path(&ws);
  let f1 = change_id(&repo, "@--");
  jj::jj_describe(&ws_path, &f1, "renamed from workspace").expect("describe");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

/// A `git switch` in a terminal that jj has not read yet is the user's move;
/// a rewrite in another workspace must not drag HEAD back.
#[test]
fn sibling_rewrite_keeps_unread_external_checkout() {
  let (repo, main) = home_on_feature();
  let ws = repo
    .create_workspace_with_commit("ws-branch", "ws.txt", "ws\n", None)
    .expect("workspace");
  let ws_path = repo.workspace_full_path(&ws);
  git(&repo, &["switch", &main]);

  let ws_tip = jj::jj_get_change_id(&ws_path, "@-").expect("workspace tip");
  jj::jj_describe(&ws_path, &ws_tip, "renamed in workspace").expect("describe");

  assert_eq!(
    git(&repo, &["symbolic-ref", "HEAD"]),
    format!("refs/heads/{main}")
  );
  assert!(jj::jj_get_changed_files(&repo.repo_path)
    .expect("re-read home")
    .is_empty());
  assert!(assert_git_head_follows_wc_parent(&repo, Some(&main)).is_empty());
}

#[test]
fn restore_file() {
  let (repo, _) = home_on_feature();
  TestRepo::write_workspace_file(&repo.repo_path, "f1.txt", "edited\n").expect("write");
  jj::jj_get_changed_files(&repo.repo_path).expect("snapshot disk");
  jj::jj_restore_file(&repo.repo_path, "f1.txt").expect("restore file");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn delete_workspace() {
  let (repo, _) = home_on_feature();
  let ws = repo
    .create_workspace_simple("ws-branch")
    .expect("workspace");
  core::delete_workspace(&repo.repo_path, &ws.id).expect("delete workspace");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

#[test]
fn workspace_bookmark_rebase() {
  let (repo, main) = home_on_feature();
  let ws = repo
    .create_workspace_with_commit("ws-branch", "ws.txt", "ws\n", None)
    .expect("workspace");
  let ws_path = repo.workspace_full_path(&ws);
  jj::jj_rebase_workspace_bookmark_onto(&ws_path, "ws-branch", &main).expect("rebase workspace");
  assert!(assert_git_head_follows_wc_parent(&repo, Some("feature")).is_empty());
}

/// Rebasing a workspace stacked on home's checked-out branch rewrites that
/// branch's commits. Home's `@` must move with them: HEAD stays on the branch
/// by name and uncommitted home edits survive.
#[test]
fn stacked_workspace_rebase_keeps_home_on_branch() {
  let (repo, main) = home_on_feature();
  let ws = repo
    .create_workspace_with_commit("ws-branch", "ws.txt", "ws\n", Some("feature"))
    .expect("stacked workspace");
  let ws_path = repo.workspace_full_path(&ws);
  TestRepo::write_workspace_file(&repo.repo_path, "f1.txt", "edited at home\n").expect("write");
  TestRepo::write_workspace_file(&repo.repo_path, "home.txt", "new at home\n").expect("write");
  jj::jj_get_changed_files(&repo.repo_path).expect("snapshot home edits");

  jj::jj_rebase_workspace_bookmark_onto(&ws_path, "ws-branch", &main).expect("rebase workspace");

  git(&repo, &["merge-base", "--is-ancestor", &main, "feature"]);
  assert_eq!(
    assert_git_head_follows_wc_parent(&repo, Some("feature")),
    vec!["f1.txt", "home.txt"]
  );
  let home = std::path::Path::new(&repo.repo_path);
  assert_eq!(
    std::fs::read_to_string(home.join("f1.txt")).unwrap(),
    "edited at home\n"
  );
  assert!(
    home.join("target_0.txt").exists(),
    "home should see the rebased base"
  );
}

/// Covers the resolve workspace lifecycle: start (detach), pick a side,
/// commit the resolution, then forget the resolve workspace.
#[test]
fn resolve_workspace_conflict() {
  let repo = TestRepo::new().expect("create repo");
  let main = repo.default_branch().to_string();
  let ws = repo
    .create_workspace_with_commit("ws-branch", "conflict.txt", "workspace\n", None)
    .expect("workspace");
  let ws_path = repo.workspace_full_path(&ws);
  repo
    .create_file("conflict.txt", "main\n")
    .expect("write main side");
  jj::jj_commit(&repo.repo_path, "main commit").expect("commit main");
  jj::jj_rebase_workspace_bookmark_onto(&ws_path, "ws-branch", &main).expect("conflicting rebase");

  let session =
    core::start_resolve_conflicts(&repo.repo_path, Some(ws.id), None).expect("start resolve");
  let target = session.targets.first().expect("conflicted commit");
  let result = core::resolve_commit(
    &repo.repo_path,
    &target.change_id,
    &[core::ResolveSide::Side2],
    None,
  )
  .expect("resolve");
  assert!(result.success, "{}", result.message);
  assert!(assert_git_head_follows_wc_parent(&repo, Some(&main)).is_empty());
}

fn collect_home_head_markers(dir: &std::path::Path, markers: &mut Vec<(String, String)>) {
  for entry in std::fs::read_dir(dir).expect("read src dir") {
    let path = entry.expect("dir entry").path();
    if path.is_dir() {
      collect_home_head_markers(&path, markers);
    } else if path.extension().is_some_and(|ext| ext == "rs") {
      let source = std::fs::read_to_string(&path).expect("read source");
      for line in source.lines() {
        if let Some(names) = line.strip_prefix("// home-head-test: ") {
          for name in names.split(", ") {
            markers.push((path.display().to_string(), name.to_string()));
          }
        }
      }
    }
  }
}

/// The `home-git-head-history-edit-needs-test` ast-grep rule makes every
/// history-editing function name its tests with a `// home-head-test:` line;
/// this checks those names are tests in this file.
#[test]
fn home_head_test_markers_name_tests_in_this_file() {
  let this_file = include_str!("jj_home_git_head_sync_test.rs");
  let mut markers = Vec::new();
  collect_home_head_markers(
    &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
    &mut markers,
  );
  assert!(!markers.is_empty(), "no home-head-test markers found");
  let missing: Vec<_> = markers
    .iter()
    .filter(|(_, name)| !this_file.contains(&format!("#[test]\nfn {name}()")))
    .collect();
  assert!(
    missing.is_empty(),
    "home-head-test markers name missing tests: {missing:?}"
  );
}
