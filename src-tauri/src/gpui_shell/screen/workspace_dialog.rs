//! `UnifiedWorkspaceDialog`: "Stack a new Workspace". Creates a workspace,
//! stacks one on another, or moves commits or files into a new or an
//! existing workspace, all through the backend
//! ([`workspace_dialog_data`]). React only says which dialog opened, with
//! which defaults; it hears back the workspace to open.
//!
//! [`workspace_dialog_data`]: super::workspace_dialog_data

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{collections::HashMap, sync::Arc, time::Duration};

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
  input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp, Textarea, TextareaState},
  select::{Select, SelectEvent, SelectState},
  IndexPath,
};
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, App, ClickEvent, Context, Div, Entity,
  FocusHandle, FontWeight, Hsla, KeyDownEvent, MouseButton, SharedString, Stateful, Subscription,
  Task, Window,
};

use super::{
  workspace_deletion::matches,
  workspace_dialog_data::{
    self as data, branch_from_title, branch_name_error, new_workspace_chain, ChainEntry, Position,
    Route, Source, StashCommit, Submit, Submitted, Tab, WorkspaceDialogHost,
  },
  workspace_modal::{dialog_description, dialog_header, dialog_panel, label_xs, press, Check},
  ActionSink, ScreenAction,
};
use crate::{
  core::WorkspaceCommit,
  gpui_shell::{
    form::SelectOption,
    theme::{self, Palette},
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::{BranchStatus, JjBranch, JjDiffHunk, JjFileChange},
  local_db::Workspace,
};

/// `useDebounce(branchName, 500)`.
const DEBOUNCE: Duration = Duration::from_millis(500);
/// `treq/{name}`, until the repository's pattern loads.
const DEFAULT_PATTERN: &str = "treq/{name}";

/// `branchStatus` as the stack card shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchState {
  New,
  Local,
  Remote,
  Checking,
}

/// A file's hunks once expanded.
#[derive(Debug, Clone, Default)]
pub struct Hunks {
  pub hunks: Vec<JjDiffHunk>,
  pub loading: bool,
}

/// The text fields, made on the first draw.
struct Fields {
  title: Entity<InputState>,
  description: Entity<TextareaState>,
  branch: Entity<InputState>,
  sparse: Entity<TextareaState>,
  symlinks: Entity<TextareaState>,
  /// The pattern the branch placeholder shows.
  pattern: String,
}

/// `TargetBranchSelector`'s open popover.
struct BranchPicker {
  input: Entity<InputState>,
  query: String,
  /// The highlighted branch's name.
  selected: Option<String>,
  _subscription: Subscription,
}

/// The Target Workspace select's options, and the select.
type TargetSelect = (Vec<SelectOption>, Entity<SelectState<Vec<SelectOption>>>);

pub struct WorkspaceDialog {
  route: Route,
  host: Arc<dyn WorkspaceDialogHost>,
  sink: ActionSink,
  dark: bool,
  focus: FocusHandle,
  fields: Option<Fields>,
  picker: Option<BranchPicker>,
  target_select: Option<TargetSelect>,
  pub(super) description: String,
  pub(super) title: String,
  pub(super) sparse_paths: String,
  pub(super) symlinked_dirs: String,
  pub(super) branch_name: String,
  pub(super) branch_pattern: String,
  pub(super) editing_branch: bool,
  pub(super) loading: bool,
  pub(super) error: String,
  /// The last check, with the name it was for.
  pub(super) branch_check: Option<(String, BranchStatus)>,
  pub(super) checking_branch: bool,
  pub(super) target_branch: Option<String>,
  pub(super) branches: Vec<JjBranch>,
  pub(super) position: Position,
  pub(super) tab: Tab,
  pub(super) changed_files: Vec<JjFileChange>,
  pub(super) hunks: HashMap<String, Hunks>,
  pub(super) expanded: Vec<String>,
  /// `path::hunk` keys, in the order picked.
  pub(super) selected_hunks: Vec<String>,
  /// Change ids, in the order picked.
  pub(super) selected_commits: Vec<String>,
  pub(super) commits_ahead: Vec<WorkspaceCommit>,
  pub(super) data_loading: bool,
  pub(super) workspaces: Vec<Workspace>,
  pub(super) move_section_open: bool,
  pub(super) advanced_open: bool,
  pub(super) move_to_existing: bool,
  pub(super) target_workspace_id: Option<i64>,
  pub(super) default_branch: Option<String>,
  pub(super) gitignore_suggestions: Vec<String>,
  pub(super) dismissed: bool,
  _check: Option<Task<()>>,
  _subscriptions: Vec<Subscription>,
}

fn hunk_key(path: &str, id: &str) -> String {
  format!("{path}::{id}")
}

fn toggle(list: &mut Vec<String>, item: &str) {
  match list.iter().position(|i| i == item) {
    Some(at) => {
      list.remove(at);
    }
    None => list.push(item.to_string()),
  }
}

impl WorkspaceDialog {
  pub fn new(
    route: Route,
    host: Arc<dyn WorkspaceDialogHost>,
    sink: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let defaults = route.defaults.clone();
    let mut this = Self {
      host,
      sink,
      dark,
      focus: cx.focus_handle(),
      fields: None,
      picker: None,
      target_select: None,
      description: defaults.description.clone().unwrap_or_default(),
      title: defaults.title.clone().unwrap_or_default(),
      sparse_paths: String::new(),
      symlinked_dirs: String::new(),
      branch_name: defaults.branch_name.clone().unwrap_or_default(),
      branch_pattern: DEFAULT_PATTERN.into(),
      editing_branch: defaults
        .branch_name
        .as_deref()
        .is_some_and(|b| !b.is_empty()),
      loading: false,
      error: String::new(),
      branch_check: None,
      checking_branch: false,
      target_branch: defaults.target_branch.clone(),
      branches: Vec::new(),
      position: Position::After,
      tab: defaults.active_tab.unwrap_or_default(),
      changed_files: Vec::new(),
      hunks: HashMap::new(),
      expanded: Vec::new(),
      selected_hunks: Vec::new(),
      selected_commits: defaults.pre_selected_commits.clone(),
      commits_ahead: Vec::new(),
      data_loading: true,
      workspaces: Vec::new(),
      move_section_open: !defaults.pre_selected_commits.is_empty()
        || !defaults.pre_selected_files.is_empty()
        || defaults.apply_stash_id.is_some(),
      advanced_open: false,
      move_to_existing: false,
      target_workspace_id: None,
      default_branch: None,
      gitignore_suggestions: Vec::new(),
      dismissed: false,
      _check: None,
      _subscriptions: Vec::new(),
      route,
    };
    if !this.editing_branch {
      this.branch_name = branch_from_title(&this.branch_pattern, &this.title);
    }
    this.load(cx);
    this.schedule_check(cx);
    this
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  fn source(&self) -> &Source {
    &self.route.defaults.source
  }

  fn source_id(&self) -> Option<i64> {
    match self.source() {
      Source::Workspace { id, .. } => Some(*id),
      _ => None,
    }
  }

  fn is_home(&self) -> bool {
    *self.source() == Source::Home
  }

  /// `hasSourceWorkspace`: stacking on a workspace or the home repository.
  fn has_source(&self) -> bool {
    *self.source() != Source::Create
  }

  fn stash_commit(&self) -> Option<&StashCommit> {
    self.route.defaults.apply_stash_commit.as_ref()
  }

  /// Reads what the dialog shows, by mode (`useWorkspaceDialogEffects`).
  fn load(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    let repo = self.route.repo_path.clone();
    let source = self.source().clone();
    let needs_branches = self.is_home() || self.route.defaults.target_branch.is_none();
    cx.spawn(async move |this, cx| {
      let loaded = cx
        .background_spawn({
          let host = host.clone();
          let repo = repo.clone();
          async move {
            let workspaces = host.all_workspaces(&repo).unwrap_or_default();
            match source {
              Source::Workspace { id, .. } => (
                host.commits_ahead(&repo, id).unwrap_or_default(),
                host.changed_files(&repo, Some(id)).unwrap_or_default(),
                workspaces,
                None,
              ),
              Source::Home => {
                host.fetch_in_background(&repo);
                (
                  Vec::new(),
                  host.changed_files(&repo, None).unwrap_or_default(),
                  workspaces,
                  Some(host.repo_branches(&repo).unwrap_or_default()),
                )
              }
              Source::Create => {
                let branches = needs_branches.then(|| {
                  host.fetch_in_background(&repo);
                  host.repo_branches(&repo).unwrap_or_default()
                });
                (Vec::new(), Vec::new(), workspaces, branches)
              }
            }
          }
        })
        .await;
      this
        .update(cx, |view, cx| {
          let (commits, files, workspaces, branches) = loaded;
          view.commits_ahead = commits;
          view.changed_files = files;
          if let Some(source_id) = view.source_id() {
            view.target_workspace_id = workspaces.iter().find(|w| w.id != source_id).map(|w| w.id);
          }
          view.workspaces = workspaces;
          if let Some(branches) = branches {
            view.branches = branches;
          }
          view.data_loading = false;
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();

    let suggest = !matches!(self.source(), Source::Workspace { .. });
    let (host, repo) = (self.host.clone(), self.route.repo_path.clone());
    cx.spawn(async move |this, cx| {
      let (default_branch, pattern, suggestions) = cx
        .background_spawn(async move {
          let pattern = host
            .repo_settings(&repo, &["branch_name_pattern".to_string()])
            .ok()
            .and_then(|values| values.get("branch_name_pattern").cloned().flatten());
          (
            host.default_branch(&repo).ok(),
            pattern,
            if suggest {
              host.gitignore_suggestions(&repo)
            } else {
              Vec::new()
            },
          )
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.default_branch = default_branch;
          view.gitignore_suggestions = suggestions;
          view.branch_pattern = pattern
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| DEFAULT_PATTERN.into());
          view.derive_branch(cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// The branch follows the title until it is edited.
  fn derive_branch(&mut self, cx: &mut Context<Self>) {
    if self.editing_branch {
      return;
    }
    let next = branch_from_title(&self.branch_pattern, &self.title);
    if next != self.branch_name {
      self.branch_name = next;
      self.schedule_check(cx);
    }
  }

  /// Checks the branch name once typing pauses.
  fn schedule_check(&mut self, cx: &mut Context<Self>) {
    let name = self.branch_name.clone();
    if name.trim().is_empty() || self.move_to_existing {
      self.branch_check = None;
      self.checking_branch = false;
      self._check = None;
      return;
    }
    let (host, repo) = (self.host.clone(), self.route.repo_path.clone());
    self._check = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DEBOUNCE).await;
      this
        .update(cx, |view, cx| {
          view.checking_branch = true;
          cx.notify();
        })
        .ignore_closed();
      let status = cx
        .background_spawn({
          let name = name.clone();
          async move { host.branch_status(&repo, &name).ok() }
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.checking_branch = false;
          // A new name has no result until its check resolves.
          view.branch_check = status.map(|s| (name, s));
          cx.notify();
        })
        .ignore_closed();
    }));
  }

  /// `branchStatus`.
  pub(super) fn branch_state(&self) -> Option<BranchState> {
    let stale = !self.branch_name.trim().is_empty()
      && self.branch_check.as_ref().map(|(n, _)| n) != Some(&self.branch_name);
    if self.checking_branch || stale {
      return Some(BranchState::Checking);
    }
    self.branch_check.as_ref().map(|(_, s)| {
      if s.local_exists {
        BranchState::Local
      } else if s.remote_exists {
        BranchState::Remote
      } else {
        BranchState::New
      }
    })
  }

  pub(super) fn branch_error(&self) -> Option<String> {
    if self.move_to_existing {
      return None;
    }
    branch_name_error(&self.branch_name, &self.workspaces)
  }

  /// The files of the picked hunks (`selectedFilePaths`).
  pub(super) fn selected_files(&self) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for key in &self.selected_hunks {
      if let Some((path, _)) = key.split_once("::") {
        if !paths.iter().any(|p| p == path) {
          paths.push(path.to_string());
        }
      }
    }
    paths
  }

  pub(super) fn can_submit(&self) -> bool {
    if self.loading {
      return false;
    }
    if self.move_to_existing {
      let picked = match self.tab {
        Tab::Commits => !self.selected_commits.is_empty(),
        Tab::Changes => !self.selected_hunks.is_empty(),
      };
      return picked && self.target_workspace_id.is_some();
    }
    !self.branch_name.trim().is_empty() && self.branch_error().is_none()
  }

  pub(super) fn submit_label(&self) -> String {
    if self.loading {
      return "Creating...".into();
    }
    if self.route.defaults.apply_stash_id.is_some() {
      return "Create & apply stash".into();
    }
    if self.move_to_existing {
      let count = match self.tab {
        Tab::Commits => self.selected_commits.len(),
        Tab::Changes => self.selected_hunks.len(),
      };
      return if count > 0 {
        format!("Move {count} to Existing Workspace")
      } else {
        "Move to Existing Workspace".into()
      };
    }
    let has_selection = !self.selected_commits.is_empty() || !self.selected_hunks.is_empty();
    if self.source_id().is_some() && has_selection {
      let count = self.selected_commits.len() + self.selected_files().len();
      return format!("Split {count} item{}", if count != 1 { "s" } else { "" });
    }
    if self.is_home() && !self.selected_hunks.is_empty() {
      return format!("Create with {} file(s)", self.selected_files().len());
    }
    "Create Workspace".into()
  }

  fn other_workspaces(&self) -> Vec<&Workspace> {
    let source = self.source_id();
    self
      .workspaces
      .iter()
      .filter(|w| Some(w.id) != source)
      .collect()
  }

  /// The branch the stack card's chain starts from.
  fn parent_branch(&self) -> Option<String> {
    match self.source() {
      Source::Workspace { branch_name, .. } => Some(branch_name.clone()),
      _ => self
        .target_branch
        .clone()
        .or_else(|| self.default_branch.clone()),
    }
  }

  pub(super) fn set_move_to_existing(&mut self, on: bool, cx: &mut Context<Self>) {
    self.move_to_existing = on;
    // Moving into an existing workspace needs a selection from the move section.
    if on {
      self.move_section_open = true;
    }
    self.schedule_check(cx);
    cx.notify();
  }

  pub(super) fn toggle_commit(&mut self, change_id: &str, cx: &mut Context<Self>) {
    toggle(&mut self.selected_commits, change_id);
    cx.notify();
  }

  /// `handleToggleFileExpand`, loading the file's hunks the first time.
  pub(super) fn toggle_file_expand(&mut self, path: &str, cx: &mut Context<Self>) {
    toggle(&mut self.expanded, path);
    if !self.hunks.contains_key(path) {
      self.hunks.insert(
        path.to_string(),
        Hunks {
          hunks: Vec::new(),
          loading: true,
        },
      );
      let (host, repo, id) = (
        self.host.clone(),
        self.route.repo_path.clone(),
        self.source_id(),
      );
      let path = path.to_string();
      cx.spawn(async move |this, cx| {
        let hunks = cx
          .background_spawn({
            let path = path.clone();
            async move {
              host
                .file_hunks(&repo, id, vec![path], None)
                .ok()
                .and_then(|batch| batch.files.into_iter().next())
                .filter(|file| file.error.is_none())
                .map(|file| file.hunks)
                .unwrap_or_default()
            }
          })
          .await;
        this
          .update(cx, |view, cx| {
            view.hunks.insert(
              path,
              Hunks {
                hunks,
                loading: false,
              },
            );
            cx.notify();
          })
          .ignore_closed();
      })
      .detach();
    }
    cx.notify();
  }

  /// `getFileSelectionState`.
  pub(super) fn file_state(&self, path: &str) -> Check {
    let Some(data) = self.hunks.get(path).filter(|d| !d.hunks.is_empty()) else {
      return Check::Off;
    };
    let count = data
      .hunks
      .iter()
      .filter(|h| self.selected_hunks.contains(&hunk_key(path, &h.id)))
      .count();
    if count == 0 {
      Check::Off
    } else if count == data.hunks.len() {
      Check::On
    } else {
      Check::Some
    }
  }

  /// `toggleFileHunks`.
  pub(super) fn toggle_file_hunks(&mut self, path: &str, cx: &mut Context<Self>) {
    let keys = match self.hunks.get(path) {
      Some(data) if !data.loading => data
        .hunks
        .iter()
        .map(|h| hunk_key(path, &h.id))
        .collect::<Vec<_>>(),
      _ => return self.toggle_file_expand(path, cx),
    };
    let all = !keys.is_empty() && keys.iter().all(|k| self.selected_hunks.contains(k));
    for key in keys {
      let at = self.selected_hunks.iter().position(|k| *k == key);
      match (all, at) {
        (true, Some(at)) => {
          self.selected_hunks.remove(at);
        }
        (false, None) => self.selected_hunks.push(key),
        _ => {}
      }
    }
    cx.notify();
  }

  pub(super) fn toggle_hunk(&mut self, key: &str, cx: &mut Context<Self>) {
    toggle(&mut self.selected_hunks, key);
    cx.notify();
  }

  /// `selectAllHunks`: every hunk loaded so far.
  pub(super) fn select_all_hunks(&mut self, cx: &mut Context<Self>) {
    let mut keys = Vec::new();
    for file in &self.changed_files {
      if let Some(data) = self.hunks.get(&file.path) {
        keys.extend(data.hunks.iter().map(|h| hunk_key(&file.path, &h.id)));
      }
    }
    for (path, data) in &self.hunks {
      for hunk in &data.hunks {
        let key = hunk_key(path, &hunk.id);
        if !keys.contains(&key) {
          keys.push(key);
        }
      }
    }
    self.selected_hunks = keys;
    cx.notify();
  }

  /// Closing mid-create would hide a workspace that is still being made.
  pub(super) fn close_unless_creating(&mut self, cx: &mut Context<Self>) {
    if self.loading {
      return;
    }
    self.dismissed = true;
    press(&self.sink, "workspaceDialog.close", cx);
    cx.notify();
  }

  pub(super) fn submit(&mut self, cx: &mut Context<Self>) {
    if !self.can_submit() {
      return;
    }
    self.loading = true;
    self.error.clear();
    cx.notify();
    let form = Submit {
      repo_path: self.route.repo_path.clone(),
      title: self.title.clone(),
      description: self.description.clone(),
      sparse_paths: self.sparse_paths.clone(),
      symlinked_dirs: self.symlinked_dirs.clone(),
      branch_name: self.branch_name.trim().to_string(),
      move_to_existing: self.move_to_existing,
      source: self.source().clone(),
      position: self.position,
      target_branch: self.target_branch.clone(),
      workspaces: self.workspaces.clone(),
      branch_check: self.branch_check.clone(),
      tab: self.tab,
      selected_commits: self.selected_commits.clone(),
      selected_hunks: self.selected_hunks.len(),
      selected_files: self.selected_files(),
      target_workspace_id: self.target_workspace_id,
      apply_stash_id: self.route.defaults.apply_stash_id,
    };
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let submitted = cx
        .background_spawn(async move { data::submit(&*host, &form) })
        .await;
      this
        .update(cx, |view, cx| {
          view.loading = false;
          match submitted {
            Submitted::Done(id) => {
              view.dismissed = true;
              (view.sink)(
                ScreenAction::Field {
                  field: "workspaceDialog.created".into(),
                  value: id.into(),
                },
                cx,
              );
            }
            Submitted::Failed(message) => view.error = message,
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search branches..."));
    input.update(cx, |input, cx| input.focus(window, cx));
    let subscription = cx.subscribe(&input, |view, input, event: &InputEvent, cx| match event {
      InputEvent::Change => {
        let query = input.read(cx).value().to_string();
        if let Some(picker) = &mut view.picker {
          picker.query = query;
        }
        let first = view.picker_branches().first().map(|b| b.name.clone());
        if let Some(picker) = &mut view.picker {
          picker.selected = first;
        }
        cx.notify();
      }
      InputEvent::PressEnter { .. } => {
        let selected = view.picker.as_ref().and_then(|p| p.selected.clone());
        if let Some(name) = selected {
          view.pick_branch(name, cx);
        }
      }
      _ => {}
    });
    self.picker = Some(BranchPicker {
      input,
      query: String::new(),
      selected: None,
      _subscription: subscription,
    });
    let first = self.picker_branches().first().map(|b| b.name.clone());
    if let Some(picker) = &mut self.picker {
      picker.selected = first;
    }
    cx.notify();
  }

  fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.picker = None;
    self.focus.focus(window, cx);
    cx.notify();
  }

  fn pick_branch(&mut self, name: String, cx: &mut Context<Self>) {
    self.target_branch = Some(name);
    self.picker = None;
    cx.notify();
  }

  /// The branches the selector offers: every one until a branch name is
  /// typed, then the other workspaces' (`getValidTargets`).
  fn selector_branches(&self) -> Vec<&JjBranch> {
    self
      .branches
      .iter()
      .filter(|b| {
        self.branch_name.is_empty()
          || self
            .workspaces
            .iter()
            .any(|w| w.branch_name == b.name && w.branch_name != self.branch_name)
      })
      .collect()
  }

  fn picker_branches(&self) -> Vec<&JjBranch> {
    let query = self.picker.as_ref().map(|p| p.query.as_str()).unwrap_or("");
    self
      .selector_branches()
      .into_iter()
      .filter(|b| matches(&b.name, query))
      .collect()
  }

  fn step_picker(&mut self, delta: isize, cx: &mut Context<Self>) {
    let names: Vec<String> = self
      .picker_branches()
      .iter()
      .map(|b| b.name.clone())
      .collect();
    let Some(picker) = &mut self.picker else {
      return;
    };
    if names.is_empty() {
      return;
    }
    let at = picker
      .selected
      .as_ref()
      .and_then(|s| names.iter().position(|n| n == s))
      .unwrap_or(0) as isize;
    let next = (at + delta).clamp(0, names.len() as isize - 1) as usize;
    picker.selected = Some(names[next].clone());
    cx.notify();
  }

  #[expect(clippy::expect_used, reason = "the fields are created just above")]
  fn fields(&mut self, window: &mut Window, cx: &mut Context<Self>) -> &Fields {
    if self.fields.is_none() {
      let input = |value: &str, placeholder: &str, window: &mut Window, cx: &mut Context<Self>| {
        let (value, placeholder) = (value.to_string(), placeholder.to_string());
        cx.new(|cx| {
          InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
        })
      };
      let area = |value: &str, placeholder: &str, window: &mut Window, cx: &mut Context<Self>| {
        let (value, placeholder) = (value.to_string(), placeholder.to_string());
        cx.new(|cx| {
          TextareaState::new(window, cx)
            .auto_grow(2, 12)
            .placeholder(placeholder)
            .default_value(value)
        })
      };
      let title = input(&self.title, "e.g., Settings Dark Mode", window, cx);
      let description = area(
        &self.description,
        "e.g., Add dark mode to settings",
        window,
        cx,
      );
      let placeholder = self.branch_pattern.replace("{name}", "example");
      let branch = input(&self.branch_name, &placeholder, window, cx);
      let sparse = area(&self.sparse_paths, "e.g., src/api\ndocs", window, cx);
      let symlinks = area(
        &self.symlinked_dirs,
        "e.g., node_modules\ntarget",
        window,
        cx,
      );
      self
        ._subscriptions
        .push(cx.subscribe(&title, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.title = input.read(cx).value().to_string();
            view.derive_branch(cx);
            cx.notify();
          }
        }));
      self._subscriptions.push(cx.subscribe(
        &description,
        |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.description = input.read(cx).value().to_string();
            cx.notify();
          }
        },
      ));
      self._subscriptions.push(
        cx.subscribe(&branch, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.branch_name = input.read(cx).value().to_string();
            view.editing_branch = true;
            view.schedule_check(cx);
            cx.notify();
          }
        }),
      );
      self._subscriptions.push(
        cx.subscribe(&sparse, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.sparse_paths = input.read(cx).value().to_string();
            cx.notify();
          }
        }),
      );
      self._subscriptions.push(
        cx.subscribe(&symlinks, |view, input, event: &InputEvent, cx| {
          if let InputEvent::Change = event {
            view.symlinked_dirs = input.read(cx).value().to_string();
            cx.notify();
          }
        }),
      );
      // `autoFocus` on the description for a plain create; otherwise the
      // dialog itself takes the keys.
      if self.has_source() {
        self.focus.focus(window, cx);
      } else {
        description.update(cx, |input, cx| input.focus(window, cx));
      }
      self.fields = Some(Fields {
        title,
        description,
        branch,
        sparse,
        symlinks,
        pattern: self.branch_pattern.clone(),
      });
    }
    let fields = self.fields.as_mut().expect("made above");
    let pattern_changed = fields.pattern != self.branch_pattern;
    fields.pattern = self.branch_pattern.clone();
    // The branch follows the title, and the placeholder the pattern.
    let branch = fields.branch.clone();
    let symlinks = fields.symlinks.clone();
    let (value, placeholder) = (
      self.branch_name.clone(),
      self.branch_pattern.replace("{name}", "example"),
    );
    branch.update(cx, |state, cx| {
      if state.value() != value.as_str() {
        state.set_value(value, window, cx);
      }
      if pattern_changed {
        state.set_placeholder(placeholder, window, cx);
      }
    });
    let dirs = self.symlinked_dirs.clone();
    symlinks.update(cx, |state, cx| {
      if state.value() != dirs.as_str() {
        state.set_value(dirs, window, cx);
      }
    });
    self.fields.as_ref().expect("made above")
  }

  fn target_select(
    &mut self,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<Entity<SelectState<Vec<SelectOption>>>> {
    let options: Vec<SelectOption> = self
      .other_workspaces()
      .iter()
      .map(|w| SelectOption {
        value: w.id.to_string(),
        label: w.branch_name.clone(),
      })
      .collect();
    if options.is_empty() {
      return None;
    }
    let value = self.target_workspace_id.map(|id| id.to_string());
    let index = value
      .as_ref()
      .and_then(|v| options.iter().position(|o| &o.value == v))
      .map(IndexPath::new);
    if let Some((known, state)) = &mut self.target_select {
      let state = state.clone();
      if *known != options {
        *known = options.clone();
        state.update(cx, |s, cx| s.set_items(options, window, cx));
      }
      if state.read(cx).selected_value() != value.as_ref() {
        state.update(cx, |s, cx| match &value {
          Some(value) => s.set_selected_value(value, window, cx),
          None => s.set_selected_index(None, window, cx),
        });
      }
      return Some(state);
    }
    let state = cx.new(|cx| SelectState::new(options.clone(), index, window, cx));
    self._subscriptions.push(cx.subscribe(
      &state,
      |view, _, event: &SelectEvent<Vec<SelectOption>>, cx| {
        if let SelectEvent::Confirm(Some(value)) = event {
          view.target_workspace_id = value.parse().ok();
          cx.notify();
        }
      },
    ));
    self.target_select = Some((options, state.clone()));
    Some(state)
  }
}

/// `text-xs text-muted-foreground hover:text-foreground`, as the dialog's
/// small text buttons.
fn text_button(id: &'static str, label: &'static str, p: &Palette) -> Stateful<Div> {
  let fg = p.foreground;
  ui::text(div(), TextSize::Xs)
    .id(id)
    .debug_selector(move || id.into())
    .flex()
    .items_center()
    .gap_1()
    .cursor_pointer()
    .text_color(p.muted_foreground)
    .hover(move |s| s.text_color(fg))
    .child(label)
}

/// A collapsible section's toggle: a chevron and its label, a button named
/// by the label.
pub(super) fn disclosure(
  id: &'static str,
  label: impl Into<SharedString>,
  open: bool,
  p: &Palette,
  on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
  let fg = p.foreground;
  let label: SharedString = label.into();
  ui::text(div(), TextSize::Xs)
    .id(id)
    .debug_selector(move || id.into())
    .role(gpui::Role::Button)
    .aria_label(label.clone())
    .on_click(on_click)
    .flex()
    .items_center()
    .gap_1()
    .cursor_pointer()
    .text_color(p.muted_foreground)
    .hover(move |s| s.text_color(fg))
    .child(ui::icon(
      if open {
        theme::ICON_CHEVRON_DOWN
      } else {
        theme::ICON_CHEVRON_RIGHT
      },
      12.,
      p.muted_foreground,
    ))
    .child(label)
    .test_support()
}

/// `StackDot`: a 14px dot with a background ring.
fn stack_dot(highlighted: bool, p: &Palette) -> Div {
  div().flex_none().mt_0p5().child(
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

/// `statusIcon` and `statusColor`.
fn file_status(status: &str, p: &Palette) -> (&'static str, Hsla) {
  match status {
    "modified" => ("M", p.warning),
    "added" => ("A", p.success),
    "deleted" => ("D", p.destructive),
    "renamed" => ("R", p.primary),
    _ => ("?", p.muted_foreground),
  }
}

impl WorkspaceDialog {
  fn left_panel(&mut self, window: &mut Window, p: &Palette, cx: &mut Context<Self>) -> Div {
    let fields = self.fields(window, cx);
    let (title, description, branch, sparse, symlinks) = (
      fields.title.clone(),
      fields.description.clone(),
      fields.branch.clone(),
      fields.sparse.clone(),
      fields.symlinks.clone(),
    );
    let p = *p;
    let source = self.source().clone();
    let stacking_on = match &source {
      Source::Workspace {
        branch_name,
        target_branch,
        ..
      } => {
        let shown = match (self.position, target_branch) {
          (Position::Before, Some(target)) => target.clone(),
          _ => branch_name.clone(),
        };
        // A disabled `<Input className="text-xs text-muted-foreground h-8">`.
        ui::text(div(), TextSize::Xs)
          .debug_selector(|| "workspace-dialog-stacking-on".into())
          .h(zpx(32.))
          .px_3()
          .flex()
          .items_center()
          .rounded(radius::SM)
          .border_1()
          .border_color(p.input)
          .bg(p.background)
          .opacity(0.4)
          .text_color(p.muted_foreground)
          .child(shown)
          .into_any_element()
      }
      _ => self.branch_selector(&p, cx).into_any_element(),
    };
    let mut panel = div().flex().flex_col().gap_3().w_full().child(
      div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(label_xs("Stacking on"))
        .child(stacking_on),
    );

    // Only stacking on a workspace honours "before".
    if matches!(source, Source::Workspace { .. }) && !self.move_to_existing {
      let segment = |id: &'static str, label: &'static str, pos: Position, current: Position| {
        let active = pos == current;
        let fg = p.foreground;
        ui::text(div(), TextSize::Xs)
          .id(id)
          .debug_selector(move || id.into())
          .px_2p5()
          .py_1()
          .rounded(radius::SM)
          .font_weight(FontWeight::MEDIUM)
          .cursor_pointer()
          .when(active, |this| {
            this.bg(p.selected()).text_color(p.foreground)
          })
          .when(!active, |this| {
            this
              .text_color(p.muted_foreground)
              .hover(move |s| s.text_color(fg))
          })
          .child(label)
      };
      panel = panel.child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(label_xs("Position:"))
          .child(
            div()
              .flex()
              .gap_1()
              .bg(p.muted)
              .p_0p5()
              .rounded(radius::MD)
              .child(
                segment("position-before", "Before", Position::Before, self.position).on_click(
                  cx.listener(|view, _, _, cx| {
                    view.position = Position::Before;
                    cx.notify();
                  }),
                ),
              )
              .child(
                segment("position-after", "After", Position::After, self.position).on_click(
                  cx.listener(|view, _, _, cx| {
                    view.position = Position::After;
                    cx.notify();
                  }),
                ),
              ),
          ),
      );
    }

    if !self.move_to_existing {
      if let Some(base) = self.default_branch.clone() {
        panel = panel.child(self.stack_card(&branch, &base, &p));
      }
    }

    if matches!(source, Source::Workspace { .. }) {
      let on = self.move_to_existing;
      panel = panel.child(
        div()
          .border_t_1()
          .border_color(p.border.opacity(0.5))
          .pt_3()
          .child(
            div()
              .id("move-to-existing")
              .debug_selector(|| "move-to-existing".into())
              .flex()
              .items_center()
              .gap_2()
              .cursor_pointer()
              .child(ui::Checkbox::new("move-to-existing-box", &p).checked(on))
              .child(
                ui::text(div(), TextSize::Xs)
                  .text_color(p.muted_foreground)
                  .child("Move to existing workspace instead"),
              )
              .on_click(cx.listener(move |view, _, _, cx| view.set_move_to_existing(!on, cx))),
          ),
      );
    }

    if self.move_to_existing {
      let select = self.target_select(window, cx);
      panel = panel.child(
        div()
          .flex()
          .flex_col()
          .gap_1p5()
          .child(label_xs("Target workspace"))
          .child(match select {
            None => ui::text(div(), TextSize::Xs)
              .debug_selector(|| "no-other-workspaces".into())
              .text_color(p.muted_foreground)
              .child("No other workspaces available.")
              .into_any_element(),
            Some(state) => div()
              .id("target-workspace")
              .debug_selector(|| "target-workspace".into())
              .w_full()
              .child(
                ui::text(
                  Select::new(&state).id("target-workspace-select"),
                  TextSize::Xs,
                )
                .h(zpx(32.)),
              )
              .into_any_element(),
          }),
      );
    } else {
      panel = panel
        .child(
          div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(label_xs("Title (optional)"))
            .child(
              div()
                .id("workspace-dialog-title")
                .debug_selector(|| "workspace-dialog-title".into())
                .w_full()
                .child(ui::text(
                  Styled::h(Input::new(&title).aria_label("Title"), zpx(32.)),
                  TextSize::Sm,
                )),
            ),
        )
        .child(
          div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(label_xs("Description (optional)"))
            .child(
              div()
                .debug_selector(|| "workspace-dialog-description".into())
                .min_h(zpx(58.))
                .child(ui::text(Textarea::new(&description), TextSize::Sm)),
            ),
        );
    }

    // Sparse checkout and symlinked dirs, for a create from scratch.
    if !self.move_to_existing && !matches!(source, Source::Workspace { .. }) {
      let open = self.advanced_open;
      let mut advanced = div().flex().flex_col().gap_1p5().child(disclosure(
        "workspace-dialog-advanced",
        "Advanced",
        open,
        &p,
        cx.listener(|view, _, _, cx| {
          view.advanced_open = !view.advanced_open;
          cx.notify();
        }),
      ));
      if open {
        let chips = self.gitignore_suggestions.clone();
        advanced = advanced.child(
          div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
              div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(label_xs("Sparse paths (optional, one per line)"))
                .child(
                  div()
                    .debug_selector(|| "workspace-dialog-sparse".into())
                    .min_h(zpx(58.))
                    .child(ui::text(Textarea::new(&sparse), TextSize::Sm)),
                ),
            )
            .child(
              div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(label_xs("Symlink from home repo (optional, one per line)"))
                .child(
                  div()
                    .debug_selector(|| "workspace-dialog-symlinks".into())
                    .min_h(zpx(58.))
                    .child(ui::text(Textarea::new(&symlinks), TextSize::Sm)),
                )
                .child(
                  ui::text(div(), TextSize::Xs)
                    .text_color(p.muted_foreground)
                    .child(
                      "Heavy dirs are linked instead of copied so each workspace shares the home \
                       tree.",
                    ),
                )
                .when(!chips.is_empty(), |this| {
                  this.child(div().flex().flex_wrap().gap_1p5().children(
                    chips.into_iter().enumerate().map(|(i, chip)| {
                      let label = format!("+ {chip}");
                      ui::text(
                        Button::new(SharedString::from(format!("symlink-suggestion-{i}")), &p)
                          .variant(Variant::Outline)
                          .size(Size::Sm)
                          .h(zpx(24.))
                          .label(label),
                        TextSize::Xs,
                      )
                      .on_click(cx.listener(move |view, _, _, cx| {
                        view.symlinked_dirs =
                          data::append_path_suggestion(&view.symlinked_dirs, &chip);
                        cx.notify();
                      }))
                    }),
                  ))
                }),
            ),
        );
      }
      panel = panel.child(advanced);
    }
    panel
  }

  /// `TargetBranchSelector`: the button, and its search popover while open.
  fn branch_selector(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
    let p = *p;
    let target = self.target_branch.clone();
    let disabled = self.loading;
    let button = Button::new("workspace-dialog-target", &p)
      .aria_label("Workspace target")
      .variant(Variant::Outline)
      .size(Size::Sm)
      .disabled(disabled)
      .max_w(zpx(220.))
      .child(ui::icon(theme::ICON_BRANCH, 16., p.foreground))
      .child(
        div()
          .font_family(theme::MONO_FONT)
          .min_w_0()
          .truncate()
          .child(target.clone().unwrap_or_else(|| "Select...".into())),
      )
      .child(ui::icon(
        theme::ICON_CHEVRON_DOWN,
        16.,
        p.foreground.opacity(0.5),
      ))
      .on_click(cx.listener(|view, _, window, cx| {
        if view.picker.is_some() {
          view.close_picker(window, cx);
        } else {
          view.open_picker(window, cx);
        }
      }));
    let popover = self.picker.as_ref().map(|picker| {
      let selected = picker.selected.clone();
      let branches: Vec<String> = self
        .picker_branches()
        .iter()
        .map(|b| b.name.clone())
        .collect();
      let list = if branches.is_empty() {
        ui::text(div(), TextSize::Sm)
          .px_2()
          .py_4()
          .text_center()
          .text_color(p.muted_foreground)
          .child("No branches found")
          .into_any_element()
      } else {
        div()
          .children(branches.into_iter().enumerate().map(|(i, name)| {
            let id = SharedString::from(format!("branch-option-{i}"));
            let selector = id.to_string();
            let active = selected.as_deref() == Some(name.as_str());
            let checked = target.as_deref() == Some(name.as_str());
            let pick = name.clone();
            ui::text(div(), TextSize::Sm)
              .id(id)
              .debug_selector(move || selector)
              .px_3()
              .py_1p5()
              .flex()
              .items_center()
              .gap_2()
              .cursor_pointer()
              .font_family(theme::MONO_FONT)
              .when(active, |this| this.bg(p.menu_accent))
              .when(!active, |this| this.hover(move |s| s.bg(p.hover())))
              .child(div().flex_1().min_w_0().truncate().child(name))
              .when(checked, |this| {
                this.child(ui::icon(theme::ICON_CHECK, 16., p.popover_foreground))
              })
              .on_click(cx.listener(move |view, _, _, cx| view.pick_branch(pick.clone(), cx)))
          }))
          .into_any_element()
      };
      deferred(
        anchored().child(
          div()
            .id("branch-picker")
            .debug_selector(|| "branch-picker".into())
            .mt_1()
            .w(zpx(300.))
            .rounded(radius::MD)
            .border_1()
            .border_color(p.border)
            .bg(p.popover)
            .text_color(p.popover_foreground)
            .shadow_md()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|view, _, window, cx| view.close_picker(window, cx)))
            .capture_action(cx.listener(|view, _: &MoveDown, _, cx| {
              view.step_picker(1, cx);
              cx.stop_propagation();
            }))
            .capture_action(cx.listener(|view, _: &MoveUp, _, cx| {
              view.step_picker(-1, cx);
              cx.stop_propagation();
            }))
            .capture_action(cx.listener(|view, _: &Escape, window, cx| {
              view.close_picker(window, cx);
              cx.stop_propagation();
            }))
            .child(
              ui::text(div(), TextSize::Sm)
                .h(zpx(40.))
                .px_3()
                .flex()
                .items_center()
                .child(
                  ui::input(&picker.input)
                    .appearance(false)
                    .bordered(false)
                    .flex_1(),
                ),
            )
            .child(
              div()
                .id("branch-picker-list")
                .max_h(zpx(300.))
                .overflow_y_scroll()
                .child(list),
            ),
        ),
      )
      .with_priority(3)
    });
    div().flex().flex_col().child(button).children(popover)
  }

  /// `NewWorkspaceStackCard`.
  fn stack_card(&self, branch: &Entity<InputState>, base: &str, p: &Palette) -> Div {
    let p = *p;
    let chain = self
      .parent_branch()
      .map(|parent| new_workspace_chain(&self.workspaces, &parent, self.position, base))
      .unwrap_or_default();
    let error = self.branch_error();
    let status = self.branch_state();
    let status_icon = match (&error, status) {
      (Some(_), _) => {
        Some(ui::icon(theme::ICON_CIRCLE_ALERT, 14., p.destructive).into_any_element())
      }
      (None, Some(BranchState::Checking)) => {
        Some(ui::spinner("branch-checking", 14., p.muted_foreground).into_any_element())
      }
      (None, Some(BranchState::New)) => {
        Some(ui::icon(theme::ICON_CHECK, 14., p.success).into_any_element())
      }
      (None, Some(BranchState::Local)) => {
        Some(ui::icon(theme::ICON_CIRCLE_ALERT, 14., p.warning).into_any_element())
      }
      (None, Some(BranchState::Remote)) => {
        Some(ui::icon(theme::ICON_CLOUD, 14., p.primary).into_any_element())
      }
      (None, None) => None,
    };
    let hint = match (&error, status) {
      (Some(err), _) => Some((err.clone(), p.destructive, "branch-name-error")),
      (None, Some(BranchState::Local)) => Some((
        "Branch already exists locally".to_string(),
        p.warning,
        "branch-exists-local",
      )),
      (None, Some(BranchState::Remote)) => Some((
        "Branch exists on remote — will check out".to_string(),
        p.primary,
        "branch-exists-remote",
      )),
      _ => None,
    };
    let mut new_row = Some(
      div()
        .debug_selector(|| "new-workspace-stack-item".into())
        .relative()
        .flex()
        .w_full()
        .items_start()
        .gap_3()
        .py_2()
        .px_2()
        .mx_neg_2()
        .rounded(radius::MD)
        // Opaque: GPUI draws the shadow under the whole box, where a
        // browser leaves it out, so a see-through tint would darken.
        .bg(p.popover.blend(p.primary.opacity(0.1)))
        .border_1()
        .border_color(p.primary.opacity(0.4))
        .shadow_sm()
        .child(div().mt_1p5().child(stack_dot(true, &p)))
        .child(
          div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
              div()
                .id("workspace-dialog-branch")
                .debug_selector(|| "workspace-dialog-branch".into())
                .w_full()
                .child(
                  ui::text(
                    Styled::h(Input::new(branch).aria_label("Branch Name"), zpx(32.)),
                    TextSize::Sm,
                  )
                  .font_family(theme::MONO_FONT)
                  .when_some(status_icon, |this, icon| this.suffix(icon)),
                ),
            )
            .when_some(hint, |this, (text, color, id)| {
              this.child(
                ui::text(div(), TextSize::Xs)
                  .debug_selector(move || id.into())
                  .text_color(color)
                  .child(text),
              )
            }),
        ),
    );
    let rows = chain.into_iter().filter_map(|entry| match entry {
      ChainEntry::New => new_row.take().map(IntoElement::into_any_element),
      ChainEntry::Branch { branch, workspace } => Some(
        div()
          .relative()
          .flex()
          .w_full()
          .items_start()
          .gap_3()
          .py_2()
          .px_2()
          .mx_neg_2()
          .child(stack_dot(false, &p))
          .child(
            ui::text(div(), TextSize::Sm)
              .flex_1()
              .min_w_0()
              .font_family(theme::MONO_FONT)
              .whitespace_nowrap()
              .overflow_hidden()
              .text_ellipsis_start()
              .when(!workspace, |this| this.text_color(p.muted_foreground))
              .child(branch),
          )
          .into_any_element(),
      ),
    });
    let rows: Vec<_> = rows.collect();
    div()
      .debug_selector(|| "new-workspace-stack-card".into())
      .border_1()
      .border_color(p.border)
      .rounded(radius::MD)
      .px_4()
      .py_2()
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
          .child(
            div().flex().flex_col().children(rows).child(
              div()
                .relative()
                .flex()
                .w_full()
                .items_start()
                .gap_3()
                .py_2()
                .px_2()
                .mx_neg_2()
                .text_color(p.muted_foreground)
                .child(
                  div()
                    .flex_none()
                    .mt_0p5()
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
                    .child(base.to_string()),
                ),
            ),
          ),
      )
  }

  /// `WorkspaceRightPanel`.
  fn right_panel(&self, p: &Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
    let p = *p;
    let locked = self.stash_commit().cloned();
    if self.data_loading && locked.is_none() {
      return div()
        .flex()
        .items_center()
        .justify_center()
        .py_6()
        .child(ui::spinner(
          "workspace-dialog-loading",
          20.,
          p.muted_foreground,
        ))
        .child(
          ui::text(div(), TextSize::Sm)
            .ml_2()
            .text_color(p.muted_foreground)
            .child("Loading..."),
        )
        .into_any_element();
    }
    let mut commits: Vec<WorkspaceCommit> = Vec::new();
    if let Some(stash) = &locked {
      commits.push(WorkspaceCommit {
        hash: stash.hash.clone(),
        timestamp: stash.timestamp.clone(),
        message: stash.message.clone(),
      });
    }
    commits.extend(
      self
        .commits_ahead
        .iter()
        .filter(|c| locked.as_ref().is_none_or(|s| s.hash != c.hash))
        .cloned(),
    );
    let tab_trigger =
      |id: &'static str, label: &'static str, count: usize, active: bool, disabled: bool| {
        let fg = p.foreground;
        ui::text(div(), TextSize::Xs)
          .id(id)
          .debug_selector(move || id.into())
          .flex()
          .items_center()
          .gap_1p5()
          .px_3()
          .py_1()
          .rounded(radius::MD)
          .font_weight(FontWeight::MEDIUM)
          .when(active, |this| {
            this.bg(p.selected()).text_color(p.foreground)
          })
          .when(!active, |this| this.text_color(p.muted_foreground))
          .when(!active && !disabled, |this| {
            this
              .cursor_pointer()
              .hover(move |s| s.bg(p.hover()).text_color(fg))
          })
          .when(disabled, |this| this.opacity(0.5))
          .aria_label(format!("{label} ({count})"))
          .child(label)
          .child(ui::Count::new(count, &p))
      };
    let tabs = div()
      .flex()
      .gap_1()
      .bg(p.muted)
      .p_1()
      .rounded(radius::MD)
      .mb_2()
      .child(
        tab_trigger(
          "workspace-dialog-tab-commits",
          "Commits",
          commits.len(),
          self.tab == Tab::Commits,
          false,
        )
        .on_click(cx.listener(|view, _, _, cx| {
          view.tab = Tab::Commits;
          cx.notify();
        })),
      )
      .child({
        let disabled = locked.is_some();
        tab_trigger(
          "workspace-dialog-tab-changes",
          "Changes",
          self.changed_files.len(),
          self.tab == Tab::Changes,
          disabled,
        )
        .when(!disabled, |this| {
          this.on_click(cx.listener(|view, _, _, cx| {
            view.tab = Tab::Changes;
            cx.notify();
          }))
        })
      });
    let body = match self.tab {
      Tab::Commits => self.commits_tab(commits, locked.as_ref(), &p, cx),
      Tab::Changes => self.changes_tab(&p, cx),
    };
    div()
      .flex()
      .flex_col()
      .min_w_0()
      .child(div().flex().child(tabs))
      .child(body)
      .into_any_element()
  }

  fn commits_tab(
    &self,
    commits: Vec<WorkspaceCommit>,
    locked: Option<&StashCommit>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let p = *p;
    let count = commits.len();
    let rows = if commits.is_empty() {
      ui::text(div(), TextSize::Sm)
        .p_4()
        .text_center()
        .text_color(p.muted_foreground)
        .child("No mutable commits in this workspace")
        .into_any_element()
    } else {
      div()
        .children(commits.into_iter().enumerate().map(|(i, commit)| {
          let is_locked = locked.is_some_and(|s| s.hash == commit.hash);
          let checked = is_locked || self.selected_commits.contains(&commit.hash);
          let id = SharedString::from(format!("dialog-commit-{i}"));
          let selector = id.to_string();
          let hash = commit.hash.clone();
          let short: String = commit.hash.chars().take(8).collect();
          let message = if commit.message.is_empty() {
            "(no description)".to_string()
          } else {
            commit.message
          };
          div()
            .id(id)
            .debug_selector(move || selector)
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1p5()
            .when(i + 1 < count, |this| {
              this.border_b_1().border_color(p.border)
            })
            .when(is_locked, |this| this.bg(p.menu_accent.opacity(0.4)))
            .when(!is_locked, |this| {
              this
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover()))
                .on_click(cx.listener(move |view, _, _, cx| view.toggle_commit(&hash, cx)))
            })
            .child(
              ui::Checkbox::new(SharedString::from(format!("dialog-commit-box-{i}")), &p)
                .checked(checked)
                .disabled(is_locked),
            )
            .child(
              ui::text(div(), TextSize::Xs)
                .flex_none()
                .font_family(theme::MONO_FONT)
                .text_color(p.muted_foreground)
                .child(short),
            )
            .child(
              ui::text(div(), TextSize::Xs)
                .flex_1()
                .min_w_0()
                .truncate()
                .child(if is_locked {
                  format!("{message} (stash)")
                } else {
                  message
                }),
            )
        }))
        .into_any_element()
    };
    div()
      .flex()
      .flex_col()
      .flex_1()
      .child(
        div()
          .id("dialog-commits")
          .max_h(zpx(280.))
          .overflow_y_scroll()
          .border_1()
          .border_color(p.border)
          .rounded(radius::MD)
          .child(rows),
      )
      .when(count > 0 && locked.is_none(), |this| {
        this.child(
          div()
            .flex()
            .gap_2()
            .mt_1p5()
            .child(
              text_button("dialog-commits-select-all", "Select all", &p).on_click(cx.listener(
                |view, _, _, cx| {
                  view.selected_commits =
                    view.commits_ahead.iter().map(|c| c.hash.clone()).collect();
                  cx.notify();
                },
              )),
            )
            .child(
              text_button("dialog-commits-clear", "Clear", &p).on_click(cx.listener(
                |view, _, _, cx| {
                  view.selected_commits.clear();
                  cx.notify();
                },
              )),
            ),
        )
      })
      .child(
        ui::text(div(), TextSize::Xs)
          .mt_1p5()
          .text_color(p.muted_foreground)
          .child(if locked.is_some() {
            "Stashed changes will be copied onto the new workspace"
          } else {
            "Select commits to move to the new workspace"
          }),
      )
  }

  fn changes_tab(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
    let p = *p;
    let count = self.changed_files.len();
    let rows = if self.changed_files.is_empty() {
      ui::text(div(), TextSize::Sm)
        .p_4()
        .text_center()
        .text_color(p.muted_foreground)
        .child("No uncommitted changes")
        .into_any_element()
    } else {
      div()
        .children(self.changed_files.iter().enumerate().map(|(i, file)| {
          let path = file.path.clone();
          let expanded = self.expanded.contains(&path);
          let state = self.file_state(&path);
          let (letter, color) = file_status(&file.status, &p);
          let fg = p.foreground;
          let row = div()
            .id(SharedString::from(format!("dialog-file-{i}")))
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1p5()
            .hover(move |s| s.bg(p.hover()))
            .child(
              div()
                .id(SharedString::from(format!("dialog-file-expand-{i}")))
                .flex_none()
                .size(zpx(16.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(ui::icon(
                  if expanded {
                    theme::ICON_CHEVRON_DOWN
                  } else {
                    theme::ICON_CHEVRON_RIGHT
                  },
                  12.,
                  p.muted_foreground,
                ))
                .hover(move |s| s.text_color(fg))
                .on_click(cx.listener({
                  let path = path.clone();
                  move |view, _, _, cx| view.toggle_file_expand(&path, cx)
                })),
            )
            .child(
              ui::Checkbox::new(SharedString::from(format!("dialog-file-box-{i}")), &p)
                .checked(state)
                .cursor_pointer()
                .on_click(cx.listener({
                  let path = path.clone();
                  move |view, _, _, cx| view.toggle_file_hunks(&path, cx)
                })),
            )
            .child(
              ui::text(div(), TextSize::Xs)
                .w_4()
                .flex_none()
                .text_center()
                .font_family(theme::MONO_FONT)
                .text_color(color)
                .child(letter),
            )
            .child(
              ui::text(div(), TextSize::Xs)
                .id(SharedString::from(format!("dialog-file-path-{i}")))
                .debug_selector({
                  let selector = format!("dialog-file-path-{i}");
                  move || selector
                })
                .flex_1()
                .min_w_0()
                .truncate()
                .cursor_pointer()
                .child(path.clone())
                .on_click(cx.listener({
                  let path = path.clone();
                  move |view, _, _, cx| view.toggle_file_expand(&path, cx)
                })),
            );
          let hunks = expanded.then(|| self.hunk_rows(i, &path, &p, cx));
          div()
            .when(i + 1 < count, |this| {
              this.border_b_1().border_color(p.border)
            })
            .child(row)
            .children(hunks)
        }))
        .into_any_element()
    };
    div()
      .flex()
      .flex_col()
      .flex_1()
      .child(
        div()
          .id("dialog-changes")
          .max_h(zpx(280.))
          .overflow_y_scroll()
          .border_1()
          .border_color(p.border)
          .rounded(radius::MD)
          .child(rows),
      )
      .when(count > 0, |this| {
        this.child(
          div()
            .flex()
            .gap_2()
            .mt_1p5()
            .child(
              text_button("dialog-hunks-select-all", "Select all", &p)
                .on_click(cx.listener(|view, _, _, cx| view.select_all_hunks(cx))),
            )
            .child(
              text_button("dialog-hunks-clear", "Clear", &p).on_click(cx.listener(
                |view, _, _, cx| {
                  view.selected_hunks.clear();
                  cx.notify();
                },
              )),
            ),
        )
      })
      .child(
        ui::text(div(), TextSize::Xs)
          .mt_1p5()
          .text_color(p.muted_foreground)
          .child("Select file changes to move to the new workspace"),
      )
  }

  fn hunk_rows(&self, file: usize, path: &str, p: &Palette, cx: &mut Context<Self>) -> Div {
    let p = *p;
    let muted_bg = p.muted.opacity(0.1);
    let data = self.hunks.get(path);
    match data {
      Some(data) if data.loading => ui::text(div(), TextSize::Xs)
        .flex()
        .items_center()
        .gap_2()
        .px_8()
        .py_2()
        .bg(muted_bg)
        .text_color(p.muted_foreground)
        .child(ui::spinner(
          SharedString::from(format!("hunks-loading-{file}")),
          12.,
          p.muted_foreground,
        ))
        .child("Loading hunks..."),
      Some(data) if !data.hunks.is_empty() => {
        div().children(data.hunks.iter().enumerate().map(|(i, hunk)| {
          let key = hunk_key(path, &hunk.id);
          let checked = self.selected_hunks.contains(&key);
          let selector = format!("dialog-hunk-{file}-{i}");
          div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .flex()
            .items_start()
            .gap_2()
            .px_8()
            .py_1p5()
            .cursor_pointer()
            .bg(muted_bg)
            .border_t_1()
            .border_color(p.border.opacity(0.3))
            .hover(move |s| s.bg(p.hover()))
            .child(
              div().mt_0p5().child(
                ui::Checkbox::new(
                  SharedString::from(format!("dialog-hunk-box-{file}-{i}")),
                  &p,
                )
                .checked(checked),
              ),
            )
            .child(
              div()
                .flex()
                .flex_col()
                .min_w_0()
                .overflow_hidden()
                .font_family(theme::MONO_FONT)
                .child(
                  ui::text(div(), TextSize::Xs)
                    .truncate()
                    .text_color(ui::Tone::Primary.text(&p))
                    .child(hunk.header.clone()),
                )
                .children(hunk.lines.iter().take(3).map(|line| {
                  let color = if line.starts_with('+') {
                    p.success
                  } else if line.starts_with('-') {
                    p.destructive
                  } else {
                    p.muted_foreground
                  };
                  ui::text(div(), TextSize::Xs)
                    .truncate()
                    .text_color(color)
                    .child(line.clone())
                }))
                .when(hunk.lines.len() > 3, |this| {
                  this.child(
                    ui::text(div(), TextSize::Xs)
                      .font_family(theme::SANS_FONT)
                      .text_color(p.muted_foreground)
                      .child(format!("+{} more lines", hunk.lines.len() - 3)),
                  )
                }),
            )
            .on_click(cx.listener(move |view, _, _, cx| view.toggle_hunk(&key, cx)))
        }))
      }
      _ => ui::text(div(), TextSize::Xs)
        .px_8()
        .py_1p5()
        .bg(muted_bg)
        .text_color(p.muted_foreground)
        .child("No hunks"),
    }
  }
}

impl Render for WorkspaceDialog {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.dismissed {
      return div().into_any_element();
    }
    let p = theme::palette(self.dark);
    let viewport = window.viewport_size();
    let description = match self.source() {
      Source::Workspace { branch_name, .. } => format!(
        "Create a new workspace stacked on {branch_name}. Optionally move commits or file changes."
      ),
      Source::Home => {
        "Create a new workspace from the current branch. Optionally move file changes.".into()
      }
      Source::Create => "Create a new workspace for parallel development.".into(),
    };
    let show_right = self.has_source() || self.route.defaults.apply_stash_id.is_some();
    let left = self.left_panel(window, &p, cx);
    let right = (show_right && self.move_section_open).then(|| self.right_panel(&p, cx));
    let move_section = show_right.then(|| {
      div()
        .flex()
        .flex_col()
        .gap_2()
        .border_t_1()
        .border_color(p.border.opacity(0.5))
        .pt_3()
        .child(disclosure(
          "workspace-dialog-move-section",
          "Move to workspace",
          self.move_section_open,
          &p,
          cx.listener(|view, _, _, cx| {
            view.move_section_open = !view.move_section_open;
            cx.notify();
          }),
        ))
        .children(right)
    });
    let loading = self.loading;
    let (backdrop, panel) = dialog_panel("workspace-dialog", 640., viewport, &p);
    let keys = cx.listener(|view, e: &KeyDownEvent, _, cx| {
      let k = &e.keystroke;
      if k.key == "enter"
        && (crate::gpui_shell::keys::held(k.modifiers).platform
          || crate::gpui_shell::keys::held(k.modifiers).control)
      {
        cx.stop_propagation();
        view.submit(cx);
      } else if ui::is_escape(k) && view.picker.is_none() {
        cx.stop_propagation();
        view.close_unless_creating(cx);
      }
    });
    // A fixed width, so opening Advanced or Move to workspace only makes the
    // dialog taller; the body scrolls once it reaches the window's height.
    let panel = panel
      .overflow_hidden()
      .child(dialog_header(
        "Stack a new Workspace",
        dialog_description(description, &p),
      ))
      .child(
        div()
          .id("workspace-dialog-body")
          .mt_2()
          .mx_neg_1()
          .px_1()
          .max_h(viewport.height - crate::gpui_shell::ui::zoomed(224., window))
          .overflow_y_scroll()
          .flex()
          .flex_col()
          .gap_3()
          .child(left)
          .children(move_section),
      )
      .when(!self.error.is_empty(), |this| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "workspace-dialog-error".into())
            .mt_2()
            .text_color(p.destructive)
            .child(self.error.clone()),
        )
      })
      .child(
        div()
          .flex()
          .justify_end()
          .gap_2()
          .mt_4()
          .pt_3()
          .border_t_1()
          .border_color(p.border)
          .child(
            Button::new("workspace-dialog-cancel", &p)
              .variant(Variant::Outline)
              .disabled(loading)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.close_unless_creating(cx))),
          )
          .child(
            Button::new("workspace-dialog-submit", &p)
              .disabled(!self.can_submit())
              .when(loading, |this| {
                this.child(ui::spinner(
                  "workspace-dialog-spin",
                  14.,
                  p.primary_foreground,
                ))
              })
              .label(self.submit_label())
              .on_click(cx.listener(|view, _, _, cx| view.submit(cx))),
          ),
      );
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        backdrop
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, _, cx| view.close_unless_creating(cx)),
          )
          .child(
            panel
              .id("workspace-dialog-panel")
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
