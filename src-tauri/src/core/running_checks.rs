//! Workflow checks started from the app and not finished yet, and how a run
//! is named and reported (`useRunningChecks`, `useChecksCommands`).
//!
//! A run is named by its key, `<workspace id>:<file>[:<job>]`, with the
//! setup script as the file `<setup>`. Keys are kept per repository, since
//! workspace ids are only unique within one.

use std::collections::BTreeSet;

use super::checks::JobResult;

/// The file part of a setup script run's key.
pub const SETUP_KEY: &str = "<setup>";

/// The workspace a set of checks runs in (`ChecksTarget`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksTarget {
  pub repo_path: String,
  pub workspace_id: i64,
  /// The workspace's full path, where the workflows run.
  pub workspace_path: String,
}

/// One thing to run in a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckRun {
  /// Every job of the workflow `.treq/workflows/<file>`.
  Workflow(String),
  /// One job of a workflow.
  Job { file: String, job: String },
  /// The repository's setup script, again.
  Setup,
}

impl CheckRun {
  /// `runKey`: how the run is named while it runs.
  pub fn key(&self, workspace_id: i64) -> String {
    match self {
      CheckRun::Workflow(file) => run_key(workspace_id, file, None),
      CheckRun::Job { file, job } => run_key(workspace_id, file, Some(job)),
      CheckRun::Setup => run_key(workspace_id, SETUP_KEY, None),
    }
  }
}

/// `runKey`.
pub fn run_key(workspace_id: i64, file: &str, job: Option<&str>) -> String {
  match job {
    Some(job) => format!("{workspace_id}:{file}:{job}"),
    None => format!("{workspace_id}:{file}"),
  }
}

/// What runs in one workspace, as the workspace header draws it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunningOf {
  /// Workflow files running, a single job of them included.
  pub files: Vec<String>,
  /// Workflow files running as a whole (`isRunning(filename)`).
  pub workflows: Vec<String>,
  /// The setup script is being re-run.
  pub setup: bool,
}

/// The runs in flight, by repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunningChecks {
  keys: BTreeSet<(String, String)>,
}

impl RunningChecks {
  /// Marks `key` running in `repo_path`; false when it already was.
  pub fn start(&mut self, repo_path: &str, key: &str) -> bool {
    self.keys.insert((repo_path.to_string(), key.to_string()))
  }

  pub fn finish(&mut self, repo_path: &str, key: &str) {
    self.keys.remove(&(repo_path.to_string(), key.to_string()));
  }

  pub fn is_running(&self, repo_path: &str, key: &str) -> bool {
    self
      .keys
      .contains(&(repo_path.to_string(), key.to_string()))
  }

  /// Every key running in `repo_path`, sorted (`useRunningCheckKeys`).
  pub fn keys(&self, repo_path: &str) -> Vec<String> {
    self
      .keys
      .iter()
      .filter(|(repo, _)| repo == repo_path)
      .map(|(_, key)| key.clone())
      .collect()
  }

  /// `useRunningChecksOf`: what runs in workspace `workspace_id`.
  pub fn of(&self, repo_path: &str, workspace_id: i64) -> RunningOf {
    let prefix = format!("{workspace_id}:");
    let keys: Vec<String> = self
      .keys(repo_path)
      .into_iter()
      .filter_map(|key| key.strip_prefix(&prefix).map(str::to_string))
      .collect();
    let mut files: Vec<String> = Vec::new();
    for key in &keys {
      let file = key.split(':').next().unwrap_or_default();
      if file != SETUP_KEY && !files.iter().any(|f| f == file) {
        files.push(file.to_string());
      }
    }
    RunningOf {
      files,
      workflows: keys
        .iter()
        .filter(|key| !key.contains(':') && *key != SETUP_KEY)
        .cloned()
        .collect(),
      setup: keys.iter().any(|key| key == SETUP_KEY),
    }
  }
}

/// Runs `run` in `target`, blocking until it finishes.
pub fn run(target: &ChecksTarget, run: &CheckRun) -> Result<Vec<JobResult>, String> {
  let ChecksTarget {
    repo_path,
    workspace_id,
    workspace_path,
  } = target;
  match run {
    CheckRun::Workflow(file) => {
      super::checks::run_workflow_sync(repo_path, file, *workspace_id, workspace_path)
    }
    CheckRun::Job { file, job } => {
      super::checks::run_workflow_job_sync(repo_path, file, job, *workspace_id, workspace_path)
        .map(|result| vec![result])
    }
    CheckRun::Setup => {
      super::checks::rerun_workspace_setup_script_sync(repo_path, *workspace_id, workspace_path)
        .map(|result| vec![result])
    }
  }
}

/// A finished run's toast (`useChecksCommands`' `report`): its title, the
/// error as description, and whether it passed.
pub fn report(label: &str, result: &Result<Vec<JobResult>, String>) -> (String, String, bool) {
  match result {
    Ok(jobs) => {
      let passed = jobs.iter().all(|job| job.success);
      let title = if passed {
        format!("{label} passed")
      } else {
        format!("{label} failed")
      };
      (title, String::new(), passed)
    }
    Err(error) => (format!("{label} could not run"), error.clone(), false),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn job(success: bool) -> JobResult {
    JobResult {
      job_id: "test".into(),
      steps: vec![],
      success,
    }
  }

  #[test]
  fn names_runs_as_react_does() {
    assert_eq!(CheckRun::Workflow("ci.yaml".into()).key(3), "3:ci.yaml");
    assert_eq!(
      CheckRun::Job {
        file: "ci.yaml".into(),
        job: "test".into()
      }
      .key(3),
      "3:ci.yaml:test"
    );
    assert_eq!(CheckRun::Setup.key(3), "3:<setup>");
  }

  #[test]
  fn marks_a_workflow_running_until_it_finishes() {
    let mut running = RunningChecks::default();
    assert!(running.start("/repo", "3:ci.yaml"));
    assert!(!running.start("/repo", "3:ci.yaml"), "already running");
    assert!(running.is_running("/repo", "3:ci.yaml"));
    running.finish("/repo", "3:ci.yaml");
    assert!(!running.is_running("/repo", "3:ci.yaml"));
  }

  #[test]
  fn reads_what_runs_in_one_workspace() {
    let mut running = RunningChecks::default();
    for key in ["3:ci.yaml", "3:lint.yaml:fmt", "3:<setup>", "4:ci.yaml"] {
      running.start("/repo", key);
    }
    running.start("/other", "3:docs.yaml");
    assert_eq!(
      running.of("/repo", 3),
      RunningOf {
        files: vec!["ci.yaml".into(), "lint.yaml".into()],
        workflows: vec!["ci.yaml".into()],
        setup: true,
      }
    );
    assert_eq!(running.of("/repo", 5), RunningOf::default());
    assert_eq!(running.keys("/other"), vec!["3:docs.yaml".to_string()]);
  }

  #[test]
  fn does_not_mistake_workspace_ids_sharing_a_prefix() {
    let mut running = RunningChecks::default();
    running.start("/repo", "31:ci.yaml");
    assert_eq!(running.of("/repo", 3), RunningOf::default());
  }

  #[test]
  fn reports_as_the_palette_does() {
    assert_eq!(
      report("Checks", &Ok(vec![job(true), job(true)])),
      ("Checks passed".into(), String::new(), true)
    );
    assert_eq!(
      report("CI", &Ok(vec![job(true), job(false)])),
      ("CI failed".into(), String::new(), false)
    );
    assert_eq!(
      report("CI", &Err("boom".into())),
      ("CI could not run".into(), "boom".into(), false)
    );
  }
}
