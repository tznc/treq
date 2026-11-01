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

mod rows;

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::{zpx, DismissExt as _};
use crate::result_ext::ResultExt;
use std::{
  collections::{HashMap, HashSet, VecDeque},
  rc::Rc,
  time::Duration,
};

use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, deferred, div, prelude::*, AnyElement, App, ClipboardItem, Context, Div, Entity,
  FontWeight, KeyDownEvent, MouseButton, Pixels, SharedString, Stateful, Subscription, Window,
};

use super::{
  code::Parent,
  code_data::Agent,
  commits_data::{
    comment_prompt, comment_target, empty_diff, first_diff, first_file_diff, first_line,
    has_content, headline, line_label, resolve_session_name, revisions, selection, CommentTarget,
    CommitsHost, History, Host, LineRef, ShiftForm, HOME_PAGE, TARGET_PAGE,
  },
  prefetch::{self, CommitDiffs},
  ActionSink, ScreenAction,
};
use crate::{
  gpui_shell::{
    confirm::{self, Question},
    native::{self, NativeEvent, ToastAction, ToastTone},
    popup::{MenuIcon, NativeMenuItem, PopupRequest, PopupSink, Side, TooltipContent},
    theme::{self, Palette},
    time,
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::{JjFileChange, JjFileDiff, JjLogCommit, JjRevisionDiff},
};

/// `REMOVE_ANIMATION_MS`: how long a removed commit stays, its buttons
/// disabled, before it leaves the list.
/// Diffs of collapsed commits kept, so opening one again draws at once.
const CLOSED_DIFFS: usize = 4;
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
  /// Each shown file's line numbers, by hunk.
  pub numbers: HashMap<String, Rc<Vec<Vec<LineNumbers>>>>,
}

/// A diff line's old and new numbers.
type LineNumbers = (Option<u32>, Option<u32>);

impl DiffState {
  /// The diff lists files, each a row of the card, rather than a message.
  fn split(&self) -> bool {
    !self.loading
      && self.error.is_none()
      && self
        .diff
        .as_ref()
        .is_some_and(|d| !d.too_large_to_render && !d.committed_files.is_empty())
  }
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
  /// The open confirmation (delete, undo or revert a commit).
  confirm: Option<confirm::Pending<Self>>,
  react: ActionSink,
  pub(super) parent: Parent,
  dark: bool,
  tips: Tooltips,
  popups: PopupSink,
  pub(super) history: Option<History>,
  loading_more: bool,
  pub(super) target_limit: usize,
  pub(super) home_limit: usize,
  /// Commits abandoned, stashed or undone, until the next read drops them.
  pub(super) removed: HashSet<String>,
  pub(super) removing: HashSet<String>,
  /// Commits removed before the backend answered, which a read landing
  /// meanwhile still lists.
  unconfirmed: HashSet<String>,
  pub(super) hide_tentative: bool,
  pub(super) expanded: HashSet<String>,
  pub(super) diffs: HashMap<String, DiffState>,
  /// Collapsed commits whose diff is still in `diffs`, the oldest first: a
  /// diff can hold thousands of lines, so only the last few stay.
  closed: VecDeque<String>,
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
  feed: rows::Feed,
  /// Bumped when the history or any diff changes, which the rows lay out.
  layout_gen: u64,
  /// The revision the tab was hidden at, while another tab or workspace
  /// shows.
  hidden_at: Option<u64>,
  /// The commit row the pointer rests on, whose diff is read ahead.
  hovered: prefetch::Intent<String>,
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
      tips: Tooltips::new(popups.clone(), dark),
      popups,
      history: None,
      loading_more: false,
      target_limit: TARGET_PAGE,
      home_limit: HOME_PAGE,
      removed: HashSet::new(),
      removing: HashSet::new(),
      unconfirmed: HashSet::new(),
      hide_tentative: false,
      expanded: HashSet::new(),
      diffs: HashMap::new(),
      closed: VecDeque::new(),
      shifting: false,
      move_menu: None,
      selection: None,
      comment: None,
      dialog: None,
      confirm: None,
      requested: scroll_to.clone(),
      scroll_to,
      feed: rows::Feed::default(),
      layout_gen: 0,
      hidden_at: None,
      hovered: prefetch::Intent::default(),
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
    // The log depends on nothing else: the conflicted files or the
    // workspace's record changing alone don't read it again.
    let reread = moved
      || self.parent.revision != parent.revision
      || self.parent.default_branch != parent.default_branch;
    self.parent = parent;
    if moved {
      self.history = None;
      self.layout_gen += 1;
      self.target_limit = TARGET_PAGE;
      self.home_limit = HOME_PAGE;
      self.removed.clear();
      self.removing.clear();
      self.unconfirmed.clear();
      self.expanded.clear();
      self.diffs.clear();
      self.closed.clear();
      self.selection = None;
      self.comment = None;
      self.dialog = None;
      self.move_menu = None;
    }
    if reread {
      self.load(cx);
    }
    cx.notify();
  }

  /// Its first read landed.
  pub(super) fn ready(&self) -> bool {
    self.history.is_some()
  }

  /// Hidden while another tab or workspace shows; shown again, it reads
  /// again behind what it draws, unless a newer revision already did.
  pub(super) fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
    match (shown, self.hidden_at) {
      (false, None) => self.hidden_at = Some(self.parent.revision),
      (true, Some(at)) => {
        self.hidden_at = None;
        // Until its first read lands (a view made ahead), that read is
        // still on the way.
        if at == self.parent.revision && self.ready() {
          self.load(cx);
        }
      }
      _ => {}
    }
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
      this
        .update(cx, |view, cx| {
          if view.parent.key() == key {
            apply(view, value, cx);
            cx.notify();
          }
        })
        .ignore_closed();
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
            // A fresh read no longer lists what was removed, unless the
            // removal is still on its way.
            let unconfirmed = &view.unconfirmed;
            view.removed.retain(|id| unconfirmed.contains(id));
            view.history = Some(history);
            view.layout_gen += 1;
          }
          Err(err) => {
            log::warn!("Failed to load commits: {err}");
            view.history.get_or_insert_with(History::default);
            view.layout_gen += 1;
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

  /// A listed commit: the tentative working copy, or one of the branch's
  /// own or the target's not removed.
  fn find(&self, commit_id: &str) -> Option<&JjLogCommit> {
    let history = self.history.as_ref()?;
    history
      .tentative
      .iter()
      .map(|(c, _)| c)
      .chain(
        history
          .own
          .iter()
          .chain(&history.target)
          .filter(|c| !self.removed.contains(&c.commit_id)),
      )
      .find(|c| c.commit_id == commit_id)
  }

  pub(super) fn toggle(&mut self, commit_id: &str, cx: &mut Context<Self>) {
    if self.expanded.remove(commit_id) {
      self.keep_closed(commit_id);
    } else {
      self.expand(commit_id, cx);
    }
    cx.notify();
  }

  /// Remembers that `commit_id` closed with its diff read, and drops the
  /// diffs of the commits closed longest ago. Opening one again reads it
  /// from [`CommitDiffs`] or the backend.
  fn keep_closed(&mut self, commit_id: &str) {
    self.closed.push_back(commit_id.to_string());
    while self.closed.len() > CLOSED_DIFFS {
      if let Some(old) = self.closed.pop_front() {
        if !self.expanded.contains(&old) {
          self.diffs.remove(&old);
        }
      }
    }
  }

  /// `fetchAndExpand`.
  fn expand(&mut self, commit_id: &str, cx: &mut Context<Self>) {
    self.expanded.insert(commit_id.to_string());
    self.closed.retain(|closed| closed != commit_id);
    if self.diffs.contains_key(commit_id) {
      return;
    }
    self.layout_gen += 1;
    // Read ahead while the pointer rested on the row: drawn at once.
    let key = (self.parent.repo_path.clone(), commit_id.to_string());
    if let Some(diff) = cx.default_global::<CommitDiffs>().0.get(&key).cloned() {
      self.diffs.insert(
        commit_id.to_string(),
        DiffState {
          diff: Some(diff),
          ..DiffState::default()
        },
      );
      return;
    }
    self.diffs.insert(
      commit_id.to_string(),
      DiffState {
        loading: true,
        ..DiffState::default()
      },
    );
    let change = self.find(commit_id).map(|c| c.change_id.clone());
    let revs = revisions(commit_id, change.as_deref());
    let id = commit_id.to_string();
    self.run(
      move |host, parent| {
        first_diff(&revs, |rev| {
          host.commit_diff(&parent.repo_path, parent.workspace_id(), rev)
        })
      },
      move |view, result, _| {
        view.layout_gen += 1;
        // A diff dropped while it read is not kept.
        let Some(state) = view.diffs.get_mut(&id) else {
          return;
        };
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

  /// The pointer entered or left commit `commit_id`'s row.
  fn hover_commit(&mut self, commit_id: String, hovered: bool, cx: &mut Context<Self>) {
    let id = commit_id.clone();
    self.hovered.hover(commit_id, hovered, cx, move |view, cx| {
      view.prefetch_diff(id, cx)
    });
  }

  /// Reads commit `commit_id`'s diff into [`CommitDiffs`], unless it is
  /// open or kept there already. Nothing redraws.
  fn prefetch_diff(&mut self, commit_id: String, cx: &mut Context<Self>) {
    let key = (self.parent.repo_path.clone(), commit_id);
    if self.diffs.contains_key(&key.1) || cx.default_global::<CommitDiffs>().0.get(&key).is_some() {
      return;
    }
    let (host, workspace_id) = (self.host.clone(), self.parent.workspace_id());
    cx.spawn(async move |this, cx| {
      let (repo, commit_id) = key.clone();
      let read = cx
        .background_spawn(async move { host.commit_diff(&repo, workspace_id, &commit_id) })
        .await;
      let diff = match read {
        Ok(diff) if has_content(&diff) => diff,
        _ => return,
      };
      this
        .update(cx, |_, cx| {
          let now = cx.background_executor().now();
          cx.default_global::<CommitDiffs>().0.insert(key, now, diff);
        })
        .ignore_closed();
    })
    .detach();
  }

  fn load_file_diff(&mut self, commit_id: &str, path: &str, cx: &mut Context<Self>) {
    let Some(state) = self.diffs.get_mut(commit_id) else {
      return;
    };
    state.deferred_loading.insert(path.to_string());
    state.deferred_errors.remove(path);
    cx.notify();
    let change = self.find(commit_id).map(|c| c.change_id.clone());
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
          view.layout_gen += 1;
          state.deferred_loading.remove(&path);
          match result {
            Ok(file) => {
              state.numbers.remove(&file.path);
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
      self.layout_gen += 1;
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
      this
        .update(cx, |view, cx| {
          if view.removing.remove(&commit_id) {
            view.removed.insert(commit_id);
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn confirm_slot(view: &mut Self) -> &mut Option<confirm::Pending<Self>> {
    &mut view.confirm
  }

  /// Asks `question`; its verb runs `go`, unless the tab has moved to
  /// another workspace meanwhile.
  fn confirmed(
    &mut self,
    question: Question,
    go: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let key = self.parent.key();
    confirm::ask(
      &mut self.confirm,
      question,
      move |view, cx| {
        if view.parent.key() == key {
          go(view, cx);
        }
      },
      cx,
    );
  }

  /// Removes commit `commit_id` from the list at once, not after the
  /// backend answers, and runs `act` off the main thread; a failure puts
  /// the commit back.
  fn remove<T: Send + 'static>(
    &mut self,
    commit_id: String,
    act: impl FnOnce(&dyn CommitsHost, &Parent) -> Result<T, String> + Send + 'static,
    apply: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    self.unconfirmed.insert(commit_id.clone());
    self.remove_later(commit_id.clone(), cx);
    cx.notify();
    self.run(
      act,
      move |view, result, cx| {
        view.unconfirmed.remove(&commit_id);
        if result.is_err() {
          view.removing.remove(&commit_id);
          view.removed.remove(&commit_id);
        }
        apply(view, result, cx);
      },
      cx,
    );
  }

  fn abandon(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let ws = self.parent.workspace_id();
    let question = Question::new(
      format!("Delete commit {}?", commit.short_id),
      format!(
        "\u{201c}{}\u{201d} is abandoned. You can undo it from the toast that follows.",
        first_line(&commit)
      ),
      "Delete commit",
    )
    .destructive();
    let change = commit.change_id.clone();
    let id = commit.commit_id.clone();
    let act =
      move |host: &dyn CommitsHost, parent: &Parent| host.abandon(&parent.repo_path, ws, &change);
    let apply =
      move |view: &mut Self, result: Result<String, String>, _: &mut Context<Self>| match result {
        Ok(operation) => {
          let (host, parent) = (view.host.clone(), view.parent.clone());
          Self::changed(host.as_ref(), &parent);
          let short = commit.short_id.clone();
          let undo = native::register_toast_action(move || {
            let event = match host.undo_operation(&parent.repo_path, ws, &operation) {
              Ok(()) => {
                Self::changed(host.as_ref(), &parent);
                toast_event(ToastTone::Success, &format!("Restored commit {short}"), "")
              }
              Err(err) => toast_event(ToastTone::Error, "Undo failed", err),
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
      };
    self.confirmed(
      question,
      move |view, cx| view.remove(id, act, apply, cx),
      cx,
    );
  }

  fn stash(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let change = commit.change_id.clone();
    self.remove(
      commit.commit_id,
      move |host, parent| host.stash(&parent.repo_path, parent.workspace_id(), &change),
      move |view, result, cx| match result {
        Ok(_) => {
          view.field("commits.stashed", serde_json::Value::Null, cx);
          Self::changed(view.host.as_ref(), &view.parent);
          view.toast(ToastTone::Success, "Commit stashed", "");
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
    let question = Question::new(
      format!("Undo commit {}?", commit.short_id),
      format!(
        "\u{201c}{}\u{201d} is undone and its changes return to your working copy.",
        first_line(&commit)
      ),
      "Undo commit",
    );
    let change = commit.change_id.clone();
    let id = commit.commit_id.clone();
    let act = move |host: &dyn CommitsHost, parent: &Parent| {
      host.undo_commit(&parent.repo_path, ws, &change)
    };
    let apply =
      move |view: &mut Self, result: Result<(), String>, _: &mut Context<Self>| match result {
        Ok(()) => {
          Self::changed(view.host.as_ref(), &view.parent);
          view.toast(
            ToastTone::Success,
            &format!("Undid commit {}", commit.short_id),
            "",
          );
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to undo commit", err),
      };
    self.confirmed(
      question,
      move |view, cx| view.remove(id, act, apply, cx),
      cx,
    );
  }

  fn revert(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let ws = self.parent.workspace_id();
    let question = Question::new(
      format!("Revert commit {}?", commit.short_id),
      format!(
        "A new commit reverses the changes of \u{201c}{}\u{201d}.",
        first_line(&commit)
      ),
      "Revert commit",
    );
    let change = commit.change_id.clone();
    self.confirmed(
      question,
      move |view, cx| {
        view.run(
          move |host, parent| host.revert(&parent.repo_path, ws, &change),
          move |view, result, _| match result {
            Ok(()) => {
              Self::changed(view.host.as_ref(), &view.parent);
              view.toast(
                ToastTone::Success,
                &format!("Reverted commit {}", commit.short_id),
                "",
              );
            }
            Err(err) => view.toast(ToastTone::Error, "Failed to revert commit", err),
          },
          cx,
        )
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
            view.toast(ToastTone::Success, "Timestamps updated", "");
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
                toast_event(ToastTone::Success, "Changes restored", "")
              }
              Err(err) => toast_event(ToastTone::Error, "Undo failed", err),
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
        let name = "Code comment";
        let id = host.create_session(&parent.repo_path, parent.workspace_id(), name)?;
        let repo = if parent.repo_path.is_empty() {
          parent.working_directory()
        } else {
          parent.repo_path.clone()
        };
        Ok::<_, String>(super::launch_data::started(
          host,
          json!({
            "sessionId": id,
            "sessionName": name,
            "workspaceId": parent.workspace_id(),
            "workspacePath": null,
            "repoPath": repo,
            "pendingPrompt": prompt,
            "permissionMode": mode,
          }),
        ))
      },
      |view, result, _cx| match result {
        Ok(_) => {
          view.toast(ToastTone::Success, "Comment sent to agent", "");
        }
        Err(err) => view.toast(ToastTone::Error, "Failed to create agent", err),
      },
      cx,
    );
  }

  // --- Dialogs ---

  pub(super) fn edit_description(&mut self, commit: JjLogCommit, cx: &mut Context<Self>) {
    let ws = self.parent.workspace_id();
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
    let ws = self.parent.workspace_id();
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
    let (change, description) = (commit.change_id.clone(), text.clone());
    // The row shows the new description at once; a failure puts the old one
    // back.
    let before = self.set_description(&change, &description);
    self.dialog = None;
    self.layout_gen += 1;
    cx.notify();
    self.run(
      move |host, parent| host.describe(&parent.repo_path, ws, &change, &description),
      move |view, result, _| match result {
        Ok(()) => {
          view.toast(ToastTone::Success, "Description updated", "");
          Self::changed(view.host.as_ref(), &view.parent);
        }
        Err(err) => {
          for (commit_id, description) in before {
            view.restore_description(&commit_id, description);
          }
          view.layout_gen += 1;
          view.toast(ToastTone::Error, "Failed to update description", err);
        }
      },
      cx,
    );
  }

  /// Sets the description of every loaded commit of change `change_id`;
  /// returns what each had.
  fn set_description(&mut self, change_id: &str, description: &str) -> Vec<(String, String)> {
    let Some(history) = &mut self.history else {
      return Vec::new();
    };
    history
      .tentative
      .iter_mut()
      .map(|(c, _)| c)
      .chain(history.own.iter_mut())
      .chain(history.target.iter_mut())
      .filter(|c| c.change_id == change_id)
      .map(|c| {
        let before = std::mem::replace(&mut c.description, description.to_string());
        (c.commit_id.clone(), before)
      })
      .collect()
  }

  fn restore_description(&mut self, commit_id: &str, description: String) {
    let Some(history) = &mut self.history else {
      return;
    };
    if let Some(commit) = history
      .tentative
      .iter_mut()
      .map(|(c, _)| c)
      .chain(history.own.iter_mut())
      .chain(history.target.iter_mut())
      .find(|c| c.commit_id == commit_id)
    {
      commit.description = description;
    }
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
    let change = commit.change_id.clone();
    self.run(
      move |host, parent| host.shift_timestamp(&parent.repo_path, ws, &change, &shift),
      move |view, result, _| match result {
        Ok(()) => {
          view.dialog = None;
          view.toast(ToastTone::Success, "Timestamp updated", "");
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
          super::launch_data::started(
            host,
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
          ),
          count,
        ))
      },
      |view, result, _cx| match result {
        Ok(_) => {
          view.dialog = None;
          view.toast(ToastTone::Success, "Resolve session started", "");
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
        "Add a comment…",
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
          "Describe this commit…",
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

  /// Scrolls to and opens the commit React asked for, once it is listed.
  fn scroll_to_commit(&mut self, cx: &mut Context<Self>) {
    let Some(change) = self.scroll_to.clone() else {
      return;
    };
    let Some((ix, commit_id)) = self.commit_row(&change) else {
      return;
    };
    self.scroll_to_row(ix);
    self.scroll_to = None;
    self.expand(&commit_id, cx);
    self.field("commits.scrolled", serde_json::Value::Null, cx);
    cx.notify();
  }
}

/// A diff with no files to list: too large to render, or empty.
fn diff_message(diff: &JjRevisionDiff, p: &Palette) -> Div {
  if diff.too_large_to_render {
    return div()
      .debug_selector(|| "commit-diff-blocked".into())
      .p_3()
      .text_color(p.muted_foreground)
      .child(
        diff
          .render_block_reason
          .clone()
          .unwrap_or_else(|| "This commit diff is too large to render.".into()),
      );
  }
  div()
    .p_3()
    .text_color(p.muted_foreground)
    .child("No changes")
}

impl Render for CommitsView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    self.ensure_inputs(window, cx);
    self.sync_rows(window);
    self.scroll_to_commit(cx);
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
    if self.history.is_none() {
      return root.child(ui::delayed(
        "commits-loading-row",
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
              .child("Loading commits…"),
          ),
      ));
    }
    if self.feed_is_empty() {
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
    let dialog = self.dialog_view(window, &p, cx);
    let confirm = confirm::render(Self::confirm_slot, self, window, &p, cx);
    root
      .child(self.feed_list(cx))
      .children(dialog)
      .children(confirm)
  }
}

impl CommitsView {
  /// "Shift to now" and the conflicts banner.
  fn banners(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
    let ids = self.feed.conflicted().to_vec();
    let n = ids.len();
    div()
      .when(self.feed.shift(), |this| {
        this.child(
          div()
            .when(n > 0, |this| this.mb_3())
            .flex()
            .justify_end()
            .child(
              Button::new("shift-commits-to-now", p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .gap_1p5()
                .disabled(self.shifting)
                .child(if self.shifting {
                  ui::spinner("shift-spin", 16., p.foreground).into_any_element()
                } else {
                  ui::icon(theme::ICON_CLOCK, 16., p.foreground).into_any_element()
                })
                .label("Shift to now")
                .on_click(cx.listener(|view, _, _, cx| view.shift_to_now(false, cx))),
            ),
        )
      })
      .when(n > 0, |this| {
        this.child(
          ui::Alert::new(
            "resolve-conflicts-banner",
            format!(
              "{n} commit{} conflicts",
              if n == 1 { " has" } else { "s have" }
            ),
            p,
          )
          .severity(ui::Severity::Danger)
          .message("Resolve them in place with a short-lived resolve workspace.")
          .action(
            Button::new("resolve-conflicts-button", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .gap_1p5()
              .icon(theme::ICON_MERGE, 16.)
              .label("Resolve…")
              .aria_label("Resolve conflicts…")
              .on_click(cx.listener(move |view, _, _, cx| view.open_resolve(ids.clone(), cx))),
          ),
        )
      })
  }

  /// "Load more commits", of the target's or the branch's own.
  fn load_more_row(&self, target: bool, p: &Palette, cx: &mut Context<Self>) -> Div {
    let id = if target {
      "commits-target-load-more"
    } else {
      "commits-load-more"
    };
    let loading = self.loading_more;
    div().pl_7().child(
      ui::Row::load_more(id, loading, p)
        .title("Load more commits")
        .when(!loading, |row| {
          row.on_click(cx.listener(move |view, _, _, cx| view.load_more(target, cx)))
        }),
    )
  }
}

impl CommitsView {
  fn commit(
    &mut self,
    commit: &JjLogCommit,
    kind: Kind,
    label: Option<&String>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let tentative = kind == Kind::Tentative;
    let first = matches!(kind, Kind::Own { first: true }) || tentative;
    let expanded = self.expanded.contains(&commit.commit_id);
    let dot = if tentative {
      ui::Tone::Warning.color(p)
    } else if first {
      p.primary
    } else {
      p.muted_foreground
    };
    let yellow = ui::Tone::Warning.tint(p);
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
    let mut title = div().min_w_0().truncate().child(headline(commit));
    if let Some(label) = label.filter(|l| !l.is_empty()) {
      // The workspace's label in mono after the headline.
      title = div()
        .min_w_0()
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
    // An unfolded own commit edits its description and timestamp in place.
    let own = expanded && matches!(kind, Kind::Own { .. });
    let short = &commit.change_id;
    let edit_description = own.then(|| {
      self.edit_icon(
        format!("edit-description-{short}"),
        "Edit description",
        commit,
        Self::edit_description,
        p,
        cx,
      )
    });
    let edit_timestamp =
      (own && self.parent.workspace.is_some() && !commit.is_immutable).then(|| {
        self.edit_icon(
          format!("edit-timestamp-{short}"),
          "Edit timestamp",
          commit,
          Self::edit_timestamp,
          p,
          cx,
        )
      });
    // The working copy's commit moves as files change: never read ahead.
    let read_ahead = (!tentative && !commit.is_working_copy).then(|| {
      let id = commit.commit_id.clone();
      cx.listener(move |view, hovered: &bool, _, cx| view.hover_commit(id.clone(), *hovered, cx))
    });
    let row = ui::Row::new(
      SharedString::from(format!("commit-row-{}", commit.commit_id)),
      p,
    )
    .label(headline(commit))
    .relative()
    .items_start()
    .gap_3()
    .py_2()
    .mx_neg_2()
    .w_full()
    .rounded(radius::MD)
    .when(tentative, |this| this.bg(yellow))
    .leading(
      div().relative().flex_none().child(
        div()
          .size(zpx(14.))
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
        .pt_0p5()
        .child(
          div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(ui::icon(
              if expanded {
                theme::ICON_CHEVRON_DOWN
              } else {
                theme::ICON_CHEVRON_RIGHT
              },
              16.,
              p.muted_foreground,
            ))
            .child(title)
            .children(edit_description),
        )
        .child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .mt_0p5()
            .pl_5()
            .when(commit.has_conflicts, |this| {
              this.child(
                div()
                  .flex_none()
                  .debug_selector({
                    let id = format!("commit-conflict-badge-{}", commit.change_id);
                    move || id
                  })
                  .child(ui::Badge::new("Conflict", p).tone(ui::Tone::Danger)),
              )
            })
            .child(time)
            .children(edit_timestamp)
            .when(commit.insertions > 0 || commit.deletions > 0, |this| {
              this.child(
                div()
                  .ml_auto()
                  .flex()
                  .child(
                    div()
                      .text_color(ui::Tone::GitOpen.text(p))
                      .child(format!("+{}", commit.insertions)),
                  )
                  .child("\u{a0}")
                  .child(
                    div()
                      .text_color(ui::Tone::GitClosed.text(p))
                      .child(format!("-{}", commit.deletions)),
                  ),
              )
            }),
        ),
    )
    .when_some(read_ahead, |this, listener| this.on_hover(listener))
    .when(!tentative, |this| {
      let commit = commit.clone();
      this.on_mouse_down(
        MouseButton::Right,
        cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
          view.commit_menu(&commit, kind, e.position, cx);
        }),
      )
    })
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
        this.child(self.expanded_commit(commit, kind, p, cx))
      })
  }

  fn expanded_commit(
    &mut self,
    commit: &JjLogCommit,
    kind: Kind,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let removing = self.removing.contains(&commit.commit_id);
    let split = self
      .diffs
      .get(&commit.commit_id)
      .is_some_and(DiffState::split);
    // One-word labels; `name` is the fuller accessible name.
    let outline = |id: String, icon: &'static str, label: &'static str, name: &'static str| {
      Button::new(id, p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .gap_1p5()
        .icon(icon, 16.)
        .label(label)
        .aria_label(name)
    };
    let destructive_outline = |id: String, name: &'static str| {
      outline(id, theme::ICON_TRASH, "Delete", name)
        .variant(Variant::DestructiveOutline)
        .disabled(removing)
    };
    let short = &commit.change_id;
    let move_name = if kind == Kind::Tentative {
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
          "Move",
          move_name,
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
          .top(zpx(36.))
          .left_0()
          .min_w(zpx(128.))
          .p_1()
          .rounded(radius::MD)
          .border_1()
          .border_color(p.border)
          .bg(p.popover)
          .text_color(p.popover_foreground)
          .shadow_md()
          .occlude()
          .child(
            ui::MenuItem::new("move-commit-new", "Move to new workspace", p)
              .on_click(cx.listener(move |view, _, _, cx| view.move_commit(&to_new, false, cx))),
          )
          .child(
            ui::MenuItem::new("move-commit-existing", "Move to existing workspace", p).on_click(
              cx.listener(move |view, _, _, cx| view.move_commit(&to_existing, true, cx)),
            ),
          )
          .dismiss_with("move-commit-menu", cx, |view, cx| {
            let open = view.move_menu.take().is_some();
            if open {
              cx.notify();
            }
            open
          });
        this.child(deferred(panel).with_priority(1))
      });
    let mut actions = div()
      .flex()
      .flex_wrap()
      .items_center()
      .gap_2()
      .mb_2()
      .mt_2();
    if kind == Kind::Tentative {
      actions = actions
        .child(
          outline(
            "view-tentative-changes".into(),
            theme::ICON_FILE_TEXT,
            "View",
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
        .when(
          matches!(kind, Kind::Own { first: true }) && workspace,
          |this| {
            this.child(
              outline(
                format!("undo-commit-{short}"),
                theme::ICON_UNDO,
                "Undo",
                "Undo commit",
              )
              .disabled(removing)
              .on_click(Self::on_commit(commit, Self::undo, cx)),
            )
          },
        )
        .child(
          outline(
            format!("revert-commit-{short}"),
            theme::ICON_ROTATE_CCW,
            "Revert",
            "Revert commit",
          )
          .on_click(Self::on_commit(commit, Self::revert, cx)),
        )
        .when(own, |this| {
          this
            .child(move_button)
            .child(
              outline(
                "stash-commit-button".into(),
                theme::ICON_ARCHIVE,
                "Stash",
                "Stash commit",
              )
              .disabled(removing)
              .on_click(Self::on_commit(commit, Self::stash, cx)),
            )
            .child(
              destructive_outline(format!("delete-commit-{short}"), "Delete commit")
                .on_click(Self::on_commit(commit, Self::abandon, cx)),
            )
        });
    }
    let content = match self.diffs.get(&commit.commit_id) {
      Some(state) if state.loading => ui::delayed(
        format!("diff-loading-{}", commit.commit_id),
        div()
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
          .child("Loading diff…"),
      )
      .into_any_element(),
      Some(DiffState {
        error: Some(err), ..
      }) => div()
        .p_3()
        .text_color(p.destructive)
        .child(format!("Failed to load diff: {err}"))
        .into_any_element(),
      Some(DiffState {
        diff: Some(diff), ..
      }) => diff_message(diff, p).into_any_element(),
      _ => div().into_any_element(),
    };
    // A diff that lists files continues as rows of its own.
    div().ml_7().child(actions).when(!split, |this| {
      this.child(
        div()
          .border_1()
          .border_color(p.border)
          .rounded(radius::MD)
          .overflow_hidden()
          .child(content),
      )
    })
  }

  /// A commit row's right-click menu: copy its ids or message, then the
  /// actions its expanded row offers.
  fn commit_menu(
    &self,
    commit: &JjLogCommit,
    kind: Kind,
    at: gpui::Point<Pixels>,
    cx: &mut Context<Self>,
  ) {
    let view = cx.entity().downgrade();
    let copy = |label: &str, text: String, what: &'static str| {
      let view = view.clone();
      NativeMenuItem::item(label, move |cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        view
          .update(cx, |view, _| {
            view.toast(ToastTone::Success, &format!("{what} copied"), "")
          })
          .ignore_closed();
      })
      .icon(MenuIcon::Copy)
    };
    let run = |label: &str, f: fn(&mut Self, JjLogCommit, &mut Context<Self>)| {
      let (view, commit) = (view.clone(), commit.clone());
      NativeMenuItem::item(label, move |cx| {
        view
          .update(cx, |view, cx| f(view, commit.clone(), cx))
          .ignore_closed();
      })
    };
    let own = matches!(kind, Kind::Own { .. });
    let workspace = self.parent.workspace.is_some();
    let removing = self.removing.contains(&commit.commit_id);
    let mut items = vec![
      copy("Copy commit ID", commit.commit_id.clone(), "Commit ID"),
      copy("Copy change ID", commit.change_id.clone(), "Change ID"),
      copy(
        "Copy message",
        commit.description.trim().to_string(),
        "commit message",
      ),
      NativeMenuItem::Separator,
    ];
    if own {
      items.push(run("Edit description", Self::edit_description).icon(MenuIcon::Pencil));
    }
    if matches!(kind, Kind::Own { first: true }) && workspace && !removing {
      items.push(run("Undo commit", Self::undo));
    }
    items.push(run("Revert commit", Self::revert));
    if own && workspace && !commit.is_immutable {
      items.push(run("Edit timestamp", Self::edit_timestamp));
    }
    if own && !removing {
      items.push(run("Stash commit", Self::stash).icon(MenuIcon::Archive));
      items.push(NativeMenuItem::Separator);
      items.push(
        run("Delete commit", Self::abandon)
          .icon(MenuIcon::Trash2)
          .destructive(),
      );
    }
    let popups = self.popups.clone();
    let request = PopupRequest::ShowMenu {
      entries: crate::gpui_shell::popup::native_entries(items),
      at,
      dark: self.dark,
    };
    cx.defer(move |cx| popups(request, cx));
  }

  /// A click that runs `f` on `commit`.
  /// A small pencil button with a tooltip of `name` that runs `f` on
  /// `commit` without folding its row.
  fn edit_icon(
    &self,
    id: String,
    name: &'static str,
    commit: &JjLogCommit,
    f: fn(&mut Self, JjLogCommit, &mut Context<Self>),
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    let on_click = Self::on_commit(commit, f, cx);
    self.tips.wrap(
      id.clone(),
      Side::Top,
      Some(TooltipContent::Text(name.into())),
      Button::new(id, p)
        .variant(Variant::Ghost)
        .size(Size::IconXs)
        .icon(theme::ICON_PENCIL, 14.)
        .aria_label(name)
        .on_click(move |event, window, cx| {
          cx.stop_propagation();
          on_click(event, window, cx);
        }),
      cx,
    )
  }

  fn on_commit(
    commit: &JjLogCommit,
    f: fn(&mut Self, JjLogCommit, &mut Context<Self>),
    cx: &mut Context<Self>,
  ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let (c, view) = (commit.clone(), cx.entity().downgrade());
    move |_, _, cx| {
      view
        .update(cx, |view, cx| f(view, c.clone(), cx))
        .ignore_closed();
    }
  }

  /// "Load diff" for a file whose diff was held back.
  pub(super) fn deferred_file(
    &self,
    commit: &JjLogCommit,
    f: usize,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let Some(state) = self.diffs.get(&commit.commit_id) else {
      return div().into_any_element();
    };
    let Some(file) = state.diff.as_ref().and_then(|d| d.committed_files.get(f)) else {
      return div().into_any_element();
    };
    let busy = state.deferred_loading.contains(&file.path);
    let (id, path) = (commit.commit_id.clone(), file.path.clone());
    let muted = p.muted_foreground;
    let fg = p.foreground;
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
          .when_some(state.deferred_errors.get(&file.path), |this, err| {
            this.child(div().text_color(p.destructive).child(err.clone()))
          })
          .child(
            div()
              .id(SharedString::from(format!("load-diff-{}", file.path)))
              .debug_selector(|| "load-deferred-diff".into())
              .rounded(radius::MD)
              .border_1()
              .border_color(p.border)
              .px_2()
              .py_1()
              .text_color(muted)
              .when(busy, |this| this.opacity(0.6))
              .when(!busy, |this| {
                this
                  .cursor_pointer()
                  .hover(move |s| s.bg(p.hover()).text_color(fg))
                  .on_click(cx.listener(move |view, _, _, cx| view.load_file_diff(&id, &path, cx)))
              })
              .child(if busy { "Loading…" } else { "Load diff" }),
          ),
      )
      .into_any_element()
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
      "A" => (ui::Tone::GitOpen.text(p), ui::Tone::GitOpen.tint(p)),
      "M" => (ui::Tone::Primary.text(p), ui::Tone::Primary.tint(p)),
      "D" => (ui::Tone::GitClosed.text(p), ui::Tone::GitClosed.tint(p)),
      _ => (p.foreground, gpui::transparent_black()),
    };
    let (id, path) = (commit.commit_id.clone(), file.path.clone());
    ui::Row::new(
      SharedString::from(format!("commit-file-{}-{}", commit.commit_id, file.path)),
      p,
    )
    .debug_selector(|| "commit-file-row".into())
    .label(file.path.clone())
    .density(ui::RowDensity::Default)
    .px_3()
    .w_full()
    .rounded_none()
    .leading(ui::icon(
      if open {
        theme::ICON_CHEVRON_DOWN
      } else {
        theme::ICON_CHEVRON_RIGHT
      },
      16.,
      p.muted_foreground,
    ))
    .leading(ui::icon(theme::ICON_FILE_TEXT, 16., p.muted_foreground))
    .child(
      div()
        .flex_1()
        .min_w_0()
        .truncate()
        .font_family(theme::MONO_FONT)
        .child(file.path.clone()),
    )
    .trailing(
      div()
        .font_weight(FontWeight::MEDIUM)
        .px_1p5()
        .py_0p5()
        .rounded(radius::SM)
        .text_color(fg)
        .bg(bg)
        .child(file.status.clone()),
    )
    .on_click(cx.listener(move |view, _, _, cx| view.toggle_file(&id, &path, cx)))
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
      .rounded(radius::MD)
      .mx_2()
      .my_1()
      .px_4()
      .py_3()
      .font_family(theme::SANS_FONT)
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        if ui::is_escape(k) {
          cx.stop_propagation();
          view.cancel_comment(cx);
        } else if k.key == "enter"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
        {
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
    let dialog = match self.dialog.as_ref()? {
      Dialog::Describe {
        commit,
        loaded,
        input,
        text,
        saving,
      } => {
        let fetching = loaded.is_none();
        let unchanged = loaded.as_ref().and_then(|l| l.as_ref().ok()) == Some(text)
          || matches!(loaded, Some(Err(_))) && text.is_empty();
        let can_submit = !fetching && !saving && !unchanged;
        let error = loaded.as_ref().and_then(|l| l.as_ref().err()).cloned();
        ui::Dialog::new("edit-description-dialog", p)
          .width(560.)
          .title("Edit commit description")
          .description(format!("Editing {}", commit.short_id))
          .on_primary(ui::handler(cx, |view, _, cx| view.save_description(cx)))
          .primary_disabled(!can_submit)
          .child(
            div()
              .flex()
              .flex_col()
              .gap_4()
              .child(if fetching {
                div()
                  .flex()
                  .items_center()
                  .gap_2()
                  .text_color(p.muted_foreground)
                  .child(ui::spinner("description-spin", 16., p.muted_foreground))
                  .child("Loading description…")
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
          .footer([
            Button::new("description-cancel", p)
              .variant(Variant::Outline)
              .disabled(*saving)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.close_dialog(cx)))
              .into_any_element(),
            Button::new("description-save", p)
              .disabled(!can_submit)
              .label(if *saving { "Saving…" } else { "Save" })
              .on_click(cx.listener(|view, _, _, cx| view.save_description(cx)))
              .into_any_element(),
          ])
      }
      Dialog::Timestamp {
        commit,
        form,
        inputs,
        saving,
        error,
      } => {
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
            .gap_1p5()
            .child(
              ui::text(div(), TextSize::Sm)
                .font_weight(FontWeight::MEDIUM)
                .child(label),
            )
            .children(input.map(|i| Styled::h(ui::input(i), zpx(40.))))
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
        ui::Dialog::new("edit-timestamp-dialog", p)
          .width(520.)
          .title("Edit commit timestamp")
          .description(format!(
            "Current time: {}. Later commits on this lineage that would become older are \
             shifted by the same gaps.",
            time::full(&commit.timestamp)
          ))
          .on_primary(ui::handler(cx, |view, _, cx| view.apply_timestamp(cx)))
          .primary_disabled(!can_submit)
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
          // The stack-wide shift sits apart, left of the action pair.
          .footer([
            Button::new("shift-stack-to-now", p)
              .variant(Variant::Outline)
              .disabled(*saving)
              .label("Shift stack to now")
              .on_click(cx.listener(|view, _, _, cx| view.shift_to_now(true, cx)))
              .into_any_element(),
            div().flex_1().into_any_element(),
            Button::new("timestamp-cancel", p)
              .variant(Variant::Outline)
              .disabled(*saving)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.close_dialog(cx)))
              .into_any_element(),
            Button::new("timestamp-apply", p)
              .disabled(!can_submit)
              .when(*saving, |this| {
                this
                  .child(ui::spinner("timestamp-spin", 16., p.primary_foreground))
                  .label("Saving…")
              })
              .when(!*saving, |this| this.label("Apply"))
              .on_click(cx.listener(|view, _, _, cx| view.apply_timestamp(cx)))
              .into_any_element(),
          ])
      }
      Dialog::Resolve {
        session,
        input,
        submitting,
        ..
      } => {
        let preparing = session.is_none();
        let ready = matches!(session, Some(Ok(_)));
        let failed = match session {
          Some(Err(err)) => Some(err.clone()),
          _ => None,
        };
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
        ui::Dialog::new("resolve-conflicts-dialog", p)
          .relative_size(0.42, None)
          .title("Resolve commit conflicts in place")
          .header(
            div()
              .flex()
              .items_center()
              .gap_1p5()
              .child(
                ui::text(div(), TextSize::Lg)
                  .font_weight(FontWeight::SEMIBOLD)
                  .child("Resolve commit conflicts in place"),
              )
              .child(help),
          )
          .on_primary(ui::handler(cx, |view, _, cx| view.submit_resolve(cx)))
          .primary_disabled(!ready || *submitting)
          .child(
            div()
              .debug_selector(|| "resolve-conflicts-prompt".into())
              .children(input.as_ref().map(Textarea::new)),
          )
          .when_some(failed, |this, err| {
            this.child(
              ui::text(div(), TextSize::Sm)
                .debug_selector(|| "resolve-conflicts-error".into())
                .text_color(p.destructive)
                .child(err),
            )
          })
          .footer([
            Button::new("resolve-cancel", p)
              .variant(Variant::Outline)
              .disabled(*submitting)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.close_dialog(cx)))
              .into_any_element(),
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
              .on_click(cx.listener(|view, _, _, cx| view.submit_resolve(cx)))
              .into_any_element(),
          ])
      }
    };
    Some(
      dialog
        .overlay(viewport)
        .on_close(ui::handler(cx, |view, _, cx| view.close_dialog(cx))),
    )
  }
}
