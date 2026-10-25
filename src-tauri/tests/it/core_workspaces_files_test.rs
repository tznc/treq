use crate::common::TestRepo;

#[test]
fn test_ls_workspace_lists_top_level_entries_and_respects_gitignore() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/list-files",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  TestRepo::write_workspace_file(workspace_path.to_str().unwrap(), "alpha_dir/.keep", "")
    .expect("Failed to create alpha_dir");
  TestRepo::write_workspace_file(workspace_path.to_str().unwrap(), "zeta.txt", "zeta")
    .expect("Failed to write zeta.txt");
  TestRepo::write_workspace_file(
    workspace_path.to_str().unwrap(),
    ".gitignore",
    "ignored.txt\n",
  )
  .expect("Failed to write .gitignore");
  TestRepo::write_workspace_file(workspace_path.to_str().unwrap(), "ignored.txt", "ignored")
    .expect("Failed to write ignored.txt");

  let entries = treq_lib::core::ls_workspace(&repo.repo_path, Some(workspace.id))
    .expect("ls_workspace should succeed");

  let names: Vec<String> = entries.iter().map(|entry| entry.name.clone()).collect();
  assert!(
    names.contains(&"alpha_dir".to_string()),
    "Expected alpha_dir in listing, got {:?}",
    names
  );
  assert!(
    names.contains(&"zeta.txt".to_string()),
    "Expected zeta.txt in listing, got {:?}",
    names
  );
  assert!(
    !names.contains(&"ignored.txt".to_string()),
    "Did not expect ignored.txt in listing, got {:?}",
    names
  );

  let alpha_dir_idx = names
    .iter()
    .position(|name| name == "alpha_dir")
    .expect("alpha_dir should be present");
  let zeta_idx = names
    .iter()
    .position(|name| name == "zeta.txt")
    .expect("zeta.txt should be present");
  assert!(
    alpha_dir_idx < zeta_idx,
    "Directories should sort before files, got {:?}",
    names
  );
}

#[test]
fn test_workspace_overview_returns_top_level_readme_contents() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/readme", None, None, None, None, None)
      .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let expected = "# Workspace README\n\nThis is the workspace overview.\n";
  TestRepo::write_workspace_file(workspace_path.to_str().unwrap(), "README.md", expected)
    .expect("Failed to write README.md");

  let (_, readme) = treq_lib::core::workspace_overview(&repo.repo_path, Some(workspace.id))
    .expect("workspace_overview should succeed");

  assert_eq!(readme.as_deref(), Some(expected));
}

#[test]
fn test_workspace_overview_returns_no_readme_when_missing() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/no-readme",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  TestRepo::remove_file_path(workspace_path.join("README.md")).expect("Failed to remove README.md");

  let (_, readme) = treq_lib::core::workspace_overview(&repo.repo_path, Some(workspace.id))
    .expect("workspace_overview should succeed");

  assert_eq!(readme, None);
}

fn status_of<'a>(entries: &'a [treq_lib::core::WorkspaceEntry], name: &str) -> Option<&'a str> {
  entries
    .iter()
    .find(|entry| entry.name == name)
    .unwrap_or_else(|| panic!("{} missing from listing", name))
    .status
    .as_deref()
}

#[test]
fn list_workspace_files_reports_status_of_each_top_level_entry() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch().to_string();
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/file-status",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo
    .workspaces_dir()
    .join(&workspace.workspace_path)
    .to_string_lossy()
    .to_string();

  // Conflicted: both sides edit conflict.txt, then rebase onto main.
  TestRepo::write_workspace_file(&workspace_path, "conflict.txt", "workspace version\n").unwrap();
  TestRepo::write_workspace_file(&workspace_path, "src/committed.rs", "fn a() {}\n").unwrap();
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit").unwrap();
  repo
    .create_file("conflict.txt", "main version\n")
    .expect("write main conflict.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("commit main");
  treq_lib::jj::jj_rebase_workspace_bookmark_onto(
    &workspace_path,
    &workspace.branch_name,
    &default_branch,
  )
  .expect("rebase");

  // Working copy: an uncommitted edit in @.
  TestRepo::write_workspace_file(&workspace_path, "dirty.txt", "uncommitted\n").unwrap();

  let entries = treq_lib::core::list_workspace_files(&repo.repo_path, Some(workspace.id))
    .expect("list_workspace_files should succeed");

  assert_eq!(status_of(&entries, "conflict.txt"), Some("conflict"));
  assert_eq!(status_of(&entries, "src"), Some("committed"));
  assert_eq!(status_of(&entries, "dirty.txt"), Some("workingCopy"));
  assert_eq!(status_of(&entries, "README.md"), Some("untouched"));
}
