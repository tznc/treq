use crate::common::TestRepo;

/// Creating a workspace with sparse patterns materializes only matching paths.
#[test]
fn sparse_workspace_materializes_only_matching_paths() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .commit_file("docs/guide.md", "# Guide\n", "add docs/guide.md")
    .expect("Failed to commit docs/guide.md");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse",
    Some("sparse workspace".to_string()),
    None, // moved_files
    None, // source_branch
    None, // included_copy_files
    Some(vec!["src".to_string()]),
  )
  .expect("Failed to create sparse workspace");

  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(
    ws.join("src/lib.rs").exists(),
    "src/lib.rs should be materialized in sparse workspace"
  );
  assert!(
    !ws.join("docs").exists(),
    "docs/ should not be materialized in sparse workspace"
  );
  assert!(
    !ws.join("README.md").exists(),
    "README.md should not be materialized in sparse workspace"
  );
  assert!(ws.join(".jj").exists(), ".jj directory should exist");

  // Workspace must be a valid jj workspace with a clean working copy.
  assert!(
    TestRepo::jj_working_copy_is_clean(ws.to_str().unwrap()),
    "Sparse workspace should have a clean working copy"
  );
}

/// `None` and an empty pattern list both mean a full checkout — never jj's
/// "empty patterns = materialize nothing" behavior.
#[test]
fn none_or_empty_sparse_patterns_full_checkout() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .commit_file("docs/guide.md", "# Guide\n", "add docs/guide.md")
    .expect("Failed to commit docs/guide.md");

  for (branch, sparse) in [
    ("feat/full-none", None),
    ("feat/full-empty", Some(Vec::new())),
  ] {
    let workspace =
      treq_lib::core::create_workspace(&repo.repo_path, branch, None, None, None, None, sparse)
        .unwrap_or_else(|e| panic!("Failed to create workspace '{}': {}", branch, e));

    let ws = repo.workspaces_dir().join(&workspace.workspace_path);
    for file in ["src/lib.rs", "docs/guide.md", "README.md"] {
      assert!(
        ws.join(file).exists(),
        "{} should be materialized in full workspace '{}'",
        file,
        branch
      );
    }
  }
}

/// Invalid patterns are rejected before any side effect: no workspace
/// directory and no database row.
#[test]
fn invalid_sparse_pattern_rejected_before_side_effects() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  for bad in ["../escape", "/abs/path"] {
    let err = treq_lib::core::create_workspace(
      &repo.repo_path,
      "feat/bad-sparse",
      None,
      None,
      None,
      None,
      Some(vec![bad.to_string()]),
    )
    .expect_err(&format!("pattern '{}' should be rejected", bad));
    assert!(
      err.contains(bad),
      "error should name the offending pattern '{}', got: {}",
      bad,
      err
    );
  }

  assert!(
    !repo.workspaces_dir().join("feat-bad-sparse").exists(),
    "no workspace directory should be left behind"
  );
  let record = treq_lib::local_db::get_workspace_by_branch(&repo.repo_path, "feat/bad-sparse")
    .expect("db query should succeed");
  assert!(record.is_none(), "no db record should be left behind");
}

/// A single-file pattern materializes just that file.
#[test]
fn sparse_pattern_single_file() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .commit_file("docs/guide.md", "# Guide\n", "add docs/guide.md")
    .expect("Failed to commit docs/guide.md");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse-file",
    None,
    None,
    None,
    None,
    Some(vec!["docs/guide.md".to_string()]),
  )
  .expect("Failed to create sparse workspace");

  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(
    ws.join("docs/guide.md").exists(),
    "docs/guide.md should be materialized"
  );
  assert!(!ws.join("src").exists(), "src/ should not be materialized");
  assert!(
    !ws.join("README.md").exists(),
    "README.md should not be materialized"
  );
}

/// moved_files that fall outside the sparse patterns are rejected up front —
/// otherwise they would land in the commit but be invisible on disk.
#[test]
fn moved_files_outside_sparse_patterns_error() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .create_file("docs/x.md", "# uncommitted\n")
    .expect("Failed to create docs/x.md");

  let err = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse-moved",
    None,
    Some(vec!["docs/x.md".to_string()]),
    None,
    None,
    Some(vec!["src".to_string()]),
  )
  .expect_err("moved file outside sparse patterns should be rejected");
  assert!(
    err.contains("docs/x.md"),
    "error should name the moved file, got: {}",
    err
  );
  assert!(
    !repo.workspaces_dir().join("feat-sparse-moved").exists(),
    "no workspace directory should be left behind"
  );
}

/// A moved file inside the sparse patterns is accepted.
#[test]
fn moved_files_inside_sparse_patterns_accepted() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .create_file("src/wip.rs", "// wip\n")
    .expect("Failed to create src/wip.rs");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse-moved-ok",
    None,
    Some(vec!["src/wip.rs".to_string()]),
    None,
    None,
    Some(vec!["src".to_string()]),
  )
  .expect("moved file inside sparse patterns should be accepted");

  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(
    ws.join("src/wip.rs").exists(),
    "moved file should be materialized in the sparse workspace"
  );
}

/// included_copy_files are copied into the workspace regardless of sparse
/// patterns — they are typically gitignored config files needed for builds.
#[test]
fn included_copy_files_copied_regardless_of_sparse() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file(".gitignore", ".env\n", "add gitignore")
    .expect("Failed to commit .gitignore");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .create_file(".env", "SECRET=1\n")
    .expect("Failed to create .env");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse-env",
    None,
    None,
    None,
    Some(vec![".env".to_string()]),
    Some(vec!["src".to_string()]),
  )
  .expect("Failed to create sparse workspace");

  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(
    ws.join(".env").exists(),
    ".env should be copied into the sparse workspace"
  );
  let changes =
    treq_lib::jj::jj_get_changed_files(ws.to_str().unwrap()).expect("Failed to get changed files");
  assert!(
    changes.is_empty(),
    "gitignored copied file should not appear as a change, got: {:?}",
    changes.iter().map(|c| &c.path).collect::<Vec<_>>()
  );
}

/// A stacked workspace (created from another workspace's branch) respects
/// sparse patterns while still starting from the parent's head.
#[test]
fn stacked_workspace_respects_sparse_patterns() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .commit_file("docs/guide.md", "# Guide\n", "add docs/guide.md")
    .expect("Failed to commit docs/guide.md");

  let parent =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/parent", None, None, None, None, None)
      .expect("Failed to create parent workspace");
  repo
    .commit_workspace_file(
      &parent,
      "src/parent.rs",
      "pub fn parent() {}\n",
      "add parent",
    )
    .expect("Failed to commit in parent workspace");

  let child = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/child",
    None,
    None,
    Some("feat/parent"),
    None,
    Some(vec!["src".to_string()]),
  )
  .expect("Failed to create stacked sparse workspace");

  let ws = repo.workspaces_dir().join(&child.workspace_path);
  assert!(
    ws.join("src/parent.rs").exists(),
    "parent's committed file under the pattern should be materialized"
  );
  assert!(
    ws.join("src/lib.rs").exists(),
    "base file under the pattern should be materialized"
  );
  assert!(
    !ws.join("docs").exists(),
    "docs/ is outside the pattern and should not be materialized"
  );
  assert_eq!(
    child.target_branch.as_deref(),
    Some("feat/parent"),
    "stacked workspace should target the parent branch"
  );
}

/// Files written outside the sparse patterns are neither reported as changes
/// nor swept into commits — jj's snapshotting respects the sparse matcher.
#[test]
fn files_outside_sparse_patterns_are_not_snapshotted() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse-snapshot",
    None,
    None,
    None,
    None,
    Some(vec!["src".to_string()]),
  )
  .expect("Failed to create sparse workspace");

  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_str = ws.to_str().unwrap();
  TestRepo::write_workspace_file(ws_str, "src/new.rs", "pub fn new() {}\n")
    .expect("Failed to write src/new.rs");
  TestRepo::write_workspace_file(ws_str, "docs/new.md", "# stray\n")
    .expect("Failed to write docs/new.md");

  let changes = treq_lib::jj::jj_get_changed_files(ws_str).expect("Failed to get changed files");
  let changed_paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
  assert!(
    changed_paths.contains(&"src/new.rs"),
    "src/new.rs should be reported as changed, got: {:?}",
    changed_paths
  );
  assert!(
    !changed_paths.contains(&"docs/new.md"),
    "docs/new.md is outside the sparse patterns and should not be snapshotted, got: {:?}",
    changed_paths
  );

  // Committing must not sweep the out-of-pattern file into the commit tree.
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "sparse commit")
    .expect("Failed to commit workspace");
  let tree_files = TestRepo::jj_files_at_revision(ws_str, "feat/sparse-snapshot")
    .expect("Failed to list files at branch");
  assert!(
    tree_files.iter().any(|f| f == "src/new.rs"),
    "committed tree should contain src/new.rs, got: {:?}",
    tree_files
  );
  assert!(
    !tree_files.iter().any(|f| f == "docs/new.md"),
    "committed tree should not contain docs/new.md, got: {:?}",
    tree_files
  );
}

/// Sparse patterns are registered in jj's working-copy state, not just a disk effect.
#[test]
fn sparse_patterns_registered_in_jj() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/sparse-list",
    None,
    None,
    None,
    None,
    Some(vec!["src".to_string()]),
  )
  .expect("Failed to create sparse workspace");

  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  let patterns =
    TestRepo::jj_sparse_patterns(ws.to_str().unwrap()).expect("jj sparse list should succeed");
  assert_eq!(
    patterns,
    vec!["src".to_string()],
    "jj sparse list should contain exactly 'src', got: {:?}",
    patterns
  );
}

/// The create-workspace metadata JSON the app sends carries the sparse
/// patterns through to the checkout and the stored workspace row.
#[test]
fn create_new_workspace_applies_and_persists_sparse_patterns_from_metadata() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("src/lib.rs", "pub fn lib() {}\n", "add src/lib.rs")
    .expect("Failed to commit src/lib.rs");
  repo
    .commit_file("docs/guide.md", "# Guide\n", "add docs/guide.md")
    .expect("Failed to commit docs/guide.md");

  let metadata = treq_lib::core::parse_workspace_metadata(Some(r#"{"sparse_patterns":["src"]}"#));
  treq_lib::core::create_new_workspace(&repo.repo_path, "feat/sparse-int", None, metadata, None)
    .expect("Failed to create sparse workspace");

  let stored = treq_lib::local_db::get_workspaces(&repo.repo_path)
    .unwrap()
    .into_iter()
    .find(|w| w.branch_name == "feat/sparse-int")
    .expect("workspace should be listed");
  assert_eq!(stored.sparse_patterns, Some(vec!["src".to_string()]));
  let ws = repo.workspaces_dir().join(&stored.workspace_path);
  assert!(ws.join("src/lib.rs").exists());
  assert!(!ws.join("docs").exists());
}

/// Metadata without sparse patterns stores none and checks out everything.
#[test]
fn create_new_workspace_without_sparse_metadata_stores_no_patterns() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file("docs/guide.md", "# Guide\n", "add docs/guide.md")
    .expect("Failed to commit docs/guide.md");

  let metadata = treq_lib::core::parse_workspace_metadata(None);
  let workspace =
    treq_lib::core::create_new_workspace(&repo.repo_path, "feat/full-int", None, metadata, None)
      .expect("Failed to create workspace");

  assert_eq!(workspace.sparse_patterns, None);
  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(ws.join("docs/guide.md").exists());
}
