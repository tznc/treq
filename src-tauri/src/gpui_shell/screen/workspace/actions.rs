//! The workspace header's actions GPUI runs itself, which React's
//! `useWorkspaceChecks`, `CreatePrButtonGroup`, `ViewPrButton`,
//! `handleTargetBranchSelect` and Dashboard's `handleDelete` ran: workflow
//! checks, pull requests, retargeting and the More menu's Reset to remote and Delete Workspace.

use crate::result_ext::ResultExt;
use tauri_plugin_gpui::gpui::{prelude::*, App, Context};

use super::{
  super::{
    github_data::{compare_pr_url, conventional_pr_title},
    workspace_data::{needs_push, Pending},
    workspace_ops::{self, Removed},
    ScreenAction,
  },
  WorkspaceView,
};
use crate::{
  core::{
    context_menu::{self, MenuAction},
    running_checks::{CheckRun, ChecksTarget},
    RemoteSyncStatus,
  },
  gpui_shell::{
    checks_run,
    confirm::{self, Question},
    menus,
    native::{self, NativeEvent, OpenRequest, ToastAction, ToastTone},
    popup::PopupRequest,
  },
};

/// Header actions GPUI runs while they are in flight.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Busy {
  pub creating_pr: bool,
  pub retargeting: bool,
}

impl WorkspaceView {
  /// Runs `action` here when it is one of the header's own; false leaves it
  /// to React (as for a remote repository, whose flows React runs).
  pub(super) fn act(&mut self, action: &ScreenAction, cx: &mut Context<Self>) -> bool {
    if !self.actions.acts_locally(&self.route.repo_path) {
      return false;
    }
    match action {
      ScreenAction::Press { id } => match id.as_str() {
        "workspace.checksRunAll" => self.run_all_checks(cx),
        "workspace.checksTrust" => self.trust_checks(cx),
        "workspace.checksRerunSetup" => self.run_check(CheckRun::Setup, cx),
        "workspace.createPr" => self.start_pr(false, cx),
        "workspace.createPrMenu" => self.create_pr_menu(cx),
        "workspace.viewPr" => self.open_pr_view(),
        "workspace.more" => self.more_menu(cx),
        "workspace.archive" => self.archive_workspace(cx),
        _ => return false,
      },
      ScreenAction::Field { field, value } => match (field.as_str(), value.as_str()) {
        ("workspace.checksRun", Some(file)) => self.run_check(CheckRun::Workflow(file.into()), cx),
        ("workspace.target", Some(branch)) => self.retarget(branch.to_string(), cx),
        _ => return false,
      },
      _ => return false,
    }
    true
  }

  /// The workspace the checks run in.
  fn checks_target(&self) -> Option<ChecksTarget> {
    let workspace = self.loaded.as_ref()?.workspace.as_ref()?;
    Some(ChecksTarget {
      repo_path: workspace.repo_path.clone(),
      workspace_id: workspace.id,
      workspace_path: context_menu::full_workspace_path(workspace),
    })
  }

  /// The repository the workspace's runs are kept under.
  pub(super) fn checks_repo(&self) -> String {
    self
      .loaded
      .as_ref()
      .and_then(|l| l.workspace.as_ref())
      .map_or_else(|| self.route.repo_path.clone(), |w| w.repo_path.clone())
  }

  fn run_check(&mut self, run: CheckRun, cx: &mut Context<Self>) {
    let Some(target) = self.checks_target() else {
      return;
    };
    let (host, view) = (self.actions.clone(), cx.weak_entity());
    let repo_path = target.repo_path.clone();
    checks_run::start(
      cx,
      target,
      run,
      move |target, run| host.run_checks(target, run),
      move |_, cx| {
        view
          .update(cx, |view, _| {
            view
              .host
              .notify(NativeEvent::WorkspaceChanged { repo_path });
          })
          .ignore_closed();
      },
    );
  }

  /// `runAll`: every workflow at once.
  fn run_all_checks(&mut self, cx: &mut Context<Self>) {
    let files: Vec<String> = self
      .loaded
      .as_ref()
      .and_then(|l| l.checks.as_ref())
      .map(|c| c.workflows.iter().map(|w| w.filename.clone()).collect())
      .unwrap_or_default();
    for file in files {
      self.run_check(CheckRun::Workflow(file), cx);
    }
  }

  fn trust_checks(&mut self, cx: &mut Context<Self>) {
    let (host, repo) = (self.actions.clone(), self.checks_repo());
    cx.spawn(async move |this, cx| {
      let trusted = cx
        .background_spawn(async move { host.trust_repo(&repo) })
        .await;
      this
        .update(cx, |view, cx| {
          if let Err(err) = trusted {
            log::warn!("failed to trust the repository: {err}");
          }
          view.load(cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// `ViewPrButton`'s click: the pull request in Treq's PR view.
  fn open_pr_view(&self) {
    if let Some(Some(pr)) = &self.pr.pr {
      self.host.notify(NativeEvent::Open {
        what: OpenRequest::PrReview {
          number: pr.number,
          state: pr.state.clone(),
        },
      });
    }
  }

  /// Opens `items` below the header button `key`, running picks with `run`.
  fn dropdown(
    &self,
    key: &str,
    items: Vec<context_menu::MenuItem>,
    run: impl Fn(&mut Self, MenuAction, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let Some(anchor) = self.bounds.borrow().get(key).copied() else {
      return;
    };
    let view = cx.entity().downgrade();
    let run = std::rc::Rc::new(run);
    let host = self.actions.clone();
    let entries = menus::entries(
      items,
      std::rc::Rc::new(move |action, cx: &mut App| {
        let Some(action) = menus::run_common(action, &*host, cx) else {
          return;
        };
        let run = run.clone();
        view
          .update(cx, |view, cx| run(view, action, cx))
          .ignore_closed();
      }),
    );
    (self.popups)(
      PopupRequest::ShowDropdown {
        entries,
        anchor,
        dark: self.dark,
      },
      cx,
    );
  }

  /// The More menu: Reset to remote and Delete Workspace.
  fn more_menu(&mut self, cx: &mut Context<Self>) {
    let Some(loaded) = self.loaded.as_ref() else {
      return;
    };
    let Some(workspace) = loaded.workspace.clone() else {
      return;
    };
    let ahead = match loaded.status.as_ref().map(|s| &s.remote_sync) {
      Some(RemoteSyncStatus::NotOnRemote) | None => None,
      Some(RemoteSyncStatus::Ahead { count }) => Some(*count),
      Some(RemoteSyncStatus::Diverged { ahead, .. }) => Some(*ahead),
      Some(_) => Some(0),
    };
    let reset = ahead.map(|ahead| {
      workspace_ops::reset_question(&workspace.branch_name, ahead, loaded.uncommitted_count)
    });
    let items =
      context_menu::workspace_more_items(&workspace.repo_path, workspace.id, reset.is_some());
    let removed = Removed::of(&workspace);
    self.dropdown(
      "workspace-more",
      items,
      move |view, action, cx| match action {
        MenuAction::ResetToRemote => {
          if let Some(question) = reset.clone() {
            view.reset_to_remote(question, removed.id, cx);
          }
        }
        MenuAction::DeleteWorkspace { .. } => view.delete_workspace(removed.clone(), cx),
        _ => {}
      },
      cx,
    );
  }

  /// Asks, then resets the branch to `origin`. The header's Sync spins
  /// meanwhile, and its other actions wait.
  fn reset_to_remote(&mut self, question: Question, workspace_id: i64, cx: &mut Context<Self>) {
    confirm::ask(
      &mut self.confirm,
      question,
      move |view, cx| {
        let repo = view.route.repo_path.clone();
        view.run(Pending::Reset, cx, move |host| {
          host.reset_to_remote(&repo, workspace_id)
        });
      },
      cx,
    );
  }

  /// The confirmation's slot.
  pub(super) fn confirm_slot(view: &mut Self) -> &mut Option<confirm::Pending<Self>> {
    &mut view.confirm
  }

  /// Asks, then deletes the workspace.
  fn delete_workspace(&mut self, workspace: Removed, cx: &mut Context<Self>) {
    let question = workspace_ops::delete_question(&workspace);
    confirm::ask(
      &mut self.confirm,
      question,
      move |view, cx| view.delete_confirmed(workspace, cx),
      cx,
    );
  }

  fn delete_confirmed(&mut self, workspace: Removed, cx: &mut Context<Self>) {
    let host = self.actions.clone();
    cx.spawn(async move |this, cx| {
      let path = workspace.path.clone();
      let gone = cx
        .background_spawn(async move { workspace_ops::delete(&*host, &workspace) })
        .await;
      if gone {
        this
          .update(cx, |view, cx| view.close_terminals(path, cx))
          .ignore_closed();
      }
    })
    .detach();
  }

  /// Archives the shown workspace, as the sidebar's Archive Workspace does.
  fn archive_workspace(&mut self, cx: &mut Context<Self>) {
    let Some(workspace) = self.loaded.as_ref().and_then(|l| l.workspace.as_ref()) else {
      return;
    };
    let (workspace, host) = (Removed::of(workspace), self.actions.clone());
    cx.spawn(async move |this, cx| {
      let path = workspace.path.clone();
      let gone = cx
        .background_spawn(async move { workspace_ops::archive(&*host, &workspace) })
        .await;
      if gone {
        this
          .update(cx, |view, cx| view.close_terminals(path, cx))
          .ignore_closed();
      }
    })
    .detach();
  }

  /// Closes the terminals of a workspace GPUI removed.
  pub(crate) fn close_terminals(&mut self, path: String, cx: &mut Context<Self>) {
    self.terminal_command("closeWorkspace", Some(path), cx);
  }

  /// Gives the terminal pane a command of kind `kind`.
  pub(crate) fn terminal_command(
    &mut self,
    kind: &str,
    dir: Option<String>,
    cx: &mut Context<Self>,
  ) {
    if let Some((pane, _)) = &self.terminal {
      pane.update(cx, |pane, cx| pane.run_later(kind, dir, cx));
    }
  }

  fn create_pr_menu(&mut self, cx: &mut Context<Self>) {
    self.dropdown(
      "workspace-create-pr-menu",
      context_menu::create_pr_items(),
      |view, action, cx| match action {
        MenuAction::CreatePr { draft } => view.start_pr(draft, cx),
        MenuAction::CreatePrManually => view.create_pr_manually(cx),
        _ => {}
      },
      cx,
    );
  }

  /// What a pull request for the workspace is made from: the repository's
  /// `owner/repo`, the branch, its target and the title.
  fn pr_request(&self) -> Option<PrRequest> {
    let loaded = self.loaded.as_ref()?;
    let workspace = loaded.workspace.as_ref()?;
    let remote = loaded.remote.as_ref()?;
    Some(PrRequest {
      repo_path: self.route.repo_path.clone(),
      workspace_id: workspace.id,
      owner: remote.owner.clone(),
      repo: remote.repo.clone(),
      full_name: remote.full_name.clone(),
      branch: workspace.branch_name.clone(),
      base: workspace
        .target_branch
        .clone()
        .unwrap_or_else(|| self.route.default_target().to_string()),
      title: conventional_pr_title(&workspace.title, &workspace.branch_name),
      body: workspace.description.clone().unwrap_or_default(),
      push: needs_push(loaded),
    })
  }

  /// `useCreateWorkspacePr`: pushes first when the remote lacks the
  /// branch's commits, then `gh pr create`.
  pub(super) fn start_pr(&mut self, draft: bool, cx: &mut Context<Self>) {
    if self.busy.creating_pr {
      return;
    }
    let Some(request) = self.pr_request() else {
      return;
    };
    self.busy.creating_pr = true;
    self.rebuild(cx);
    let host = self.actions.clone();
    cx.spawn(async move |this, cx| {
      let created = {
        let (host, request) = (host.clone(), request.clone());
        cx.background_spawn(async move {
          if request.push {
            host.push_workspace(&request.repo_path, request.workspace_id)?;
          }
          let number = host.create_pr(
            &request.full_name,
            &request.title,
            &latest_body(&*host, &request),
            &request.base,
            &request.branch,
            draft,
          )?;
          // Stays pending until the PR is cached, or a second click re-creates it.
          host.refresh_pr_status(&request.repo_path, &request.branch);
          Ok::<_, String>(number)
        })
        .await
      };
      this
        .update(cx, |view, cx| {
          view.busy.creating_pr = false;
          match created {
            Ok(number) => {
              let url = format!("https://github.com/{}/pull/{number}", request.full_name);
              let opener = host.clone();
              view.host.notify(NativeEvent::Toast {
                title: if draft {
                  "Draft PR created"
                } else {
                  "Pull request created"
                }
                .into(),
                description: format!("#{number}"),
                tone: ToastTone::Success,
                action: Some(ToastAction {
                  label: "Open in Web".into(),
                  id: native::register_toast_action(move || opener.open_url(&url)),
                }),
              });
            }
            Err(err) => view.host.notify(NativeEvent::Toast {
              title: "Failed to create PR".into(),
              description: err,
              tone: ToastTone::Error,
              action: None,
            }),
          }
          view.host.notify(NativeEvent::WorkspaceChanged {
            repo_path: request.repo_path.clone(),
          });
          view.rebuild(cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// "Create PR manually": pushes when needed, then GitHub's compare page
  /// with the title and description filled in.
  fn create_pr_manually(&mut self, cx: &mut Context<Self>) {
    if self.busy.creating_pr {
      return;
    }
    let Some(request) = self.pr_request() else {
      return;
    };
    self.busy.creating_pr = true;
    self.rebuild(cx);
    let host = self.actions.clone();
    cx.spawn(async move |this, cx| {
      let url = {
        let (host, request) = (host.clone(), request.clone());
        cx.background_spawn(async move {
          if request.push {
            host.push_workspace(&request.repo_path, request.workspace_id)?;
          }
          Ok::<_, String>(compare_pr_url(
            &request.owner,
            &request.repo,
            &request.base,
            &request.branch,
            &request.title,
            &latest_body(&*host, &request),
          ))
        })
        .await
      };
      this
        .update(cx, |view, cx| {
          view.busy.creating_pr = false;
          match url {
            Ok(url) => {
              if request.push {
                view.host.notify(NativeEvent::WorkspaceChanged {
                  repo_path: request.repo_path.clone(),
                });
              }
              view.host.open_url(&url);
            }
            Err(err) => {
              workspace_ops::toast(&*view.host, ToastTone::Error, "Failed to push branch", &err)
            }
          }
          view.rebuild(cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// `handleTargetBranchSelect`: retargets and rebases onto `branch`.
  fn retarget(&mut self, branch: String, cx: &mut Context<Self>) {
    let Some(workspace) = self.loaded.as_ref().and_then(|l| l.workspace.clone()) else {
      return;
    };
    let current = workspace
      .target_branch
      .clone()
      .unwrap_or_else(|| self.route.default_target().to_string());
    if branch == current || self.busy.retargeting {
      return;
    }
    self.busy.retargeting = true;
    self.rebuild(cx);
    let (host, repo_path) = (self.actions.clone(), self.route.repo_path.clone());
    cx.spawn(async move |this, cx| {
      let done = {
        let (host, repo_path, branch) = (host.clone(), repo_path.clone(), branch.clone());
        cx.background_spawn(async move { host.retarget(&repo_path, &workspace, &branch) })
          .await
      };
      this
        .update(cx, |view, cx| {
          view.busy.retargeting = false;
          match done {
            Ok(()) => workspace_ops::toast(
              &*view.host,
              ToastTone::Success,
              &format!("Rebased onto {branch}"),
              "",
            ),
            Err(err) => workspace_ops::toast(&*view.host, ToastTone::Error, "Rebase failed", &err),
          }
          view
            .host
            .notify(NativeEvent::WorkspaceChanged { repo_path });
          view.rebuild(cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// The route as drawn: React's, with what GPUI runs itself on top.
  pub(crate) fn drawn_route(&self, cx: &App) -> super::Route {
    let mut route = self.route.clone();
    route.creating_pr |= self.busy.creating_pr;
    route.retargeting |= self.busy.retargeting;
    if let Some(id) = route.workspace_id {
      let running = checks_run::of(cx, &self.checks_repo(), id);
      merge(&mut route.running_checks, running.files);
      merge(&mut route.running_workflows, running.workflows);
      route.setup_rerunning |= running.setup;
    }
    route
  }
}

fn merge(into: &mut Vec<String>, more: Vec<String>) {
  for item in more {
    if !into.contains(&item) {
      into.push(item);
    }
  }
}

#[derive(Debug, Clone)]
struct PrRequest {
  repo_path: String,
  workspace_id: i64,
  owner: String,
  repo: String,
  full_name: String,
  branch: String,
  base: String,
  title: String,
  body: String,
  push: bool,
}

/// Agents rewrite the description with `treq set -d` while the view is
/// open, so it is read at click time (`latestBody`).
fn latest_body(host: &dyn workspace_ops::WorkspaceActionsHost, request: &PrRequest) -> String {
  super::super::code_data::CodeHost::workspaces(host, &request.repo_path)
    .ok()
    .and_then(|list| list.into_iter().find(|w| w.id == request.workspace_id))
    .map_or_else(
      || request.body.clone(),
      |w| w.description.unwrap_or_default(),
    )
}
