//! The workspace Code tab (`ShowWorkspace`'s Overview): the task input, the
//! workspace's description and stack, its files and README, and its commit
//! history in a column on the right.
//!
//! Drawn from the backend directly ([`code_data`]). What stays React's goes
//! back to it as screen actions: opening a file or folder in the file
//! browser, the Commits and Changes tabs, Cmd+P's picker, moving to another
//! workspace, and the agent terminal a new task starts in.
//!
//! [`code_data`]: super::code_data

use crate::gpui_shell::ui::zpx;
use std::{sync::Arc, time::Duration};

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
  input::{Enter, Escape, IndentInline, InputEvent, MoveDown, MoveUp, Textarea, TextareaState},
  text::{TextView, TextViewStyle},
  Sizable,
};
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, deferred, div, prelude::*, px, relative, rems, rgb, Animation, AnimationExt as _, App,
  Context, Div, Entity, FontWeight, Hsla, Role, SharedString, Stateful, Subscription, Task, Window,
};

use super::{
  code_data::{
    self, conflict_count, description, diff_stats, display_title, group_by_day, headline,
    history_commits, insert_at, looks_like_markdown, mention_at, session_name, task_branch_name,
    task_settings, workspace_stack, Agent, CodeHost, Overview, TaskSettings, ENVIRONMENT_SETTING,
    NEW_WORKSPACE_SETTING, REMOTE_FILE_TREE_REASON,
  },
  environment,
  file_picker::{FilePicker, FilePickerEvent, Scope},
  merge::full_path,
  ActionSink, ScreenAction,
};
use crate::{
  core::WorkspaceEntry,
  gpui_shell::{
    form,
    native::{NativeEvent, ToastTone},
    popup::{PopupSink, Side, TooltipContent},
    theme::{self, Palette},
    time,
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::{JjLogCommit, JjLogResult},
  local_db::Workspace,
};

/// `useDebounce(mentionQuery, 150)`.
const MENTION_DEBOUNCE: Duration = Duration::from_millis(150);
/// `LinearCommitHistory`'s page size.
const HISTORY_PAGE: usize = 14;
const STACK_DOCS_PATH: &str = "/docs/concepts/workspaces#stacks-and-rebasing";

/// What the workspace header already read, which the tab draws from too.
#[derive(Debug, Clone)]
pub struct Parent {
  pub repo_path: String,
  /// `None` for the home repository.
  pub workspace: Option<Workspace>,
  /// The workspace status's conflicted files.
  pub conflicted_files: Vec<String>,
  /// `defaultTargetBranch`: the stack's base.
  pub default_branch: String,
  /// Bumped when React refreshes its caches, so the tab reads again.
  pub revision: u64,
  /// The repository is remote: local-only actions stay hidden.
  pub remote: bool,
}

impl PartialEq for Parent {
  fn eq(&self, other: &Self) -> bool {
    let json = |w: &Option<Workspace>| serde_json::to_value(w).ok();
    self.repo_path == other.repo_path
      && self.remote == other.remote
      && self.conflicted_files == other.conflicted_files
      && self.default_branch == other.default_branch
      && self.revision == other.revision
      && json(&self.workspace) == json(&other.workspace)
  }
}

/// Tells React what the tab changed.
fn tell(host: &dyn CodeHost, event: NativeEvent) {
  super::workspace_data::WorkspaceHost::notify(host, event);
}

impl Parent {
  pub(super) fn key(&self) -> (String, Option<i64>) {
    (
      self.repo_path.clone(),
      self.workspace.as_ref().map(|w| w.id),
    )
  }

  pub(super) fn workspace_id(&self) -> Option<i64> {
    self.workspace.as_ref().map(|w| w.id)
  }

  /// `workingDirectory`.
  pub(super) fn working_directory(&self) -> String {
    self
      .workspace
      .as_ref()
      .map_or_else(|| self.repo_path.clone(), full_path)
  }
}

/// A stack row with its lines changed.
#[derive(Debug, Clone)]
pub struct StackRow {
  pub workspace: Workspace,
  pub current: bool,
  pub stats: (u32, u32),
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

pub struct CodeView {
  host: Arc<dyn CodeHost>,
  react: ActionSink,
  pub(super) parent: Parent,
  dark: bool,
  tips: Tooltips,
  pub(super) overview: Option<Overview>,
  pub(super) history: Option<JjLogResult>,
  pub(super) history_limit: usize,
  history_loading: bool,
  pub(super) stack: Option<Vec<StackRow>>,
  pub(super) settings: Option<TaskSettings>,
  task: Option<Entity<TextareaState>>,
  pub(super) task_text: String,
  pub(super) focused: bool,
  pub(super) mention: Option<Mention>,
  pub(super) agent_override: Option<Agent>,
  pub(super) save_default: bool,
  show_save_default: bool,
  pub(super) submitting: bool,
  pub(super) menu: Option<Menu>,
  /// The description being edited, and its editor.
  pub(super) description_draft: Option<Entity<TextareaState>>,
  saving_description: bool,
  syncing_submodule: bool,
  starting_devcontainer: bool,
  pub(super) picker: Option<Entity<FilePicker>>,
  /// Text to put in the task input at its cursor on the next render.
  insert: Option<String>,
  _mention_search: Option<Task<()>>,
  _subscriptions: Vec<Subscription>,
}

impl HasTooltips for CodeView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl CodeView {
  pub fn new(
    parent: Parent,
    host: Arc<dyn CodeHost>,
    react: ActionSink,
    popups: PopupSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let mut this = Self {
      host,
      react,
      parent,
      dark,
      tips: Tooltips::new(popups, dark),
      overview: None,
      history: None,
      history_limit: HISTORY_PAGE,
      history_loading: false,
      stack: None,
      settings: None,
      task: None,
      task_text: String::new(),
      focused: false,
      mention: None,
      agent_override: None,
      save_default: false,
      show_save_default: false,
      submitting: false,
      menu: None,
      description_draft: None,
      saving_description: false,
      syncing_submodule: false,
      starting_devcontainer: false,
      picker: None,
      insert: None,
      _mention_search: None,
      _subscriptions: Vec::new(),
    };
    this.load(cx);
    this
  }

  pub fn set_parent(&mut self, parent: Parent, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
      cx.notify();
    }
    if self.parent == parent {
      return;
    }
    let moved = self.parent.key() != parent.key();
    let stale = moved || self.parent.revision != parent.revision;
    self.parent = parent;
    if moved {
      self.overview = None;
      self.history = None;
      self.history_limit = HISTORY_PAGE;
      self.stack = None;
      self.settings = None;
      self.agent_override = None;
      self.save_default = false;
      self.show_save_default = false;
      self.description_draft = None;
      self.menu = None;
    }
    if stale {
      self.load(cx);
    }
    cx.notify();
  }

  fn load(&mut self, cx: &mut Context<Self>) {
    self.load_overview(cx);
    self.load_history(cx);
    self.load_stack(cx);
    self.load_settings(cx);
  }

  /// Runs `read` off the main thread, then `apply`s it unless the tab moved
  /// to another workspace meanwhile.
  fn read<T: Send + 'static>(
    &self,
    read: impl FnOnce(&dyn CodeHost, &Parent) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let (host, parent) = (self.host.clone(), self.parent.clone());
    let key = parent.key();
    cx.spawn(async move |this, cx| {
      let value = cx
        .background_spawn(async move { read(host.as_ref(), &parent) })
        .await;
      let _ = this.update(cx, |view, cx| {
        if view.parent.key() == key {
          apply(view, value, cx);
          cx.notify();
        }
      });
    })
    .detach();
  }

  fn load_overview(&mut self, cx: &mut Context<Self>) {
    self.read(
      |host, parent| {
        Overview::or_unavailable(host.overview(&parent.repo_path, parent.workspace_id()))
      },
      |view, overview, _| view.overview = Some(overview),
      cx,
    );
  }

  fn load_history(&mut self, cx: &mut Context<Self>) {
    self.history_loading = true;
    let limit = self.history_limit;
    self.read(
      move |host, parent| {
        let home = parent.workspace.is_none();
        host
          .commits(
            &parent.repo_path,
            parent.workspace_id(),
            true,
            Some(if home { limit } else { HISTORY_PAGE }),
          )
          .map_err(|err| log::warn!("Failed to load commits: {err}"))
          .ok()
      },
      |view, history, _| {
        view.history_loading = false;
        if history.is_some() || view.history.is_none() {
          view.history = history;
        }
      },
      cx,
    );
  }

  fn load_stack(&mut self, cx: &mut Context<Self>) {
    if self.parent.workspace.is_none() {
      self.stack = None;
      return;
    }
    self.read(
      |host, parent| {
        let current = parent.workspace.as_ref()?;
        let workspaces = host.workspaces(&parent.repo_path).ok()?;
        let stack = workspace_stack(&workspaces, current.id, &parent.default_branch)?;
        Some(
          stack
            .into_iter()
            .map(|(workspace, current)| {
              let stats = host
                .commits(&parent.repo_path, Some(workspace.id), false, None)
                .map(|log| diff_stats(&log.commits))
                .unwrap_or_default();
              StackRow {
                workspace,
                current,
                stats,
              }
            })
            .collect(),
        )
      },
      |view, stack, _| view.stack = stack,
      cx,
    );
  }

  fn load_settings(&mut self, cx: &mut Context<Self>) {
    self.read(
      |host, parent| task_settings(host, &parent.repo_path),
      |view, settings, _| view.settings = Some(settings),
      cx,
    );
  }

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    tell(
      self.host.as_ref(),
      NativeEvent::Toast {
        title: title.into(),
        description: description.into(),
        tone,
        action: None,
      },
    );
  }

  fn send_react(&self, action: ScreenAction, cx: &mut App) {
    let react = self.react.clone();
    // Never inside this view's own update: React's sink may route back here.
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

  /// `configuredDefaultAgent` and `selectedAgent`.
  pub(super) fn agents(&self) -> (Agent, Agent) {
    let configured = self.settings.as_ref().map(|s| s.agent).unwrap_or_default();
    (configured, self.agent_override.unwrap_or(configured))
  }

  /// The home view's "Run in a new workspace", when offered and on.
  pub(super) fn run_in_new_workspace(&self) -> bool {
    self.parent.workspace.is_none() && self.settings.as_ref().is_some_and(|s| s.new_workspace)
  }

  fn task_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
    if let Some(task) = &self.task {
      return task.clone();
    }
    let task = cx.new(|cx| {
      TextareaState::new(window, cx)
        .placeholder("Describe a task...")
        .auto_grow(1, 9)
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
  pub(super) fn text_changed(&mut self, text: String, cursor: usize, cx: &mut Context<Self>) {
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
    let (host, parent) = (self.host.clone(), self.parent.clone());
    let path = parent.working_directory();
    self._mention_search = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(MENTION_DEBOUNCE).await;
      let search = query.clone();
      let results = cx
        .background_spawn(async move {
          host.search_files(&parent.repo_path, parent.workspace_id(), &path, &search, 4)
        })
        .await
        .unwrap_or_default();
      let _ = this.update(cx, |view, cx| {
        if let Some(mention) = view.mention.as_mut().filter(|m| m.query == query) {
          mention.results = results;
          mention.index = 0;
          cx.notify();
        }
      });
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
  pub(super) fn select_mention(
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
    let text = format!(
      "{}{inserted}{}",
      &self.task_text[..mention.anchor.min(self.task_text.len())],
      &self.task_text[cursor.min(self.task_text.len())..]
    );
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

  fn open_picker(&mut self, cx: &mut Context<Self>) {
    self.menu = None;
    let scope = Scope {
      repo_path: self.parent.repo_path.clone(),
      workspace_id: self.parent.workspace_id(),
      workspace_path: self.parent.working_directory(),
    };
    let picker = cx.new(|cx| FilePicker::new(self.host.clone(), scope, self.dark, cx));
    self._subscriptions.push(
      cx.subscribe(&picker, |view, _, event: &FilePickerEvent, cx| {
        if let FilePickerEvent::Selected(path) = event {
          view.insert = Some(format!("@{path} "));
        }
        view.picker = None;
        cx.notify();
      }),
    );
    self.picker = Some(picker);
    cx.notify();
  }

  fn attach(&mut self, cx: &mut Context<Self>) {
    self.menu = None;
    let host = self.host.clone();
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
      let _ = this.update(cx, |view, cx| {
        view.insert = Some(insertion);
        cx.notify();
      });
    })
    .detach();
  }

  fn choose_agent(&mut self, agent: Agent, cx: &mut Context<Self>) {
    let (configured, _) = self.agents();
    self.agent_override = Some(agent);
    self.save_default = false;
    self.show_save_default = agent != configured;
    self.menu = None;
    cx.notify();
  }

  fn set_new_workspace(&mut self, on: bool, cx: &mut Context<Self>) {
    let previous = self.settings.as_ref().map(|s| s.new_workspace);
    if let Some(settings) = self.settings.as_mut() {
      settings.new_workspace = on;
    }
    cx.notify();
    self.read(
      move |host, parent| {
        host.set_repo_settings(
          &parent.repo_path,
          &[(NEW_WORKSPACE_SETTING.into(), on.to_string())],
        )
      },
      move |view, result, _| match result {
        Ok(()) => tell(
          view.host.as_ref(),
          NativeEvent::RepoSettingsChanged {
            repo_path: view.parent.repo_path.clone(),
          },
        ),
        Err(err) => {
          if let (Some(settings), Some(previous)) = (view.settings.as_mut(), previous) {
            settings.new_workspace = previous;
          }
          view.toast(ToastTone::Error, "Failed to save setting", err);
        }
      },
      cx,
    );
  }

  fn set_environment(&mut self, devcontainer: bool, cx: &mut Context<Self>) {
    self.menu = None;
    let value = if devcontainer {
      "devcontainer"
    } else {
      "local"
    };
    self.read(
      move |host, parent| {
        host.set_repo_settings(
          &parent.repo_path,
          &[(ENVIRONMENT_SETTING.into(), value.into())],
        )
      },
      move |view, result, cx| {
        if let Err(err) = result {
          view.toast(ToastTone::Error, "Failed to save setting", err);
          return;
        }
        tell(
          view.host.as_ref(),
          NativeEvent::RepoSettingsChanged {
            repo_path: view.parent.repo_path.clone(),
          },
        );
        let Some(env) = view.settings.as_mut().and_then(|s| s.environment.as_mut()) else {
          return;
        };
        env.devcontainer = devcontainer;
        let stopped = env.status.as_ref().is_some_and(|s| !s.running);
        if devcontainer && stopped && !view.starting_devcontainer {
          view.start_devcontainer(cx);
        }
      },
      cx,
    );
  }

  fn start_devcontainer(&mut self, cx: &mut Context<Self>) {
    self.menu = None;
    self.starting_devcontainer = true;
    cx.notify();
    self.read(
      |host, parent| host.devcontainer_up(&parent.repo_path),
      |view, result, _| {
        view.starting_devcontainer = false;
        match result {
          Ok(status) => {
            if let Some(env) = view.settings.as_mut().and_then(|s| s.environment.as_mut()) {
              env.status = Some(status);
            }
          }
          Err(err) => view.toast(ToastTone::Error, "Devcontainer did not start", err),
        }
      },
      cx,
    );
  }

  /// `submitTask`: starts an agent session on the task, in a new workspace
  /// when the home view asks for one.
  pub(super) fn submit(&mut self, plan: bool, cx: &mut Context<Self>) {
    let prompt = self.task_text.trim().to_string();
    if prompt.is_empty() || self.submitting {
      return;
    }
    self.submitting = true;
    self.menu = None;
    cx.notify();
    let (configured, agent) = self.agents();
    let save_default = self.save_default && agent != configured;
    let new_workspace = self.run_in_new_workspace();
    let mode = if plan { "plan" } else { "acceptEdits" };
    let (host, parent) = (self.host.clone(), self.parent.clone());
    let key = parent.key();
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move {
          let repo = parent.repo_path.as_str();
          if save_default {
            host.set_repo_settings(repo, &[("default_agent".into(), agent.id().into())])?;
            tell(
              host.as_ref(),
              NativeEvent::RepoSettingsChanged {
                repo_path: repo.into(),
              },
            );
          }
          let name = session_name(&prompt);
          let mut workspace_id = parent.workspace_id();
          if new_workspace {
            let taken = host.branch_names(repo)?;
            let pattern = host
              .repo_settings(repo, &["branch_name_pattern".into()])
              .ok()
              .and_then(|v| v.get("branch_name_pattern").cloned().flatten())
              .filter(|p| !p.is_empty())
              .unwrap_or_else(|| "treq/{name}".into());
            let branch = task_branch_name(&prompt, &taken, &pattern);
            workspace_id = Some(host.create_workspace(repo, &branch, &name)?);
            tell(
              host.as_ref(),
              NativeEvent::WorkspaceChanged {
                repo_path: repo.into(),
              },
            );
          }
          let session_id = host.create_session(repo, workspace_id, &name)?;
          Ok::<_, String>(super::launch_data::started(host.as_ref(), json!({
            "sessionId": session_id,
            "sessionName": name,
            "workspaceId": workspace_id,
            "workspacePath": null,
            "repoPath": if repo.is_empty() { parent.working_directory() } else { repo.to_string() },
            "pendingPrompt": prompt,
            "permissionMode": mode,
            "agent": agent.id(),
          })))
        })
        .await;
      let _ = this.update_in(cx, |view, window, cx| {
        view.submitting = false;
        match result {
          Ok(session) => {
            if save_default {
              view.save_default = false;
              if let Some(settings) = view.settings.as_mut() {
                settings.agent = agent;
                settings.from_repo = true;
              }
            }
            let _ = session;
            if view.parent.key() == key {
              view.show_save_default = false;
              view.set_task_text(String::new(), 0, window, cx);
            }
          }
          Err(err) => view.toast(ToastTone::Error, "Failed to create task", err),
        }
        cx.notify();
      });
    })
    .detach();
  }

  fn edit_description(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
    let draft = cx.new(|cx| {
      TextareaState::new(window, cx)
        .auto_grow(1, 30)
        .default_value(text)
    });
    draft.update(cx, |d, cx| d.focus(window, cx));
    self.description_draft = Some(draft);
    cx.notify();
  }

  fn save_description(&mut self, cx: &mut Context<Self>) {
    let (Some(draft), Some(ws)) = (&self.description_draft, &self.parent.workspace) else {
      return;
    };
    let text = draft.read(cx).value().to_string();
    let id = ws.id;
    self.saving_description = true;
    self.read(
      move |host, parent| host.set_description(&parent.repo_path, id, &text),
      |view, result, _| {
        view.saving_description = false;
        match result {
          Ok(()) => {
            view.description_draft = None;
            tell(
              view.host.as_ref(),
              NativeEvent::WorkspaceChanged {
                repo_path: view.parent.repo_path.clone(),
              },
            );
          }
          Err(err) => view.toast(ToastTone::Error, "Failed to save description", err),
        }
      },
      cx,
    );
  }

  fn sync_submodule(&mut self, path: String, enabled: bool, cx: &mut Context<Self>) {
    self.syncing_submodule = true;
    self.read(
      move |host, parent| host.set_submodule_synced(&parent.repo_path, &path, enabled),
      |view, result, cx| {
        view.syncing_submodule = false;
        if let Err(err) = result {
          log::warn!("Failed to set submodule sync: {err}");
        }
        view.load_overview(cx);
      },
      cx,
    );
  }

  fn open_entry(&self, entry: &WorkspaceEntry, cx: &mut App) {
    self.field(
      "code.open",
      json!({ "name": entry.name, "isDirectory": entry.is_directory }),
      cx,
    );
  }

  fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
    self.menu = if self.menu == Some(menu) {
      None
    } else {
      Some(menu)
    };
    cx.notify();
  }
}

/// Tailwind's `animate-pulse`.
fn pulse(el: Div, id: &'static str) -> impl IntoElement {
  el.with_animation(
    id,
    Animation::new(Duration::from_secs(2)).repeat(),
    |el, delta| el.opacity(1. - 0.5 * (1. - (2. * delta - 1.).abs())),
  )
}

pub(super) fn tw(color: u32) -> Hsla {
  rgb(color).into()
}

pub(super) const BLUE_400: u32 = 0x60a5fa;
pub(super) const BLUE_500: u32 = 0x3b82f6;
const GREEN_400: u32 = 0x4ade80;
pub(super) const GREEN_500: u32 = 0x22c55e;
pub(super) const GREEN_600: u32 = 0x16a34a;
const RED_400: u32 = 0xf87171;
const RED_500: u32 = 0xef4444;
pub(super) const RED_600: u32 = 0xdc2626;
const YELLOW_500: u32 = 0xeab308;

/// `rounded-lg border` around a section.
fn section(p: &Palette) -> Div {
  div().rounded(zpx(12.)).border_1().border_color(p.border)
}

/// A dropdown's panel (`DropdownMenuContent`), drawn over the tab.
pub(super) fn menu_panel(id: &'static str, p: &Palette) -> Stateful<Div> {
  div()
    .id(id)
    .debug_selector(move || id.into())
    .absolute()
    .p_1()
    .rounded(zpx(8.))
    .border_1()
    .border_color(p.border)
    .bg(p.popover)
    .text_color(p.popover_foreground)
    .shadow_md()
    .occlude()
}

/// A `DropdownMenuItem`.
pub(super) fn menu_item(id: impl Into<SharedString>, p: &Palette, disabled: bool) -> Stateful<Div> {
  let id: SharedString = id.into();
  let selector = id.to_string();
  let hover = p.menu_accent;
  ui::text(div(), TextSize::Sm)
    .id(id)
    .debug_selector(move || selector)
    .flex()
    .items_center()
    .gap_2()
    .px_2()
    .py(zpx(6.))
    .rounded(zpx(6.))
    .when(disabled, |this| this.opacity(0.5))
    .when(!disabled, |this| {
      this.cursor_pointer().hover(move |s| s.bg(hover))
    })
}

/// A native `<input type="checkbox">`, 14px, in `accent`.
pub(super) fn checkbox(
  id: impl Into<SharedString>,
  checked: bool,
  accent: Hsla,
  p: &Palette,
) -> Stateful<Div> {
  let id: SharedString = id.into();
  let selector = id.to_string();
  div()
    .id(id)
    .debug_selector(move || selector)
    .size(zpx(13.))
    .flex_none()
    .flex()
    .items_center()
    .justify_center()
    .rounded(zpx(3.))
    .when(checked, |this| {
      this
        .bg(accent)
        .child(ui::icon(theme::ICON_CHECK, 11., gpui::white()))
    })
    .when(!checked, |this| {
      this
        .border_1()
        .border_color(p.muted_foreground)
        .bg(p.background)
    })
}

/// `ui/kbd.tsx`.
pub(super) fn kbd(p: &Palette, text: &str) -> Div {
  ui::text(div(), TextSize::Xs)
    .h(zpx(20.))
    .min_w(zpx(20.))
    .px_1()
    .flex()
    .items_center()
    .justify_center()
    .rounded(zpx(6.))
    .bg(p.muted)
    .text_color(p.muted_foreground)
    .font_weight(FontWeight::MEDIUM)
    .child(text.to_string())
}

pub(super) fn agent_icon(agent: Agent) -> &'static str {
  match agent {
    Agent::Claude => theme::ICON_AGENT_CLAUDE,
    Agent::Codex => theme::ICON_AGENT_CODEX,
    Agent::Cursor => theme::ICON_AGENT_CURSOR,
    Agent::Copilot => theme::ICON_AGENT_COPILOT,
  }
}

/// `MarkdownContent`'s prose, at `size`/`line` (16/28 for `prose`, 14/24
/// for `prose-sm`).
pub(super) fn markdown(
  id: impl Into<SharedString>,
  text: &str,
  size: f32,
  line: f32,
  dark: bool,
) -> impl IntoElement {
  let id: SharedString = id.into();
  let style = TextViewStyle {
    is_dark: dark,
    ..TextViewStyle::default()
  }
  .paragraph_gap(rems(1.25 * size / 16.))
  .heading_font_size(|level, base| match level {
    1 => px(30.),
    2 => px(20.),
    3 => px(17.),
    _ => base,
  });
  // HTML comments never show, as in `MarkdownContent`.
  let text = regex::Regex::new(r"(?s)<!--.*?-->")
    .expect("regex")
    .replace_all(text, "")
    .to_string();
  div()
    .text_size(zpx(size))
    .line_height(zpx(line))
    .child(TextView::markdown(id, text).style(style).selectable(true))
}

impl Render for CodeView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    let p = theme::palette(self.dark);
    if let Some(insertion) = self.insert.take() {
      self.insert_text(&insertion, window, cx);
    }
    let left = div()
      .id("code-scroll")
      .flex_1()
      .min_w_0()
      .h_full()
      .overflow_y_scroll()
      .border_r_1()
      .border_color(p.border)
      .child(
        div()
          .p_4()
          .flex()
          .flex_col()
          .gap_4()
          .when(self.overview.is_none(), |this| this.child(skeleton(&p)))
          .children(self.conflicts(&p))
          .child(self.task_card(&p, window, cx))
          .children(self.description(&p, window, cx))
          .children(self.stack_panel(&p, cx))
          .child(self.go_to_file(&p))
          .child(self.files(&p, cx))
          .child(self.readme(&p)),
      );
    div()
      .debug_selector(|| "workspace-code".into())
      .size_full()
      .flex()
      .bg(p.background)
      .text_color(p.foreground)
      .child(left)
      .child(
        div()
          .w(zpx(240.))
          .flex_none()
          .h_full()
          .bg(p.muted.opacity(0.2))
          .child(self.history(&p, cx)),
      )
      .children(self.picker.clone())
  }
}

fn skeleton(p: &Palette) -> impl IntoElement {
  let bar = |h: f32| div().h(zpx(h)).rounded(zpx(4.)).bg(p.muted.opacity(0.4));
  div()
    .debug_selector(|| "workspace-overview-skeleton".into())
    .flex()
    .flex_col()
    .gap_4()
    .child(pulse(
      div().h(zpx(40.)).rounded(zpx(12.)).bg(p.muted.opacity(0.5)),
      "sk-1",
    ))
    .child(pulse(
      div().h(zpx(40.)).rounded(zpx(12.)).bg(p.muted.opacity(0.5)),
      "sk-2",
    ))
    .child(
      section(p)
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(pulse(
          div()
            .h(zpx(16.))
            .w(zpx(128.))
            .rounded(zpx(4.))
            .bg(p.muted.opacity(0.5)),
          "sk-3",
        ))
        .child(pulse(bar(12.), "sk-4"))
        .child(pulse(bar(12.).w(relative(11. / 12.)), "sk-5"))
        .child(pulse(bar(12.).w(relative(10. / 12.)), "sk-6")),
    )
}

impl CodeView {
  fn conflicts(&self, p: &Palette) -> Option<impl IntoElement> {
    let count = conflict_count(&self.parent.conflicted_files);
    if count == 0 {
      return None;
    }
    let red = p.destructive;
    let react = self.react.clone();
    Some(
      div()
        .debug_selector(|| "code-conflicts".into())
        .rounded(zpx(8.))
        .border_1()
        .border_color(red.opacity(0.3))
        .bg(red.opacity(0.05))
        .p_4()
        .flex()
        .items_start()
        .gap_3()
        .child(
          div()
            .mt(zpx(2.))
            .child(ui::icon(theme::ICON_TRIANGLE_ALERT, 20., red)),
        )
        .child(
          div()
            .flex_1()
            .child(
              ui::text(div(), TextSize::Sm)
                .font_weight(FontWeight::MEDIUM)
                .text_color(red)
                .child(format!(
                  "{count} {} detected",
                  if count == 1 { "conflict" } else { "conflicts" }
                )),
            )
            .child(
              ui::text(div(), TextSize::Sm)
                .mt_1()
                .text_color(p.muted_foreground)
                .child("Some files have conflicts that need to be resolved"),
            ),
        )
        .child(
          Button::new("code-view-conflicts", p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .border_color(red.opacity(0.3))
            .text_color(red)
            .hover_bg(red.opacity(0.1))
            .label("View conflicts")
            .on_click(move |_, _, cx| {
              let react = react.clone();
              cx.defer(move |cx| {
                react(
                  ScreenAction::Field {
                    field: "workspace.tab".into(),
                    value: "changes".into(),
                  },
                  cx,
                )
              });
            }),
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
    let border = if self.focused { tw(BLUE_400) } else { p.border };
    let mention = self.mention.clone().filter(|m| !m.results.is_empty());
    div()
      .debug_selector(|| "agent-task-input".into())
      .w_full()
      .max_w(zpx(672.))
      .mx_auto()
      .child(
        div()
          .id("task-card")
          .min_h(zpx(112.))
          .rounded(zpx(16.))
          .border_1()
          .border_color(border)
          .bg(p.background)
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
          .child(
            ui::text(div(), TextSize::Sm)
              .debug_selector(|| "task-input".into())
              // The textarea's own 4px inset makes up React's `px-4`.
              .px_3()
              .pt_3()
              .pb_1()
              .min_h(zpx(44.))
              .max_h(zpx(200.))
              .child(
                Textarea::new(&task)
                  .appearance(false)
                  .bordered(false)
                  .xsmall()
                  .text_size(zpx(13.))
                  .line_height(zpx(20.)),
              ),
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
      .rounded(zpx(12.))
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
          .py(zpx(6.))
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
    let empty = self.task_text.trim().is_empty();
    let plan = agent.has_plan_mode();
    let new_workspace = (self.parent.workspace.is_none())
      .then(|| self.settings.as_ref().map(|s| s.new_workspace))
      .flatten();
    let small = |b: Button| b.size(Size::Sm).h(zpx(28.));

    let add = div()
      .relative()
      .child(
        small(Button::new("task-add", p).variant(Variant::Ghost))
          .px_2()
          .gap_1()
          .child(
            ui::text(div(), TextSize::Xs)
              .flex()
              .items_center()
              .gap_1()
              .child(ui::icon(theme::ICON_PLUS, 16., p.foreground))
              .child("Add")
              .child(ui::icon(theme::ICON_CHEVRON_DOWN, 14., p.foreground)),
          )
          .on_click(cx.listener(|view, _, _, cx| view.toggle_menu(Menu::Add, cx))),
      )
      .when(self.menu == Some(Menu::Add), |this| {
        this.child(self.add_menu(p, cx))
      });

    let mut left = div().flex().items_center().gap_1().child(add);
    if let Some(on) = new_workspace {
      left = left.child(
        div()
          .ml_1()
          .flex()
          .items_center()
          .gap(zpx(6.))
          .child(
            form::switch_sized(
              "task-new-workspace",
              on,
              false,
              p,
              (28., 16., 12.),
              cx.listener(move |view, checked: &bool, _, cx| view.set_new_workspace(*checked, cx)),
            )
            .aria_label("Run in a new workspace"),
          )
          .child(
            ui::text(div(), TextSize::Xs)
              .id("task-new-workspace-label")
              .cursor_pointer()
              .text_color(p.muted_foreground)
              .child("Run in a new workspace")
              .on_click(cx.listener(move |view, _, _, cx| view.set_new_workspace(!on, cx))),
          ),
      );
    }

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
          .flex()
          .items_center()
          .gap(zpx(6.))
          .cursor_pointer()
          .text_color(p.muted_foreground)
          .child(checkbox("task-save-default-box", checked, tw(BLUE_500), p))
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
        .gap(zpx(6.))
        .child(ui::icon(agent_icon(agent), 14., p.muted_foreground))
        .child(
          ui::text(div(), TextSize::Xs)
            .id("task-agent")
            .debug_selector(|| "task-agent".into())
            .h(zpx(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .rounded(zpx(8.))
            .border_1()
            .border_color(p.border)
            .bg(p.background)
            .cursor_pointer()
            .child(agent.label())
            .child(ui::icon(theme::ICON_CHEVRON_DOWN, 12., p.muted_foreground))
            .on_click(cx.listener(|view, _, _, cx| view.toggle_menu(Menu::Agent, cx))),
        )
        .when(self.menu == Some(Menu::Agent), |this| {
          this.child(self.agent_menu(agent, p, cx))
        }),
    );

    let disabled = empty || self.submitting;
    let label = if self.submitting {
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
      .when(self.submitting, |b| {
        b.child(ui::spinner("task-starting", 14., p.primary_foreground))
      })
      .aria_label(label)
      .child(ui::text(div(), TextSize::Xs).child(label))
      .on_click(cx.listener(|view, _, _, cx| view.submit(false, cx)));
    let submit = self.tips.wrap(
      "task-submit",
      Side::Top,
      Some(TooltipContent::Text(
        crate::gpui_shell::keys::hint("⌘+Enter").into(),
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
            .border_color(gpui::white().opacity(0.2))
            .px(zpx(6.))
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

  fn close_menu_on_outside(&self, panel: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
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
        menu_item("task-add-file", p, false)
          .child(ui::icon(theme::ICON_PLUS, 16., p.popover_foreground))
          .child("File")
          .on_click(cx.listener(|view, _, _, cx| view.open_picker(cx))),
      )
      .child(
        menu_item("task-add-attach", p, false)
          .child(ui::icon(theme::ICON_PAPERCLIP, 16., p.popover_foreground))
          .child("Attach")
          .on_click(cx.listener(|view, _, _, cx| view.attach(cx))),
      );
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(1)
  }

  fn plan_menu(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let panel = menu_panel("task-plan-menu", p)
      .top(zpx(32.))
      .right_0()
      .w(zpx(224.))
      .child(
        menu_item("task-plan", p, false)
          .items_start()
          .child(
            div()
              .flex_1()
              .min_w_0()
              .flex()
              .flex_col()
              .gap(zpx(2.))
              .child(div().font_weight(FontWeight::MEDIUM).child("Plan"))
              .child(
                ui::text(div(), TextSize::Xs)
                  .text_color(p.muted_foreground)
                  .child("Plan the change first, without editing files"),
              ),
          )
          .on_click(cx.listener(|view, _, _, cx| view.submit(true, cx))),
      );
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(1)
  }

  fn agent_menu(&self, selected: Agent, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let panel = menu_panel("task-agent-menu", p)
      .top(zpx(32.))
      .right_0()
      .min_w(zpx(128.))
      .children(Agent::ALL.into_iter().map(|agent| {
        menu_item(format!("task-agent-{}", agent.id()), p, false)
          .child(ui::icon(agent_icon(agent), 14., p.muted_foreground))
          .child(div().flex_1().child(agent.label()))
          .when(agent == selected, |this| {
            this.child(ui::icon(theme::ICON_CHECK, 14., p.popover_foreground))
          })
          .on_click(cx.listener(move |view, _, _, cx| view.choose_agent(agent, cx)))
      }));
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(1)
  }

  fn environment(
    &self,
    env: &code_data::Environment,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let button = environment::trigger(
      "task-environment",
      env,
      self.starting_devcontainer,
      environment::TASK_TRIGGER,
      p,
    )
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
    env: &code_data::Environment,
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
    deferred(self.close_menu_on_outside(panel, cx)).with_priority(1)
  }

  fn description(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    let text = self.parent.workspace.as_ref().and_then(description)?;
    let card = section(p)
      .debug_selector(|| "workspace-description".into())
      .relative()
      .px_4()
      .py_3();
    let Some(draft) = self.description_draft.clone() else {
      let shown = if looks_like_markdown(&text) {
        div()
          .pr_8()
          .child(markdown(
            "workspace-description-md",
            &text,
            14.,
            24.,
            self.dark,
          ))
          .into_any_element()
      } else {
        ui::text(div(), TextSize::Sm)
          .pr_8()
          .whitespace_normal()
          .child(text.clone())
          .into_any_element()
      };
      return Some(
        card
          .child(
            div().absolute().top_2().right_2().child(
              Button::new("workspace-description-edit", p)
                .variant(Variant::Ghost)
                .size(Size::Icon)
                .w(zpx(28.))
                .h(zpx(28.))
                .child(ui::icon(theme::ICON_PENCIL, 16., p.muted_foreground))
                .on_click(cx.listener(move |view, _, window, cx| {
                  view.edit_description(text.clone(), window, cx)
                })),
            ),
          )
          .child(shown)
          .into_any_element(),
      );
    };
    let _ = window;
    let saving = self.saving_description;
    Some(
      card
        .child(
          div()
            .id("workspace-description-editor")
            .flex()
            .flex_col()
            .gap_2()
            .capture_action(cx.listener(|view, action: &Enter, _, cx| {
              if action.secondary {
                view.save_description(cx);
                cx.stop_propagation();
              }
            }))
            .capture_action(cx.listener(|view, _: &Escape, _, cx| {
              view.description_draft = None;
              cx.notify();
              cx.stop_propagation();
            }))
            .child(
              ui::text(div(), TextSize::Sm)
                .debug_selector(|| "workspace-description-input".into())
                .child(Textarea::new(&draft)),
            )
            .child(
              div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                  Button::new("workspace-description-cancel", p)
                    .variant(Variant::Ghost)
                    .size(Size::Sm)
                    .disabled(saving)
                    .label("Cancel")
                    .on_click(cx.listener(|view, _, _, cx| {
                      view.description_draft = None;
                      cx.notify();
                    })),
                )
                .child(
                  Button::new("workspace-description-save", p)
                    .size(Size::Sm)
                    .disabled(saving)
                    .label("Save")
                    .on_click(cx.listener(|view, _, _, cx| view.save_description(cx))),
                ),
            ),
        )
        .into_any_element(),
    )
  }

  fn stack_panel(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let stack = self.stack.clone()?;
    let current = stack.iter().position(|row| row.current).unwrap_or(0);
    let max = stack
      .iter()
      .map(|r| r.stats.0.max(r.stats.1))
      .max()
      .unwrap_or(0);
    let digits = |n: u32| n.to_string().len() + 1;
    let plus_ch = digits(stack.iter().map(|r| r.stats.0).max().unwrap_or(0));
    let minus_ch = digits(stack.iter().map(|r| r.stats.1).max().unwrap_or(0));
    let help = self.tips.wrap(
      "stack-help",
      Side::Top,
      Some(TooltipContent::Text(
        "A stack is a chain of workspaces that build on each other. Each targets the workspace \
         below it instead of the default branch. Click a workspace to navigate to it."
          .into(),
      )),
      div()
        .id("stack-help")
        .cursor_pointer()
        .child(ui::icon(
          theme::ICON_CIRCLE_HELP,
          14.,
          p.muted_foreground.opacity(0.7),
        ))
        .on_click(cx.listener(|view, _, _, _| {
          let url = format!("{}{STACK_DOCS_PATH}", crate::gpui_shell::native::web_url());
          view.host.open_url(&url);
        })),
      cx,
    );
    let header = ui::text(div(), TextSize::Sm)
      .flex()
      .items_center()
      .gap(zpx(6.))
      .font_weight(FontWeight::SEMIBOLD)
      .text_color(p.muted_foreground)
      .mb_4()
      .child(ui::icon(theme::ICON_LAYERS, 16., p.muted_foreground))
      .child("Stack")
      .child(help)
      .child(
        ui::text(div(), TextSize::Xs)
          .ml_auto()
          .font_weight(FontWeight::NORMAL)
          .child(format!("{} of {}", current + 1, stack.len())),
      );
    let base = div()
      .relative()
      .flex()
      .items_start()
      .gap_3()
      .py_2()
      .px_2()
      .mx_neg_2()
      .text_color(p.muted_foreground)
      .child(
        div()
          .flex_none()
          .mt(zpx(2.))
          .size(zpx(14.))
          .flex()
          .items_center()
          .justify_center()
          .child(ui::icon(theme::ICON_ARROW_DOWN, 14., p.muted_foreground)),
      )
      .child(
        ui::text(div(), TextSize::Sm)
          .flex_1()
          .min_w_0()
          .font_family(theme::MONO_FONT)
          .truncate()
          .child(self.parent.default_branch.clone()),
      );
    let rows: Vec<_> = stack
      .into_iter()
      .map(|row| self.stack_row(row, max, (plus_ch, minus_ch), p, cx))
      .collect();
    Some(
      section(p)
        .debug_selector(|| "workspace-stack-panel".into())
        .p_4()
        .child(header)
        .child(
          div()
            .relative()
            .child(
              div()
                .absolute()
                .left(zpx(7.))
                .top_2()
                .bottom_2()
                .w(zpx(2.))
                .bg(p.border),
            )
            .children(rows)
            .child(base),
        ),
    )
  }

  fn stack_row(
    &mut self,
    row: StackRow,
    max: u32,
    (plus_ch, minus_ch): (usize, usize),
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = SharedString::from(format!("workspace-stack-item-{}", row.workspace.id));
    let selector = id.to_string();
    let title = display_title(&row.workspace);
    let created = row.workspace.created_at.clone();
    let time = self.tips.wrap(
      format!("stack-time-{}", row.workspace.id),
      Side::Top,
      Some(TooltipContent::Text(time::full(&created).into())),
      ui::text(div(), TextSize::Xs)
        .text_color(p.muted_foreground)
        .child(time::relative(&created)),
      cx,
    );
    let (ins, del) = row.stats;
    let workspace = row.workspace.clone();
    let muted = p.muted;
    div()
      .id(id)
      .debug_selector(move || selector)
      .relative()
      .flex()
      .w_full()
      .items_start()
      .gap_3()
      .py_2()
      .px_2()
      .mx_neg_2()
      .rounded(zpx(8.))
      .cursor_pointer()
      .when(row.current, |this| {
        this
          // Opaque, or the shadow below shows through.
          .bg(p.background.blend(p.primary.opacity(0.1)))
          .border_1()
          .border_color(p.primary.opacity(0.4))
          .shadow_sm()
      })
      .when(!row.current, |this| this.hover(move |s| s.bg(muted)))
      .child(stack_dot(row.current, p))
      .child(
        div()
          .flex_1()
          .min_w_0()
          .child(ui::text(div(), TextSize::Sm).truncate().child(title))
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .mt(zpx(2.))
              .flex_wrap()
              .child(time)
              .when(ins > 0 || del > 0, |this| {
                this.child(div().ml_auto().child(loc_indicator(
                  ins,
                  del,
                  max,
                  (plus_ch, minus_ch),
                  self.dark,
                  p,
                )))
              }),
          ),
      )
      .on_click(cx.listener(move |view, _, _, cx| {
        let value = serde_json::to_value(&workspace).unwrap_or_default();
        view.field("code.stack", value, cx);
      }))
  }

  fn go_to_file(&self, p: &Palette) -> impl IntoElement {
    let react = self.react.clone();
    let hover = p.muted.opacity(0.3);
    div().flex().justify_end().child(
      div()
        .id("code-go-to-file")
        .debug_selector(|| "code-go-to-file".into())
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .border_1()
        .border_color(p.border)
        .rounded(zpx(12.))
        .bg(p.background)
        .hover(move |s| s.bg(hover))
        .cursor_pointer()
        .w_full()
        .max_w(zpx(320.))
        .child(ui::icon(theme::ICON_SEARCH, 16., p.muted_foreground))
        .child(
          ui::text(div(), TextSize::Sm)
            .flex_1()
            .text_color(p.muted_foreground)
            .child("Go to file"),
        )
        .child(kbd(p, &crate::gpui_shell::keys::hint("⌘ + P")))
        .on_click(move |_, _, cx| {
          let react = react.clone();
          cx.defer(move |cx| {
            react(
              ScreenAction::Press {
                id: "code.filePicker".into(),
              },
              cx,
            )
          });
        }),
    )
  }

  fn files(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let entries = self
      .overview
      .as_ref()
      .map(|o| o.entries.clone())
      .unwrap_or_default();
    let font = self.settings.as_ref().map_or(14., |s| s.font_size);
    let home = self.parent.workspace.is_none();
    let hover = p.muted.opacity(0.6);
    let muted_fg = p.muted_foreground;
    let mut list = section(p).flex().flex_col();
    let loaded = self.overview.is_some();
    let empty = entries.is_empty();
    let unavailable = self.overview.as_ref().is_some_and(|o| o.unavailable);
    for (i, entry) in entries.into_iter().enumerate() {
      let pin = entry.submodule_pin.clone();
      let submodule = pin.is_some();
      let icon = match (&pin, entry.is_directory) {
        (Some(_), _) => (theme::ICON_FOLDER_GIT, tw(BLUE_500)),
        (None, true) => (theme::ICON_FOLDER, tw(BLUE_500)),
        (None, false) => (theme::ICON_FILE, p.muted_foreground),
      };
      let pip = match entry.status.as_deref() {
        Some("conflict") => Some(tw(RED_500)),
        Some("committed") => Some(tw(BLUE_500)),
        Some("workingCopy") => Some(tw(YELLOW_500)),
        _ => None,
      };
      let name_color = if entry.status.is_some() {
        p.foreground
      } else {
        p.muted_foreground.opacity(0.7)
      };
      let id = SharedString::from(format!("code-entry-{}", entry.name));
      let selector = if submodule {
        format!("submodule-row-{}", entry.name)
      } else {
        id.to_string()
      };
      let open = entry.clone();
      let synced = entry.submodule_synced.unwrap_or(false);
      let path = entry.name.clone();
      let syncing = self.syncing_submodule;
      list =
        list.child(
          div()
            .debug_selector(move || selector)
            .when(i > 0, |this| this.border_t_1().border_color(p.border))
            .flex()
            .items_center()
            .hover(move |s| s.bg(hover))
            .child(
              ui::text(div(), TextSize::Sm)
                .id(id)
                .role(Role::Button)
                .aria_label(entry.name.clone())
                .test_support()
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_1()
                .flex_1()
                .min_w_0()
                .cursor_pointer()
                .child(ui::icon(icon.0, 16., icon.1))
                .child(
                  div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .font_family(theme::MONO_FONT)
                    .text_size(zpx(font))
                    .line_height(zpx((font * 1.5).round()))
                    .child(
                      div()
                        .min_w_0()
                        .truncate()
                        .text_color(name_color)
                        .child(entry.name.clone()),
                    )
                    .when_some(pin, |this, pin| {
                      this.child(
                        div()
                          .flex_none()
                          .text_color(muted_fg)
                          .child(format!("\u{a0}@ {}", &pin[..pin.len().min(7)])),
                      )
                    }),
                )
                .children(pip.map(|c| div().size_2().flex_none().rounded_full().bg(c)))
                .on_click(cx.listener(move |view, _, _, cx| view.open_entry(&open, cx))),
            )
            .when(home && submodule, |this| {
              this.child(
                ui::text(div(), TextSize::Xs)
                  .id(SharedString::from(format!("submodule-sync-{path}")))
                  .flex()
                  .items_center()
                  .gap(zpx(6.))
                  .flex_none()
                  .pr_4()
                  .pl_2()
                  .text_color(p.muted_foreground)
                  .cursor_pointer()
                  .when(syncing, |this| this.opacity(0.5))
                  .child(checkbox(
                    format!("submodule-sync-box-{path}"),
                    synced,
                    p.primary,
                    p,
                  ))
                  .child("Sync")
                  .when(!syncing, |this| {
                    let path = path.clone();
                    this.on_click(cx.listener(move |view, _, _, cx| {
                      view.sync_submodule(path.clone(), !synced, cx)
                    }))
                  }),
              )
            }),
        );
    }
    if loaded && empty {
      list = list.child(
        ui::text(div(), TextSize::Sm)
          .debug_selector(move || {
            if unavailable {
              "code-files-unavailable"
            } else {
              "code-files-empty"
            }
            .into()
          })
          .px_4()
          .py_8()
          .text_center()
          .text_color(p.muted_foreground)
          .child(if unavailable {
            REMOTE_FILE_TREE_REASON
          } else {
            "No files found"
          }),
      );
    }
    list
  }

  fn readme(&self, p: &Palette) -> impl IntoElement {
    let readme = self.overview.as_ref().and_then(|o| o.readme.clone());
    section(p)
      .debug_selector(|| "code-readme".into())
      .p_6()
      .child(match readme {
        Some(text) => div()
          .child(
            ui::text(div(), TextSize::Lg)
              .font_weight(FontWeight::SEMIBOLD)
              .mb_4()
              .child("README.md"),
          )
          .child(markdown("code-readme-md", &text, 16., 28., self.dark))
          .into_any_element(),
        None => ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .text_center()
          .py_4()
          .child("No README.md found")
          .into_any_element(),
      })
  }

  fn history(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let Some(log) = self.history.clone() else {
      return div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child("Loading commits..."),
        )
        .into_any_element();
    };
    let heading = ui::text(div(), TextSize::Sm)
      .font_weight(FontWeight::SEMIBOLD)
      .text_color(p.muted_foreground)
      .mb_4()
      .child("Commits");
    let (own, target) = history_commits(&log);
    let home = self.parent.workspace.is_none();
    let default_workspace =
      !home && !log.workspace_branch.is_empty() && log.workspace_branch == log.target_branch;
    let target_section = !home && !default_workspace && !target.is_empty();
    let diverged = home && !target.is_empty();
    let tentative = log.tentative_working_copy.clone();
    if own.is_empty() && !diverged && !target_section && tentative.is_none() {
      return div()
        .p_4()
        .child(heading)
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .text_center()
            .child("No commits yet."),
        )
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .text_center()
            .child("Changes you commit will appear here."),
        )
        .into_any_element();
    }

    let mut track = div().relative().child(
      div()
        .absolute()
        .left(zpx(7.))
        .top_2()
        .bottom_2()
        .w(zpx(2.))
        .bg(p.border),
    );
    if let Some(wc) = &tentative {
      track = track.child(div().mb_3().child(self.commit_item(
        &wc.commit,
        Kind::Tentative(wc.workspace_label.clone()),
        true,
        p,
        cx,
      )));
    }
    let mut first = true;
    for (label, commits) in group_by_day(&own) {
      // `first:mt-0` never applies (the track line is the first child);
      // block layout collapses the margin as Chromium does.
      let mut group = div().mt_5();
      group = group.child(day_label(&label, p));
      for commit in &commits {
        group = group.child(self.commit_item(commit, Kind::Own, first, p, cx));
        first = false;
      }
      track = track.child(group);
    }
    if home && own.len() + 1 >= self.history_limit {
      let loading = self.history_loading && self.history_limit > HISTORY_PAGE;
      track = track.child(
        ui::text(div(), TextSize::Xs)
          .id("history-load-more")
          .debug_selector(|| "history-load-more".into())
          .mt_3()
          .pl_7()
          .text_color(p.muted_foreground)
          .cursor_pointer()
          .when(loading, |this| {
            this
              .flex()
              .items_center()
              .gap(zpx(6.))
              .child(ui::spinner("history-loading", 12., p.muted_foreground))
              .child("Loading...")
          })
          .when(!loading, |this| {
            this
              .child("Load more commits")
              .on_click(cx.listener(|view, _, _, cx| {
                view.history_limit += HISTORY_PAGE;
                view.load_history(cx);
                cx.notify();
              }))
          }),
      );
    }
    if diverged || target_section {
      if target_section && own.is_empty() {
        track = track.child(
          ui::text(div(), TextSize::Sm)
            .pl_7()
            .mb_3()
            .text_color(p.muted_foreground)
            .child("There are no commits within this workspace branch yet."),
        );
      }
      let label = if diverged {
        div()
          .flex()
          .child("Diverged from\u{a0}")
          .child(
            div()
              .font_family(theme::MONO_FONT)
              .child(log.target_branch.clone()),
          )
          .when_some(log.merge_base_id.clone(), |this, base| {
            this.child(
              div()
                .ml_1()
                .font_family(theme::MONO_FONT)
                .opacity(0.6)
                .child(format!("@ {base}")),
            )
          })
      } else {
        div().child(format!("Recent on {}", log.target_branch))
      };
      track = track.child(separator(label, p));
      for (label, commits) in group_by_day(&target) {
        let mut group = div().mt_5().child(day_label(&label, p));
        for commit in &commits {
          group = group.child(self.commit_item(commit, Kind::Target, false, p, cx));
        }
        track = track.child(group);
      }
    }
    div()
      .id("history-scroll")
      .size_full()
      .overflow_y_scroll()
      .child(div().p_4().child(heading).child(track))
      .into_any_element()
  }

  fn commit_item(
    &mut self,
    commit: &JjLogCommit,
    kind: Kind,
    first: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let tentative = matches!(kind, Kind::Tentative(_));
    let target = matches!(kind, Kind::Target);
    let title = headline(commit, tentative);
    let (yellow_bg, yellow_hover) = if self.dark {
      (tw(0x422006).opacity(0.2), tw(0x422006).opacity(0.3))
    } else {
      (tw(0xfefce8).opacity(0.8), tw(0xfef9c3).opacity(0.8))
    };
    let yellow_border = if self.dark {
      tw(0x713f12).opacity(0.6)
    } else {
      tw(0xfef08a).opacity(0.7)
    };
    let dot = if tentative {
      tw(YELLOW_500)
    } else if target {
      p.muted_foreground.opacity(0.5)
    } else if first {
      p.primary
    } else {
      p.muted_foreground
    };
    let ts = commit.timestamp.clone();
    let time = self.tips.wrap(
      format!("commit-time-{}", commit.commit_id),
      Side::Top,
      Some(TooltipContent::Text(time::full(&ts).into())),
      ui::text(div(), TextSize::Xs)
        .text_color(p.muted_foreground)
        .child(time::relative(&ts)),
      cx,
    );
    let (green, red) = (tw(GREEN_600), tw(RED_600));
    let change = commit.change_id.clone();
    let id = SharedString::from(format!("commit-item-{}", commit.commit_id));
    let selector = id.to_string();
    let column = p.background.blend(p.muted.opacity(0.2));
    // The row's hover fill, and the card's `hover:bg-muted/40` over it. GPUI
    // paints a shadow under the whole box, so a translucent card shows the
    // shadow through: blend both down to opaque.
    let row_hover = if tentative { yellow_hover } else { p.muted };
    let card_hover = column.blend(row_hover).blend(p.muted.opacity(0.4));
    let body = div()
      .id(id)
      .debug_selector(move || selector)
      .flex_1()
      .min_w_0()
      .pt(zpx(2.))
      .rounded(zpx(8.))
      .cursor_pointer()
      .when(first && !target && !tentative, |this| {
        // Opaque over the column's `bg-muted/20`, or the shadow shows through.
        this
          .bg(column.blend(p.accent.opacity(0.5)))
          .p_2()
          .m_neg_2()
          .shadow_sm()
          .border_1()
          .border_color(p.accent)
      })
      .when(tentative, |this| {
        this.bg(column.blend(yellow_bg)).p_2().m_neg_2().shadow_sm()
      })
      .when(target, |this| this.bg(p.muted.opacity(0.3)).p_2().m_neg_2())
      .hover(move |s| s.bg(card_hover))
      .child(ui::text(div(), TextSize::Sm).truncate().child(title))
      .when_some(
        match &kind {
          Kind::Tentative(label) if !label.is_empty() => Some(label.clone()),
          _ => None,
        },
        |this, label| {
          this.child(
            ui::text(div(), TextSize::Sm)
              .mt(zpx(2.))
              .font_family(theme::MONO_FONT)
              .text_color(p.muted_foreground)
              .truncate()
              .child(label),
          )
        },
      )
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .mt(zpx(2.))
          .flex_wrap()
          .when(commit.is_immutable, |this| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .px(zpx(6.))
                .py(zpx(2.))
                .rounded_full()
                .bg(p.muted)
                .text_color(p.muted_foreground)
                .border_1()
                .border_color(p.border)
                .font_weight(FontWeight::MEDIUM)
                .child("Immutable"),
            )
          })
          .child(time)
          .when(commit.insertions > 0 || commit.deletions > 0, |this| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .ml_auto()
                .flex()
                .child(
                  div()
                    .text_color(green)
                    .child(format!("+{}", commit.insertions)),
                )
                .child("\u{a0}")
                .child(
                  div()
                    .text_color(red)
                    .child(format!("-{}", commit.deletions)),
                ),
            )
          }),
      )
      .on_click(cx.listener(move |view, _, _, cx| {
        view.field("code.commit", change.clone().into(), cx);
      }));
    div()
      .relative()
      .flex()
      .items_start()
      .gap_3()
      .py_2()
      .px_2()
      .mx_neg_2()
      .rounded(zpx(8.))
      .hover(move |s| s.bg(row_hover))
      .when(tentative, |this| {
        this.bg(yellow_bg).border_1().border_color(yellow_border)
      })
      .when(target, |this| this.opacity(0.6))
      .child(
        div().relative().flex_none().child(
          div()
            .size(zpx(14.))
            .rounded_full()
            .border_2()
            .border_color(p.background)
            .bg(dot),
        ),
      )
      .child(body)
  }
}

#[derive(Debug, Clone)]
enum Kind {
  Own,
  Target,
  /// The dirty working copy, with its workspace's label.
  Tentative(String),
}

fn day_label(label: &str, p: &Palette) -> impl IntoElement {
  ui::text(div(), TextSize::Xs)
    .mb_1()
    .pl_7()
    .text_color(p.muted_foreground)
    .child(label.to_string())
}

/// The dashed `Diverged from` / `Recent on` rule.
fn separator(label: Div, p: &Palette) -> impl IntoElement {
  // `border-t border-dashed`, as Chromium dashes a 1px border.
  let dashes = || {
    div()
      .flex_1()
      .h(zpx(1.))
      .overflow_hidden()
      .flex()
      .gap(zpx(3.))
      .children((0..60).map(|_| div().flex_none().w(zpx(3.)).h(zpx(1.)).bg(p.border)))
  };
  div().my_4().pl_7().child(
    div()
      .flex()
      .items_center()
      .gap_2()
      .child(dashes())
      .child(
        ui::text(label, TextSize::Xs)
          .whitespace_nowrap()
          .px_2()
          .text_color(p.muted_foreground),
      )
      .child(dashes()),
  )
}

/// `StackDot`.
fn stack_dot(highlighted: bool, p: &Palette) -> impl IntoElement {
  div().flex_none().mt(zpx(2.)).child(
    div()
      .size(zpx(14.))
      .rounded_full()
      .border_2()
      .border_color(p.background)
      .bg(if highlighted {
        p.primary
      } else {
        p.muted_foreground
      }),
  )
}

/// `WorkspaceLocIndicator`: `+N [bar] -M` in columns `plus_ch`/`minus_ch`
/// characters wide, so the bars line up down a stack.
fn loc_indicator(
  insertions: u32,
  deletions: u32,
  max: u32,
  (plus_ch, minus_ch): (usize, usize),
  dark: bool,
  p: &Palette,
) -> impl IntoElement {
  const HALF: f32 = 14.;
  // JetBrains Mono's advance at 11px.
  const CH: f32 = 11. * 0.6;
  let width = |n: u32| {
    if n == 0 || max == 0 {
      0.
    } else {
      ((n as f32 / max as f32) * HALF).round().max(1.)
    }
  };
  let green = tw(if dark { GREEN_400 } else { GREEN_600 });
  let red = tw(if dark { RED_400 } else { RED_600 });
  let count = |text: String, color: Hsla, ch: usize| {
    ui::text(div(), TextSize::Xs)
      .flex_none()
      .whitespace_nowrap()
      .w(zpx(ch as f32 * CH))
      .font_family(theme::MONO_FONT)
      .text_color(color)
      .child(text)
  };
  div()
    .debug_selector(|| "workspace-loc-indicator".into())
    .flex()
    .items_center()
    .gap(zpx(6.))
    .child(
      count(format!("+{insertions}"), green, plus_ch)
        .flex()
        .justify_end(),
    )
    .child(
      div()
        .flex()
        .flex_none()
        .items_center()
        .h(zpx(6.))
        .child(
          div()
            .flex()
            .justify_end()
            .h_full()
            .w(zpx(HALF))
            .child(div().h_full().w(zpx(width(insertions))).bg(green)),
        )
        .child(div().w_px().h_full().bg(p.border))
        .child(
          div()
            .flex()
            .h_full()
            .w(zpx(HALF))
            .child(div().h_full().w(zpx(width(deletions))).bg(red)),
        ),
    )
    .child(count(format!("-{deletions}"), red, minus_ch))
}
