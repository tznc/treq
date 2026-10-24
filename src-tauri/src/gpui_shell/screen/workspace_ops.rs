//! What GPUI does itself that React's Dashboard and `ShowWorkspace`
//! handlers did: run checks, create pull requests, retarget, queue for the
//! merge queue, and delete or archive workspaces. Each reports with a toast
//! and tells React which caches to refresh (`NativeEvent`).

use super::{
  github_data::GithubHost, merge_queue_data::MergeQueueHost, workspace_data::WorkspaceHost,
};
use crate::{
  core::{
    self,
    account::{self, QueueAction},
    supabase::ApiError,
  },
  gpui_shell::{
    confirm::Question,
    native::{Backend, NativeEvent, ToastTone},
  },
};

/// What the workspace actions ask of the backend; faked in tests. All but
/// the `WorkspaceHost` methods block, so they are called off the main
/// thread.
pub trait WorkspaceActionsHost: GithubHost + MergeQueueHost {
  /// Adds the branch to, or takes it off, the merge queue.
  fn queue(&self, full_name: &str, branch: &str, action: QueueAction) -> Result<(), ApiError>;
  fn delete_workspace(&self, repo_path: &str, workspace_id: i64) -> Result<(), String>;
  fn archive_workspace(&self, repo_path: &str, workspace_id: i64) -> Result<(), String>;
}

impl WorkspaceActionsHost for Backend {
  fn queue(&self, full_name: &str, branch: &str, action: QueueAction) -> Result<(), ApiError> {
    let cloud = crate::commands::cloud::owned_cloud(self.app())
      .ok_or_else(|| ApiError::new("no_account", "The account is not available in this window."))?;
    crate::gpui_shell::block_on(account::enqueue(&cloud.supabase, full_name, branch, action))
  }

  fn delete_workspace(&self, repo_path: &str, workspace_id: i64) -> Result<(), String> {
    if let Some(remote) = self.remote(repo_path) {
      return remote.delete_workspace(workspace_id);
    }
    core::delete_workspace_removing_directory_later(repo_path, workspace_id)
  }

  fn archive_workspace(&self, repo_path: &str, workspace_id: i64) -> Result<(), String> {
    if let Some(remote) = self.remote(repo_path) {
      return remote.archive_workspace(workspace_id);
    }
    core::archive_workspace_removing_directory_later(repo_path, workspace_id)
  }
}

pub fn toast(host: &dyn WorkspaceHost, tone: ToastTone, title: &str, description: &str) {
  WorkspaceHost::notify(
    host,
    NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
      action: None,
    },
  );
}

/// The workspace a removal is for.
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
  pub repo_path: String,
  pub id: i64,
  pub branch: String,
  /// Its full path, whose terminals close.
  pub path: String,
}

impl Removed {
  pub fn of(workspace: &crate::local_db::Workspace) -> Self {
    Self {
      repo_path: workspace.repo_path.clone(),
      id: workspace.id,
      branch: workspace.branch_name.clone(),
      path: core::context_menu::full_workspace_path(workspace),
    }
  }
}

/// What `handleDelete` asks before [`delete`].
pub fn delete_question(workspace: &Removed) -> Question {
  Question::new(
    format!("Delete {}?", workspace.branch),
    "Its checkout is removed from disk. This can't be undone.",
    "Delete workspace",
  )
  .destructive()
}

/// `handleDelete`, once confirmed ([`delete_question`]): deletes, toasts
/// and tells React. Returns whether the workspace went, so its terminals
/// close.
pub fn delete(host: &dyn WorkspaceActionsHost, workspace: &Removed) -> bool {
  removing(host, &workspace.repo_path, vec![workspace.id]);
  match host.delete_workspace(&workspace.repo_path, workspace.id) {
    Ok(()) => {
      removed(host, &workspace.repo_path, vec![workspace.id]);
      toast(host, ToastTone::Success, "Workspace deleted", "");
      true
    }
    Err(err) => {
      restored(host, &workspace.repo_path, vec![workspace.id]);
      toast(host, ToastTone::Error, "Delete Failed", &err);
      false
    }
  }
}

/// `archiveWorkspaceMutation`: archives, toasts and tells React.
pub fn archive(host: &dyn WorkspaceActionsHost, workspace: &Removed) -> bool {
  removing(host, &workspace.repo_path, vec![workspace.id]);
  match host.archive_workspace(&workspace.repo_path, workspace.id) {
    Ok(()) => {
      removed(host, &workspace.repo_path, vec![workspace.id]);
      toast(host, ToastTone::Success, "Workspace archived", "");
      true
    }
    Err(err) => {
      restored(host, &workspace.repo_path, vec![workspace.id]);
      toast(host, ToastTone::Error, "Archive Failed", &err);
      false
    }
  }
}

/// `handleBulkArchive`: archives every workspace, even after one fails,
/// and toasts once for those archived and once for the failures. Returns
/// the ones archived.
pub fn archive_all(host: &dyn WorkspaceActionsHost, workspaces: &[Removed]) -> Vec<Removed> {
  let mut archived = Vec::new();
  let mut failures = Vec::new();
  let mut failed_ids = Vec::new();
  if let Some(first) = workspaces.first() {
    removing(
      host,
      &first.repo_path,
      workspaces.iter().map(|w| w.id).collect(),
    );
  }
  for workspace in workspaces {
    match host.archive_workspace(&workspace.repo_path, workspace.id) {
      Ok(()) => archived.push(workspace.clone()),
      Err(err) => {
        failed_ids.push(workspace.id);
        failures.push(err);
      }
    }
  }
  if let Some(first) = archived.first() {
    let n = archived.len();
    let s = if n > 1 { "s" } else { "" };
    toast(
      host,
      ToastTone::Success,
      &format!("{n} workspace{s} archived"),
      "",
    );
    removed(
      host,
      &first.repo_path,
      archived.iter().map(|w| w.id).collect(),
    );
  }
  if !failures.is_empty() {
    if let Some(first) = workspaces.first() {
      restored(host, &first.repo_path, failed_ids);
    }
    toast(
      host,
      ToastTone::Error,
      "Archive Failed",
      &failures.join("\n"),
    );
  }
  archived
}

/// The workspaces are going: their rows leave at once, ahead of the
/// removal.
fn removing(host: &dyn WorkspaceActionsHost, repo_path: &str, ids: Vec<i64>) {
  WorkspaceHost::notify(
    host,
    NativeEvent::WorkspacesRemoving {
      repo_path: repo_path.into(),
      ids,
    },
  );
}

/// The removal failed: the rows come back.
fn restored(host: &dyn WorkspaceActionsHost, repo_path: &str, ids: Vec<i64>) {
  WorkspaceHost::notify(
    host,
    NativeEvent::WorkspacesRestored {
      repo_path: repo_path.into(),
      ids,
    },
  );
}

/// The workspaces went: React refreshes and leaves them.
fn removed(host: &dyn WorkspaceActionsHost, repo_path: &str, ids: Vec<i64>) {
  WorkspaceHost::notify(
    host,
    NativeEvent::WorkspaceChanged {
      repo_path: repo_path.into(),
    },
  );
  WorkspaceHost::notify(
    host,
    NativeEvent::WorkspacesRemoved {
      repo_path: repo_path.into(),
      ids,
    },
  );
}

/// `handleMoveWorkspace`: stacks the workspace on `target`.
pub fn move_workspace(
  host: &dyn WorkspaceActionsHost,
  repo_path: &str,
  workspace: &crate::local_db::Workspace,
  target: &str,
) {
  match host.retarget(repo_path, workspace, target) {
    Ok(()) => WorkspaceHost::notify(
      host,
      NativeEvent::WorkspaceChanged {
        repo_path: repo_path.into(),
      },
    ),
    Err(err) => toast(host, ToastTone::Error, "Failed to move workspace", &err),
  }
}
