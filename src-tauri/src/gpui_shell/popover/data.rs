//! What the header popovers read from the backend; faked in tests.

use crate::{
  core,
  gpui_shell::{native::Backend, screen::WorkspaceHost},
  jj::JjBranch,
};

/// `getWorkspaceStartingPrompt`: the first prompt sent in the workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct StartingPrompt {
  pub id: i64,
  pub text: String,
}

/// One workflow file and its latest run, as `useWorkspaceChecks` lists them.
#[derive(Debug, Clone, PartialEq)]
pub struct Workflow {
  pub filename: String,
  pub name: String,
  /// The latest run's id and status (`passed`, `failed`, `running`).
  pub latest: Option<(i64, String)>,
}

/// `getWorkspaceSetupStatus`.
#[derive(Debug, Clone, PartialEq)]
pub struct Setup {
  pub configured: bool,
  /// `not_configured`, `pending`, `running`, `passed` or `failed`.
  pub status: String,
}

impl Setup {
  /// The setup script has not finished, which holds checks back.
  pub fn running(&self) -> bool {
    self.configured && matches!(self.status.as_str(), "pending" | "running")
  }
}

/// What `useWorkspaceChecks` reads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChecksData {
  pub workflows: Vec<Workflow>,
  pub trusted: bool,
  pub setup: Option<Setup>,
}

impl ChecksData {
  /// `summarizeChecks`: the latest run of each workflow rolled up, with the
  /// `running` ones overriding the stored status. Returns the state
  /// (`none`, `not-run`, `running`, `passed` or `failed`) and the counts of
  /// workflows passed and in all.
  pub fn rollup(&self, running: &[String]) -> (&'static str, usize, usize) {
    let (mut passed, mut failed, mut runs) = (0, 0, 0);
    for workflow in &self.workflows {
      let status = if running.contains(&workflow.filename) {
        Some("running")
      } else {
        workflow.latest.as_ref().map(|(_, status)| status.as_str())
      };
      match status {
        Some("passed") => passed += 1,
        Some("failed") => failed += 1,
        Some("running") => runs += 1,
        _ => {}
      }
    }
    let total = self.workflows.len();
    let state = if total == 0 {
      "none"
    } else if runs > 0 {
      "running"
    } else if failed > 0 {
      "failed"
    } else if passed == total {
      "passed"
    } else {
      "not-run"
    };
    (state, passed, total)
  }

  /// The setup script has not finished, which holds checks back.
  pub fn setup_running(&self) -> bool {
    self.setup.as_ref().is_some_and(Setup::running)
  }

  /// `canRun`.
  pub fn can_run(&self) -> bool {
    self.trusted && !self.setup_running()
  }
}

/// All but the `WorkspaceHost` methods block, so they are called off the
/// main thread.
pub trait PopoverHost: WorkspaceHost {
  fn starting_prompt(&self, repo_path: &str, workspace_id: i64) -> Option<StartingPrompt>;
  /// `listRepoBranches`.
  fn branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String>;
}

impl PopoverHost for Backend {
  fn starting_prompt(&self, repo_path: &str, workspace_id: i64) -> Option<StartingPrompt> {
    core::sessions::workspace_starting_prompt(repo_path, workspace_id)
      .map_err(|err| log::warn!("starting prompt: {err}"))
      .ok()
      .flatten()
      .map(|entry| StartingPrompt {
        id: entry.id,
        text: entry.prompt_text,
      })
  }

  fn branches(&self, repo_path: &str) -> Result<Vec<JjBranch>, String> {
    core::list_repo_branches(repo_path)
  }
}

/// `useWorkspaceChecks`'s reads for a workspace. Blocks.
pub fn read_checks(repo_path: &str, workspace_id: i64) -> ChecksData {
  let workflows = core::list_workflows_sync(repo_path)
    .unwrap_or_default()
    .into_iter()
    .map(|w| {
      let latest = core::latest_workflow_run_sync(repo_path, workspace_id, &w.filename)
        .ok()
        .flatten();
      Workflow {
        filename: w.filename,
        name: w.name,
        latest,
      }
    })
    .collect();
  ChecksData {
    workflows,
    trusted: core::repo::is_repo_trusted(repo_path),
    setup: core::get_setup_script_status_sync(repo_path, workspace_id)
      .ok()
      .map(|s| Setup {
        configured: s.configured,
        status: s.status,
      }),
  }
}
