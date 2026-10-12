use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
use gix::refs::Target;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[allow(dead_code)]
pub const PASSING_WORKFLOW: &str = "
name: Passing CI
on:
  workflow_dispatch: {}
jobs:
  greet:
    name: Greet Job
    steps:
      - name: Say hello
        run: echo hello
      - name: Say world
        run: echo world
";

#[allow(dead_code)]
pub const FAILING_WORKFLOW: &str = "
name: Failing CI
on:
  workflow_dispatch: {}
jobs:
  check:
    name: Check Job
    steps:
      - name: Fail here
        run: exit 1
      - name: Never runs
        run: echo skipped
";

fn unique_remote_clone_path(parent: &Path, label: &str) -> PathBuf {
  static CLONE_COUNTER: AtomicU64 = AtomicU64::new(0);
  let seq = CLONE_COUNTER.fetch_add(1, Ordering::Relaxed);
  parent.join(format!("{label}_{seq}"))
}

fn random_default_branch_name() -> String {
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_nanos();
  format!("branch-{nanos}-{seq}")
}

#[allow(dead_code)]
pub struct TestRepo {
  pub temp_dir: TempDir,
  pub repo_path: String,
  default_branch: String,
  remote_temp_dir: Option<TempDir>,
}

#[allow(dead_code)]
impl TestRepo {
  /// Creates a new temporary Git repository for testing.
  /// Simulates cloning a git repo by initializing with proper git config.
  /// Calls `core::init()` to initialize jj and local db.
  pub fn new() -> Result<Self, String> {
    Self::create(true)
  }

  /// Creates a new temporary Git repository without jj/db initialization.
  pub fn new_without_init() -> Result<Self, String> {
    Self::create(false)
  }

  fn create(init: bool) -> Result<Self, String> {
    let temp_dir = TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let repo_path = temp_dir.path().to_string_lossy().to_string();

    // Initialize git repo via gix instead of shelling out — this cuts the
    // subprocess spawns entirely (not just to one call), which matters most
    // on Windows where process-spawn overhead dominates these setup calls.
    gix::init(&repo_path).map_err(|e| format!("Failed to init git repo: {}", e))?;

    // Configure git user (required for commits) and record the default branch in
    // local git config so get_default_branch() can discover it via the merged
    // init.defaultBranch fallback, even when HEAD moves to a feature branch and
    // there is no remote or main/master branch.
    //
    // core.autocrlf=false is set too: tests assert on exact file bytes, and
    // Windows git installs that default to core.autocrlf=true would rewrite LF
    // to CRLF on checkout.
    let default_branch = random_default_branch_name();
    Self::append_git_config(
      &repo_path,
      &format!(
        "[core]\n\tautocrlf = false\n[user]\n\tname = Test User\n\temail = test@example.com\n[init]\n\tdefaultBranch = {}\n",
        default_branch
      ),
    )?;

    // Point HEAD at the default branch before the first commit exists (equivalent
    // to `git branch -M` + `git checkout -b` on a fresh repo with an unborn HEAD).
    Self::set_head_branch(&repo_path, &default_branch)?;

    // Create initial commit (git repos need at least one commit)
    let readme_path = temp_dir.path().join("README.md");
    fs::write(&readme_path, "# Test Repository\n")
      .map_err(|e| format!("Failed to write README: {}", e))?;

    Self::gix_commit_all(&repo_path, "Initial commit")?;

    if init {
      treq_lib::core::init(&repo_path)?;
    }

    Ok(TestRepo {
      temp_dir,
      repo_path,
      default_branch,
      remote_temp_dir: None,
    })
  }

  /// Returns the repository's default branch name (set at creation time).
  pub fn default_branch(&self) -> &str {
    &self.default_branch
  }

  /// Full on-disk path for a workspace's working copy.
  pub fn workspace_full_path(&self, ws: &treq_lib::local_db::Workspace) -> String {
    self
      .workspaces_dir()
      .join(&ws.workspace_path)
      .to_string_lossy()
      .to_string()
  }

  fn remote_fixture_dir(&self) -> PathBuf {
    self
      .remote_temp_dir
      .as_ref()
      .expect("remote fixture requested for a repo without a remote")
      .path()
      .to_path_buf()
  }

  pub fn remote_path(&self) -> PathBuf {
    self.remote_fixture_dir().join("remote.git")
  }

  /// Create a workspace with no description/moved-files/source/sparse options set.
  pub fn create_workspace_simple(
    &self,
    branch_name: &str,
  ) -> Result<treq_lib::local_db::Workspace, String> {
    treq_lib::core::create_workspace(
      &self.repo_path,
      branch_name,
      Some(branch_name.to_string()),
      None,
      None,
      None,
      None,
    )
  }

  /// Create a workspace (optionally stacked on `source_branch`), write `filename` with
  /// `content`, and commit it (no push).
  pub fn create_workspace_with_commit(
    &self,
    branch_name: &str,
    filename: &str,
    content: &str,
    source_branch: Option<&str>,
  ) -> Result<treq_lib::local_db::Workspace, String> {
    let ws = treq_lib::core::create_workspace(
      &self.repo_path,
      branch_name,
      Some(branch_name.to_string()),
      None,
      source_branch,
      None,
      None,
    )?;
    let full_path = self.workspace_full_path(&ws);
    Self::write_workspace_file(&full_path, filename, content)?;
    treq_lib::core::commit_workspace(&self.repo_path, ws.id, &format!("Add {}", filename))?;
    Ok(ws)
  }

  /// Create a workspace, make a local commit, and push it to the remote
  /// (requires `with_remote()`). Fetches afterward so the home repo sees the push.
  pub fn setup_workspace_with_pushed_commit(
    &self,
    branch_name: &str,
    filename: &str,
    content: &str,
  ) -> Result<treq_lib::local_db::Workspace, String> {
    let ws = self.create_workspace_with_commit(branch_name, filename, content, None)?;
    let full_path = self.workspace_full_path(&ws);

    treq_lib::jj::jj_push(&full_path).map_err(|e| e.to_string())?;
    treq_lib::jj::jj_git_fetch(&self.repo_path).map_err(|e| e.to_string())?;

    Ok(ws)
  }

  /// Creates a test repo with a remote origin for testing remote branch operations.
  /// Calls `core::init()` to initialize jj and local db.
  pub fn with_remote() -> Result<Self, String> {
    Self::with_remote_create(true)
  }

  /// Creates a test repo with a remote origin, without jj/db initialization.
  pub fn with_remote_without_init() -> Result<Self, String> {
    Self::with_remote_create(false)
  }

  fn with_remote_create(init: bool) -> Result<Self, String> {
    let mut repo = Self::create(init)?;

    repo.remote_temp_dir =
      Some(TempDir::new().map_err(|e| format!("Failed to create remote temp dir: {}", e))?);

    // Create a "remote" repository
    // Keep remote test infrastructure under Git metadata so jj never snapshots
    // the bare repository or temporary clones as working-copy content.
    let remote_dir = repo.remote_path();
    fs::create_dir_all(&remote_dir).map_err(|e| format!("Failed to create remote dir: {}", e))?;

    let remote_path = remote_dir.to_string_lossy().to_string();
    gix::init_bare(&remote_dir).map_err(|e| format!("Failed to init bare remote: {}", e))?;

    // A bare repo has no `.git/config` of its own to inherit user identity from, and
    // `remote_commit_file`/`remote_commit_on_branch` below commit straight into it via
    // gix (which reads user.name/user.email from config, same as `repo.commit()`
    // elsewhere in this file).
    Self::append_config_at(
      &remote_dir.join("config"),
      "[user]\n\tname = Test User\n\temail = test@example.com\n",
    )?;

    // Add remote to main repo (a plain config write; gix has no high-level "remote add").
    // Git config values treat `\` as an escape character, so on Windows a raw path
    // (`C:\Users\...`) corrupts the file; forward slashes parse fine everywhere.
    Self::append_git_config(
      &repo.repo_path,
      &format!(
        "[remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
        remote_path.replace('\\', "/")
      ),
    )?;

    // Push default branch to remote. gix (this version) has no push support, so this
    // still shells out to git, same as the other genuinely network-bound operations below.
    let default_branch = repo.default_branch();
    Self::run_git(&repo.repo_path, &["push", "-u", "origin", default_branch])?;
    // `git remote set-head` only ever writes a local symbolic ref; no network call
    // involved, so this doesn't need the subprocess either.
    Self::set_symbolic_ref(
      &repo.repo_path,
      "refs/remotes/origin/HEAD",
      &format!("refs/remotes/origin/{}", default_branch),
    )?;
    // Create a remote branch with a commit for testing
    // The test expects a "feature.txt" file in the remote branch
    Self::gix_create_branch_at_head(&repo.repo_path, "feature-remote")?;
    Self::set_head_branch(&repo.repo_path, "feature-remote")?;

    let feature_file = repo.temp_dir.path().join("feature.txt");
    fs::write(&feature_file, "This is a feature from remote branch")
      .map_err(|e| format!("Failed to write feature file: {}", e))?;

    Self::gix_commit_all(&repo.repo_path, "Add feature.txt")?;

    // Push the feature branch to remote
    Self::run_git(&repo.repo_path, &["push", "-u", "origin", "feature-remote"])?;

    // Return to default branch
    Self::set_head_branch(&repo.repo_path, default_branch)?;

    // Fetch to ensure jj knows about the remote branch
    if init {
      let _ = treq_lib::jj::jj_git_fetch(&repo.repo_path);
    }

    Ok(repo)
  }

  /// Run a git command in the specified directory.
  pub fn run_git(cwd: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
      .current_dir(cwd)
      .args(args)
      .output()
      .map_err(|e| format!("Failed to execute git: {}", e))?;

    if !output.status.success() {
      return Err(format!(
        "Git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
      ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
  }

  /// Run a jj command via the real `jj` CLI binary. Only used by the `#[cfg(unix)]`
  /// differential tests in jj_lib_vs_cli_test.rs, which check the jj-lib-backed
  /// helpers below against the actual CLI on platforms where it's reliably
  /// available (see that file for why Windows is excluded). Test bodies
  /// themselves should use the jj-lib wrappers, not this.
  #[allow(dead_code)]
  pub fn run_jj_cli(cwd: &str, args: &[&str]) -> Result<String, String> {
    let jj_binary = treq_lib::binary_paths::detect_binary("jj").unwrap_or_else(|| "jj".to_string());
    let output = Command::new(jj_binary)
      .current_dir(cwd)
      .args(args)
      .output()
      .map_err(|e| format!("Failed to execute jj {:?}: {}", args, e))?;

    if !output.status.success() {
      return Err(format!(
        "jj {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
      ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
  }

  /// Import git branches into jj bookmarks before resolving a bookmark/branch name.
  /// The real `jj` CLI does this automatically on every invocation; raw jj-lib calls
  /// don't, so a bookmark set purely via git config (e.g. TestRepo's default branch)
  /// isn't resolvable as a jj revision until something imports it. Best-effort: a
  /// bare `@`/`@-` lookup has nothing to import, so failures here are ignored and
  /// left for the real resolve call to report.
  fn import_git_refs_best_effort(repo_path: &str) {
    let _ = treq_lib::jj::jj_import_remaining_git_refs(repo_path);
  }

  /// Create a new working-copy commit on top of `parent_revisions` (equivalent to
  /// `jj new <revisions...>`), via jj-lib directly (treq_lib::jj::jj_new_with_parents).
  /// Returns the new commit's short id.
  pub fn jj_new(workspace_path: &str, parent_revisions: &[&str]) -> Result<String, String> {
    treq_lib::jj::jj_new_with_parents(
      workspace_path,
      &parent_revisions
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())
  }

  /// Set a commit's description (equivalent to `jj describe -m <message> -r <change_id>`).
  pub fn jj_describe(workspace_path: &str, change_id: &str, message: &str) -> Result<(), String> {
    treq_lib::jj::jj_describe(workspace_path, change_id, message).map_err(|e| e.to_string())?;
    Ok(())
  }

  /// Finalize the working copy with `message` and start a new empty commit on top
  /// (equivalent to `jj commit -m <message>`).
  pub fn jj_commit(workspace_path: &str, message: &str) -> Result<String, String> {
    treq_lib::jj::jj_commit(workspace_path, message).map_err(|e| e.to_string())
  }

  /// Resolve `revision` to its 12-char change id (equivalent to
  /// `jj log -r <revision> --no-graph -T change_id`).
  pub fn jj_change_id(workspace_path: &str, revision: &str) -> Result<String, String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::jj_get_change_id(workspace_path, revision).map_err(|e| e.to_string())
  }

  /// Update a stale workspace's working copy (equivalent to `jj workspace update-stale`).
  pub fn jj_update_stale(workspace_path: &str) -> Result<(), String> {
    treq_lib::jj::jj_workspace_update_stale(workspace_path).map_err(|e| e.to_string())?;
    Ok(())
  }

  /// Whether the working copy has no changes relative to its parent (equivalent to
  /// checking `jj status`/`jj st` for "The working copy has no changes.").
  pub fn jj_working_copy_is_clean(workspace_path: &str) -> bool {
    treq_lib::jj::jj_is_working_copy_empty(workspace_path).unwrap_or(false)
  }

  /// Snapshot the working copy and return its commit id, failing if `workspace_path`
  /// isn't a valid jj workspace. Used where a test only needs to confirm the
  /// workspace is valid (what `jj status`/`jj st` was previously used for, ignoring
  /// its text output).
  pub fn jj_snapshot(workspace_path: &str) -> Result<String, String> {
    treq_lib::jj::jj_snapshot_working_copy(workspace_path).map_err(|e| e.to_string())
  }

  /// List the working copy's current sparse checkout patterns (equivalent to
  /// `jj sparse list`).
  pub fn jj_sparse_patterns(workspace_path: &str) -> Result<Vec<String>, String> {
    treq_lib::jj::jj_get_sparse_patterns(workspace_path).map_err(|e| e.to_string())
  }

  /// List every file tracked in `revision`'s tree (equivalent to
  /// `jj file list -r <revision>`).
  pub fn jj_files_at_revision(workspace_path: &str, revision: &str) -> Result<Vec<String>, String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::jj_list_files_at_revision(workspace_path, revision).map_err(|e| e.to_string())
  }

  /// Initialize a fresh jj repo with its own internal (non-colocated) Git backend
  /// in an empty directory (equivalent to `jj git init`, no `--colocate`).
  pub fn jj_git_init(repo_path: &str) -> Result<(), String> {
    treq_lib::jj::jj_git_init_bare(repo_path).map_err(|e| e.to_string())
  }

  /// Point a bookmark at `revision`, moving it even backwards (equivalent to
  /// `jj bookmark set <name> -r <revision> --allow-backwards`).
  pub fn jj_set_bookmark(
    workspace_path: &str,
    bookmark_name: &str,
    revision: &str,
  ) -> Result<(), String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::jj_set_bookmark(workspace_path, bookmark_name, revision)
      .map_err(|e| e.to_string())
  }

  /// Change ids for commits matching `revset`, in the revset's own order
  /// (equivalent to `jj log --no-graph -r <revset> -T 'change_id.short() ++ "\n"'`).
  /// A repeated change id across two entries means those commits diverged (jj
  /// CLI marks this with a trailing `??` in its default log output).
  pub fn jj_change_ids_in_revset(
    workspace_path: &str,
    revset: &str,
  ) -> Result<Vec<String>, String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::jj_log_revset_change_ids(workspace_path, revset).map_err(|e| e.to_string())
  }

  /// Short commit ids for commits matching `revset`, in the revset's own order
  /// (equivalent to `jj log --no-graph -r <revset> -T 'commit_id.short(12) ++ "\n"'`).
  /// Same short-id format as `JjVerifier::get_commit_id_for_rev`/`get_bookmark_commit_id`.
  pub fn jj_commit_ids_in_revset(
    workspace_path: &str,
    revset: &str,
  ) -> Result<Vec<String>, String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::jj_log_revset_commit_ids(workspace_path, revset).map_err(|e| e.to_string())
  }

  /// Local bookmarks pointing at `revision` (equivalent to `jj bookmark list` filtered
  /// to the ones jj status would show for that revision).
  pub fn jj_bookmarks_on_revision(
    workspace_path: &str,
    revision: &str,
  ) -> Result<Vec<String>, String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::get_bookmarks_on_revision(workspace_path, revision).map_err(|e| e.to_string())
  }

  /// Full log entries (description, short commit id, emptiness) for commits matching
  /// `revset`, capped at `limit` (equivalent to `jj log -r <revset> -n <limit> --no-graph`,
  /// minus jj's bookmark/graph decorations — `is_empty` mirrors its `(empty)` marker).
  pub fn jj_log_entries(
    workspace_path: &str,
    revset: &str,
    limit: usize,
  ) -> Result<Vec<treq_lib::jj::JjLogEntry>, String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::jj_log_entries(workspace_path, revset, Some(limit)).map_err(|e| e.to_string())
  }

  /// (description first line, short commit id) pairs for commits matching `revset`,
  /// capped at `limit` (equivalent to
  /// `jj log --no-graph -r <revset> -T 'description.first_line() ++ "|" ++ commit_id.short(12) ++ "\n"' -n <limit>`).
  pub fn jj_log_descriptions(
    workspace_path: &str,
    revset: &str,
    limit: Option<usize>,
  ) -> Result<Vec<(String, String)>, String> {
    Self::import_git_refs_best_effort(workspace_path);
    treq_lib::jj::jj_log_descriptions(workspace_path, revset, limit).map_err(|e| e.to_string())
  }

  /// Whether the working copy's uncommitted changes contain a diff hunk removing a
  /// line equal to `content` in any changed file (equivalent to grepping
  /// `jj diff --git` for a `-<content>` line, but checked per-file via jj-lib
  /// instead of string-matching CLI diff text).
  pub fn jj_has_revert_hunk_for_line(workspace_path: &str, content: &str) -> bool {
    let Ok(changed_files) = treq_lib::jj::jj_get_changed_files(workspace_path) else {
      return false;
    };
    changed_files.iter().any(|file| {
      let Ok(hunks) = treq_lib::jj::jj_get_file_hunks(workspace_path, &file.path, "git") else {
        return false;
      };
      hunks
        .iter()
        .any(|hunk| hunk.lines.iter().any(|line| line == &format!("-{content}")))
    })
  }

  /// Append raw INI text to a git config file at `config_path`. Used instead of
  /// `git config` because gix has no high-level "set config and persist to disk"
  /// call in its facade API.
  fn append_config_at(config_path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
      .create(true)
      .append(true)
      .open(config_path)
      .map_err(|e| format!("Failed to open git config: {}", e))?;
    file
      .write_all(content.as_bytes())
      .map_err(|e| format!("Failed to write git config: {}", e))
  }

  /// Append raw INI text to a non-bare repo's `.git/config`.
  fn append_git_config(repo_path: &str, content: &str) -> Result<(), String> {
    Self::append_config_at(&Path::new(repo_path).join(".git").join("config"), content)
  }

  /// Point ref `name` (e.g. `HEAD` or `refs/remotes/origin/HEAD`) at symbolic
  /// target `target` (e.g. `refs/heads/main`) via a gix ref-transaction. Works
  /// both on an unborn ref (before any commit exists) and to repoint an existing
  /// one. Equivalent to `git symbolic-ref name target`.
  fn set_symbolic_ref(repo_path: &str, name: &str, target: &str) -> Result<(), String> {
    let repo = gix::open(repo_path).map_err(|e| format!("Failed to open repo: {}", e))?;
    let target_name: gix::refs::FullName = target
      .try_into()
      .map_err(|e| format!("Invalid target ref name '{}': {}", target, e))?;
    let ref_name: gix::refs::FullName = name
      .try_into()
      .map_err(|e| format!("Invalid ref name '{}': {}", name, e))?;

    repo
      .edit_reference(RefEdit {
        change: Change::Update {
          log: Default::default(),
          expected: PreviousValue::Any,
          new: Target::Symbolic(target_name),
        },
        name: ref_name,
        deref: false,
      })
      .map_err(|e| format!("Failed to update ref '{}': {}", name, e))?;
    Ok(())
  }

  /// Point HEAD at `refs/heads/{branch}` (works both on an unborn HEAD, i.e.
  /// before any commit exists, and to switch branches afterward).
  fn set_head_branch(repo_path: &str, branch: &str) -> Result<(), String> {
    Self::set_symbolic_ref(repo_path, "HEAD", &format!("refs/heads/{}", branch))
  }

  /// Create `refs/heads/{branch}` pointing at the current HEAD commit (equivalent to
  /// `git branch <branch>` without switching to it).
  fn gix_create_branch_at_head(repo_path: &str, branch: &str) -> Result<(), String> {
    let repo = gix::open(repo_path).map_err(|e| format!("Failed to open repo: {}", e))?;
    let head_id = repo
      .head_id()
      .map_err(|e| format!("Failed to resolve HEAD: {}", e))?
      .detach();
    let branch_name: gix::refs::FullName = format!("refs/heads/{}", branch)
      .try_into()
      .map_err(|e| format!("Invalid branch ref name: {}", e))?;

    repo
      .edit_reference(RefEdit {
        change: Change::Update {
          log: LogChange {
            mode: RefLog::AndReference,
            force_create_reflog: false,
            message: "branch: Created".into(),
          },
          expected: PreviousValue::MustNotExist,
          new: Target::Object(head_id),
        },
        name: branch_name,
        deref: false,
      })
      .map_err(|e| format!("Failed to create branch '{}': {}", branch, e))?;
    Ok(())
  }

  /// Recursively build a git tree object from the contents of `dir`, writing blobs and
  /// trees via gix. Skips treq/jj/git's own metadata directories.
  fn write_tree_from_dir(repo: &gix::Repository, dir: &Path) -> Result<gix::ObjectId, String> {
    let mut read_entries: Vec<_> = fs::read_dir(dir)
      .map_err(|e| format!("Failed to read dir {}: {}", dir.display(), e))?
      .filter_map(|e| e.ok())
      .collect();
    read_entries.sort_by_key(|e| e.file_name());

    let mut entries = Vec::new();
    for entry in read_entries {
      let name = entry.file_name();
      let name_str = name.to_string_lossy();
      if name_str == ".git" || name_str == ".jj" || name_str == ".treq" {
        continue;
      }
      let path = entry.path();
      let file_type = entry
        .file_type()
        .map_err(|e| format!("Failed to read file type for {}: {}", path.display(), e))?;

      if file_type.is_dir() {
        let sub_tree_id = Self::write_tree_from_dir(repo, &path)?;
        entries.push(gix::objs::tree::Entry {
          mode: gix::objs::tree::EntryKind::Tree.into(),
          filename: name_str.into_owned().into(),
          oid: sub_tree_id,
        });
      } else if file_type.is_file() {
        let content =
          fs::read(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let blob_id = repo
          .write_blob(content)
          .map_err(|e| format!("Failed to write blob for {}: {}", path.display(), e))?
          .detach();

        #[cfg(unix)]
        let is_executable = {
          use std::os::unix::fs::PermissionsExt;
          entry
            .metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
        };
        #[cfg(not(unix))]
        let is_executable = false;

        let mode = if is_executable {
          gix::objs::tree::EntryKind::BlobExecutable
        } else {
          gix::objs::tree::EntryKind::Blob
        };

        entries.push(gix::objs::tree::Entry {
          mode: mode.into(),
          filename: name_str.into_owned().into(),
          oid: blob_id,
        });
      }
    }

    entries.sort();
    let tree_id = repo
      .write_object(&gix::objs::Tree { entries })
      .map_err(|e| format!("Failed to write tree for {}: {}", dir.display(), e))?
      .detach();
    Ok(tree_id)
  }

  /// Stage the entire working directory and create a commit on HEAD, mirroring
  /// `git add . && git commit -m <message>` but via gix.
  fn gix_commit_all(repo_path: &str, message: &str) -> Result<gix::ObjectId, String> {
    let repo = gix::open(repo_path).map_err(|e| format!("Failed to open repo: {}", e))?;
    let tree_id = Self::write_tree_from_dir(&repo, Path::new(repo_path))?;

    let parents: Vec<gix::ObjectId> = match repo.head_id() {
      Ok(id) => vec![id.detach()],
      Err(_) => Vec::new(),
    };

    let commit_id = repo
      .commit("HEAD", message, tree_id, parents)
      .map_err(|e| format!("Failed to create commit: {}", e))?;

    // Writing the commit via gix bypasses the git index entirely. Without syncing it
    // to the new tree, git sees every working-tree file as untracked and later `git
    // checkout` calls (used elsewhere in these tests, and by jj's colocated git backend)
    // refuse to switch branches ("would be overwritten by checkout").
    Self::sync_index_to_tree(&repo, &tree_id)?;

    Ok(commit_id.detach())
  }

  /// Rewrite `.git/index` to match `tree_id`, keeping git's view of the working
  /// directory in sync with commits made directly through gix.
  fn sync_index_to_tree(repo: &gix::Repository, tree_id: &gix::ObjectId) -> Result<(), String> {
    let mut index = repo
      .index_from_tree(tree_id)
      .map_err(|e| format!("Failed to build index from tree: {}", e))?;
    index
      .write(gix::index::write::Options::default())
      .map_err(|e| format!("Failed to write git index: {}", e))?;
    Ok(())
  }

  /// Return `base_tree`'s entries (or an empty tree if `base_tree` is `None`) with
  /// `components` resolving to a blob containing `content`, creating any
  /// intermediate subtree entries needed and leaving everything else untouched.
  /// Mirrors `git add <path>` at the tree level, without needing an index or a
  /// checked-out working copy.
  fn set_path_in_tree(
    repo: &gix::Repository,
    base_tree: Option<gix::ObjectId>,
    components: &[&str],
    content: &[u8],
  ) -> Result<gix::ObjectId, String> {
    let mut entries: Vec<gix::objs::tree::Entry> = match base_tree {
      Some(id) => {
        let tree = repo
          .find_object(id)
          .map_err(|e| format!("Failed to find tree {}: {}", id, e))?
          .try_into_tree()
          .map_err(|e| format!("Object {} is not a tree: {}", id, e))?;
        let decoded = tree
          .decode()
          .map_err(|e| format!("Failed to decode tree {}: {}", id, e))?;
        decoded
          .entries
          .iter()
          .map(|e| gix::objs::tree::Entry {
            mode: e.mode,
            filename: e.filename.to_owned(),
            oid: e.oid.to_owned(),
          })
          .collect()
      }
      None => Vec::new(),
    };

    let (name, rest) = components
      .split_first()
      .ok_or_else(|| "set_path_in_tree: empty path".to_string())?;
    let name: gix::bstr::BString = (*name).into();

    let new_entry = if rest.is_empty() {
      let blob_id = repo
        .write_blob(content)
        .map_err(|e| format!("Failed to write blob for '{}': {}", name, e))?
        .detach();
      gix::objs::tree::Entry {
        mode: gix::objs::tree::EntryKind::Blob.into(),
        filename: name.clone(),
        oid: blob_id,
      }
    } else {
      let existing_subtree = entries
        .iter()
        .find(|e| e.filename == name && e.mode.is_tree())
        .map(|e| e.oid);
      let subtree_id = Self::set_path_in_tree(repo, existing_subtree, rest, content)?;
      gix::objs::tree::Entry {
        mode: gix::objs::tree::EntryKind::Tree.into(),
        filename: name.clone(),
        oid: subtree_id,
      }
    };

    entries.retain(|e| e.filename != name);
    entries.push(new_entry);
    entries.sort();

    repo
      .write_object(&gix::objs::Tree { entries })
      .map_err(|e| format!("Failed to write tree: {}", e))
      .map(|id| id.detach())
  }

  /// Create a commit setting `relative_path` to `content` on `refs/heads/{branch}`
  /// in the repo at `repo_path`, based on the branch's current tip if it already
  /// exists there (or as a root commit otherwise).
  ///
  /// The "remote" repos in these tests are always local bare repos on the same
  /// filesystem, so writing straight into their object database and refs via gix
  /// is equivalent to — and far cheaper than — cloning, checking out, editing,
  /// and pushing through real git subprocesses.
  fn gix_commit_file_on_ref(
    repo_path: &str,
    branch: &str,
    relative_path: &str,
    content: &str,
    message: &str,
  ) -> Result<(), String> {
    let repo = gix::open(repo_path).map_err(|e| format!("Failed to open repo: {}", e))?;
    let ref_name: gix::refs::FullName = format!("refs/heads/{}", branch)
      .try_into()
      .map_err(|e| format!("Invalid branch ref name: {}", e))?;

    let (base_tree, parents) = match repo.find_reference(&ref_name) {
      Ok(r) => {
        let commit = r
          .id()
          .object()
          .map_err(|e| format!("Failed to resolve '{}': {}", branch, e))?
          .try_into_commit()
          .map_err(|e| format!("'{}' does not point at a commit: {}", branch, e))?;
        let tree_id = commit
          .tree_id()
          .map_err(|e| format!("Failed to read tree for '{}': {}", branch, e))?
          .detach();
        (Some(tree_id), vec![commit.id])
      }
      Err(_) => (None, Vec::new()),
    };

    let components: Vec<&str> = relative_path.split('/').collect();
    let tree_id = Self::set_path_in_tree(&repo, base_tree, &components, content.as_bytes())?;

    repo
      .commit(ref_name, message, tree_id, parents)
      .map_err(|e| format!("Failed to commit to '{}': {}", branch, e))?;

    Ok(())
  }

  /// Create a file in the repository.
  pub fn create_file(&self, relative_path: &str, content: &str) -> Result<PathBuf, String> {
    let file_path = Path::new(&self.repo_path).join(relative_path);
    Self::write_file_at_path(file_path.clone(), content, false)?;

    Ok(file_path)
  }

  /// Write a YAML workflow file to `.treq/workflows/{filename}` in the repo.
  pub fn write_workflow(&self, filename: &str, content: &str) -> Result<PathBuf, String> {
    self.create_file(&format!(".treq/workflows/{}", filename), content)
  }

  /// Write or append file content at an absolute path.
  fn write_file_at_path(file_path: PathBuf, content: &str, append: bool) -> Result<(), String> {
    if let Some(parent) = file_path.parent() {
      fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dirs: {}", e))?;
    }

    if append {
      use std::io::Write;
      let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .map_err(|e| format!("Failed to open file for append: {}", e))?;
      file
        .write_all(content.as_bytes())
        .map_err(|e| format!("Failed to append file: {}", e))?;
    } else {
      fs::write(&file_path, content).map_err(|e| format!("Failed to write file: {}", e))?;
    }

    Ok(())
  }

  /// Create or overwrite a file inside a workspace path.
  pub fn write_workspace_file(
    workspace_path: &str,
    relative_path: &str,
    content: &str,
  ) -> Result<PathBuf, String> {
    let file_path = Path::new(workspace_path).join(relative_path);
    Self::write_file_at_path(file_path.clone(), content, false)?;
    Ok(file_path)
  }

  /// Recursively remove a directory (e.g. `.jj` or a workspace path) from tests.
  /// Keeps `fs::remove_dir_all` out of `*_test.rs` for ast-grep `no-fs-mutation-in-test-files`.
  pub fn remove_dir_all_path(path: impl AsRef<Path>) -> Result<(), String> {
    fs::remove_dir_all(path.as_ref()).map_err(|e| e.to_string())
  }

  /// Remove a single file from tests (avoids `fs::remove_file` in `*_test.rs`).
  pub fn remove_file_path(path: impl AsRef<Path>) -> Result<(), String> {
    fs::remove_file(path.as_ref()).map_err(|e| e.to_string())
  }

  /// Create a directory and any missing parents (avoids `fs::create_dir_all` in `*_test.rs`).
  pub fn ensure_dir(path: impl AsRef<Path>) -> Result<(), String> {
    fs::create_dir_all(path.as_ref()).map_err(|e| e.to_string())
  }

  /// Write bytes to a path, creating parent directories as needed.
  pub fn write_path(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
      Self::ensure_dir(parent)?;
    }
    fs::write(path, content.as_ref()).map_err(|e| e.to_string())
  }

  /// Rename a file or directory (avoids `fs::rename` in `*_test.rs`).
  pub fn rename_path(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), String> {
    fs::rename(from.as_ref(), to.as_ref()).map_err(|e| e.to_string())
  }

  /// Set a file's modification time to `now - delta` (for age-based cleanup tests).
  pub fn set_file_modified_back(
    path: impl AsRef<Path>,
    delta: std::time::Duration,
  ) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::time::SystemTime;
    let file = OpenOptions::new()
      .write(true)
      .open(path.as_ref())
      .map_err(|e| e.to_string())?;
    let when = SystemTime::now()
      .checked_sub(delta)
      .ok_or_else(|| "clock cannot go that far back".to_string())?;
    file.set_modified(when).map_err(|e| e.to_string())
  }

  /// Copy a file (avoids `fs::copy` in `*_test.rs`).
  pub fn copy_path(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), String> {
    fs::copy(from.as_ref(), to.as_ref())
      .map(|_| ())
      .map_err(|e| e.to_string())
  }

  /// Append to a file inside a workspace path.
  pub fn append_workspace_file(
    workspace_path: &str,
    relative_path: &str,
    content: &str,
  ) -> Result<PathBuf, String> {
    let file_path = Path::new(workspace_path).join(relative_path);
    Self::write_file_at_path(file_path.clone(), content, true)?;
    Ok(file_path)
  }

  /// Create a commit with a file change.
  pub fn commit_file(
    &self,
    relative_path: &str,
    content: &str,
    message: &str,
  ) -> Result<(), String> {
    self.create_file(relative_path, content)?;
    Self::gix_commit_all(&self.repo_path, message)?;
    Ok(())
  }

  /// Write a file in a workspace and create a commit with the given message.
  pub fn commit_workspace_file(
    &self,
    workspace: &treq_lib::local_db::Workspace,
    relative_path: &str,
    content: &str,
    message: &str,
  ) -> Result<(), String> {
    let workspace_path = self.workspaces_dir().join(&workspace.workspace_path);
    let workspace_path_str = workspace_path
      .to_str()
      .ok_or_else(|| format!("workspace path is not utf-8: {}", workspace_path.display()))?;
    Self::write_workspace_file(workspace_path_str, relative_path, content)?;
    treq_lib::core::commit_workspace(&self.repo_path, workspace.id, message)?;
    Ok(())
  }

  /// Create a commit in the bare remote (requires with_remote()). Commits directly
  /// into the bare remote's object database via gix (see gix_commit_file_on_ref);
  /// the local repo is never touched.
  pub fn remote_commit_file(
    &self,
    relative_path: &str,
    content: &str,
    message: &str,
  ) -> Result<(), String> {
    Self::gix_commit_file_on_ref(
      &self.remote_path().to_string_lossy(),
      self.default_branch(),
      relative_path,
      content,
      message,
    )
  }

  /// Push a branch to remote (requires with_remote()).
  pub fn push_branch(&self, branch_name: &str) -> Result<(), String> {
    Self::run_git(&self.repo_path, &["push", "origin", branch_name])?;
    Ok(())
  }

  /// Create a commit on a specific branch in the bare remote (requires with_remote()).
  /// Commits directly into the bare remote's object database via gix, based on the
  /// branch's current tip if it exists there already (or as a root commit otherwise).
  /// The local repo is never modified — use jj_git_fetch after this to see the new
  /// remote commit.
  pub fn remote_commit_on_branch(
    &self,
    branch_name: &str,
    relative_path: &str,
    content: &str,
    message: &str,
  ) -> Result<(), String> {
    Self::gix_commit_file_on_ref(
      &self.remote_path().to_string_lossy(),
      branch_name,
      relative_path,
      content,
      message,
    )
  }

  /// Merge a target branch into another branch in a clone of the bare remote,
  /// create the merge without a description, and push it back to the remote.
  /// Returns the merge commit ID. The local repo is never modified.
  pub fn remote_undescribed_merge(
    &self,
    branch_name: &str,
    target_branch: &str,
  ) -> Result<String, String> {
    let remote_fixture_dir = self.remote_fixture_dir();
    let remote_path = self.remote_path();
    let clone_path = unique_remote_clone_path(&remote_fixture_dir, "remote_clone_merge");
    let clone_path_str = clone_path.to_string_lossy().to_string();

    Self::run_git(
      &remote_fixture_dir.to_string_lossy(),
      &[
        "clone",
        remote_path.to_str().unwrap(),
        clone_path.to_str().unwrap(),
      ],
    )?;
    Self::run_git(
      &clone_path_str,
      &["config", "user.email", "test@example.com"],
    )?;
    Self::run_git(&clone_path_str, &["config", "user.name", "Test User"])?;
    Self::run_git(&clone_path_str, &["checkout", branch_name])?;
    Self::run_git(
      &clone_path_str,
      &[
        "merge",
        "--no-ff",
        "--no-commit",
        &format!("origin/{target_branch}"),
      ],
    )?;
    Self::run_git(
      &clone_path_str,
      &["commit", "--allow-empty-message", "-m", ""],
    )?;
    Self::run_git(&clone_path_str, &["push", "origin", branch_name])?;
    Self::run_git(&clone_path_str, &["rev-parse", "HEAD"])
  }

  /// Get the path to the .treq directory.
  pub fn treq_dir(&self) -> PathBuf {
    Path::new(&self.repo_path).join(".treq")
  }

  /// Create and initialize a database for this repo.
  pub fn create_db(&self) -> Result<treq_lib::db::Database, String> {
    let treq_dir = self.treq_dir();
    std::fs::create_dir_all(&treq_dir).map_err(|e| format!("Failed to create .treq dir: {}", e))?;
    let db_path = treq_dir.join("test.db");
    let db = treq_lib::db::Database::new(db_path)
      .map_err(|e| format!("Failed to create database: {}", e))?;
    db.init()
      .map_err(|e| format!("Failed to init database: {}", e))?;
    Ok(db)
  }

  /// Get the path to the workspaces directory.
  pub fn workspaces_dir(&self) -> PathBuf {
    self.treq_dir().join("workspaces")
  }

  /// Ensure the .treq/workspaces directory exists.
  /// Call this before creating workspaces.
  pub fn ensure_workspaces_dir(&self) -> Result<(), String> {
    let dir = self.workspaces_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create workspaces dir: {}", e))
  }

  /// Check if jj is initialized for this repo.
  pub fn is_jj_initialized(&self) -> bool {
    Path::new(&self.repo_path).join(".jj").exists()
  }

  /// Check if the local database exists.
  pub fn has_local_db(&self) -> bool {
    self.treq_dir().join("local.db").exists()
  }

  /// Get the contents of .gitignore.
  pub fn read_gitignore(&self) -> Result<String, String> {
    let gitignore_path = Path::new(&self.repo_path).join(".gitignore");
    if !gitignore_path.exists() {
      return Ok(String::new());
    }
    fs::read_to_string(&gitignore_path).map_err(|e| format!("Failed to read .gitignore: {}", e))
  }
}

#[allow(dead_code)]
pub fn create_test_repo(with_remote: bool) -> Result<TestRepo, String> {
  if with_remote {
    TestRepo::with_remote()
  } else {
    TestRepo::new()
  }
}

#[allow(dead_code)]
pub fn write_test_file(
  base_path: &str,
  relative_path: &str,
  content: &str,
  append: bool,
) -> Result<String, String> {
  let file_path = if append {
    TestRepo::append_workspace_file(base_path, relative_path, content)?
  } else {
    TestRepo::write_workspace_file(base_path, relative_path, content)?
  };
  Ok(file_path.to_string_lossy().to_string())
}

/// Helpers for verifying jj state
pub struct JjVerifier;

#[allow(dead_code)]
impl JjVerifier {
  /// Get list of jj workspaces, including `default` (equivalent to `jj workspace list`,
  /// name column only). Not `list_jj_workspaces`, which filters `default` out for
  /// product purposes — tests here check the raw jj-level workspace count/membership.
  pub fn list_workspaces(repo_path: &str) -> Result<Vec<String>, String> {
    treq_lib::jj::list_all_workspace_names(repo_path).map_err(|e| e.to_string())
  }

  /// Get (description, commit id) pairs for the last `limit` commits in `@`'s
  /// ancestry (equivalent to `jj log -n <limit> --no-graph`, minus jj's own
  /// graph/bookmark/`(empty)` decorations — see jj_log_entries for those).
  pub fn get_log(workspace_path: &str, limit: usize) -> Result<Vec<(String, String)>, String> {
    TestRepo::jj_log_descriptions(workspace_path, "::@", Some(limit))
  }

  /// Resolve a revset to a single commit id, or `None` if it doesn't resolve.
  pub fn get_commit_id_for_rev(workspace_path: &str, rev: &str) -> Result<Option<String>, String> {
    TestRepo::import_git_refs_best_effort(workspace_path);
    Ok(treq_lib::jj::jj_get_commit_id(workspace_path, rev).ok())
  }

  /// Get the description of `@-` (equivalent to
  /// `jj log -n 1 --no-graph -r @- -T description`).
  pub fn get_log_previous_commit(workspace_path: &str) -> Result<String, String> {
    treq_lib::jj::jj_get_commit_description(workspace_path, "@-").map_err(|e| e.to_string())
  }

  /// Get list of all local bookmarks in the repo (equivalent to
  /// `jj bookmark list --all`, local names only).
  pub fn list_bookmarks(repo_path: &str) -> Result<Vec<String>, String> {
    TestRepo::import_git_refs_best_effort(repo_path);
    Ok(
      treq_lib::jj::get_branches(repo_path)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|b| b.name)
        .collect(),
    )
  }

  /// Return the commit id the bookmark currently points to, or None if the bookmark doesn't resolve.
  pub fn get_bookmark_commit_id(repo_path: &str, bookmark: &str) -> Result<Option<String>, String> {
    TestRepo::import_git_refs_best_effort(repo_path);
    Ok(treq_lib::jj::jj_get_commit_id(repo_path, bookmark).ok())
  }

  /// Check if jj working copy has changes (is dirty).
  pub fn has_changes(workspace_path: &str) -> Result<bool, String> {
    Ok(!treq_lib::jj::jj_is_working_copy_empty(workspace_path).map_err(|e| e.to_string())?)
  }

  /// Snapshot the working copy and confirm it produced a commit (equivalent to
  /// checking `jj status` returned non-empty output).
  pub fn get_status(workspace_path: &str) -> Result<String, String> {
    TestRepo::jj_snapshot(workspace_path)
  }

  /// Check if a file exists in the jj working copy
  pub fn file_exists_in_workspace(workspace_path: &str, file_path: &str) -> bool {
    Path::new(workspace_path).join(file_path).exists()
  }
}
