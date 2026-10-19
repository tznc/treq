//! What the overlays read and change through the backend; faked in tests.

use crate::{core, gpui_shell::native::Backend, jj::JjBranch, local_db};

/// `useWorkspaceChecks`, as far as the palette's commands need it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaletteChecks {
  /// `(filename, name)` of each `.treq/workflows` file.
  pub workflows: Vec<(String, String)>,
  pub trusted: bool,
  /// `.treq/config.yaml` declares a setup script.
  pub setup_configured: bool,
  /// That script is pending or running, which holds checks back.
  pub setup_running: bool,
}

/// The workspace's pull request, from the PR poller's cache.
#[derive(Debug, Clone, PartialEq)]
pub struct PrLink {
  pub number: u64,
  pub url: String,
}

/// One row of the workspace picker.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceRow {
  pub id: i64,
  pub branch_name: String,
  /// The workspace's agent sessions.
  pub agents: usize,
}

/// All but `open_url` block, so they are called off the main thread.
pub trait OverlayHost: Send + Sync {
  fn checks(&self, repo_path: &str, workspace_id: i64) -> PaletteChecks;
  /// Also starts polling the repository's pull requests.
  fn pr(&self, repo_path: &str, branch: &str) -> Option<PrLink>;
  fn workspaces(&self, repo_path: &str) -> Result<Vec<WorkspaceRow>, String>;
  fn branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String>;
  fn switch_branch(&self, repo_path: &str, name: &str) -> Result<(), String>;
  fn open_url(&self, url: &str);
}

impl OverlayHost for Backend {
  fn checks(&self, repo_path: &str, workspace_id: i64) -> PaletteChecks {
    let workflows = core::list_workflows_sync(repo_path)
      .unwrap_or_default()
      .into_iter()
      .map(|w| (w.filename, w.name))
      .collect();
    let setup = core::get_setup_script_status_sync(repo_path, workspace_id).ok();
    PaletteChecks {
      workflows,
      trusted: local_db::is_repo_trusted(repo_path),
      setup_configured: setup.as_ref().is_some_and(|s| s.configured),
      setup_running: setup
        .as_ref()
        .is_some_and(|s| s.configured && matches!(s.status.as_str(), "pending" | "running")),
    }
  }

  fn pr(&self, repo_path: &str, branch: &str) -> Option<PrLink> {
    let poller = crate::pr_status::global();
    poller.watch_repo(repo_path);
    let pr = poller.get_cached(repo_path, branch).flatten()?;
    Some(PrLink {
      number: pr.number,
      url: pr.url,
    })
  }

  fn workspaces(&self, repo_path: &str) -> Result<Vec<WorkspaceRow>, String> {
    let sessions = local_db::get_sessions(repo_path).unwrap_or_default();
    Ok(
      core::list_workspaces(repo_path)?
        .into_iter()
        .map(|w| WorkspaceRow {
          agents: sessions
            .iter()
            .filter(|s| s.workspace_id == Some(w.id))
            .count(),
          id: w.id,
          branch_name: w.branch_name,
        })
        .collect(),
    )
  }

  fn branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String> {
    core::list_repo_branches(repo_path)
  }

  fn switch_branch(&self, repo_path: &str, name: &str) -> Result<(), String> {
    core::switch_repo_branch(repo_path, name).map(|_| ())
  }

  fn open_url(&self, url: &str) {
    Backend::open_url(self, url);
  }
}
