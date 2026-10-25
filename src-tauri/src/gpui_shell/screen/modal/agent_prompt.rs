//! `AgentPromptDialog`: starts an agent session on a prompt. The workspace
//! to run in, the prompt (with `@` file mentions and attachments), the agent
//! and the permission mode are chosen here, and the backend starts the
//! session (`startAgentTask`, in `core::agent_launch`): it opens the issue's
//! workspaces, records the prompt and tells React which session to show.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::time::Duration;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
  input::{
    Enter, Escape, IndentInline, InputEvent, InputState, MoveDown, MoveUp, Textarea, TextareaState,
  },
  Sizable,
};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, deferred, div, prelude::*, Context, Entity, FontWeight, SharedString, Stateful,
  Subscription, Task, Window,
};

use super::{dialog, fuzzy_matches, overlay, title, Env};
use crate::core::agent_launch::{self as launch, IssueAttachment, TaskRequest};
use crate::gpui_shell::{
  native::{NativeEvent, ToastTone},
  popup::{Side, TooltipContent},
  screen::{
    code::{agent_icon, menu_panel},
    code_data::{
      self, insert_at, mention_at, replace_range, task_settings, Agent, Environment, TaskSettings,
      ENVIRONMENT_SETTING,
    },
    environment,
    file_picker::{FilePicker, FilePickerEvent, Scope},
  },
  theme::{self, Palette},
  tooltips::{HasTooltips, Tooltips},
  ui::{self, Button, Size, TextSize, Variant},
};

/// `useDebounce(mentionQuery, 150)`.
const MENTION_DEBOUNCE: Duration = Duration::from_millis(150);

/// A workspace the prompt can run in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChoice {
  pub id: i64,
  pub branch_name: String,
  /// `getFullWorkspacePath`, the checkout the file search reads.
  pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueSource {
  Github,
  Linear,
}

impl IssueSource {
  /// `ISSUE_SOURCES[source].label`.
  fn label(self) -> &'static str {
    match self {
      IssueSource::Github => "GitHub",
      IssueSource::Linear => "Linear",
    }
  }
}

/// The issue attached to the prompt (`IssueAttachment`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
  pub source: IssueSource,
  pub key: String,
  pub title: String,
  pub include_sub_items: bool,
  /// What the source's backend needs to open the issue's workspace.
  #[serde(default)]
  pub id: String,
  #[serde(default)]
  pub url: String,
  #[serde(default)]
  pub sub_item_ids: Vec<String>,
}

impl Issue {
  fn attachment(&self) -> IssueAttachment {
    IssueAttachment {
      source: match self.source {
        IssueSource::Github => launch::IssueSource::Github,
        IssueSource::Linear => launch::IssueSource::Linear,
      },
      id: self.id.clone(),
      key: self.key.clone(),
      url: self.url.clone(),
      title: self.title.clone(),
      include_sub_items: self.include_sub_items,
      sub_item_ids: self.sub_item_ids.clone(),
    }
  }
}

/// What React sends: where the prompt can run, and what it starts with.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  pub default_branch: String,
  pub workspaces: Vec<WorkspaceChoice>,
  /// Pre-fills the prompt (re-running a past one).
  #[serde(default)]
  pub initial_prompt: Option<String>,
  /// Pre-selects the workspace the prompt first ran in.
  #[serde(default)]
  pub initial_workspace_id: Option<i64>,
  /// While one is attached the session runs in the issue's workspace.
  #[serde(default)]
  pub issue: Option<Issue>,
  /// React is starting the session (React's own dialog flow).
  #[serde(default)]
  pub starting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
  Add,
  Plan,
  Agent,
  Environment,
}

/// `mentionQuery` and its results.
#[derive(Debug, Clone, Default)]
pub struct Mention {
  pub anchor: usize,
  pub query: String,
  pub results: Vec<String>,
  pub index: usize,
}

/// The workspace combobox's open popover.
struct Picker {
  input: Entity<InputState>,
  query: String,
  index: usize,
  _subscription: Subscription,
}

pub struct AgentPromptView {
  env: Env,
  tips: Tooltips,
  pub(in crate::gpui_shell::screen) route: Route,
  dark: bool,
  /// The chosen workspace; `None` runs in the default branch.
  pub(in crate::gpui_shell::screen) selected: Option<i64>,
  picker: Option<Picker>,
  /// The attached issue; removing the chip detaches it.
  pub(in crate::gpui_shell::screen) issue: Option<Issue>,
  task: Option<Entity<TextareaState>>,
  pub(in crate::gpui_shell::screen) task_text: String,
  focused: bool,
  pub(in crate::gpui_shell::screen) settings: Option<TaskSettings>,
  pub(in crate::gpui_shell::screen) agent_override: Option<Agent>,
  /// Agents whose command-line tool isn't installed, left out of the menu.
  missing: Vec<Agent>,
  save_default: bool,
  show_save_default: bool,
  /// Submitted, until React reports back through the route.
  submitted: bool,
  dark_changed: bool,
  pub(in crate::gpui_shell::screen) menu: Option<Menu>,
  pub(in crate::gpui_shell::screen) mention: Option<Mention>,
  pub(in crate::gpui_shell::screen) file_picker: Option<Entity<FilePicker>>,
  starting_devcontainer: bool,
  /// Text to put in the task input at its cursor on the next render.
  insert: Option<String>,
  /// Takes the keyboard on opening, so Escape reaches the dialog.
  focus: gpui::FocusHandle,
  _mention_search: Option<Task<()>>,
  _subscriptions: Vec<Subscription>,
}

impl HasTooltips for AgentPromptView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

/// Runs `f` on the dialog's host in the background, then `apply`s it.
macro_rules! read {
  ($self:ident, $cx:ident, |$host:ident, $route:ident| $read:expr, |$view:ident, $value:ident, $cx2:ident| $apply:expr) => {{
    let ($host, $route) = ($self.env.host.clone(), $self.route.clone());
    $cx
      .spawn(async move |this, cx| {
        let $value = cx
          .background_spawn(async move {
            let $host = $host;
            let $route = $route;
            $read
          })
          .await;
        this
          .update(cx, |$view, $cx2| {
            $apply;
            $cx2.notify();
          })
          .ignore_closed();
      })
      .detach();
  }};
}

impl AgentPromptView {
  pub fn new(env: Env, route: Route, dark: bool, cx: &mut Context<Self>) -> Self {
    let tips = Tooltips::new(env.popups.clone(), dark);
    let mut this = Self {
      env,
      tips,
      selected: route
        .initial_workspace_id
        .filter(|id| route.workspaces.iter().any(|w| w.id == *id)),
      issue: route.issue.clone(),
      task_text: route.initial_prompt.clone().unwrap_or_default(),
      route,
      dark,
      picker: None,
      task: None,
      focused: false,
      settings: None,
      agent_override: None,
      missing: Vec::new(),
      save_default: false,
      show_save_default: false,
      submitted: false,
      dark_changed: false,
      menu: None,
      mention: None,
      file_picker: None,
      starting_devcontainer: false,
      insert: None,
      focus: cx.focus_handle(),
      _mention_search: None,
      _subscriptions: Vec::new(),
    };
    this.load_settings(cx);
    this.load_missing(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
      self.dark_changed = true;
    }
    if self.route == route && !self.dark_changed {
      return;
    }
    self.dark_changed = false;
    let moved = self.route.repo_path != route.repo_path;
    // React reports the start (and a failure) through `starting`.
    if self.route.starting != route.starting {
      self.submitted = false;
    }
    // Keeps the chosen workspace and the text while React refreshes its
    // lists; a workspace that went away falls back to the default branch.
    if self
      .selected
      .is_some_and(|id| !route.workspaces.iter().any(|w| w.id == id))
    {
      self.selected = None;
    }
    self.route = route;
    if moved {
      self.settings = None;
      self.load_settings(cx);
    }
    cx.notify();
  }

  /// `TaskInput`'s `agentSettings` and the repository's environment.
  fn load_settings(&mut self, cx: &mut Context<Self>) {
    read!(
      self,
      cx,
      |host, route| task_settings(host.as_ref(), &route.repo_path),
      |view, settings, cx| {
        let environment = settings.environment.is_some();
        view.settings = Some(settings);
        if environment {
          view.load_environment_status(cx);
        }
      }
    );
  }

  /// Whether the repository's devcontainer runs (a `docker ps`), read after
  /// the settings so the default agent shows without waiting for it.
  fn load_environment_status(&mut self, cx: &mut Context<Self>) {
    read!(
      self,
      cx,
      |host, route| host.devcontainer_status(&route.repo_path),
      |view, status, _cx| {
        if let Some(env) = view.settings.as_mut().and_then(|s| s.environment.as_mut()) {
          env.status = Some(status);
        }
      }
    );
  }

  fn load_missing(&mut self, cx: &mut Context<Self>) {
    read!(
      self,
      cx,
      |host, _route| host.check_prerequisites(),
      |view, statuses, _cx| {
        view.missing = Agent::ALL
          .into_iter()
          .filter(|agent| {
            statuses
              .iter()
              .any(|s| s.binary == agent.binary() && !s.installed)
          })
          .collect()
      }
    );
  }

  /// `selectedAgent` and `configuredDefaultAgent`.
  pub(in crate::gpui_shell::screen) fn agents(&self) -> (Agent, Agent) {
    let configured = self.settings.as_ref().map(|s| s.agent).unwrap_or_default();
    (configured, self.agent_override.unwrap_or(configured))
  }

  fn workspace(&self) -> Option<&WorkspaceChoice> {
    self
      .selected
      .and_then(|id| self.route.workspaces.iter().find(|w| w.id == id))
  }

  /// `workingDirectory`.
  fn working_directory(&self) -> String {
    self
      .workspace()
      .map_or_else(|| self.route.repo_path.clone(), |w| w.path.clone())
  }

  pub(in crate::gpui_shell::screen) fn submitting(&self) -> bool {
    self.submitted || self.route.starting
  }

  fn close(&self, cx: &mut Context<Self>) {
    self.env.press("agentPrompt.close", cx);
  }

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    self.env.toast(tone, title, description);
  }

  // The workspace combobox.

  fn selectable(&self) -> Vec<&WorkspaceChoice> {
    self
      .route
      .workspaces
      .iter()
      .filter(|w| w.branch_name != self.route.default_branch)
      .collect()
  }

  /// The combobox rows left after the search: `None` is the default branch.
  fn picker_rows(&self) -> Vec<Option<&WorkspaceChoice>> {
    let query = self.picker.as_ref().map_or("", |p| p.query.as_str());
    std::iter::once(None)
      .chain(self.selectable().into_iter().map(Some))
      .filter(|row| {
        let name = row.map_or(self.route.default_branch.as_str(), |w| {
          w.branch_name.as_str()
        });
        fuzzy_matches(query, name)
      })
      .collect()
  }

  /// Closes the combobox, handing the keyboard back to the prompt: its
  /// search field goes with it, and Escape needs a focused element.
  fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
    if self.picker.take().is_none() {
      return false;
    }
    self.focus_task(window, cx);
    cx.notify();
    true
  }

  fn focus_task(&self, window: &mut Window, cx: &mut Context<Self>) {
    match &self.task {
      Some(task) => task.update(cx, |task, cx| task.focus(window, cx)),
      None => window.focus(&self.focus, cx),
    }
  }

  fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.menu = None;
    if self.close_picker(window, cx) {
      return;
    }
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search workspaces..."));
    input.update(cx, |input, cx| input.focus(window, cx));
    let subscription = cx.subscribe_in(
      &input,
      window,
      |view, input, event: &InputEvent, window, cx| match event {
        InputEvent::Change => {
          if let Some(picker) = view.picker.as_mut() {
            picker.query = input.read(cx).value().to_string();
            picker.index = 0;
            cx.notify();
          }
        }
        InputEvent::PressEnter { .. } => {
          let index = view.picker.as_ref().map_or(0, |p| p.index);
          view.choose_row(index, window, cx);
        }
        _ => {}
      },
    );
    self.picker = Some(Picker {
      input,
      query: String::new(),
      index: 0,
      _subscription: subscription,
    });
    cx.notify();
  }

  fn step_picker(&mut self, delta: isize, cx: &mut Context<Self>) {
    let len = self.picker_rows().len() as isize;
    if let (Some(picker), true) = (self.picker.as_mut(), len > 0) {
      picker.index = (picker.index as isize + delta).rem_euclid(len) as usize;
      cx.notify();
    }
  }

  fn choose_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
    let rows = self.picker_rows();
    let Some(row) = rows.get(index).map(|row| row.map(|w| w.id)) else {
      return;
    };
    self.choose_workspace(row, window, cx);
  }

  /// `setSelectedWorkspace`, and `TaskInput`'s reset for a new workspace.
  pub(in crate::gpui_shell::screen) fn choose_workspace(
    &mut self,
    id: Option<i64>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.close_picker(window, cx);
    if self.selected != id {
      self.selected = id;
      self.agent_override = None;
      self.save_default = false;
      self.show_save_default = false;
      self.mention = None;
      self.load_settings(cx);
    }
    cx.notify();
  }

  // The prompt.

  fn task_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
    if let Some(task) = &self.task {
      return task.clone();
    }
    let text = self.task_text.clone();
    let task = cx.new(|cx| {
      TextareaState::new(window, cx)
        .placeholder("Describe a task...")
        .auto_grow(1, 9)
        .default_value(text)
    });
    self._subscriptions.push(cx.subscribe(
      &task,
      |view, task, event: &InputEvent, cx| match event {
        InputEvent::Change => {
          let state = task.read(cx);
          let (text, cursor) = (state.value().to_string(), state.cursor());
          view.text_changed(text, cursor, cx);
        }
        InputEvent::Focus => {
          view.focused = true;
          cx.notify();
        }
        InputEvent::Blur => {
          view.focused = false;
          view.mention = None;
          cx.notify();
        }
        InputEvent::PressEnter { .. } => {}
      },
    ));
    task.update(cx, |task, cx| task.focus(window, cx));
    self.focused = true;
    self.task = Some(task.clone());
    task
  }

  /// `handleTextChange`: follows an `@` being typed.
  pub(in crate::gpui_shell::screen) fn text_changed(
    &mut self,
    text: String,
    cursor: usize,
    cx: &mut Context<Self>,
  ) {
    self.task_text = text;
    match mention_at(&self.task_text, cursor) {
      Some((anchor, query)) => {
        let results = self.mention.take().map(|m| m.results).unwrap_or_default();
        self.mention = Some(Mention {
          anchor,
          query: query.clone(),
          results,
          index: 0,
        });
        self.search_mentions(query, cx);
      }
      None => self.mention = None,
    }
    cx.notify();
  }

  fn search_mentions(&mut self, query: String, cx: &mut Context<Self>) {
    let (host, repo) = (self.env.host.clone(), self.route.repo_path.clone());
    let (workspace_id, path) = (self.selected, self.working_directory());
    self._mention_search = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(MENTION_DEBOUNCE).await;
      let search = query.clone();
      let results = cx
        .background_spawn(async move { host.search_files(&repo, workspace_id, &path, &search, 4) })
        .await
        .unwrap_or_default();
      this
        .update(cx, |view, cx| {
          if let Some(mention) = view.mention.as_mut().filter(|m| m.query == query) {
            mention.results = results;
            mention.index = 0;
            cx.notify();
          }
        })
        .ignore_closed();
    }));
  }

  fn mention_open(&self) -> bool {
    self.mention.as_ref().is_some_and(|m| !m.results.is_empty())
  }

  fn step_mention(&mut self, delta: isize, cx: &mut Context<Self>) {
    if let Some(m) = self.mention.as_mut().filter(|m| !m.results.is_empty()) {
      let len = m.results.len() as isize;
      m.index = (m.index as isize + delta).rem_euclid(len) as usize;
      cx.notify();
    }
  }

  /// `handleMentionSelect`: replaces `@query` with `@path `.
  pub(in crate::gpui_shell::screen) fn select_mention(
    &mut self,
    index: usize,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let Some(mention) = self.mention.take() else {
      return;
    };
    let Some(path) = mention.results.get(index) else {
      return;
    };
    let cursor = self
      .task
      .as_ref()
      .map_or(self.task_text.len(), |t| t.read(cx).cursor());
    let inserted = format!("@{path} ");
    let text = replace_range(&self.task_text, mention.anchor, cursor, &inserted);
    self.set_task_text(text, mention.anchor + inserted.len(), window, cx);
  }

  fn set_task_text(
    &mut self,
    text: String,
    cursor: usize,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.task_text = text.clone();
    if let Some(task) = &self.task {
      task.update(cx, |task, cx| {
        task.set_value(text, window, cx);
        task.set_selected_range(cursor..cursor, cx);
        task.focus(window, cx);
      });
    }
    cx.notify();
  }

  /// `insertAtCursor`.
  fn insert_text(&mut self, insertion: &str, window: &mut Window, cx: &mut Context<Self>) {
    let cursor = self
      .task
      .as_ref()
      .map_or(self.task_text.len(), |t| t.read(cx).cursor());
    let (text, cursor) = insert_at(&self.task_text, cursor, insertion);
    self.set_task_text(text, cursor, window, cx);
  }

  fn open_file_picker(&mut self, cx: &mut Context<Self>) {
    self.menu = None;
    let scope = Scope {
      repo_path: self.route.repo_path.clone(),
      workspace_id: self.selected,
      workspace_path: self.working_directory(),
    };
    let host: std::sync::Arc<dyn code_data::CodeHost> = self.env.host.clone();
    let dark = self.dark;
    let picker = cx.new(|cx| FilePicker::new(host, scope, dark, cx));
    self._subscriptions.push(
      cx.subscribe(&picker, |view, _, event: &FilePickerEvent, cx| {
        if let FilePickerEvent::Selected(path) = event {
          view.insert = Some(format!("@{path} "));
        }
        view.file_picker = None;
        cx.notify();
      }),
    );
    self.file_picker = Some(picker);
    cx.notify();
  }

  /// `handleAttachFromFinder`.
  fn attach(&mut self, cx: &mut Context<Self>) {
    self.menu = None;
    let host = self.env.host.clone();
    cx.spawn(async move |this, cx| {
      let files = cx.background_spawn(async move { host.pick_files() }).await;
      let Some(files) = files.filter(|f| !f.is_empty()) else {
        return;
      };
      let insertion = format!(
        "{} ",
        files
          .iter()
          .map(|p| format!("'{p}'"))
          .collect::<Vec<_>>()
          .join(" ")
      );
      this
        .update(cx, |view, cx| {
          view.insert = Some(insertion);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  // The toolbar.

  fn choose_agent(&mut self, agent: Agent, cx: &mut Context<Self>) {
    let (configured, _) = self.agents();
    self.agent_override = Some(agent);
    self.save_default = false;
    self.show_save_default = agent != configured;
    self.menu = None;
    cx.notify();
  }

  fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
    self.picker = None;
    self.menu = if self.menu == Some(menu) {
      None
    } else {
      Some(menu)
    };
    cx.notify();
  }

  fn set_environment(&mut self, devcontainer: bool, cx: &mut Context<Self>) {
    self.menu = None;
    let value = if devcontainer {
      "devcontainer"
    } else {
      "local"
    };
    read!(
      self,
      cx,
      |host, route| host.set_repo_settings(
        &route.repo_path,
        &[(ENVIRONMENT_SETTING.into(), value.into())],
      ),
      |view, result, cx| {
        if let Err(err) = result {
          view.toast(ToastTone::Error, "Failed to save setting", err);
        } else {
          view.env.notify(NativeEvent::RepoSettingsChanged {
            repo_path: view.route.repo_path.clone(),
          });
          if let Some(env) = view.settings.as_mut().and_then(|s| s.environment.as_mut()) {
            env.devcontainer = devcontainer;
            let stopped = env.status.as_ref().is_some_and(|s| !s.running);
            if devcontainer && stopped && !view.starting_devcontainer {
              view.start_devcontainer(cx);
            }
          }
        }
      }
    );
  }

  fn start_devcontainer(&mut self, cx: &mut Context<Self>) {
    self.menu = None;
    self.starting_devcontainer = true;
    cx.notify();
    read!(
      self,
      cx,
      |host, route| host.devcontainer_up(&route.repo_path),
      |view, result, _cx| {
        view.starting_devcontainer = false;
        match result {
          Ok(status) => {
            if let Some(env) = view.settings.as_mut().and_then(|s| s.environment.as_mut()) {
              env.status = Some(status);
            }
          }
          Err(err) => view.toast(ToastTone::Error, "Devcontainer did not start", err),
        }
      }
    );
  }

  /// `submitTask`: the backend starts the session, and the dialog closes
  /// once it has.
  pub(in crate::gpui_shell::screen) fn submit(&mut self, plan: bool, cx: &mut Context<Self>) {
    if self.submitting() || (self.task_text.trim().is_empty() && self.issue.is_none()) {
      return;
    }
    self.submitted = true;
    self.menu = None;
    cx.notify();
    let (configured, agent) = self.agents();
    let task = TaskRequest {
      repo_path: self.route.repo_path.clone(),
      workspace_id: self.selected,
      text: self.task_text.clone(),
      issue: self.issue.as_ref().map(Issue::attachment),
      agent: agent.id().into(),
      mode: if plan { "plan" } else { "acceptEdits" }.into(),
      save_as_repo_default: self.save_default && agent != configured,
    };
    read!(
      self,
      cx,
      |host, _route| host.start_agent_task(&task),
      |view, result, cx| match result {
        Ok(true) => view.env.press("agentPrompt.close", cx),
        Ok(false) => view.submitted = false,
        Err(err) => {
          view.submitted = false;
          view
            .env
            .toast(ToastTone::Error, "Failed to create task", err);
        }
      }
    );
  }
}

impl AgentPromptView {
  /// The `Select` of workspaces (`Popover` + `Command`).
  fn workspace_picker(&mut self, p: &Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
    if let Some(issue) = &self.issue {
      let noun = if issue.include_sub_items {
        " and its sub-issues"
      } else {
        ""
      };
      return ui::text(div(), TextSize::Sm)
        .debug_selector(|| "agent-prompt-issue-note".into())
        .flex()
        .items_center()
        .gap_2()
        .text_color(p.muted_foreground)
        .child(ui::icon(theme::ICON_BRANCH, 16., p.muted_foreground))
        .child(format!("Opens a workspace for {}{noun}", issue.key))
        .into_any_element();
    }
    let name = self
      .workspace()
      .map_or(self.route.default_branch.clone(), |w| w.branch_name.clone());
    let button = Button::new("agent-prompt-workspace", p)
      .aria_label(format!("Workspace: {name}"))
      .variant(Variant::Outline)
      .min_w(zpx(240.))
      .max_w_full()
      .justify_between()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .min_w_0()
          .child(ui::icon(theme::ICON_BRANCH, 16., p.foreground))
          .child(div().truncate().child(name)),
      )
      .child(ui::icon(theme::ICON_CHEVRONS_UP_DOWN, 16., p.foreground).opacity(0.5))
      .on_click(cx.listener(|view, _, window, cx| view.toggle_picker(window, cx)));
    let popover = self.picker.as_ref().map(|picker| {
      let rows = self.picker_rows();
      let accent = p.menu_accent;
      let index = picker.index;
      let list = if rows.is_empty() {
        ui::text(div(), TextSize::Sm)
          .p_4()
          .text_center()
          .text_color(p.muted_foreground)
          .child("No workspaces found")
          .into_any_element()
      } else {
        div()
          .children(rows.iter().enumerate().map(|(i, row)| {
            let id = row.map(|w| w.id);
            let label = row.map_or(self.route.default_branch.clone(), |w| w.branch_name.clone());
            let chosen = self.selected == id;
            let selector = SharedString::from(format!("agent-prompt-workspace-{}", label));
            ui::text(div(), TextSize::Sm)
              .id(selector.clone())
              .debug_selector(move || selector.to_string())
              .flex()
              .items_center()
              .gap_2()
              .px_2()
              .py_2()
              .rounded(radius::SM)
              .cursor_pointer()
              .when(i == index, |this| this.bg(accent))
              .child(
                ui::icon(theme::ICON_CHECK, 16., p.popover_foreground).opacity(if chosen {
                  1.
                } else {
                  0.
                }),
              )
              .child(div().truncate().child(label))
              .on_click(
                cx.listener(move |view, _, window, cx| view.choose_workspace(id, window, cx)),
              )
          }))
          .into_any_element()
      };
      deferred(
        div()
          .id("agent-prompt-workspace-popover")
          .debug_selector(|| "agent-prompt-workspace-popover".into())
          .absolute()
          .top(zpx(44.))
          .left_0()
          .w(zpx(288.))
          .rounded(radius::MD)
          .border_1()
          .border_color(p.border)
          .bg(p.popover)
          .text_color(p.popover_foreground)
          .shadow_md()
          .occlude()
          .capture_action(cx.listener(|view, _: &MoveDown, _, cx| {
            view.step_picker(1, cx);
            cx.stop_propagation();
          }))
          .capture_action(cx.listener(|view, _: &MoveUp, _, cx| {
            view.step_picker(-1, cx);
            cx.stop_propagation();
          }))
          .on_mouse_down_out(cx.listener(|view, _, window, cx| {
            view.close_picker(window, cx);
          }))
          .child(
            ui::text(div(), TextSize::Sm)
              .flex()
              .items_center()
              .h(zpx(40.))
              .px_3()
              .border_b_1()
              .border_color(p.border)
              .child(
                ui::input(&picker.input)
                  .appearance(false)
                  .bordered(false)
                  .flex_1(),
              ),
          )
          .child(
            div()
              .id("agent-prompt-workspace-list")
              .max_h(zpx(256.))
              .overflow_y_scroll()
              .p_1()
              .child(list),
          ),
      )
      .with_priority(3)
    });
    div()
      .flex()
      .child(
        div()
          .relative()
          .max_w_full()
          .child(button)
          .children(popover),
      )
      .into_any_element()
  }

  /// `IssueChip`.
  fn issue_chip(&self, issue: &Issue, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let color = match issue.source {
      IssueSource::Github => ui::Tone::Success.text(p),
      IssueSource::Linear => ui::Tone::GitMerged.text(p),
    };
    div()
      .flex()
      .flex_wrap()
      .items_center()
      .gap_1p5()
      .px_3()
      .pt_3()
      .child(
        ui::text(div(), TextSize::Xs)
          .debug_selector(|| "issue-chip".into())
          .flex()
          .items_center()
          .gap_1()
          .rounded(radius::MD)
          .border_1()
          .border_color(p.border)
          .bg(p.muted.opacity(0.5))
          .px_2()
          .py_0p5()
          .child(ui::icon(theme::ICON_CIRCLE_DOT, 12., color))
          .child(div().font_weight(FontWeight::MEDIUM).child(format!(
            "{} {}",
            issue.source.label(),
            issue.key
          )))
          .child(
            div()
              .id("issue-chip-remove")
              .debug_selector(|| "issue-chip-remove".into())
              .ml_0p5()
              .p_0p5()
              .rounded(radius::SM)
              .cursor_pointer()
              .hover({
                let muted = p.muted;
                move |s| s.bg(muted)
              })
              .child(ui::icon(theme::ICON_X, 12., p.muted_foreground))
              .on_click(cx.listener(|view, _, _, cx| {
                view.issue = None;
                cx.notify();
              })),
          ),
      )
  }

  fn task_card(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let task = self.task_input(window, cx);
    let border = if self.focused { p.primary } else { p.border };
    let mention = self.mention.clone().filter(|m| !m.results.is_empty());
    let issue = self.issue.clone();
    div()
      .debug_selector(|| "agent-task-input".into())
      .w_full()
      .max_w(zpx(672.))
      .mx_auto()
      .child(
        div()
          .id("task-card")
          .min_h(zpx(112.))
          .rounded(radius::LG)
          .border_1()
          .border_color(border)
          .bg(p.background)
          .text_color(p.foreground)
          .relative()
          .flex()
          .flex_col()
          .capture_action(cx.listener(|view, _: &MoveDown, _, cx| {
            if view.mention_open() {
              view.step_mention(1, cx);
              cx.stop_propagation();
            }
          }))
          .capture_action(cx.listener(|view, _: &MoveUp, _, cx| {
            if view.mention_open() {
              view.step_mention(-1, cx);
              cx.stop_propagation();
            }
          }))
          .capture_action(cx.listener(|view, action: &Enter, window, cx| {
            if view.mention_open() && !action.secondary {
              let index = view.mention.as_ref().map_or(0, |m| m.index);
              view.select_mention(index, window, cx);
              cx.stop_propagation();
            } else if action.secondary && !action.shift {
              view.submit(false, cx);
              cx.stop_propagation();
            }
          }))
          .capture_action(cx.listener(|view, _: &Escape, _, cx| {
            if view.mention.take().is_some() {
              cx.notify();
            }
          }))
          .capture_action(cx.listener(|view, _: &IndentInline, _, cx| {
            if view.mention.take().is_some() {
              cx.notify();
            }
          }))
          .children(issue.map(|issue| self.issue_chip(&issue, p, cx)))
          .child(
            ui::text(div(), TextSize::Sm)
              .debug_selector(|| "task-input".into())
              // The textarea's own 4px inset makes up React's `px-4`.
              .px_3()
              .pt_3()
              .pb_1()
              .min_h(zpx(44.))
              .max_h(zpx(200.))
              .child(ui::text(
                Textarea::new(&task)
                  .appearance(false)
                  .bordered(false)
                  .xsmall(),
                TextSize::Sm,
              )),
          )
          .children(mention.map(|m| self.mention_dropdown(&m, p, cx)))
          .child(div().mt_auto().child(self.toolbar(p, cx))),
      )
  }

  fn mention_dropdown(
    &self,
    mention: &Mention,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let accent = p.accent;
    div()
      .debug_selector(|| "task-mentions".into())
      .mx_2()
      .mb_1()
      .rounded(radius::LG)
      .border_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.5))
      .overflow_hidden()
      .child(
        ui::text(div(), TextSize::Xs)
          .px_3()
          .py_1()
          .font_weight(FontWeight::MEDIUM)
          .text_color(p.muted_foreground)
          .border_b_1()
          .border_color(p.border)
          .child("FILES"),
      )
      .children(mention.results.iter().enumerate().map(|(i, path)| {
        let id = SharedString::from(format!("task-mention-{i}"));
        let selector = id.to_string();
        let selected = i == mention.index;
        div()
          .id(id)
          .debug_selector(move || selector)
          .w_full()
          .px_3()
          .py_1p5()
          .flex()
          .items_center()
          .gap_2()
          .cursor_pointer()
          .when(selected, |this| this.bg(accent.opacity(0.5)))
          .when(!selected, |this| {
            this.hover(move |s| s.bg(accent.opacity(0.3)))
          })
          .child(ui::icon(theme::ICON_FILE_TEXT, 16., p.muted_foreground))
          .child(
            ui::text(div(), TextSize::Xs)
              .font_family(theme::MONO_FONT)
              .min_w_0()
              .truncate()
              .child(path.clone()),
          )
          .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |view, _, window, cx| {
              cx.stop_propagation();
              window.prevent_default();
              view.select_mention(i, window, cx);
            }),
          )
      }))
  }

  fn toolbar(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let (configured, agent) = self.agents();
    let empty = self.task_text.trim().is_empty() && self.issue.is_none();
    let plan = agent.has_plan_mode();
    let small = |b: Button| b.size(Size::Sm).h(zpx(28.));
    let submitting = self.submitting();

    let add = div()
      .relative()
      .child(
        small(Button::new("task-add", p).variant(Variant::Ghost))
          .aria_label("Add")
          .px_2()
          .gap_1()
          .child(
            ui::text(div(), TextSize::Xs)
              .flex()
              .items_center()
              .gap_1()
              .child(ui::icon(theme::ICON_PLUS, 16., p.foreground))
              .child("Add"),
          )
          .on_click(cx.listener(|view, _, _, cx| view.toggle_menu(Menu::Add, cx))),
      )
      .when(self.menu == Some(Menu::Add), |this| {
        this.child(self.add_menu(p, cx))
      });
    let left = div().flex().items_center().gap_1().child(add);

    let mut right = div()
      .flex()
      .min_w_0()
      .flex_wrap()
      .items_center()
      .justify_end()
      .gap_2();
    if self.show_save_default && agent != configured {
      let checked = self.save_default;
      right = right.child(
        ui::text(div(), TextSize::Xs)
          .id("task-save-default")
          .role(gpui::Role::CheckBox)
          .aria_label("Set as default for this repo")
          .aria_toggled(if checked {
            gpui::accesskit::Toggled::True
          } else {
            gpui::accesskit::Toggled::False
          })
          .test_support()
          .flex()
          .items_center()
          .gap_1p5()
          .cursor_pointer()
          .text_color(p.muted_foreground)
          .child(ui::Checkbox::new("task-save-default-box", p).checked(checked))
          .child("Set as default for this repo")
          .on_click(cx.listener(move |view, _, _, cx| {
            view.save_default = !checked;
            cx.notify();
          })),
      );
    }
    if let Some(env) = self.settings.as_ref().and_then(|s| s.environment.clone()) {
      right = right.child(self.environment(&env, p, cx));
    }
    right = right.child(
      div()
        .relative()
        .flex()
        .items_center()
        .child(
          // The wrapper keeps the combobox name, value and click; the
          // picker inside draws it.
          div()
            .id("task-agent")
            .debug_selector(|| "task-agent".into())
            .role(gpui::Role::ComboBox)
            .aria_label("Agent")
            .aria_value(agent.label())
            .aria_expanded(self.menu == Some(Menu::Agent))
            .test_support()
            .cursor_pointer()
            .child(
              ui::Picker::new("task-agent-picker", p)
                .placement(ui::PickerPlacement::Toolbar)
                .icon(agent_icon(agent))
                .value(agent.label())
                .open(self.menu == Some(Menu::Agent)),
            )
            .on_click(cx.listener(|view, _, _, cx| view.toggle_menu(Menu::Agent, cx))),
        )
        .when(self.menu == Some(Menu::Agent), |this| {
          this.child(self.agent_menu(agent, p, cx))
        }),
    );

    let disabled = empty || submitting;
    let label = if submitting {
      "Starting…"
    } else if agent == Agent::Claude {
      "Edit"
    } else {
      "Run"
    };
    let submit = small(Button::new("task-submit", p))
      .px_3()
      .disabled(disabled)
      .when(plan, |b| b.rounded_r_none())
      .when(submitting, |b| {
        b.child(ui::spinner("task-starting", 14., p.primary_foreground))
      })
      .aria_label(label)
      .child(ui::text(div(), TextSize::Xs).child(label))
      .on_click(cx.listener(|view, _, _, cx| view.submit(false, cx)));
    let submit = self.tips.wrap(
      "task-submit",
      Side::Top,
      Some(TooltipContent::Text(
        crate::gpui_shell::keys::hint("⌘ + Enter").into(),
      )),
      submit,
      cx,
    );
    let mut buttons = div().relative().flex().items_center().child(submit);
    if plan {
      buttons = buttons
        .child(
          small(Button::new("task-submit-menu", p))
            .aria_label("More submit options")
            .disabled(disabled)
            .rounded_l_none()
            .border_l_1()
            .border_color(p.primary_foreground.opacity(0.2))
            .px_1p5()
            .icon(theme::ICON_CHEVRON_DOWN, 14.)
            .on_click(cx.listener(|view, _, _, cx| view.toggle_menu(Menu::Plan, cx))),
        )
        .when(self.menu == Some(Menu::Plan) && !disabled, |this| {
          this.child(self.plan_menu(p, cx))
        });
    }
    right = right.child(buttons);

    div()
      .px_2()
      .pb_2()
      .pt_1()
      .flex()
      .min_w_0()
      .flex_wrap()
      .items_center()
      .justify_between()
      .gap_1()
      .child(left)
      .child(right)
  }

  fn close_menu_on_outside(
    &self,
    panel: Stateful<gpui::Div>,
    cx: &mut Context<Self>,
  ) -> Stateful<gpui::Div> {
    panel.on_mouse_down_out(cx.listener(|view, _, _, cx| {
      if view.menu.take().is_some() {
        cx.notify();
      }
    }))
  }

  fn add_menu(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let panel = menu_panel("task-add-menu", p)
      .top(zpx(32.))
      .left_0()
      .min_w(zpx(128.))
      .child(
        ui::MenuItem::new("task-add-file", "File", p)
          .icon(theme::ICON_PLUS)
          .on_click(cx.listener(|view, _, _, cx| view.open_file_picker(cx))),
      )
      .child(
        ui::MenuItem::new("task-add-attach", "Attach", p)
          .icon(theme::ICON_PAPERCLIP)
          .on_click(cx.listener(|view, _, _, cx| view.attach(cx))),
      );
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(3)
  }

  fn plan_menu(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let panel = menu_panel("task-plan-menu", p)
      .top(zpx(32.))
      .right_0()
      .w(zpx(224.))
      .child(
        ui::MenuItem::new("task-plan", "Plan", p)
          .description("Plan the change first, without editing files")
          .on_click(cx.listener(|view, _, _, cx| view.submit(true, cx))),
      );
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(3)
  }

  fn agent_menu(&self, selected: Agent, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let panel = menu_panel("task-agent-menu", p)
      .top(zpx(32.))
      .right_0()
      .min_w(zpx(128.))
      .child(ui::MenuLabel::new("Agent", p))
      // Agents that aren't installed are left out, unless already chosen.
      .children(
        Agent::ALL
          .into_iter()
          .filter(|agent| *agent == selected || !self.missing.contains(agent))
          .map(|agent| {
            ui::MenuItem::new(format!("task-agent-{}", agent.id()), agent.label(), p)
              .icon(agent_icon(agent))
              .checked(agent == selected)
              .aria_selected(agent == selected)
              .on_click(cx.listener(move |view, _, _, cx| view.choose_agent(agent, cx)))
          }),
      );
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(3)
  }

  fn environment(
    &self,
    env: &Environment,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let button = environment::picker("task-environment", env, p)
      .open(self.menu == Some(Menu::Environment))
      .on_click(cx.listener(|view, _, _, cx| view.toggle_menu(Menu::Environment, cx)));
    div()
      .relative()
      .child(button)
      .when(self.menu == Some(Menu::Environment), |this| {
        this.child(self.environment_menu(env, p, cx))
      })
  }

  fn environment_menu(
    &self,
    env: &Environment,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let panel = environment::menu(
      "task-environment-menu",
      env,
      self.starting_devcontainer,
      p,
      Self::set_environment,
      Self::start_devcontainer,
      cx,
    )
    .top(zpx(32.))
    .right_0();
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(3)
  }
}

impl Render for AgentPromptView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    let p = theme::palette(self.dark);
    if let Some(insertion) = self.insert.take() {
      self.insert_text(&insertion, window, cx);
    }
    let viewport = window.viewport_size();
    let (backdrop, panel) = dialog(
      "agent-prompt-dialog",
      viewport.width * 0.4,
      None,
      viewport,
      &p,
    );
    let card = self.task_card(&p, window, cx);
    let picker = self.workspace_picker(&p, cx);
    let keys = cx.listener(|view, e: &gpui::KeyDownEvent, window, cx| {
      if !ui::is_escape(&e.keystroke) || view.file_picker.is_some() {
        return;
      }
      cx.stop_propagation();
      if view.menu.take().is_some()
        || view.close_picker(window, cx)
        || view.mention.take().is_some()
      {
        cx.notify();
      } else {
        view.close(cx);
      }
    });
    overlay(
      backdrop
        .on_mouse_down(
          gpui::MouseButton::Left,
          cx.listener(|view, _, _, cx| view.close(cx)),
        )
        .children(self.file_picker.clone()),
      panel
        .id("agent-prompt-panel")
        .role(gpui::Role::Dialog)
        .aria_label("Start a new agent session")
        .test_support()
        .track_focus(&self.focus)
        .capture_key_down(keys)
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(title("Start a new agent session").text_color(p.popover_foreground))
        .child(picker)
        .child(card),
    )
  }
}
