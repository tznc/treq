//! What the workspace dialogs (`UnifiedWorkspaceDialog`,
//! `RenameWorkspaceDialog` and the move-files confirmation) read and change
//! through the backend, and the rules they share with React: which name a
//! new branch may take, the stack a new workspace joins, and the steps
//! `useWorkspaceDialogSubmit` takes to create or move.

use crate::result_ext::ResultExt;
use std::collections::HashSet;

use serde::Deserialize;

use super::{
  changes_data::ChangesHost, code_data::apply_branch_name_pattern, merge::full_path,
  workspace_data::WorkspaceHost,
};
use crate::{
  core::{self, RenameWorkspaceResult, WorkspaceCommit, WorkspaceMoveRequest},
  gpui_shell::native::{Backend, NativeEvent, ToastTone},
  jj::{BranchStatus, JjBranch},
  local_db::Workspace,
};

/// What the workspace dialogs ask of the backend; faked in tests. Every call
/// blocks, so the dialogs make them off the main thread.
pub trait WorkspaceDialogHost: ChangesHost {
  /// `get_workspaces`.
  fn all_workspaces(&self, repo_path: &str) -> Result<Vec<Workspace>, String>;
  /// `get_workspace_status`'s `commits_ahead_of_target`.
  fn commits_ahead(
    &self,
    repo_path: &str,
    workspace_id: i64,
  ) -> Result<Vec<WorkspaceCommit>, String>;
  /// `list_repo_branches`.
  fn repo_branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String>;
  /// `jj_git_fetch_background`: starts a fetch and returns.
  fn fetch_in_background(&self, repo_path: &str);
  fn default_branch(&self, repo_path: &str) -> Result<String, String>;
  /// The home repository's checked-out branch, if any.
  fn current_branch(&self, repo_path: &str) -> Result<Option<String>, String>;
  /// `jj_check_branch_exists`.
  fn branch_status(&self, repo_path: &str, branch: &str) -> Result<BranchStatus, String>;
  fn gitignore_suggestions(&self, repo_path: &str) -> Vec<String>;
  /// `create_workspace`, with the repository's copy files and setup script.
  fn new_workspace(
    &self,
    repo_path: &str,
    branch: &str,
    source_branch: Option<&str>,
    metadata: Option<&str>,
  ) -> Result<i64, String>;
  /// `move_workspace_changes`.
  fn move_changes(
    &self,
    repo_path: &str,
    source_branch: &str,
    destination_branch: &str,
    request: WorkspaceMoveRequest,
  ) -> Result<(), String>;
  /// `set_workspace_target_branch`.
  fn retarget(&self, repo_path: &str, workspace: &Workspace, target: &str) -> Result<(), String>;
  /// `apply_stash`.
  fn apply_stash(&self, repo_path: &str, stash_id: i64, branch: &str) -> Result<(), String>;
  /// `rename_workspace`.
  fn rename_workspace(
    &self,
    repo_path: &str,
    workspace_id: i64,
    branch: &str,
    dry_run: bool,
  ) -> Result<RenameWorkspaceResult, String>;
}

impl WorkspaceDialogHost for Backend {
  fn all_workspaces(&self, repo_path: &str) -> Result<Vec<Workspace>, String> {
    if let Some(remote) = self.remote(repo_path) {
      return remote.workspaces();
    }
    core::list_workspaces(repo_path)
  }

  fn commits_ahead(
    &self,
    repo_path: &str,
    workspace_id: i64,
  ) -> Result<Vec<WorkspaceCommit>, String> {
    if let Some(remote) = self.remote(repo_path) {
      return remote
        .status(Some(workspace_id))
        .map(|s| s.commits_ahead_of_target);
    }
    core::workspace_status(repo_path, Some(workspace_id)).map(|s| s.commits_ahead_of_target)
  }

  fn repo_branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String> {
    if let Some(remote) = self.remote(repo_path) {
      return remote.branches();
    }
    core::list_repo_branches(repo_path)
  }

  fn fetch_in_background(&self, repo_path: &str) {
    // The host fetches a remote repository's branches itself.
    if self.remote(repo_path).is_some() {
      return;
    }
    let repo_path = repo_path.to_string();
    std::thread::spawn(move || {
      crate::core::repo::fetch_repo(&repo_path).warn_on_err("fetch repo");
    });
  }

  fn default_branch(&self, repo_path: &str) -> Result<String, String> {
    if self.remote(repo_path).is_some() {
      return self
        .inspect_remote(repo_path)
        .map(|i| i.default_branch)
        .ok_or_else(|| format!("Could not read the default branch of {repo_path}"));
    }
    core::get_repo_default_branch(repo_path)
  }

  fn current_branch(&self, repo_path: &str) -> Result<Option<String>, String> {
    if self.remote(repo_path).is_some() {
      return self
        .inspect_remote(repo_path)
        .map(|i| i.current_branch)
        .ok_or_else(|| format!("Could not read the current branch of {repo_path}"));
    }
    core::get_repo_current_branch(repo_path).map(|b| b.current_branch)
  }

  fn branch_status(&self, repo_path: &str, branch: &str) -> Result<BranchStatus, String> {
    if let Some(remote) = self.remote(repo_path) {
      // The host lists its branches, not whether origin has one.
      let local_exists = remote.branches()?.iter().any(|b| b.name == branch);
      return Ok(BranchStatus {
        local_exists,
        remote_exists: false,
        remote_name: None,
        remote_ref: None,
      });
    }
    crate::core::repo::repo_branch_status(repo_path, branch)
  }

  fn gitignore_suggestions(&self, repo_path: &str) -> Vec<String> {
    if self.remote(repo_path).is_some() {
      return Vec::new();
    }
    core::list_gitignored_path_suggestions(repo_path).unwrap_or_default()
  }

  fn new_workspace(
    &self,
    repo_path: &str,
    branch: &str,
    source_branch: Option<&str>,
    metadata: Option<&str>,
  ) -> Result<i64, String> {
    if let Some(remote) = self.remote(repo_path) {
      // Copy files and the setup script are local repository settings,
      // which a remote repository has none of.
      return remote.create_workspace(branch, source_branch, metadata.map(str::to_string));
    }
    let copy_files = self.with_db(|db| {
      let raw = db
        .get_repo_setting(repo_path, "included_copy_files")
        .ok()
        .flatten();
      core::parse_included_copy_files(raw.as_deref())
    });
    let metadata = core::parse_workspace_metadata(metadata);
    let workspace =
      core::create_new_workspace(repo_path, branch, source_branch, metadata, copy_files)?;
    crate::commands::workspace::spawn_setup_script_if_configured(&workspace);
    Ok(workspace.id)
  }

  fn move_changes(
    &self,
    repo_path: &str,
    source_branch: &str,
    destination_branch: &str,
    request: WorkspaceMoveRequest,
  ) -> Result<(), String> {
    self.local_only(repo_path, "Moving changes between workspaces")?;
    core::move_workspace_changes(repo_path, source_branch, destination_branch, request).map(|_| ())
  }

  fn retarget(&self, repo_path: &str, workspace: &Workspace, target: &str) -> Result<(), String> {
    if let Some(remote) = self.remote(repo_path) {
      return remote
        .update_workspace(workspace.id, Some(target), None, None)
        .map(drop);
    }
    let path = full_path(workspace);
    if !std::path::Path::new(&path).exists() {
      return Err(format!(
        "Workspace directory is missing on disk and could not be recovered from JJ state: {path}"
      ));
    }
    core::update_workspace(
      repo_path,
      workspace.id,
      core::MaybeEmptyParam::Some(target.to_string()),
      core::MaybeEmptyParam::Omitted,
    )
    .map(|_| ())
  }

  fn apply_stash(&self, repo_path: &str, stash_id: i64, branch: &str) -> Result<(), String> {
    self.local_only(repo_path, "Stashing")?;
    core::apply_stash(repo_path, stash_id, branch)
  }

  fn rename_workspace(
    &self,
    repo_path: &str,
    workspace_id: i64,
    branch: &str,
    dry_run: bool,
  ) -> Result<RenameWorkspaceResult, String> {
    self.local_only(repo_path, "Renaming a workspace")?;
    core::rename_workspace(repo_path, workspace_id, branch, dry_run)
  }
}

/// Which workspace the dialog stacks on (`defaults.sourceWorkspace`).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
  /// `undefined`: a plain create.
  #[default]
  Create,
  /// `null`: the home repository.
  Home,
  /// A workspace, as Dashboard holds it.
  #[serde(rename_all = "camelCase")]
  Workspace {
    id: i64,
    branch_name: String,
    #[serde(default)]
    target_branch: Option<String>,
  },
}

/// The dialog's right-panel tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
  #[default]
  Commits,
  Changes,
}

/// Where the new workspace goes relative to the one it stacks on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
  Before,
  #[default]
  After,
}

/// The locked stash commit (`applyStashCommit`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StashCommit {
  pub hash: String,
  pub message: String,
  pub timestamp: String,
}

/// `WorkspaceDialogDefaults`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Defaults {
  pub target_branch: Option<String>,
  pub source: Source,
  pub pre_selected_commits: Vec<String>,
  pub pre_selected_files: Vec<String>,
  pub description: Option<String>,
  pub title: Option<String>,
  pub branch_name: Option<String>,
  pub active_tab: Option<Tab>,
  pub apply_stash_id: Option<i64>,
  pub apply_stash_commit: Option<StashCommit>,
}

/// Dashboard's open `UnifiedWorkspaceDialog`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  /// Changes with each open, so each starts fresh.
  pub key: u64,
  pub defaults: Defaults,
}

/// Why `name` cannot be a new workspace's branch, as the dialog shows it
/// (`branchNameError`).
pub fn branch_name_error(name: &str, workspaces: &[Workspace]) -> Option<String> {
  let name = name.trim();
  if name.is_empty() {
    return None;
  }
  if let Err(err) = core::validate_branch_name(name) {
    let prefix = format!("Invalid branch name '{name}': ");
    let reason = err.strip_prefix(&prefix).unwrap_or(&err);
    return Some(format!("Invalid branch name: {reason}"));
  }
  workspaces
    .iter()
    .any(|w| w.branch_name == name)
    .then(|| format!("A workspace for {name} already exists"))
}

/// One row of the stack card, tip first (`NewWorkspaceChainEntry`).
#[derive(Debug, Clone, PartialEq)]
pub enum ChainEntry {
  New,
  Branch {
    branch: String,
    /// Whether a workspace has the branch; the rest are drawn muted.
    workspace: bool,
  },
}

/// `buildNewWorkspaceChain`: the stack a new workspace joins, from the tip
/// down to, but not including, `default_branch`.
pub fn new_workspace_chain(
  workspaces: &[Workspace],
  parent_branch: &str,
  position: Position,
  default_branch: &str,
) -> Vec<ChainEntry> {
  let mut below = Vec::new();
  let mut visited = HashSet::new();
  let mut branch = Some(parent_branch.to_string());
  while let Some(name) = branch.filter(|b| !b.is_empty() && b != default_branch) {
    if !visited.insert(name.clone()) {
      break;
    }
    let workspace = workspaces.iter().find(|w| w.branch_name == name);
    below.push(ChainEntry::Branch {
      branch: name.clone(),
      workspace: workspace.is_some(),
    });
    // A branch that is not a workspace yet sits on the default branch.
    branch = workspace.and_then(|w| w.target_branch.clone());
  }
  if position == Position::Before && !below.is_empty() {
    let mut chain = vec![below.remove(0), ChainEntry::New];
    chain.extend(below);
    return chain;
  }
  let mut chain = vec![ChainEntry::New];
  chain.extend(below);
  chain
}

/// The branch name the title suggests, or none for an empty title.
pub fn branch_from_title(pattern: &str, title: &str) -> String {
  if title.trim().is_empty() {
    String::new()
  } else {
    apply_branch_name_pattern(pattern, title)
  }
}

/// `parseNewlinePathsInput`.
pub fn newline_paths(input: &str) -> Option<Vec<String>> {
  let paths: Vec<String> = input
    .split('\n')
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(str::to_string)
    .collect();
  (!paths.is_empty()).then_some(paths)
}

/// `appendPathSuggestion`.
pub fn append_path_suggestion(current: &str, suggestion: &str) -> String {
  let exists = current
    .split('\n')
    .map(str::trim)
    .any(|line| line == suggestion);
  if exists {
    return current.to_string();
  }
  if current.trim().is_empty() {
    suggestion.to_string()
  } else {
    format!("{}\n{suggestion}", current.trim_end())
  }
}

fn non_empty(text: &str) -> Option<String> {
  let text = text.trim();
  (!text.is_empty()).then(|| text.to_string())
}

/// `JSON.stringify({ title, description })` with empty ones left out.
fn title_metadata(title: &str, description: &str) -> String {
  let mut map = serde_json::Map::new();
  if let Some(title) = non_empty(title) {
    map.insert("title".into(), title.into());
  }
  if let Some(description) = non_empty(description) {
    map.insert("description".into(), description.into());
  }
  serde_json::Value::Object(map).to_string()
}

/// `buildCreateMetadata`.
pub fn create_metadata(
  title: &str,
  description: &str,
  moved_files: Option<&[String]>,
  sparse_paths: &str,
  symlinked_dirs: &str,
) -> String {
  let mut map = serde_json::Map::new();
  if let Some(title) = non_empty(title) {
    map.insert("title".into(), title.into());
  }
  if let Some(description) = non_empty(description) {
    map.insert("description".into(), description.into());
  }
  if let Some(files) = moved_files {
    map.insert("moved_files".into(), files.into());
  }
  if let Some(paths) = newline_paths(sparse_paths) {
    map.insert("sparse_patterns".into(), paths.into());
  }
  if let Some(dirs) = newline_paths(symlinked_dirs) {
    map.insert("symlinked_dirs".into(), dirs.into());
  }
  serde_json::Value::Object(map).to_string()
}

/// What a submit acts on: the dialog's form when it was pressed.
#[derive(Debug, Clone, Default)]
pub struct Submit {
  pub repo_path: String,
  pub title: String,
  pub description: String,
  pub sparse_paths: String,
  pub symlinked_dirs: String,
  /// Trimmed.
  pub branch_name: String,
  pub move_to_existing: bool,
  pub source: Source,
  pub position: Position,
  pub target_branch: Option<String>,
  pub workspaces: Vec<Workspace>,
  /// The last branch check, with the name it was for.
  pub branch_check: Option<(String, BranchStatus)>,
  pub tab: Tab,
  pub selected_commits: Vec<String>,
  /// How many hunks are picked, and the files they are in.
  pub selected_hunks: usize,
  pub selected_files: Vec<String>,
  pub target_workspace_id: Option<i64>,
  pub apply_stash_id: Option<i64>,
}

/// How a submit ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Submitted {
  /// Open this workspace and close the dialog.
  Done(i64),
  /// Show `message` and keep the dialog open.
  Failed(String),
}

fn toast(host: &dyn WorkspaceDialogHost, tone: ToastTone, title: &str, description: String) {
  WorkspaceHost::notify(
    host,
    NativeEvent::Toast {
      title: title.into(),
      description,
      tone,
      action: None,
    },
  );
}

/// `useWorkspaceDialogSubmit`'s `handleSubmit`: creates, splits or moves,
/// and reports it in a toast.
pub fn submit(host: &dyn WorkspaceDialogHost, form: &Submit) -> Submitted {
  // Once the requested workspace exists, a later failure must not leave the
  // dialog open: a retry would fail on the name that is now taken.
  let mut created = None;
  match run_submit(host, form, &mut created) {
    Ok(Ok(id)) => Submitted::Done(id),
    Ok(Err(message)) => Submitted::Failed(message),
    Err(message) => match created {
      Some(id) => {
        toast(
          host,
          ToastTone::Error,
          "Workspace created with errors",
          format!(
            "{} was created, but a follow-up step failed: {message}",
            form.branch_name
          ),
        );
        Submitted::Done(id)
      }
      None => {
        toast(
          host,
          ToastTone::Error,
          "Failed to create workspace",
          message.clone(),
        );
        Submitted::Failed(message)
      }
    },
  }
}

/// The branch a split or stash stacks on: the source's target "before" it,
/// the source itself "after".
fn split_base(
  host: &dyn WorkspaceDialogHost,
  form: &Submit,
  branch_name: &str,
  target_branch: &Option<String>,
) -> Result<String, String> {
  match form.position {
    Position::Before => match target_branch {
      Some(target) => Ok(target.clone()),
      None => host.default_branch(&form.repo_path),
    },
    Position::After => Ok(branch_name.to_string()),
  }
}

/// "Before": the source now stacks on the new workspace.
fn reparent_source(
  host: &dyn WorkspaceDialogHost,
  form: &Submit,
  source_id: i64,
) -> Result<(), String> {
  if form.position != Position::Before {
    return Ok(());
  }
  let workspaces = host.all_workspaces(&form.repo_path)?;
  match workspaces.iter().find(|w| w.id == source_id) {
    Some(source) => host.retarget(&form.repo_path, source, &form.branch_name),
    None => Ok(()),
  }
}

/// `Ok(Ok(id))` done, `Ok(Err(..))` a message without a toast, `Err` a
/// failure (toasted by [`submit`]).
fn run_submit(
  host: &dyn WorkspaceDialogHost,
  form: &Submit,
  created: &mut Option<i64>,
) -> Result<Result<i64, String>, String> {
  let repo = form.repo_path.as_str();
  let branch = form.branch_name.as_str();
  let source = match &form.source {
    Source::Workspace {
      id,
      branch_name,
      target_branch,
    } => Some((*id, branch_name.clone(), target_branch.clone())),
    _ => None,
  };

  // Copy an immutable stash onto a new workspace.
  if let Some(stash_id) = form.apply_stash_id {
    let base = match &source {
      Some((_, name, target)) => Some(split_base(host, form, name, target)?),
      None => form.target_branch.clone(),
    };
    let id = host.new_workspace(
      repo,
      branch,
      base.as_deref(),
      Some(&title_metadata(&form.title, &form.description)),
    )?;
    *created = Some(id);
    host.apply_stash(repo, stash_id, branch)?;
    if let Some((source_id, ..)) = &source {
      reparent_source(host, form, *source_id)?;
    }
    toast(
      host,
      ToastTone::Success,
      "Workspace created",
      format!("Applied stash onto {branch}"),
    );
    return Ok(Ok(id));
  }

  if form.move_to_existing {
    if let (Some(target_id), Some((_, source_branch, _))) = (form.target_workspace_id, &source) {
      let Some(target) = form.workspaces.iter().find(|w| w.id == target_id) else {
        return Ok(Err("Target workspace not found".into()));
      };
      if form.tab == Tab::Commits && !form.selected_commits.is_empty() {
        host.move_changes(
          repo,
          source_branch,
          &target.branch_name,
          WorkspaceMoveRequest {
            commits: form.selected_commits.clone(),
            ..Default::default()
          },
        )?;
        toast(
          host,
          ToastTone::Success,
          "Commits moved",
          format!("Moved to workspace: {}", target.branch_name),
        );
        return Ok(Ok(target_id));
      }
      if form.tab == Tab::Changes && form.selected_hunks > 0 {
        host.move_changes(
          repo,
          source_branch,
          &target.branch_name,
          WorkspaceMoveRequest {
            files: form.selected_files.clone(),
            ..Default::default()
          },
        )?;
        toast(
          host,
          ToastTone::Success,
          "Files moved",
          format!(
            "Moved {} file(s) to {}",
            form.selected_files.len(),
            target.branch_name
          ),
        );
        return Ok(Ok(target_id));
      }
      return Ok(Err("Please select commits or files to move".into()));
    }
  }

  // Split commits or files off into a new workspace.
  if let Some((source_id, source_branch, source_target)) = &source {
    let commits = form.tab == Tab::Commits && !form.selected_commits.is_empty();
    let files = form.tab == Tab::Changes && form.selected_hunks > 0;
    if commits || files {
      let base = split_base(host, form, source_branch, source_target)?;
      let id = host.new_workspace(
        repo,
        branch,
        Some(&base),
        Some(&title_metadata(&form.title, &form.description)),
      )?;
      *created = Some(id);
      let request = if commits {
        WorkspaceMoveRequest {
          commits: form.selected_commits.clone(),
          ..Default::default()
        }
      } else {
        WorkspaceMoveRequest {
          files: form.selected_files.clone(),
          ..Default::default()
        }
      };
      host.move_changes(repo, source_branch, branch, request)?;
      reparent_source(host, form, *source_id)?;
      if commits {
        toast(
          host,
          ToastTone::Success,
          "Workspace created",
          format!(
            "Moved {} commit(s) to {branch}",
            form.selected_commits.len()
          ),
        );
      } else {
        toast(
          host,
          ToastTone::Success,
          "Workspace split",
          format!("Moved {} file(s) to {branch}", form.selected_files.len()),
        );
      }
      return Ok(Ok(id));
    }
  }

  // Create from the home repository, taking its picked files along.
  if form.source == Source::Home && form.selected_hunks > 0 {
    let metadata = create_metadata(
      &form.title,
      &form.description,
      Some(&form.selected_files),
      &form.sparse_paths,
      &form.symlinked_dirs,
    );
    let id = host.new_workspace(repo, branch, None, Some(&metadata))?;
    *created = Some(id);
    toast(
      host,
      ToastTone::Success,
      "Workspace created",
      format!(
        "Created {branch} with {} file(s) moved",
        form.selected_files.len()
      ),
    );
    return Ok(Ok(id));
  }

  if let Some((source_id, source_branch, _)) = &source {
    return create_stacked(host, form, *source_id, source_branch, created).map(Ok);
  }

  // A plain create, stacked on the target branch when one is picked.
  let metadata = create_metadata(
    &form.title,
    &form.description,
    None,
    &form.sparse_paths,
    &form.symlinked_dirs,
  );
  let remote_ref = form
    .branch_check
    .as_ref()
    .filter(|(name, status)| name == branch && status.remote_exists)
    .and_then(|(_, status)| status.remote_ref.clone());
  let id = create_on_target(
    host,
    repo,
    branch,
    remote_ref.as_deref(),
    form.target_branch.as_deref(),
    &metadata,
    created,
  )?;
  toast(
    host,
    ToastTone::Success,
    "Workspace created",
    branch.to_string(),
  );
  Ok(Ok(id))
}

/// Creates a workspace for `branch` (from `source_ref` when it is a remote
/// branch), stacked on `target` when one is picked. A target branch with no
/// workspace gets one first, unless it is checked out in the home working
/// copy. `created` holds the workspace once made, even when stacking it
/// then fails.
pub fn create_on_target(
  host: &dyn WorkspaceDialogHost,
  repo: &str,
  branch: &str,
  source_ref: Option<&str>,
  target: Option<&str>,
  metadata: &str,
  created: &mut Option<i64>,
) -> Result<i64, String> {
  let mut has_target_workspace = false;
  if let Some(target) = target {
    if host
      .all_workspaces(repo)?
      .iter()
      .any(|w| w.branch_name == target)
    {
      has_target_workspace = true;
    } else if host.current_branch(repo)?.as_deref() == Some(target) {
      // The home repo's own branch is checked out in the home working copy,
      // so it needs no workspace of its own.
      has_target_workspace = true;
    } else {
      let metadata = serde_json::json!({ "description": format!("Workspace for {target}") });
      let target_id = host.new_workspace(repo, target, None, Some(&metadata.to_string()))?;
      has_target_workspace = host.all_workspaces(repo)?.iter().any(|w| w.id == target_id);
    }
  }
  let id = host.new_workspace(repo, branch, source_ref, Some(metadata))?;
  *created = Some(id);
  if let (Some(target), true) = (target, has_target_workspace) {
    if let Some(workspace) = host.all_workspaces(repo)?.iter().find(|w| w.id == id) {
      host.retarget(repo, workspace, target)?;
    }
  }
  Ok(id)
}

/// `useCreateStackedWorkspace` with the dialog's branch name.
fn create_stacked(
  host: &dyn WorkspaceDialogHost,
  form: &Submit,
  parent_id: i64,
  parent_branch: &str,
  created: &mut Option<i64>,
) -> Result<i64, String> {
  let repo = form.repo_path.as_str();
  let branch = form.branch_name.as_str();
  let workspaces = host.all_workspaces(repo)?;
  // The parent may have been removed while the dialog was open.
  let Some(parent) = workspaces.iter().find(|w| w.id == parent_id) else {
    return Err(format!("Workspace {parent_branch} no longer exists"));
  };
  let metadata = non_empty(&form.description)
    .map(|description| serde_json::json!({ "description": description }).to_string());
  let default_branch = host.default_branch(repo)?;
  let effective_parent = match form.position {
    Position::Before => parent
      .target_branch
      .clone()
      .filter(|t| !t.is_empty())
      .unwrap_or_else(|| default_branch.clone()),
    Position::After => parent_branch.to_string(),
  };
  // No source for the default branch: naming it bases the workspace on the
  // home working copy.
  let source = (effective_parent != default_branch).then_some(effective_parent.as_str());
  let id = host.new_workspace(repo, branch, source, metadata.as_deref())?;
  *created = Some(id);
  if let Some(workspace) = host.all_workspaces(repo)?.iter().find(|w| w.id == id) {
    host.retarget(repo, workspace, &effective_parent)?;
  }
  if form.position == Position::Before {
    host.retarget(repo, parent, branch)?;
  }
  toast(
    host,
    ToastTone::Success,
    "Stacked workspace created",
    format!("Created {branch} stacked on {effective_parent}"),
  );
  Ok(id)
}
