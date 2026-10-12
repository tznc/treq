use crate::common::TestRepo;
use std::fs;
use std::path::Path;

/// symlinked_dirs creates a symlink in the workspace pointing at the home repo path.
#[test]
fn symlink_included_dirs_links_directory_to_home_repo() {
  let temp = tempfile::TempDir::new().expect("temp dir");
  let home = temp.path().join("home");
  let workspace = temp.path().join("workspace");
  fs::create_dir_all(home.join("node_modules/pkg")).expect("create source dir");
  fs::write(
    home.join("node_modules/pkg/index.js"),
    "module.exports = 1;\n",
  )
  .expect("write source file");
  fs::create_dir_all(&workspace).expect("create workspace");

  treq_lib::core::symlink_included_dirs(
    home.to_str().unwrap(),
    workspace.to_str().unwrap(),
    &["node_modules".to_string()],
  )
  .expect("symlink should succeed");

  let link = workspace.join("node_modules");
  assert!(
    link
      .symlink_metadata()
      .expect("link metadata")
      .file_type()
      .is_symlink(),
    "node_modules should be a symlink"
  );
  let target = fs::read_link(&link).expect("read link");
  assert_eq!(
    target.canonicalize().unwrap_or(target),
    home.join("node_modules").canonicalize().unwrap()
  );
  assert_eq!(
    fs::read_to_string(link.join("pkg/index.js")).expect("read through symlink"),
    "module.exports = 1;\n"
  );
}

/// Missing patterns are skipped, same as copy_included_files.
#[test]
fn symlink_included_dirs_skips_missing_paths() {
  let temp = tempfile::TempDir::new().expect("temp dir");
  let home = temp.path().join("home");
  let workspace = temp.path().join("workspace");
  fs::create_dir_all(&home).expect("create home");
  fs::create_dir_all(&workspace).expect("create workspace");

  treq_lib::core::symlink_included_dirs(
    home.to_str().unwrap(),
    workspace.to_str().unwrap(),
    &["node_modules".to_string(), "vendor/cache".to_string()],
  )
  .expect("missing patterns should be skipped");

  assert!(!workspace.join("node_modules").exists());
  assert!(!workspace.join("vendor").exists());
}

/// Nested relative paths get parent dirs created before the symlink.
#[test]
fn symlink_included_dirs_creates_parent_dirs_for_nested_path() {
  let temp = tempfile::TempDir::new().expect("temp dir");
  let home = temp.path().join("home");
  let workspace = temp.path().join("workspace");
  fs::create_dir_all(home.join("vendor/cache")).expect("create source");
  fs::write(home.join("vendor/cache/data.bin"), "cached").expect("write");
  fs::create_dir_all(&workspace).expect("create workspace");

  treq_lib::core::symlink_included_dirs(
    home.to_str().unwrap(),
    workspace.to_str().unwrap(),
    &["vendor/cache".to_string()],
  )
  .expect("nested symlink should succeed");

  let link = workspace.join("vendor/cache");
  assert!(link
    .symlink_metadata()
    .expect("link metadata")
    .file_type()
    .is_symlink());
  assert_eq!(
    fs::read_to_string(link.join("data.bin")).expect("read"),
    "cached"
  );
}

/// Files can also be symlinked (e.g. a large local artifact).
#[test]
fn symlink_included_dirs_links_files() {
  let temp = tempfile::TempDir::new().expect("temp dir");
  let home = temp.path().join("home");
  let workspace = temp.path().join("workspace");
  fs::create_dir_all(&home).expect("create home");
  fs::write(home.join(".env.local"), "SECRET=1\n").expect("write");
  fs::create_dir_all(&workspace).expect("create workspace");

  treq_lib::core::symlink_included_dirs(
    home.to_str().unwrap(),
    workspace.to_str().unwrap(),
    &[".env.local".to_string()],
  )
  .expect("file symlink should succeed");

  let link = workspace.join(".env.local");
  assert!(link
    .symlink_metadata()
    .expect("link metadata")
    .file_type()
    .is_symlink());
  assert_eq!(fs::read_to_string(&link).expect("read"), "SECRET=1\n");
}

/// create_workspace applies symlinked_dirs after checkout.
#[test]
fn create_workspace_symlinks_configured_dirs_from_home_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file(".gitignore", "node_modules/\n", "add gitignore")
    .expect("Failed to commit .gitignore");
  fs::create_dir_all(Path::new(&repo.repo_path).join("node_modules/left-pad"))
    .expect("create node_modules");
  fs::write(
    Path::new(&repo.repo_path).join("node_modules/left-pad/index.js"),
    "module.exports = 'pad';\n",
  )
  .expect("write package");

  let workspace = treq_lib::core::create_workspace_with_symlinked_dirs(
    &repo.repo_path,
    "feat/with-symlink",
    None,
    None,
    None,
    None,
    None,
    Some(vec!["node_modules".to_string()]),
  )
  .expect("Failed to create workspace with symlinks");

  let link = repo
    .workspaces_dir()
    .join(&workspace.workspace_path)
    .join("node_modules");
  assert!(
    link
      .symlink_metadata()
      .expect("link metadata")
      .file_type()
      .is_symlink(),
    "workspace node_modules should be a symlink to the home repo"
  );
  assert_eq!(
    fs::read_to_string(link.join("left-pad/index.js")).expect("read through symlink"),
    "module.exports = 'pad';\n"
  );
}

/// Symlinks and copies can be combined — copy small files, symlink heavy dirs.
#[test]
fn create_workspace_can_copy_and_symlink_together() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .commit_file(".gitignore", ".env\nnode_modules/\n", "add gitignore")
    .expect("Failed to commit .gitignore");
  repo
    .create_file(".env", "SECRET=1\n")
    .expect("Failed to create .env");
  fs::create_dir_all(Path::new(&repo.repo_path).join("node_modules/x"))
    .expect("create node_modules");
  fs::write(
    Path::new(&repo.repo_path).join("node_modules/x/a.js"),
    "export {};\n",
  )
  .expect("write");

  let workspace = treq_lib::core::create_workspace_with_symlinked_dirs(
    &repo.repo_path,
    "feat/copy-and-link",
    None,
    None,
    None,
    Some(vec![".env".to_string()]),
    None,
    Some(vec!["node_modules".to_string()]),
  )
  .expect("Failed to create workspace");

  let ws = repo.workspaces_dir().join(&workspace.workspace_path);
  assert!(ws.join(".env").is_file(), ".env should be a regular copy");
  assert!(
    !ws
      .join(".env")
      .symlink_metadata()
      .unwrap()
      .file_type()
      .is_symlink(),
    ".env must not be a symlink"
  );
  assert!(
    ws.join("node_modules")
      .symlink_metadata()
      .unwrap()
      .file_type()
      .is_symlink(),
    "node_modules should be a symlink"
  );
}
