//! `GitHubPanel`: the repository's issues and pull requests, their detail,
//! and the forms that create them.
//!
//! GPUI reads and changes everything through `gh` and the local database
//! (see [`github_data`]). React keeps the place in the URL, so moving
//! around (a tab, a state filter, an item, the create form, leaving for a
//! workspace or the PR review) is handed back as a `github.*` press and
//! comes back as the next [`Route`]. The Merge Queue tab and the PR review
//! page are still React's: GPUI hands their routes back and React draws them.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::{zpx, DismissExt as _};
use crate::result_ext::ResultExt;
use std::{
  collections::{HashMap, HashSet},
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use gpui_kit::component::{input::Input, GlobalState};
use tauri_plugin_gpui::gpui::{
  self, deferred, div, prelude::*, App, Context, Div, FocusHandle, FontWeight, SharedString, Window,
};

use super::{
  github_data::{
    self as data, GithubHost, LogLine, PrFilters, Route, StateFilter, Tab, TreqChecks,
  },
  ActionSink, ScreenAction,
};
use crate::{
  core::{
    running_checks::{CheckRun, ChecksTarget},
    RunSummary,
  },
  github::{GhIssue, GhListPage, GhPullRequest, GitRemoteInfo, PrCiStatus},
  gpui_shell::{
    checks_run,
    form::{self, Controls, SelectOption},
    native::{register_toast_action, NativeEvent, ToastAction, ToastTone},
    popup::{self, PopupSink},
    theme::{self, Palette},
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
  local_db::Workspace,
};

mod detail;
mod new_pr;
pub(super) mod parts;

use parts::named;

/// How often the open PR's GitHub checks are read again (`usePrChecksForPr`).
const CHECKS_POLL: Duration = Duration::from_secs(30);
/// How often a pending or running setup script's status is read again.
const SETUP_POLL: Duration = Duration::from_millis(1500);

/// Pages of one list, as `useSWRInfinite` keeps them.
pub(super) struct Pages<T> {
  /// The repository and state filter the pages are for.
  key: Option<(String, StateFilter)>,
  pub(super) pages: Vec<GhListPage<T>>,
  /// The first page is loading, with nothing to show yet.
  pub(super) loading: bool,
  /// The next page is loading.
  pub(super) fetching_next: bool,
  pub(super) error: Option<String>,
  /// Bumped per load, so a stale one is dropped.
  generation: u64,
}

impl<T> Default for Pages<T> {
  fn default() -> Self {
    Self {
      key: None,
      pages: Vec::new(),
      loading: false,
      fetching_next: false,
      error: None,
      generation: 0,
    }
  }
}

impl<T> Pages<T> {
  fn has_next(&self) -> bool {
    self.pages.last().is_some_and(|p| p.has_more)
  }
}

/// Every item of the pages, the first copy of each number.
fn items<T: Clone>(pages: &Pages<T>, number: impl Fn(&T) -> u64) -> Vec<T> {
  data::unique_by_number(
    pages.pages.iter().flat_map(|p| p.items.clone()).collect(),
    number,
  )
}

/// What the open detail is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct DetailKey {
  tab: Tab,
  repo: String,
  number: u64,
}

/// A button whose action runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Busy {
  Comment,
  Close,
  Reopen,
  Draft,
  Workspace,
  Save,
  Delete,
}

/// The open issue's or pull request's detail.
#[derive(Default)]
pub(super) struct Detail {
  pub(super) key: Option<DetailKey>,
  pub(super) issue: Option<GhIssue>,
  pub(super) pr: Option<GhPullRequest>,
  pub(super) loading: bool,
  pub(super) error: Option<String>,
  pub(super) checks: Option<PrCiStatus>,
  pub(super) busy: HashSet<Busy>,
  /// The issue's title and body being edited.
  pub(super) editing: Option<(String, String)>,
  /// The delete confirmation is open.
  pub(super) confirm_delete: bool,
  /// The PR's workspace's Treq checks, once read.
  pub(super) treq: Option<TreqChecks>,
  /// Workflow files whose run history is open, and what it read.
  pub(super) history_open: HashSet<String>,
  pub(super) history: HashMap<String, Vec<RunSummary>>,
  /// Runs whose job logs are open, and the logs read, by run and job.
  pub(super) runs_open: HashSet<i64>,
  pub(super) logs: HashMap<(i64, String), Result<Vec<LogLine>, String>>,
  generation: u64,
}

/// The inline New Issue form (`CreateIssueForm`).
#[derive(Default)]
pub(super) struct NewIssue {
  pub(super) title: String,
  pub(super) body: String,
  pub(super) pending: bool,
  pub(super) error: Option<String>,
}

/// A popover or dropdown open over the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Popover {
  Filters,
  CloseIssue,
  PrType,
  CiChecks,
}

/// One of the filter popover's selects: its field, label, value and
/// `(value, label)` options.
type FilterSelect = (&'static str, &'static str, String, Vec<(String, String)>);

/// The GitHub view and what it read.
pub struct GithubView {
  pub(super) host: Arc<dyn GithubHost>,
  /// React's handlers, for moving around.
  react: ActionSink,
  pub(super) controls: Controls,
  pub(super) tips: Tooltips,
  popups: PopupSink,
  pub(super) route: Route,
  pub(super) dark: bool,
  /// `None` while reading.
  pub(super) remote: Option<Option<GitRemoteInfo>>,
  remote_for: String,
  pub(super) issues: Pages<GhIssue>,
  pub(super) prs: Pages<GhPullRequest>,
  pub(super) filters: PrFilters,
  pub(super) detail: Detail,
  /// The comment being written, kept across items of the same kind.
  pub(super) comment: String,
  /// Fields whose control must show their state again although focused.
  reset: HashSet<&'static str>,
  pub(super) new_issue: NewIssue,
  pub(super) new_pr: Option<new_pr::NewPr>,
  pub(super) popover: Option<Popover>,
  /// The comment whose reaction picker is open.
  pub(super) reaction_picker: Option<String>,
  /// The popover a press outside just closed, so the press on its own
  /// button does not open it again.
  popover_closed: Option<(Popover, Instant)>,
  pub(super) workspaces: Vec<Workspace>,
  pub(super) logs_enabled: bool,
  /// The dialog's focus, for Escape.
  dialog_focus: FocusHandle,
  dialog_focused: bool,
  /// Draws checks as running while one runs anywhere in the app.
  _running: gpui::Subscription,
}

impl HasTooltips for GithubView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl GithubView {
  pub fn new(
    route: Route,
    host: Arc<dyn GithubHost>,
    react: ActionSink,
    dark: bool,
    popups: PopupSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let view = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      let view = view.clone();
      cx.defer(move |cx| {
        view
          .update(cx, |view, cx| view.handle(action, cx))
          .ignore_closed();
      });
    });
    let mut this = Self {
      host,
      react,
      controls: Controls::new(sink),
      tips: Tooltips::new(popups.clone(), dark),
      popups,
      remote_for: route.repo_path.clone(),
      route,
      dark,
      remote: None,
      issues: Pages::default(),
      prs: Pages::default(),
      filters: PrFilters::default(),
      detail: Detail::default(),
      comment: String::new(),
      reset: HashSet::new(),
      new_issue: NewIssue::default(),
      new_pr: None,
      popover: None,
      reaction_picker: None,
      popover_closed: None,
      workspaces: Vec::new(),
      logs_enabled: false,
      dialog_focus: cx.focus_handle(),
      dialog_focused: false,
      _running: cx.observe_global::<checks_run::Running>(|_, cx| cx.notify()),
    };
    this.load_remote(cx);
    this.load_workspaces(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    self.dark = dark;
    self.tips.dark = dark;
    if self.route != route {
      let checks_changed = self.route.running_checks != route.running_checks;
      self.route = route;
      if self.route.repo_path != self.remote_for {
        self.remote_for = self.route.repo_path.clone();
        self.remote = None;
        self.load_remote(cx);
        self.load_workspaces(cx);
      }
      self.sync(cx);
      if checks_changed {
        // A run that finished wrote its results.
        self.reload_checks(cx);
      }
    }
    cx.notify();
  }

  /// The repository's `owner/repo`, once read.
  pub(super) fn repo(&self) -> Option<String> {
    self
      .remote
      .as_ref()?
      .as_ref()
      .map(|remote| remote.full_name.clone())
  }

  /// Runs `work` off the main thread, then `done` with its result.
  fn run<T: Send + 'static>(
    &self,
    cx: &mut Context<Self>,
    work: impl FnOnce(&dyn GithubHost) -> T + Send + 'static,
    done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
  ) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let out = cx
        .background_spawn(async move { work(host.as_ref()) })
        .await;
      this
        .update(cx, |view, cx| {
          done(view, out, cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn load_remote(&mut self, cx: &mut Context<Self>) {
    let repo_path = self.route.repo_path.clone();
    self.run(
      cx,
      move |host| {
        let remote = host.remote(&repo_path).unwrap_or_else(|err| {
          log::warn!("github panel: remote: {err}");
          None
        });
        (repo_path, remote, host.logs_enabled())
      },
      |view, (repo_path, remote, logs), cx| {
        if view.route.repo_path == repo_path {
          view.remote = Some(remote);
          view.logs_enabled = logs;
          view.sync(cx);
        }
      },
    );
  }

  pub(super) fn load_workspaces(&mut self, cx: &mut Context<Self>) {
    let repo_path = self.route.repo_path.clone();
    self.run(
      cx,
      move |host| (host.workspaces(&repo_path), repo_path),
      |view, (workspaces, repo_path), cx| {
        if view.route.repo_path != repo_path {
          return;
        }
        match workspaces {
          Ok(workspaces) => view.workspaces = workspaces,
          Err(err) => log::warn!("github panel: workspaces: {err}"),
        }
        view.load_treq(cx);
      },
    );
  }

  /// Loads what the route shows and nothing loaded yet.
  fn sync(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = self.repo() else {
      return;
    };
    let key = Some((repo.clone(), self.route.filter));
    match self.route.tab {
      Tab::Issues if self.issues.key != key => self.load_issues(false, cx),
      Tab::Prs if self.prs.key != key => self.load_prs(false, cx),
      _ => {}
    }
    let detail = self.route.selected.map(|number| DetailKey {
      tab: self.route.tab,
      repo,
      number,
    });
    if detail != self.detail.key {
      if detail.as_ref().map(|d| d.tab) != self.detail.key.as_ref().map(|d| d.tab) {
        self.comment.clear();
      }
      self.popover = None;
      self.detail = Detail {
        key: detail,
        generation: self.detail.generation + 1,
        ..Detail::default()
      };
      self.load_detail(cx);
    }
    let creating_pr = self.route.creating && self.route.tab == Tab::Prs;
    if !creating_pr {
      self.new_pr = None;
    } else if self.new_pr.is_none() {
      self.open_new_pr(cx);
    }
    if !(self.route.creating && self.route.tab == Tab::Issues) {
      self.new_issue = NewIssue::default();
    }
  }

  /// Reads the active list's pages again; `more` reads only the next one.
  pub(super) fn reload_list(&mut self, more: bool, cx: &mut Context<Self>) {
    match self.route.tab {
      Tab::Issues => self.load_issues(more, cx),
      Tab::Prs => self.load_prs(more, cx),
    }
  }

  fn load_issues(&mut self, more: bool, cx: &mut Context<Self>) {
    let Some(repo) = self.repo() else {
      return;
    };
    let filter = self.route.filter;
    let plan = plan_load(&mut self.issues, (repo.clone(), filter), more);
    let generation = self.issues.generation;
    self.run(
      cx,
      move |host| {
        fetch_pages(plan, |_, after: Option<&str>| {
          host.list_issues(&repo, filter.value(), after)
        })
      },
      move |view, result, _| {
        if view.issues.generation == generation {
          finish_load(&mut view.issues, result);
        }
      },
    );
  }

  fn load_prs(&mut self, more: bool, cx: &mut Context<Self>) {
    let Some(repo) = self.repo() else {
      return;
    };
    let filter = self.route.filter;
    let plan = plan_load(&mut self.prs, (repo.clone(), filter), more);
    let generation = self.prs.generation;
    self.run(
      cx,
      move |host| {
        fetch_pages(plan, |page: u32, _: Option<&str>| {
          host.list_prs(&repo, filter.value(), page)
        })
      },
      move |view, result, _| {
        if view.prs.generation == generation {
          finish_load(&mut view.prs, result);
        }
      },
    );
  }

  /// The loaded pull requests the filters keep.
  pub(super) fn visible_prs(&self) -> Vec<GhPullRequest> {
    let prs = items(&self.prs, |pr| pr.number);
    data::filter_prs(&prs, &self.filters, chrono::Utc::now())
      .into_iter()
      .cloned()
      .collect()
  }

  pub(super) fn issue_items(&self) -> Vec<GhIssue> {
    items(&self.issues, |issue| issue.number)
  }

  pub(super) fn pr_items(&self) -> Vec<GhPullRequest> {
    items(&self.prs, |pr| pr.number)
  }

  /// Reads the open detail again, keeping what shows meanwhile.
  pub(super) fn load_detail(&mut self, cx: &mut Context<Self>) {
    let Some(key) = self.detail.key.clone() else {
      return;
    };
    let shown = self.detail.issue.is_some() || self.detail.pr.is_some();
    self.detail.loading = !shown;
    let generation = self.detail.generation;
    let k = key.clone();
    self.run(
      cx,
      move |host| match k.tab {
        Tab::Issues => (host.issue(&k.repo, k.number).map(Ok), None),
        Tab::Prs => (
          host.pr(&k.repo, k.number).map(Err),
          Some(host.pr_checks(&k.repo, k.number)),
        ),
      },
      move |view, (read, checks), cx| {
        if view.detail.generation != generation {
          return;
        }
        let first = view.detail.issue.is_none() && view.detail.pr.is_none();
        view.detail.loading = false;
        match read {
          Ok(Ok(issue)) => view.detail.issue = Some(issue),
          Ok(Err(pr)) => view.detail.pr = Some(pr),
          Err(err) => view.detail.error = Some(err),
        }
        if let Some(checks) = checks {
          view.detail.checks = checks.unwrap_or_else(|err| {
            log::warn!("github panel: checks: {err}");
            None
          });
        }
        if first && key.tab == Tab::Prs {
          view.load_treq(cx);
          view.poll_checks(generation, cx);
        }
      },
    );
  }

  /// Reads the open PR's checks every [`CHECKS_POLL`] while it shows.
  fn poll_checks(&mut self, generation: u64, cx: &mut Context<Self>) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(CHECKS_POLL).await;
      let Ok(Some(key)) = this.update(cx, |view, _| {
        (view.detail.generation == generation)
          .then(|| view.detail.key.clone())
          .flatten()
      }) else {
        return;
      };
      let host = host.clone();
      let checks = cx
        .background_spawn(async move { host.pr_checks(&key.repo, key.number) })
        .await;
      this
        .update(cx, |view, cx| {
          if view.detail.generation == generation {
            if let Ok(checks) = checks {
              view.detail.checks = checks;
            }
            cx.notify();
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  /// The open PR's workspace, if one is on its head branch.
  pub(super) fn pr_workspace(&self) -> Option<&Workspace> {
    let pr = self.detail.pr.as_ref()?;
    self
      .workspaces
      .iter()
      .find(|w| w.branch_name == pr.head_ref_name)
  }

  pub(super) fn load_treq(&mut self, cx: &mut Context<Self>) {
    let Some(ws) = self.pr_workspace().cloned() else {
      return;
    };
    let generation = self.detail.generation;
    self.run(
      cx,
      move |host| host.treq_checks(&ws.repo_path, ws.id),
      move |view, checks, cx| {
        if view.detail.generation != generation {
          return;
        }
        let setting_up = checks
          .setup
          .as_ref()
          .is_some_and(|s| s.configured && (s.status == "pending" || s.status == "running"));
        view.detail.treq = Some(checks);
        if setting_up {
          cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETUP_POLL).await;
            this
              .update(cx, |view, cx| {
                if view.detail.generation == generation {
                  view.load_treq(cx);
                }
              })
              .ignore_closed();
          })
          .detach();
        }
      },
    );
  }

  pub(super) fn load_history(&mut self, filename: String, cx: &mut Context<Self>) {
    let Some(ws) = self.pr_workspace().cloned() else {
      return;
    };
    let generation = self.detail.generation;
    self.run(
      cx,
      move |host| {
        let runs = host.run_history(&ws.repo_path, ws.id, &filename);
        (filename, runs)
      },
      move |view, (filename, runs), _| {
        if view.detail.generation != generation {
          return;
        }
        match runs {
          Ok(runs) => {
            view.detail.history.insert(filename, runs);
          }
          Err(err) => log::warn!("github panel: run history: {err}"),
        }
      },
    );
  }

  pub(super) fn load_logs(&mut self, run_id: i64, job: String, cx: &mut Context<Self>) {
    let Some(ws) = self.pr_workspace().cloned() else {
      return;
    };
    let generation = self.detail.generation;
    self.run(
      cx,
      move |host| {
        let logs = host.run_logs(&ws.repo_path, run_id, &job);
        (job, logs)
      },
      move |view, (job, logs), _| {
        if view.detail.generation == generation {
          view.detail.logs.insert((run_id, job), logs);
        }
      },
    );
  }

  pub(super) fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    self.host.notify(NativeEvent::Toast {
      title: title.into(),
      description: description.into(),
      tone,
      action: None,
    });
  }

  /// Every run's key in the repository: React's, and those GPUI runs
  /// ([`checks_run`]).
  pub(super) fn running_keys(&self, cx: &App) -> Vec<String> {
    let mut keys = self.route.running_checks.clone();
    for key in checks_run::keys(cx, &self.route.repo_path) {
      if !keys.contains(&key) {
        keys.push(key);
      }
    }
    keys
  }

  /// Runs a workflow, a job or the setup script of `ws` (`github.checks.*`),
  /// marked running meanwhile, then reads the results again. React runs
  /// them for a remote repository.
  pub(super) fn run_treq_check(&self, ws: &Workspace, run: CheckRun, cx: &mut Context<Self>) {
    let target = ChecksTarget {
      repo_path: ws.repo_path.clone(),
      workspace_id: ws.id,
      workspace_path: super::merge::full_path(ws),
    };
    if !self.host.acts_locally(&ws.repo_path) {
      let mut value = serde_json::json!({
        "repoPath": target.repo_path,
        "workspaceId": target.workspace_id,
        "workspacePath": target.workspace_path,
      });
      let field = match &run {
        CheckRun::Setup => "github.checks.setup",
        CheckRun::Workflow(file) => {
          value["filename"] = file.clone().into();
          value["jobId"] = serde_json::Value::Null;
          "github.checks.run"
        }
        CheckRun::Job { file, job } => {
          value["filename"] = file.clone().into();
          value["jobId"] = job.clone().into();
          "github.checks.run"
        }
      };
      return self.field(field, value, cx);
    }
    let (host, view) = (self.host.clone(), cx.weak_entity());
    let repo_path = target.repo_path.clone();
    checks_run::start(
      cx,
      target,
      run,
      move |target, run| host.run_checks(target, run),
      move |_, cx| {
        view
          .update(cx, |view, cx| {
            view
              .host
              .notify(NativeEvent::WorkspaceChanged { repo_path });
            view.reload_checks(cx);
          })
          .ignore_closed();
      },
    );
  }

  /// Reads the workspace's checks and their open run histories again.
  fn reload_checks(&mut self, cx: &mut Context<Self>) {
    self.load_treq(cx);
    let open: Vec<String> = self.detail.history_open.iter().cloned().collect();
    for filename in open {
      self.load_history(filename, cx);
    }
  }

  /// Hands `id` to React.
  pub(super) fn press(&self, id: impl Into<String>, cx: &mut App) {
    (self.react)(ScreenAction::Press { id: id.into() }, cx);
  }

  pub(super) fn field(&self, field: &str, value: serde_json::Value, cx: &mut App) {
    (self.react)(
      ScreenAction::Field {
        field: field.into(),
        value,
      },
      cx,
    );
  }

  pub(super) fn open_url(&self, url: &str) {
    self.host.open_url(url);
  }

  /// A right-click on what opens `url`: its "Copy link" menu.
  pub(super) fn copy_link(
    &self,
    url: String,
  ) -> impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static {
    let (popups, host, dark) = (self.popups.clone(), self.host.clone(), self.dark);
    move |e, _, cx| {
      let host = host.clone();
      let menu = popup::copy_link_menu(url.clone(), e.position, dark, move |e| host.notify(e));
      popups(menu, cx);
    }
  }

  /// Opens `popover`, or closes it when its own button is pressed again.
  pub(super) fn toggle_popover(&mut self, popover: Popover, cx: &mut Context<Self>) {
    let just_closed = self
      .popover_closed
      .take()
      .is_some_and(|(p, at)| p == popover && at.elapsed() < Duration::from_millis(500));
    self.popover = if self.popover == Some(popover) || just_closed {
      None
    } else {
      Some(popover)
    };
    cx.notify();
  }

  /// A press outside the open popover closes it, unless it lands in one of
  /// its own selects' menus.
  /// Closes `popover` if it is the open one, and says whether it was.
  pub(super) fn close_popover(&mut self, popover: Popover, cx: &mut Context<Self>) -> bool {
    if self.popover != Some(popover) || GlobalState::is_in_deferred_context(cx) {
      return false;
    }
    self.popover = None;
    self.popover_closed = Some((popover, Instant::now()));
    cx.notify();
    true
  }

  /// Fields typed into, and the choices of the filter selects.
  pub(super) fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    let ScreenAction::Field { field, value } = action else {
      return;
    };
    let text = value.as_str().unwrap_or_default().to_string();
    let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
    match field.as_str() {
      "github.search" => self.filters.query = text,
      "github.filter.author" => self.filters.author = some(&text),
      "github.filter.label" => self.filters.label = some(&text),
      "github.filter.head" => self.filters.head = some(&text),
      "github.filter.base" => self.filters.base = some(&text),
      "github.filter.stackLevel" => self.filters.stack_level = text.parse().ok(),
      "github.filter.opened" => self.filters.opened_within_days = text.parse().ok(),
      "github.filter.conflict" => self.filters.conflict = some(&text),
      "github.comment" => self.comment = text,
      "github.newIssue.title" => self.new_issue.title = text,
      "github.newIssue.body" => self.new_issue.body = text,
      "github.edit.title" => {
        if let Some((title, _)) = &mut self.detail.editing {
          *title = text;
        }
      }
      "github.edit.body" => {
        if let Some((_, body)) = &mut self.detail.editing {
          *body = text;
        }
      }
      other => {
        if let Some(new_pr) = &mut self.new_pr {
          new_pr.set_field(other, text);
        }
      }
    }
    cx.notify();
  }

  /// Shows `value` in `field`'s control, even while it has focus.
  pub(super) fn reset_field(&mut self, field: &'static str) {
    self.reset.insert(field);
  }

  pub(super) fn input(
    &mut self,
    field: &'static str,
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> gpui::Entity<gpui_kit::component::input::InputState> {
    let state = self.controls.input(field, value, placeholder, window, cx);
    if self.reset.remove(field) {
      let value = value.to_string();
      state.update(cx, |s, cx| s.set_value(value, window, cx));
    }
    state
  }

  pub(super) fn textarea(
    &mut self,
    field: &'static str,
    value: &str,
    placeholder: &str,
    rows: usize,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> gpui::Entity<gpui_kit::component::input::TextareaState> {
    let state = self
      .controls
      .textarea(field, value, placeholder, rows, window, cx);
    if self.reset.remove(field) {
      let value = value.to_string();
      state.update(cx, |s, cx| s.set_value(value, window, cx));
    }
    state
  }

  /// Tells the PR-status poller and React that `branch`'s PR changed.
  pub(super) fn pr_status_changed(&self, branch: &str) {
    self.host.refresh_pr_status(&self.route.repo_path, branch);
  }

  /// `useCreateWorkspacePr`'s success toast, with its "Open in Web" button.
  pub(super) fn pr_created_toast(&self, repo: &str, number: u64, draft: bool) {
    let url = format!("https://github.com/{repo}/pull/{number}");
    let host = self.host.clone();
    let id = register_toast_action(move || host.open_url(&url));
    self.host.notify(NativeEvent::Toast {
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
        id,
      }),
    });
  }

  fn create_issue(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = self.repo() else {
      return;
    };
    if self.new_issue.pending || self.new_issue.title.trim().is_empty() {
      return;
    }
    self.new_issue.pending = true;
    self.new_issue.error = None;
    let (title, body) = (self.new_issue.title.clone(), self.new_issue.body.clone());
    self.run(
      cx,
      move |host| host.create_issue(&repo, &title, &body),
      |view, created, cx| {
        view.new_issue.pending = false;
        match created {
          Ok(number) => {
            view.new_issue = NewIssue::default();
            view.reset_field("github.newIssue.title");
            view.reset_field("github.newIssue.body");
            view.reload_list(false, cx);
            view.press(format!("github.created:{number}"), cx);
          }
          Err(err) => view.new_issue.error = Some(err),
        }
      },
    );
  }
}

/// What a load reads: the pages it keeps and how many more to read.
struct LoadPlan<T> {
  kept: Vec<GhListPage<T>>,
  count: usize,
}

/// Starts a load of `pages` for `key`: the next page with `more`, else
/// every page shown (at least the first) again.
fn plan_load<T: Clone>(
  pages: &mut Pages<T>,
  key: (String, StateFilter),
  more: bool,
) -> LoadPlan<T> {
  pages.generation += 1;
  if pages.key.as_ref() != Some(&key) {
    *pages = Pages {
      key: Some(key),
      generation: pages.generation,
      ..Pages::default()
    };
  }
  if more {
    pages.fetching_next = true;
    LoadPlan {
      kept: pages.pages.clone(),
      count: 1,
    }
  } else {
    pages.loading = pages.pages.is_empty();
    LoadPlan {
      kept: Vec::new(),
      count: pages.pages.len().max(1),
    }
  }
}

/// Reads `plan.count` pages after the kept ones; stops early at the end.
fn fetch_pages<T>(
  plan: LoadPlan<T>,
  fetch: impl Fn(u32, Option<&str>) -> Result<GhListPage<T>, String>,
) -> Result<Vec<GhListPage<T>>, String> {
  let mut pages = plan.kept;
  for _ in 0..plan.count {
    if pages.last().is_some_and(|p| !p.has_more) {
      break;
    }
    let after = pages.last().and_then(|p| p.end_cursor.clone());
    let page = fetch(pages.len() as u32 + 1, after.as_deref())?;
    pages.push(page);
  }
  Ok(pages)
}

/// Shows a load's pages; a failure keeps what showed.
fn finish_load<T>(pages: &mut Pages<T>, result: Result<Vec<GhListPage<T>>, String>) {
  pages.loading = false;
  pages.fetching_next = false;
  match result {
    Ok(read) => {
      pages.pages = read;
      pages.error = None;
    }
    Err(err) => pages.error = Some(err),
  }
}

impl Render for GithubView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    let p = theme::palette(self.dark);
    let show_detail = self.repo().is_some() && self.route.selected.is_some();
    let list = self.list_panel(show_detail, &p, window, cx);
    let detail = show_detail.then(|| self.detail_panel(&p, window, cx));
    let dialogs = self.dialogs(&p, window, cx);
    div()
      .debug_selector(|| "github-panel".into())
      .size_full()
      .flex()
      .bg(p.background)
      .text_color(p.foreground)
      .child(list)
      .children(detail)
      .children(dialogs)
  }
}

impl GithubView {
  fn dialogs(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<gpui::AnyElement> {
    let open = self.new_pr.is_some() || self.detail.confirm_delete;
    if !open {
      self.dialog_focused = false;
      return None;
    }
    if !self.dialog_focused {
      self.dialog_focused = true;
      self.dialog_focus.focus(window, cx);
    }
    if self.detail.confirm_delete {
      return Some(self.delete_dialog(p, window, cx).into_any_element());
    }
    Some(self.new_pr_dialog(p, window, cx).into_any_element())
  }

  pub(super) fn dialog_focus(&self) -> &FocusHandle {
    &self.dialog_focus
  }

  fn list_panel(
    &mut self,
    show_detail: bool,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let remote = self.remote.clone();
    let title = match &remote {
      None => ui::text(div(), TextSize::Sm)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.muted_foreground)
        .child("Loading…"),
      Some(Some(info)) => ui::text(div(), TextSize::Sm)
        .font_weight(FontWeight::SEMIBOLD)
        .font_family(theme::MONO_FONT)
        .child(info.full_name.clone()),
      Some(None) => ui::text(div(), TextSize::Sm)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.destructive)
        .child("No GitHub remote"),
    };
    let header = div()
      .flex()
      .flex_none()
      .items_center()
      .px_4()
      .pt_4()
      .pb_2()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(theme::ICON_GITHUB, 20., p.muted_foreground))
          .child(title),
      );
    let has_remote = matches!(remote, Some(Some(_)));
    let body = self.list_body(show_detail, p, cx);
    named(div(), "github-list")
      .flex()
      .flex_col()
      .border_r_1()
      .border_color(p.border)
      .overflow_hidden()
      .when(show_detail, |d| d.w(zpx(380.)).flex_none())
      .when(!show_detail, |d| d.flex_1().min_w_0())
      .child(header)
      .child(
        div()
          .flex()
          .flex_none()
          .px_4()
          .pb_2()
          .child(self.view_tabs(p, cx)),
      )
      .child(self.toolbar(has_remote, p, window, cx))
      .when(
        has_remote && self.route.creating && self.route.tab == Tab::Issues,
        |d| d.child(self.new_issue_form(p, window, cx)),
      )
      .child(
        div()
          .id("github-list-scroll")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .child(body),
      )
  }

  fn view_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let view = cx.weak_entity();
    let merge_queue = self.route.merge_queue.as_ref();
    let selected = match self.route.tab {
      Tab::Issues => 0,
      Tab::Prs => 1,
    };
    let mut tabs = vec![
      ui::Tab::new("github-tab-issues", "Issues"),
      ui::Tab::new("github-tab-prs", "Pull Requests"),
    ];
    if merge_queue.is_some() {
      let mut tab = ui::Tab::new("github-tab-merge-queue", "Merge Queue");
      if merge_queue.is_some_and(|mq| !mq.pro) {
        tab = tab.badge("PRO", ui::Tone::Neutral);
      }
      tabs.push(tab);
    }
    div().flex().items_center().gap_2().child(
      ui::Tabs::new("github-tabs", p)
        .tabs(tabs)
        .selected(selected)
        .on_select(move |ix, _, cx| {
          let press = [
            "github.tab:issues",
            "github.tab:prs",
            "github.tab:merge-queue",
          ][ix];
          view.update(cx, |view, cx| view.press(press, cx)).ok();
        }),
    )
  }

  fn toolbar(
    &mut self,
    has_remote: bool,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let current = self.route.filter;
    let accent = p.foreground;
    let filters = div()
      .flex()
      .items_center()
      .gap_1()
      .border_1()
      .border_color(p.border)
      .rounded(radius::MD)
      .p_0p5()
      .bg(p.muted.opacity(0.3))
      .children(StateFilter::options(self.route.tab).iter().map(|&filter| {
        let active = filter == current;
        ui::text(
          named(div(), format!("github-filter-{}", filter.value())),
          TextSize::Sm,
        )
        .px_2()
        .py_0p5()
        .rounded(radius::SM)
        .cursor_pointer()
        .when(active, |b| {
          b.bg(p.background).text_color(p.foreground).shadow_sm()
        })
        .when(!active, |b| {
          b.text_color(p.muted_foreground)
            .hover(move |s| s.text_color(accent))
        })
        .on_click(cx.listener(move |view, _, _, cx| {
          view.press(format!("github.filter:{}", filter.value()), cx)
        }))
        .child(filter.label())
      }));
    let prs = self.route.tab == Tab::Prs;
    let filter_menu = prs.then(|| self.filter_menu(p, window, cx));
    let search = prs.then(|| {
      let state = self.input(
        "github.search",
        &self.filters.query.clone(),
        "Search PRs",
        window,
        cx,
      );
      div()
        .id("github-search")
        .debug_selector(|| "github-search".into())
        .flex_1()
        .min_w(zpx(160.))
        .max_w(zpx(320.))
        .child(ui::text(
          Styled::h(
            Input::new(&state).prefix(ui::icon(theme::ICON_SEARCH, 14., p.muted_foreground)),
            zpx(28.),
          ),
          TextSize::Sm,
        ))
    });
    div()
      .flex()
      .flex_none()
      .flex_wrap()
      .items_center()
      .gap_2()
      .px_4()
      .pb_2()
      .child(filters)
      .children(filter_menu)
      .children(search)
      .child(div().flex_1())
      .child(
        Button::new("github-refresh", p)
          .aria_label("Refresh")
          .variant(Variant::Ghost)
          .size(Size::Icon)
          .w(zpx(28.))
          .h(zpx(28.))
          .icon(theme::ICON_REFRESH, 14.)
          .on_click(cx.listener(|view, _, _, cx| view.reload_list(false, cx))),
      )
      .when(has_remote, |d| {
        d.child(
          Button::new("github-new", p)
            .size(Size::Sm)
            .h(zpx(28.))
            .gap_0()
            .child(ui::icon(theme::ICON_PLUS, 12., p.primary_foreground).mr_1())
            .label("New")
            .on_click(cx.listener(|view, _, _, cx| view.press("github.new", cx))),
        )
      })
  }

  /// `PrFilterMenu`: the Filters button and its popover.
  fn filter_menu(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let count = self.filters.active_count();
    let button = Button::new("github-filters", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .h(zpx(28.))
      .gap_1()
      .icon(theme::ICON_LIST_FILTER, 14.)
      .label("Filters")
      .when(count > 0, |b| b.child(ui::Count::new(count, p)))
      .on_click(cx.listener(|view, _, _, cx| view.toggle_popover(Popover::Filters, cx)));
    let panel = (self.popover == Some(Popover::Filters)).then(|| self.filter_panel(p, window, cx));
    div().relative().child(button).children(panel)
  }

  fn filter_panel(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let prs = self.pr_items();
    let options = data::filter_options(&prs);
    let plain = |values: &[String]| -> Vec<(String, String)> {
      values.iter().map(|v| (v.clone(), v.clone())).collect()
    };
    let f = self.filters.clone();
    let opt = |v: &Option<String>| v.clone().unwrap_or_default();
    let selects: Vec<FilterSelect> = vec![
      (
        "github.filter.author",
        "Author",
        opt(&f.author),
        plain(&options.authors),
      ),
      (
        "github.filter.label",
        "Label",
        opt(&f.label),
        plain(&options.labels),
      ),
      (
        "github.filter.head",
        "Source branch",
        opt(&f.head),
        plain(&options.heads),
      ),
      (
        "github.filter.base",
        "Target branch",
        opt(&f.base),
        plain(&options.bases),
      ),
      (
        "github.filter.stackLevel",
        "Stack level",
        f.stack_level.map(|n| n.to_string()).unwrap_or_default(),
        options
          .stack_levels
          .iter()
          .map(|&n| {
            let label = if n == 1 {
              "1 (bottom)".to_string()
            } else {
              n.to_string()
            };
            (label, n.to_string())
          })
          .collect(),
      ),
      (
        "github.filter.opened",
        "Opened",
        f.opened_within_days
          .map(|n| n.to_string())
          .unwrap_or_default(),
        [
          ("Last 24 hours", 1),
          ("Last 7 days", 7),
          ("Last 30 days", 30),
          ("Last 90 days", 90),
        ]
        .iter()
        .map(|(label, days)| (label.to_string(), days.to_string()))
        .collect(),
      ),
      (
        "github.filter.conflict",
        "Merge conflicts",
        opt(&f.conflict),
        vec![
          ("Has conflicts".into(), "conflicting".into()),
          ("No conflicts".into(), "clean".into()),
        ],
      ),
    ];
    let mut cells: Vec<gpui::AnyElement> = Vec::new();
    for (field, label, value, options) in selects {
      // Keep a choice the loaded PRs no longer have, so the select still
      // shows what hides the list.
      let mut shown: Vec<SelectOption> = vec![SelectOption {
        value: String::new(),
        label: "Any".into(),
      }];
      shown.extend(options.iter().map(|(label, value)| SelectOption {
        value: value.clone(),
        label: label.clone(),
      }));
      if !value.is_empty() && !options.iter().any(|(_, v)| *v == value) {
        shown.push(SelectOption {
          value: value.clone(),
          label: value.clone(),
        });
      }
      let state = self.controls.select(field, &shown, &value, window, cx);
      cells.push(
        div()
          .flex()
          .flex_col()
          .gap_1()
          .min_w_0()
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child(label),
          )
          .child(ui::text(
            div().h(zpx(32.)).child(form::select_field(
              SharedString::from(field.replace('.', "-")),
              label,
              &state,
              false,
            )),
            TextSize::Sm,
          ))
          .into_any_element(),
      );
    }
    let count = self.filters.active_count();
    cells.push(
      div()
        .flex()
        .items_end()
        .child(
          Button::new("github-filters-clear", p)
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .w_full()
            .disabled(count == 0)
            .label("Clear filters")
            .on_click(cx.listener(|view, _, _, cx| {
              view.filters = view.filters.cleared();
              cx.notify();
            })),
        )
        .into_any_element(),
    );
    let rows: Vec<Div> = cells
      .chunks_mut(2)
      .map(|pair| {
        let mut row = div().flex().gap_2();
        for cell in pair.iter_mut() {
          let cell = std::mem::replace(cell, div().into_any_element());
          row = row.child(div().flex_1().min_w_0().child(cell));
        }
        row
      })
      .collect();
    deferred(
      named(div(), "github-filters-popover")
        .absolute()
        .top(zpx(32.))
        .left_0()
        .w(zpx(448.))
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(radius::MD)
        .border_1()
        .border_color(p.border)
        .bg(p.popover)
        .text_color(p.popover_foreground)
        .shadow_md()
        .occlude()
        .dismiss_with("github-filters", cx, |view, cx| {
          view.close_popover(Popover::Filters, cx)
        })
        .children(rows),
    )
    .with_priority(1)
  }

  fn list_body(
    &mut self,
    show_detail: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::AnyElement {
    match &self.remote {
      None => return div().into_any_element(),
      Some(None) => {
        return div()
          .size_full()
          .flex()
          .flex_col()
          .items_center()
          .justify_center()
          .p_8()
          .gap_3()
          .text_center()
          .child(ui::icon(theme::ICON_CIRCLE_ALERT, 32., p.muted_foreground))
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child("No GitHub remote detected for this repository."),
          )
          .into_any_element()
      }
      Some(Some(_)) => {}
    }
    let retry =
      || -> parts::OnClick { Box::new(cx.listener(|view, _, _, cx| view.reload_list(false, cx))) };
    match self.route.tab {
      Tab::Issues => {
        let pages = &self.issues;
        if pages.loading {
          return div()
            .py_12()
            .child(ui::Loading::new("github-list-loading", p))
            .into_any_element();
        }
        let issues = self.issue_items();
        if let (Some(err), true) = (&pages.error, issues.is_empty()) {
          return parts::error_state(
            "github-list",
            "Could not load issues",
            err,
            p,
            Some(retry()),
          )
          .into_any_element();
        }
        if issues.is_empty() {
          return ui::EmptyState::new("github-no-issues", "No issues found.", p)
            .icon(theme::ICON_CIRCLE_DOT)
            .into_any_element();
        }
        let (has_next, next) = (pages.has_next(), pages.fetching_next);
        let selected = self.route.selected;
        div()
          .children(issues.iter().map(|issue| {
            let n = issue.number;
            self
              .row(
                format!("github-issue-{n}"),
                selected == Some(n),
                p,
                cx,
                move |view, cx| view.press(format!("github.select:{n}"), cx),
              )
              .child(
                div()
                  .flex_1()
                  .min_w_0()
                  .child(parts::row_title(&issue.title, n, p))
                  .child(
                    parts::row_meta(p)
                      .child(parts::state_chip(&issue.state, false, p))
                      .child(
                        div()
                          .flex_none()
                          .child(crate::gpui_shell::time::date(&issue.created_at)),
                      ),
                  ),
              )
              .child(chevron(p))
          }))
          .when(has_next, |d| {
            d.child(parts::load_more(
              "github-load-more",
              next,
              p,
              Box::new(cx.listener(|view, _, _, cx| view.reload_list(true, cx))),
            ))
          })
          .into_any_element()
      }
      Tab::Prs => {
        let pages = &self.prs;
        if pages.loading {
          return div()
            .py_12()
            .child(ui::Loading::new("github-list-loading", p))
            .into_any_element();
        }
        let prs = self.pr_items();
        if let (Some(err), true) = (&pages.error, prs.is_empty()) {
          return parts::error_state(
            "github-list",
            "Could not load pull requests",
            err,
            p,
            Some(retry()),
          )
          .into_any_element();
        }
        if prs.is_empty() {
          return ui::EmptyState::new("github-no-prs", "No pull requests found.", p)
            .icon(theme::ICON_PR)
            .into_any_element();
        }
        let (has_next, next) = (pages.has_next(), pages.fetching_next);
        let visible = self.visible_prs();
        let selected = self.route.selected;
        div()
          .when(visible.is_empty(), |d| {
            d.child(
              ui::EmptyState::new(
                "github-no-matching-prs",
                "No loaded pull requests match the filters.",
                p,
              )
              .icon(theme::ICON_PR),
            )
          })
          .children(visible.iter().map(|pr| {
            let n = pr.number;
            self
              .row(
                format!("github-pr-{n}"),
                selected == Some(n),
                p,
                cx,
                move |view, cx| view.press(format!("github.select:{n}"), cx),
              )
              .child(parts::pr_list_item(pr, !show_detail, p))
              .child(chevron(p))
          }))
          .when(has_next, |d| {
            d.child(parts::load_more(
              "github-load-more",
              next,
              p,
              Box::new(cx.listener(|view, _, _, cx| view.reload_list(true, cx))),
            ))
          })
          .into_any_element()
      }
    }
  }

  /// An `IssueListItem` or `PrListItem` button.
  fn row(
    &self,
    id: String,
    selected: bool,
    p: &Palette,
    cx: &mut Context<Self>,
    on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
  ) -> gpui::Stateful<Div> {
    let muted = p.muted;
    named(div(), id)
      .w_full()
      .px_3()
      .py_2p5()
      .border_b_1()
      .border_color(p.border)
      .flex()
      .items_start()
      .gap_2()
      .cursor_pointer()
      .when(selected, |d| d.bg(p.selected()))
      .when(!selected, |d| d.hover(move |s| s.bg(muted.opacity(0.5))))
      .on_click(cx.listener(move |view, _, _, cx| on_click(view, cx)))
  }

  /// `CreateIssueForm`.
  fn new_issue_form(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let (title, body) = (self.new_issue.title.clone(), self.new_issue.body.clone());
    let title_state = self.input("github.newIssue.title", &title, "Title", window, cx);
    let body_state = self.textarea(
      "github.newIssue.body",
      &body,
      "Description (optional)",
      4,
      window,
      cx,
    );
    let pending = self.new_issue.pending;
    named(div(), "github-new-issue")
      .flex_none()
      .p_4()
      .flex()
      .flex_col()
      .gap_3()
      .border_b_1()
      .border_color(p.border)
      .child(
        ui::text(div(), TextSize::Sm)
          .font_weight(FontWeight::SEMIBOLD)
          .child("New Issue"),
      )
      .child(ui::text(
        div().child(form::input_field(
          "github-new-issue-title",
          &title_state,
          false,
          false,
        )),
        TextSize::Sm,
      ))
      .child(ui::text(
        div().child(form::textarea_field(&body_state, false, false)),
        TextSize::Sm,
      ))
      .child(
        div()
          .flex()
          .gap_2()
          .child(
            parts::busy_spinner(
              Button::new("github-new-issue-submit", p)
                .size(Size::Sm)
                .gap_1()
                .disabled(title.trim().is_empty() || pending),
              "github-new-issue-submit",
              pending,
              p.primary_foreground,
            )
            .label("Submit Issue")
            .on_click(cx.listener(|view, _, _, cx| view.create_issue(cx))),
          )
          .child(
            Button::new("github-new-issue-cancel", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.press("github.close", cx))),
          ),
      )
      .children(self.new_issue.error.clone().map(|err| {
        ui::text(div(), TextSize::Sm)
          .text_color(p.destructive)
          .child(err)
      }))
  }
}

fn chevron(p: &Palette) -> impl IntoElement {
  div()
    .mt_1()
    .child(ui::icon(theme::ICON_CHEVRON_RIGHT, 16., p.muted_foreground))
}
