//! The New Workspaces dialog, the one way to create workspaces: a line per
//! workspace (a title or branch name for a new one, or an existing branch
//! or GitHub pull request or branch link), plus each pull request or Linear
//! issue picked from the integrations beside it, all at once and stacked on
//! one branch.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{
  collections::{HashMap, HashSet},
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use gpui_kit::{
  base::TestSupportExt as _,
  component::{
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    IndexPath,
  },
};
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, Context, Div, Entity, FocusHandle,
  FontWeight, KeyDownEvent, MouseButton, SharedString, Subscription, Task, Window,
};

use super::{
  github::parts,
  integrations_cache::cache,
  linear,
  open_workspaces_data::{self as data, Checked, Line, OpenWorkspacesHost, Route, Target},
  workspace_data::WorkspaceHost,
  workspace_dialog_data::{self as dialog_data, WorkspaceDialogHost},
  workspace_modal::{dialog_description, dialog_header, dialog_panel, label_xs, press},
  ActionSink, ScreenAction,
};
use crate::{
  github::GhPullRequest,
  gpui_shell::{
    form::SelectOption,
    native::{NativeEvent, ToastTone},
    theme::{self, Palette},
    ui::{self, Button, TextSize, Variant},
  },
  linear::LinearIssue,
};

/// A workspace's id and whether it was made, or why it could not be.
type Opened = Result<(i64, bool), String>;

/// How long typing pauses before the lines are checked or the pull
/// requests searched.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// The pull request list beside the lines.
#[derive(Default)]
pub(super) struct PrList {
  pub(super) loading: bool,
  pub(super) error: Option<String>,
  pub(super) items: Vec<GhPullRequest>,
  /// The list holds only the user's own pull requests.
  pub(super) mine: bool,
  /// Bumped per load, so an older answer is dropped.
  generation: u64,
}

impl PrList {
  /// Starts a load, so any answer still in flight is dropped; the generation
  /// to settle it with.
  pub(super) fn start(&mut self) -> u64 {
    self.generation += 1;
    self.loading = true;
    self.error = None;
    self.generation
  }

  /// Takes a load's answer, unless a newer load started since.
  pub(super) fn settle(
    &mut self,
    generation: u64,
    result: Result<(Vec<GhPullRequest>, bool), String>,
  ) -> bool {
    if generation != self.generation {
      return false;
    }
    self.loading = false;
    match result {
      Ok((items, mine)) => {
        self.items = items;
        self.mine = mine;
      }
      Err(err) => self.error = Some(err),
    }
    true
  }
}

/// What the pull request list reads.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PrQuery {
  /// The user's own, or everyone's when they have none.
  MineFirst,
  /// Everyone's that match this GitHub search.
  Search(String),
}

/// The read for a search box's `query`, narrowed to the pull requests
/// assigned to the user when `assigned` is on.
pub(super) fn pr_query(query: &str, assigned: bool) -> PrQuery {
  let query = query.trim();
  match (assigned, query.is_empty()) {
    (false, true) => PrQuery::MineFirst,
    (false, false) => PrQuery::Search(query.to_string()),
    (true, true) => PrQuery::Search("assignee:@me".into()),
    (true, false) => PrQuery::Search(format!("assignee:@me {query}")),
  }
}

/// The Linear issues beside the lines.
#[derive(Default)]
pub(super) struct IssueList {
  pub(super) loading: bool,
  pub(super) error: Option<String>,
  pub(super) items: Vec<LinearIssue>,
  /// The user's Linear id, whose issues show first.
  pub(super) viewer: Option<String>,
}

/// The integrations' tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Integration {
  Github,
  Linear,
}

struct Fields {
  lines: Entity<TextareaState>,
  search: Entity<InputState>,
  issue_search: Entity<InputState>,
  sparse: Entity<TextareaState>,
  symlinks: Entity<TextareaState>,
}

/// The Stack on select's branches, and the select.
type StackSelect = (Vec<SelectOption>, Entity<SelectState<Branches>>);

/// The Stack on select's searchable branches.
type Branches = SearchableVec<SelectOption>;

/// `treq/{name}`, until the repository's pattern loads.
const DEFAULT_PATTERN: &str = "treq/{name}";

pub struct OpenWorkspacesDialog {
  route: Route,
  host: Arc<dyn OpenWorkspacesHost>,
  sink: ActionSink,
  dark: bool,
  focus: FocusHandle,
  fields: Option<Fields>,
  _subscriptions: Vec<Subscription>,
  pub(super) text: String,
  /// Each repository's `owner/name`, once read.
  remotes: Option<Vec<(String, String)>>,
  /// The repository's branch name pattern, which names a title's branch.
  pattern: String,
  /// The repository's default branch, once read.
  pub(super) default_branch: Option<String>,
  /// The repository's branches, for the Stack on select.
  branches: Vec<String>,
  /// The branch new workspaces stack on; the default branch when `None`.
  pub(super) stack_on: Option<String>,
  stack_select: Option<StackSelect>,
  pub(super) advanced_open: bool,
  pub(super) sparse_paths: String,
  pub(super) symlinked_dirs: String,
  /// The repositories have been fetched, so their branches can be checked.
  fetched: bool,
  /// Each line's check, by its text.
  pub(super) checks: HashMap<String, Checked>,
  checking: bool,
  _check: Option<Task<()>>,
  /// The repository on screen's `owner/name`: the pull request list shows
  /// only for a GitHub repository.
  pub(super) github: Option<String>,
  pub(super) prs: PrList,
  pub(super) query: String,
  /// The list holds only pull requests assigned to the user. Off each time
  /// the dialog opens.
  pub(super) assigned: bool,
  _search: Option<Task<()>>,
  /// The picked pull requests, in the order picked.
  pub(super) chosen: Vec<GhPullRequest>,
  pub(super) issues: IssueList,
  pub(super) issue_query: String,
  /// The picked Linear issues, in the order picked.
  pub(super) chosen_issues: Vec<LinearIssue>,
  /// The tab the user picked; GitHub's when there is one, until then.
  picked: Option<Integration>,
  /// The popover listing each line and what it opens.
  pub(super) lines_open: bool,
  /// When a press outside closed the popover, so the press on its own
  /// trigger does not reopen it.
  lines_closed: Option<Instant>,
  /// Branches of the repository on screen that have a workspace.
  open_branches: HashSet<String>,
  pub(super) pending: bool,
  /// What failed in the last open, a line per workspace.
  pub(super) errors: Vec<String>,
  pub(super) dismissed: bool,
}

impl OpenWorkspacesDialog {
  pub fn new(
    route: Route,
    host: Arc<dyn OpenWorkspacesHost>,
    sink: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let stack_on = route.stack_on.clone();
    let mut view = Self {
      route,
      host,
      sink,
      dark,
      focus: cx.focus_handle(),
      fields: None,
      _subscriptions: Vec::new(),
      text: String::new(),
      remotes: None,
      pattern: DEFAULT_PATTERN.into(),
      default_branch: None,
      branches: Vec::new(),
      stack_on,
      stack_select: None,
      advanced_open: false,
      sparse_paths: String::new(),
      symlinked_dirs: String::new(),
      fetched: false,
      checks: HashMap::new(),
      checking: false,
      _check: None,
      github: None,
      prs: PrList::default(),
      query: String::new(),
      assigned: false,
      _search: None,
      chosen: Vec::new(),
      issues: IssueList::default(),
      issue_query: String::new(),
      chosen_issues: Vec::new(),
      picked: None,
      lines_open: false,
      lines_closed: None,
      open_branches: HashSet::new(),
      pending: false,
      errors: Vec::new(),
      dismissed: false,
    };
    // A repository seen before lists its pull requests at once, without
    // waiting for the remote to be read again.
    let repo = view.route.repo_path.clone();
    if let Some(name) = cache(cx).remote_names.get(&repo).cloned() {
      view.github = Some(name);
      view.load_prs(cx);
    }
    view.load(cx);
    view.load_issues(cx);
    view
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  /// Reads the remotes and lists pull requests at once, and the branch
  /// settings beside them; then fetches, so pasted branches check against
  /// the remote's latest.
  fn load(&mut self, cx: &mut Context<Self>) {
    let (host, route) = (self.host.clone(), self.route.clone());
    // The pull request list needs only the remote, a file read: it does not
    // wait for the branches and settings behind jj.
    cx.spawn({
      let (host, route) = (host.clone(), route.clone());
      async move |this, cx| {
        let remotes = cx
          .background_spawn(async move { data::remotes(&*host, &route.repos) })
          .await;
        this
          .update(cx, |view, cx| {
            let github = remotes
              .iter()
              .find(|(path, _)| *path == view.route.repo_path)
              .map(|(_, name)| name.clone());
            match &github {
              Some(name) => cache(cx).remote_names.insert(
                view.route.repo_path.clone(),
                Instant::now(),
                name.clone(),
              ),
              None => {
                cache(cx).remote_names.take(&view.route.repo_path);
              }
            }
            // Already listed from a remembered remote unless it changed.
            let listed = view.prs.generation > 0 && view.github == github;
            view.github = github;
            view.remotes = Some(remotes);
            if !listed {
              view.load_prs(cx);
            }
            view.check_lines(cx);
            cx.notify();
          })
          .ignore_closed();
      }
    })
    .detach();
    cx.spawn(async move |this, cx| {
      let (open, settings) = cx
        .background_spawn({
          let (host, route) = (host.clone(), route.clone());
          async move {
            let repo = route.repo_path.as_str();
            let open: HashSet<String> = super::github_data::GithubHost::workspaces(&*host, repo)
              .unwrap_or_default()
              .into_iter()
              .map(|w| w.branch_name)
              .collect();
            let pattern = host
              .repo_settings(repo, &["branch_name_pattern".to_string()])
              .ok()
              .and_then(|values| values.get("branch_name_pattern").cloned().flatten())
              .filter(|p| !p.is_empty());
            let branches: Vec<String> = host
              .repo_branches(repo)
              .unwrap_or_default()
              .into_iter()
              .map(|b| b.name)
              .collect();
            let default = WorkspaceDialogHost::default_branch(&*host, repo).ok();
            (open, (pattern, default, branches))
          }
        })
        .await;
      this
        .update(cx, |view, cx| {
          let (pattern, default, branches) = settings;
          if let Some(pattern) = pattern {
            view.pattern = pattern;
          }
          view.default_branch = default;
          view.branches = branches;
          view.open_branches = open;
          cx.notify();
        })
        .ignore_closed();
      cx.background_spawn(async move {
        for repo in &route.repos {
          host.fetch(repo);
        }
      })
      .await;
      this
        .update(cx, |view, cx| {
          view.fetched = true;
          view.check_lines(cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// The user's open pull requests, or everyone's when they have none; with
  /// a search, everyone's that match.
  fn load_prs(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = self.github.clone() else {
      return;
    };
    let generation = self.prs.start();
    let (host, query) = (self.host.clone(), pr_query(&self.query, self.assigned));
    // Keyed by what is read, so Assigned to me has its own entries.
    let read = match &query {
      PrQuery::MineFirst => String::new(),
      PrQuery::Search(search) => search.clone(),
    };
    let key = (repo.clone(), read);
    // What this search read last shows meanwhile, then the read replaces it.
    if self.prs.items.is_empty() {
      if let Some((items, mine)) = cache(cx).open_prs.get(&key).cloned() {
        self.prs.items = items;
        self.prs.mine = mine;
      }
    }
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move {
          if let PrQuery::Search(query) = query {
            return host.open_prs(&repo, false, &query).map(|prs| (prs, false));
          }
          match host.open_prs(&repo, true, "")? {
            mine if mine.is_empty() => host.open_prs(&repo, false, "").map(|prs| (prs, false)),
            mine => Ok((mine, true)),
          }
        })
        .await;
      this
        .update(cx, |view, cx| {
          if let Ok(read) = &result {
            cache(cx).open_prs.insert(key, Instant::now(), read.clone());
          }
          if view.prs.settle(generation, result) {
            cx.notify();
          }
        })
        .ignore_closed();
    })
    .detach();
    cx.notify();
  }

  /// The Linear issues and who the user is, when the integration is on.
  fn load_issues(&mut self, cx: &mut Context<Self>) {
    if !self.route.linear {
      return;
    }
    self.issues.loading = true;
    self.issues.error = None;
    let (host, repo) = (self.host.clone(), self.route.repo_path.clone());
    if self.issues.items.is_empty() {
      let cache = cache(cx);
      if let Some(items) = cache.linear_issues.get(&(repo.clone(), None)).cloned() {
        self.issues.items = Vec::clone(&items);
        self.issues.viewer = cache.linear_viewer.get(&repo).map(|v| v.id.clone());
      }
    }
    cx.spawn(async move |this, cx| {
      let key = repo.clone();
      let result = cx
        .background_spawn(async move {
          // Two independent requests: run them side by side.
          std::thread::scope(|scope| {
            let viewer = scope.spawn(|| host.linear_viewer(&repo).ok());
            let issues = host.linear_issues(&repo, None);
            let viewer = super::joined(viewer);
            issues.map(|issues| (issues, viewer))
          })
        })
        .await;
      this
        .update(cx, |view, cx| {
          if let Ok((items, viewer)) = &result {
            let cache = cache(cx);
            cache
              .linear_issues
              .insert((key.clone(), None), Instant::now(), Rc::new(items.clone()));
            if let Some(viewer) = viewer {
              cache
                .linear_viewer
                .insert(key, Instant::now(), viewer.clone());
            }
          }
          view.issues.loading = false;
          match result {
            Ok((items, viewer)) => {
              view.issues.items = items;
              view.issues.viewer = viewer.map(|v| v.id);
            }
            Err(err) => view.issues.error = Some(err),
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
    cx.notify();
  }

  /// The integrations the repository has: GitHub once its remote is read.
  pub(super) fn integrations(&self) -> Vec<Integration> {
    let mut all = Vec::new();
    if self.github.is_some() {
      all.push(Integration::Github);
    }
    if self.route.linear {
      all.push(Integration::Linear);
    }
    all
  }

  pub(super) fn integration(&self) -> Option<Integration> {
    let all = self.integrations();
    self
      .picked
      .filter(|picked| all.contains(picked))
      .or_else(|| all.first().copied())
  }

  fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
    if query == self.query {
      return;
    }
    self.query = query;
    if self.github.is_some() {
      // Searching from the first keystroke: the debounce counts as the wait,
      // and an answer for the old query no longer lands.
      self.prs.start();
      cx.notify();
    }
    self._search = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DEBOUNCE).await;
      this
        .update(cx, |view, cx| view.load_prs(cx))
        .ignore_closed();
    }));
  }

  fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
    self.text = text;
    self.errors.clear();
    cx.notify();
    self._check = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DEBOUNCE).await;
      this
        .update(cx, |view, cx| view.check_lines(cx))
        .ignore_closed();
    }));
  }

  /// Checks the lines not checked yet.
  fn check_lines(&mut self, cx: &mut Context<Self>) {
    let Some(remotes) = self.remotes.clone().filter(|_| self.fetched) else {
      return;
    };
    let unchecked: Vec<(String, Line)> = data::parse_lines(&self.text)
      .into_iter()
      .filter(|(raw, _)| !self.checks.contains_key(raw))
      .collect();
    if unchecked.is_empty() {
      return;
    }
    self.checking = true;
    cx.notify();
    let (host, repo, pattern) = (
      self.host.clone(),
      self.route.repo_path.clone(),
      self.pattern.clone(),
    );
    cx.spawn(async move |this, cx| {
      let checked = cx
        .background_spawn(async move {
          unchecked
            .into_iter()
            .map(|(raw, line)| {
              let checked = data::check(&*host, &repo, &remotes, &pattern, &line);
              (raw, checked)
            })
            .collect::<Vec<_>>()
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.checks.extend(checked);
          view.checking = false;
          // Lines typed while checking.
          view.check_lines(cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Each line and its check, an indented line stacked on the line above
  /// it; `None` while it, or the line it stacks on, is checked.
  pub(super) fn lines(&self) -> Vec<(String, Option<Checked>)> {
    let parents = data::stack_parents(&self.text);
    let mut resolved: HashMap<String, Option<Checked>> = HashMap::new();
    data::parse_lines(&self.text)
      .into_iter()
      .map(|(raw, _)| {
        let own = self.checks.get(&raw);
        let checked = match parents.get(&raw).map(|parent| resolved.get(parent)) {
          None => own.cloned(),
          Some(Some(Some(parent))) => own.map(|own| data::stack_on_line(own, parent)),
          Some(_) => None,
        };
        resolved.insert(raw.clone(), checked.clone());
        (raw, checked)
      })
      .collect()
  }

  /// The workspaces the dialog will open: the lines', then the picked pull
  /// requests', each branch once.
  pub(super) fn targets(&self) -> Vec<Target> {
    let mut seen = HashSet::new();
    let lines = self.lines();
    let from_lines = lines
      .iter()
      .filter_map(|(_, checked)| checked.as_ref().and_then(|c| c.as_ref().ok()).cloned());
    let from_prs = self.chosen.iter().map(|pr| {
      let existing = self.open_branches.contains(&pr.head_ref_name);
      Target::from_pr(&self.route.repo_path, pr, existing)
    });
    let from_issues = self.chosen_issues.iter().map(|issue| {
      let existing = self.open_branches.contains(&issue.branch_name);
      Target::from_issue(&self.route.repo_path, issue, existing)
    });
    from_lines
      .chain(from_prs)
      .chain(from_issues)
      .filter(|t| seen.insert((t.repo_path.clone(), t.branch.clone())))
      .collect()
  }

  /// Lines still to check.
  fn unchecked(&self) -> usize {
    self.lines().iter().filter(|(_, c)| c.is_none()).count()
  }

  pub(super) fn can_submit(&self) -> bool {
    !self.pending && self.unchecked() == 0 && !self.targets().is_empty()
  }

  fn toggle_pr(&mut self, pr: &GhPullRequest, cx: &mut Context<Self>) {
    match self.chosen.iter().position(|c| c.number == pr.number) {
      Some(at) => {
        self.chosen.remove(at);
      }
      None => self.chosen.push(pr.clone()),
    }
    self.errors.clear();
    cx.notify();
  }

  fn toggle_issue(&mut self, issue: &LinearIssue, cx: &mut Context<Self>) {
    match self.chosen_issues.iter().position(|c| c.id == issue.id) {
      Some(at) => {
        self.chosen_issues.remove(at);
      }
      None => self.chosen_issues.push(issue.clone()),
    }
    self.errors.clear();
    cx.notify();
  }

  fn toggle_lines(&mut self, cx: &mut Context<Self>) {
    let just_closed = self
      .lines_closed
      .take()
      .is_some_and(|at| at.elapsed() < Duration::from_millis(500));
    self.lines_open = !self.lines_open && !just_closed;
    cx.notify();
  }

  fn close_lines(&mut self, cx: &mut Context<Self>) {
    if self.lines_open {
      self.lines_open = false;
      self.lines_closed = Some(Instant::now());
      cx.notify();
    }
  }

  fn close(&mut self, cx: &mut Context<Self>) {
    self.dismissed = true;
    press(&self.sink, "openWorkspaces.close", cx);
    cx.notify();
  }

  fn toast(&self, tone: ToastTone, title: &str, description: String) {
    WorkspaceHost::notify(
      &*self.host,
      NativeEvent::Toast {
        title: title.into(),
        description,
        tone,
        action: None,
      },
    );
  }

  /// Opens every target, a repository at a time.
  fn submit(&mut self, cx: &mut Context<Self>) {
    if !self.can_submit() {
      return;
    }
    let targets = self.targets();
    self.pending = true;
    self.errors.clear();
    cx.notify();
    let host = self.host.clone();
    let plan = Plan {
      repo_path: self.route.repo_path.clone(),
      stack_on: self.stack_on.clone(),
      sparse_paths: self.sparse_paths.clone(),
      symlinked_dirs: self.symlinked_dirs.clone(),
    };
    cx.spawn(async move |this, cx| {
      let results = cx
        .background_spawn(async move { plan.run(&*host, targets) })
        .await;
      this
        .update(cx, |view, cx| view.opened(results, cx))
        .ignore_closed();
    })
    .detach();
  }

  fn opened(&mut self, results: Vec<(Target, Opened)>, cx: &mut Context<Self>) {
    self.pending = false;
    let mut repos: Vec<String> = Vec::new();
    let (mut created, mut reused, mut first) = (0, 0, None);
    for (target, result) in &results {
      match result {
        Ok((id, made)) => {
          if *made {
            created += 1;
          } else {
            reused += 1;
          }
          first.get_or_insert((target.repo_path.clone(), *id));
          if !repos.contains(&target.repo_path) {
            repos.push(target.repo_path.clone());
          }
        }
        Err(err) => self.errors.push(format!("{}: {err}", target.branch)),
      }
    }
    for repo_path in repos {
      WorkspaceHost::notify(&*self.host, NativeEvent::WorkspaceChanged { repo_path });
    }
    let opened = summary(created, reused);
    if self.errors.is_empty() {
      self.toast(
        ToastTone::Success,
        opened.trim_end_matches('.'),
        String::new(),
      );
      if let Some((repo_path, id)) = first {
        (self.sink)(
          ScreenAction::Field {
            field: "openWorkspaces.opened".into(),
            value: serde_json::json!({ "repoPath": repo_path, "id": id }),
          },
          cx,
        );
      }
      self.close(cx);
    } else {
      self.toast(
        ToastTone::Error,
        "Some workspaces could not be opened",
        format!("{} failed. {opened}", self.errors.len()),
      );
      // What opened is open now, and a retry should say so.
      self.checks.clear();
      self.open_branches.extend(
        results
          .iter()
          .filter(|(t, r)| r.is_ok() && t.repo_path == self.route.repo_path)
          .map(|(t, _)| t.branch.clone()),
      );
      self.check_lines(cx);
    }
    cx.notify();
  }

  #[expect(
    clippy::expect_used,
    reason = "the fields are created before this read"
  )]
  fn fields(&mut self, window: &mut Window, cx: &mut Context<Self>) -> &Fields {
    if self.fields.is_none() {
      let lines = cx.new(|cx| {
        // A fixed height, not auto-grow, so Tab indents the lines.
        TextareaState::new(window, cx).rows(8).placeholder(
          "Fix the login flow\n  feat/login-tests\nhttps://github.com/acme/app/pull/42",
        )
      });
      let search =
        cx.new(|cx| InputState::new(window, cx).placeholder("Search open pull requests"));
      let issue_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search open issues"));
      let area = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
        cx.new(|cx| {
          TextareaState::new(window, cx)
            .auto_grow(2, 6)
            .placeholder(placeholder)
        })
      };
      let sparse = area("e.g., src/api\ndocs", window, cx);
      let symlinks = area("e.g., node_modules\ntarget", window, cx);
      self._subscriptions.push(
        cx.subscribe(&sparse, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.sparse_paths = input.read(cx).value().to_string();
          }
        }),
      );
      self._subscriptions.push(
        cx.subscribe(&symlinks, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.symlinked_dirs = input.read(cx).value().to_string();
          }
        }),
      );
      // `autoFocus`.
      lines.update(cx, |lines, cx| lines.focus(window, cx));
      self
        ._subscriptions
        .push(cx.subscribe(&lines, |view, lines, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            let text = lines.read(cx).value().to_string();
            view.set_text(text, cx);
          }
        }));
      self._subscriptions.push(
        cx.subscribe(&search, |view, search, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            let query = search.read(cx).value().to_string();
            view.set_query(query, cx);
          }
        }),
      );
      self._subscriptions.push(cx.subscribe(
        &issue_search,
        |view, search, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.issue_query = search.read(cx).value().to_string();
            cx.notify();
          }
        },
      ));
      self.fields = Some(Fields {
        lines,
        search,
        issue_search,
        sparse,
        symlinks,
      });
    }
    self.fields.as_ref().expect("fields")
  }

  /// The Stack on select: the default branch first, then the others.
  fn stack_select(
    &mut self,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<Entity<SelectState<Branches>>> {
    let default = self.default_branch.clone()?;
    let options: Vec<SelectOption> = std::iter::once(default.clone())
      .chain(self.branches.iter().filter(|b| **b != default).cloned())
      .map(|name| SelectOption {
        value: name.clone(),
        label: name,
      })
      .collect();
    let value = self.stack_on.clone().unwrap_or(default);
    if let Some((known, state)) = &mut self.stack_select {
      let state = state.clone();
      if *known != options {
        *known = options.clone();
        state.update(cx, |s, cx| {
          s.set_items(SearchableVec::new(options), window, cx);
          s.set_selected_value(&value, window, cx);
        });
      }
      return Some(state);
    }
    let index = options
      .iter()
      .position(|o| o.value == value)
      .map(IndexPath::new);
    let state = cx.new(|cx| {
      SelectState::new(SearchableVec::new(options.clone()), index, window, cx).searchable(true)
    });
    self._subscriptions.push(
      cx.subscribe(&state, |view, _, event: &SelectEvent<Branches>, cx| {
        if let SelectEvent::Confirm(Some(value)) = event {
          view.stack_on = Some(value.clone());
          cx.notify();
        }
      }),
    );
    self.stack_select = Some((options, state.clone()));
    Some(state)
  }

  /// Where new workspaces stack, and their sparse checkout and symlinks.
  fn create_options(
    &self,
    select: Option<Entity<SelectState<Branches>>>,
    sparse: &Entity<TextareaState>,
    symlinks: &Entity<TextareaState>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let field = |label: &'static str, selector: &'static str, area: &Entity<TextareaState>| {
      div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap_1p5()
        .child(label_xs(label))
        .child(
          div()
            .debug_selector(move || selector.into())
            .child(ui::text(
              Textarea::new(area).aria_label(label).disabled(self.pending),
              TextSize::Sm,
            )),
        )
    };
    div()
      .flex()
      .flex_col()
      .gap_2()
      .child(
        div()
          .flex()
          .items_center()
          .gap_3()
          .child(
            ui::text(div(), TextSize::Sm)
              .flex_none()
              .text_color(p.muted_foreground)
              .child("Stack on"),
          )
          .child(
            div()
              .id("open-workspaces-stack-on")
              .debug_selector(|| "open-workspaces-stack-on".into())
              .w(zpx(240.))
              .map(|this| match select {
                Some(state) => this.child(ui::text(
                  Select::new(&state)
                    .id("open-workspaces-stack-on-select")
                    .disabled(self.pending)
                    .h(zpx(28.)),
                  TextSize::Sm,
                )),
                None => this.child(
                  ui::text(div(), TextSize::Sm)
                    .text_color(p.muted_foreground)
                    .child("Loading branches..."),
                ),
              }),
          )
          .child(div().flex_1())
          .child(super::workspace_dialog::disclosure(
            "open-workspaces-advanced",
            "Advanced",
            self.advanced_open,
            p,
            cx.listener(|view, _, _, cx| {
              view.advanced_open = !view.advanced_open;
              cx.notify();
            }),
          )),
      )
      .when(self.advanced_open, |this| {
        this.child(
          div()
            .flex()
            .gap_3()
            .child(field(
              "Sparse paths (one per line)",
              "open-workspaces-sparse",
              sparse,
            ))
            .child(field(
              "Symlink from home repo (one per line)",
              "open-workspaces-symlinks",
              symlinks,
            )),
        )
      })
  }

  /// The lines' column: the text, what it will open, and the buttons.
  fn lines_column(
    &self,
    lines: &Entity<TextareaState>,
    options: Div,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let checks = self.lines();
    let targets = self.targets();
    let unchecked = checks.iter().filter(|(_, c)| c.is_none()).count();
    let invalid = checks
      .iter()
      .filter(|(_, c)| matches!(c, Some(Err(_))))
      .count();
    let existing = targets.iter().filter(|t| t.existing).count();
    let new = targets.len() - existing;
    let status = if unchecked > 0 {
      format!("Checking {}...", plural(unchecked, "1 line", "lines"))
    } else if targets.is_empty() {
      "Nothing to create yet.".to_string()
    } else {
      summary_future(new, existing)
    };
    div()
      .flex()
      .flex_col()
      .flex_1()
      .min_w_0()
      .gap_3()
      .child(
        div()
          .flex()
          .flex_col()
          .gap_1p5()
          .child(label_xs("Workspaces"))
          .child(
            div()
              .debug_selector(|| "open-workspaces-lines".into())
              .child(ui::text(
                Textarea::new(lines)
                  .aria_label("Workspaces")
                  .disabled(self.pending),
                TextSize::Sm,
              )),
          )
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child("One per line: a title or branch name for a new workspace, an existing branch, or a GitHub pull request or branch link of one of your repositories. Indent lines (Tab) to stack each on the line above."),
          ),
      )
      .child(options)
      .when(!self.errors.is_empty(), |this| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "open-workspaces-errors".into())
            .flex()
            .flex_col()
            .gap_1()
            .text_color(p.destructive)
            .children(self.errors.iter().enumerate().map(|(i, error)| {
              div()
                .id(("open-workspaces-error", i))
                .role(gpui::Role::Alert)
                .aria_label(SharedString::from(error.clone()))
                .test_support()
                .child(error.clone())
            })),
        )
      })
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(self.status(status, invalid, &checks, p, cx))
          .child(
            Button::new("open-workspaces-cancel", p)
              .variant(Variant::Outline)
              .disabled(self.pending)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.close(cx))),
          )
          .child(
            Button::new("open-workspaces-submit", p)
              .disabled(!self.can_submit())
              .label(if self.pending {
                "Creating...".to_string()
              } else {
                match targets.len() {
                  0 => "Create Workspaces".to_string(),
                  1 => "Create 1 Workspace".to_string(),
                  n => format!("Create {n} Workspaces"),
                }
              })
              .on_click(cx.listener(|view, _, _, cx| view.submit(cx))),
          ),
      )
  }

  /// What the lines and picks will open, beside the buttons; pressed, it
  /// lists each of them above it.
  fn status(
    &self,
    status: String,
    invalid: usize,
    checks: &[(String, Option<Checked>)],
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let muted = p.muted;
    let rows = checks.len() + self.chosen.len() + self.chosen_issues.len();
    // The invalid count goes under the summary, so neither runs under the
    // buttons.
    div()
      .relative()
      .flex()
      .flex_col()
      .flex_1()
      .min_w_0()
      .items_start()
      .when(self.lines_open, |this| {
        // The popover's anchor: the summary's top left corner.
        this.child(
          div()
            .absolute()
            .top_0()
            .left(zpx(-8.))
            .child(self.lines_popover(checks, p, cx)),
        )
      })
      .child(
        ui::text(div(), TextSize::Sm)
          .id("open-workspaces-status")
          .debug_selector(|| "open-workspaces-status".into())
          .role(gpui::Role::Status)
          .aria_label(SharedString::from(status.clone()))
          .aria_expanded(self.lines_open)
          .flex()
          .items_center()
          .gap_1()
          .px_2()
          .py_1()
          .ml_neg_2()
          .rounded(radius::MD)
          .font_weight(FontWeight::MEDIUM)
          .child(status)
          .when(rows > 0, |this| {
            this
              .cursor_pointer()
              .hover(move |s| s.bg(p.hover()))
              .when(self.lines_open, |this| this.bg(muted))
              .child(ui::icon(
                if self.lines_open {
                  theme::ICON_CHEVRON_DOWN
                } else {
                  theme::ICON_CHEVRON_UP
                },
                14.,
                p.muted_foreground,
              ))
              .on_click(cx.listener(|view, _, _, cx| view.toggle_lines(cx)))
          })
          .test_support(),
      )
      .when(invalid > 0, |this| {
        let invalid = plural(invalid, "1 line can't be opened", "lines can't be opened");
        this.child(
          ui::text(div(), TextSize::Sm)
            .id("open-workspaces-invalid")
            .role(gpui::Role::Alert)
            .aria_label(SharedString::from(invalid.clone()))
            .max_w_full()
            .truncate()
            .text_color(p.destructive)
            .child(invalid)
            .test_support(),
        )
      })
  }

  /// Each line and pick, and what it opens or why it can't; it opens
  /// upwards from the summary.
  fn lines_popover(
    &self,
    checks: &[(String, Option<Checked>)],
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let repo_path = &self.route.repo_path;
    let picks = self
      .chosen
      .iter()
      .map(|pr| {
        let existing = self.open_branches.contains(&pr.head_ref_name);
        (
          format!("open-workspaces-pr-{}", pr.number),
          Target::from_pr(repo_path, pr, existing),
        )
      })
      .chain(self.chosen_issues.iter().map(|issue| {
        let existing = self.open_branches.contains(&issue.branch_name);
        (
          format!("open-workspaces-issue-{}", issue.id),
          Target::from_issue(repo_path, issue, existing),
        )
      }));
    deferred(
      anchored().anchor(gpui::Anchor::BottomLeft).child(
        div()
          .id("open-workspaces-lines-popover")
          .debug_selector(|| "open-workspaces-lines-popover".into())
          .role(gpui::Role::List)
          .aria_label("Workspaces to open")
          .mb_1()
          .w(zpx(520.))
          .max_h(zpx(320.))
          .overflow_y_scroll()
          .p_2()
          .flex()
          .flex_col()
          .gap_1()
          .rounded(radius::MD)
          .border_1()
          .border_color(p.border)
          .bg(p.popover)
          .text_color(p.popover_foreground)
          .shadow_md()
          .occlude()
          .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .on_mouse_down_out(cx.listener(|view, _, _, cx| view.close_lines(cx)))
          .children(
            checks
              .iter()
              .enumerate()
              .map(|(i, (raw, checked))| result_row(i, raw, checked.as_ref(), repo_path, p)),
          )
          .children(picks.map(|(id, target)| {
            target_row(SharedString::from(id), &target, repo_path, p).into_any_element()
          }))
          .test_support(),
      ),
    )
    .with_priority(3)
  }

  /// The integrations' column: a tab for each, with a search and a list to
  /// pick from.
  fn integrations_column(
    &self,
    fields: (&Entity<InputState>, &Entity<InputState>),
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let current = self.integration();
    let integrations = self.integrations();
    let selected = integrations
      .iter()
      .position(|tab| Some(*tab) == current)
      .unwrap_or(0);
    let tabs = ui::Tabs::segmented("open-workspaces-tabs", p)
      .tabs(integrations.iter().map(|tab| {
        let (icon, label, picked) = match tab {
          Integration::Github => (theme::ICON_GITHUB, "GitHub", self.chosen.len()),
          Integration::Linear => (theme::ICON_LINEAR, "Linear", self.chosen_issues.len()),
        };
        let tab = ui::Tab::new(format!("open-workspaces-tab-{label}"), label).icon(icon);
        if picked > 0 {
          tab.count(picked)
        } else {
          tab
        }
      }))
      .selected(selected)
      .on_select({
        let view = cx.weak_entity();
        move |ix, _, cx| {
          let tab = integrations[ix];
          view
            .update(cx, |view, cx| {
              view.picked = Some(tab);
              cx.notify();
            })
            .ok();
        }
      });
    div()
      .flex()
      .flex_col()
      .w(zpx(420.))
      .flex_none()
      .min_h_0()
      .border_l_1()
      .border_color(p.border)
      .pl_4()
      .gap_2()
      .child(tabs)
      .map(|this| match current {
        Some(Integration::Linear) => this.child(self.issues_pane(fields.1, p, cx)),
        _ => this.child(self.prs_pane(fields.0, p, cx)),
      })
  }

  /// The Linear tab: a search and the issues to pick from.
  fn issues_pane(&self, search: &Entity<InputState>, p: &Palette, cx: &mut Context<Self>) -> Div {
    let (items, mine) = data::issue_list(
      &self.issues.items,
      self.issues.viewer.as_deref(),
      &self.issue_query,
    );
    let heading = if !self.issue_query.trim().is_empty() {
      "Matching issues"
    } else if mine {
      "Your issues"
    } else {
      "Open issues"
    };
    let list: gpui::AnyElement = if self.issues.loading && self.issues.items.is_empty() {
      crate::gpui_shell::ui::delayed(
        "open-workspaces-issues-loading",
        div().py_12().child(crate::gpui_shell::ui::Loading::new(
          "open-workspaces-issues-loading-spinner",
          p,
        )),
      )
      .into_any_element()
    } else if let Some(err) = &self.issues.error {
      parts::error_state(
        "open-workspaces-issues",
        "Could not load Linear issues",
        err,
        p,
        Some(Box::new(cx.listener(|view, _, _, cx| view.load_issues(cx)))),
      )
      .into_any_element()
    } else if items.is_empty() {
      crate::gpui_shell::ui::EmptyState::new("open-workspaces-no-open-issues", "No open issues.", p)
        .icon(theme::ICON_LINEAR)
        .into_any_element()
    } else {
      div()
        .children(items.into_iter().map(|issue| self.issue_row(issue, p, cx)))
        .into_any_element()
    };
    pane(
      "open-workspaces-issues",
      heading,
      self.chosen_issues.len(),
      Input::new(search).aria_label("Search issues"),
      None,
      list,
      p,
    )
  }

  /// The GitHub tab: a search and the pull requests to pick from.
  fn prs_pane(&self, search: &Entity<InputState>, p: &Palette, cx: &mut Context<Self>) -> Div {
    let heading = if !self.query.trim().is_empty() {
      "Matching pull requests"
    } else if self.assigned {
      "Assigned to you"
    } else if self.prs.mine {
      "Your pull requests"
    } else {
      "Open pull requests"
    };
    let list: gpui::AnyElement = if self.prs.loading && self.prs.items.is_empty() {
      crate::gpui_shell::ui::delayed(
        "open-workspaces-prs-loading",
        div().py_12().child(crate::gpui_shell::ui::Loading::new(
          "open-workspaces-prs-loading-spinner",
          p,
        )),
      )
      .into_any_element()
    } else if let Some(err) = &self.prs.error {
      parts::error_state(
        "open-workspaces-prs",
        "Could not load pull requests",
        err,
        p,
        Some(Box::new(cx.listener(|view, _, _, cx| view.load_prs(cx)))),
      )
      .into_any_element()
    } else if self.prs.items.is_empty() {
      let query = self.query.trim();
      let (id, message) = if query.is_empty() && self.assigned {
        (
          "open-workspaces-no-assigned-pull-requests",
          "No open pull requests are assigned to you.".to_string(),
        )
      } else if query.is_empty() {
        (
          "open-workspaces-no-open-pull-requests",
          "No open pull requests.".to_string(),
        )
      } else {
        (
          "open-workspaces-no-matching-pull-requests",
          format!("No pull requests match \"{query}\"."),
        )
      };
      crate::gpui_shell::ui::EmptyState::new(id, message, p)
        .icon(theme::ICON_PR)
        .into_any_element()
    } else {
      // The last answer stays readable, dimmed, while a search replaces it.
      div()
        .when(self.prs.loading, |d| d.opacity(0.5))
        .children(self.prs.items.iter().map(|pr| self.pr_row(pr, p, cx)))
        .into_any_element()
    };
    // The centered spinner covers a search with nothing on screen yet.
    let searching = (self.prs.loading && !self.prs.items.is_empty()).then(|| {
      div()
        .id("open-workspaces-prs-searching")
        .role(gpui::Role::ProgressIndicator)
        .aria_label("Searching pull requests")
        .pr_1()
        .child(ui::spinner(
          "open-workspaces-prs-searching-spinner",
          14.,
          p.muted_foreground,
        ))
        .test_support()
    });
    let assigned = Button::new("open-workspaces-assigned", p)
      .variant(Variant::Outline)
      .size(ui::Size::Sm)
      .h(zpx(28.))
      .icon(theme::ICON_USER, 12.)
      .label("Assigned to me")
      .pressed(self.assigned)
      .on_click(cx.listener(|view, _, _, cx| {
        view.assigned = !view.assigned;
        view.load_prs(cx);
      }));
    pane(
      "open-workspaces-prs",
      heading,
      self.chosen.len(),
      Input::new(search)
        .aria_label("Search pull requests")
        .when_some(searching, |input, spinner| input.suffix(spinner)),
      Some(assigned.into_any_element()),
      list,
      p,
    )
  }

  /// A pull request with its checkbox: `PrListItem` without the branches,
  /// which do not fit.
  fn pr_row(&self, pr: &GhPullRequest, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let checked = self.chosen.iter().any(|c| c.number == pr.number);
    let pr_for_click = pr.clone();
    pick_row(
      SharedString::from(format!("open-workspaces-pr-{}", pr.number)),
      format!("#{} {}", pr.number, pr.title),
      checked,
      self.pending,
      parts::pr_list_item(pr, false, p),
      p,
    )
    .on_click(cx.listener(move |view, _, _, cx| {
      if !view.pending {
        view.toggle_pr(&pr_for_click, cx);
      }
    }))
  }

  /// A Linear issue with its checkbox.
  fn issue_row(
    &self,
    issue: &LinearIssue,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let checked = self.chosen_issues.iter().any(|c| c.id == issue.id);
    let issue_for_click = issue.clone();
    pick_row(
      SharedString::from(format!("open-workspaces-issue-{}", issue.id)),
      format!("{} {}", issue.identifier, issue.title),
      checked,
      self.pending,
      linear::issue_list_item(issue, p),
      p,
    )
    .on_click(cx.listener(move |view, _, _, cx| {
      if !view.pending {
        view.toggle_issue(&issue_for_click, cx);
      }
    }))
  }
}

/// A tab's heading, search with an optional filter beside it, and list.
fn pane(
  id: &'static str,
  heading: &'static str,
  picked: usize,
  search: Input,
  filter: Option<gpui::AnyElement>,
  list: gpui::AnyElement,
  p: &Palette,
) -> Div {
  div()
    .flex()
    .flex_col()
    .flex_1()
    .min_h_0()
    .gap_2()
    .child(
      div()
        .flex()
        .items_center()
        .justify_between()
        .child(
          div()
            .id(SharedString::from(format!("{id}-heading")))
            .role(gpui::Role::Heading)
            .aria_label(heading)
            .child(label_xs(heading))
            .test_support(),
        )
        .when(picked > 0, |this| {
          this.child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(format!("{picked} selected")),
          )
        }),
    )
    .child(
      div()
        .flex()
        .items_center()
        .gap_2()
        .child(
          div()
            .flex_1()
            .min_w_0()
            .debug_selector(move || format!("{id}-search"))
            .child(ui::text(
              Styled::h(
                search.prefix(ui::icon(theme::ICON_SEARCH, 14., p.muted_foreground)),
                zpx(28.),
              ),
              TextSize::Sm,
            )),
        )
        .children(filter),
    )
    .child(
      div()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .rounded_md()
        .border_1()
        .border_color(p.border)
        .child(list),
    )
}

/// A list row with its checkbox, named `name`.
fn pick_row(
  id: SharedString,
  name: String,
  checked: bool,
  disabled: bool,
  item: Div,
  p: &Palette,
) -> ui::Row {
  ui::Row::new(SharedString::from(format!("{id}-row")), p)
    .role(gpui::Role::CheckBox)
    .label(name)
    .aria_toggled(if checked {
      gpui::accesskit::Toggled::True
    } else {
      gpui::accesskit::Toggled::False
    })
    .density(ui::RowDensity::Default)
    .selected(checked)
    .w_full()
    .px_3()
    .py_2p5()
    .border_b_1()
    .border_color(p.border)
    .rounded_none()
    .items_start()
    .leading(
      div().mt_1().child(
        ui::Checkbox::new(SharedString::from(format!("{id}-check")), p)
          .checked(checked)
          .disabled(disabled),
      ),
    )
    .child(item)
}

/// How the dialog's targets are opened or made.
struct Plan {
  repo_path: String,
  stack_on: Option<String>,
  sparse_paths: String,
  symlinked_dirs: String,
}

impl Plan {
  /// Opens or makes the targets in order, so a stacked line's parent comes
  /// first: runs of existing branches of one repository at a time, new
  /// branches on their base or the Stack on branch, and Linear issues as the
  /// Linear view opens them; each result in the targets' order.
  fn run(&self, host: &dyn OpenWorkspacesHost, targets: Vec<Target>) -> Vec<(Target, Opened)> {
    let mut results: Vec<Option<Opened>> = vec![None; targets.len()];
    let mut batch: Vec<usize> = Vec::new();
    for i in 0..=targets.len() {
      let next = targets.get(i);
      let joins = next.is_some_and(|t| {
        t.issue.is_none()
          && !t.create
          && batch
            .first()
            .is_none_or(|&b| targets[b].repo_path == t.repo_path)
      });
      if joins {
        batch.push(i);
        continue;
      }
      if let Some(&first) = batch.first() {
        let repo = &targets[first].repo_path;
        let (_, stack_on) = self.bases(host, repo);
        let requests: Vec<_> = batch
          .iter()
          .map(|&b| targets[b].request(&stack_on))
          .collect();
        for (b, opened) in batch.drain(..).zip(host.open_workspaces(repo, &requests)) {
          results[b] = Some(opened);
        }
      }
      let Some(target) = next else { break };
      results[i] = Some(if let Some(issue) = &target.issue {
        host.open_issue(&target.repo_path, issue)
      } else if target.create {
        self.create(host, target)
      } else {
        batch.push(i);
        continue;
      });
    }
    targets
      .into_iter()
      .zip(results)
      .map(|(t, r)| (t, r.unwrap_or_else(|| Err("Not opened".into()))))
      .collect()
  }

  /// The repository's default branch, and the branch it stacks on: the
  /// Stack on select's, which only the repository on screen has.
  fn bases(&self, host: &dyn OpenWorkspacesHost, repo: &str) -> (String, String) {
    let default = WorkspaceDialogHost::default_branch(host, repo).unwrap_or_default();
    let stack_on = Some(repo)
      .filter(|r| *r == self.repo_path)
      .and(self.stack_on.clone())
      .unwrap_or_else(|| default.clone());
    (default, stack_on)
  }

  /// Makes the target's branch on its base, or on the Stack on branch.
  fn create(&self, host: &dyn OpenWorkspacesHost, target: &Target) -> Opened {
    let (default, stack_on) = self.bases(host, &target.repo_path);
    let metadata = dialog_data::create_metadata(
      target.title.as_deref().unwrap_or_default(),
      "",
      None,
      &self.sparse_paths,
      &self.symlinked_dirs,
    );
    // The default branch is where a plain workspace starts already.
    let on = Some(target.base.as_deref().unwrap_or(&stack_on)).filter(|b| *b != default);
    let mut created = None;
    let made = dialog_data::create_on_target(
      host,
      &target.repo_path,
      &target.branch,
      None,
      on,
      &metadata,
      &mut created,
    );
    match (made, created) {
      (Ok(id), _) => Ok((id, true)),
      (Err(err), Some(id)) => {
        log::warn!("new workspaces: {} was made, but: {err}", target.branch);
        Ok((id, true))
      }
      (Err(err), None) => Err(err),
    }
  }
}

/// `n` as "1 thing" or "n things".
fn plural(n: usize, one: &str, many: &str) -> String {
  if n == 1 {
    one.to_string()
  } else {
    format!("{n} {many}")
  }
}

/// What the dialog will open, before it does.
fn summary_future(new: usize, existing: usize) -> String {
  let created = plural(
    new,
    "1 workspace will be created",
    "workspaces will be created",
  );
  match (new, existing) {
    (_, 0) => created,
    (0, _) => plural(
      existing,
      "1 workspace is already open",
      "workspaces are already open",
    ),
    _ => format!("{created}, {existing} already open"),
  }
}

/// What the dialog opened.
fn summary(created: usize, reused: usize) -> String {
  match (created, reused) {
    (c, 0) => format!("Created {}.", plural(c, "1 workspace", "workspaces")),
    (0, r) => format!(
      "Opened {}.",
      plural(r, "1 existing workspace", "existing workspaces")
    ),
    (c, r) => format!(
      "Created {}, opened {r} existing.",
      plural(c, "1 workspace", "workspaces")
    ),
  }
}

/// A pasted line and what it opens, or why it can't.
fn result_row(
  index: usize,
  raw: &str,
  checked: Option<&Checked>,
  repo_path: &str,
  p: &Palette,
) -> gpui::AnyElement {
  let id = SharedString::from(format!("open-workspaces-line-{index}"));
  match checked {
    Some(Ok(target)) => target_row(id, target, repo_path, p).into_any_element(),
    Some(Err(reason)) => row(id, format!("{raw}: {reason}"), p)
      .child(ui::icon(theme::ICON_CIRCLE_ALERT, 14., p.destructive))
      .child(mono(raw))
      .child(
        div()
          .min_w_0()
          .truncate()
          .text_color(p.destructive)
          .child(reason.clone()),
      )
      .into_any_element(),
    None => row(id.clone(), format!("{raw}: checking"), p)
      .child(ui::spinner(id, 14., p.muted_foreground))
      .child(mono(raw))
      .into_any_element(),
  }
}

fn target_row(id: SharedString, target: &Target, repo_path: &str, p: &Palette) -> impl IntoElement {
  let mut detail = Vec::new();
  if let Some(n) = target.pr {
    detail.push(format!("PR #{n}"));
  }
  if target.repo_path != repo_path {
    detail.push(repo_name(&target.repo_path).to_string());
  }
  if let Some(base) = target.base.as_ref().filter(|_| target.pr.is_none()) {
    detail.push(format!("on {base}"));
  }
  detail.push(
    if target.existing {
      "already open"
    } else if target.create {
      "new branch"
    } else {
      "new"
    }
    .to_string(),
  );
  let detail = detail.join(" · ");
  row(id, format!("{}: {detail}", target.branch), p)
    .child(ui::icon(theme::ICON_CHECK, 14., p.muted_foreground))
    .child(mono(&target.branch))
    .child(
      div()
        .min_w_0()
        .truncate()
        .text_color(p.muted_foreground)
        .child(detail),
    )
}

/// A line of the list under the text, named by its line and what it opens.
/// It takes no click.
fn row(id: SharedString, name: String, p: &Palette) -> ui::Row {
  ui::Row::new(id, p)
    .label(name)
    .interactive(false)
    .px_0()
    .min_w_0()
}

fn mono(text: &str) -> Div {
  div()
    .flex_none()
    .max_w(zpx(260.))
    .truncate()
    .font_family(theme::MONO_FONT)
    .child(text.to_string())
}

/// A repository's folder name.
fn repo_name(path: &str) -> &str {
  path
    .trim_end_matches('/')
    .rsplit('/')
    .next()
    .unwrap_or(path)
}

impl Render for OpenWorkspacesDialog {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let (lines, search, issue_search, sparse, symlinks) = {
      let fields = self.fields(window, cx);
      (
        fields.lines.clone(),
        fields.search.clone(),
        fields.issue_search.clone(),
        fields.sparse.clone(),
        fields.symlinks.clone(),
      )
    };
    let select = self.stack_select(window, cx);
    let options = self.create_options(select, &sparse, &symlinks, &p, cx);
    let split = self.integration().is_some();
    let viewport = window.viewport_size();
    let (backdrop, panel) = dialog_panel(
      "open-workspaces-dialog",
      if split { 1040. } else { 560. },
      viewport,
      &p,
    );
    let keys = cx.listener(|view, e: &KeyDownEvent, _, cx| {
      let k = &e.keystroke;
      let held = crate::gpui_shell::keys::held(k.modifiers);
      if ui::is_escape(k) {
        cx.stop_propagation();
        if view.lines_open {
          view.close_lines(cx);
        } else {
          view.close(cx);
        }
      } else if k.key == "enter" && (held.platform || held.control) {
        cx.stop_propagation();
        view.submit(cx);
      }
    });
    let body = div()
      .flex()
      .gap_4()
      .h(zpx(500.))
      .max_h(viewport.height - px(160.))
      .pt_4()
      .child(self.lines_column(&lines, options, &p, cx))
      .when(split, |this| {
        this.child(self.integrations_column((&search, &issue_search), &p, cx))
      });
    let panel = panel
      .child(dialog_header(
        "New Workspaces",
        dialog_description(
          if split {
            "Create a workspace for each line, and for each item you pick."
          } else {
            "Create a workspace for each line."
          },
          &p,
        ),
      ))
      .child(body);
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        backdrop
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, _, cx| {
              if !view.pending {
                view.close(cx)
              }
            }),
          )
          .child(
            panel
              .id("open-workspaces-panel")
              .track_focus(&self.focus)
              .capture_key_down(keys)
              .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
          ),
      ),
    )
    .with_priority(2)
    .into_any_element()
  }
}
