//! The workspace Commits tab (`CommitDiffViewer`): the branch's commits by
//! day, each opening to its diff and the actions on it, the target branch's
//! recent commits below, and the dialogs that edit a commit.
//!
//! Drawn from the backend directly ([`commits_data`]). What stays React's
//! goes back to it as screen actions: moving a commit to another workspace,
//! the stash list, the Changes tab, and the agent terminal a comment or a
//! conflict resolution starts in.
//!
//! [`commits_data`]: super::commits_data

use std::{
  cell::RefCell,
  collections::{HashMap, HashSet},
  rc::Rc,
  time::Duration,
};

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, App, Bounds, Context, Div, Entity,
  FontWeight, Hsla, KeyDownEvent, MouseButton, MouseMoveEvent, Pixels, ScrollHandle, SharedString,
  Stateful, Subscription, Window,
};

use super::{
  code::Parent,
  code_data::{group_by_day, Agent},
  commits_data::{
    comment_prompt, comment_target, empty_diff, first_diff, first_file_diff, first_line, headline,
    line_kind, line_label, line_numbers, resolve_session_name, revisions, selection, CommentTarget,
    CommitsHost, History, Host, LineKind, LineRef, ShiftForm, HOME_PAGE, TARGET_PAGE,
  },
  highlight, ActionSink, ScreenAction,
};
use crate::{
  gpui_shell::{
    native::{self, NativeEvent, ToastAction, ToastTone},
    popup::{PopupSink, Side, TooltipContent},
    theme::{self, tw, Palette},
    time,
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::{JjDiffHunk, JjFileChange, JjFileDiff, JjLogCommit, JjRevisionDiff},
};

/// `REMOVE_ANIMATION_MS`: how long a removed commit stays, its buttons
/// disabled, before it leaves the list.
const REMOVE_DELAY: Duration = Duration::from_millis(220);
const RESOLVE_DOCS_PATH: &str = "/docs/concepts/commit-management#resolve-commit-conflicts-inplace";
const RESOLVE_HELP: &str = "Treq opens short-lived resolve directories for each conflicted \
  commit and starts an agent there. Resolving rewrites the commit in place. It does not add a \
  follow-up resolution commit, and it leaves your product workspace untouched.";

/// Tells React what the tab changed.
fn tell(host: &dyn CommitsHost, event: NativeEvent) {
  super::workspace_data::WorkspaceHost::notify(host, event);
}

fn toast_event(tone: ToastTone, title: &str, description: impl Into<String>) -> NativeEvent {
  NativeEvent::Toast {
    title: title.into(),
    description: description.into(),
    tone,
    action: None,
  }
}

/// A commit's diff, as read, and what of it is open.
#[derive(Default)]
pub(super) struct DiffState {
  pub diff: Option<JjRevisionDiff>,
  pub loading: bool,
  pub error: Option<String>,
  /// Files closed (`expandedFiles` starts with every file open).
  pub collapsed: HashSet<String>,
  pub deferred_loading: HashSet<String>,
  pub deferred_errors: HashMap<String, String>,
}

/// The diff lines picked for a comment (`diffLineSelection`).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Selection {
  pub commit_id: String,
  pub path: String,
  pub anchor: LineRef,
  pub lines: Vec<LineRef>,
  pub dragging: bool,
}

/// The comment box open under a diff line (`pendingComment`).
pub(super) struct Comment {
  pub commit_id: String,
  pub short_id: String,
  pub target: CommentTarget,
  input: Option<Entity<TextareaState>>,
  pub text: String,
}

pub(super) enum Dialog {
  Describe {
    commit: JjLogCommit,
    loaded: Option<Result<String, String>>,
    input: Option<Entity<TextareaState>>,
    text: String,
    saving: bool,
  },
  Timestamp {
    commit: JjLogCommit,
    form: ShiftForm,
    /// Days, hours, minutes and the day.
    inputs: Option<[Entity<InputState>; 4]>,
    saving: bool,
    error: Option<String>,
  },
  Resolve {
    change_ids: Vec<String>,
    session: Option<Result<crate::core::ResolveConflictsSession, String>>,
    input: Option<Entity<TextareaState>>,
    text: String,
    submitting: bool,
  },
}

/// Where a commit row sits in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
  Own { first: bool },
  Target,
  Tentative,
}

pub struct CommitsView {
  host: Host,
  react: ActionSink,
  pub(super) parent: Parent,
  dark: bool,
  tips: Tooltips,
  pub(super) history: Option<History>,
  loading_more: bool,
  pub(super) target_limit: usize,
  pub(super) home_limit: usize,
  /// Commits abandoned, stashed or undone, until the next read drops them.
  pub(super) removed: HashSet<String>,
  pub(super) removing: HashSet<String>,
  pub(super) hide_tentative: bool,
  pub(super) expanded: HashSet<String>,
  pub(super) diffs: HashMap<String, DiffState>,
  shifting: bool,
  /// The commit whose "Move commit" menu is open.
  pub(super) move_menu: Option<String>,
  pub(super) selection: Option<Selection>,
  pub(super) comment: Option<Comment>,
  pub(super) dialog: Option<Dialog>,
  /// The change id React asked to show (`scrollToCommitId`).
  pub(super) scroll_to: Option<String>,
  /// The request last seen, so a repeat of it is not scrolled to again.
  requested: Option<String>,
  scroll: ScrollHandle,
  /// Where each commit row was last drawn, by commit id.
  rows: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
  _subscriptions: Vec<Subscription>,
}

impl HasTooltips for CommitsView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl CommitsView {
  pub fn new(
    parent: Parent,
    scroll_to: Option<String>,
    host: Host,
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
      history: None,
      loading_more: false,
      target_limit: TARGET_PAGE,
      home_limit: HOME_PAGE,
      removed: HashSet::new(),
      removing: HashSet::new(),
      hide_tentative: false,
      expanded: HashSet::new(),
      diffs: HashMap::new(),
      shifting: false,
      move_menu: None,
      selection: None,
      comment: None,
      dialog: None,
      requested: scroll_to.clone(),
      scroll_to,
      scroll: ScrollHandle::new(),
      rows: Rc::default(),
      _subscriptions: Vec::new(),
    };
    this.load(cx);
    this
  }

  pub fn set_parent(
    &mut self,
    parent: Parent,
    scroll_to: Option<String>,
    dark: bool,
    cx: &mut Context<Self>,
  ) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
      cx.notify();
    }
    // Each request once: React clears it after `commits.scrolled`.
    if self.requested != scroll_to {
      self.requested = scroll_to.clone();
      if scroll_to.is_some() {
        self.scroll_to = scroll_to;
        cx.notify();
      }
    }
    if self.parent == parent {
      return;
    }
    let moved = self.parent.key() != parent.key();
    self.parent = parent;
    if moved {
      self.history = None;
      self.target_limit = TARGET_PAGE;
      self.home_limit = HOME_PAGE;
      self.removed.clear();
      self.removing.clear();
      self.expanded.clear();
      self.diffs.clear();
      self.selection = None;
      self.comment = None;
      self.dialog = None;
      self.move_menu = None;
    }
    self.load(cx);
    cx.notify();
  }

  /// A dialog covers the whole window.
  pub(super) fn covers_window(&self) -> bool {
    self.dialog.is_some()
  }

  fn home(&self) -> bool {
    self.parent.workspace.is_none()
  }

  /// Runs `work` off the main thread, then `apply`s it unless the tab moved
  /// to another workspace meanwhile.
  fn run<T: Send + 'static>(
    &self,
    work: impl FnOnce(&dyn CommitsHost, &Parent) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let (host, parent) = (self.host.clone(), self.parent.clone());
    let key = parent.key();
    cx.spawn(async move |this, cx| {
      let value = cx
        .background_spawn(async move { work(host.as_ref(), &parent) })
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

  fn load(&mut self, cx: &mut Context<Self>) {
    let (target, home_limit) = (self.target_limit, self.home_limit);
    let home = self.home();
    self.run(
      move |host, parent| {
        host.log(
          &parent.repo_path,
          parent.workspace_id(),
          (target > TARGET_PAGE).then_some(target),
          (home && home_limit > HOME_PAGE).then_some(home_limit),
        )
      },
      move |view, log, _| {
        view.loading_more = false;
        match log {
          Ok(log) => {
            let history = History::new(log, home);
            let tentative = |h: &Option<History>| {
              h.as_ref()
                .and_then(|h| h.tentative.as_ref())
                .map(|(c, _)| c.commit_id.clone())
            };
            if tentative(&view.history) != tentative(&Some(history.clone())) {
              view.hide_tentative = false;
            }
            // A fresh read no longer lists what was removed.
            view.removed.clear();
            view.history = Some(history);
          }
          Err(err) => {
            log::warn!("Failed to load commits: {err}");
            view.history.get_or_insert_with(History::default);
          }
        }
      },
      cx,
    );
  }

  fn load_more(&mut self, target: bool, cx: &mut Context<Self>) {
    if target {
      self.target_limit += TARGET_PAGE;
    } else {
      self.home_limit += HOME_PAGE;
    }
    self.loading_more = true;
    cx.notify();
    self.load(cx);
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

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    tell(self.host.as_ref(), toast_event(tone, title, description));
  }

  /// Everything React read from the repository is stale.
  fn changed(host: &dyn CommitsHost, parent: &Parent) {
    tell(
      host,
      NativeEvent::WorkspaceChanged {
        repo_path: parent.repo_path.clone(),
      },
    );
  }

  /// The commits listed: the tentative working copy, then the branch's own.
  fn listed(&self) -> Vec<JjLogCommit> {
    let Some(history) = &self.history else {
      return Vec::new();
    };
    history
      .tentative
      .iter()
      .map(|(c, _)| c.clone())
      .chain(
        history
          .own
          .iter()
          .chain(&history.target)
          .filter(|c| !self.removed.contains(&c.commit_id))
          .cloned(),
      )
      .collect()
  }

  fn find(&self, commit_id: &str) -> Option<JjLogCommit> {
    self.listed().into_iter().find(|c| c.commit_id == commit_id)
  }

  pub(super) fn toggle(&mut self, commit_id: &str, cx: &mut Context<Self>) {
    if !self.expanded.remove(commit_id) {
      self.expand(commit_id, cx);
    }
    cx.notify();
  }

  /// `fetchAndExpand`.
  fn expand(&mut self, commit_id: &str, cx: &mut Context<Self>) {
    self.expanded.insert(commit_id.to_string());
    if self.diffs.contains_key(commit_id) {
      return;
    }
    self.diffs.insert(
      commit_id.to_string(),
      DiffState {
        loading: true,
        ..DiffState::default()
      },
    );
    let change = self.find(commit_id).map(|c| c.change_id);
    let revs = revisions(commit_id, change.as_deref());
    let id = commit_id.to_string();
    self.run(
      move |host, parent| {
        first_diff(&revs, |rev| {
          host.commit_diff(&parent.repo_path, parent.workspace_id(), rev)
        })
      },
      move |view, result, _| {
        let state = view.diffs.entry(id).or_default();
        state.loading = false;
        match result {
          Ok(diff) => state.diff = Some(diff),
          Err(err) => {
            state.diff = Some(empty_diff());
            state.error = Some(err);
          }
        }
      },
      cx,
    );
  }

  fn load_file_diff(&mut self, commit_id: &str, path: &str, cx: &mut Context<Self>) {
    let Some(state) = self.diffs.get_mut(commit_id) else {
      return;
    };
    state.deferred_loading.insert(path.to_string());
    state.deferred_errors.remove(path);
    cx.notify();
    let change = self.find(commit_id).map(|c| c.change_id);
    let revs = revisions(commit_id, change.as_deref());
    let (id, file) = (commit_id.to_string(), path.to_string());
    self.run(
      move |host, parent| {
        first_file_diff(&revs, |rev| {
          host.commit_file_diff(&parent.repo_path, parent.workspace_id(), rev, &file)
        })
      },
      {
        let path = path.to_string();
        move |view, result: Result<JjFileDiff, String>, _| {
          let Some(state) = view.diffs.get_mut(&id) else {
            return;
          };
          state.deferred_loading.remove(&path);
          match result {
            Ok(file) => {
              if let Some(diff) = state.diff.as_mut() {
                diff.hunks_by_file.retain(|f| f.path != file.path);
                diff.hunks_by_file.push(file);
              }
            }
            Err(err) => {
              state.deferred_errors.insert(path, err);
            }
          }
        }
      },
      cx,
    );
  }

  fn toggle_file(&mut self, commit_id: &str, path: &str, cx: &mut Context<Self>) {
    if let Some(state) = self.diffs.get_mut(commit_id) {
      if !state.collapsed.remove(path) {
        state.collapsed.insert(path.to_string());
      }
      cx.notify();
    }
  }

  /// Marks `commit` removed now and drops it from the list a moment later.
  fn remove_later(&mut self, commit_id: String, cx: &mut Context<Self>) {
    self.removing.insert(commit_id.clone());
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(REMOVE_DELAY).await;
      let _ = this.update(cx, |view, cx| {
        if view.removing.remove(&commit_id) {
          view.removed.insert(commit_id);
        }
        cx.notify();
      });
    })
    .detach();
  }

  /// `ask`, then `act` off the main thread.
  fn confirmed<T: Send + 'static>(
    &self,
    title: &'static str,
    message: String,
    act: impl FnOnce(&dyn CommitsHost, &Parent) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    self.run(
      move |host, parent| host.confirm(title, &message).then(|| act(host, parent)),
      move |view, result, cx| {
        if let Some(result) = result {
          apply(view, result, cx);
        }
      },
      cx,
    );
  }

  fn abandon(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let Some(ws) = self.parent.workspace_id() else {
      return;
    };
    let message = format!(
      "Abandon commit {} — {}?",
      commit.short_id,
      first_line(&commit)
    );
    let change = commit.change_id.clone();
    self.confirmed(
      "Delete Commit",
      message,
      move |host, parent| host.abandon(&parent.repo_path, ws, &change),
      move |view, result, cx| match result {
        Ok(operation) => {
          view.remove_later(commit.commit_id.clone(), cx);
          let (host, parent) = (view.host.clone(), view.parent.clone());
          Self::changed(host.as_ref(), &parent);
          let short = commit.short_id.clone();
          let undo = native::register_toast_action(move || {
            let event = match host.undo_operation(&parent.repo_path, Some(ws), &operation) {
              Ok(()) => {
                Self::changed(host.as_ref(), &parent);
                toast_event(
                  ToastTone::Success,
                  "Restored",
                  format!("Restored commit {short}"),
                )
              }
              Err(err) => toast_event(ToastTone::Error, "Undo Failed", err),
            };
            tell(host.as_ref(), event);
          });
          tell(
            view.host.as_ref(),
            NativeEvent::Toast {
              title: "Commit deleted".into(),
              description: format!("Abandoned commit {}", commit.short_id),
              tone: ToastTone::Success,
              action: Some(ToastAction {
                label: "Undo".into(),
                id: undo,
              }),
            },
          );
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to delete commit", err),
      },
      cx,
    );
  }

  fn stash(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let change = commit.change_id.clone();
    self.run(
      move |host, parent| host.stash(&parent.repo_path, parent.workspace_id(), &change),
      move |view, result, cx| match result {
        Ok(short) => {
          view.remove_later(commit.commit_id.clone(), cx);
          view.field("commits.stashed", serde_json::Value::Null, cx);
          Self::changed(view.host.as_ref(), &view.parent);
          view.toast(
            ToastTone::Success,
            "Commit stashed",
            format!("Parked {short} — apply it onto another workspace from Stashed Changes"),
          );
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to stash commit", err),
      },
      cx,
    );
  }

  fn undo(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let Some(ws) = self.parent.workspace_id() else {
      return;
    };
    let message = format!(
      "Undo commit {} — {}? Its changes will be returned to your working copy.",
      commit.short_id,
      first_line(&commit)
    );
    let change = commit.change_id.clone();
    self.confirmed(
      "Undo Commit",
      message,
      move |host, parent| host.undo_commit(&parent.repo_path, ws, &change),
      move |view, result, cx| match result {
        Ok(()) => {
          view.remove_later(commit.commit_id.clone(), cx);
          Self::changed(view.host.as_ref(), &view.parent);
          view.toast(
            ToastTone::Success,
            "Commit undone",
            format!("Undid commit {}", commit.short_id),
          );
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to undo commit", err),
      },
      cx,
    );
  }

  fn revert(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let Some(ws) = self.parent.workspace_id() else {
      return;
    };
    let message = format!(
      "Revert commit {} — {}? This creates a new commit that reverses its changes.",
      commit.short_id,
      first_line(&commit)
    );
    let change = commit.change_id.clone();
    self.confirmed(
      "Revert Commit",
      message,
      move |host, parent| host.revert(&parent.repo_path, ws, &change),
      move |view, result, _| match result {
        Ok(()) => {
          Self::changed(view.host.as_ref(), &view.parent);
          view.toast(
            ToastTone::Success,
            "Commit reverted",
            format!("Reverted commit {}", commit.short_id),
          );
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to revert commit", err),
      },
      cx,
    );
  }

  /// `handleShiftToNow`; from the timestamp dialog, closes it on success.
  fn shift_to_now(&mut self, from_dialog: bool, cx: &mut Context<Self>) {
    let Some(ws) = self.parent.workspace_id() else {
      return;
    };
    if from_dialog {
      match &mut self.dialog {
        Some(Dialog::Timestamp { saving, error, .. }) if !*saving => {
          *saving = true;
          *error = None;
        }
        _ => return,
      }
    } else if self.shifting {
      return;
    } else {
      self.shifting = true;
    }
    cx.notify();
    self.run(
      move |host, parent| host.shift_to_now(&parent.repo_path, ws),
      move |view, result, _| {
        view.shifting = false;
        match result {
          Ok(()) => {
            view.toast(
              ToastTone::Success,
              "Timestamps updated",
              "Mutable commits on this branch now end at the current time",
            );
            Self::changed(view.host.as_ref(), &view.parent);
            if from_dialog {
              view.dialog = None;
            }
          }
          Err(err) => {
            if let Some(Dialog::Timestamp { saving, error, .. }) = &mut view.dialog {
              *saving = false;
              *error = Some(err.clone());
            }
            view.toast(ToastTone::Error, "Failed to shift timestamps", err);
          }
        }
      },
      cx,
    );
  }

  /// `handleDeleteTentativeChanges`, with an Undo on its toast.
  fn delete_tentative(&mut self, cx: &mut Context<Self>) {
    if self.parent.workspace.is_none() {
      return;
    }
    self.run(
      |host, parent| {
        let path = parent.working_directory();
        let snapshot = host.snapshot_working_copy(&path)?;
        host.discard_all(&path)?;
        Ok::<_, String>((path, snapshot))
      },
      |view, result, _| match result {
        Ok((path, snapshot)) => {
          view.hide_tentative = true;
          let (host, parent) = (view.host.clone(), view.parent.clone());
          Self::changed(host.as_ref(), &parent);
          let undo = native::register_toast_action(move || {
            let event = match host.restore_snapshot(&path, &snapshot) {
              Ok(()) => {
                Self::changed(host.as_ref(), &parent);
                toast_event(
                  ToastTone::Success,
                  "Restored",
                  "Working copy changes were restored",
                )
              }
              Err(err) => toast_event(ToastTone::Error, "Undo Failed", err),
            };
            tell(host.as_ref(), event);
          });
          tell(
            view.host.as_ref(),
            NativeEvent::Toast {
              title: "Changes discarded".into(),
              description: "Working copy changes were removed".into(),
              tone: ToastTone::Success,
              action: Some(ToastAction {
                label: "Undo".into(),
                id: undo,
              }),
            },
          );
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to discard changes", err),
      },
      cx,
    );
  }

  fn move_commit(&mut self, commit: &JjLogCommit, existing: bool, cx: &mut Context<Self>) {
    self.move_menu = None;
    let field = if existing {
      "commits.moveToExisting"
    } else {
      "commits.moveToNew"
    };
    self.field(field, serde_json::to_value(commit).unwrap_or_default(), cx);
    cx.notify();
  }

  // --- Comments on diff lines ---

  fn file_diff(&self, commit_id: &str, path: &str) -> Option<&JjFileDiff> {
    self
      .diffs
      .get(commit_id)?
      .diff
      .as_ref()?
      .hunks_by_file
      .iter()
      .find(|f| f.path == path)
  }

  /// `handleLineMouseDown`: starts a selection, or extends it with Shift.
  pub(super) fn press_line(
    &mut self,
    commit_id: &str,
    path: &str,
    at: LineRef,
    shift: bool,
    cx: &mut Context<Self>,
  ) {
    let same_file = |s: &Selection| s.commit_id == commit_id && s.path == path;
    if shift {
      if let Some(anchor) = self
        .selection
        .as_ref()
        .filter(|s| same_file(s))
        .map(|s| s.anchor)
      {
        if let Some(file) = self.file_diff(commit_id, path) {
          let lines = selection(&file.hunks, anchor, at);
          if let Some(s) = self.selection.as_mut() {
            s.lines = lines;
          }
        }
        self.comment = None;
        cx.notify();
        return;
      }
    }
    self.selection = Some(Selection {
      commit_id: commit_id.into(),
      path: path.into(),
      anchor: at,
      lines: vec![at],
      dragging: true,
    });
    self.comment = None;
    cx.notify();
  }

  /// `handleLineMouseEnter` while dragging.
  pub(super) fn drag_to(
    &mut self,
    commit_id: &str,
    path: &str,
    at: LineRef,
    cx: &mut Context<Self>,
  ) {
    let Some(s) = self.selection.as_ref() else {
      return;
    };
    if !s.dragging || s.commit_id != commit_id || s.path != path {
      return;
    }
    let anchor = s.anchor;
    let Some(lines) = self
      .file_diff(commit_id, path)
      .map(|f| selection(&f.hunks, anchor, at))
    else {
      return;
    };
    if let Some(s) = self.selection.as_mut() {
      if s.lines != lines {
        s.lines = lines;
        cx.notify();
      }
    }
  }

  fn end_drag(&mut self, cx: &mut Context<Self>) {
    if let Some(s) = self.selection.as_mut().filter(|s| s.dragging) {
      s.dragging = false;
      cx.notify();
    }
  }

  /// `handleAddComment`: the selection when it spans lines, else this line.
  pub(super) fn add_comment(
    &mut self,
    commit: &JjLogCommit,
    path: &str,
    at: LineRef,
    cx: &mut Context<Self>,
  ) {
    let lines = match &self.selection {
      Some(s) if s.commit_id == commit.commit_id && s.path == path && s.lines.len() > 1 => {
        s.lines.clone()
      }
      _ => vec![at],
    };
    let Some(target) = self
      .file_diff(&commit.commit_id, path)
      .and_then(|f| comment_target(f, &lines))
    else {
      return;
    };
    self.comment = Some(Comment {
      commit_id: commit.commit_id.clone(),
      short_id: commit.short_id.clone(),
      target,
      input: None,
      text: String::new(),
    });
    cx.notify();
  }

  fn cancel_comment(&mut self, cx: &mut Context<Self>) {
    self.comment = None;
    cx.notify();
  }

  /// `handleCreateAgentWithComment`: a new agent session with the comment.
  pub(super) fn send_comment(&mut self, plan: bool, cx: &mut Context<Self>) {
    let Some(comment) = &self.comment else {
      return;
    };
    let text = comment.text.trim().to_string();
    if text.is_empty() {
      return;
    }
    let prompt = comment_prompt(&comment.target, &text, &comment.short_id);
    self.comment = None;
    self.selection = None;
    cx.notify();
    let mode = if plan { "plan" } else { "acceptEdits" };
    self.run(
      move |host, parent| {
        let name = "Code Comment";
        let id = host.create_session(&parent.repo_path, parent.workspace_id(), name)?;
        let repo = if parent.repo_path.is_empty() {
          parent.working_directory()
        } else {
          parent.repo_path.clone()
        };
        Ok::<_, String>(json!({
          "sessionId": id,
          "sessionName": name,
          "workspaceId": parent.workspace_id(),
          "workspacePath": null,
          "repoPath": repo,
          "pendingPrompt": prompt,
          "permissionMode": mode,
        }))
      },
      |view, result, cx| match result {
        Ok(session) => {
          view.field("code.sessionCreated", session, cx);
          view.toast(
            ToastTone::Success,
            "Comment sent to agent",
            "Created new agent session and sent comment",
          );
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to create agent", err),
      },
      cx,
    );
  }

  // --- Dialogs ---

  pub(super) fn edit_description(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let Some(ws) = self.parent.workspace_id() else {
      return;
    };
    let change = commit.change_id.clone();
    self.dialog = Some(Dialog::Describe {
      commit,
      loaded: None,
      input: None,
      text: String::new(),
      saving: false,
    });
    cx.notify();
    self.run(
      move |host, parent| host.commit_description(&parent.repo_path, ws, &change),
      |view, result, _| {
        // The editor shows once the description is read, holding it.
        if let Some(Dialog::Describe { loaded, text, .. }) = &mut view.dialog {
          *text = result.clone().unwrap_or_default();
          *loaded = Some(result);
        }
      },
      cx,
    );
  }

  fn save_description(&mut self, cx: &mut Context<Self>) {
    let Some(ws) = self.parent.workspace_id() else {
      return;
    };
    let Some(Dialog::Describe {
      commit,
      loaded: Some(loaded),
      text,
      saving,
      ..
    }) = &mut self.dialog
    else {
      return;
    };
    if *saving || Some(&*text) == loaded.as_ref().ok() {
      return;
    }
    *saving = true;
    cx.notify();
    let (change, short, description) = (
      commit.change_id.clone(),
      commit.short_id.clone(),
      text.clone(),
    );
    self.run(
      move |host, parent| host.describe(&parent.repo_path, ws, &change, &description),
      move |view, result, _| match result {
        Ok(()) => {
          view.dialog = None;
          view.toast(
            ToastTone::Success,
            "Description updated",
            format!("Updated commit {short}"),
          );
          Self::changed(view.host.as_ref(), &view.parent);
        }
        Err(err) => {
          if let Some(Dialog::Describe { saving, .. }) = &mut view.dialog {
            *saving = false;
          }
          view.toast(ToastTone::Error, "Failed to update description", err);
        }
      },
      cx,
    );
  }

  pub(super) fn edit_timestamp(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    self.dialog = Some(Dialog::Timestamp {
      commit,
      form: ShiftForm::default(),
      inputs: None,
      saving: false,
      error: None,
    });
    cx.notify();
  }

  fn set_form(&mut self, update: impl FnOnce(&mut ShiftForm), cx: &mut Context<Self>) {
    if let Some(Dialog::Timestamp { form, .. }) = &mut self.dialog {
      update(form);
      cx.notify();
    }
  }

  fn apply_timestamp(&mut self, cx: &mut Context<Self>) {
    let Some(ws) = self.parent.workspace_id() else {
      return;
    };
    let Some(Dialog::Timestamp {
      commit,
      form,
      saving,
      error,
      ..
    }) = &mut self.dialog
    else {
      return;
    };
    let Some(shift) = form.shift().filter(|_| !*saving) else {
      return;
    };
    *saving = true;
    *error = None;
    cx.notify();
    let (change, short) = (commit.change_id.clone(), commit.short_id.clone());
    self.run(
      move |host, parent| host.shift_timestamp(&parent.repo_path, ws, &change, &shift),
      move |view, result, _| match result {
        Ok(()) => {
          view.dialog = None;
          view.toast(
            ToastTone::Success,
            "Timestamp updated",
            format!("Updated commit {short} and repaired later commits on this lineage"),
          );
          Self::changed(view.host.as_ref(), &view.parent);
        }
        Err(err) => {
          if let Some(Dialog::Timestamp { saving, error, .. }) = &mut view.dialog {
            *saving = false;
            *error = Some(err.clone());
          }
          view.toast(ToastTone::Error, "Failed to update timestamp", err);
        }
      },
      cx,
    );
  }

  /// Opens the resolve dialog, preparing its resolve directories meanwhile.
  pub(super) fn open_resolve(&mut self, change_ids: Vec<String>, cx: &mut Context<Self>) {
    self.dialog = Some(Dialog::Resolve {
      change_ids: change_ids.clone(),
      session: None,
      input: None,
      text: String::new(),
      submitting: false,
    });
    cx.notify();
    let ids = change_ids.clone();
    self.run(
      move |host, parent| host.start_resolve(&parent.repo_path, parent.workspace_id(), Some(ids)),
      move |view, result, _| {
        if let Err(err) = &result {
          view.toast(
            ToastTone::Error,
            "Failed to prepare resolve workspaces",
            err.clone(),
          );
        }
        if let Some(Dialog::Resolve {
          change_ids: open,
          session,
          ..
        }) = &mut view.dialog
        {
          if *open == change_ids {
            *session = Some(result);
          }
        }
      },
      cx,
    );
  }

  fn submit_resolve(&mut self, cx: &mut Context<Self>) {
    let Some(Dialog::Resolve {
      session: Some(Ok(session)),
      text,
      submitting,
      ..
    }) = &mut self.dialog
    else {
      return;
    };
    if *submitting {
      return;
    }
    *submitting = true;
    cx.notify();
    let (session, prompt) = (session.clone(), text.trim().to_string());
    self.run(
      move |host, parent| {
        let full = host.resolve_prompt(&prompt, &session);
        let keys = ["default_agent".to_string()];
        let get = |values: Result<super::settings_data::Values, String>| {
          values
            .ok()
            .and_then(|v| v.get("default_agent").cloned().flatten())
        };
        let agent = get(host.repo_settings(&parent.repo_path, &keys))
          .filter(|v| !v.is_empty())
          .or_else(|| get(host.settings(&keys)));
        let agent = Agent::parse(agent.as_deref()).unwrap_or_default();
        let primary = session
          .targets
          .first()
          .ok_or("No resolve targets were created")?;
        let name = resolve_session_name(&session).unwrap_or_default();
        let workspace_id = session.source_workspace_id.unwrap_or(primary.workspace_id);
        let id = host.create_session(&parent.repo_path, Some(workspace_id), &name)?;
        let count = session.targets.len();
        Ok::<_, String>((
          json!({
            "sessionId": id,
            "sessionName": name,
            "workspaceId": workspace_id,
            "workspacePath": session.agent_cwd,
            "repoPath": parent.repo_path,
            "pendingPrompt": full,
            "permissionMode": "acceptEdits",
            "agent": agent.id(),
          }),
          count,
        ))
      },
      |view, result, cx| match result {
        Ok((session, count)) => {
          view.dialog = None;
          view.field("code.sessionCreated", session, cx);
          view.toast(
            ToastTone::Success,
            "Resolve session started",
            format!(
              "{count} resolve director{} ready for the agent",
              if count == 1 { "y" } else { "ies" }
            ),
          );
        }
        Err(err) => {
          if let Some(Dialog::Resolve { submitting, .. }) = &mut view.dialog {
            *submitting = false;
          }
          view.toast(ToastTone::Error, "Failed to start resolve", err);
        }
      },
      cx,
    );
  }

  fn close_dialog(&mut self, cx: &mut Context<Self>) {
    let busy = match &self.dialog {
      Some(Dialog::Describe { saving, .. } | Dialog::Timestamp { saving, .. }) => *saving,
      Some(Dialog::Resolve { submitting, .. }) => *submitting,
      None => false,
    };
    if !busy {
      self.dialog = None;
      cx.notify();
    }
  }

  /// Mirrors a text field's value into the view as it is typed.
  fn textarea(
    &mut self,
    placeholder: &str,
    rows: usize,
    value: &str,
    on_change: fn(&mut Self, String),
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Entity<TextareaState> {
    let state = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(rows)
        .placeholder(placeholder.to_string())
        .default_value(value.to_string())
    });
    self._subscriptions.push(
      cx.subscribe(&state, move |view, state, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          on_change(view, state.read(cx).value().to_string());
          cx.notify();
        }
      }),
    );
    state.update(cx, |s, cx| s.focus(window, cx));
    state
  }

  fn input(
    &mut self,
    value: &str,
    placeholder: &str,
    on_change: fn(&mut ShiftForm, String),
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Entity<InputState> {
    let state = cx.new(|cx| {
      InputState::new(window, cx)
        .placeholder(placeholder.to_string())
        .default_value(value.to_string())
    });
    self._subscriptions.push(
      cx.subscribe(&state, move |view, state, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          let value = state.read(cx).value().to_string();
          view.set_form(|form| on_change(form, value), cx);
        }
      }),
    );
    state
  }

  /// Creates the open dialog's and comment box's text fields, which need
  /// the window.
  fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.comment.as_ref().is_some_and(|c| c.input.is_none()) {
      let input = self.textarea(
        "Add a comment...",
        3,
        "",
        |view, text| {
          if let Some(c) = view.comment.as_mut() {
            c.text = text;
          }
        },
        window,
        cx,
      );
      if let Some(c) = self.comment.as_mut() {
        c.input = Some(input);
      }
    }
    match &self.dialog {
      Some(Dialog::Describe {
        input: None,
        loaded: Some(_),
        text,
        ..
      }) => {
        let text = text.clone();
        let state = self.textarea(
          "Describe this commit...",
          7,
          &text,
          |view, value| {
            if let Some(Dialog::Describe { text, .. }) = &mut view.dialog {
              *text = value;
            }
          },
          window,
          cx,
        );
        if let Some(Dialog::Describe { input, .. }) = &mut self.dialog {
          *input = Some(state);
        }
      }
      Some(Dialog::Resolve { input: None, .. }) => {
        let state = self.textarea(
          "e.g. Prefer the workspace-side changes in README, keep both imports in lib.rs…",
          5,
          "",
          |view, value| {
            if let Some(Dialog::Resolve { text, .. }) = &mut view.dialog {
              *text = value;
            }
          },
          window,
          cx,
        );
        if let Some(Dialog::Resolve { input, .. }) = &mut self.dialog {
          *input = Some(state);
        }
      }
      Some(Dialog::Timestamp {
        inputs: None, form, ..
      }) => {
        let form = form.clone();
        let inputs = [
          self.input(&form.days, "", |f, v| f.days = v, window, cx),
          self.input(&form.hours, "", |f, v| f.hours = v, window, cx),
          self.input(&form.minutes, "", |f, v| f.minutes = v, window, cx),
          self.input(&form.day, "yyyy-mm-dd", |f, v| f.day = v, window, cx),
        ];
        if let Some(Dialog::Timestamp { inputs: slot, .. }) = &mut self.dialog {
          *slot = Some(inputs);
        }
      }
      _ => {}
    }
  }

  /// Scrolls to and opens the commit React asked for, once it is drawn.
  fn scroll_to_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Some(change) = self.scroll_to.clone() else {
      return;
    };
    let Some(history) = &self.history else {
      return;
    };
    let commit = history
      .own
      .iter()
      .filter(|c| !self.removed.contains(&c.commit_id))
      .chain(history.tentative.iter().map(|(c, _)| c))
      .find(|c| c.change_id == change)
      .cloned();
    let Some(commit) = commit else {
      return;
    };
    let bounds = self.rows.borrow().get(&commit.commit_id).copied();
    match bounds {
      Some(row) => {
        let view = self.scroll.bounds();
        let offset = self.scroll.offset();
        self
          .scroll
          .set_offset(point(offset.x, offset.y - (row.top() - view.top())));
        self.scroll_to = None;
        self.expand(&commit.commit_id, cx);
        self.field("commits.scrolled", serde_json::Value::Null, cx);
        cx.notify();
      }
      // Not drawn yet: try again once it is.
      None => cx.on_next_frame(window, |_, _, cx| cx.notify()),
    }
  }
}

/// `ui/dialog.tsx`: the backdrop over the whole window and the panel.
pub(super) fn dialog_panel(
  id: &'static str,
  max_width: f32,
  viewport: gpui::Size<Pixels>,
  p: &Palette,
) -> (Stateful<Div>, Div) {
  let backdrop = div()
    .id(SharedString::from(format!("{id}-backdrop")))
    .w(viewport.width)
    .h(viewport.height)
    .p_4()
    .flex()
    .items_center()
    .justify_center()
    .bg(gpui::black().opacity(0.4))
    .occlude();
  let panel = div()
    .debug_selector(move || id.into())
    .w_full()
    .max_w(px(max_width))
    .p_6()
    .rounded(px(16.))
    .border_1()
    .border_color(p.border.opacity(0.5))
    .bg(p.popover)
    .text_color(p.popover_foreground)
    .shadow_2xl();
  (backdrop, panel)
}

pub(super) fn dialog_title(title: impl IntoElement) -> Div {
  ui::text(div(), TextSize::Lg)
    .line_height(px(15.))
    .font_weight(FontWeight::SEMIBOLD)
    .child(title)
}

pub(super) fn dialog_description(text: impl Into<SharedString>, p: &Palette) -> Div {
  ui::text(div(), TextSize::Sm)
    .text_color(p.muted_foreground)
    .child(text.into())
}

/// A pill badge (`px-1.5 py-0.5 rounded-full border font-medium`).
fn badge(text: &'static str, fg: Hsla, bg: Hsla, border: Hsla) -> Div {
  div()
    .px(px(6.))
    .py(px(2.))
    .rounded_full()
    .bg(bg)
    .text_color(fg)
    .border_1()
    .border_color(border)
    .font_weight(FontWeight::MEDIUM)
    .child(text)
}

/// A `DropdownMenuItem`.
fn menu_item(id: impl Into<SharedString>, label: &'static str, p: &Palette) -> Stateful<Div> {
  let id: SharedString = id.into();
  let selector = id.to_string();
  let hover = p.menu_accent;
  ui::text(div(), TextSize::Sm)
    .id(id)
    .debug_selector(move || selector)
    .px_2()
    .py(px(6.))
    .rounded(px(6.))
    .whitespace_nowrap()
    .cursor_pointer()
    .hover(move |s| s.bg(hover))
    .child(label)
}

impl Render for CommitsView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    self.ensure_inputs(window, cx);
    self.scroll_to_commit(window, cx);
    let p = theme::palette(self.dark);
    let root = ui::text(div(), TextSize::Sm)
      .debug_selector(|| "workspace-commits".into())
      .size_full()
      .bg(p.background)
      .text_color(p.foreground)
      .on_mouse_up(
        MouseButton::Left,
        cx.listener(|view, _, _, cx| view.end_drag(cx)),
      )
      .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|view, _, _, cx| view.end_drag(cx)),
      );
    let Some(history) = self.history.clone() else {
      return root.child(
        ui::text(div(), TextSize::Sm)
          .size_full()
          .flex()
          .items_center()
          .justify_center()
          .p_4()
          .child(
            div()
              .mr_2()
              .child(ui::spinner("commits-loading", 16., p.foreground)),
          )
          .child(
            div()
              .text_color(p.muted_foreground)
              .child("Loading commits..."),
          ),
      );
    };
    let home = self.home();
    let own: Vec<JjLogCommit> = history
      .own
      .iter()
      .filter(|c| !self.removed.contains(&c.commit_id))
      .cloned()
      .collect();
    let tentative = history
      .tentative
      .clone()
      .filter(|_| !home && !self.hide_tentative);
    let shows_target = history.shows_target();
    let visible = History {
      own: own.clone(),
      ..history.clone()
    };
    if visible.is_empty() {
      return root.child(
        ui::text(div(), TextSize::Sm)
          .p_4()
          .flex()
          .flex_col()
          .items_center()
          .text_color(p.muted_foreground)
          .child("No commits yet.")
          .child("Changes you commit will appear here."),
      );
    }
    let conflicted = visible.resolvable(home);
    let mut list = div().relative().child(
      div()
        .absolute()
        .left(px(7.))
        .top_2()
        .bottom_2()
        .w(px(2.))
        .bg(p.border),
    );
    if let Some((commit, label)) = &tentative {
      list = list.child(div().mb_3().child(self.commit(
        commit,
        Kind::Tentative,
        Some(label),
        false,
        &p,
        cx,
      )));
    }
    let mut first = true;
    for (label, commits) in group_by_day(&own) {
      let mut group = div().mt_5().child(
        ui::text(div(), TextSize::Sm)
          .mb_1()
          .pl_7()
          .text_color(p.muted_foreground)
          .child(label),
      );
      for commit in &commits {
        let resolvable = conflicted.iter().any(|c| c.change_id == commit.change_id);
        group = group.child(self.commit(commit, Kind::Own { first }, None, resolvable, &p, cx));
        first = false;
      }
      list = list.child(group);
    }
    let load_more = |id: &'static str, target: bool, view: &Self, cx: &mut Context<Self>| {
      let muted = p.muted_foreground;
      let fg = p.foreground;
      div().mt_3().pl_7().child(
        ui::text(div(), TextSize::Sm)
          .id(id)
          .debug_selector(move || id.into())
          .text_color(muted)
          .when(!view.loading_more, |this| {
            this
              .cursor_pointer()
              .hover(move |s| s.text_color(fg))
              .on_click(cx.listener(move |view, _, _, cx| view.load_more(target, cx)))
          })
          .when(view.loading_more, |this| {
            this
              .flex()
              .items_center()
              .gap(px(6.))
              .child(ui::spinner(
                SharedString::from(format!("{id}-spin")),
                12.,
                muted,
              ))
              .child("Loading...")
          })
          .when(!view.loading_more, |this| this.child("Load more commits")),
      )
    };
    if home && own.len() + 1 >= self.home_limit {
      list = list.child(load_more("commits-load-more", false, self, cx));
    }
    if shows_target {
      if own.is_empty() {
        list = list.child(
          div()
            .mb_3()
            .pl_7()
            .text_color(p.muted_foreground)
            .child(if home {
              "There are no commits on this branch yet."
            } else {
              "There are no commits within this workspace branch yet."
            }),
        );
      }
      list = list
        // `border-t`, drawn as a rule: an empty div paints no border.
        .child(div().h(px(1.)).bg(p.border).my_4().mx_2())
        .child(
          ui::text(div(), TextSize::Sm)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.muted_foreground)
            .mb_2()
            .pl_7()
            .child(format!("Recent on {}", history.target_branch)),
        );
      for (label, commits) in group_by_day(&history.target) {
        let mut group = div().mt_5().child(
          ui::text(div(), TextSize::Sm)
            .mb_1()
            .pl_7()
            .text_color(p.muted_foreground)
            .child(label),
        );
        for commit in &commits {
          group = group.child(self.commit(commit, Kind::Target, None, false, &p, cx));
        }
        list = list.child(group);
      }
      if history.target.len() >= self.target_limit {
        list = list.child(load_more("commits-target-load-more", true, self, cx));
      }
    }
    let mut body = div().p_4();
    if !home && visible.has_mutable() {
      body = body.child(
        div().mb_3().flex().justify_end().child(
          Button::new("shift-commits-to-now", &p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .gap(px(6.))
            .disabled(self.shifting)
            .child(if self.shifting {
              ui::spinner("shift-spin", 16., p.foreground).into_any_element()
            } else {
              ui::icon(theme::ICON_CLOCK, 16., p.foreground).into_any_element()
            })
            .label("Shift to now")
            .on_click(cx.listener(|view, _, _, cx| view.shift_to_now(false, cx))),
        ),
      );
    }
    if !conflicted.is_empty() {
      let ids: Vec<String> = conflicted.iter().map(|c| c.change_id.clone()).collect();
      let n = conflicted.len();
      body = body.child(
        div()
          .debug_selector(|| "resolve-conflicts-banner".into())
          .mb_4()
          .flex()
          .items_center()
          .justify_between()
          .gap_3()
          .rounded(px(8.))
          .border_1()
          .border_color(p.destructive.opacity(0.4))
          .bg(p.destructive.opacity(0.05))
          .px_3()
          .py_2()
          .child(
            div()
              .min_w_0()
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_weight(FontWeight::MEDIUM)
                  .text_color(p.destructive)
                  .child(format!(
                    "{n} conflicted commit{}",
                    if n == 1 { "" } else { "s" }
                  )),
              )
              .child(
                ui::text(div(), TextSize::Xs)
                  .text_color(p.muted_foreground)
                  .child("Resolve in place via a short-lived resolve workspace — no extra commit."),
              ),
          )
          .child(
            Button::new("resolve-conflicts-button", &p)
              .variant(Variant::Destructive)
              .size(Size::Sm)
              .gap(px(6.))
              .icon(theme::ICON_MERGE, 16.)
              .label("Resolve conflicts…")
              .on_click(cx.listener(move |view, _, _, cx| view.open_resolve(ids.clone(), cx))),
          ),
      );
    }
    let dialog = self.dialog_view(window, &p, cx);
    root
      .child(
        div()
          .id("commits-scroll")
          .size_full()
          .overflow_y_scroll()
          .track_scroll(&self.scroll)
          .child(body.child(list)),
      )
      .children(dialog)
  }
}

impl CommitsView {
  #[allow(clippy::too_many_arguments)]
  fn commit(
    &mut self,
    commit: &JjLogCommit,
    kind: Kind,
    label: Option<&String>,
    resolvable: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let tentative = kind == Kind::Tentative;
    let first = matches!(kind, Kind::Own { first: true }) || tentative;
    let expanded = self.expanded.contains(&commit.commit_id);
    let dot = if tentative {
      tw::color(tw::YELLOW_500)
    } else if first {
      p.primary
    } else {
      p.muted_foreground
    };
    let (yellow, yellow_hover) = if self.dark {
      (
        tw::color(tw::YELLOW_950).opacity(0.2),
        tw::color(tw::YELLOW_950).opacity(0.3),
      )
    } else {
      (
        tw::color(tw::YELLOW_50).opacity(0.8),
        tw::color(tw::YELLOW_100).opacity(0.8),
      )
    };
    let ts = commit.timestamp.clone();
    let time = self.tips.wrap(
      format!("commits-time-{}", commit.commit_id),
      Side::Top,
      Some(TooltipContent::Text(time::full(&ts).into())),
      div()
        .text_color(p.muted_foreground)
        .child(time::relative(&ts)),
      cx,
    );
    let mut title = div().min_w_0().flex_1().truncate().child(headline(commit));
    if let Some(label) = label.filter(|l| !l.is_empty()) {
      // The workspace's label in mono after the headline.
      title = div()
        .min_w_0()
        .flex_1()
        .flex()
        .overflow_hidden()
        .child(div().flex_none().child(headline(commit)))
        .child(
          div()
            .ml_2()
            .truncate()
            .font_family(theme::MONO_FONT)
            .text_color(p.muted_foreground)
            .child(format!("- {label}")),
        );
    }
    let id = commit.commit_id.clone();
    let rows = self.rows.clone();
    let hover = if tentative { yellow_hover } else { p.muted };
    let row = div()
      .id(SharedString::from(format!(
        "commit-row-{}",
        commit.commit_id
      )))
      .debug_selector({
        let id = format!("commit-row-{}", commit.commit_id);
        move || id
      })
      .relative()
      .flex()
      .items_start()
      .gap_3()
      .py_2()
      .px_2()
      .mx_neg_2()
      .w_full()
      .rounded(px(8.))
      .cursor_pointer()
      .when(tentative, |this| this.bg(yellow))
      .hover(move |s| s.bg(hover))
      .child(
        div().relative().flex_none().child(
          div()
            .size(px(14.))
            .rounded_full()
            .border_2()
            .border_color(p.background)
            .bg(dot),
        ),
      )
      .child(
        div()
          .flex_1()
          .min_w_0()
          .pt(px(2.))
          .child(
            div()
              .flex()
              .items_center()
              .gap(px(6.))
              .child(ui::icon(
                if expanded {
                  theme::ICON_CHEVRON_DOWN
                } else {
                  theme::ICON_CHEVRON_RIGHT
                },
                16.,
                p.muted_foreground,
              ))
              .child(title),
          )
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .mt(px(2.))
              .pl_5()
              .when(commit.has_conflicts, |this| {
                this.child(
                  badge(
                    "Conflict",
                    p.destructive,
                    p.destructive.opacity(0.1),
                    p.destructive.opacity(0.3),
                  )
                  .debug_selector({
                    let id = format!("commit-conflict-badge-{}", commit.change_id);
                    move || id
                  }),
                )
              })
              .when(commit.is_immutable, |this| {
                this.child(badge("Immutable", p.muted_foreground, p.muted, p.border))
              })
              .child(time)
              .when(commit.insertions > 0 || commit.deletions > 0, |this| {
                this.child(
                  div()
                    .ml_auto()
                    .flex()
                    .child(
                      div()
                        .text_color(tw::color(tw::GREEN_600))
                        .child(format!("+{}", commit.insertions)),
                    )
                    .child("\u{a0}")
                    .child(
                      div()
                        .text_color(tw::color(tw::RED_600))
                        .child(format!("-{}", commit.deletions)),
                    ),
                )
              }),
          ),
      )
      .child(
        gpui::canvas(
          move |bounds, _, _| {
            rows.borrow_mut().insert(id.clone(), bounds);
          },
          |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
      )
      .on_click({
        let id = commit.commit_id.clone();
        cx.listener(move |view, _, _, cx| view.toggle(&id, cx))
      });
    div()
      .debug_selector({
        let id = format!("commit-{}", commit.commit_id);
        move || id
      })
      .child(row)
      .when(expanded, |this| {
        this.child(self.expanded_commit(commit, kind, resolvable, p, cx))
      })
  }

  fn expanded_commit(
    &mut self,
    commit: &JjLogCommit,
    kind: Kind,
    resolvable: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let removing = self.removing.contains(&commit.commit_id);
    let outline = |id: String, icon: &'static str, label: &'static str| {
      Button::new(id, p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .gap(px(6.))
        .icon(icon, 16.)
        .label(label)
    };
    let destructive_outline = |id: String, label: &'static str| {
      let red = p.destructive;
      Button::new(id, p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .gap(px(6.))
        .border_color(red.opacity(0.3))
        .text_color(red)
        .hover_bg(red.opacity(0.1))
        .disabled(removing)
        .child(ui::icon(theme::ICON_TRASH, 16., red))
        .child(label)
    };
    let short = &commit.change_id;
    let move_label = if kind == Kind::Tentative {
      "Move changes"
    } else {
      "Move commit"
    };
    let move_open = self.move_menu.as_deref() == Some(&commit.commit_id);
    let move_button = div()
      .relative()
      .child(
        outline(
          format!("move-commit-{short}"),
          theme::ICON_ARROW_RIGHT_LEFT,
          move_label,
        )
        .on_click({
          let id = commit.commit_id.clone();
          cx.listener(move |view, _, _, cx| {
            view.move_menu = if view.move_menu.as_deref() == Some(&id) {
              None
            } else {
              Some(id.clone())
            };
            cx.notify();
          })
        }),
      )
      .when(move_open, |this| {
        let (to_new, to_existing) = (commit.clone(), commit.clone());
        let panel = div()
          .id("move-commit-menu")
          .debug_selector(|| "move-commit-menu".into())
          .absolute()
          .top(px(36.))
          .left_0()
          .min_w(px(128.))
          .p_1()
          .rounded(px(8.))
          .border_1()
          .border_color(p.border)
          .bg(p.popover)
          .text_color(p.popover_foreground)
          .shadow_md()
          .occlude()
          .child(
            menu_item("move-commit-new", "Move to New Workspace", p)
              .on_click(cx.listener(move |view, _, _, cx| view.move_commit(&to_new, false, cx))),
          )
          .child(
            menu_item("move-commit-existing", "Move to Existing Workspace", p).on_click(
              cx.listener(move |view, _, _, cx| view.move_commit(&to_existing, true, cx)),
            ),
          )
          .on_mouse_down_out(cx.listener(|view, _, _, cx| {
            if view.move_menu.take().is_some() {
              cx.notify();
            }
          }));
        this.child(deferred(panel).with_priority(1))
      });
    let mut actions = div().flex().items_center().gap_2().mb_2().mt_2();
    if kind == Kind::Tentative {
      actions = actions
        .child(
          outline(
            "view-tentative-changes".into(),
            theme::ICON_FILE_TEXT,
            "View changes",
          )
          .on_click(cx.listener(|view, _, _, cx| {
            view.field("workspace.tab", "changes".into(), cx);
          })),
        )
        .child(move_button)
        .child(
          destructive_outline("delete-tentative-changes".into(), "Delete changes")
            .on_click(cx.listener(|view, _, _, cx| view.delete_tentative(cx))),
        );
    } else {
      let own = matches!(kind, Kind::Own { .. });
      let workspace = self.parent.workspace.is_some();
      actions = actions
        .when(commit.has_conflicts && resolvable, |this| {
          let ids = vec![commit.change_id.clone()];
          this.child(
            Button::new(format!("resolve-commit-{short}"), p)
              .variant(Variant::Destructive)
              .size(Size::Sm)
              .gap(px(6.))
              .icon(theme::ICON_MERGE, 16.)
              .label("Resolve…")
              .on_click(cx.listener(move |view, _, _, cx| view.open_resolve(ids.clone(), cx))),
          )
        })
        .when(own && workspace, |this| {
          this.child(
            outline(
              format!("edit-description-{short}"),
              theme::ICON_PENCIL,
              "Edit description",
            )
            .on_click(Self::on_commit(commit, Self::edit_description, cx)),
          )
        })
        .when(
          matches!(kind, Kind::Own { first: true }) && workspace,
          |this| {
            this.child(
              outline(
                format!("undo-commit-{short}"),
                theme::ICON_UNDO,
                "Undo commit",
              )
              .disabled(removing)
              .on_click(Self::on_commit(commit, Self::undo, cx)),
            )
          },
        )
        .when(workspace, |this| {
          this.child(
            outline(
              format!("revert-commit-{short}"),
              theme::ICON_ROTATE_CCW,
              "Revert commit",
            )
            .on_click(Self::on_commit(commit, Self::revert, cx)),
          )
        })
        .when(own && workspace && !commit.is_immutable, |this| {
          this.child(
            outline(
              format!("edit-timestamp-{short}"),
              theme::ICON_CLOCK,
              "Edit timestamp",
            )
            .on_click(Self::on_commit(commit, Self::edit_timestamp, cx)),
          )
        })
        .when(own, |this| {
          this
            .child(move_button)
            .child(
              outline(
                "stash-commit-button".into(),
                theme::ICON_ARCHIVE,
                "Stash commit",
              )
              .disabled(removing)
              .on_click(Self::on_commit(commit, Self::stash, cx)),
            )
            .when(workspace, |this| {
              this.child(
                destructive_outline(format!("delete-commit-{short}"), "Delete commit")
                  .on_click(Self::on_commit(commit, Self::abandon, cx)),
              )
            })
        });
    }
    let content = match self.diffs.get(&commit.commit_id) {
      Some(state) if state.loading => div()
        .flex()
        .items_center()
        .gap_2()
        .p_3()
        .text_color(p.muted_foreground)
        .child(ui::spinner(
          SharedString::from(format!("diff-spin-{}", commit.commit_id)),
          16.,
          p.muted_foreground,
        ))
        .child("Loading diff...")
        .into_any_element(),
      Some(DiffState {
        error: Some(err), ..
      }) => div()
        .p_3()
        .text_color(p.destructive)
        .child(format!("Failed to load diff: {err}"))
        .into_any_element(),
      Some(DiffState { diff: Some(_), .. }) => self.diff_content(commit, p, cx).into_any_element(),
      _ => div().into_any_element(),
    };
    div().ml_7().mb_3().child(actions).child(
      div()
        .border_1()
        .border_color(p.border)
        .rounded(px(8.))
        .overflow_hidden()
        .child(content),
    )
  }

  /// A click that runs `f` on `commit`.
  fn on_commit(
    commit: &JjLogCommit,
    f: fn(&mut Self, JjLogCommit, &mut Context<Self>),
    cx: &mut Context<Self>,
  ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let (c, view) = (commit.clone(), cx.entity().downgrade());
    move |_, _, cx| {
      let _ = view.update(cx, |view, cx| f(view, c.clone(), cx));
    }
  }

  /// `CommitDiffContent`.
  fn diff_content(&mut self, commit: &JjLogCommit, p: &Palette, cx: &mut Context<Self>) -> Div {
    let Some(state) = self.diffs.get(&commit.commit_id) else {
      return div();
    };
    let Some(diff) = state.diff.clone() else {
      return div();
    };
    if diff.too_large_to_render {
      return div().p_3().text_color(p.muted_foreground).child(
        diff
          .render_block_reason
          .clone()
          .unwrap_or_else(|| "This commit diff is too large to render.".into()),
      );
    }
    if diff.committed_files.is_empty() {
      return div()
        .p_3()
        .text_color(p.muted_foreground)
        .child("No changes");
    }
    let collapsed = state.collapsed.clone();
    let loading = state.deferred_loading.clone();
    let errors = state.deferred_errors.clone();
    let mut list = div();
    for (i, file) in diff.committed_files.iter().enumerate() {
      let file_diff = diff.hunks_by_file.iter().find(|f| f.path == file.path);
      let open = !collapsed.contains(&file.path);
      let mut el = div()
        .when(i > 0, |this| this.border_t_1().border_color(p.border))
        .child(self.file_row(commit, file, open, p, cx));
      if open {
        let mut body = div().bg(p.muted.opacity(0.2));
        match file_diff {
          Some(file_diff) => {
            for (h, hunk) in file_diff.hunks.iter().enumerate() {
              body = body.child(self.hunk(commit, &file.path, h, hunk, p, cx));
            }
          }
          None if file.diff_deferred => {
            let busy = loading.contains(&file.path);
            let (id, path) = (commit.commit_id.clone(), file.path.clone());
            let muted = p.muted_foreground;
            let (fg, hover) = (p.foreground, p.muted);
            body =
              body.child(
                div()
                  .flex()
                  .items_center()
                  .justify_between()
                  .gap_3()
                  .border_t_1()
                  .border_color(p.border)
                  .px_3()
                  .py_3()
                  .child(div().text_color(muted).child(format!(
                    "This diff has {} changed lines.",
                    file.changed_line_count
                  )))
                  .child(
                    div()
                      .flex()
                      .items_center()
                      .gap_3()
                      .when_some(errors.get(&file.path), |this, err| {
                        this.child(div().text_color(p.destructive).child(err.clone()))
                      })
                      .child(
                        div()
                          .id(SharedString::from(format!("load-diff-{}", file.path)))
                          .debug_selector(|| "load-deferred-diff".into())
                          .rounded(px(8.))
                          .border_1()
                          .border_color(p.border)
                          .px_2()
                          .py_1()
                          .text_color(muted)
                          .when(busy, |this| this.opacity(0.6))
                          .when(!busy, |this| {
                            this
                              .cursor_pointer()
                              .hover(move |s| s.bg(hover).text_color(fg))
                              .on_click(cx.listener(move |view, _, _, cx| {
                                view.load_file_diff(&id, &path, cx)
                              }))
                          })
                          .child(if busy { "Loading..." } else { "Load diff" }),
                      ),
                  ),
              );
          }
          None => {
            body = body.child(
              div()
                .border_t_1()
                .border_color(p.border)
                .px_3()
                .py_3()
                .text_color(p.muted_foreground)
                .child("No diff content available."),
            );
          }
        }
        el = el.child(body);
      }
      list = list.child(el);
    }
    list
  }

  fn file_row(
    &self,
    commit: &JjLogCommit,
    file: &JjFileChange,
    open: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let (fg, bg) = match file.status.as_str() {
      "A" => (
        tw::color(tw::GREEN_600),
        tw::color(tw::GREEN_500).opacity(0.1),
      ),
      "M" => (
        tw::color(tw::BLUE_600),
        tw::color(tw::BLUE_500).opacity(0.1),
      ),
      "D" => (tw::color(tw::RED_600), tw::color(tw::RED_500).opacity(0.1)),
      _ => (p.foreground, gpui::transparent_black()),
    };
    let hover = p.muted.opacity(0.4);
    let (id, path) = (commit.commit_id.clone(), file.path.clone());
    div()
      .id(SharedString::from(format!(
        "commit-file-{}-{}",
        commit.commit_id, file.path
      )))
      .debug_selector(|| "commit-file-row".into())
      .flex()
      .items_center()
      .gap_2()
      .px_3()
      .py(px(6.))
      .w_full()
      .cursor_pointer()
      .hover(move |s| s.bg(hover))
      .child(ui::icon(
        if open {
          theme::ICON_CHEVRON_DOWN
        } else {
          theme::ICON_CHEVRON_RIGHT
        },
        16.,
        p.muted_foreground,
      ))
      .child(ui::icon(theme::ICON_FILE_TEXT, 16., p.muted_foreground))
      .child(
        div()
          .flex_1()
          .min_w_0()
          .truncate()
          .font_family(theme::MONO_FONT)
          .child(file.path.clone()),
      )
      .child(
        div()
          .font_weight(FontWeight::MEDIUM)
          .px(px(6.))
          .py(px(2.))
          .rounded(px(4.))
          .text_color(fg)
          .bg(bg)
          .child(file.status.clone()),
      )
      .on_click(cx.listener(move |view, _, _, cx| view.toggle_file(&id, &path, cx)))
  }

  /// `HunkView`: the header, then each line with its numbers and the
  /// comment button, and the comment box under the line it is on.
  fn hunk(
    &mut self,
    commit: &JjLogCommit,
    path: &str,
    h: usize,
    hunk: &JjDiffHunk,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let numbers = line_numbers(hunk);
    let language = highlight::language(path);
    let selected: HashSet<LineRef> = self
      .selection
      .as_ref()
      .filter(|s| s.commit_id == commit.commit_id && s.path == path)
      .map(|s| s.lines.iter().copied().collect())
      .unwrap_or_default();
    let comment_at = self
      .comment
      .as_ref()
      .filter(|c| c.commit_id == commit.commit_id && c.target.file_path == path)
      .map(|c| c.target.at);
    let gutter_fg = p.muted_foreground.opacity(0.6);
    let gutter_hover = p.muted.opacity(0.5);
    let mut el = div().font_family(theme::MONO_FONT).child(
      div()
        .px_3()
        .py(px(2.))
        .bg(p.muted.opacity(0.6))
        .text_color(p.muted_foreground)
        .border_t_1()
        .border_color(p.border)
        .child(hunk.header.clone()),
    );
    for (l, line) in hunk.lines.iter().enumerate() {
      let at = (h, l);
      let (old, new) = numbers.get(l).copied().unwrap_or((None, None));
      let bg = if selected.contains(&at) {
        Some(tw::color(tw::BLUE_500).opacity(0.1))
      } else {
        match line_kind(line) {
          LineKind::Added => Some(tw::color(tw::EMERALD_500).opacity(0.2)),
          LineKind::Removed => Some(tw::color(tw::RED_500).opacity(0.2)),
          LineKind::Context => None,
        }
      };
      let prefix = match line_kind(line) {
        LineKind::Added => "+",
        LineKind::Removed => "-",
        LineKind::Context => " ",
      };
      let gutter = |n: Option<u32>, which: &'static str, view: &Self, cx: &mut Context<Self>| {
        let (id, file) = (commit.commit_id.clone(), path.to_string());
        let _ = view;
        div()
          .id(SharedString::from(format!("gutter-{which}-{h}-{l}")))
          .w_10()
          .flex_none()
          .pr_1()
          .flex()
          .justify_end()
          .text_color(gutter_fg)
          .cursor_pointer()
          .hover(move |s| s.bg(gutter_hover))
          .child(n.map(|n| n.to_string()).unwrap_or_default())
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
              view.press_line(&id, &file, at, e.modifiers.shift, cx);
            }),
          )
      };
      let plus = {
        let (c, file) = (commit.clone(), path.to_string());
        div()
          .w_5()
          .flex_none()
          .flex()
          .items_center()
          .justify_center()
          .child(
            div()
              .id(SharedString::from(format!("add-comment-{h}-{l}")))
              .debug_selector(move || format!("add-comment-{h}-{l}"))
              .p(px(1.))
              .rounded(px(4.))
              .bg(p.primary)
              .opacity(0.)
              .group_hover("diff-line", |s| s.opacity(1.))
              .cursor_pointer()
              .child(ui::icon(theme::ICON_PLUS, 12., p.primary_foreground))
              .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
              .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation();
                view.add_comment(&c, &file, at, cx);
              })),
          )
      };
      let (id, file) = (commit.commit_id.clone(), path.to_string());
      el = el.child(
        div()
          .id(SharedString::from(format!("diff-line-{h}-{l}")))
          .group("diff-line")
          .flex()
          .when_some(bg, |this, bg| this.bg(bg))
          .on_mouse_move(cx.listener(move |view, _: &MouseMoveEvent, _, cx| {
            view.drag_to(&id, &file, at, cx);
          }))
          .child(gutter(old, "old", self, cx))
          .child(gutter(new, "new", self, cx))
          .child(plus)
          .child(
            div()
              .w_4()
              .flex_none()
              .flex()
              .justify_center()
              .text_color(gutter_fg)
              .child(prefix),
          )
          .child(
            div()
              .flex_1()
              .min_w_0()
              .overflow_hidden()
              .whitespace_nowrap()
              .child(highlight::styled(
                line.get(1..).unwrap_or(""),
                language,
                p.dark,
              )),
          ),
      );
      if comment_at == Some(at) {
        el = el.child(self.comment_box(p, cx));
      }
    }
    el
  }

  /// `CommentInput` with Plan and Edit.
  fn comment_box(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let Some(comment) = &self.comment else {
      return div();
    };
    let empty = comment.text.trim().is_empty();
    let target = &comment.target;
    div()
      .debug_selector(|| "comment-input".into())
      .bg(p.muted)
      .border_1()
      .border_color(p.border)
      .rounded(px(8.))
      .mx_2()
      .my_1()
      .px_4()
      .py_3()
      .font_family(theme::SANS_FONT)
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        if k.key == "escape" {
          cx.stop_propagation();
          view.cancel_comment(cx);
        } else if k.key == "enter" && (k.modifiers.platform || k.modifiers.control) {
          cx.stop_propagation();
          view.send_comment(false, cx);
        }
      }))
      .child(div().mb_2().text_color(p.muted_foreground).child(format!(
        "{}:{}",
        target.file_path,
        line_label(target.start_line, target.end_line)
      )))
      .children(
        comment
          .input
          .as_ref()
          .map(|input| div().mb_2().child(Textarea::new(input))),
      )
      .child(
        div()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("comment-cancel", p)
              .variant(Variant::Ghost)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.cancel_comment(cx))),
          )
          .child(
            Button::new("comment-plan", p)
              .variant(Variant::Secondary)
              .size(Size::Sm)
              .disabled(empty)
              .label("Plan")
              .on_click(cx.listener(|view, _, _, cx| view.send_comment(true, cx))),
          )
          .child(
            Button::new("comment-edit", p)
              .size(Size::Sm)
              .disabled(empty)
              .label("Edit")
              .on_click(cx.listener(|view, _, _, cx| view.send_comment(false, cx))),
          ),
      )
  }

  fn dialog_view(
    &mut self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    let viewport = window.viewport_size();
    let keys = cx.listener(|view, e: &KeyDownEvent, _, cx| {
      let k = &e.keystroke;
      if k.key == "escape" {
        cx.stop_propagation();
        view.close_dialog(cx);
      } else if k.key == "enter" && (k.modifiers.platform || k.modifiers.control) {
        cx.stop_propagation();
        match view.dialog {
          Some(Dialog::Describe { .. }) => view.save_description(cx),
          Some(Dialog::Timestamp { .. }) => view.apply_timestamp(cx),
          Some(Dialog::Resolve { .. }) | None => {}
        }
      }
    });
    let (backdrop, panel) = match self.dialog.as_ref()? {
      Dialog::Describe {
        commit,
        loaded,
        input,
        text,
        saving,
      } => {
        let (backdrop, panel) = dialog_panel("edit-description-dialog", 560., viewport, p);
        let fetching = loaded.is_none();
        let unchanged = loaded.as_ref().and_then(|l| l.as_ref().ok()) == Some(text)
          || matches!(loaded, Some(Err(_))) && text.is_empty();
        let can_submit = !fetching && !saving && !unchanged;
        let error = loaded.as_ref().and_then(|l| l.as_ref().err()).cloned();
        let panel = panel
          .child(
            div()
              .flex()
              .flex_col()
              .gap(px(6.))
              .child(dialog_title("Edit Commit Description"))
              .child(dialog_description(
                format!("Editing {}", commit.short_id),
                p,
              )),
          )
          .child(
            div()
              .flex()
              .flex_col()
              .gap_4()
              .py_4()
              .child(if fetching {
                div()
                  .flex()
                  .items_center()
                  .gap_2()
                  .text_color(p.muted_foreground)
                  .child(ui::spinner("description-spin", 16., p.muted_foreground))
                  .child("Loading description...")
                  .into_any_element()
              } else {
                div()
                  .children(input.as_ref().map(Textarea::new))
                  .into_any_element()
              })
              .when_some(error, |this, err| {
                this.child(
                  ui::text(div(), TextSize::Sm)
                    .text_color(p.destructive)
                    .child(err),
                )
              }),
          )
          .child(
            div()
              .flex()
              .justify_end()
              .gap_2()
              .child(
                Button::new("description-cancel", p)
                  .variant(Variant::Outline)
                  .disabled(*saving)
                  .label("Cancel")
                  .on_click(cx.listener(|view, _, _, cx| view.close_dialog(cx))),
              )
              .child(
                Button::new("description-save", p)
                  .disabled(!can_submit)
                  .label(if *saving { "Saving..." } else { "Save" })
                  .on_click(cx.listener(|view, _, _, cx| view.save_description(cx))),
              ),
          );
        (backdrop, panel)
      }
      Dialog::Timestamp {
        commit,
        form,
        inputs,
        saving,
        error,
      } => {
        let (backdrop, panel) = dialog_panel("edit-timestamp-dialog", 520., viewport, p);
        let toggle = |id: &'static str, label: &'static str, on: bool| {
          Button::new(id, p)
            .size(Size::Sm)
            .variant(if on {
              Variant::Default
            } else {
              Variant::Outline
            })
            .label(label)
        };
        let field = |label: &'static str, input: Option<&Entity<InputState>>| {
          div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
              ui::text(div(), TextSize::Sm)
                .font_weight(FontWeight::MEDIUM)
                .child(label),
            )
            .children(input.map(|i| Styled::h(Input::new(i), px(40.))))
        };
        let inputs = inputs.as_ref();
        let fields = if form.to_day {
          field("Day", inputs.map(|i| &i[3])).into_any_element()
        } else {
          div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
              div()
                .flex()
                .gap_2()
                .child(
                  toggle("shift-forward", "Forward", !form.back)
                    .on_click(cx.listener(|view, _, _, cx| view.set_form(|f| f.back = false, cx))),
                )
                .child(
                  toggle("shift-back", "Back", form.back)
                    .on_click(cx.listener(|view, _, _, cx| view.set_form(|f| f.back = true, cx))),
                ),
            )
            .child(
              div()
                .flex()
                .gap_3()
                .child(field("Days", inputs.map(|i| &i[0])).flex_1())
                .child(field("Hours", inputs.map(|i| &i[1])).flex_1())
                .child(field("Minutes", inputs.map(|i| &i[2])).flex_1()),
            )
            .into_any_element()
        };
        let can_submit = !saving && form.shift().is_some();
        let panel = panel
          .child(
            div()
              .flex()
              .flex_col()
              .gap(px(6.))
              .child(dialog_title("Edit commit timestamp"))
              .child(dialog_description(
                format!(
                  "Current time: {}. Later commits on this lineage that would become older are \
                   shifted by the same gaps.",
                  time::full(&commit.timestamp)
                ),
                p,
              )),
          )
          .child(
            div()
              .flex()
              .flex_col()
              .gap_4()
              .py_2()
              .child(
                div()
                  .flex()
                  .gap_2()
                  .child(
                    toggle("shift-mode-offset", "Shift by duration", !form.to_day).on_click(
                      cx.listener(|view, _, _, cx| view.set_form(|f| f.to_day = false, cx)),
                    ),
                  )
                  .child(
                    toggle("shift-mode-day", "Move to day", form.to_day).on_click(
                      cx.listener(|view, _, _, cx| view.set_form(|f| f.to_day = true, cx)),
                    ),
                  ),
              )
              .child(fields)
              .when_some(error.clone(), |this, err| {
                this.child(
                  ui::text(div(), TextSize::Sm)
                    .text_color(p.destructive)
                    .child(err),
                )
              }),
          )
          .child(
            div()
              .flex()
              .items_center()
              .justify_between()
              .gap_2()
              .child(
                Button::new("shift-stack-to-now", p)
                  .variant(Variant::Outline)
                  .disabled(*saving)
                  .label("Shift stack to now")
                  .on_click(cx.listener(|view, _, _, cx| view.shift_to_now(true, cx))),
              )
              .child(
                div()
                  .flex()
                  .gap_2()
                  .child(
                    Button::new("timestamp-cancel", p)
                      .variant(Variant::Outline)
                      .disabled(*saving)
                      .label("Cancel")
                      .on_click(cx.listener(|view, _, _, cx| view.close_dialog(cx))),
                  )
                  .child(
                    Button::new("timestamp-apply", p)
                      .disabled(!can_submit)
                      .when(*saving, |this| {
                        this
                          .child(ui::spinner("timestamp-spin", 16., p.primary_foreground))
                          .label("Saving...")
                      })
                      .when(!*saving, |this| this.label("Apply"))
                      .on_click(cx.listener(|view, _, _, cx| view.apply_timestamp(cx))),
                  ),
              ),
          );
        (backdrop, panel)
      }
      Dialog::Resolve {
        session,
        input,
        submitting,
        ..
      } => {
        let (backdrop, panel) = dialog_panel(
          "resolve-conflicts-dialog",
          f32::from(viewport.width) * 0.42,
          viewport,
          p,
        );
        let preparing = session.is_none();
        let ready = matches!(session, Some(Ok(_)));
        let help = self.tips.wrap(
          "resolve-conflicts-help",
          Side::Top,
          Some(TooltipContent::Text(RESOLVE_HELP.into())),
          div()
            .id("resolve-conflicts-help")
            .debug_selector(|| "resolve-conflicts-help".into())
            .cursor_pointer()
            .child(ui::icon(
              theme::ICON_CIRCLE_HELP,
              16.,
              p.muted_foreground.opacity(0.7),
            ))
            .on_click(cx.listener(|view, _, _, _| {
              let url = format!("{}{RESOLVE_DOCS_PATH}", native::web_url());
              view.host.open_url(&url);
            })),
          cx,
        );
        let panel = panel
          .w(viewport.width * 0.42)
          .flex()
          .flex_col()
          .gap_4()
          .child(
            div()
              .flex()
              .items_center()
              .gap(px(6.))
              .child(dialog_title("Resolve commit conflicts inplace"))
              .child(help),
          )
          .child(
            div()
              .debug_selector(|| "resolve-conflicts-prompt".into())
              .children(input.as_ref().map(Textarea::new)),
          )
          .child(
            div()
              .flex()
              .items_center()
              .justify_end()
              .gap_2()
              .child(
                Button::new("resolve-cancel", p)
                  .variant(Variant::Ghost)
                  .disabled(*submitting)
                  .label("Cancel")
                  .on_click(cx.listener(|view, _, _, cx| view.close_dialog(cx))),
              )
              .child(
                Button::new("resolve-conflicts-submit", p)
                  .disabled(!ready || *submitting)
                  .when(preparing, |this| {
                    this
                      .child(ui::spinner("resolve-spin", 16., p.primary_foreground))
                      .label("Preparing…")
                  })
                  .when(!preparing, |this| {
                    this.label(if *submitting {
                      "Starting…"
                    } else {
                      "Resolve"
                    })
                  })
                  .on_click(cx.listener(|view, _, _, cx| view.submit_resolve(cx))),
              ),
          );
        (backdrop, panel)
      }
    };
    Some(
      deferred(
        anchored().position(point(px(0.), px(0.))).child(
          backdrop
            .on_mouse_down(
              MouseButton::Left,
              cx.listener(|view, _, _, cx| view.close_dialog(cx)),
            )
            .child(
              panel
                .id("commits-dialog")
                .capture_key_down(keys)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            ),
        ),
      )
      .with_priority(2),
    )
  }
}
