mod common;

use common::TestRepo;
use std::collections::HashSet;
use std::path::Path;
use treq_lib::core::{HunkSpec, WorkspaceMoveRequest};
use treq_lib::local_db::Workspace;

fn setup_parent_child_graph(repo: &TestRepo) -> (Workspace, Workspace) {
  let parent = treq_lib::core::create_workspace(
    &repo.repo_path,
    "move-parent",
    Some("parent".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("should create parent workspace");

  let child = treq_lib::core::create_workspace(
    &repo.repo_path,
    "move-child",
    Some("child".to_string()),
    None,
    Some(&parent.branch_name),
    None,
    None,
  )
  .expect("should create child workspace");

  (parent, child)
}

fn setup_single_workspace(repo: &TestRepo) -> Workspace {
  treq_lib::core::create_workspace(
    &repo.repo_path,
    "move-home-ws",
    Some("workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("should create workspace")
}

fn setup_sibling_graph(repo: &TestRepo) -> (Workspace, Workspace) {
  let root = treq_lib::core::create_workspace(
    &repo.repo_path,
    "move-root",
    Some("root".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("should create root workspace");

  let left = treq_lib::core::create_workspace(
    &repo.repo_path,
    "move-left",
    Some("left".to_string()),
    None,
    Some(&root.branch_name),
    None,
    None,
  )
  .expect("should create left sibling workspace");

  let right = treq_lib::core::create_workspace(
    &repo.repo_path,
    "move-right",
    Some("right".to_string()),
    None,
    Some(&root.branch_name),
    None,
    None,
  )
  .expect("should create right sibling workspace");

  (left, right)
}

fn make_commit_fixture(
  repo: &TestRepo,
  source: &Workspace,
  file_path: &str,
  marker: &str,
) -> String {
  let source_path = repo.workspace_full_path(source);
  TestRepo::write_workspace_file(&source_path, file_path, &format!("{}\n", marker))
    .expect("should write fixture file");
  treq_lib::core::commit_workspace(&repo.repo_path, source.id, &format!("commit-{}", marker))
    .expect("should create workspace commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(source.id), false, None, None)
    .expect("should list source commits");
  log
    .commits
    .iter()
    .find(|c| c.description.contains(marker))
    .map(|c| c.commit_id.clone())
    .expect("commit should be discoverable in source history")
}

fn make_file_fixture(repo: &TestRepo, source: &Workspace, file_path: &str, content: &str) {
  let source_path = repo.workspace_full_path(source);
  TestRepo::write_workspace_file(&source_path, file_path, content)
    .expect("should write file fixture");
}

fn make_hunk_fixture(repo: &TestRepo, source: &Workspace, file_path: &str) {
  let source_path = repo.workspace_full_path(source);
  TestRepo::write_workspace_file(&source_path, file_path, "line-1\nline-2\nline-3\nline-4\n")
    .expect("should write hunk fixture");
}

fn read_workspace_file(repo: &TestRepo, workspace: &Workspace, file_path: &str) -> String {
  let abs = Path::new(&repo.workspace_full_path(workspace)).join(file_path);
  std::fs::read_to_string(abs).expect("expected workspace file to exist")
}

fn read_home_file(repo: &TestRepo, file_path: &str) -> String {
  let abs = Path::new(&repo.repo_path).join(file_path);
  std::fs::read_to_string(abs).expect("expected home repo file to exist")
}

fn make_home_commit_fixture(repo: &TestRepo, file_path: &str, marker: &str) -> String {
  TestRepo::write_workspace_file(&repo.repo_path, file_path, &format!("{}\n", marker))
    .expect("should write home fixture file");
  treq_lib::core::commit_workspace(
    &repo.repo_path,
    Option::<i64>::None,
    &format!("commit-{}", marker),
  )
  .expect("should create home commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("should list home commits");
  log
    .commits
    .iter()
    .find(|c| c.description.contains(marker))
    .map(|c| c.commit_id.clone())
    .expect("commit should be discoverable in home history")
}

fn assert_home_history_contains_commit(repo: &TestRepo, commit_id: &str) {
  let log = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("should list home commits");
  let ids: HashSet<String> = log
    .commits
    .into_iter()
    .flat_map(|c| [c.commit_id, c.change_id])
    .collect();
  assert!(
    ids.contains(commit_id),
    "expected commit '{}' to be present in home history",
    commit_id
  );
}

fn assert_home_history_does_not_contain_commit(repo: &TestRepo, commit_id: &str) {
  let log = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("should list home commits");
  let ids: HashSet<String> = log
    .commits
    .into_iter()
    .flat_map(|c| [c.commit_id, c.change_id])
    .collect();
  assert!(
    !ids.contains(commit_id),
    "expected commit '{}' to be absent from home history",
    commit_id
  );
}

fn assert_history_contains_commit(repo: &TestRepo, workspace: &Workspace, commit_id: &str) {
  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("should list workspace commits");
  let ids: HashSet<String> = log
    .commits
    .into_iter()
    .flat_map(|c| [c.commit_id, c.change_id])
    .collect();
  assert!(
    ids.contains(commit_id),
    "expected commit '{}' to be present in workspace history",
    commit_id
  );
}

fn assert_history_does_not_contain_commit(repo: &TestRepo, workspace: &Workspace, commit_id: &str) {
  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("should list workspace commits");
  let ids: HashSet<String> = log
    .commits
    .into_iter()
    .flat_map(|c| [c.commit_id, c.change_id])
    .collect();
  assert!(
    !ids.contains(commit_id),
    "expected commit '{}' to be absent from workspace history",
    commit_id
  );
}

fn assert_source_working_copy_not_changed_for_file(
  repo: &TestRepo,
  workspace: &Workspace,
  file_path: &str,
) {
  let changed_files = treq_lib::core::list_changed_files(&repo.repo_path, Some(workspace.id))
    .expect("should list changed files");
  assert!(
    !changed_files.iter().any(|change| change.path == file_path),
    "expected '{}' to be absent from source working-copy changes",
    file_path
  );
}

fn assert_source_working_copy_hunks_do_not_contain_lines(
  repo: &TestRepo,
  workspace: &Workspace,
  file_path: &str,
  forbidden_lines: &[&str],
) {
  let hunks =
    treq_lib::core::list_file_hunks(&repo.repo_path, Some(workspace.id), file_path, "git")
      .expect("should list file hunks");
  let all_hunk_lines = hunks
    .iter()
    .flat_map(|h| h.lines.iter())
    .cloned()
    .collect::<Vec<String>>();
  for forbidden_line in forbidden_lines {
    assert!(
      !all_hunk_lines
        .iter()
        .any(|line| line.contains(forbidden_line)),
      "expected moved line '{}' to be absent from source working-copy hunks for '{}'",
      forbidden_line,
      file_path
    );
  }
}

#[test]
fn moves_commits_parent_to_child() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  let commit_id = make_commit_fixture(&repo, &parent, "commit-parent-to-child.txt", "parent-child");

  assert_history_contains_commit(&repo, &parent, &commit_id);

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      commits: vec![commit_id.clone()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move commits should succeed");

  assert_eq!(result.commits_moved, 1);
  assert_history_does_not_contain_commit(&repo, &parent, &commit_id);
}

#[test]
fn moves_files_parent_to_child() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_file_fixture(
    &repo,
    &parent,
    "files-parent-to-child.txt",
    "file-parent-child\n",
  );

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      files: vec!["files-parent-to-child.txt".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move files should succeed");

  assert_eq!(result.files_moved, 1);
  assert!(
    read_workspace_file(&repo, &child, "files-parent-to-child.txt").contains("file-parent-child")
  );
  assert_source_working_copy_not_changed_for_file(&repo, &parent, "files-parent-to-child.txt");
}

#[test]
fn moves_modified_file_without_deleting_it_from_source() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_file_fixture(
    &repo,
    &parent,
    "README.md",
    "# Test Repository\n\nsource-only change\n",
  );

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      files: vec!["README.md".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move modified file should succeed");

  assert_eq!(result.files_moved, 1);
  assert_eq!(
    read_workspace_file(&repo, &parent, "README.md"),
    "# Test Repository\n"
  );
  assert_eq!(
    read_workspace_file(&repo, &child, "README.md"),
    "# Test Repository\n\nsource-only change\n"
  );
  assert_source_working_copy_not_changed_for_file(&repo, &parent, "README.md");
}

#[test]
fn moves_modified_file_hunk_without_deleting_file_from_source() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_file_fixture(
    &repo,
    &parent,
    "README.md",
    "# Test Repository\nsource-only line\n",
  );

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      hunks: vec![HunkSpec {
        file_path: "README.md".to_string(),
        start_line: 2,
        end_line: 2,
      }],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move modified file hunk should succeed");

  assert_eq!(result.hunks_applied, 1);
  assert_eq!(
    read_workspace_file(&repo, &parent, "README.md"),
    "# Test Repository\n"
  );
  assert_eq!(
    read_workspace_file(&repo, &child, "README.md"),
    "# Test Repository\nsource-only line\n"
  );
  assert_source_working_copy_not_changed_for_file(&repo, &parent, "README.md");
}

#[test]
fn moves_commit_modifying_file_without_deleting_file_from_source() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_file_fixture(
    &repo,
    &parent,
    "README.md",
    "# Test Repository\ncommitted source change\n",
  );
  treq_lib::core::commit_workspace(&repo.repo_path, parent.id, "modify tracked file")
    .expect("should commit tracked-file modification");
  let commit_id = treq_lib::core::list_commits(&repo.repo_path, Some(parent.id), false, None, None)
    .expect("should list source commits")
    .commits
    .into_iter()
    .find(|commit| commit.description.contains("modify tracked file"))
    .expect("commit should be present")
    .commit_id;

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      commits: vec![commit_id.clone()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move tracked-file commit should succeed");

  assert_eq!(result.commits_moved, 1);
  assert_history_does_not_contain_commit(&repo, &parent, &commit_id);
  assert_eq!(
    read_workspace_file(&repo, &parent, "README.md"),
    "# Test Repository\n"
  );
  assert_eq!(
    read_workspace_file(&repo, &child, "README.md"),
    "# Test Repository\ncommitted source change\n"
  );
}

#[test]
fn moves_only_selected_file_and_leaves_unselected_new_file_in_source() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_sibling_graph(&repo);
  make_file_fixture(&repo, &parent, "selected.txt", "selected change\n");
  make_file_fixture(&repo, &parent, "unselected.txt", "unselected change\n");

  treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      files: vec!["selected.txt".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("selected file move should succeed");

  assert_eq!(
    read_workspace_file(&repo, &child, "selected.txt"),
    "selected change\n"
  );
  let destination_changes = treq_lib::core::list_changed_files(&repo.repo_path, Some(child.id))
    .expect("should list destination changes");
  assert!(!destination_changes
    .iter()
    .any(|change| change.path == "unselected.txt"));
  assert_eq!(
    read_workspace_file(&repo, &parent, "unselected.txt"),
    "unselected change\n"
  );
  assert!(!Path::new(&repo.workspace_full_path(&parent))
    .join("selected.txt")
    .exists());
}

#[test]
fn moves_only_selected_hunk_and_leaves_other_hunk_in_source() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_sibling_graph(&repo);
  make_file_fixture(
    &repo,
    &parent,
    "README.md",
    "# Test Repository\nMOVE_ME\nKEEP_ME\n",
  );

  treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      hunks: vec![HunkSpec {
        file_path: "README.md".to_string(),
        start_line: 2,
        end_line: 2,
      }],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("selected hunk move should succeed");

  assert_eq!(
    read_workspace_file(&repo, &parent, "README.md"),
    "# Test Repository\nKEEP_ME\n"
  );
  assert_eq!(
    read_workspace_file(&repo, &child, "README.md"),
    "# Test Repository\nMOVE_ME\n"
  );
  assert_source_working_copy_hunks_do_not_contain_lines(&repo, &parent, "README.md", &["MOVE_ME"]);
}

#[test]
fn moves_only_selected_commit_and_leaves_unselected_commit_in_source() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_sibling_graph(&repo);
  let selected_commit =
    make_commit_fixture(&repo, &parent, "selected-commit.txt", "selected-commit");
  let _unselected_commit =
    make_commit_fixture(&repo, &parent, "unselected-commit.txt", "unselected-commit");

  treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      commits: vec![selected_commit.clone()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("selected commit move should succeed");

  assert_history_does_not_contain_commit(&repo, &parent, &selected_commit);
  let source_log =
    treq_lib::core::list_commits(&repo.repo_path, Some(parent.id), false, None, None)
      .expect("should list source commits after move");
  assert!(source_log
    .commits
    .iter()
    .any(|commit| commit.description.contains("unselected-commit")));
  let destination_log =
    treq_lib::core::list_commits(&repo.repo_path, Some(child.id), false, None, None)
      .expect("should list destination commits after move");
  assert!(!destination_log
    .commits
    .iter()
    .any(|commit| commit.description.contains("unselected-commit")));
  assert_eq!(
    read_workspace_file(&repo, &child, "selected-commit.txt"),
    "selected-commit\n"
  );
  assert!(!Path::new(&repo.workspace_full_path(&child))
    .join("unselected-commit.txt")
    .exists());
  assert_eq!(
    read_workspace_file(&repo, &parent, "unselected-commit.txt"),
    "unselected-commit\n"
  );
}

#[test]
fn moves_hunks_parent_to_child() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_hunk_fixture(&repo, &parent, "hunks-parent-to-child.txt");

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      hunks: vec![
        HunkSpec {
          file_path: "hunks-parent-to-child.txt".to_string(),
          start_line: 2,
          end_line: 3,
        },
        HunkSpec {
          file_path: "hunks-parent-to-child.txt".to_string(),
          start_line: 9,
          end_line: 10,
        },
      ],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move hunks should succeed");

  assert_eq!(result.hunks_applied, 1);
  assert_eq!(result.hunks_skipped, 1);
  assert!(result
    .warnings
    .iter()
    .any(|w| w.contains("source range out of bounds")));

  let source_content = read_workspace_file(&repo, &parent, "hunks-parent-to-child.txt");
  let destination_content = read_workspace_file(&repo, &child, "hunks-parent-to-child.txt");
  assert!(source_content.contains("line-1\nline-4\n"));
  assert!(destination_content.contains("line-2\nline-3\n"));
  assert_source_working_copy_hunks_do_not_contain_lines(
    &repo,
    &parent,
    "hunks-parent-to-child.txt",
    &["line-2", "line-3"],
  );
}

#[test]
fn moves_commits_child_to_parent() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  let commit_id = make_commit_fixture(&repo, &child, "commit-child-to-parent.txt", "child-parent");

  assert_history_contains_commit(&repo, &child, &commit_id);

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &child.branch_name,
    &parent.branch_name,
    WorkspaceMoveRequest {
      commits: vec![commit_id.clone()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move commits should succeed");

  assert_eq!(result.commits_moved, 1);
  assert_history_does_not_contain_commit(&repo, &child, &commit_id);
}

#[test]
fn moves_files_child_to_parent() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_file_fixture(
    &repo,
    &child,
    "files-child-to-parent.txt",
    "file-child-parent\n",
  );

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &child.branch_name,
    &parent.branch_name,
    WorkspaceMoveRequest {
      files: vec!["files-child-to-parent.txt".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move files should succeed");

  assert_eq!(result.files_moved, 1);
  assert!(
    read_workspace_file(&repo, &parent, "files-child-to-parent.txt").contains("file-child-parent")
  );
  assert_source_working_copy_not_changed_for_file(&repo, &child, "files-child-to-parent.txt");
}

#[test]
fn moves_hunks_child_to_parent() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_hunk_fixture(&repo, &child, "hunks-child-to-parent.txt");

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &child.branch_name,
    &parent.branch_name,
    WorkspaceMoveRequest {
      hunks: vec![
        HunkSpec {
          file_path: "hunks-child-to-parent.txt".to_string(),
          start_line: 2,
          end_line: 3,
        },
        HunkSpec {
          file_path: "hunks-child-to-parent.txt".to_string(),
          start_line: 99,
          end_line: 100,
        },
      ],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move hunks should succeed");

  assert_eq!(result.hunks_applied, 1);
  assert_eq!(result.hunks_skipped, 1);
  assert!(result
    .warnings
    .iter()
    .any(|w| w.contains("source range out of bounds")));

  let source_content = read_workspace_file(&repo, &child, "hunks-child-to-parent.txt");
  let destination_content = read_workspace_file(&repo, &parent, "hunks-child-to-parent.txt");
  assert!(source_content.contains("line-1\nline-4\n"));
  assert!(destination_content.contains("line-2\nline-3\n"));
  assert_source_working_copy_hunks_do_not_contain_lines(
    &repo,
    &child,
    "hunks-child-to-parent.txt",
    &["line-2", "line-3"],
  );
}

#[test]
fn moves_commits_between_siblings() {
  let repo = TestRepo::new().expect("should create repo");
  let (left, right) = setup_sibling_graph(&repo);
  let commit_id = make_commit_fixture(&repo, &left, "commit-left-to-right.txt", "left-right");

  assert_history_contains_commit(&repo, &left, &commit_id);

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &left.branch_name,
    &right.branch_name,
    WorkspaceMoveRequest {
      commits: vec![commit_id.clone()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move commits should succeed");

  assert_eq!(result.commits_moved, 1);
  assert_history_does_not_contain_commit(&repo, &left, &commit_id);
}

#[test]
fn moves_files_between_siblings() {
  let repo = TestRepo::new().expect("should create repo");
  let (left, right) = setup_sibling_graph(&repo);
  make_file_fixture(&repo, &left, "files-left-to-right.txt", "file-left-right\n");

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &left.branch_name,
    &right.branch_name,
    WorkspaceMoveRequest {
      files: vec!["files-left-to-right.txt".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move files should succeed");

  assert_eq!(result.files_moved, 1);
  assert!(
    read_workspace_file(&repo, &right, "files-left-to-right.txt").contains("file-left-right")
  );
  assert_source_working_copy_not_changed_for_file(&repo, &left, "files-left-to-right.txt");
}

#[test]
fn moves_hunks_between_siblings() {
  let repo = TestRepo::new().expect("should create repo");
  let (left, right) = setup_sibling_graph(&repo);
  make_hunk_fixture(&repo, &left, "hunks-left-to-right.txt");

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &left.branch_name,
    &right.branch_name,
    WorkspaceMoveRequest {
      hunks: vec![
        HunkSpec {
          file_path: "hunks-left-to-right.txt".to_string(),
          start_line: 2,
          end_line: 3,
        },
        HunkSpec {
          file_path: "hunks-left-to-right.txt".to_string(),
          start_line: 77,
          end_line: 88,
        },
      ],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move hunks should succeed");

  assert_eq!(result.hunks_applied, 1);
  assert_eq!(result.hunks_skipped, 1);
  assert!(result
    .warnings
    .iter()
    .any(|w| w.contains("source range out of bounds")));

  let source_content = read_workspace_file(&repo, &left, "hunks-left-to-right.txt");
  let destination_content = read_workspace_file(&repo, &right, "hunks-left-to-right.txt");
  assert!(source_content.contains("line-1\nline-4\n"));
  assert!(destination_content.contains("line-2\nline-3\n"));
  assert_source_working_copy_hunks_do_not_contain_lines(
    &repo,
    &left,
    "hunks-left-to-right.txt",
    &["line-2", "line-3"],
  );
}

#[test]
fn moves_combined_selectors_in_commit_file_hunk_order() {
  let repo = TestRepo::new().expect("should create repo");
  let (left, right) = setup_sibling_graph(&repo);

  let commit_id = make_commit_fixture(&repo, &left, "combo-commit.txt", "combo-commit");
  make_file_fixture(&repo, &left, "combo-file.txt", "combo-file\n");
  make_hunk_fixture(&repo, &left, "combo-hunk.txt");

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &left.branch_name,
    &right.branch_name,
    WorkspaceMoveRequest {
      commits: vec![commit_id],
      files: vec!["combo-file.txt".to_string()],
      hunks: vec![HunkSpec {
        file_path: "combo-hunk.txt".to_string(),
        start_line: 2,
        end_line: 3,
      }],
    },
  )
  .expect("combined move should succeed");

  assert_eq!(result.commits_moved, 1);
  assert_eq!(result.files_moved, 1);
  assert_eq!(result.hunks_applied, 1);

  assert!(read_workspace_file(&repo, &right, "combo-commit.txt").contains("combo-commit"));
  assert!(read_workspace_file(&repo, &right, "combo-file.txt").contains("combo-file"));
  assert!(read_workspace_file(&repo, &right, "combo-hunk.txt").contains("line-2\nline-3\n"));
}

#[test]
fn moves_combined_selectors_workspace_to_home() {
  let repo = TestRepo::new().expect("should create repo");
  let ws = setup_single_workspace(&repo);

  let commit_id = make_commit_fixture(
    &repo,
    &ws,
    "combo-ws-to-home-commit.txt",
    "combo-ws-home-commit",
  );
  make_file_fixture(
    &repo,
    &ws,
    "combo-ws-to-home-file.txt",
    "combo-ws-home-file\n",
  );
  make_hunk_fixture(&repo, &ws, "combo-ws-to-home-hunk.txt");

  assert_history_contains_commit(&repo, &ws, &commit_id);

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &ws.branch_name,
    ".",
    WorkspaceMoveRequest {
      commits: vec![commit_id.clone()],
      files: vec!["combo-ws-to-home-file.txt".to_string()],
      hunks: vec![HunkSpec {
        file_path: "combo-ws-to-home-hunk.txt".to_string(),
        start_line: 2,
        end_line: 3,
      }],
    },
  )
  .expect("combined move to home should succeed");

  assert_eq!(result.commits_moved, 1);
  assert_eq!(result.files_moved, 1);
  assert_eq!(result.hunks_applied, 1);

  assert_history_does_not_contain_commit(&repo, &ws, &commit_id);
  assert!(read_home_file(&repo, "combo-ws-to-home-commit.txt").contains("combo-ws-home-commit"));
  assert!(read_home_file(&repo, "combo-ws-to-home-file.txt").contains("combo-ws-home-file"));
  assert!(read_home_file(&repo, "combo-ws-to-home-hunk.txt").contains("line-2\nline-3\n"));
}

#[test]
fn moves_combined_selectors_home_to_workspace() {
  let repo = TestRepo::new().expect("should create repo");
  let ws = setup_single_workspace(&repo);

  let commit_id =
    make_home_commit_fixture(&repo, "combo-home-to-ws-commit.txt", "combo-home-ws-commit");
  TestRepo::write_workspace_file(
    &repo.repo_path,
    "combo-home-to-ws-file.txt",
    "combo-home-ws-file\n",
  )
  .expect("should write home file fixture");
  TestRepo::write_workspace_file(
    &repo.repo_path,
    "combo-home-to-ws-hunk.txt",
    "line-1\nline-2\nline-3\nline-4\n",
  )
  .expect("should write home hunk fixture");

  assert_home_history_contains_commit(&repo, &commit_id);

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    ".",
    &ws.branch_name,
    WorkspaceMoveRequest {
      commits: vec![commit_id.clone()],
      files: vec!["combo-home-to-ws-file.txt".to_string()],
      hunks: vec![HunkSpec {
        file_path: "combo-home-to-ws-hunk.txt".to_string(),
        start_line: 2,
        end_line: 3,
      }],
    },
  )
  .expect("combined move from home should succeed");

  assert_eq!(result.commits_moved, 1);
  assert_eq!(result.files_moved, 1);
  assert_eq!(result.hunks_applied, 1);

  assert_home_history_does_not_contain_commit(&repo, &commit_id);
  assert!(
    read_workspace_file(&repo, &ws, "combo-home-to-ws-commit.txt").contains("combo-home-ws-commit")
  );
  assert!(
    read_workspace_file(&repo, &ws, "combo-home-to-ws-file.txt").contains("combo-home-ws-file")
  );
  assert!(read_workspace_file(&repo, &ws, "combo-home-to-ws-hunk.txt").contains("line-2\nline-3\n"));
}

#[test]
fn returns_error_when_source_and_destination_are_the_same() {
  let repo = TestRepo::new().expect("should create repo");
  let ws = setup_single_workspace(&repo);
  make_file_fixture(&repo, &ws, "same-endpoint.txt", "content\n");

  let err = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    ".",
    ".",
    WorkspaceMoveRequest {
      files: vec!["same-endpoint.txt".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect_err("should reject moving home to itself");

  assert!(err.contains("must be different"));
}

#[test]
fn returns_error_when_destination_workspace_not_found() {
  let repo = TestRepo::new().expect("should create repo");
  let ws = setup_single_workspace(&repo);
  make_file_fixture(&repo, &ws, "missing-destination.txt", "content\n");

  let err = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &ws.branch_name,
    "does-not-exist",
    WorkspaceMoveRequest {
      files: vec!["missing-destination.txt".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect_err("should fail for unknown destination workspace");

  assert!(err.contains("Destination workspace 'does-not-exist' not found"));
}

#[test]
fn returns_error_when_commit_not_in_source_history() {
  let repo = TestRepo::new().expect("should create repo");
  let (parent, child) = setup_parent_child_graph(&repo);

  let err = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      commits: vec!["missing-commit-id".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect_err("should fail for commit not found in source history");

  assert!(err.contains("not found in source workspace history"));
}

#[test]
fn move_files_rejects_files_with_no_changes_in_the_source() {
  let repo = TestRepo::new().expect("create test repo");
  let (parent, child) = setup_parent_child_graph(&repo);

  let err = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      files: vec!["does-not-exist.txt".to_string(), "README.md".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect_err("moving unchanged files must not report success");
  assert!(err.contains("no changes"), "{err}");
}

#[test]
fn move_files_counts_only_changed_files_and_warns_about_the_rest() {
  let repo = TestRepo::new().expect("create test repo");
  let (parent, child) = setup_parent_child_graph(&repo);
  make_file_fixture(&repo, &parent, "changed.txt", "changed\n");

  let result = treq_lib::core::move_workspace_changes(
    &repo.repo_path,
    &parent.branch_name,
    &child.branch_name,
    WorkspaceMoveRequest {
      files: vec!["changed.txt".to_string(), "does-not-exist.txt".to_string()],
      ..WorkspaceMoveRequest::default()
    },
  )
  .expect("move should succeed for the changed file");

  assert_eq!(result.files_moved, 1);
  assert!(
    result
      .warnings
      .iter()
      .any(|w| w.contains("does-not-exist.txt")),
    "{:?}",
    result.warnings
  );
  assert!(read_workspace_file(&repo, &child, "changed.txt").contains("changed"));
}
