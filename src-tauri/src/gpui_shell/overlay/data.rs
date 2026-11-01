//! What the overlays read and change through the backend; faked in tests.

use crate::{
  core::{
    self,
    checks::JobResult,
    palette_commands::TerminalCommand,
    running_checks::{CheckRun, ChecksTarget},
  },
  gpui_shell::native::{Backend, NativeEvent},
  jj::JjBranch,
};

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
  /// What the workspace is called (its title, else its branch).
  pub title: String,
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
  /// Where workspace `workspace_id`'s checks run.
  fn checks_target(&self, repo_path: &str, workspace_id: i64) -> Option<ChecksTarget>;
  /// Runs a workflow or the setup script to the end.
  fn run_checks(&self, target: &ChecksTarget, run: &CheckRun) -> Result<Vec<JobResult>, String>;
  fn trust_repo(&self, repo_path: &str) -> Result<(), String>;
  /// Tells the window what changed, or shows a toast. Never blocks.
  fn notify(&self, event: NativeEvent);
  /// Gives the window's terminal pane a command. Never blocks.
  fn terminal(&self, command: TerminalCommand);
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
      trusted: core::repo::is_repo_trusted(repo_path),
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
    let sessions = core::sessions::list_sessions(repo_path).unwrap_or_default();
    Ok(
      core::list_workspaces(repo_path)?
        .into_iter()
        .map(|w| WorkspaceRow {
          agents: sessions
            .iter()
            .filter(|s| s.workspace_id == Some(w.id))
            .count(),
          id: w.id,
          title: crate::gpui_shell::screen::code_data::display_title(&w),
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

  fn checks_target(&self, repo_path: &str, workspace_id: i64) -> Option<ChecksTarget> {
    let workspace = core::list_workspaces(repo_path)
      .ok()?
      .into_iter()
      .find(|w| w.id == workspace_id)?;
    Some(ChecksTarget {
      repo_path: workspace.repo_path.clone(),
      workspace_id,
      workspace_path: core::context_menu::full_workspace_path(&workspace),
    })
  }

  fn run_checks(&self, target: &ChecksTarget, run: &CheckRun) -> Result<Vec<JobResult>, String> {
    core::running_checks::run(target, run)
  }

  fn trust_repo(&self, repo_path: &str) -> Result<(), String> {
    core::repo::trust_repo(repo_path)?;
    Backend::notify(
      self,
      NativeEvent::WorkspaceChanged {
        repo_path: repo_path.into(),
      },
    );
    Ok(())
  }

  fn notify(&self, event: NativeEvent) {
    Backend::notify(self, event);
  }

  fn terminal(&self, command: TerminalCommand) {
    let dir = None;
    crate::gpui_shell::terminal_command(self.app(), self.label(), command.kind(), dir);
  }
}
