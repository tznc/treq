//! Workflow checks GPUI runs, and which still run
//! ([`crate::core::running_checks`]): one set for the whole app, so a run
//! started from the palette shows as running in the workspace header, the
//! Checks popover and the GitHub view. Views observe [`Running`].

use tauri_plugin_gpui::gpui::{prelude::*, App, Global};

use crate::core::{
  checks::JobResult,
  running_checks::{CheckRun, ChecksTarget, RunningChecks, RunningOf},
};

/// The runs in flight, app-wide.
#[derive(Default, Clone, PartialEq)]
pub struct Running(pub RunningChecks);

impl Global for Running {}

/// Every run's key in `repo_path` (`useRunningCheckKeys`).
pub fn keys(cx: &App, repo_path: &str) -> Vec<String> {
  cx.try_global::<Running>()
    .map(|r| r.0.keys(repo_path))
    .unwrap_or_default()
}

/// What runs in one workspace (`useRunningChecksOf`).
pub fn of(cx: &App, repo_path: &str, workspace_id: i64) -> RunningOf {
  cx.try_global::<Running>()
    .map(|r| r.0.of(repo_path, workspace_id))
    .unwrap_or_default()
}

/// The result of a run.
pub type Outcome = Result<Vec<JobResult>, String>;

/// Runs `run` in `target` with `work` off the main thread, marked running
/// meanwhile, then calls `done` with what it returned. Does nothing, and
/// returns false, when the same run already runs.
pub fn start(
  cx: &mut App,
  target: ChecksTarget,
  run: CheckRun,
  work: impl FnOnce(&ChecksTarget, &CheckRun) -> Outcome + Send + 'static,
  done: impl FnOnce(Outcome, &mut App) + 'static,
) -> bool {
  let key = run.key(target.workspace_id);
  let repo = target.repo_path.clone();
  let mut running = cx.try_global::<Running>().cloned().unwrap_or_default();
  if !running.0.start(&repo, &key) {
    return false;
  }
  cx.set_global(running);
  let task = cx.background_spawn(async move { work(&target, &run) });
  cx.spawn(async move |cx| {
    let outcome = task.await;
    cx.update(move |cx| {
      let mut running = cx.try_global::<Running>().cloned().unwrap_or_default();
      running.0.finish(&repo, &key);
      cx.set_global(running);
      done(outcome, cx);
    });
  })
  .detach();
  true
}
