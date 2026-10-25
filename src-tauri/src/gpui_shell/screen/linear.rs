//! `LinearPanel`: the repository's Linear issues (as a list or a Kanban
//! board) and projects with their documents, each with its comments and the
//! review agent's findings.
//!
//! Drawn from the backend directly (`linear_data::LinearHost`): GPUI reads
//! Linear through the backend's client, writes review snapshots, and
//! resolves, deletes and applies review comments, and starts the review
//! agent itself. React still owns the prompt dialog, so GPUI hands back an
//! issue to kick off (`linear.kickoff`) for React to open it with.
//!
//! Issues are created from the toolbar's "New issue", and edited or deleted
//! from their page.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{
  cell::RefCell,
  collections::{HashMap, HashSet},
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use gpui_kit::base::TestSupportExt as _;
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, AnyElement, App, Bounds, ClickEvent, Context, Div, FocusHandle,
  FontWeight, Hsla, Pixels, Role, SharedString, Stateful, Task, Window,
};

use super::{
  changes::threads::suggestion_box,
  code::markdown,
  github::parts::busy_spinner,
  integrations_cache::cache,
  linear_data::{
    self as data, agent_count_label, comment_file, Anchor, Commented, FilterOption, IssueFilters,
    IssueView, LinearHost, ProjectFilters, ProjectView, Route,
  },
  modal::{dialog, overlay, title},
  rows::Rows,
  ActionSink, ScreenAction,
};
use crate::{
  core::linear_review::BODY_FILE,
  gpui_shell::{
    form::{self, Controls, SelectOption},
    native::{NativeEvent, ToastTone},
    popup::{self, NativeMenuItem, PopupRequest, PopupSink},
    theme::{self, Palette},
    time,
    ui::{self, Button, Size, TextSize, Variant},
  },
  linear::{LinearComment, LinearDocument, LinearIssue, LinearProject, LinearTeam, LinearUser},
  local_db::AgentReviewComment,
};

/// How often the review agent's comments are read again (`refreshMs`).
const REVIEW_POLL: Duration = Duration::from_secs(10);
/// How long the issues a visit read stand in for reading them again.
const ISSUES_FRESH: Duration = Duration::from_secs(30);

/// `Issues` or `Projects`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
  #[default]
  Issues,
  Projects,
}

/// The issue list's `List` / `Kanban` switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IssueMode {
  #[default]
  List,
  Kanban,
}

/// One SWR read: what it returned last, why it failed, and whether a read
/// is running.
#[derive(Debug, Clone)]
pub struct Fetch<T> {
  pub data: Option<T>,
  /// The last read's error, which says how to connect Linear when it is not.
  pub error: Option<String>,
  fetching: bool,
}

impl<T> Default for Fetch<T> {
  fn default() -> Self {
    Self {
      data: None,
      error: None,
      fetching: false,
    }
  }
}

impl<T> Fetch<T> {
  /// SWR's `isLoading`: reading, with nothing read yet.
  pub fn is_loading(&self) -> bool {
    self.fetching && self.data.is_none()
  }

  pub fn failed(&self) -> bool {
    self.error.is_some()
  }

  fn settle(&mut self, result: Result<T, String>, what: &str) {
    self.fetching = false;
    match result {
      Ok(data) => {
        self.data = Some(data);
        self.error = None;
      }
      Err(err) => {
        log::warn!("Failed to load Linear {what}: {err}");
        self.error = Some(err);
      }
    }
  }
}

/// A text the agent can review: an issue, a project or a document.
#[derive(Debug, Clone)]
struct Target {
  of: Commented,
  title: String,
  url: String,
  /// The text itself, empty when it has none.
  body: String,
  /// What the content column shows: the body, or its placeholder.
  shown: String,
}

/// A filter of the Filter menu: its values and the one picked.
struct FilterGroup {
  key: &'static str,
  label: &'static str,
  value: Option<String>,
  options: Vec<FilterOption>,
}

impl FilterGroup {
  /// The picked value's label, or the value itself.
  fn value_label(&self) -> Option<String> {
    let value = self.value.as_ref()?;
    Some(
      self
        .options
        .iter()
        .find(|o| &o.value == value)
        .map_or_else(|| value.clone(), |o| o.label.clone()),
    )
  }
}

/// The Linear view and everything it read.
pub struct LinearView {
  host: Arc<dyn LinearHost>,
  react: ActionSink,
  popups: PopupSink,
  route: Route,
  dark: bool,
  focus: FocusHandle,
  /// Buttons' bounds, for the dropdowns below them.
  anchors: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
  pub(super) section: Section,
  /// The team key the issues are read for; `None` for all teams.
  pub(super) team: Option<String>,
  teams: Fetch<Vec<LinearTeam>>,
  viewer: Option<LinearUser>,
  /// The issues, by team.
  pub(super) issues: HashMap<Option<String>, Fetch<Rc<Vec<LinearIssue>>>>,
  pub(super) mode: IssueMode,
  pub(super) issue_view: IssueView,
  pub(super) issue_filters: IssueFilters,
  /// The issue whose page shows, in place of the list.
  pub(super) open_issue: Option<String>,
  /// The New issue form, while open.
  new_issue: Option<NewIssue>,
  /// The open issue's title and description being edited.
  editing: Option<(String, String)>,
  saving: bool,
  /// The delete confirmation is open.
  pub(super) confirm_delete: bool,
  deleting: bool,
  delete_focused: bool,
  controls: Controls,
  /// Fields whose controls show the view's value even while focused.
  reset: HashSet<&'static str>,
  /// The issue list's rows, and each Kanban column's, by its name.
  issue_rows: Rows<IssueRow>,
  column_rows: HashMap<String, Rows<IssueRow>>,
  comments: HashMap<Commented, Fetch<Vec<LinearComment>>>,
  /// Comment threads whose fold is open: every comment shows.
  comments_expanded: HashSet<Commented>,
  pub(super) projects: Fetch<Vec<LinearProject>>,
  pub(super) project_view: ProjectView,
  pub(super) project_filters: ProjectFilters,
  selected_project: Option<String>,
  /// The project whose documents and comments were read last.
  shown_project: Option<String>,
  documents: HashMap<String, Fetch<Vec<LinearDocument>>>,
  /// The document dialog's document, while open.
  pub(super) open_document: Option<LinearDocument>,
  dialog_focused: bool,
  /// The review agent's comments, by reviewed text.
  pub(super) reviews: HashMap<Commented, Vec<AgentReviewComment>>,
  /// Review comments with an action running.
  busy: HashSet<String>,
  /// Texts whose review is being prepared.
  pub(super) starting: HashSet<Commented>,
  _poll: Task<()>,
}

/// The New issue form's fields.
#[derive(Debug, Clone, Default)]
struct NewIssue {
  /// The team picked; `None` for the default (`create_team`).
  team_id: Option<String>,
  title: String,
  description: String,
  pending: bool,
  error: Option<String>,
}

/// An issue's row in the list or a Kanban column: the issue, by its index
/// in the team's issues and its id, and how the row draws it.
#[derive(Debug, Clone, PartialEq)]
struct IssueRow {
  ix: usize,
  id: String,
  indent: bool,
  /// The parent's identifier, which a Kanban card names.
  parent: Option<String>,
}

impl IssueRow {
  fn new(
    index: &HashMap<&str, usize>,
    issue: &LinearIssue,
    indent: bool,
    parent: Option<String>,
  ) -> Self {
    Self {
      ix: index.get(issue.id.as_str()).copied().unwrap_or(usize::MAX),
      id: issue.id.clone(),
      indent,
      parent,
    }
  }
}

/// Each issue's index in the team's issues, by id.
fn issue_index(all: &[LinearIssue]) -> HashMap<&str, usize> {
  all
    .iter()
    .enumerate()
    .map(|(i, issue)| (issue.id.as_str(), i))
    .collect()
}

impl LinearView {
  pub fn new(
    route: Route,
    host: Arc<dyn LinearHost>,
    react: ActionSink,
    dark: bool,
    popups: PopupSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let poll = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(REVIEW_POLL).await;
      if this.update(cx, |view, cx| view.poll_reviews(cx)).is_err() {
        return;
      }
    });
    let view = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      let view = view.clone();
      cx.defer(move |cx| {
        view
          .update(cx, |view, cx| view.handle_field(action, cx))
          .ignore_closed();
      });
    });
    let mut this = Self {
      host,
      react,
      popups,
      route,
      dark,
      focus: cx.focus_handle(),
      anchors: Rc::default(),
      section: Section::Issues,
      team: None,
      teams: Fetch::default(),
      viewer: None,
      issues: HashMap::new(),
      mode: IssueMode::List,
      issue_view: IssueView::All,
      issue_filters: IssueFilters::default(),
      open_issue: None,
      new_issue: None,
      editing: None,
      saving: false,
      confirm_delete: false,
      deleting: false,
      delete_focused: false,
      controls: Controls::new(sink),
      reset: HashSet::new(),
      issue_rows: Rows::default(),
      column_rows: HashMap::new(),
      comments: HashMap::new(),
      comments_expanded: HashSet::new(),
      projects: Fetch::default(),
      project_view: ProjectView::All,
      project_filters: ProjectFilters::default(),
      selected_project: None,
      shown_project: None,
      documents: HashMap::new(),
      open_document: None,
      dialog_focused: false,
      reviews: HashMap::new(),
      busy: HashSet::new(),
      starting: HashSet::new(),
      _poll: poll,
    };
    this.load_all(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route != route {
      let fresh = Self::new(
        route,
        self.host.clone(),
        self.react.clone(),
        dark,
        self.popups.clone(),
        cx,
      );
      *self = fresh;
    }
    self.dark = dark;
    cx.notify();
  }

  /// What the panel reads when it opens.
  fn load_all(&mut self, cx: &mut Context<Self>) {
    // What the last visit read shows in the first frame, under the reads.
    let repo = self.route.repo_path.clone();
    let cached = cache(cx);
    let issues = cached.linear_issues.get(&(repo.clone(), None)).cloned();
    self.viewer = cached.linear_viewer.get(&repo).cloned();
    if let Some(issues) = issues {
      self.issues.entry(None).or_default().data = Some(issues);
    }
    self.run(
      |host, repo| host.linear_teams(repo),
      |view, result, _| view.teams.settle(result, "teams"),
      cx,
    );
    self.teams.fetching = true;
    self.run(
      |host, repo| host.linear_viewer(repo),
      |view, result, cx| {
        if let Ok(viewer) = &result {
          cache(cx).linear_viewer.insert(
            view.route.repo_path.clone(),
            Instant::now(),
            viewer.clone(),
          );
        }
        view.viewer = result.ok();
      },
      cx,
    );
    self.load_issues_unless_fresh(cx);
  }

  /// Runs `work` off the main thread, then `apply` on the view, unless the
  /// view moved on to another repository meanwhile.
  fn run<T: Send + 'static>(
    &self,
    work: impl FnOnce(&dyn LinearHost, &str) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let (host, repo) = (self.host.clone(), self.route.repo_path.clone());
    let key = repo.clone();
    cx.spawn(async move |this, cx| {
      let value = cx
        .background_spawn(async move { work(host.as_ref(), &repo) })
        .await;
      this
        .update(cx, |view, cx| {
          if view.route.repo_path == key {
            apply(view, value, cx);
            cx.notify();
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Reads the issues again, unless a read the cache holds is under
  /// [`ISSUES_FRESH`] old: opening the panel, switching the section or the
  /// team would otherwise repeat every page of a long list each time.
  fn load_issues_unless_fresh(&mut self, cx: &mut Context<Self>) {
    let key = (self.route.repo_path.clone(), self.team.clone());
    let fresh = cache(cx)
      .linear_issues
      .age(&key, Instant::now())
      .is_some_and(|age| age < ISSUES_FRESH);
    if fresh
      && self
        .issues
        .get(&self.team)
        .is_some_and(|f| f.data.is_some())
    {
      return;
    }
    self.load_issues(cx);
  }

  fn load_issues(&mut self, cx: &mut Context<Self>) {
    let team = self.team.clone();
    let seed = cache(cx)
      .linear_issues
      .get(&(self.route.repo_path.clone(), team.clone()))
      .cloned();
    let entry = self.issues.entry(team.clone()).or_default();
    entry.fetching = true;
    if let Some(issues) = seed.filter(|_| entry.data.is_none()) {
      entry.data = Some(issues);
    }
    let key = team.clone();
    self.run(
      move |host, repo| host.linear_issues(repo, team.as_deref()),
      move |view, result, cx| {
        let result = result.map(Rc::new);
        if let Ok(issues) = &result {
          cache(cx).linear_issues.insert(
            (view.route.repo_path.clone(), key.clone()),
            Instant::now(),
            issues.clone(),
          );
        }
        view.issues.entry(key).or_default().settle(result, "issues")
      },
      cx,
    );
  }

  fn load_projects(&mut self, cx: &mut Context<Self>) {
    self.projects.fetching = true;
    self.run(
      |host, repo| host.linear_projects(repo),
      |view, result, _| view.projects.settle(result, "projects"),
      cx,
    );
  }

  fn load_documents(&mut self, project: String, cx: &mut Context<Self>) {
    self.documents.entry(project.clone()).or_default().fetching = true;
    let key = project.clone();
    self.run(
      move |host, repo| host.linear_documents(repo, &project),
      move |view, result, _| {
        view
          .documents
          .entry(key)
          .or_default()
          .settle(result, "documents")
      },
      cx,
    );
  }

  /// Reads the comments on `of`, and the review agent's comments too.
  fn load_comments(&mut self, of: Commented, cx: &mut Context<Self>) {
    self.comments.entry(of.clone()).or_default().fetching = true;
    let key = of.clone();
    self.run(
      {
        let of = of.clone();
        move |host, repo| host.linear_comments(repo, &of)
      },
      move |view, result, _| {
        view
          .comments
          .entry(key)
          .or_default()
          .settle(result, "comments")
      },
      cx,
    );
    self.load_reviews(of, cx);
  }

  fn load_reviews(&mut self, of: Commented, cx: &mut Context<Self>) {
    let key = of.clone();
    self.run(
      move |host, repo| host.linear_review_comments(repo, &of),
      move |view, result, _| match result {
        Ok(comments) => {
          view.reviews.insert(key, comments);
        }
        Err(err) => log::warn!("Failed to read agent review comments: {err}"),
      },
      cx,
    );
  }

  /// The texts shown now, whose review comments the poll reads.
  fn shown_texts(&self) -> Vec<Commented> {
    let mut shown: Vec<Commented> = Vec::new();
    if self.section == Section::Issues {
      shown.extend(self.open_issue.clone().map(Commented::Issue));
    }
    if self.section == Section::Projects {
      shown.extend(self.shown_project.clone().map(Commented::Project));
    }
    shown.extend(
      self
        .open_document
        .as_ref()
        .map(|d| Commented::Document(d.id.clone())),
    );
    shown
  }

  fn poll_reviews(&mut self, cx: &mut Context<Self>) {
    for of in self.shown_texts() {
      self.load_reviews(of, cx);
    }
  }

  fn toast(&self, title: &str, description: impl Into<String>) {
    super::workspace_data::WorkspaceHost::notify(
      self.host.as_ref(),
      NativeEvent::Toast {
        title: title.into(),
        description: description.into(),
        tone: ToastTone::Error,
        action: None,
      },
    );
  }

  fn send_react(&self, action: ScreenAction, cx: &mut App) {
    let react = self.react.clone();
    cx.defer(move |cx| react(action, cx));
  }

  fn field(&self, field: &str, value: serde_json::Value, cx: &mut App) {
    self.send_react(
      ScreenAction::Field {
        field: field.into(),
        value,
      },
      cx,
    );
  }

  /// Settings' Integrations tab, where Linear is connected.
  fn open_settings(&self, cx: &mut App) {
    self.send_react(
      ScreenAction::Press {
        id: "linear.openSettings".into(),
      },
      cx,
    );
  }

  fn open_url(&self, url: &str) {
    self.host.open_url(url);
  }

  fn popup(&self, request: PopupRequest, cx: &mut App) {
    let popups = self.popups.clone();
    cx.defer(move |cx| popups(request, cx));
  }

  // ---- What the controls do. ----

  pub(super) fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
    if self.section == section {
      return;
    }
    self.section = section;
    // React unmounts the other section, which forgets its own state and
    // reads again (SWR keeps what it read before).
    match section {
      Section::Issues => {
        self.mode = IssueMode::List;
        self.issue_view = IssueView::All;
        self.issue_filters = IssueFilters::default();
        self.close_issue_forms();
        self.open_issue = None;
        self.load_issues_unless_fresh(cx);
      }
      Section::Projects => {
        self.project_view = ProjectView::All;
        self.project_filters = ProjectFilters::default();
        self.selected_project = None;
        self.shown_project = None;
        self.open_document = None;
        self.load_projects(cx);
      }
    }
    cx.notify();
  }

  pub(super) fn set_team(&mut self, team: Option<String>, cx: &mut Context<Self>) {
    if self.team == team {
      return;
    }
    self.team = team;
    self.close_issue_forms();
    self.open_issue = None;
    self.load_issues_unless_fresh(cx);
    cx.notify();
  }

  fn set_issue_filter(&mut self, key: &str, value: Option<String>, cx: &mut Context<Self>) {
    let f = &mut self.issue_filters;
    match key {
      "assignee" => f.assignee = value,
      "priority" => f.priority = value.and_then(|v| v.parse().ok()),
      "label" => f.label = value,
      "project" => f.project = value,
      _ => {}
    }
    cx.notify();
  }

  fn set_project_filter(&mut self, key: &str, value: Option<String>, cx: &mut Context<Self>) {
    match key {
      "status" => self.project_filters.state = value,
      "lead" => self.project_filters.lead = value,
      _ => {}
    }
    cx.notify();
  }

  /// Shows the issue's page, with its description and comments.
  pub(super) fn open_issue(&mut self, id: &str, cx: &mut Context<Self>) {
    self.new_issue = None;
    self.editing = None;
    self.open_issue = Some(id.to_string());
    self.load_comments(Commented::Issue(id.to_string()), cx);
    cx.notify();
  }

  /// Back from the issue's page to the list or board.
  pub(super) fn close_issue(&mut self, cx: &mut Context<Self>) {
    self.open_issue = None;
    self.editing = None;
    cx.notify();
  }

  fn close_issue_forms(&mut self) {
    self.new_issue = None;
    self.editing = None;
    self.confirm_delete = false;
  }

  /// A form control changed (`Controls` reports it as a field).
  fn handle_field(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    let ScreenAction::Field { field, value } = action else {
      return;
    };
    let text = value.as_str().unwrap_or_default().to_string();
    match field.as_str() {
      "linear.new.team" => {
        if let Some(form) = &mut self.new_issue {
          form.team_id = Some(text);
        }
      }
      "linear.new.title" => {
        if let Some(form) = &mut self.new_issue {
          form.title = text;
        }
      }
      "linear.new.description" => {
        if let Some(form) = &mut self.new_issue {
          form.description = text;
        }
      }
      "linear.edit.title" => {
        if let Some((title, _)) = &mut self.editing {
          *title = text;
        }
      }
      "linear.edit.description" => {
        if let Some((_, description)) = &mut self.editing {
          *description = text;
        }
      }
      _ => {}
    }
    cx.notify();
  }

  fn input(
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

  fn textarea(
    &mut self,
    field: &'static str,
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> gpui::Entity<gpui_kit::component::input::TextareaState> {
    let state = self
      .controls
      .textarea(field, value, placeholder, 6, window, cx);
    if self.reset.remove(field) {
      let value = value.to_string();
      state.update(cx, |s, cx| s.set_value(value, window, cx));
    }
    state
  }

  /// The team a new issue goes to: the one picked, else the team shown,
  /// else the first.
  fn create_team(&self) -> Option<LinearTeam> {
    let teams = self.teams.data.as_deref().unwrap_or_default();
    let picked = self
      .new_issue
      .as_ref()
      .and_then(|f| f.team_id.as_ref())
      .and_then(|id| teams.iter().find(|t| &t.id == id));
    picked
      .or_else(|| {
        self
          .team
          .as_ref()
          .and_then(|key| teams.iter().find(|t| &t.key == key))
      })
      .or(teams.first())
      .cloned()
  }

  pub(super) fn open_new_issue(&mut self, cx: &mut Context<Self>) {
    self.editing = None;
    self.new_issue = Some(NewIssue::default());
    for field in [
      "linear.new.title",
      "linear.new.description",
      "linear.new.team",
    ] {
      self.reset.insert(field);
    }
    cx.notify();
  }

  fn create_issue(&mut self, cx: &mut Context<Self>) {
    let Some(form) = self.new_issue.as_mut() else {
      return;
    };
    if form.pending || form.title.trim().is_empty() {
      return;
    }
    let team = self.create_team();
    let Some(form) = self.new_issue.as_mut() else {
      return;
    };
    let Some(team) = team else {
      form.error = Some("There is no Linear team to create the issue in.".into());
      cx.notify();
      return;
    };
    form.pending = true;
    form.error = None;
    let (title, description) = (form.title.clone(), form.description.clone());
    self.run(
      move |host, repo| host.linear_create_issue(repo, &team.id, &title, &description),
      |view, result, cx| match result {
        Ok(issue) => {
          view.new_issue = None;
          view.load_issues(cx);
          view.open_issue(&issue.id, cx);
        }
        Err(err) => {
          if let Some(form) = &mut view.new_issue {
            form.pending = false;
            form.error = Some(err);
          }
        }
      },
      cx,
    );
    cx.notify();
  }

  fn edit_issue(&mut self, issue: &LinearIssue, cx: &mut Context<Self>) {
    self.editing = Some((
      issue.title.clone(),
      issue.description.clone().unwrap_or_default(),
    ));
    self.reset.insert("linear.edit.title");
    self.reset.insert("linear.edit.description");
    cx.notify();
  }

  fn save_issue(&mut self, cx: &mut Context<Self>) {
    let (Some(id), Some((title, description))) = (self.open_issue.clone(), self.editing.clone())
    else {
      return;
    };
    if self.saving || title.trim().is_empty() {
      return;
    }
    self.saving = true;
    self.run(
      move |host, repo| host.linear_update_issue(repo, &id, &title, &description),
      |view, result, cx| {
        view.saving = false;
        match result {
          Ok(_) => {
            view.editing = None;
            view.load_issues(cx);
          }
          Err(err) => view.toast("Failed to update issue", err),
        }
      },
      cx,
    );
    cx.notify();
  }

  fn delete_issue(&mut self, cx: &mut Context<Self>) {
    let Some(id) = self.open_issue.clone() else {
      return;
    };
    if self.deleting {
      return;
    }
    self.deleting = true;
    self.run(
      move |host, repo| host.linear_delete_issue(repo, &id),
      |view, result, cx| {
        view.deleting = false;
        view.confirm_delete = false;
        match result {
          Ok(()) => {
            view.open_issue = None;
            view.editing = None;
            view.load_issues(cx);
          }
          Err(err) => view.toast("Failed to delete issue", err),
        }
      },
      cx,
    );
    cx.notify();
  }

  fn cancel_delete(&mut self, cx: &mut Context<Self>) {
    if !self.deleting {
      self.confirm_delete = false;
      cx.notify();
    }
  }

  /// The agent button: the agent prompt opens over the window with the
  /// issue attached, to edit before it starts.
  fn kickoff(&self, id: &str, cx: &mut App) {
    let Some(issue) = self
      .current_issues()
      .and_then(|issues| issues.iter().find(|i| i.id == id))
    else {
      return;
    };
    // The issue's real sub-issues, not the ones a filter shows.
    self.field(
      "linear.kickoff",
      json!({
        "id": issue.id,
        "identifier": issue.identifier,
        "url": issue.url,
        "title": issue.title,
        "includeSubissues": !issue.sub_issue_ids.is_empty(),
        "sub_issue_ids": issue.sub_issue_ids,
      }),
      cx,
    );
  }

  fn open_document(&mut self, document: LinearDocument, cx: &mut Context<Self>) {
    let of = Commented::Document(document.id.clone());
    self.open_document = Some(document);
    self.dialog_focused = false;
    self.load_comments(of, cx);
    cx.notify();
  }

  pub(super) fn close_document(&mut self, cx: &mut Context<Self>) {
    self.open_document = None;
    cx.notify();
  }

  /// "Review with agent": writes the snapshot, then starts the review in
  /// the home repository, where the agent can still check the text against
  /// the code.
  fn start_review(&mut self, target: Target, cx: &mut Context<Self>) {
    if !self.starting.insert(target.of.clone()) {
      return;
    }
    let comments = self
      .comments
      .get(&target.of)
      .and_then(|f| f.data.clone())
      .unwrap_or_default();
    let of = target.of.clone();
    self.run(
      move |host, repo| {
        let snapshot = data::review_snapshot(&target.title, &target.url, &target.body, &comments);
        let dir = host.linear_prepare_review(repo, &target.of, &snapshot)?;
        let agent = host
          .repo_settings(repo, &["review_agent".to_string()])
          .ok()
          .and_then(|s| s.get("review_agent").cloned().flatten())
          .filter(|a| !a.is_empty());
        let prompt = data::review_prompt(&target.of, &target.title, comments.len(), &dir);
        host.launch_session(&review_launch(repo, prompt, agent), true)?;
        Ok::<_, String>(())
      },
      move |view, result, _cx| {
        view.starting.remove(&of);
        if let Err(err) = result {
          view.toast("Failed to start review", err);
        }
      },
      cx,
    );
    cx.notify();
  }

  /// Resolve, Delete or Apply to Linear, then a fresh read.
  fn review_action(&mut self, of: Commented, id: String, action: Action, cx: &mut Context<Self>) {
    if !self.busy.insert(id.clone()) {
      return;
    }
    let comment = id.clone();
    self.run(
      move |host, repo| match action {
        Action::Resolve => host.resolve_agent_comment(repo, &comment),
        Action::Delete => host.delete_agent_comment(repo, &comment),
        Action::Apply => host.linear_apply_suggestion(repo, &comment),
      },
      move |view, result, cx| {
        view.busy.remove(&id);
        match result {
          Ok(()) if action == Action::Apply => view.content_changed(&of, cx),
          Ok(()) => {}
          Err(err) => view.toast("Review comment action failed", err),
        }
        view.load_reviews(of, cx);
      },
      cx,
    );
    cx.notify();
  }

  /// A suggestion landed in Linear: read what shows it again.
  fn content_changed(&mut self, of: &Commented, cx: &mut Context<Self>) {
    match of {
      Commented::Issue(_) => self.load_issues(cx),
      Commented::Project(_) => self.load_projects(cx),
      Commented::Document(_) => {
        if let Some(project) = self.shown_project.clone() {
          self.load_documents(project, cx);
        }
      }
    }
  }

  // ---- What the view shows. ----

  pub(super) fn current_issues(&self) -> Option<&Vec<LinearIssue>> {
    self.issues.get(&self.team).and_then(|f| f.data.as_deref())
  }

  fn viewer_id(&self) -> Option<&str> {
    self.viewer.as_ref().map(|v| v.id.as_str())
  }

  /// The project the detail shows: the one picked, else the first listed.
  pub(super) fn selected_project(&self) -> Option<&LinearProject> {
    let projects = self.projects.data.as_deref().unwrap_or_default();
    let shown = data::filter_projects(
      &data::project_view(projects, self.project_view, self.viewer_id()),
      &self.project_filters,
    );
    shown
      .iter()
      .find(|p| Some(&p.id) == self.selected_project.as_ref())
      .or(shown.first())
      .copied()
  }

  /// Reads the shown project's documents and comments when it changes, as
  /// SWR does on a new key.
  fn sync_project(&mut self, cx: &mut Context<Self>) {
    if self.section != Section::Projects {
      return;
    }
    let shown = self.selected_project().map(|p| p.id.clone());
    if shown != self.shown_project {
      self.shown_project = shown.clone();
      if let Some(project) = shown {
        self.load_documents(project.clone(), cx);
        self.load_comments(Commented::Project(project), cx);
      }
    }
  }
}

/// Where a text's comments go, as in Linear: an issue's under it, a
/// document's or project's under the paragraph each quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommentPlacement {
  Below,
  Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
  Resolve,
  Delete,
  Apply,
}

/// An issue's identifier, state, priority, project and assignee.
fn issue_meta(identifier: impl IntoElement, issue: &LinearIssue, p: &Palette) -> Div {
  div()
    .flex()
    .items_center()
    .gap_2()
    .flex_wrap()
    .child(
      ui::text(div(), TextSize::Sm)
        .font_family(theme::MONO_FONT)
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.primary)
        .child(identifier),
    )
    .child(ui::Badge::new(issue.state.name.clone(), p))
    .when(
      !issue.priority_label.is_empty() && issue.priority != 0,
      |this| this.child(ui::Badge::new(issue.priority_label.clone(), p)),
    )
    .when_some(issue.project.as_ref(), |this, project| {
      this.child(ui::Badge::new(project.name.clone(), p))
    })
    .when_some(issue.assignee.as_ref(), |this, assignee| {
      this.child(
        ui::text(div(), TextSize::Xs)
          .text_color(p.muted_foreground)
          .child(assignee.name.clone()),
      )
    })
}

/// A list row's second line: the identifier, state, priority, project,
/// labels and assignee.
fn issue_meta_line(issue: &LinearIssue) -> String {
  let mut parts = vec![issue.identifier.clone(), issue.state.name.clone()];
  if !issue.priority_label.is_empty() && issue.priority != 0 {
    parts.push(issue.priority_label.clone());
  }
  parts.extend(issue.project.as_ref().map(|p| p.name.clone()));
  parts.extend(issue.labels.iter().cloned());
  parts.extend(issue.assignee.as_ref().map(|a| a.name.clone()));
  parts.join(" · ")
}

/// An issue row's text without its links or actions, for lists that pick
/// issues.
pub(super) fn issue_list_item(issue: &LinearIssue, p: &Palette) -> Div {
  div()
    .flex_1()
    .min_w_0()
    .child(issue_meta(issue.identifier.clone(), issue, p))
    .child(
      ui::text(div(), TextSize::Sm)
        .mt_1()
        .truncate()
        .child(issue.title.clone()),
    )
}

/// CSS `capitalize`: each word's first letter in upper case.
pub(super) fn capitalize(text: &str) -> String {
  text
    .split(' ')
    .map(|word| {
      let mut chars = word.chars();
      match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
      }
    })
    .collect::<Vec<_>>()
    .join(" ")
}

/// A tab strip of `items` (value, id suffix, label), each tab's id `{prefix}-{suffix}`.
fn tab_strip<V: Copy + PartialEq + 'static>(
  prefix: &'static str,
  items: Vec<(V, &'static str, &'static str)>,
  active: V,
  p: &Palette,
  on_pick: impl Fn(&V, &mut Window, &mut App) + 'static,
) -> ui::Tabs {
  let values: Vec<V> = items.iter().map(|(value, ..)| *value).collect();
  ui::Tabs::new(format!("{prefix}s"), p)
    .tabs(
      items
        .into_iter()
        .map(|(_, key, label)| ui::Tab::new(format!("{prefix}-{key}"), label)),
    )
    .selected(values.iter().position(|v| *v == active).unwrap_or(0))
    .on_select(move |ix, window, cx| on_pick(&values[ix], window, cx))
}

impl LinearView {
  /// Records its parent's bounds as `key`'s, for a dropdown below it.
  fn anchor(&self, key: &str) -> impl IntoElement {
    let (anchors, key) = (self.anchors.clone(), key.to_string());
    gpui::canvas(
      move |b, _, _| {
        anchors.borrow_mut().insert(key.clone(), b);
      },
      |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
  }

  /// An `<a target="_blank">`: opens `url`, never its parent's click.
  fn link(
    &self,
    id: String,
    text: impl Into<SharedString>,
    url: &str,
    color: Hsla,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    let (url, selector) = (url.to_string(), id.clone());
    div()
      .id(SharedString::from(id))
      .debug_selector(move || selector)
      .text_color(color)
      .cursor_pointer()
      .hover(|s| s.underline())
      .child(text.into())
      .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
        cx.stop_propagation();
        view.open_url(&url);
      }))
  }

  /// The `h-7 w-7` ghost button that reads again, spinning while loading.
  fn refresh_button(
    &self,
    id: &'static str,
    loading: bool,
    p: &Palette,
    cx: &mut Context<Self>,
    on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
  ) -> impl IntoElement {
    let icon = ui::icon(theme::ICON_REFRESH, 14., p.foreground);
    Button::new(id, p)
      .aria_label("Refresh")
      .variant(Variant::Ghost)
      .size(Size::Icon)
      .w(zpx(28.))
      .h(zpx(28.))
      .disabled(loading)
      .child(if loading {
        ui::spin(format!("{id}-spin"), icon).into_any_element()
      } else {
        icon.into_any_element()
      })
      .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| on_click(view, cx)))
  }

  /// `LinearFilterMenu`: the Filter button, and a chip per filter set.
  fn filter_menu(
    &self,
    test_id: &'static str,
    groups: Vec<FilterGroup>,
    projects: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let view = cx.entity().downgrade();
    let set = move |key: &'static str, value: Option<String>| {
      let view = view.clone();
      move |cx: &mut App| {
        let value = value.clone();
        view
          .update(cx, |view, cx| {
            if projects {
              view.set_project_filter(key, value, cx)
            } else {
              view.set_issue_filter(key, value, cx)
            }
          })
          .ignore_closed();
      }
    };
    let mut items = vec![
      NativeMenuItem::Label("Filter by".into()),
      NativeMenuItem::Separator,
    ];
    for group in &groups {
      let mut sub =
        vec![NativeMenuItem::item("Any", set(group.key, None)).radio(group.value.is_none())];
      sub.extend(group.options.iter().map(|o| {
        NativeMenuItem::item(o.label.clone(), set(group.key, Some(o.value.clone())))
          .radio(group.value.as_ref() == Some(&o.value))
      }));
      items.push(NativeMenuItem::submenu(
        group.label,
        group.value_label(),
        sub,
      ));
    }
    let items = RefCell::new(Some(items));
    let (anchors, dark) = (self.anchors.clone(), self.dark);
    let trigger = Button::new(test_id, p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .gap_1p5()
      .icon(theme::ICON_FILTER, 14.)
      .label("Filter")
      .child(ui::icon(theme::ICON_CHEVRON_DOWN, 14., p.muted_foreground))
      .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
        let Some(anchor) = anchors.borrow().get(test_id).copied() else {
          return;
        };
        let Some(items) = items.borrow_mut().take() else {
          return;
        };
        // `align="start"`: the menu's left edge under the button's.
        let at = gpui::point(anchor.left(), anchor.bottom() + px(4.));
        view.popup(
          PopupRequest::ShowMenu {
            entries: popup::native_entries(items),
            at,
            dark,
          },
          cx,
        );
        cx.notify();
      }));
    let chips = groups
      .iter()
      .filter(|g| g.value.is_some())
      .map(|group| {
        let key = group.key;
        let clear = format!("linear-filter-clear-{key}");
        let chip = format!("linear-filter-chip-{key}");
        ui::text(div(), TextSize::Xs)
          .debug_selector(move || chip)
          .flex()
          .items_center()
          .gap_1()
          .px_2()
          .py_1()
          .rounded_full()
          .bg(p.muted)
          .text_color(p.foreground)
          .child(
            div()
              .text_color(p.muted_foreground)
              .child(format!("{}:", group.label)),
          )
          .child(group.value_label().unwrap_or_default())
          .child({
            let selector = clear.clone();
            let destructive = p.destructive;
            div()
              .id(SharedString::from(clear))
              .debug_selector(move || selector)
              .cursor_pointer()
              .text_color(p.foreground)
              .hover(move |s| s.text_color(destructive))
              .child(ui::icon(theme::ICON_X, 12., p.foreground))
              .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if projects {
                  view.set_project_filter(key, None, cx)
                } else {
                  view.set_issue_filter(key, None, cx)
                }
              }))
          })
      })
      .collect::<Vec<_>>();
    div()
      .flex()
      .items_center()
      .gap_1p5()
      .flex_wrap()
      .child(div().relative().child(trigger).child(self.anchor(test_id)))
      .children(chips)
  }

  fn team_selector(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let teams = self.teams.data.clone().unwrap_or_default();
    let label = match &self.team {
      Some(key) => teams
        .iter()
        .find(|t| &t.key == key)
        .map_or_else(|| key.clone(), |t| t.name.clone()),
      None => "All Teams".to_string(),
    };
    let (anchors, dark, current) = (self.anchors.clone(), self.dark, self.team.clone());
    let view = cx.entity().downgrade();
    let pick = move |team: Option<String>| {
      let view = view.clone();
      move |cx: &mut App| {
        let team = team.clone();
        view
          .update(cx, |view, cx| view.set_team(team, cx))
          .ignore_closed();
      }
    };
    div()
      .relative()
      .child(
        Button::new("linear-team-selector", p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .gap_2()
          .label(label)
          .child(ui::icon(theme::ICON_CHEVRON_DOWN, 16., p.muted_foreground))
          .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
            let Some(anchor) = anchors.borrow().get("linear-team-selector").copied() else {
              return;
            };
            let mut items = vec![
              NativeMenuItem::Label("Teams".into()),
              NativeMenuItem::Separator,
              NativeMenuItem::item("All Teams", pick(None)).radio(current.is_none()),
            ];
            items.extend(teams.iter().map(|t| {
              NativeMenuItem::item(t.name.clone(), pick(Some(t.key.clone())))
                .radio(current.as_ref() == Some(&t.key))
            }));
            view.popup(
              PopupRequest::ShowDropdown {
                entries: popup::native_entries(items),
                anchor,
                dark,
              },
              cx,
            );
          })),
      )
      .child(self.anchor("linear-team-selector"))
  }
}

impl LinearView {
  /// A read's error that only Settings can fix: Linear is not connected, or
  /// refuses the key.
  fn connection_error(&self) -> Option<String> {
    let issues = self.issues.get(&self.team).and_then(|f| f.error.as_ref());
    [issues, self.projects.error.as_ref()]
      .into_iter()
      .flatten()
      .find(|error| crate::linear::needs_connection(error))
      .cloned()
  }

  /// The empty state in place of the tabs while Linear can't be read.
  fn connection_state(
    &self,
    error: String,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let title = if error == crate::linear::NOT_CONNECTED {
      "Connect Linear"
    } else {
      "Linear rejected the API key"
    };
    div()
      .id("linear-connection")
      .role(Role::Label)
      .aria_label(SharedString::from(error.clone()))
      .test_support()
      .flex_1()
      .min_h_0()
      .flex()
      .items_center()
      .justify_center()
      .child(
        ui::EmptyState::new("linear-connection-state", title, p)
          .icon(theme::ICON_LINEAR)
          .error(error != crate::linear::NOT_CONNECTED)
          .message(error)
          .action(
            Button::new("linear-open-settings", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Open Settings")
              .on_click(cx.listener(|view, _, _, cx| view.open_settings(cx))),
          ),
      )
  }
}

impl Render for LinearView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.sync_project(cx);
    let p = theme::palette(self.dark);
    let connection = self.connection_error();
    let header = div()
      .flex()
      .items_center()
      .justify_between()
      .px_4()
      .pt_4()
      .pb_2()
      .flex_none()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(theme::ICON_LINEAR, 20., p.muted_foreground))
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::SEMIBOLD)
              .child("Linear"),
          ),
      )
      .children(connection.is_none().then(|| self.team_selector(&p, cx)));
    // Without a working key no tab can show anything: the whole view says
    // what to fix.
    if let Some(error) = connection {
      return div()
        .debug_selector(|| "linear-panel".into())
        .size_full()
        .flex()
        .flex_col()
        .bg(p.background)
        .text_color(p.foreground)
        .child(header)
        .child(self.connection_state(error, &p, cx))
        .into_any_element();
    }
    let sections = div()
      .px_4()
      .pb_2()
      .flex_none()
      .child(div().flex().child(tab_strip(
        "linear-section",
        vec![
          (Section::Issues, "issues", "Issues"),
          (Section::Projects, "projects", "Projects"),
        ],
        self.section,
        &p,
        cx.listener(|view, section: &Section, _, cx| view.set_section(*section, cx)),
      )));
    let body = match self.section {
      Section::Issues => self.issues_section(&p, window, cx).into_any_element(),
      Section::Projects => self.projects_section(&p, cx).into_any_element(),
    };
    let dialog = self
      .delete_dialog(&p, window, cx)
      .or_else(|| self.document_dialog(&p, window, cx));
    div()
      .debug_selector(|| "linear-panel".into())
      .size_full()
      .flex()
      .flex_col()
      .bg(p.background)
      .text_color(p.foreground)
      .child(header)
      .child(sections)
      .child(body)
      .children(dialog)
      .into_any_element()
  }
}

impl LinearView {
  fn issues_section(&mut self, p: &Palette, window: &mut Window, cx: &mut Context<Self>) -> Div {
    let fetch = self.issues.get(&self.team).cloned().unwrap_or_default();
    let loading = fetch.is_loading();
    let all = fetch.data.clone().unwrap_or_default();
    let viewed = data::issue_view(&all, self.issue_view, self.viewer_id());
    let options = data::issue_filter_options(&viewed);
    let shown = data::filter_issues(&viewed, &self.issue_filters);
    let open = self
      .open_issue
      .as_ref()
      .and_then(|id| all.iter().find(|i| &i.id == id));
    if self.new_issue.is_some() {
      return self.new_issue_page(p, window, cx);
    }
    if let Some(issue) = open.cloned() {
      return self.issue_page(&issue, &all, p, window, cx);
    }
    let f = &self.issue_filters;
    let groups = vec![
      FilterGroup {
        key: "assignee",
        label: "Assignee",
        value: f.assignee.clone(),
        options: options.assignees,
      },
      FilterGroup {
        key: "priority",
        label: "Priority",
        value: f.priority.map(|p| p.to_string()),
        options: options.priorities,
      },
      FilterGroup {
        key: "label",
        label: "Label",
        value: f.label.clone(),
        options: options.labels,
      },
      FilterGroup {
        key: "project",
        label: "Project",
        value: f.project.clone(),
        options: options.projects,
      },
    ];

    let views = div().px_4().pb_2().flex_none().child(
      div().flex().child(tab_strip(
        "linear-view",
        IssueView::ALL
          .into_iter()
          .map(|v| (v, v.value(), v.label()))
          .collect(),
        self.issue_view,
        p,
        cx.listener(|view, v: &IssueView, _, cx| {
          view.issue_view = *v;
          cx.notify();
        }),
      )),
    );
    let toolbar = div()
      .flex()
      .items_center()
      .gap_2()
      .px_4()
      .pb_2()
      .flex_none()
      .flex_wrap()
      .child(tab_strip(
        "linear-mode",
        vec![
          (IssueMode::List, "list", "List"),
          (IssueMode::Kanban, "kanban", "Kanban"),
        ],
        self.mode,
        p,
        cx.listener(|view, mode: &IssueMode, _, cx| {
          view.mode = *mode;
          cx.notify();
        }),
      ))
      .child(self.filter_menu("linear-issues-filter-trigger", groups, false, p, cx))
      .child(div().flex_1())
      .child(
        Button::new("linear-new-issue", p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .gap_1()
          .icon(theme::ICON_PLUS, 14.)
          .label("New issue")
          .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.open_new_issue(cx))),
      )
      .child(
        self.refresh_button("linear-issues-refresh", loading, p, cx, |view, cx| {
          view.load_issues(cx)
        }),
      );

    let message = |id: &'static str, text: SharedString, color: Hsla| {
      ui::text(div(), TextSize::Sm)
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Label)
        .aria_label(text.clone())
        .test_support()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .text_center()
        .p_8()
        .gap_3()
        .text_color(color)
        .child(text)
    };
    // The list and each Kanban column are virtual lists that scroll
    // themselves.
    let mut content = div()
      .id("linear-issues-scroll")
      .flex_1()
      .min_h_0()
      .overflow_hidden();
    content = content
      .when(loading, |this| {
        this.child(
          div()
            .py_12()
            .child(ui::Loading::new("linear-issues-loading", p)),
        )
      })
      .when_some(fetch.error.clone(), |this, error| {
        this.child(
          message("linear-issues-error", error.into(), p.destructive).child(
            Button::new("linear-issues-retry", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Try again")
              .on_click(cx.listener(|view, _, _, cx| view.load_issues(cx))),
          ),
        )
      });
    if !loading && !fetch.failed() {
      content = if shown.is_empty() {
        content.child(message(
          "linear-no-issues",
          "No issues found".into(),
          p.muted_foreground,
        ))
      } else {
        match self.mode {
          IssueMode::List => content.child(self.issue_list(&shown, &all, window, cx)),
          IssueMode::Kanban => content.child(self.kanban(&shown, &all, p, window, cx)),
        }
      };
    }
    div()
      .flex_1()
      .min_h_0()
      .flex()
      .flex_col()
      .child(views)
      .child(toolbar)
      .child(content)
  }

  /// `LinearIssuesList`: roots, each followed by its shown sub-issues, a
  /// row each, built only while on screen.
  fn issue_list(
    &mut self,
    shown: &[&LinearIssue],
    all: &[LinearIssue],
    window: &Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let index = issue_index(all);
    let (roots, subs) = data::split_roots(shown);
    let mut rows = Vec::new();
    for root in roots {
      rows.push(IssueRow::new(&index, root, false, None));
      rows.extend(
        subs
          .get(&root.id)
          .into_iter()
          .flatten()
          .map(|sub| IssueRow::new(&index, sub, true, None)),
      );
    }
    self.issue_rows.sync(window, rows, |a, b| a == b);
    gpui::list(
      self.issue_rows.list.clone(),
      cx.processor(|view, ix, _, cx| view.issue_list_row(ix, cx)),
    )
    .size_full()
  }

  /// The team's issue a row stands for.
  fn row_issue(&self, row: &IssueRow) -> Option<&LinearIssue> {
    self
      .issues
      .get(&self.team)?
      .data
      .as_ref()?
      .get(row.ix)
      .filter(|issue| issue.id == row.id)
  }

  fn issue_list_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
    let p = theme::palette(self.dark);
    let Some(row) = self.issue_rows.get(ix) else {
      return div().into_any_element();
    };
    let Some(issue) = self.row_issue(row) else {
      return div().into_any_element();
    };
    div()
      .px_2()
      .child(self.issue_row(issue, row.indent, &p, cx))
      .into_any_element()
  }

  /// Linear's list row: the title, then the identifier, state, priority,
  /// project, labels and assignee; the agent button on the right.
  fn issue_row(
    &self,
    issue: &LinearIssue,
    indent: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = issue.id.clone();
    ui::Row::new(SharedString::from(format!("linear-issue-{id}")), p)
      .density(ui::RowDensity::TwoLine)
      .title(issue.title.clone())
      .meta(issue_meta_line(issue))
      .actions(
        self.agent_button(format!("linear-kickoff-{id}"), &id, p, cx),
        24.,
      )
      .cursor_pointer()
      .when(indent, |this| this.ml_6())
      .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.open_issue(&id, cx)))
  }

  /// The robot button that opens the agent prompt for the issue `id`,
  /// without opening its page.
  fn agent_button(
    &self,
    button: String,
    id: &str,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> ui::IconButton {
    let id = id.to_string();
    ui::IconButton::new(button, theme::ICON_BOT, "Start agent", p)
      .size(ui::IconButtonSize::Small)
      .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
        cx.stop_propagation();
        view.kickoff(&id, cx);
      }))
  }

  /// The issue's page: its title and details, the description, then the
  /// comments under it, as in Linear.
  fn issue_page(
    &mut self,
    issue: &LinearIssue,
    all: &[LinearIssue],
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Div {
    let editing = self.editing.is_some();
    let parent = issue
      .parent_id
      .as_deref()
      .and_then(|id| all.iter().find(|i| i.id == id));
    let bar = div()
      .flex()
      .flex_none()
      .items_center()
      .gap_2()
      .px_4()
      .py_2()
      .border_b_1()
      .border_color(p.border)
      .child(
        ui::IconButton::new(
          "linear-issue-back",
          theme::ICON_ARROW_LEFT,
          "Back to issues",
          p,
        )
        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.close_issue(cx))),
      )
      .children(parent.map(|parent| {
        ui::text(div(), TextSize::Sm)
          .font_family(theme::MONO_FONT)
          .text_color(p.muted_foreground)
          .child(format!("{} ›", parent.identifier))
      }))
      .child(
        ui::text(div(), TextSize::Sm)
          .font_family(theme::MONO_FONT)
          .font_weight(FontWeight::MEDIUM)
          .child(issue.identifier.clone()),
      )
      .child(div().flex_1())
      .when(!editing, |this| {
        this
          .child(
            ui::IconButton::new("linear-issue-edit", theme::ICON_PENCIL, "Edit issue", p).on_click(
              cx.listener({
                let issue = issue.clone();
                move |view, _: &ClickEvent, _, cx| view.edit_issue(&issue, cx)
              }),
            ),
          )
          .child(
            ui::IconButton::new("linear-issue-delete", theme::ICON_TRASH, "Delete issue", p)
              .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.confirm_delete = true;
                view.delete_focused = false;
                cx.notify();
              })),
          )
      })
      .child(self.agent_button("linear-issue-page-kickoff".into(), &issue.id, p, cx))
      .child({
        let url = issue.url.clone();
        Button::new("linear-issue-open", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_1p5()
          .icon(theme::ICON_EXTERNAL_LINK, 14.)
          .label("Open in Linear")
          .on_click(cx.listener(move |view, _: &ClickEvent, _, _| view.open_url(&url)))
      });
    let labels = (!issue.labels.is_empty()).then(|| {
      div().flex().gap_1().flex_wrap().children(
        issue
          .labels
          .iter()
          .map(|label| ui::Badge::new(label.clone(), p)),
      )
    });
    let target = Target {
      of: Commented::Issue(issue.id.clone()),
      title: format!("{} {}", issue.identifier, issue.title),
      url: issue.url.clone(),
      body: issue.description.clone().unwrap_or_default(),
      shown: issue
        .description
        .clone()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "_No description._".into()),
    };
    let selector = format!("linear-issue-page-{}", issue.id);
    div()
      .debug_selector(move || selector)
      .flex_1()
      .min_h_0()
      .flex()
      .flex_col()
      .child(bar)
      .child(
        div()
          .id("linear-issue-page-scroll")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .child(
            div()
              .max_w(zpx(820.))
              .px_6()
              .py_6()
              .flex()
              .flex_col()
              .gap_3()
              .map(|this| {
                if editing {
                  this
                    .child(issue_meta(issue.identifier.clone(), issue, p))
                    .child(self.edit_form(p, window, cx))
                } else {
                  this
                    .child(
                      ui::text(div(), TextSize::Lg)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(issue.title.clone()),
                    )
                    .child(issue_meta(issue.identifier.clone(), issue, p))
                    .children(labels)
                    .child(div().pt_3().child(self.commented(
                      target,
                      CommentPlacement::Below,
                      p,
                      cx,
                    )))
                }
              }),
          ),
      )
  }

  /// The New issue page: a back button over the form.
  fn new_issue_page(&mut self, p: &Palette, window: &mut Window, cx: &mut Context<Self>) -> Div {
    let bar = div()
      .flex()
      .flex_none()
      .items_center()
      .gap_2()
      .px_4()
      .py_2()
      .border_b_1()
      .border_color(p.border)
      .child(
        ui::IconButton::new(
          "linear-new-issue-back",
          theme::ICON_ARROW_LEFT,
          "Back to issues",
          p,
        )
        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
          view.new_issue = None;
          cx.notify();
        })),
      );
    div().flex_1().min_h_0().flex().flex_col().child(bar).child(
      div()
        .id("linear-new-issue-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
          div()
            .max_w(zpx(820.))
            .px_2()
            .child(self.new_issue_form(p, window, cx)),
        ),
    )
  }

  /// The open issue's title and description, editable.
  fn edit_form(&mut self, p: &Palette, window: &mut Window, cx: &mut Context<Self>) -> Div {
    let (title, description) = self.editing.clone().unwrap_or_default();
    let title_state = self.input("linear.edit.title", &title, "Title", window, cx);
    let body_state = self.textarea(
      "linear.edit.description",
      &description,
      "Description (optional, Markdown)",
      window,
      cx,
    );
    let saving = self.saving;
    div()
      .debug_selector(|| "linear-issue-edit-form".into())
      .flex()
      .flex_col()
      .gap_2()
      .child(ui::text(
        div().child(form::input_field(
          "linear-issue-edit-title",
          &title_state,
          false,
          saving,
        )),
        TextSize::Sm,
      ))
      .child(form::textarea_field(&body_state, false, saving))
      .child(
        div()
          .flex()
          .gap_2()
          .child(
            busy_spinner(
              Button::new("linear-issue-edit-save", p)
                .size(Size::Sm)
                .gap_1()
                .disabled(title.trim().is_empty() || saving),
              "linear-issue-edit-save",
              saving,
              p.primary_foreground,
            )
            .label("Save")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save_issue(cx))),
          )
          .child(
            Button::new("linear-issue-edit-cancel", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .disabled(saving)
              .label("Cancel")
              .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.editing = None;
                cx.notify();
              })),
          ),
      )
  }

  /// "New issue": a team, a title and a description.
  fn new_issue_form(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let form = self.new_issue.clone().unwrap_or_default();
    let team = self.create_team();
    let options: Vec<SelectOption> = self
      .teams
      .data
      .iter()
      .flatten()
      .map(|t| SelectOption {
        value: t.id.clone(),
        label: format!("{} ({})", t.name, t.key),
      })
      .collect();
    let team_state = self.controls.select(
      "linear.new.team",
      &options,
      team.as_ref().map(|t| t.id.as_str()).unwrap_or_default(),
      window,
      cx,
    );
    let title_state = self.input("linear.new.title", &form.title, "Title", window, cx);
    let body_state = self.textarea(
      "linear.new.description",
      &form.description,
      "Description (optional, Markdown)",
      window,
      cx,
    );
    let pending = form.pending;
    div()
      .debug_selector(|| "linear-new-issue-form".into())
      .p_4()
      .flex()
      .flex_col()
      .gap_3()
      .child(
        ui::text(div(), TextSize::Lg)
          .font_weight(FontWeight::SEMIBOLD)
          .child("New issue"),
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_1()
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child("Team"),
          )
          .child(ui::text(
            div().child(form::select_field(
              "linear-new-issue-team",
              "Team",
              &team_state,
              pending,
            )),
            TextSize::Sm,
          )),
      )
      .child(ui::text(
        div().child(form::input_field(
          "linear-new-issue-title",
          &title_state,
          false,
          pending,
        )),
        TextSize::Sm,
      ))
      .child(form::textarea_field(&body_state, false, pending))
      .child(
        div()
          .flex()
          .gap_2()
          .child(
            busy_spinner(
              Button::new("linear-new-issue-submit", p)
                .size(Size::Sm)
                .gap_1()
                .disabled(form.title.trim().is_empty() || pending),
              "linear-new-issue-submit",
              pending,
              p.primary_foreground,
            )
            .label("Create issue")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.create_issue(cx))),
          )
          .child(
            Button::new("linear-new-issue-cancel", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .disabled(pending)
              .label("Cancel")
              .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.new_issue = None;
                cx.notify();
              })),
          ),
      )
      .children(form.error.map(|err| {
        ui::text(div(), TextSize::Sm)
          .text_color(p.destructive)
          .whitespace_normal()
          .child(err)
      }))
  }

  /// The delete confirmation for the open issue.
  fn delete_dialog(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    if !self.confirm_delete {
      return None;
    }
    let identifier = self
      .open_issue
      .as_ref()
      .and_then(|id| self.current_issues()?.iter().find(|i| &i.id == id))
      .map_or_else(|| "this issue".to_string(), |i| i.identifier.clone());
    if !self.delete_focused {
      self.delete_focused = true;
      window.focus(&self.focus, cx);
    }
    let pending = self.deleting;
    let viewport = window.viewport_size();
    let (backdrop, panel) = dialog(
      "linear-delete-dialog",
      crate::gpui_shell::ui::zoomed(512., window),
      None,
      viewport,
      p,
    );
    let keys = cx.listener(|view, e: &gpui::KeyDownEvent, _, cx| {
      if ui::is_escape(&e.keystroke) {
        cx.stop_propagation();
        view.cancel_delete(cx);
      }
    });
    Some(
      overlay(
        backdrop.on_mouse_down(
          gpui::MouseButton::Left,
          cx.listener(|view, _, _, cx| view.cancel_delete(cx)),
        ),
        panel
          .id("linear-delete-panel")
          .track_focus(&self.focus)
          .capture_key_down(keys)
          .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .child(
            div()
              .flex()
              .flex_col()
              .gap_2()
              .child(title(format!("Delete {identifier}?")))
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .whitespace_normal()
                  .child(
                    "Linear moves the issue to its trash, where it can be restored for 30 days.",
                  ),
              ),
          )
          .child(
            div()
              .flex()
              .justify_end()
              .gap_2()
              .child(
                Button::new("linear-delete-cancel", p)
                  .variant(Variant::Ghost)
                  .disabled(pending)
                  .label("Cancel")
                  .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.cancel_delete(cx))),
              )
              .child(
                busy_spinner(
                  Button::new("linear-delete-confirm", p)
                    .bg(p.destructive)
                    .hover_bg(p.destructive.opacity(0.9))
                    .text_color(p.destructive_foreground)
                    .gap_1()
                    .disabled(pending),
                  "linear-delete-confirm",
                  pending,
                  p.destructive_foreground,
                )
                .label("Delete")
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.delete_issue(cx))),
              ),
          ),
      )
      .into_any_element(),
    )
  }

  /// `LinearKanbanView`: a column per state, every shown issue in its own;
  /// each column builds only the cards on screen.
  fn kanban(
    &mut self,
    shown: &[&LinearIssue],
    all: &[LinearIssue],
    p: &Palette,
    window: &Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let index = issue_index(all);
    let identifiers: HashMap<&str, &str> = all
      .iter()
      .map(|i| (i.id.as_str(), i.identifier.as_str()))
      .collect();
    let columns = data::group_by_state(shown);
    let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
    for column in &columns {
      let rows = column
        .issues
        .iter()
        .map(|issue| {
          let parent = issue
            .parent_id
            .as_deref()
            .and_then(|id| identifiers.get(id))
            .map(|id| id.to_string());
          IssueRow::new(&index, issue, false, parent)
        })
        .collect();
      self
        .column_rows
        .entry(column.name.clone())
        .or_default()
        .sync(window, rows, |a, b| a == b);
    }
    self.column_rows.retain(|name, _| names.contains(name));
    div()
      .id("linear-kanban")
      .size_full()
      .flex()
      .gap_3()
      .overflow_x_scroll()
      .p_4()
      .children(names.into_iter().map(|name| {
        let selector = format!("linear-kanban-column-{name}");
        let list = self.column_rows[&name].list.clone();
        let column = name.clone();
        div()
          .debug_selector(move || selector)
          .flex_none()
          .w(zpx(320.))
          .bg(p.muted.opacity(0.3))
          .rounded(radius::LG)
          .border_1()
          .border_color(p.border)
          .p_3()
          .flex()
          .flex_col()
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::MEDIUM)
              .mb_3()
              .text_color(p.muted_foreground)
              .child(name.clone()),
          )
          .child(
            div()
              .id(SharedString::from(format!("linear-kanban-list-{name}")))
              .flex_1()
              .min_h_0()
              .child(
                gpui::list(
                  list,
                  cx.processor(move |view, ix, _, cx| view.kanban_row(&column, ix, cx)),
                )
                .size_full(),
              ),
          )
      }))
  }

  fn kanban_row(&self, column: &str, ix: usize, cx: &mut Context<Self>) -> AnyElement {
    let p = theme::palette(self.dark);
    let Some(row) = self.column_rows.get(column).and_then(|rows| rows.get(ix)) else {
      return div().into_any_element();
    };
    let Some(issue) = self.row_issue(row) else {
      return div().into_any_element();
    };
    // `gap-2` between cards, as padding: a list's rows touch.
    div()
      .when(ix > 0, |this| this.pt_2())
      .child(self.kanban_card(issue, row.parent.as_deref(), &p, cx))
      .into_any_element()
  }

  fn kanban_card(
    &self,
    issue: &LinearIssue,
    parent: Option<&str>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = issue.id.clone();
    let labels = (!issue.labels.is_empty()).then(|| {
      div()
        .flex()
        .gap_1()
        .mt_1p5()
        .flex_wrap()
        .children(issue.labels.iter().take(2).map(|label| {
          ui::text(div(), TextSize::Xs)
            .px_1()
            .py_0p5()
            .rounded(radius::SM)
            .bg(p.muted)
            .text_color(p.muted_foreground)
            .child(label.clone())
        }))
        .when(issue.labels.len() > 2, |this| {
          this.child(
            ui::text(div(), TextSize::Xs)
              .px_1()
              .py_0p5()
              .text_color(p.muted_foreground)
              .child(format!("+{}", issue.labels.len() - 2)),
          )
        })
    });
    let hover = p.hover();
    let selector = format!("linear-kanban-card-{id}");
    ui::text(div(), TextSize::Sm)
      .id(SharedString::from(selector.clone()))
      .debug_selector(move || selector)
      .role(Role::ListItem)
      .aria_label(SharedString::from(issue.title.clone()))
      .test_support()
      .bg(p.background)
      .border_1()
      .border_color(p.border)
      .rounded(radius::MD)
      .p_2p5()
      .cursor_pointer()
      .hover(move |s| s.bg(hover))
      .on_click({
        let id = id.clone();
        cx.listener(move |view, _: &ClickEvent, _, cx| view.open_issue(&id, cx))
      })
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .gap_2()
          .child(
            ui::text(div(), TextSize::Xs)
              .font_family(theme::MONO_FONT)
              .font_weight(FontWeight::MEDIUM)
              .text_color(p.muted_foreground)
              .child(issue.identifier.clone()),
          )
          .child(self.agent_button(format!("linear-kanban-kickoff-{id}"), &id, p, cx)),
      )
      .child(
        ui::text(div(), TextSize::Xs)
          .font_weight(FontWeight::MEDIUM)
          .mt_1()
          .line_clamp(2)
          .text_ellipsis()
          .child(issue.title.clone()),
      )
      .when_some(parent, |this, parent| {
        this.child(
          ui::text(div(), TextSize::Xs)
            .mt_1()
            .text_color(p.muted_foreground)
            .child(format!("Sub-issue of {parent}")),
        )
      })
      .children(labels)
  }
}

impl LinearView {
  fn projects_section(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let loading = self.projects.is_loading();
    let all = self.projects.data.clone().unwrap_or_default();
    let viewed = data::project_view(&all, self.project_view, self.viewer_id());
    let (states, leads) = data::project_filter_options(&viewed);
    let shown = data::filter_projects(&viewed, &self.project_filters);
    let selected = self.selected_project().cloned();
    let groups = vec![
      FilterGroup {
        key: "status",
        label: "Status",
        value: self.project_filters.state.clone(),
        options: states,
      },
      FilterGroup {
        key: "lead",
        label: "Lead",
        value: self.project_filters.lead.clone(),
        options: leads,
      },
    ];
    let header = div()
      .flex()
      .items_center()
      .justify_between()
      .px_4()
      .py_2()
      .flex_none()
      .child(
        ui::text(div(), TextSize::Sm)
          .font_weight(FontWeight::MEDIUM)
          .text_color(p.muted_foreground)
          .child("Projects"),
      )
      .child(
        self.refresh_button("linear-projects-refresh", loading, p, cx, |view, cx| {
          view.load_projects(cx)
        }),
      );
    let views = div().px_4().pb_2().flex_none().child(
      div().flex().child(tab_strip(
        "linear-project-view",
        ProjectView::ALL
          .into_iter()
          .map(|v| (v, v.value(), v.label()))
          .collect(),
        self.project_view,
        p,
        cx.listener(|view, v: &ProjectView, _, cx| {
          view.project_view = *v;
          cx.notify();
        }),
      )),
    );
    let filter = div().px_4().pb_2().flex_none().child(self.filter_menu(
      "linear-projects-filter-trigger",
      groups,
      true,
      p,
      cx,
    ));
    let hover = p.muted.opacity(0.5);
    let list = div()
      .id("linear-projects-scroll")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .when(loading, |this| {
        this.child(
          div()
            .py_12()
            .child(ui::Loading::new("linear-projects-loading", p)),
        )
      })
      .when_some(self.projects.error.clone(), |this, error| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .id("linear-projects-error")
            .debug_selector(|| "linear-projects-error".into())
            .role(Role::Label)
            .aria_label(error.clone())
            .test_support()
            .p_4()
            .flex()
            .flex_col()
            .items_start()
            .gap_3()
            .text_color(p.destructive)
            .child(error)
            .child(
              Button::new("linear-projects-retry", p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .label("Try again")
                .on_click(cx.listener(|view, _, _, cx| view.load_projects(cx))),
            ),
        )
      })
      .when(
        !loading && !self.projects.failed() && shown.is_empty(),
        |this| {
          this.child(
            ui::text(div(), TextSize::Sm)
              .debug_selector(|| "linear-no-projects".into())
              .p_4()
              .text_color(p.muted_foreground)
              .child("No projects found"),
          )
        },
      )
      .child(
        div()
          .flex()
          .flex_col()
          .children(shown.iter().enumerate().map(|(i, project)| {
            let on = selected.as_ref().is_some_and(|s| s.id == project.id);
            let id = project.id.clone();
            let selector = format!("linear-project-item-{id}");
            div()
              .id(SharedString::from(selector.clone()))
              .debug_selector(move || selector)
              .w_full()
              .px_4()
              .py_3()
              .cursor_pointer()
              .when(i > 0, |this| this.border_t_1().border_color(p.border))
              .hover(move |s| s.bg(hover))
              .when(on, |this| this.bg(p.muted))
              .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.selected_project = Some(id.clone());
                cx.notify();
              }))
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_weight(FontWeight::MEDIUM)
                  .truncate()
                  .child(project.name.clone()),
              )
              .child(
                div()
                  .flex()
                  .items_center()
                  .gap_2()
                  .mt_1()
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .px_1p5()
                      .py_0p5()
                      .rounded_full()
                      .bg(p.muted)
                      .text_color(p.muted_foreground)
                      .child(capitalize(&project.state)),
                  )
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .text_color(p.muted_foreground)
                      .child(format!("{}%", data::percent(project.progress))),
                  ),
              )
          })),
      );
    let left = div()
      .w(zpx(288.))
      .flex_none()
      .border_r_1()
      .border_color(p.border)
      .flex()
      .flex_col()
      .min_h_0()
      .child(header)
      .child(views)
      .child(filter)
      .child(list);
    let right = div()
      .id("linear-project-detail-scroll")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .child(match selected {
        Some(project) => self.project_detail(&project, p, cx).into_any_element(),
        None => ui::text(div(), TextSize::Sm)
          .size_full()
          .flex()
          .items_center()
          .justify_center()
          .text_color(p.muted_foreground)
          .child("Select a project")
          .into_any_element(),
      });
    div()
      .debug_selector(|| "linear-projects-section".into())
      .flex_1()
      .flex()
      .min_h_0()
      .child(left)
      .child(right)
  }

  fn project_detail(
    &self,
    project: &LinearProject,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let docs = self.documents.get(&project.id).cloned().unwrap_or_default();
    let mut meta = ui::text(div(), TextSize::Sm)
      .flex()
      .items_center()
      .gap_2()
      .mt_2()
      .text_color(p.muted_foreground)
      .flex_wrap()
      .child(
        div()
          .px_1p5()
          .py_0p5()
          .rounded_full()
          .bg(p.muted)
          .child(capitalize(&project.state)),
      )
      .child(format!("{}% complete", data::percent(project.progress)));
    if let Some(lead) = &project.lead {
      meta = meta.child(format!("Lead: {}", lead.name));
    }
    if let Some(target) = &project.target_date {
      meta = meta.child(format!("Target: {target}"));
    }
    let documents = div()
      .mt_6()
      .child(
        ui::text(div(), TextSize::Sm)
          .font_weight(FontWeight::MEDIUM)
          .text_color(p.muted_foreground)
          .mb_2()
          .child("Documents"),
      )
      .when(docs.is_loading(), |this| {
        this.child(ui::spinner(
          "linear-documents-loading",
          16.,
          p.muted_foreground,
        ))
      })
      .when(
        !docs.is_loading() && docs.data.as_ref().is_none_or(Vec::is_empty),
        |this| {
          this.child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child("No documents"),
          )
        },
      )
      .child(
        div()
          .flex()
          .flex_wrap()
          .gap_2()
          .children(docs.data.iter().flatten().map(|doc| {
            let selector = format!("linear-document-item-{}", doc.id);
            let hover = p.muted.opacity(0.7);
            let doc = doc.clone();
            ui::text(div(), TextSize::Sm)
              .id(SharedString::from(selector.clone()))
              .debug_selector(move || selector)
              .px_3()
              .py_1()
              .rounded_full()
              .bg(p.muted)
              .hover(move |s| s.bg(hover))
              .text_color(p.foreground)
              .cursor_pointer()
              .child(doc.title.clone())
              .on_click(
                cx.listener(move |view, _: &ClickEvent, _, cx| view.open_document(doc.clone(), cx)),
              )
          })),
      );
    let target = Target {
      of: Commented::Project(project.id.clone()),
      title: project.name.clone(),
      url: project.url.clone(),
      body: project.description.clone().unwrap_or_default(),
      shown: project
        .description
        .clone()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "_No description._".into()),
    };
    div()
      .debug_selector(|| "linear-project-detail".into())
      .p_4()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .flex_wrap()
          .child(
            ui::text(div(), TextSize::Lg)
              .font_weight(FontWeight::SEMIBOLD)
              .child(project.name.clone()),
          )
          .child(ui::text(
            self.link(
              "linear-project-open".into(),
              "Open in Linear",
              &project.url,
              p.primary,
              cx,
            ),
            TextSize::Xs,
          )),
      )
      .child(meta)
      .child(documents)
      .child(
        div()
          .mt_6()
          .pt_4()
          .border_t_1()
          .border_color(p.border)
          .child(self.commented(target, CommentPlacement::Inline, p, cx)),
      )
  }

  /// `DocumentDialog`: the document and its comments, over the window.
  fn document_dialog(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let id = self.open_document.as_ref()?.id.clone();
    // The latest copy, so an applied suggestion shows at once.
    let doc = self
      .shown_project
      .as_ref()
      .and_then(|project| self.documents.get(project))
      .and_then(|f| f.data.as_ref())
      .and_then(|docs| docs.iter().find(|d| d.id == id))
      .or(self.open_document.as_ref())
      .cloned()?;
    if !self.dialog_focused {
      self.dialog_focused = true;
      window.focus(&self.focus, cx);
    }
    let viewport = window.viewport_size();
    let (backdrop, panel) = dialog(
      "linear-document-dialog",
      crate::gpui_shell::ui::zoomed(896., window),
      None,
      viewport,
      p,
    );
    let target = Target {
      of: Commented::Document(doc.id.clone()),
      title: doc.title.clone(),
      url: doc.url.clone(),
      body: doc.content.clone().unwrap_or_default(),
      shown: doc
        .content
        .clone()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "No content".into()),
    };
    let keys = cx.listener(|view, e: &gpui::KeyDownEvent, _, cx| {
      if ui::is_escape(&e.keystroke) {
        cx.stop_propagation();
        view.close_document(cx);
      }
    });
    Some(
      overlay(
        backdrop.on_mouse_down(
          gpui::MouseButton::Left,
          cx.listener(|view, _, _, cx| view.close_document(cx)),
        ),
        panel
          .id("linear-document-panel")
          .max_h(viewport.height * 0.8)
          .overflow_y_scroll()
          .gap_0()
          .track_focus(&self.focus)
          .capture_key_down(keys)
          .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .child(div().flex().flex_col().child(title(doc.title)))
          .child(self.commented(target, CommentPlacement::Inline, p, cx)),
      )
      .into_any_element(),
    )
  }
}

impl LinearView {
  /// `LinearCommentedContent`: the text, highlighted where comments
  /// anchor, with its comments and the agent's findings: under it, or (as
  /// Linear shows a document's) each under the paragraph it quotes.
  fn commented(
    &self,
    target: Target,
    placement: CommentPlacement,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let inline = placement == CommentPlacement::Inline;
    let of = target.of.clone();
    let fetch = self.comments.get(&of).cloned().unwrap_or_default();
    let comments = fetch.data.clone().unwrap_or_default();
    let open: Vec<AgentReviewComment> = self
      .reviews
      .get(&of)
      .into_iter()
      .flatten()
      .filter(|c| c.status != "resolved")
      .cloned()
      .collect();
    let body_comments: Vec<&AgentReviewComment> =
      open.iter().filter(|c| c.file_path == BODY_FILE).collect();
    let mut anchors: Vec<Anchor> = comments
      .iter()
      .map(|c| Anchor {
        quote: c.quoted_text.as_deref(),
        color: data::comment_mark(self.dark),
      })
      .collect();
    // Agent quotes are multi-line markdown; only single-line ones can be
    // highlighted without breaking the rendered markdown structure.
    anchors.extend(
      body_comments
        .iter()
        .filter(|c| c.quoted_text.as_deref().is_some_and(|q| !q.contains('\n')))
        .map(|c| Anchor {
          quote: c.quoted_text.as_deref(),
          color: data::agent_mark(self.dark),
        }),
    );
    let shown = target.shown.clone();
    let key = format!("{}-{}", of.target_type(), of.id());
    let starting = self.starting.contains(&of);
    let review_button = {
      let fg = p.foreground;
      ui::text(
        Button::new(SharedString::from(format!("linear-start-review-{key}")), p),
        TextSize::Xs,
      )
      .variant(Variant::Outline)
      .size(Size::Sm)
      .h(zpx(28.))
      .gap_1p5()
      .disabled(starting)
      .child(if starting {
        ui::spinner(format!("linear-review-spin-{key}"), 12., fg).into_any_element()
      } else {
        ui::icon(theme::ICON_BOT, 12., fg).into_any_element()
      })
      .label("Review with agent")
      .on_click(
        cx.listener(move |view, _: &ClickEvent, _, cx| view.start_review(target.clone(), cx)),
      )
    };
    let toolbar = div()
      .flex()
      .items_center()
      .justify_end()
      .gap_2()
      .when(!open.is_empty(), |this| {
        this.child(
          ui::text(div(), TextSize::Xs)
            .text_color(p.muted_foreground)
            .child(agent_count_label(open.len())),
        )
      })
      .child(review_button);

    // Inline, each comment goes under the paragraph it quotes; the rest,
    // and every comment of an issue, go under the text.
    let blocks = if inline {
      data::paragraphs(&shown)
    } else {
      vec![shown]
    };
    let mut at_block: HashMap<usize, Vec<&LinearComment>> = HashMap::new();
    let mut unanchored: Vec<&LinearComment> = Vec::new();
    for comment in &comments {
      match data::quoted_block(&blocks, comment.quoted_text.as_deref()).filter(|_| inline) {
        Some(ix) => at_block.entry(ix).or_default().push(comment),
        None => unanchored.push(comment),
      }
    }
    let text = div()
      .flex()
      .flex_col()
      .gap_3()
      .children(blocks.iter().enumerate().map(|(ix, block)| {
        let anchored = at_block.remove(&ix).unwrap_or_default();
        div()
          .flex()
          .flex_col()
          .gap_2()
          .child(markdown(
            format!("linear-md-{key}-{ix}"),
            &data::highlight_quoted(block, &anchors),
            13.,
            self.dark,
          ))
          .children(anchored.into_iter().map(|c| {
            div()
              .ml_4()
              .child(self.comment_card(&of, &open, c, false, p, cx))
          }))
      }));

    let heading = |text: &'static str| {
      ui::text(div(), TextSize::Xs)
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.muted_foreground)
        .child(text)
    };
    // An issue always says it has none; a document or project without
    // comments shows only its text.
    let empty = !fetch.is_loading() && !fetch.failed() && comments.is_empty();
    let show_comments = !inline || fetch.is_loading() || fetch.failed() || !unanchored.is_empty();
    let below = (show_comments || !body_comments.is_empty()).then(|| {
      div()
        .debug_selector(|| "linear-comments-column".into())
        .w_full()
        .flex()
        .flex_col()
        .gap_4()
        .border_t_1()
        .border_color(p.border)
        .pt_4()
        .when(!body_comments.is_empty(), |this| {
          this.child(
            div()
              .debug_selector(|| "linear-agent-review-body".into())
              .flex()
              .flex_col()
              .gap_3()
              .child(heading("AGENT REVIEW"))
              .children(
                body_comments
                  .iter()
                  .map(|c| self.agent_card(&of, c, "Description".into(), p, cx)),
              ),
          )
        })
        .when(show_comments, |this| {
          this
            .child(heading("COMMENTS"))
            .when(fetch.is_loading(), |this| {
              this.child(ui::spinner(
                format!("linear-comments-loading-{key}"),
                16.,
                p.muted_foreground,
              ))
            })
            .when(fetch.failed(), |this| {
              this.child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.destructive)
                  .child("Failed to load comments"),
              )
            })
            .when(empty, |this| {
              this.child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child("No comments"),
              )
            })
            .children(
              // A long thread shows its first comment and the newest few.
              super::rows::thread(unanchored.len(), self.comments_expanded.contains(&of))
                .into_iter()
                .map(|ix| match ix {
                  Some(ix) => self
                    .comment_card(&of, &open, unanchored[ix], true, p, cx)
                    .into_any_element(),
                  None => {
                    let of = of.clone();
                    super::github::parts::fold_row(
                      "linear-comments-show-earlier",
                      super::rows::hidden(unanchored.len()),
                      p,
                      cx.listener(move |view, _, _, cx| {
                        view.comments_expanded.insert(of.clone());
                        cx.notify();
                      }),
                    )
                  }
                }),
            )
        })
    });
    div()
      .debug_selector(|| "linear-commented-content".into())
      .flex()
      .flex_col()
      .gap_4()
      .child(toolbar)
      .child(text)
      .children(below)
  }

  /// A Linear comment's bubble: its quote (when `quote` and it has one),
  /// author, time and body, then the agent's findings on it.
  fn comment_card(
    &self,
    of: &Commented,
    open: &[AgentReviewComment],
    comment: &LinearComment,
    quote: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let author = comment
      .user
      .as_ref()
      .map_or_else(|| "Unknown".to_string(), |u| u.name.clone());
    let selector = format!("linear-comment-{}", comment.id);
    let file = comment_file(&comment.id);
    let quote_border = ui::Tone::Warning.border(p);
    div()
      .debug_selector(move || selector)
      .flex()
      .flex_col()
      .gap_1p5()
      .p_3()
      .rounded(radius::MD)
      .bg(p.muted.opacity(0.3))
      .when_some(
        comment.quoted_text.clone().filter(|_| quote),
        |this, quote| {
          this.child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .italic()
              .border_l_2()
              .border_color(quote_border)
              .pl_2()
              .child(quote),
          )
        },
      )
      .child(
        ui::text(div(), TextSize::Xs)
          .flex()
          .items_center()
          .gap_2()
          .text_color(p.muted_foreground)
          .child(
            div()
              .font_weight(FontWeight::MEDIUM)
              .text_color(p.foreground)
              .child(author.clone()),
          )
          .child(time::locale(&comment.created_at)),
      )
      .child(markdown(
        format!("linear-comment-md-{}", comment.id),
        &comment.body,
        13.,
        self.dark,
      ))
      .children(
        open
          .iter()
          .filter(|c| c.file_path == file)
          .map(|c| self.agent_card(of, c, format!("Comment by {author}"), p, cx)),
      )
  }

  /// `AgentReviewCommentCard` with `applyLabel="Apply to Linear"` and no
  /// "Send to agent".
  fn agent_card(
    &self,
    of: &Commented,
    comment: &AgentReviewComment,
    location: String,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let busy = self.busy.contains(&comment.id);
    let action = |action: Action| {
      let (of, id) = (of.clone(), comment.id.clone());
      move |view: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
        view.review_action(of.clone(), id.clone(), action, cx)
      }
    };
    let id = &comment.id;
    let mut actions = div().flex().flex_wrap().items_center().gap_2();
    if comment.suggested_replacement.is_some() {
      actions = actions.child(
        Button::new(SharedString::from(format!("linear-agent-apply-{id}")), p)
          .size(Size::Sm)
          .gap_1p5()
          .disabled(busy)
          .icon(theme::ICON_WAND, 12.)
          .label("Apply to Linear")
          .on_click(cx.listener(action(Action::Apply))),
      );
    }
    actions = actions
      .child(
        Button::new(SharedString::from(format!("linear-agent-resolve-{id}")), p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_1p5()
          .disabled(busy)
          .icon(theme::ICON_CHECK, 12.)
          .label("Resolve")
          .on_click(cx.listener(action(Action::Resolve))),
      )
      .child(
        Button::new(SharedString::from(format!("linear-agent-delete-{id}")), p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .gap_1p5()
          .text_color(p.muted_foreground)
          .disabled(busy)
          .icon(theme::ICON_TRASH, 12.)
          .label("Delete")
          .on_click(cx.listener(action(Action::Delete))),
      );
    div()
      .debug_selector(|| "agent-review-comment-card".into())
      .rounded(radius::MD)
      .border_1()
      .border_color(ui::Tone::GitMerged.border(p))
      .bg(ui::Tone::GitMerged.tint(p))
      .p_3()
      .flex()
      .flex_col()
      .gap_2()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(theme::ICON_BOT, 14., ui::Tone::GitMerged.color(p)))
          .child(
            ui::text(div(), TextSize::Xs)
              .font_weight(FontWeight::MEDIUM)
              .text_color(ui::Tone::GitMerged.text(p))
              .child("Local"),
          )
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(location),
          ),
      )
      .child(
        ui::text(div(), TextSize::Sm)
          .whitespace_normal()
          .child(comment.comment_text.clone()),
      )
      .when_some(comment.suggested_replacement.clone(), |this, suggested| {
        this.child(suggestion_box(
          comment.quoted_text.as_deref(),
          &suggested,
          p,
        ))
      })
      .child(actions)
  }
}
/// A review agent's session in the home repository of `repo`, named
/// "AI Review" (`launchAgentSession`).
pub(super) fn review_launch(
  repo: &str,
  prompt: String,
  agent: Option<String>,
) -> super::launch_data::LaunchRequest {
  super::launch_data::LaunchRequest {
    repo_path: repo.to_string(),
    workspace_id: None,
    prompt: Some(prompt),
    mode: Some("acceptEdits".into()),
    agent: crate::core::agent_launch::agent_kind(agent.as_deref()),
    session_name: Some("AI Review".into()),
  }
}
