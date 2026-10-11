/// Integration tests verifying that no workspace is left with a stale working copy
/// after any history-rewriting jj operation.
///
/// Root-cause reference: `update_workspace_after_history_edit` (jj.rs) only checks out
/// `loaded.workspace` — the one workspace whose repo was loaded — but `rebase_descendants()`
/// rewrites the WC-commit of **every** sibling workspace in the repo.  Those siblings'
/// on-disk trees are never updated, so the next `jj` snapshot inverts every rewritten
/// commit into their working copy.
mod e2e_test_helpers;

use e2e_test_helpers::TestRepo;
use treq_lib::jj;

// ─── shared helpers ──────────────────────────────────────────────────────────

/// Return the short commit id of the workspace's current `@` commit.
fn workspace_wc_commit(workspace_path: &str) -> String {
  jj::jj_get_commit_id(workspace_path, "@").unwrap_or_default()
}

/// Assert that the working copy at `workspace_path` has **no** changes (is not stale / clean).
fn assert_working_copy_clean(workspace_path: &str) {
  assert!(
    TestRepo::jj_working_copy_is_clean(workspace_path),
    "Expected working copy at '{}' to be clean",
    workspace_path
  );
}

/// Assert that the working copy at `workspace_path` does NOT contain a diff hunk that
/// removes `content` (i.e. does not undo a previously-committed addition).
fn assert_no_revert_hunk(workspace_path: &str, content: &str) {
  assert!(
    !TestRepo::jj_has_revert_hunk_for_line(workspace_path, content),
    "Working copy at '{}' contains a revert hunk for '{}'",
    workspace_path,
    content
  );
}

fn assert_remote_feature_lineage_preserved(
  deferred_checkout: bool,
  remote_commit_after_workspace_creation: bool,
) {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let default_branch = repo.default_branch().to_string();

  // Make the pre-existing feature bookmark remote-only, as it is when a Treq
  // workspace is recreated on a different machine (or after local deletion).
  TestRepo::run_git(&repo.repo_path, &["branch", "-D", "feature-remote"])
    .expect("Failed to remove local feature branch");
  jj::jj_git_fetch(&repo.repo_path).expect("Failed to import remote-only feature bookmark");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-remote",
    Some("recreated remote workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to recreate workspace from remote feature branch");
  let workspace_path = repo.workspace_full_path(&workspace);

  let expected_remote_commit = if remote_commit_after_workspace_creation {
    repo
      .remote_commit_on_branch(
        &workspace.branch_name,
        "later-remote.txt",
        "later remote content\n",
        "Add later-remote.txt",
      )
      .expect("Failed to advance feature branch on remote");
    jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch later remote feature commit");
    jj::jj_resolve_bookmark_conflict_losslessly(
      &workspace_path,
      &workspace.branch_name,
      &format!("{}@origin", workspace.branch_name),
    )
    .expect("Failed to incorporate later remote feature commit");
    "Add later-remote.txt"
  } else {
    "Add feature.txt"
  };

  // Workspace creation intentionally leaves an empty working-copy commit above
  // the non-empty remote feature commit. Advance main before syncing that stack.
  repo
    .commit_file(
      "advanced-main.txt",
      "advanced main\n",
      "chore: advance main before workspace sync",
    )
    .expect("Failed to advance main");

  if deferred_checkout {
    jj::jj_rebase_workspace_bookmark_onto_deferred_checkout(
      &workspace_path,
      &workspace.branch_name,
      &default_branch,
    )
    .expect("Failed to rebase recreated workspace with deferred checkout");
    jj::update_stale_workspace(&workspace_path)
      .expect("Failed to complete deferred workspace checkout");
  } else {
    jj::jj_rebase_workspace_bookmark_onto(&workspace_path, &workspace.branch_name, &default_branch)
      .expect("Failed to rebase recreated workspace");
  }

  let branch_only_log = TestRepo::jj_log_descriptions(
    &repo.repo_path,
    &format!("{}..{}", default_branch, workspace.branch_name),
    None,
  )
  .expect("Failed to inspect rebased branch lineage");
  assert!(
    branch_only_log
      .iter()
      .any(|(desc, _)| desc.contains(expected_remote_commit)),
    "remote feature commit '{expected_remote_commit}' must remain in branch-only lineage; got:\n{branch_only_log:?}"
  );
  assert_eq!(
    std::fs::read_to_string(std::path::Path::new(&workspace_path).join("feature.txt"))
      .expect("feature tree change must remain checked out"),
    "This is a feature from remote branch"
  );
  if remote_commit_after_workspace_creation {
    assert_eq!(
      std::fs::read_to_string(std::path::Path::new(&workspace_path).join("later-remote.txt"))
        .expect("later remote tree change must remain checked out"),
      "later remote content\n"
    );
  }
  assert_eq!(
    std::fs::read_to_string(std::path::Path::new(&workspace_path).join("advanced-main.txt"))
      .expect("advanced target branch change must be checked out"),
    "advanced main\n"
  );
}

#[test]
fn rebase_recreated_remote_workspace_preserves_complete_lineage() {
  assert_remote_feature_lineage_preserved(false, false);
}

#[test]
fn deferred_rebase_recreated_remote_workspace_preserves_complete_lineage() {
  assert_remote_feature_lineage_preserved(true, false);
}

#[test]
fn rebase_workspace_preserves_remote_lineage_added_after_creation() {
  assert_remote_feature_lineage_preserved(false, true);
}

#[test]
fn deferred_rebase_workspace_preserves_remote_lineage_added_after_creation() {
  assert_remote_feature_lineage_preserved(true, true);
}

// ─── test 1: the reported bug ─────────────────────────────────────────────────

/// After rebasing workspace A's lineage, workspace B (which descends from A) must not
/// end up with a stale working copy that inverts A's previously-committed changes.
///
/// Scenario (mirrors the real bug: treq-testing-ducks / default):
///   main ─► A (commits feature-a.txt)
///             └─► B (committed; empty WC at start of test)
///
///   We then add a new commit to main, rebase A's lineage onto the new main tip, and
///   verify B's working copy is still clean (no inverse diff of A's commits).
#[test]
fn sibling_workspace_not_stale_after_lineage_rebase() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();

  // Create workspace A with a real file commit
  let ws_a = repo
    .create_workspace_with_commit("feat/ws-a", "feature-a.txt", "feature a content", None)
    .expect("Failed to create workspace with commit");
  let ws_a_path = repo.workspace_full_path(&ws_a);

  // Create workspace B stacked on top of A
  let ws_b = repo
    .create_workspace_with_commit(
      "feat/ws-b",
      "feature-b.txt",
      "feature b content",
      Some(&ws_a.branch_name),
    )
    .expect("Failed to create workspace with commit");
  let ws_b_path = repo.workspace_full_path(&ws_b);

  // Sanity: both working copies are clean before the rebase
  assert_working_copy_clean(&ws_a_path);
  assert_working_copy_clean(&ws_b_path);

  // Advance main: add a new commit to the base branch
  repo
    .commit_file(
      "main-advance.txt",
      "main advance content",
      "chore: advance main",
    )
    .expect("Failed to advance main");

  // Rebase A's lineage onto the new main tip
  jj::jj_rebase_workspace_bookmark_onto(&ws_a_path, &ws_a.branch_name, &default_branch)
    .expect("Failed to rebase workspace A lineage");

  // B must not be stale: its working copy should still be clean
  assert_working_copy_clean(&ws_b_path);

  // Specifically, B must not contain a hunk that reverts A's committed content
  assert_no_revert_hunk(&ws_b_path, "feature a content");
}

// ─── test 2: deferred rebase mode with non-empty WC ──────────────────────────

/// The deferred-checkout path must also leave sibling workspaces reconciled.
///
/// Before the fix, `jj_sync_working_copy_if_safe` only syncs when the WC is *empty*,
/// so a non-empty sibling WC after a deferred rebase was permanently stale.
#[test]
fn deferred_rebase_leaves_no_stale_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();

  // Workspace A with a committed file
  let ws_a = repo
    .create_workspace_with_commit("feat/deferred-a", "deferred-a.txt", "content-a", None)
    .expect("Failed to create workspace with commit");
  let ws_a_path = repo.workspace_full_path(&ws_a);

  // Workspace B stacked on A (also has a commit — non-empty WC to ensure
  // jj_sync_working_copy_if_safe won't silently skip it)
  let ws_b = repo
    .create_workspace_with_commit(
      "feat/deferred-b",
      "deferred-b.txt",
      "content-b",
      Some(&ws_a.branch_name),
    )
    .expect("Failed to create workspace with commit");
  let ws_b_path = repo.workspace_full_path(&ws_b);

  // Add a new commit to main to give the rebase something to do
  repo
    .commit_file(
      "main-deferred.txt",
      "main deferred",
      "chore: advance main for deferred test",
    )
    .expect("Failed to advance main");

  // Rebase A's lineage via the *deferred* path (the path that currently skips checkout)
  jj::jj_rebase_workspace_bookmark_onto_deferred_checkout(
    &ws_a_path,
    &ws_a.branch_name,
    &default_branch,
  )
  .expect("Failed to deferred-rebase workspace A");

  // B's working copy must still be clean — no inverse diff of A's committed changes
  assert_working_copy_clean(&ws_b_path);
  assert_no_revert_hunk(&ws_b_path, "content-a");
}

// ─── test 3: jj_abandon must not strand siblings ─────────────────────────────

/// `jj_abandon` rewrites descendants via `rebase_descendants()` but never called
/// `update_workspace_after_history_edit`.  A sibling workspace whose WC descends from
/// the abandoned commit must be reconciled.
#[test]
fn abandon_does_not_strand_sibling_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Workspace A: we will abandon its tip commit
  let ws_a = repo
    .create_workspace_with_commit("feat/abandon-a", "abandon-a.txt", "content-abandon-a", None)
    .expect("Failed to create workspace with commit");
  let ws_a_path = repo.workspace_full_path(&ws_a);

  // Workspace B stacked on A's commit
  let ws_b = repo
    .create_workspace_with_commit(
      "feat/abandon-b",
      "abandon-b.txt",
      "content-abandon-b",
      Some(&ws_a.branch_name),
    )
    .expect("Failed to create workspace with commit");
  let ws_b_path = repo.workspace_full_path(&ws_b);

  // Sanity pre-condition
  assert_working_copy_clean(&ws_a_path);
  assert_working_copy_clean(&ws_b_path);

  // Abandon A's tip commit (the one that added abandon-a.txt)
  // We need the change id; get it from jj log
  let change_id =
    TestRepo::jj_change_id(&ws_a_path, &ws_a.branch_name).expect("Failed to get log for ws_a");

  jj::jj_abandon(&ws_a_path, &change_id).expect("Failed to abandon commit");

  // B's working copy must not have been made stale by the abandon
  assert_working_copy_clean(&ws_b_path);
  assert_no_revert_hunk(&ws_b_path, "content-abandon-a");
}

// ─── test 4: update_stale_workspace is safe to call idempotently ─────────────

/// `update_stale_workspace` must be safe to call on a workspace that is already clean —
/// it must not introduce changes or corrupt the working copy.  This guards the self-healing
/// call sites in core/workspaces.rs and elsewhere that call it defensively after operations.
///
/// Additionally, after a rebase that already reconciled siblings, a follow-up call to
/// `update_stale_workspace` must leave the workspace still clean (no inverse diff reintroduced).
#[test]
fn update_stale_workspace_is_idempotent() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();

  // Workspace A with a committed file
  let ws_a = repo
    .create_workspace_with_commit("feat/idem-a", "idem-a.txt", "idem-a-content", None)
    .expect("Failed to create workspace with commit");
  let ws_a_path = repo.workspace_full_path(&ws_a);

  // Workspace B stacked on A
  let ws_b = repo
    .create_workspace_with_commit(
      "feat/idem-b",
      "idem-b.txt",
      "idem-b-content",
      Some(&ws_a.branch_name),
    )
    .expect("Failed to create workspace with commit");
  let ws_b_path = repo.workspace_full_path(&ws_b);

  // Advance main and rebase A (our fix reconciles B automatically)
  repo
    .commit_file(
      "main-idem.txt",
      "main idem",
      "chore: advance main for idem test",
    )
    .expect("Failed to advance main");
  jj::jj_rebase_workspace_bookmark_onto(&ws_a_path, &ws_a.branch_name, &default_branch)
    .expect("Failed to rebase A");

  // B should already be clean after the rebase (our fix reconciles it)
  assert_working_copy_clean(&ws_b_path);

  // Calling update_stale_workspace again must be harmless (idempotent)
  jj::update_stale_workspace(&ws_b_path).expect("update_stale_workspace should not fail");
  assert_working_copy_clean(&ws_b_path);

  // And again — triple-call to confirm no state corruption
  jj::update_stale_workspace(&ws_b_path).expect("update_stale_workspace should not fail");
  assert_working_copy_clean(&ws_b_path);

  // No inverse hunk for A's committed content
  assert_no_revert_hunk(&ws_b_path, "idem-a-content");
}

// ─── test 5: stacked-workspace fork bug ──────────────────────────────────────

/// Replicates the stacked-workspace fork bug: after auto-rebase the graph forks instead
/// of staying linear.
///
/// Root cause: when a child workspace is created stacked on a parent, the parent's
/// working-copy commit (`wo`, a child of the parent bookmark `tsy`) is NOT used as the
/// stack base — jj.rs:1271-1302 only selects the parent WC as base if its parents equal
/// the bookmark's parents (a sibling, not a child), so it falls back to `bookmark_id`.
/// The child therefore starts as a sibling of `wo` off `tsy`. Auto-rebase preserves the
/// fork because `roots(main..ducks-bookmark)` is bookmark-relative and cannot see `wo`.
///
/// After auto-rebase, the stacked pair must form a straight line:
///   main → chicken-bookmark → chicken-wc → ducks-bookmark → ducks-wc
///
/// FAILS on current code: chicken's working-copy commit `wo` and the ducks bookmark
/// `uorp` are siblings under the chicken bookmark `tsy`, not a straight line.
#[test]
fn test_stacked_workspace_lineage_stays_linear_after_auto_rebase() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();

  // chicken workspace stacked directly on main
  let chicken = repo
    .create_workspace_with_commit("feat/chicken", "chicken.txt", "chicken content\n", None)
    .expect("Failed to create workspace with commit");
  let chicken_path = repo.workspace_full_path(&chicken);

  // ducks workspace stacked on chicken
  let ducks = repo
    .create_workspace_with_commit(
      "feat/ducks",
      "ducks.txt",
      "ducks content\n",
      Some(&chicken.branch_name),
    )
    .expect("Failed to create workspace with commit");

  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, chicken.id, &default_branch)
    .expect("Failed to set target branch on chicken");
  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ducks.id, &default_branch)
    .expect("Failed to set target branch on ducks");

  // Print pre-rebase graph so the test output shows the starting topology.
  let pre_graph = TestRepo::jj_log_descriptions(&repo.repo_path, "all()", None).unwrap_or_default();
  println!("=== pre-rebase graph ===\n{pre_graph:?}");

  // Advance main.
  repo
    .commit_file("main-advance.txt", "advance\n", "chore: advance main")
    .expect("Failed to advance main");

  // Capture chicken's WC commit before the rebase (it will be rewritten but position
  // relative to ducks is what we care about after).
  let chicken_wc_before = workspace_wc_commit(&chicken_path);
  println!("chicken WC before rebase: {chicken_wc_before}");

  // Run the production auto-rebase entry point.
  treq_lib::auto_rebase::rebase_after_commit(&repo.repo_path, &default_branch)
    .expect("rebase_after_commit should not error");

  // Print post-rebase graph for evidence.
  let post_graph =
    TestRepo::jj_log_descriptions(&repo.repo_path, "all()", None).unwrap_or_default();
  println!("=== post-rebase graph ===\n{post_graph:?}");

  let chicken_wc_after = workspace_wc_commit(&chicken_path);
  let chicken_bm = jj::jj_get_commit_id(&repo.repo_path, &chicken.branch_name)
    .expect("Failed to resolve chicken bookmark");
  let ducks_bm = jj::jj_get_commit_id(&repo.repo_path, &ducks.branch_name)
    .expect("Failed to resolve ducks bookmark");

  println!("chicken bookmark:   {chicken_bm}");
  println!("chicken WC (after): {chicken_wc_after}");
  println!("ducks bookmark:     {ducks_bm}");

  // PRIMARY assertion: ducks bookmark must be a descendant of chicken's WC commit.
  // If they are siblings (the bug), ancestors(ducks_bm) does not contain chicken_wc.
  let ancestors_check = TestRepo::jj_commit_ids_in_revset(
    &repo.repo_path,
    &format!("{chicken_wc_after} & ancestors({ducks_bm})"),
  )
  .unwrap_or_default();
  assert!(
    !ancestors_check.is_empty(),
    "ducks bookmark ({ducks_bm}) must be a descendant of chicken's WC commit \
         ({chicken_wc_after}); got empty — they are siblings (fork bug)"
  );

  // CORROBORATING: chicken bookmark must have exactly one child (the chicken WC, not also
  // the ducks bookmark). Two children means the fork exists.
  let children_of_chicken_bm =
    TestRepo::jj_commit_ids_in_revset(&repo.repo_path, &format!("children({chicken_bm})"))
      .unwrap_or_default();
  let child_count = children_of_chicken_bm.len();
  assert_eq!(
    child_count, 1,
    "chicken bookmark ({chicken_bm}) must have exactly 1 child (its WC commit), \
         but has {child_count} — the ducks bookmark is forked as a sibling (fork bug)\n\
         children:\n{children_of_chicken_bm:?}"
  );
}

// ─── test 6: WC-staleness regression (disproven hypothesis, kept as coverage) ──

/// Verifies that auto-rebase does not leave stale on-disk working copies on a stacked pair.
/// This was the initial hypothesis for the live-repo conflict bug; it was disproven
/// (the reconciliation path already works), but the test is kept as regression coverage.
#[test]
fn test_auto_rebase_does_not_leave_stale_working_copy_on_stacked_workspaces() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();

  let chicken_base = repo
    .create_workspace_with_commit("feat/chicken-base", "CHICKEN.md", "# chicken rules\n", None)
    .expect("Failed to create workspace with commit");
  let ws_duck = repo
    .create_workspace_with_commit(
      "feat/duck-fencing",
      "CHICKEN.md",
      "# chicken rules\n\n## duck fencing\n\nUse a sturdy fence.\n",
      Some(&chicken_base.branch_name),
    )
    .expect("Failed to create workspace with commit");
  let ws_duck_path = repo.workspace_full_path(&ws_duck);
  let ws_rule = repo
        .create_workspace_with_commit(
            "feat/ducks-rule",
            "CHICKEN.md",
            "# chicken rules\n\n## duck fencing\n\nUse a sturdy fence.\n\n## ducks rule\n\nYes they do.\n",
            Some(&ws_duck.branch_name),
        )
        .expect("Failed to create workspace with commit");
  let ws_rule_path = repo.workspace_full_path(&ws_rule);

  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ws_duck.id, &default_branch)
    .expect("Failed to set target branch on ws_duck");
  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ws_rule.id, &default_branch)
    .expect("Failed to set target branch on ws_rule");

  repo
    .commit_file("main-advance.txt", "main advance\n", "chore: advance main")
    .expect("Failed to advance main");

  treq_lib::auto_rebase::rebase_after_commit(&repo.repo_path, &default_branch)
    .expect("rebase_after_commit should not error");

  for (label, path) in [("ws_duck", &ws_duck_path), ("ws_rule", &ws_rule_path)] {
    let is_clean = TestRepo::jj_working_copy_is_clean(path);
    println!("[{label}] jj working copy clean: {is_clean}");
    assert!(is_clean, "[{label}] Expected clean WC at '{path}'");
  }
}

// ─── test 7: commits stacked on a carried working copy ──────────────────────

/// A rebase carries another workspace's working copy along when it sits on the
/// moved lineage. When commits sit on top of that working copy, moving it alone
/// would split them off it, so the stack stays together.
#[test]
fn lineage_rebase_keeps_commits_on_their_stacked_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();
  let ws_a = repo
    .create_workspace_with_commit("feat/split-a", "split-a.txt", "a\n", None)
    .expect("Failed to create workspace A");
  let ws_b = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/split-b",
    None,
    None,
    Some(&ws_a.branch_name),
    None,
    None,
  )
  .expect("Failed to create empty workspace B");
  let ws_c = repo
    .create_workspace_with_commit(
      "feat/split-c",
      "split-c.txt",
      "c\n",
      Some(&ws_b.branch_name),
    )
    .expect("Failed to create workspace C");
  let a_path = repo.workspace_full_path(&ws_a);
  let b_path = repo.workspace_full_path(&ws_b);
  let c_path = repo.workspace_full_path(&ws_c);
  repo
    .commit_file("main-advance.txt", "advance\n", "chore: advance main")
    .expect("Failed to advance main");

  jj::jj_rebase_workspace_bookmark_onto(&a_path, &ws_a.branch_name, &default_branch)
    .expect("Failed to rebase workspace A");

  assert!(
    std::path::Path::new(&a_path)
      .join("main-advance.txt")
      .exists(),
    "A should be on the advanced main"
  );
  assert_eq!(
    jj::jj_get_commit_id(&c_path, "@--").expect("C base"),
    workspace_wc_commit(&b_path),
    "C's commit should stay on B's working copy"
  );
}
