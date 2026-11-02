//! The palette commands GPUI runs itself ([`palette_commands`]): the
//! workspace's checks (`useChecksCommands`) and the terminal pane's
//! commands. The rest go to the window model as `palette.<id>`, which opens
//! the dialog or page.

use std::{cell::RefCell, rc::Rc};

use tauri_plugin_gpui::gpui::{prelude::*, App, Context};

use super::{data::OverlayHost, Overlay, OverlayView};
use crate::{
  core::{
    checks::JobResult,
    palette_commands::{self, PaletteCommand},
    running_checks::{self, CheckRun, ChecksTarget},
  },
  gpui_shell::{
    checks_run,
    native::{NativeEvent, ToastTone},
  },
};

impl Overlay {
  /// Runs palette command `id`, then closes the palette as
  /// `closePalette` does.
  pub(super) fn run_palette(&mut self, id: &str, cx: &mut Context<Self>) {
    let command = palette_commands::parse(id);
    if let PaletteCommand::Route(id) = &command {
      // The window model runs the command and closes the palette.
      return self.press(&format!("palette.{id}"), cx);
    }
    let workspace = match &self.view {
      OverlayView::CommandPalette(route) => route.workspace.clone(),
      _ => None,
    };
    match command {
      PaletteCommand::Terminal(command) => self.host.terminal(command),
      PaletteCommand::TrustChecks => {
        if let Some(workspace) = workspace {
          let host = self.host.clone();
          cx.background_spawn(async move {
            if let Err(err) = host.trust_repo(&workspace.repo_path) {
              log::warn!("failed to trust the repository: {err}");
            }
          })
          .detach();
        }
      }
      PaletteCommand::RunAllChecks | PaletteCommand::Run(_) => {
        if let Some(workspace) = workspace {
          self.run_checks(workspace.repo_path, workspace.id, command, cx);
        }
      }
      PaletteCommand::Route(_) => {}
    }
    self.close(cx);
  }

  /// Finds where the checks run, off the main thread, then runs them.
  fn run_checks(
    &mut self,
    repo_path: String,
    workspace_id: i64,
    command: PaletteCommand,
    cx: &mut Context<Self>,
  ) {
    let workflows: Vec<(String, String)> = match &self.body {
      super::Body::Palette {
        checks: Some(checks),
        ..
      } => checks.workflows.clone(),
      _ => Vec::new(),
    };
    let host = self.host.clone();
    cx.spawn(async move |_, cx| {
      let target = {
        let host = host.clone();
        cx.background_spawn(async move { host.checks_target(&repo_path, workspace_id) })
          .await
      };
      let Some(target) = target else {
        return;
      };
      cx.update(|cx| start(host, target, command, &workflows, cx));
    })
    .detach();
  }
}

/// Runs the checks `command` names in `target`; `workflows` are the
/// palette's `(filename, name)` pairs.
fn start(
  host: std::sync::Arc<dyn OverlayHost>,
  target: ChecksTarget,
  command: PaletteCommand,
  workflows: &[(String, String)],
  cx: &mut App,
) {
  let runs: Vec<(CheckRun, Option<String>)> = match command {
    // One report for all of them, as `runAll().then(flat)`.
    PaletteCommand::RunAllChecks => workflows
      .iter()
      .map(|(file, _)| (CheckRun::Workflow(file.clone()), None))
      .collect(),
    PaletteCommand::Run(CheckRun::Workflow(file)) => {
      let name = workflows
        .iter()
        .find(|(f, _)| *f == file)
        .map_or_else(|| file.clone(), |(_, name)| name.clone());
      vec![(CheckRun::Workflow(file), Some(name))]
    }
    // `rerunSetup` reports nothing.
    PaletteCommand::Run(run) => vec![(run, None)],
    _ => return,
  };
  let all = matches!(runs.first(), Some((CheckRun::Workflow(_), None)));
  let pending = Rc::new(RefCell::new(Gathered {
    left: runs.len(),
    jobs: Ok(Vec::new()),
  }));
  for (run, label) in runs {
    let (host, done_host, gathered) = (host.clone(), host.clone(), pending.clone());
    let repo_path = target.repo_path.clone();
    let started = checks_run::start(
      cx,
      target.clone(),
      run,
      move |target, run| host.run_checks(target, run),
      move |outcome, _| {
        done_host.notify(NativeEvent::WorkspaceChanged { repo_path });
        if let Some(label) = label {
          report(&*done_host, &label, &outcome);
        } else if all {
          let mut gathered = gathered.borrow_mut();
          gathered.add(outcome);
          if gathered.left == 0 {
            report(&*done_host, "Checks", &gathered.jobs);
          }
        }
      },
    );
    if !started {
      pending.borrow_mut().left -= 1;
    }
  }
}

/// Run All's results so far.
struct Gathered {
  left: usize,
  jobs: Result<Vec<JobResult>, String>,
}

impl Gathered {
  fn add(&mut self, outcome: checks_run::Outcome) {
    self.left -= 1;
    match (&mut self.jobs, outcome) {
      (Ok(jobs), Ok(more)) => jobs.extend(more),
      (Ok(_), Err(err)) => self.jobs = Err(err),
      (Err(_), _) => {}
    }
  }
}

fn report(host: &dyn OverlayHost, label: &str, outcome: &checks_run::Outcome) {
  let (title, description, passed) = running_checks::report(label, outcome);
  host.notify(NativeEvent::Toast {
    title,
    description,
    tone: if passed {
      ToastTone::Success
    } else {
      ToastTone::Error
    },
    action: None,
  });
}
