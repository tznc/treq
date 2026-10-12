//! Differential tests: verify the jj-lib-backed test helpers in tests/common/mod.rs
//! (which replaced `jj` CLI subprocess calls in the rest of the test suite) actually
//! agree with the real `jj` CLI's view of a repo.
//!
//! Unix only: this workflow's CI installs the `jj` CLI for the ubuntu/macos rust
//! and integration jobs, but not reliably for Windows (see ci.yml and the earlier
//! investigation into Windows test flakiness/spawn cost) — so this file is the one
//! place in the suite that still deliberately shells out to `jj`, gated to the
//! platform where doing so is cheap and dependable. If `jj` isn't on PATH (e.g. this
//! sandbox, or a Unix box without it installed), each test prints a message and
//! returns early rather than failing — this file verifies the wrappers when an
//! oracle is available, it doesn't require one to exist.
#![cfg(unix)]

use crate::common::TestRepo;

/// Whether the real `jj` CLI is reachable. Tests skip (rather than fail) when it
/// isn't, since this file's job is cross-checking against it when present, not
/// asserting it must be installed.
fn jj_cli_available() -> bool {
  treq_lib::binary_paths::detect_binary("jj")
    .map(|bin| {
      std::process::Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    })
    .unwrap_or(false)
}

macro_rules! require_jj_cli {
  () => {
    if !jj_cli_available() {
      eprintln!("skipping: jj CLI not available in this environment");
      return;
    }
  };
}

/// Let the CLI touch the repo once and settle any git-HEAD/jj-@ desync from its own
/// first invocation, before any state gets captured for a wrapper-vs-CLI comparison.
/// `TestRepo::new()` colocated repos don't keep git HEAD synced with jj's own view on
/// every jj-lib write (confirmed by jj.rs's own unit tests: a plain `git branch` right
/// after init still points at the pre-jj-init git commit, not jj's new empty @) — the
/// CLI's first invocation on such a repo performs that reconciliation itself, which
/// would otherwise shift @ silently between a wrapper call and a later CLI read.
fn settle_cli(workspace_path: &str) {
  let _ = TestRepo::run_jj_cli(workspace_path, &["log", "-r", "@", "-n", "1", "--no-graph"]);
}

/// CLI-side helper: resolve a revision to its short commit id via `jj log -T commit_id.short(12)`.
fn cli_commit_id(workspace_path: &str, revision: &str) -> String {
  TestRepo::run_jj_cli(
    workspace_path,
    &[
      "log",
      "-r",
      revision,
      "-n",
      "1",
      "--no-graph",
      "-T",
      "commit_id.short(12)",
    ],
  )
  .expect("cli log should succeed")
  .trim()
  .to_string()
}

#[test]
fn jj_git_init_bare_matches_cli_jj_git_init() {
  require_jj_cli!();

  let temp = tempfile::TempDir::new().expect("tempdir");
  let path = temp.path().to_str().unwrap().to_string();

  TestRepo::jj_git_init(&path).expect("jj_git_init_bare should succeed");

  // The CLI must recognize this as a valid, working jj repo.
  let status = TestRepo::run_jj_cli(&path, &["status"]);
  assert!(
    status.is_ok(),
    "CLI `jj status` should succeed against a repo created by jj_git_init_bare, got: {:?}",
    status
  );
}

#[test]
fn jj_new_describe_commit_lifecycle_matches_cli() {
  require_jj_cli!();

  // Exercise the lifecycle on a created workspace, not the colocated home repo
  // root: jj_new/jj_describe are low-level primitives that (unlike jj_commit)
  // don't export the new commit to Git HEAD, and at the home-repo root that
  // leaves the workspace looking "not based on Git HEAD" to
  // reconcile_colocated_home_repo — which a later "is it clean" read triggers
  // as a side effect, silently checking @ back out to the stale Git HEAD and
  // discarding the wrapper's own write. A regular workspace isn't colocated
  // with the home repo's Git HEAD, so it isn't subject to that reconciliation,
  // matching how the app actually drives these primitives outside the home repo.
  let repo = TestRepo::new().expect("create repo");
  settle_cli(&repo.repo_path);
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/lifecycle-lib-vs-cli",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("create workspace");
  let ws_path = repo.workspace_full_path(&workspace);

  // jj_new on top of @ creates a new working-copy commit; the CLI's own log must
  // show the same commit id our wrapper returns.
  let new_id = TestRepo::jj_new(&ws_path, &["@"]).expect("jj_new should succeed");
  assert_eq!(
    new_id,
    cli_commit_id(&ws_path, "@"),
    "jj_new's returned commit id should match what the CLI resolves @ to"
  );

  // jj_describe sets the description; the CLI must see the same text.
  TestRepo::jj_describe(&ws_path, "@", "lib vs cli description")
    .expect("jj_describe should succeed");
  let cli_description = TestRepo::run_jj_cli(
    &ws_path,
    &[
      "log",
      "-r",
      "@",
      "-n",
      "1",
      "--no-graph",
      "-T",
      "description",
    ],
  )
  .expect("cli log should succeed");
  assert!(
    cli_description.contains("lib vs cli description"),
    "CLI should see the description jj_describe set, got: {cli_description}"
  );

  // jj_working_copy_is_clean must agree with the CLI's "no changes" status.
  assert!(
    TestRepo::jj_working_copy_is_clean(&ws_path),
    "freshly described commit with no file changes should still read as clean"
  );
  let cli_status = TestRepo::run_jj_cli(&ws_path, &["status"]).expect("cli status");
  assert!(
    cli_status.contains("The working copy has no changes."),
    "CLI status should agree the working copy is clean, got: {cli_status}"
  );

  // jj_commit finalizes @ and starts a new empty commit on top; the CLI's log
  // must show the finalized commit's description and an empty new @.
  TestRepo::jj_commit(&ws_path, "lib vs cli commit").expect("jj_commit should succeed");
  let cli_log = TestRepo::run_jj_cli(
    &ws_path,
    &[
      "log",
      "--no-graph",
      "-T",
      "description.first_line() ++ \"\\n\"",
      "-n",
      "5",
    ],
  )
  .expect("cli log should succeed");
  assert!(
    cli_log.contains("lib vs cli commit"),
    "CLI log should show the commit made via jj_commit, got: {cli_log}"
  );
}

#[test]
fn jj_set_bookmark_matches_cli_bookmark_list() {
  require_jj_cli!();

  let repo = TestRepo::new().expect("create repo");
  settle_cli(&repo.repo_path);
  TestRepo::jj_set_bookmark(&repo.repo_path, "lib-vs-cli-bookmark", "@")
    .expect("jj_set_bookmark should succeed");

  let cli_bookmarks =
    TestRepo::run_jj_cli(&repo.repo_path, &["bookmark", "list"]).expect("cli bookmark list");
  assert!(
    cli_bookmarks.contains("lib-vs-cli-bookmark"),
    "CLI bookmark list should show the bookmark set via jj_set_bookmark, got: {cli_bookmarks}"
  );

  let wrapper_commit_id = treq_lib::jj::jj_get_commit_id(&repo.repo_path, "lib-vs-cli-bookmark")
    .expect("jj_get_commit_id should resolve the bookmark");
  assert_eq!(
    wrapper_commit_id,
    cli_commit_id(&repo.repo_path, "lib-vs-cli-bookmark"),
    "jj_get_commit_id should agree with the CLI on where the bookmark points"
  );
}

#[test]
fn jj_files_at_revision_matches_cli_file_list() {
  require_jj_cli!();

  let repo = TestRepo::new().expect("create repo");
  settle_cli(&repo.repo_path);
  TestRepo::write_workspace_file(&repo.repo_path, "a.txt", "a").expect("write a.txt");
  TestRepo::write_workspace_file(&repo.repo_path, "dir/b.txt", "b").expect("write dir/b.txt");
  TestRepo::jj_commit(&repo.repo_path, "add files").expect("commit files");

  let wrapper_files =
    TestRepo::jj_files_at_revision(&repo.repo_path, "@-").expect("jj_files_at_revision");
  let cli_files: Vec<String> = TestRepo::run_jj_cli(&repo.repo_path, &["file", "list", "-r", "@-"])
    .expect("cli file list")
    .lines()
    .map(|l| l.trim().replace('\\', "/"))
    .filter(|l| !l.is_empty())
    .collect();

  let mut wrapper_sorted = wrapper_files.clone();
  wrapper_sorted.sort();
  let mut cli_sorted = cli_files.clone();
  cli_sorted.sort();
  assert_eq!(
    wrapper_sorted, cli_sorted,
    "jj_files_at_revision should list the same files as the CLI's `jj file list`"
  );
}

#[test]
fn jj_sparse_patterns_matches_cli_sparse_list() {
  require_jj_cli!();

  let repo = TestRepo::new().expect("create repo");
  settle_cli(&repo.repo_path);
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src")
    .expect("commit src");
  repo
    .commit_file("docs/readme.md", "docs\n", "add docs")
    .expect("commit docs");

  let sparse_patterns = vec!["src".to_string()];
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse-lib-vs-cli",
    None,
    None,
    None,
    None,
    Some(sparse_patterns),
  )
  .expect("create sparse workspace");
  let ws_path = repo.workspace_full_path(&workspace);

  let wrapper_patterns = TestRepo::jj_sparse_patterns(&ws_path).expect("jj_sparse_patterns");
  let cli_patterns: Vec<String> = TestRepo::run_jj_cli(&ws_path, &["sparse", "list"])
    .expect("cli sparse list")
    .lines()
    .map(|l| l.trim().to_string())
    .filter(|l| !l.is_empty())
    .collect();

  assert_eq!(
    wrapper_patterns, cli_patterns,
    "jj_sparse_patterns should match the CLI's `jj sparse list`"
  );
}

#[test]
fn jj_has_revert_hunk_for_line_matches_cli_diff_grep() {
  require_jj_cli!();

  let repo = TestRepo::new().expect("create repo");
  settle_cli(&repo.repo_path);
  TestRepo::write_workspace_file(&repo.repo_path, "f.txt", "line one\nline two\n")
    .expect("write f.txt");
  TestRepo::jj_commit(&repo.repo_path, "add f.txt").expect("commit f.txt");

  // Overwrite with content missing "line two" — a revert-style hunk removing it.
  TestRepo::write_workspace_file(&repo.repo_path, "f.txt", "line one\n").expect("rewrite f.txt");

  let wrapper_has_revert = TestRepo::jj_has_revert_hunk_for_line(&repo.repo_path, "line two");
  let cli_diff = TestRepo::run_jj_cli(&repo.repo_path, &["diff", "--git"]).expect("cli diff --git");
  let cli_has_revert = cli_diff.contains("-line two");

  assert_eq!(
    wrapper_has_revert, cli_has_revert,
    "jj_has_revert_hunk_for_line should agree with grepping the CLI's `jj diff --git`, cli diff:\n{cli_diff}"
  );
  assert!(
    wrapper_has_revert,
    "this fixture should actually contain a revert hunk"
  );
}
