use crate::common::TestRepo;
use tempfile::TempDir;

fn setup_jj_repo(temp_dir: &TempDir) -> String {
  let path = temp_dir.path().to_str().unwrap().to_string();
  TestRepo::jj_git_init(&path).expect("Failed to init jj repo");
  path
}

#[test]
fn test_get_jj_tracked_files_returns_all_files() {
  let temp_dir = TempDir::new().expect("Failed to create temp dir");
  let repo_path = setup_jj_repo(&temp_dir);
  let repo_path_str = repo_path.as_str();

  TestRepo::write_workspace_file(repo_path_str, "file1.txt", "content1").unwrap();
  TestRepo::write_workspace_file(repo_path_str, "file2.txt", "content2").unwrap();
  TestRepo::write_workspace_file(repo_path_str, "subdir/file3.txt", "content3").unwrap();

  TestRepo::jj_snapshot(&repo_path).expect("Failed to snapshot working copy");

  let files = treq_lib::file_indexer::get_jj_tracked_files(&repo_path).expect("Should get files");

  assert!(files.contains(&"file1.txt".to_string()));
  assert!(files.contains(&"file2.txt".to_string()));
  assert!(files.contains(&"subdir/file3.txt".to_string()));
}

#[test]
fn test_get_jj_tracked_files_includes_unchanged_committed_files() {
  let temp_dir = TempDir::new().expect("Failed to create temp dir");
  let repo_path = setup_jj_repo(&temp_dir);
  let repo_path_str = repo_path.as_str();

  TestRepo::write_workspace_file(repo_path_str, "committed.txt", "committed").unwrap();
  // This fixture is a bare (non-colocated) jj repo with no .git directory, so it can't
  // use TestRepo::jj_commit (which resolves a git branch as part of treq's normal,
  // colocated-repo commit flow). Describe-then-new is what `jj commit -m` does under
  // the hood, without needing any git-branch concept.
  TestRepo::jj_describe(&repo_path, "@", "initial").expect("Failed to describe");
  TestRepo::jj_new(&repo_path, &["@"]).expect("Failed to commit");

  TestRepo::write_workspace_file(repo_path_str, "changed.txt", "changed").unwrap();

  let files = treq_lib::file_indexer::get_jj_tracked_files(&repo_path).expect("Should get files");

  assert!(
    files.contains(&"committed.txt".to_string()),
    "Should include committed file"
  );
  assert!(
    files.contains(&"changed.txt".to_string()),
    "Should include changed file"
  );
}

#[test]
fn a_file_created_after_the_index_was_built_appears_when_the_index_is_built_again() {
  let temp_dir = TempDir::new().expect("Failed to create temp dir");
  let repo_path = setup_jj_repo(&temp_dir);
  TestRepo::write_workspace_file(&repo_path, "lib.rs", "").unwrap();

  let before = treq_lib::file_indexer::FileIndex::build(&repo_path).expect("index");
  assert!(before.search("agent_made", 10).is_empty());

  TestRepo::write_workspace_file(&repo_path, "src/agent_made.rs", "fn main() {}\n").unwrap();

  let after = treq_lib::file_indexer::FileIndex::build(&repo_path).expect("index");
  assert_eq!(after.search("agent_made", 10), vec!["src/agent_made.rs"]);
  // The index built before keeps what it had.
  assert!(before.search("agent_made", 10).is_empty());
}
