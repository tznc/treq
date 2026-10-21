//! The workspace Changes tab (`ChangesDiffViewer`): the sidebar with the
//! commit box and the file lists, and the diff of every changed file.
//!
//! Drawn from the backend directly ([`changes_data`]). What stays React's
//! goes back to it as screen actions: moving files to another workspace,
//! the agent terminal a review starts in, and creating the pull request.
//!
//! [`changes_data`]: super::changes_data

pub(in crate::gpui_shell::screen) mod pr;
mod review;
pub(in crate::gpui_shell::screen) mod search;
pub(super) mod threads;

use crate::gpui_shell::ui::zpx;
use std::{
  cell::RefCell,
  collections::{HashMap, HashSet},
  rc::Rc,
};

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use serde::Deserialize;
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, App, Bounds, ClipboardItem, Context, Div,
  Entity, FontWeight, Hsla, KeyDownEvent, MouseButton, Pixels, ScrollHandle, SharedString,
  Stateful, Subscription, Window,
};

use super::{
  changes_data::{
    after_range, before_range, body, can_expand_before, check_message, commit_label,
    conflict_lines, conflicts, deleted_sides, diff_summary, discarded, full_path, hunks_hash,
    is_collapsed, line_counts, pending_label, review_prompt, section_label, select_all_label,
    split_path, stashed, total_lines, visible_committed, Body, BodyInput, ChangedFile, ChangesHost,
    CommitAction, Conflict, EditorApps, FileHunks, Host, List, Snapshot, Staging, BATCH,
    FIRST_BATCH, HOME_ENDPOINT, LARGE_CHANGESET_LINES,
  },
  code::Parent,
  commits_data::{line_kind, line_numbers, LineKind},
  highlight, ActionSink, ScreenAction,
};
use crate::{
  conflict_markers::{ConflictLineKind, ConflictLineRole},
  gpui_shell::{
    native::{self, NativeEvent, ToastAction, ToastTone},
    popup::{MenuIcon, NativeMenuItem, PopupRequest, PopupSink, Side, TooltipContent},
    theme::{self, tw, Palette},
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::{JjDiffHunk, JjFileChange},
};

/// A workspace, or the home repository, files can move to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MoveTarget {
  pub branch: String,
  pub label: String,
}

/// What React passes the tab besides the workspace.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
  /// `showCommittedChanges`; `None` when the toggle is not offered.
  pub show_committed: Option<bool>,
  pub move_targets: Vec<MoveTarget>,
  /// Bumped when files change on disk or the window gains focus.
  pub revision: u64,
}

/// Tells React what the tab changed.
fn tell(host: &dyn ChangesHost, event: NativeEvent) {
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

/// Which side of a hunk "Show more lines" reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Dir {
  Before,
  After,
}

/// The collapsible sidebar sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Section {
  Conflicts,
  Selected,
  Changes,
}

pub struct ChangesView {
  host: Host,
  react: ActionSink,
  popups: PopupSink,
  pub(super) parent: Parent,
  pub(super) options: Options,
  dark: bool,
  tips: Tooltips,
  /// `None` until the first read (`initialLoading`).
  pub(super) snapshot: Option<Snapshot>,
  loading: bool,
  /// Another read was asked for while one ran.
  reload: bool,
  pub(super) hunks: HashMap<String, FileHunks>,
  generation: u64,
  pub(super) staging: Staging,
  pub(super) active: Option<String>,
  collapsed_sections: HashSet<Section>,
  committed_collapsed: bool,
  pub(super) collapsed_files: HashSet<String>,
  pub(super) expanded_large: HashSet<String>,
  pub(super) changeset_expanded: bool,
  /// Files marked viewed, with the diff's hash then.
  pub(super) viewed: HashMap<String, String>,
  /// Lines read by "Show more lines", by file, hunk and side.
  pub(super) context: HashMap<(String, usize, Dir), Vec<String>>,
  pub(super) message: String,
  message_input: Option<Entity<TextareaState>>,
  pub(super) message_error: Option<&'static str>,
  pub(super) pending: Option<CommitAction>,
  /// The file a discard runs on.
  file_action: Option<String>,
  pub(super) discard_dialog: bool,
  starting_review: bool,
  /// The repository has a GitHub remote, and the branch's pull request.
  github: bool,
  pr_state: Option<String>,
  editors: EditorApps,
  scroll: ScrollHandle,
  /// Where each file card was last drawn, by path.
  cards: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
  pub(super) scroll_to: Option<String>,
  /// The button a dropdown opens below, by key.
  anchors: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
  /// Line comments, notes and the pending review.
  pub(super) review: review::Review,
  /// The find bar.
  pub(super) search: search::Search,
  /// GitHub review threads and the review agent's comments.
  pub(super) threads: threads::Threads,
  /// The pull request reviewed instead of a workspace (`PrReviewPage`).
  pub(super) pr: Option<pr::PrMode>,
  _subscriptions: Vec<Subscription>,
}

impl HasTooltips for ChangesView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl ChangesView {
  pub fn new(
    parent: Parent,
    options: Options,
    host: Host,
    react: ActionSink,
    popups: PopupSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let mut this = Self::blank(parent, options, host, react, popups, dark);
    this.hydrate_review(cx);
    this.load(cx);
    this.watch_threads(cx);
    this.load_pr(cx);
    this
  }

  fn blank(
    parent: Parent,
    options: Options,
    host: Host,
    react: ActionSink,
    popups: PopupSink,
    dark: bool,
  ) -> Self {
    let editors = host.editor_apps();
    Self {
      host,
      react,
      tips: Tooltips::new(popups.clone(), dark),
      popups,
      parent,
      options,
      dark,
      snapshot: None,
      loading: false,
      reload: false,
      hunks: HashMap::new(),
      generation: 0,
      staging: Staging::default(),
      active: None,
      collapsed_sections: HashSet::new(),
      committed_collapsed: false,
      collapsed_files: HashSet::new(),
      expanded_large: HashSet::new(),
      changeset_expanded: false,
      viewed: HashMap::new(),
      context: HashMap::new(),
      message: String::new(),
      message_input: None,
      message_error: None,
      pending: None,
      file_action: None,
      discard_dialog: false,
      starting_review: false,
      github: false,
      pr_state: None,
      editors,
      scroll: ScrollHandle::new(),
      cards: Rc::default(),
      scroll_to: None,
      anchors: Rc::default(),
      review: review::Review::default(),
      search: search::Search::default(),
      threads: threads::Threads::default(),
      pr: None,
      _subscriptions: Vec::new(),
    }
  }

  pub fn set_parent(
    &mut self,
    parent: Parent,
    options: Options,
    dark: bool,
    cx: &mut Context<Self>,
  ) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
      cx.notify();
    }
    if self.parent == parent && self.options == options {
      return;
    }
    let moved = self.parent.key() != parent.key();
    // Not on `parent.revision`: React bumps that after every read this tab
    // reports (`handleRefreshingChange` invalidates the commit caches), so
    // it would read forever. Its own `options.revision` covers file changes.
    let reread = moved
      || self.options.revision != options.revision
      || self.options.show_committed != options.show_committed;
    self.parent = parent;
    self.options = options;
    if moved {
      self.snapshot = None;
      self.hunks.clear();
      self.staging = Staging::default();
      self.active = None;
      self.collapsed_files.clear();
      self.expanded_large.clear();
      self.changeset_expanded = false;
      self.viewed.clear();
      self.context.clear();
      self.message.clear();
      self.message_input = None;
      self.discard_dialog = false;
      self.review = review::Review::default();
      self.close_search(cx);
      self.watch_threads(cx);
      self.hydrate_review(cx);
      self.load_pr(cx);
    }
    if reread {
      self.load(cx);
    }
    cx.notify();
  }

  pub(super) fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
      cx.notify();
    }
  }

  /// Brings `path`'s diff into view (`scrollToFile`).
  pub(super) fn scroll_to_path(&mut self, path: &str, cx: &mut Context<Self>) {
    self.show_file(path, cx);
  }

  /// Runs `work` off the main thread, then `apply`s it unless the tab moved
  /// to another workspace meanwhile.
  fn run<T: Send + 'static>(
    &self,
    work: impl FnOnce(&dyn ChangesHost, &Parent) -> T + Send + 'static,
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
  fn changed(host: &dyn ChangesHost, parent: &Parent) {
    tell(
      host,
      NativeEvent::WorkspaceChanged {
        repo_path: parent.repo_path.clone(),
      },
    );
  }

  fn files(&self) -> &[ChangedFile] {
    self.snapshot.as_ref().map_or(&[], |s| s.files.as_slice())
  }

  /// `actualConflictedFiles`: the diff's list once read, else the status's.
  pub(super) fn conflicted(&self) -> Vec<String> {
    let list = self
      .snapshot
      .as_ref()
      .and_then(|s| s.conflicted.clone())
      .unwrap_or_else(|| self.parent.conflicted_files.clone());
    let mut seen = HashSet::new();
    list
      .into_iter()
      .map(|f| f.trim().to_string())
      .filter(|f| !f.is_empty() && seen.insert(f.clone()))
      .collect()
  }

  fn show_committed(&self) -> bool {
    self.options.show_committed.unwrap_or(false)
  }

  /// The committed files shown: all with the toggle on, else those also
  /// dirty or conflicted.
  pub(super) fn visible_committed(&self) -> Vec<JjFileChange> {
    let Some(snapshot) = &self.snapshot else {
      return Vec::new();
    };
    let always: HashSet<String> = snapshot
      .files
      .iter()
      .map(|f| f.path.clone())
      .chain(self.conflicted())
      .collect();
    visible_committed(&snapshot.committed, self.show_committed(), &always)
      .into_iter()
      .cloned()
      .collect()
  }

  /// `loadChangedFiles`: one read at a time, a request meanwhile reruns it.
  fn load(&mut self, cx: &mut Context<Self>) {
    if self.pr.is_some() {
      self.read_pr_diff(cx);
      return;
    }
    if self.loading {
      self.reload = true;
      return;
    }
    self.loading = true;
    self.field("changes.refreshing", true.into(), cx);
    let show = self.show_committed();
    self.run(
      move |host, parent| match parent.workspace_id() {
        Some(id) => host
          .workspace_diff(&parent.repo_path, id)
          .map(|diff| Snapshot::from_workspace_diff(diff, show)),
        None => host
          .changed_files(&parent.repo_path, None)
          .map(|files| Snapshot::from_home(&files)),
      },
      |view, result, cx| {
        view.loading = false;
        view.field("changes.refreshing", false.into(), cx);
        match result {
          Ok(snapshot) => {
            if !view.park(&snapshot) {
              view.apply(snapshot, cx);
            }
          }
          Err(err) => {
            view.toast(ToastTone::Error, "JJ Error", err);
            view.snapshot.get_or_insert_with(Snapshot::default);
          }
        }
        if std::mem::take(&mut view.reload) {
          view.load(cx);
        }
      },
      cx,
    );
  }

  fn apply(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
    let files: Vec<serde_json::Value> = snapshot
      .files
      .iter()
      .map(|f| {
        json!({
          "path": f.path,
          "workspaceStatus": f.status,
          "oldPath": f.old_path,
          "isConflicted": f.status == "C",
        })
      })
      .collect();
    self.field("changes.files", files.into(), cx);
    self.staging.retain(&snapshot.files);
    let paths: Vec<String> = snapshot.files.iter().map(|f| f.path.clone()).collect();
    self.snapshot = Some(snapshot);
    self.load_hunks(paths, cx);
    self.unmark_stale(cx);
  }

  /// `loadAllFileHunks`: the first files, then the rest in batches, each
  /// shown as it arrives. What was read before stays until replaced.
  fn load_hunks(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
    self.generation += 1;
    let listed: HashSet<&String> = paths.iter().collect();
    self.hunks.retain(|p, _| listed.contains(p));
    for path in &paths {
      self.hunks.entry(path.clone()).or_insert_with(|| FileHunks {
        loading: true,
        ..FileHunks::default()
      });
    }
    let mut batches: Vec<Vec<String>> = Vec::new();
    let (first, rest) = paths.split_at(paths.len().min(FIRST_BATCH));
    if !first.is_empty() {
      batches.push(first.to_vec());
    }
    batches.extend(rest.chunks(BATCH).map(<[String]>::to_vec));
    self.load_batches(self.generation, batches, cx);
  }

  fn load_batches(
    &mut self,
    generation: u64,
    mut batches: Vec<Vec<String>>,
    cx: &mut Context<Self>,
  ) {
    if batches.is_empty() {
      return;
    }
    let batch = batches.remove(0);
    self.run(
      move |host, parent| host.file_hunks(&parent.repo_path, parent.workspace_id(), batch),
      move |view, result, cx| {
        if view.generation != generation {
          return;
        }
        match result {
          Ok(batch) => {
            for file in batch.files {
              let data = FileHunks {
                hunks: file.hunks,
                loading: false,
                error: file.error,
              };
              view.hunks.insert(file.path, data);
            }
          }
          Err(err) => log::warn!("Failed to load file hunks: {err}"),
        }
        view.unmark_stale(cx);
        if batches.is_empty() {
          view.prune_comments(cx);
        }
        view.load_batches(generation, batches, cx);
      },
      cx,
    );
  }

  fn load_pr(&mut self, cx: &mut Context<Self>) {
    let branch = self
      .parent
      .workspace
      .as_ref()
      .map(|w| w.branch_name.clone());
    self.run(
      move |host, parent| {
        let github = host.github_remote(&parent.repo_path);
        let pr = branch
          .and_then(|b| host.pr_cache(&parent.repo_path, &b).pr.flatten())
          .map(|pr| pr.state);
        (github, pr)
      },
      |view, (github, pr), _| {
        view.github = github;
        view.pr_state = pr;
      },
      cx,
    );
  }

  /// The diff a file card shows: the working copy's, else the branch's.
  fn file_hunks(&self, path: &str) -> Option<FileHunks> {
    self.hunks.get(path).cloned().or_else(|| {
      self
        .snapshot
        .as_ref()
        .and_then(|s| s.committed_hunks.get(path))
        .map(|hunks| FileHunks {
          hunks: hunks.clone(),
          ..FileHunks::default()
        })
    })
  }

  /// Viewed files whose diff changed since are viewed no longer.
  fn unmark_stale(&mut self, cx: &mut Context<Self>) {
    let listed: HashSet<String> = self.files().iter().map(|f| f.path.clone()).collect();
    self.viewed.retain(|p, _| listed.contains(p));
    let stale: Vec<String> = self
      .viewed
      .iter()
      .filter(|(path, hash)| {
        !hash.is_empty()
          && self
            .hunks
            .get(*path)
            .filter(|d| !d.loading && !d.hunks.is_empty())
            .is_some_and(|d| hunks_hash(&d.hunks) != **hash)
      })
      .map(|(p, _)| p.clone())
      .collect();
    for path in stale {
      self.viewed.remove(&path);
      let workspace = self.parent.working_directory();
      self.run(
        move |host, _| host.unmark_viewed(&workspace, &path),
        |_, _, _| {},
        cx,
      );
    }
  }

  // Sidebar.

  /// A file row click: the selection, then the file's diff in view.
  fn click_file(
    &mut self,
    list: List,
    path: &str,
    shift: bool,
    meta: bool,
    cx: &mut Context<Self>,
  ) {
    let files = self.files().to_vec();
    self.staging.click(list, &files, path, shift, meta);
    self.show_file(path, cx);
  }

  /// `scrollToFileIfNeeded`.
  fn show_file(&mut self, path: &str, cx: &mut Context<Self>) {
    self.active = Some(path.to_string());
    self.changeset_expanded = true;
    self.collapsed_files.remove(path);
    self.expanded_large.insert(path.to_string());
    self.scroll_to = Some(path.to_string());
    cx.notify();
  }

  fn scroll_to_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Some(path) = self.scroll_to.clone() else {
      return;
    };
    let bounds = self.cards.borrow().get(&path).copied();
    match bounds {
      Some(card) => {
        let view = self.scroll.bounds();
        let offset = self.scroll.offset();
        self
          .scroll
          .set_offset(point(offset.x, offset.y - (card.top() - view.top())));
        self.scroll_to = None;
        cx.notify();
      }
      None => cx.on_next_frame(window, |_, _, cx| cx.notify()),
    }
  }

  fn toggle_section(&mut self, section: Section, cx: &mut Context<Self>) {
    if !self.collapsed_sections.remove(&section) {
      self.collapsed_sections.insert(section);
    }
    cx.notify();
  }

  /// Clicks on a section's empty space clear its selection.
  fn deselect(&mut self, list: List, cx: &mut Context<Self>) {
    match list {
      List::Changes => self.staging.selected.clear(),
      List::Selected => self.staging.selected_staged.clear(),
    }
    cx.notify();
  }

  fn stage(&mut self, path: &str, cx: &mut Context<Self>) {
    self.staging.stage(path);
    cx.notify();
  }

  fn unstage(&mut self, path: &str, cx: &mut Context<Self>) {
    self.staging.unstage(path);
    cx.notify();
  }

  /// `handleDiscardFiles`: the row, or the selection it is in, with an Undo.
  fn discard_files(&mut self, path: &str, cx: &mut Context<Self>) {
    let paths = self.staging.targets(path);
    self.file_action = Some(path.to_string());
    cx.notify();
    self.run(
      move |host, parent| {
        let workspace = parent.working_directory();
        let snapshot = host.snapshot_working_copy(&workspace)?;
        for path in &paths {
          host.discard_file(&workspace, path)?;
        }
        Ok::<_, String>((workspace, snapshot, paths))
      },
      |view, result, cx| {
        view.file_action = None;
        match result {
          Ok((workspace, snapshot, paths)) => {
            view.staging.selected.clear();
            view.discarded(workspace, snapshot, discarded(&paths), cx);
          }
          Err(err) => view.toast(ToastTone::Error, "Discard Failed", err),
        }
      },
      cx,
    );
  }

  /// `handleDiscardAll`, after its dialog.
  fn discard_all(&mut self, cx: &mut Context<Self>) {
    self.discard_dialog = false;
    cx.notify();
    self.run(
      |host, parent| {
        let workspace = parent.working_directory();
        let snapshot = host.snapshot_working_copy(&workspace)?;
        host.discard_all(&workspace)?;
        Ok::<_, String>((workspace, snapshot))
      },
      |view, result, cx| match result {
        Ok((workspace, snapshot)) => {
          view.discarded(workspace, snapshot, "All changes discarded".into(), cx)
        }
        Err(err) => view.toast(ToastTone::Error, "Discard All Failed", err),
      },
      cx,
    );
  }

  /// The "Discarded" toast, its Undo restoring `snapshot`.
  fn discarded(
    &mut self,
    workspace: String,
    snapshot: String,
    description: String,
    cx: &mut Context<Self>,
  ) {
    let (host, parent) = (self.host.clone(), self.parent.clone());
    Self::changed(host.as_ref(), &parent);
    let undo = native::register_toast_action(move || {
      let event = match host.restore_snapshot(&workspace, &snapshot) {
        Ok(()) => {
          Self::changed(host.as_ref(), &parent);
          toast_event(
            ToastTone::Success,
            "Restored",
            "Discarded changes have been restored",
          )
        }
        Err(err) => toast_event(ToastTone::Error, "Undo Failed", err),
      };
      tell(host.as_ref(), event);
    });
    tell(
      self.host.as_ref(),
      NativeEvent::Toast {
        title: "Discarded".into(),
        description,
        tone: ToastTone::Success,
        action: Some(ToastAction {
          label: "Undo".into(),
          id: undo,
        }),
      },
    );
    self.load(cx);
  }

  /// `handleStashAll`.
  fn stash_all(&mut self, cx: &mut Context<Self>) {
    self.run(
      |host, parent| host.stash_all(&parent.repo_path, parent.workspace_id()),
      |view, result, cx| match result {
        Ok(entry) => {
          view.toast(ToastTone::Success, "Changes stashed", stashed(&entry));
          Self::changed(view.host.as_ref(), &view.parent);
          view.field("commits.stashed", serde_json::Value::Null, cx);
          view.load(cx);
        }
        Err(err) => view.toast(ToastTone::Error, "Stash Failed", err),
      },
      cx,
    );
  }

  /// Opens React's move confirmation for `paths`.
  fn move_files(&self, paths: Vec<String>, target: &MoveTarget, cx: &mut App) {
    let source = self
      .parent
      .workspace
      .as_ref()
      .map_or_else(|| HOME_ENDPOINT.to_string(), |w| w.branch_name.clone());
    self.field(
      "changes.moveFiles",
      json!({
        "files": paths,
        "sourceBranch": source,
        "destinationBranch": target.branch,
        "destinationLabel": target.label,
      }),
      cx,
    );
  }

  /// The sidebar's Move button: the selection to a new workspace.
  fn move_to_new(&self, cx: &mut App) {
    let mut paths: Vec<String> = self.staging.selected.iter().cloned().collect();
    paths.sort();
    self.field("changes.moveToNew", paths.into(), cx);
  }

  // Commit box.

  fn can_create_pr(&self) -> bool {
    self.github && self.pr_state.is_none() && self.parent.workspace.is_some()
  }

  fn has_pr(&self) -> bool {
    self.pr_state.as_deref() == Some("OPEN")
  }

  /// `runAction`: checks the message, clears the box, then commits.
  fn run_commit(&mut self, action: CommitAction, window: &mut Window, cx: &mut Context<Self>) {
    let message = self.message.trim().to_string();
    if message.is_empty() {
      self.message_error = Some("Enter a commit message.");
      if let Some(input) = &self.message_input {
        input.update(cx, |s, cx| s.focus(window, cx));
      }
      cx.notify();
      return;
    }
    self.message_error = None;
    self.set_message(String::new(), window, cx);
    self.commit(action, message, cx);
  }

  fn set_message(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
    self.message = value.clone();
    if let Some(input) = &self.message_input {
      input.update(cx, |s, cx| s.set_value(value, window, cx));
    }
  }

  /// `performCommit`, then the push or the pull request.
  fn commit(&mut self, action: CommitAction, message: String, cx: &mut Context<Self>) {
    let message = match check_message(&message) {
      Ok(m) => m,
      Err(err) => {
        self.toast(ToastTone::Error, "Commit message", err);
        return;
      }
    };
    self.pending = Some(action);
    cx.notify();
    let staged: Vec<String> = self
      .files()
      .iter()
      .filter(|f| self.staging.staged.contains(&f.path))
      .map(|f| f.path.clone())
      .collect();
    let split = !staged.is_empty();
    self.run(
      move |host, parent| {
        let result = if split {
          host.split(&parent.working_directory(), &message, staged)
        } else {
          host.commit(&parent.repo_path, parent.workspace_id(), &message)
        };
        let result = result.map_err(|e| ("Commit failed", e))?;
        if action == CommitAction::Push {
          host
            .push_remote(&parent.repo_path, parent.workspace_id())
            .map_err(|e| ("Failed to push", e))?;
        }
        Ok::<_, (&str, String)>(result)
      },
      move |view, result, cx| {
        view.pending = None;
        if split {
          view.staging.staged.clear();
        }
        match result {
          Ok(result) => {
            let text = result.trim();
            view.toast(
              ToastTone::Success,
              "Commit created",
              if text.is_empty() {
                "Commit successful"
              } else {
                text
              },
            );
            view.committed_collapsed = true;
            if action == CommitAction::Push {
              view.toast(ToastTone::Success, "Pushed to remote", "");
            }
            if action == CommitAction::Pr {
              view.send_react(
                ScreenAction::Press {
                  id: "workspace.createPr".into(),
                },
                cx,
              );
            }
          }
          Err((title, err)) => view.toast(ToastTone::Error, title, err),
        }
        Self::changed(view.host.as_ref(), &view.parent);
        view.load(cx);
      },
      cx,
    );
  }

  fn ensure_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.message_input.is_some() {
      return;
    }
    let message = self.message.clone();
    let state = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(2)
        .placeholder("Message")
        .default_value(message)
    });
    self
      ._subscriptions
      .push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          view.message = state.read(cx).value().to_string();
          view.message_error = None;
          cx.notify();
        }
      }));
    self.message_input = Some(state);
  }

  // Diff.

  fn toggle_file(&mut self, path: &str, cx: &mut Context<Self>) {
    if !self.collapsed_files.remove(path) {
      self.collapsed_files.insert(path.to_string());
    }
    cx.notify();
  }

  fn toggle_large(&mut self, path: &str, cx: &mut Context<Self>) {
    if !self.expanded_large.remove(path) {
      self.expanded_large.insert(path.to_string());
    }
    cx.notify();
  }

  /// `handleMarkFileViewed` / `handleUnmarkFileViewed`: viewed files shut.
  fn toggle_viewed(&mut self, path: &str, cx: &mut Context<Self>) {
    let workspace = self.parent.working_directory();
    let path = path.to_string();
    // A pull request's viewed files are kept only while the screen shows.
    if self.read_only() {
      if self.viewed.remove(&path).is_none() {
        self.viewed.insert(path.clone(), String::new());
        self.collapsed_files.insert(path);
      }
      cx.notify();
      return;
    }
    if self.viewed.remove(&path).is_some() {
      self.collapsed_files.remove(&path);
      self.run(
        move |host, _| host.unmark_viewed(&workspace, &path),
        |_, _, _| {},
        cx,
      );
    } else {
      let hash = self
        .file_hunks(&path)
        .filter(|d| !d.loading && !d.hunks.is_empty())
        .map(|d| hunks_hash(&d.hunks))
        .unwrap_or_default();
      self.viewed.insert(path.clone(), hash.clone());
      self.collapsed_files.insert(path.clone());
      self.run(
        move |host, _| host.mark_viewed(&workspace, &path, &hash),
        |_, _, _| {},
        cx,
      );
    }
    cx.notify();
  }

  /// `handleExpandContext`: twenty more lines above or below a hunk.
  fn expand_context(&mut self, path: &str, h: usize, dir: Dir, cx: &mut Context<Self>) {
    // The PR's files are not checked out.
    if self.read_only() {
      return;
    }
    let Some(hunk) = self.hunks.get(path).and_then(|d| d.hunks.get(h)).cloned() else {
      return;
    };
    let key = (path.to_string(), h, dir);
    let loaded = self.context.get(&key).map_or(0, Vec::len);
    let range = match dir {
      Dir::Before => before_range(&hunk.header, loaded),
      Dir::After => Some(after_range(&hunk.header, loaded)),
    };
    let Some((start, end)) = range else {
      return;
    };
    let file = path.to_string();
    self.run(
      move |host, parent| {
        host.file_lines(&parent.repo_path, parent.workspace_id(), &file, start, end)
      },
      move |view, result, _| {
        let Ok(lines) = result else {
          return;
        };
        if lines.is_empty() {
          return;
        }
        let entry = view.context.entry(key).or_default();
        match dir {
          Dir::Before => {
            let mut all = lines;
            all.append(entry);
            *entry = all;
          }
          Dir::After => entry.extend(lines),
        }
      },
      cx,
    );
  }

  /// "Start Review": a review agent with the repository's prompt.
  fn start_review(&mut self, cx: &mut Context<Self>) {
    let Some(workspace) = self.parent.workspace.clone() else {
      return;
    };
    self.starting_review = true;
    cx.notify();
    let files = self.files().len();
    self.run(
      move |host, parent| {
        let keys = ["review_prompt".to_string(), "review_agent".to_string()];
        let settings = host
          .repo_settings(&parent.repo_path, &keys)
          .unwrap_or_default();
        let custom = settings.get("review_prompt").and_then(|v| v.clone());
        let agent = settings
          .get("review_agent")
          .and_then(|v| v.clone())
          .filter(|a| !a.is_empty());
        let summary = diff_summary(Some(&workspace.branch_name), files);
        let prompt = review_prompt(custom.as_deref(), &workspace.id.to_string(), &summary);
        (prompt, agent)
      },
      |view, (prompt, agent), cx| {
        view.starting_review = false;
        view.field(
          "changes.startReview",
          json!({ "prompt": prompt, "agent": agent }),
          cx,
        );
      },
      cx,
    );
  }

  fn copy(&self, text: String, description: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    self.toast(ToastTone::Success, "Copied", description);
  }

  fn open_in(&self, scheme: &str, path: &str) {
    let full = full_path(&self.parent.working_directory(), path);
    self.host.open_url(&format!("{scheme}://file/{full}"));
  }

  /// The editors "Open in…" lists, each opening `path`.
  fn editor_items(&self, path: &str, cx: &mut Context<Self>) -> Vec<NativeMenuItem> {
    let mut items = Vec::new();
    for (on, scheme, label) in [
      (self.editors.cursor, "cursor", "Open in Cursor"),
      (self.editors.vscode, "vscode", "Open in VSCode"),
      (self.editors.zed, "zed", "Open in Zed"),
    ] {
      if on {
        let (view, path) = (cx.entity().downgrade(), path.to_string());
        items.push(NativeMenuItem::item(label, move |cx| {
          let _ = view.update(cx, |view, _| view.open_in(scheme, &path));
        }));
      }
    }
    items
  }

  /// `FileContextMenu`: copy the path, open it, or move it.
  fn file_menu(&self, path: &str, movable: bool, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
    let workspace = self.parent.working_directory();
    let full = full_path(&workspace, path);
    let relative = full
      .strip_prefix(&format!("{}/", workspace.trim_end_matches('/')))
      .unwrap_or(&full)
      .to_string();
    let entity = cx.entity().downgrade();
    let copy = |text: String, description: String| {
      let entity = entity.clone();
      move |cx: &mut App| {
        let _ = entity.update(cx, |view, cx| {
          view.copy(text.clone(), description.clone(), cx)
        });
      }
    };
    let mut open = vec![{
      let (host, full) = (self.host.clone(), full.clone());
      NativeMenuItem::item("Open in Finder", move |_| host.reveal(&full)).icon(MenuIcon::FolderOpen)
    }];
    open.extend(self.editor_items(path, cx));
    let mut items = vec![
      NativeMenuItem::item(
        "Copy relative path",
        copy(
          relative.clone(),
          format!("Copied relative path: {relative}"),
        ),
      )
      .icon(MenuIcon::Copy),
      NativeMenuItem::item(
        "Copy full path",
        copy(full.clone(), format!("Copied full path: {full}")),
      )
      .icon(MenuIcon::Copy),
      NativeMenuItem::Separator,
      NativeMenuItem::Submenu {
        text: "Open in...".into(),
        icon: Some(MenuIcon::FolderOpen),
        detail: None,
        items: open,
      },
    ];
    let source = self
      .parent
      .workspace
      .as_ref()
      .map(|w| w.branch_name.as_str());
    let targets: Vec<MoveTarget> = self
      .options
      .move_targets
      .iter()
      .filter(|t| {
        Some(t.branch.as_str()) != source && !(source.is_none() && t.branch == HOME_ENDPOINT)
      })
      .cloned()
      .collect();
    if movable && !targets.is_empty() {
      let paths = self.staging.targets(path);
      items.push(NativeMenuItem::Separator);
      items.push(NativeMenuItem::Submenu {
        text: "Move to workspace...".into(),
        icon: Some(MenuIcon::ArrowRightLeft),
        detail: None,
        items: targets
          .into_iter()
          .map(|target| {
            let (entity, paths) = (entity.clone(), paths.clone());
            NativeMenuItem::item(target.label.clone(), move |cx| {
              let _ = entity.update(cx, |view, cx| view.move_files(paths.clone(), &target, cx));
            })
          })
          .collect(),
      });
    }
    self.popup(
      PopupRequest::ShowMenu {
        entries: crate::gpui_shell::popup::native_entries(items),
        at,
        dark: self.dark,
      },
      cx,
    );
  }

  /// The file header's More menu: discard and the editors.
  fn more_menu(&self, path: &str, key: &str, cx: &mut Context<Self>) {
    let Some(anchor) = self.anchors.borrow().get(key).copied() else {
      return;
    };
    let mut items = Vec::new();
    let dirty = self.files().iter().any(|f| f.path == path);
    if dirty && self.file_action.as_deref() != Some(path) {
      let n = self.staging.selected.len();
      let label = if self.staging.selected.contains(path) && n > 1 {
        format!("Discard {n} files")
      } else {
        "Discard file".into()
      };
      let (entity, path) = (cx.entity().downgrade(), path.to_string());
      items.push(
        NativeMenuItem::item(label, move |cx| {
          let _ = entity.update(cx, |view, cx| view.discard_files(&path, cx));
        })
        .destructive(),
      );
      items.push(NativeMenuItem::Separator);
    }
    items.extend(self.editor_items(path, cx));
    if items.is_empty() {
      return;
    }
    self.popup(
      PopupRequest::ShowDropdown {
        entries: crate::gpui_shell::popup::native_entries(items),
        anchor,
        dark: self.dark,
      },
      cx,
    );
  }

  /// The commit button's menu.
  fn commit_menu(&self, cx: &mut Context<Self>) {
    let Some(anchor) = self.anchors.borrow().get("commit-menu").copied() else {
      return;
    };
    let entity = cx.entity().downgrade();
    let action = |action: CommitAction| {
      let entity = entity.clone();
      move |cx: &mut App| {
        let _ = entity.update(cx, |view, cx| {
          let message = view.message.trim().to_string();
          if message.is_empty() {
            view.message_error = Some("Enter a commit message.");
            cx.notify();
            return;
          }
          view.message.clear();
          view.message_input = None;
          view.commit(action, message, cx);
        });
      }
    };
    let mut items = vec![NativeMenuItem::item(
      "Commit and push",
      action(CommitAction::Push),
    )];
    if !self.has_pr() && self.can_create_pr() {
      items.push(NativeMenuItem::item(
        "Commit and create PR",
        action(CommitAction::Pr),
      ));
    }
    self.popup(
      PopupRequest::ShowDropdown {
        entries: crate::gpui_shell::popup::native_entries(items),
        anchor,
        dark: self.dark,
      },
      cx,
    );
  }

  fn popup(&self, request: PopupRequest, cx: &mut App) {
    let popups = self.popups.clone();
    cx.defer(move |cx| popups(request, cx));
  }

  /// Records its parent's bounds as `key`'s, for a dropdown below it.
  fn anchor(&self, key: String) -> impl IntoElement {
    let anchors = self.anchors.clone();
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
}

/// A `p-1 rounded` icon button of the section headers.
fn header_button(
  id: impl Into<SharedString>,
  icon: &'static str,
  fg: Hsla,
  hover: Hsla,
) -> Stateful<Div> {
  let id: SharedString = id.into();
  let selector = id.to_string();
  div()
    .id(id)
    .debug_selector(move || selector)
    .p_1()
    .rounded(zpx(4.))
    .cursor_pointer()
    .text_color(fg)
    .hover(move |s| s.bg(hover))
    .child(ui::icon(icon, 12., fg))
}

impl Render for ChangesView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    self.ensure_input(window, cx);
    self.ensure_review_inputs(window, cx);
    self.ensure_reply_inputs(window, cx);
    self.watch_find_key(window, cx);
    self.ensure_search_input(window, cx);
    self.place_threads();
    self.index_matches();
    self.reveal_match(window, cx);
    self.scroll_to_file(window, cx);
    let p = theme::palette(self.dark);
    let root = ui::text(div(), TextSize::Sm)
      .debug_selector(|| "changes-diff-viewer".into())
      .size_full()
      .flex()
      .overflow_hidden()
      .bg(p.background)
      .text_color(p.foreground)
      .on_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        if k.key == "a"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
          && !crate::gpui_shell::keys::held(k.modifiers).shift
          && !view.read_only()
        {
          let files = view.files().to_vec();
          view.staging.select_all(&files);
          cx.notify();
        } else if k.key == "escape" && view.review.selection.is_some() {
          view.clear_selection(cx);
        }
      }))
      .on_mouse_up(
        MouseButton::Left,
        cx.listener(|view, _, _, cx| view.end_drag(cx)),
      )
      .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|view, _, _, cx| view.end_drag(cx)),
      );
    let sidebar = (!self.read_only()).then(|| self.sidebar(&p, cx));
    let main = div()
      .flex_1()
      .flex()
      .flex_col()
      .min_w_0()
      .children(self.action_bar(&p, cx))
      .child(
        div()
          .flex_1()
          .flex()
          .flex_col()
          .min_h_0()
          .relative()
          .child(self.content(&p, cx))
          .children(self.search_bar(&p, cx)),
      );
    let dialog = self.dialog(window, &p, cx);
    let finish = self.finish_popover(&p, cx);
    let review_dialog = self.review_dialog(window, &p, cx);
    root
      .children(sidebar)
      .child(main)
      .children(dialog)
      .children(finish)
      .children(review_dialog)
  }
}

impl ChangesView {
  fn sidebar(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let mut list = div()
      .id("changes-sidebar-scroll")
      .flex_1()
      .overflow_y_scroll()
      .px_4()
      .pb_4();
    match &self.snapshot {
      None => {
        list = list.child(
          div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(ui::spinner("changes-sidebar-spin", 24., p.muted_foreground)),
        );
      }
      Some(_) => {
        let conflicted = self.conflicted();
        if !conflicted.is_empty() {
          list = list.child(self.conflicts_section(&conflicted, p, cx));
        }
        let files = self.files().to_vec();
        let committed = self.snapshot.as_ref().map_or(0, |s| s.committed.len());
        if files.is_empty() && committed == 0 {
          list = list.child(
            div()
              .size_full()
              .flex()
              .flex_col()
              .items_center()
              .justify_center()
              .py_12()
              .child(ui::icon(
                theme::ICON_CIRCLE_CHECK,
                48.,
                p.muted_foreground.opacity(0.4),
              ))
              .child(
                div()
                  .mt_3()
                  .text_color(p.muted_foreground)
                  .child("No changes"),
              ),
          );
        } else if !files.is_empty() {
          let staged: Vec<ChangedFile> = self
            .staging
            .staged_list(&files)
            .into_iter()
            .cloned()
            .collect();
          if !staged.is_empty() {
            list = list.child(self.files_section(List::Selected, &staged, p, cx));
          }
          let unstaged: Vec<ChangedFile> =
            self.staging.unstaged(&files).into_iter().cloned().collect();
          list = list.child(self.files_section(List::Changes, &unstaged, p, cx));
        }
        if committed > 0 {
          list = list.child(self.committed_section(p, cx));
        }
      }
    }
    div()
      .w(zpx(240.))
      .flex_none()
      .h_full()
      .flex()
      .flex_col()
      .border_r_1()
      .border_color(p.border)
      .bg(p.sidebar)
      .child(self.commit_box(p, cx))
      .child(list)
  }

  fn commit_box(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let files = self.files().len();
    let disabled = files == 0 || self.pending.is_some();
    let label = match self.pending {
      Some(action) => pending_label(action).to_string(),
      None => commit_label(self.staging.staged.len()),
    };
    let error = self.message_error;
    let border = p.primary_foreground.opacity(0.2);
    let main = Button::new("commit-button", p)
      .size(Size::Sm)
      .flex_1()
      .h_auto()
      .py(zpx(6.))
      .gap(zpx(6.))
      .rounded_r_none()
      .disabled(disabled)
      .when(self.pending.is_some(), |b| {
        b.child(ui::spinner("commit-spin", 16., p.primary_foreground))
      })
      .label(label)
      .on_click(
        cx.listener(|view, _, window, cx| view.run_commit(CommitAction::Commit, window, cx)),
      );
    let menu = div()
      .relative()
      .flex()
      .child(
        Button::new("commit-options", p)
          .aria_label("More commit options")
          .size(Size::Sm)
          .h_auto()
          .py(zpx(6.))
          .px(zpx(6.))
          .rounded_l_none()
          .border_l_1()
          .border_color(border)
          .disabled(disabled)
          .child(ui::icon(
            theme::ICON_CHEVRON_DOWN,
            16.,
            p.primary_foreground,
          ))
          .on_click(cx.listener(|view, _, _, cx| view.commit_menu(cx))),
      )
      .child(self.anchor("commit-menu".into()));
    div()
      .px_4()
      .py_3()
      .border_b_1()
      .border_color(p.border)
      .flex()
      .flex_col()
      .gap_2()
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, window, cx| {
        let k = &e.keystroke;
        if crate::gpui_shell::keys::held(k.modifiers).platform
          || crate::gpui_shell::keys::held(k.modifiers).control
        {
          if k.key == "enter" {
            cx.stop_propagation();
            view.run_commit(CommitAction::Commit, window, cx);
          } else if matches!(k.key.as_str(), "a" | "c" | "x" | "v" | "z" | "y") {
            // The text box's own, not the file list's select-all.
            cx.stop_propagation();
          }
        }
      }))
      .child(
        div()
          .debug_selector(|| "commit-message".into())
          .when(error.is_some(), |this| {
            this.rounded(zpx(6.)).border_1().border_color(p.destructive)
          })
          .children(self.message_input.as_ref().map(Textarea::new)),
      )
      .when_some(error, |this, err| {
        this.child(
          div()
            .debug_selector(|| "commit-message-error".into())
            .text_color(p.destructive)
            .child(err),
        )
      })
      .child(div().flex().w_full().child(main).child(menu))
  }

  #[allow(clippy::too_many_arguments)]
  fn section_header(
    &mut self,
    id: &'static str,
    title: &'static str,
    collapsed: bool,
    color: Hsla,
    p: &Palette,
    cx: &mut Context<Self>,
    on_toggle: impl Fn(&mut Self, &mut Context<Self>) + 'static,
  ) -> Stateful<Div> {
    let fg = p.foreground;
    div()
      .id(id)
      .debug_selector(move || id.into())
      .flex()
      .items_center()
      .gap_1()
      .cursor_pointer()
      .text_color(color)
      .hover(move |s| s.text_color(fg))
      .child(ui::icon(
        if collapsed {
          theme::ICON_CHEVRON_RIGHT
        } else {
          theme::ICON_CHEVRON_DOWN
        },
        12.,
        color,
      ))
      .child(title)
      .on_click(cx.listener(move |view, _, _, cx| on_toggle(view, cx)))
  }

  fn conflicts_section(
    &mut self,
    files: &[String],
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let collapsed = self.collapsed_sections.contains(&Section::Conflicts);
    let red = p.destructive;
    let header = self.section_header(
      "conflicts-section-toggle",
      "Conflicts",
      collapsed,
      red,
      p,
      cx,
      |view, cx| view.toggle_section(Section::Conflicts, cx),
    );
    let mut rows = div().mt_2().overflow_hidden();
    if !collapsed {
      for path in files {
        let active = self.active.as_deref() == Some(path.as_str());
        let (name, dir) = split_path(path);
        let hover = p.accent.opacity(0.3);
        let file = path.clone();
        rows = rows.child(
          div()
            .id(SharedString::from(format!("conflict-row-{path}")))
            .debug_selector({
              let id = format!("conflict-row-{path}");
              move || id
            })
            .py_1()
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .when(active, |this| this.bg(p.accent.opacity(0.4)))
            .when(!active, |this| this.hover(move |s| s.bg(hover)))
            .child(
              div()
                .ml_1()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                  div()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .when(active, |this| this.text_color(red))
                    .child(name),
                )
                .when_some(dir, |this, dir| {
                  this.child(
                    ui::text(div(), TextSize::Xs)
                      .truncate()
                      .text_color(if active {
                        red.opacity(0.7)
                      } else {
                        p.muted_foreground.opacity(0.6)
                      })
                      .child(dir),
                  )
                }),
            )
            .child(div().mr_1().text_color(red).child("C"))
            .on_click(cx.listener(move |view, _, _, cx| view.show_file(&file, cx))),
        );
      }
    }
    div()
      .mt_4()
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .text_color(red)
          .child(header)
          .child(div().ml_1().child(files.len().to_string())),
      )
      .when(!collapsed, |this| this.child(rows))
  }

  fn files_section(
    &mut self,
    list: List,
    files: &[ChangedFile],
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let staged = list == List::Selected;
    let section = if staged {
      Section::Selected
    } else {
      Section::Changes
    };
    let collapsed = self.collapsed_sections.contains(&section);
    let selected = if staged {
      self.staging.selected_staged.clone()
    } else {
      self.staging.selected.clone()
    };
    let header = self.section_header(
      if staged {
        "selected-section-toggle"
      } else {
        "changes-section-toggle"
      },
      if staged { "Selected" } else { "Changes" },
      collapsed,
      p.muted_foreground,
      p,
      cx,
      move |view, cx| {
        view.toggle_section(section, cx);
        view.deselect(list, cx);
      },
    );
    let muted = p.muted_foreground;
    let hover = p.foreground.opacity(0.1);
    let mut buttons = div().flex().items_center().gap_1();
    if !files.is_empty() {
      // `onStashAll` is left out for a remote repository.
      if !staged && !self.parent.remote {
        let stash = header_button("stash-all-changes", theme::ICON_ARCHIVE, muted, hover).on_click(
          cx.listener(|view, _, _, cx| {
            cx.stop_propagation();
            view.stash_all(cx);
          }),
        );
        buttons = buttons.child(self.tips.wrap(
          "stash-all-tip",
          Side::Bottom,
          Some(TooltipContent::Text("Stash all changes".into())),
          stash,
          cx,
        ));
      }
      let discard = header_button(
        if staged {
          "discard-all-selected"
        } else {
          "discard-all-changes"
        },
        theme::ICON_UNDO,
        muted,
        hover,
      )
      .on_click(cx.listener(|view, _, _, cx| {
        cx.stop_propagation();
        view.discard_dialog = true;
        cx.notify();
      }));
      buttons = buttons.child(self.tips.wrap(
        if staged {
          "discard-selected-tip"
        } else {
          "discard-all-tip"
        },
        Side::Bottom,
        Some(TooltipContent::Text("Discard all changes".into())),
        discard,
        cx,
      ));
      if !staged {
        let label = select_all_label(selected.len(), files.len());
        let select = header_button("select-all-changes", theme::ICON_PLUS, muted, hover).on_click(
          cx.listener(|view, _, _, cx| {
            cx.stop_propagation();
            let files = view.files().to_vec();
            view.staging.toggle_all_unstaged(&files);
            cx.notify();
          }),
        );
        buttons = buttons.child(self.tips.wrap(
          "select-all-tip",
          Side::Bottom,
          Some(TooltipContent::Text(label.into())),
          select,
          cx,
        ));
      }
      if !selected.is_empty() {
        let primary = p.primary;
        let n = selected.len();
        let arrow = div()
          .id(if staged {
            "move-selected-files"
          } else {
            "move-changes-files"
          })
          .p_1()
          .rounded(zpx(4.))
          .cursor_pointer()
          .bg(primary.opacity(0.1))
          .hover(move |s| s.bg(primary.opacity(0.2)))
          .child(ui::icon(theme::ICON_ARROW_RIGHT, 12., primary))
          .on_click(cx.listener(|view, _, _, cx| {
            cx.stop_propagation();
            view.move_to_new(cx);
          }));
        buttons = buttons.child(self.tips.wrap(
          if staged {
            "move-selected-tip"
          } else {
            "move-changes-tip"
          },
          Side::Bottom,
          Some(TooltipContent::Text(
            format!("Move {n} file(s) to new workspace").into(),
          )),
          arrow,
          cx,
        ));
      }
      if staged {
        let minus = header_button("unselect-all-changes", theme::ICON_MINUS, muted, p.muted)
          .on_click(cx.listener(|view, _, _, cx| {
            cx.stop_propagation();
            view.staging.unstage_all();
            cx.notify();
          }));
        buttons = buttons.child(self.tips.wrap(
          "unselect-all-tip",
          Side::Bottom,
          Some(TooltipContent::Text("Unselect all changes".into())),
          minus,
          cx,
        ));
      }
    }
    buttons = buttons.child(div().ml_1().child(files.len().to_string()));
    let last = if staged {
      self
        .staging
        .staged_anchor
        .and_then(|i| files.get(i))
        .map(|f| f.path.clone())
    } else {
      self
        .staging
        .anchor
        .and_then(|i| self.files().get(i))
        .map(|f| f.path.clone())
    };
    let mut rows = div().mt_2().overflow_hidden();
    if !collapsed {
      for file in files {
        let is_selected = selected.contains(&file.path);
        let is_last = last.as_deref() == Some(file.path.as_str());
        rows = rows.child(self.file_row(list, file, is_selected, is_last, p, cx));
      }
    }
    div()
      .id(if staged {
        "selected-section"
      } else {
        "changes-section"
      })
      .mt_4()
      .child(
        div()
          .id(if staged {
            "selected-section-header"
          } else {
            "changes-section-header"
          })
          .flex()
          .items_center()
          .justify_between()
          .w_full()
          .text_color(p.muted_foreground)
          .child(header)
          .child(buttons)
          .on_click(cx.listener(move |view, _, _, cx| view.deselect(list, cx))),
      )
      .when(!collapsed && !files.is_empty(), |this| this.child(rows))
  }

  /// `el` with a tooltip of `text` above it.
  fn tip(
    &self,
    key: String,
    text: &'static str,
    el: impl IntoElement,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    self.tips.wrap(
      key,
      Side::Top,
      Some(TooltipContent::Text(text.into())),
      el,
      cx,
    )
  }

  #[allow(clippy::too_many_arguments)]
  fn file_row(
    &mut self,
    list: List,
    file: &ChangedFile,
    selected: bool,
    last: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let staged = list == List::Selected;
    let active = self.active.as_deref() == Some(file.path.as_str());
    let (name, dir) = split_path(&file.path);
    let blue = tw::color(tw::BLUE_500);
    let white = gpui::white();
    let hover = p.accent.opacity(0.2);
    let path = file.path.clone();
    let name_color = if selected {
      white
    } else if active {
      blue
    } else {
      p.foreground
    };
    let dir_color = if active && !selected {
      tw::color(tw::BLUE_400)
    } else if selected {
      white.opacity(0.7)
    } else {
      p.muted_foreground.opacity(0.6)
    };
    let small = |id: String, icon: &'static str, fg: Hsla, hover_bg: Hsla, hidden: bool| {
      let selector = id.clone();
      div()
        .id(SharedString::from(id))
        .debug_selector(move || selector)
        .p(zpx(2.))
        .rounded(zpx(4.))
        .cursor_pointer()
        .when(hidden, |this| {
          this.opacity(0.).group_hover("file-row", |s| s.opacity(1.))
        })
        .hover(move |s| s.bg(hover_bg))
        .child(ui::icon(icon, 12., fg))
    };
    let red = if self.dark {
      tw::color(tw::RED_400)
    } else {
      tw::color(tw::RED_600)
    };
    let mut actions = div().flex().items_center().gap_1().flex_none().child(
      div()
        .min_w(zpx(8.))
        .text_color(p.muted_foreground)
        .child(file.status.clone()),
    );
    if last && selected && !staged {
      let target = path.clone();
      let tip_key = format!("discard-tip-{}", file.path);
      let el = small(
        format!("discard-{}", file.path),
        theme::ICON_UNDO,
        red,
        tw::color(tw::RED_500).opacity(0.2),
        false,
      )
      .on_click(cx.listener(move |view, _, _, cx| {
        cx.stop_propagation();
        view.discard_files(&target, cx);
      }));
      actions = actions.child(self.tip(tip_key, "Discard selected files", el, cx));
      let target = path.clone();
      let tip_key = format!("stage-tip-{}", file.path);
      let el = small(
        format!("stage-{}", file.path),
        theme::ICON_PLUS,
        name_color,
        p.accent,
        true,
      )
      .on_click(cx.listener(move |view, _, _, cx| {
        cx.stop_propagation();
        view.stage(&target, cx);
      }));
      actions = actions.child(self.tip(tip_key, "Stage file(s) for commit", el, cx));
    }
    if staged {
      let target = path.clone();
      let tip_key = format!("unstage-tip-{}", file.path);
      let el = small(
        format!("unstage-{}", file.path),
        theme::ICON_MINUS,
        name_color,
        p.accent,
        true,
      )
      .on_click(cx.listener(move |view, _, _, cx| {
        cx.stop_propagation();
        view.unstage(&target, cx);
      }));
      actions = actions.child(self.tip(tip_key, "Unstage file", el, cx));
    }
    let (click, menu) = (path.clone(), path.clone());
    div()
      .id(SharedString::from(format!(
        "{}-row-{}",
        if staged { "selected" } else { "changes" },
        file.path
      )))
      .debug_selector({
        let id = format!("file-row-{}", file.path);
        move || id
      })
      .group("file-row")
      .relative()
      .min_w_0()
      .py_1()
      .flex()
      .items_center()
      .gap_1()
      .cursor_pointer()
      .border_l_2()
      .when(selected, |this| {
        this
          .bg(blue.opacity(0.6))
          .border_color(tw::color(tw::BLUE_600))
          .font_weight(FontWeight::SEMIBOLD)
      })
      .when(!selected, |this| {
        this
          .border_color(gpui::transparent_black())
          .hover(move |s| s.bg(hover))
      })
      .child(
        div()
          .ml_1()
          .flex_1()
          .flex()
          .items_center()
          .gap_2()
          .min_w_0()
          .child(
            div()
              .min_w_0()
              .truncate()
              .font_weight(FontWeight::MEDIUM)
              .text_color(name_color)
              .child(name),
          )
          .when_some(dir, |this, dir| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .min_w_0()
                .flex_1()
                .truncate()
                .text_color(dir_color)
                .child(dir),
            )
          }),
      )
      .child(actions)
      .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
          view.file_menu(&menu, !staged, e.position, cx);
        }),
      )
      .on_click(cx.listener(move |view, e: &gpui::ClickEvent, _, cx| {
        let m = e.modifiers();
        view.click_file(list, &click, m.shift, m.platform || m.control, cx);
      }))
  }

  fn committed_section(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let collapsed = self.committed_collapsed;
    let total = self.snapshot.as_ref().map_or(0, |s| s.committed.len());
    let visible = self.visible_committed();
    let muted = p.muted_foreground;
    let fg = p.foreground;
    let header = div()
      .id("committed-section-toggle")
      .debug_selector(|| "committed-section-toggle".into())
      .flex()
      .items_center()
      .gap_1()
      .cursor_pointer()
      .hover(move |s| s.text_color(fg))
      .child(ui::icon(
        if collapsed {
          theme::ICON_CHEVRON_RIGHT
        } else {
          theme::ICON_CHEVRON_DOWN
        },
        12.,
        muted,
      ))
      .child("Committed")
      .child(div().ml_1().child(total.to_string()))
      .on_click(cx.listener(|view, _, _, cx| {
        view.committed_collapsed = !view.committed_collapsed;
        cx.notify();
      }));
    let toggle = self.options.show_committed.map(|show| {
      let (color, hover) = if show {
        (
          if self.dark {
            tw::color(tw::BLUE_300)
          } else {
            tw::color(tw::BLUE_700)
          },
          tw::color(tw::BLUE_500).opacity(0.2),
        )
      } else {
        (muted, fg.opacity(0.1))
      };
      let button = ui::text(div(), TextSize::Xs)
        .id("show-committed-toggle")
        .debug_selector(|| "show-committed-toggle".into())
        .flex()
        .items_center()
        .gap_1()
        .px(zpx(6.))
        .py(zpx(2.))
        .rounded(zpx(4.))
        .ml_auto()
        .cursor_pointer()
        .text_color(color)
        .hover(move |s| s.bg(hover))
        .child(ui::icon(
          if show {
            theme::ICON_EYE
          } else {
            theme::ICON_EYE_OFF
          },
          14.,
          color,
        ))
        .child("Show")
        .on_click(cx.listener(move |view, _, _, cx| {
          cx.stop_propagation();
          view.field("changes.showCommitted", (!show).into(), cx);
        }));
      self.tips.wrap(
        "show-committed-tip",
        Side::Top,
        Some(TooltipContent::Text(
          if show {
            "Hide committed changes"
          } else {
            "Show committed changes"
          }
          .into(),
        )),
        button,
        cx,
      )
    });
    let mut rows = div().mt_2().overflow_hidden();
    if !collapsed {
      for file in &visible {
        let (name, dir) = split_path(&file.path);
        let active = self.active.as_deref() == Some(file.path.as_str());
        let hover = p.accent.opacity(0.2);
        let path = file.path.clone();
        let menu = file.path.clone();
        rows = rows.child(
          div()
            .id(SharedString::from(format!("committed-row-{}", file.path)))
            .py_1()
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .border_l_2()
            .border_color(gpui::transparent_black())
            .hover(move |s| s.bg(hover))
            .child(
              div()
                .ml_1()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                  div()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .when(active, |this| this.text_color(tw::color(tw::BLUE_500)))
                    .child(name),
                )
                .when_some(dir, |this, dir| {
                  this.child(
                    ui::text(div(), TextSize::Xs)
                      .flex_1()
                      .truncate()
                      .text_color(p.muted_foreground.opacity(0.6))
                      .child(dir),
                  )
                }),
            )
            .child(div().text_color(muted).child(file.status.clone()))
            .on_mouse_down(
              MouseButton::Right,
              cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
                view.file_menu(&menu, false, e.position, cx);
              }),
            )
            .on_click(cx.listener(move |view, _, _, cx| view.show_file(&path, cx))),
        );
      }
    }
    div()
      .mt_4()
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .w_full()
          .text_color(muted)
          .child(header)
          .children(toggle),
      )
      .when(!collapsed && !visible.is_empty(), |this| this.child(rows))
  }

  /// `ReviewActionBar`: for now, "Start Review" on workspaces.
  fn action_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    self.parent.workspace.as_ref()?;
    let bar = if self.show_review_bar() {
      self.review_bar(p, cx).into_any_element()
    } else {
      div()
        .flex()
        .items_center()
        .justify_end()
        .gap_2()
        .px_4()
        .py_2()
        .bg(p.muted.opacity(0.8))
        .border_b_1()
        .border_color(p.border)
        .children(self.agent_count())
        .child(self.start_review_button(p, cx))
        .into_any_element()
    };
    Some(
      div()
        .flex()
        .flex_col()
        .children(self.stale_banner(p, cx))
        .child(bar),
    )
  }

  /// `DiffContentArea`.
  fn content(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let area = div().flex_1().overflow_hidden().relative();
    if let Some(err) = self.pr.as_ref().and_then(|pr| pr.error.clone()) {
      return area.child(
        ui::text(div(), TextSize::Sm)
          .debug_selector(|| "pr-diff-error".into())
          .p_6()
          .text_color(p.destructive)
          .child(format!("Failed to load the PR diff: {err}")),
      );
    }
    let Some(snapshot) = self.snapshot.clone() else {
      let card = |p: &Palette| {
        div()
          .flex()
          .flex_col()
          .gap_2()
          .rounded(zpx(8.))
          .border_1()
          .border_color(p.border)
          .p_3()
          .child(skeleton(p).h_4().w(gpui::relative(1. / 3.)))
          .child(skeleton(p).h_3().w_full())
          .child(skeleton(p).h_3().w(gpui::relative(5. / 6.)))
      };
      return area.child(
        div()
          .debug_selector(|| "changes-loading-skeleton".into())
          .size_full()
          .p_4()
          .flex()
          .flex_col()
          .gap_4()
          .child(card(p))
          .child(card(p))
          .child(card(p)),
      );
    };
    let committed = self.visible_committed();
    if snapshot.files.is_empty() && committed.is_empty() {
      return area.child(
        div()
          .size_full()
          .flex()
          .flex_col()
          .items_center()
          .justify_center()
          .text_color(p.muted_foreground)
          .child(ui::icon(
            theme::ICON_CIRCLE_CHECK,
            48.,
            p.muted_foreground.opacity(0.4),
          ))
          .child(div().mt_3().child("No changes to review")),
      );
    }
    let total = total_lines(
      snapshot
        .files
        .iter()
        .filter_map(|f| self.hunks.get(&f.path)),
    );
    if total > LARGE_CHANGESET_LINES && !self.changeset_expanded {
      let n = snapshot.files.len();
      return area.child(
        div()
          .size_full()
          .flex()
          .flex_col()
          .items_center()
          .justify_center()
          .gap_4()
          .text_color(p.muted_foreground)
          .child(ui::icon(
            theme::ICON_FILE_TEXT,
            48.,
            p.muted_foreground.opacity(0.5),
          ))
          .child(
            div()
              .flex()
              .flex_col()
              .items_center()
              .child(
                div()
                  .font_weight(FontWeight::MEDIUM)
                  .mb_1()
                  .child("Large changeset"),
              )
              .child(format!(
                "{total} lines across {n} file{}",
                if n == 1 { "" } else { "s" }
              )),
          )
          .child(
            Button::new("view-large-changeset", p)
              .variant(Variant::Outline)
              .label("View changes")
              .on_click(cx.listener(|view, _, _, cx| {
                view.changeset_expanded = true;
                cx.notify();
              })),
          ),
      );
    }
    let conflicted: HashSet<String> = self.conflicted().into_iter().collect();
    let mut list = div().pb_32();
    for file in &snapshot.files {
      list = list.child(self.file_card(
        &file.path,
        &file.status,
        file.old_path.as_deref(),
        false,
        &conflicted,
        p,
        cx,
      ));
    }
    for file in &committed {
      list = list.child(self.file_card(
        &file.path,
        &file.status,
        file.previous_path.as_deref(),
        true,
        &conflicted,
        p,
        cx,
      ));
    }
    area.child(
      div().size_full().px_4().child(
        div()
          .id("changes-diff-scroll")
          .size_full()
          .overflow_y_scroll()
          .track_scroll(&self.scroll)
          .child(list),
      ),
    )
  }

  #[allow(clippy::too_many_arguments)]
  fn file_card(
    &mut self,
    path: &str,
    status: &str,
    old_path: Option<&str>,
    committed: bool,
    conflicted: &HashSet<String>,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let data = self.file_hunks(path);
    let input = BodyInput {
      path,
      status,
      renamed: old_path.is_some(),
      data: data.as_ref(),
      collapsed: self.collapsed_files.contains(path),
      comment_activity: self.file_comment_activity(path),
      expanded_large: self.expanded_large.contains(path),
    };
    let shut = is_collapsed(&input);
    let kind = body(&input);
    let (additions, deletions) = data
      .as_ref()
      .filter(|d| !d.loading)
      .map_or((0, 0), |d| line_counts(&d.hunks));
    let header = self.file_header(
      path,
      status,
      old_path,
      committed,
      shut,
      (additions, deletions),
      p,
      cx,
    );
    let cards = self.cards.clone();
    let key = path.to_string();
    let mut card = div()
      .debug_selector({
        let id = format!("file-card-{path}");
        move || id
      })
      .relative()
      .mb_4()
      .rounded(zpx(12.))
      .border_1()
      .border_color(p.border)
      .overflow_hidden()
      .bg(p.background)
      .child(
        gpui::canvas(
          move |bounds, _, _| {
            cards.borrow_mut().insert(key.clone(), bounds);
          },
          |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
      )
      .child(header)
      .children(self.file_comments(path, p, cx));
    if kind == Body::Collapsed {
      return card;
    }
    let conflicted_file = conflicted.contains(path);
    let muted = p.muted_foreground;
    let placeholder = |el: Div| div().font_family(theme::MONO_FONT).child(el);
    card = match kind {
      Body::Binary | Body::Deleted => card.child(placeholder(
        div()
          .when(kind == Body::Deleted, |this| {
            this.debug_selector(|| "deleted-file-placeholder".into())
          })
          .flex()
          .items_center()
          .justify_center()
          .py(zpx(32.))
          .text_color(muted)
          .child(
            div()
              .mr_2()
              .child(ui::icon(theme::ICON_FILE_TEXT, 20., muted.opacity(0.5))),
          )
          .child(if kind == Body::Binary {
            "Binary file - no diff available"
          } else {
            "File deleted"
          }),
      )),
      Body::Loading => card.child(
        div()
          .flex()
          .flex_col()
          .gap_2()
          .px_3()
          .py_4()
          .child(skeleton(p).h_4().w(gpui::relative(0.75)))
          .child(skeleton(p).h_4().w(gpui::relative(0.5)))
          .child(skeleton(p).h_4().w(gpui::relative(5. / 6.))),
      ),
      Body::Error => card.child(
        div()
          .px_3()
          .py_2()
          .text_color(p.destructive)
          .child(data.and_then(|d| d.error).unwrap_or_default()),
      ),
      Body::Empty => card.child(
        div()
          .debug_selector(|| "empty-diff-placeholder".into())
          .px_3()
          .py_6()
          .flex()
          .justify_center()
          .text_color(muted)
          .child(if conflicted_file {
            "No diff available for this conflicted file (possibly deleted)"
          } else {
            "No diff hunks available"
          }),
      ),
      Body::Large(n) => {
        let file = path.to_string();
        card.child(
          div()
            .h_20()
            .flex()
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(muted)
            .child(ui::icon(theme::ICON_FILE_TEXT, 20., muted.opacity(0.5)))
            .child(format!("Large diff ({n} lines)"))
            .child(
              Button::new(SharedString::from(format!("view-large-{path}")), p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .label("View changes")
                .on_click(cx.listener(move |view, _, _, cx| view.toggle_large(&file, cx))),
            ),
        )
      }
      Body::Hunks => {
        let hunks = data.map(|d| d.hunks).unwrap_or_default();
        card
          .children(self.unplaced_threads(path, p, cx))
          .children(self.outdated_comments(path, p, cx))
          .child(self.hunks(path, &hunks, committed, conflicted_file, p, cx))
      }
      Body::Collapsed => card,
    };
    card
  }

  #[allow(clippy::too_many_arguments)]
  fn file_header(
    &mut self,
    path: &str,
    status: &str,
    old_path: Option<&str>,
    committed: bool,
    shut: bool,
    (additions, deletions): (usize, usize),
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let muted = p.muted_foreground;
    let fg = p.foreground;
    let renamed = old_path.is_some();
    let viewed = self.viewed.contains_key(path);
    let dark = self.dark;
    let shade = |light: u32, dark_: u32| tw::color(if dark { dark_ } else { light });
    let badge = |text: &'static str, fg: Hsla, bg: Hsla| {
      div()
        .flex_none()
        .px_2()
        .py(zpx(2.))
        .rounded(zpx(4.))
        .bg(bg)
        .text_color(fg)
        .child(text)
    };
    let file = path.to_string();
    let chevron = if renamed {
      div().size_3().flex_none().into_any_element()
    } else {
      div()
        .id(SharedString::from(format!("collapse-{path}")))
        .cursor_pointer()
        .child(ui::icon(
          if shut {
            theme::ICON_CHEVRON_RIGHT
          } else {
            theme::ICON_CHEVRON_DOWN
          },
          12.,
          fg,
        ))
        .on_click(cx.listener(move |view, _, _, cx| {
          cx.stop_propagation();
          view.toggle_file(&file, cx);
        }))
        .into_any_element()
    };
    let shown_path = match old_path {
      Some(old) => format!("{old} => {path}"),
      None => path.trim_end_matches('/').to_string(),
    };
    let copy_path = path.to_string();
    let left = div()
      .flex()
      .items_center()
      .gap_2()
      .flex_1()
      .min_w_0()
      .child(chevron)
      .child(
        div()
          .min_w_0()
          .flex_1()
          .flex()
          .items_center()
          .gap(zpx(6.))
          .child(
            div()
              .truncate()
              .font_family(theme::MONO_FONT)
              .text_color(muted)
              .child(shown_path),
          )
          .child(
            self.tip(
              format!("copy-path-tip-{path}"),
              "Copy file path",
              div()
                .id(SharedString::from(format!("copy-path-{path}")))
                .flex_none()
                .cursor_pointer()
                .child(ui::icon(theme::ICON_COPY, 16., muted))
                .on_click(cx.listener(move |view, _, _, cx| {
                  cx.stop_propagation();
                  view.copy(
                    copy_path.clone(),
                    "File path copied to clipboard".into(),
                    cx,
                  );
                })),
              cx,
            ),
          )
          .when(committed, |this| {
            this.child(
              badge(
                "Committed",
                shade(tw::SKY_700, tw::SKY_300),
                tw::color(tw::SKY_500).opacity(0.2),
              )
              .debug_selector(|| "committed-file-label".into()),
            )
          }),
      );
    let accent = p.accent;
    let mut right = div().flex().items_center().gap_2();
    if !committed && !self.read_only() {
      right = right.child(
        self.tip(
          format!("add-file-comment-tip-{path}"),
          "Add file comment",
          div()
            .id(SharedString::from(format!("add-file-comment-{path}")))
            .debug_selector(|| "add-file-comment-button".into())
            .p_1()
            .rounded(zpx(4.))
            .cursor_pointer()
            .hover(move |s| s.bg(accent))
            .child(ui::icon(theme::ICON_MESSAGE_PLUS, 14., muted))
            .on_click({
              let path = path.to_string();
              cx.listener(move |view, _, _, cx| view.add_file_comment(&path, cx))
            }),
          cx,
        ),
      );
    }
    let (viewed_fg, viewed_bg, viewed_hover) = if viewed {
      (
        shade(tw::EMERALD_700, tw::EMERALD_300),
        tw::color(tw::EMERALD_500).opacity(0.25),
        tw::color(tw::EMERALD_500).opacity(0.35),
      )
    } else {
      (muted, p.muted, accent)
    };
    let file = path.to_string();
    right = right.child(
      self.tip(
        format!("viewed-tip-{path}"),
        if viewed {
          "Mark as not viewed"
        } else {
          "Mark as viewed"
        },
        div()
          .id(SharedString::from(format!("viewed-{path}")))
          .debug_selector({
            let id = format!("viewed-{path}");
            move || id
          })
          .flex()
          .items_center()
          .gap_1()
          .px_2()
          .py(zpx(2.))
          .rounded(zpx(4.))
          .cursor_pointer()
          .bg(viewed_bg)
          .text_color(viewed_fg)
          .hover(move |s| s.bg(viewed_hover))
          .child(ui::icon(
            if viewed {
              theme::ICON_CHECK
            } else {
              theme::ICON_SQUARE
            },
            12.,
            viewed_fg,
          ))
          .child("Viewed")
          .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            view.toggle_viewed(&file, cx);
          })),
        cx,
      ),
    );
    if renamed {
      right = right.child(badge(
        "Renamed",
        shade(tw::BLUE_700, tw::BLUE_300),
        tw::color(tw::BLUE_500).opacity(0.25),
      ));
    }
    let deleted = status == "D";
    if deleted {
      right = right.child(
        badge(
          "Deleted",
          shade(tw::RED_700, tw::RED_300),
          tw::color(tw::RED_500).opacity(0.25),
        )
        .debug_selector(|| "deleted-file-badge".into()),
      );
    }
    if super::changes_data::is_binary(path) {
      right = right.child(badge(
        "Binary",
        shade(tw::ZINC_700, tw::ZINC_300),
        tw::color(tw::ZINC_500).opacity(0.25),
      ));
    }
    if !deleted && (additions > 0 || deletions > 0) {
      right = right.child(
        div()
          .flex()
          .items_center()
          .gap_1()
          .font_family(theme::MONO_FONT)
          .child(
            div()
              .text_color(shade(tw::EMERALD_700, tw::EMERALD_300))
              .child(format!("+{additions}")),
          )
          .child(
            div()
              .text_color(shade(tw::RED_700, tw::RED_300))
              .child(format!("-{deletions}")),
          ),
      );
    }
    if !committed && !self.read_only() {
      let key = format!("more-{path}");
      let (file, anchor) = (path.to_string(), key.clone());
      right = right.child(
        div()
          .id(SharedString::from(key.clone()))
          .relative()
          .p_1()
          .rounded(zpx(4.))
          .cursor_pointer()
          .hover(move |s| s.bg(accent))
          .child(ui::icon(theme::ICON_MORE, 12., fg))
          .child(self.anchor(key))
          .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            view.more_menu(&file, &anchor, cx);
          })),
      );
    }
    let menu = path.to_string();
    div()
      .id(SharedString::from(format!("file-header-{path}")))
      .flex()
      .items_center()
      .justify_between()
      .px_4()
      .py_2()
      .bg(p.muted)
      .when(!shut, |this| this.border_b_1().border_color(p.border))
      .child(left)
      .child(right)
      .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
          view.file_menu(&menu, false, e.position, cx);
        }),
      )
  }

  /// The hunks of a file: each header, the lines, and context read above
  /// and below.
  #[allow(clippy::too_many_arguments)]
  fn hunks(
    &mut self,
    path: &str,
    hunks: &[JjDiffHunk],
    committed: bool,
    conflicted: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let language = highlight::language(path);
    let regions: Vec<Conflict> = conflicts(path, hunks);
    let in_region = conflict_lines(&regions);
    let mut drawn_regions: HashSet<usize> = HashSet::new();
    let border = p.border.opacity(0.4);
    let muted = p.muted_foreground;
    let mut el = div().font_family(theme::MONO_FONT);
    let gutter_cols = |el: Stateful<Div>| {
      el.child(div().w_24().flex_none().border_r_1().border_color(border))
        .child(div().w_6().flex_none())
        .child(div().w_5().flex_none())
    };
    for (h, hunk) in hunks.iter().enumerate() {
      let before = self
        .context
        .get(&(path.to_string(), h, Dir::Before))
        .cloned()
        .unwrap_or_default();
      let after = self
        .context
        .get(&(path.to_string(), h, Dir::After))
        .cloned()
        .unwrap_or_default();
      let expandable = !committed && can_expand_before(&hunk.header, before.len());
      let file = path.to_string();
      let hover = p.muted.opacity(0.8);
      el = el.child(
        gutter_cols(
          div()
            .id(SharedString::from(format!("hunk-header-{path}-{h}")))
            .flex()
            .bg(p.muted.opacity(0.6))
            .when(expandable, |this| {
              this.cursor_pointer().hover(move |s| s.bg(hover)).on_click(
                cx.listener(move |view, _, _, cx| view.expand_context(&file, h, Dir::Before, cx)),
              )
            }),
        )
        .child(
          div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .px_2()
            .py(zpx(2.))
            .when(expandable, |this| {
              this.child(
                div()
                  .mr_1()
                  .child(ui::icon(theme::ICON_CHEVRON_UP, 12., muted)),
              )
            })
            .child(
              div()
                .truncate()
                .text_color(muted)
                .child(hunk.header.clone()),
            )
            .when(expandable, |this| {
              this.child(
                ui::text(div(), TextSize::Xs)
                  .ml_2()
                  .flex_none()
                  .text_color(muted)
                  .child("Show more lines above"),
              )
            }),
        ),
      );
      let (start, count) = super::changes_data::new_range(&hunk.header);
      for (i, line) in before.iter().enumerate() {
        let n = start as i64 - before.len() as i64 + i as i64;
        el = el.child(context_row(n, line, language, p));
      }
      let numbers = line_numbers(hunk);
      for l in 0..hunk.lines.len() {
        let new = numbers.get(l).and_then(|n| n.1);
        if let Some(region) = new.and_then(|n| in_region.get(&(n as usize)).copied()) {
          if regions[region].region.start_line == new.unwrap_or(0) as usize
            && drawn_regions.insert(region)
          {
            el = el.child(self.conflict_card(&regions[region], p, cx));
          }
          continue;
        }
        el = el.child(
          self
            .diff_line(path, hunks, h, l, language, conflicted, p, cx)
            .on_mouse_down(
              MouseButton::Right,
              cx.listener(|view, e: &gpui::MouseDownEvent, _, cx| view.line_menu(e.position, cx)),
            ),
        );
        el = el.children(self.line_extras(path, hunks, h, l, p, cx));
      }
      // Drawn for committed files too, where React's click finds no hunk.
      if !(self
        .context
        .contains_key(&(path.to_string(), h, Dir::After))
        && after.is_empty())
      {
        let file = path.to_string();
        let hover = p.muted.opacity(0.5);
        el = el.child(
          gutter_cols(
            div()
              .id(SharedString::from(format!("expand-after-{path}-{h}")))
              .flex()
              .bg(p.muted.opacity(0.3))
              .cursor_pointer()
              .hover(move |s| s.bg(hover))
              .on_click(cx.listener(move |view, _, _, cx| {
                if !committed {
                  view.expand_context(&file, h, Dir::After, cx)
                }
              })),
          )
          .child(
            ui::text(div(), TextSize::Xs)
              .flex_1()
              .flex()
              .items_center()
              .justify_center()
              .gap_1()
              .px_2()
              .py(zpx(2.))
              .text_color(muted)
              .child(ui::icon(theme::ICON_CHEVRON_DOWN, 12., muted))
              .child("Show more lines below"),
          ),
        );
      }
      for (i, line) in after.iter().enumerate() {
        el = el.child(context_row((start + count + i) as i64, line, language, p));
      }
    }
    el
  }

  /// `InlineConflictCard`: the region's lines by side.
  fn conflict_card(
    &self,
    conflict: &Conflict,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let region = &conflict.region;
    let dark = self.dark;
    let shade = |light: u32, dark_: u32| tw::color(if dark { dark_ } else { light });
    let (left_deleted, right_deleted) = deleted_sides(region);
    let mut pre = div()
      .font_family(theme::MONO_FONT)
      .bg(p.muted.opacity(0.3))
      .p_3()
      .rounded(zpx(4.));
    let lines: Vec<(String, ConflictLineKind, ConflictLineRole)> = if region.lines.is_empty() {
      region
        .content
        .split('\n')
        .map(|l| {
          (
            l.to_string(),
            ConflictLineKind::Content,
            ConflictLineRole::Unknown,
          )
        })
        .collect()
    } else {
      region
        .lines
        .iter()
        .map(|l| (l.raw.clone(), l.kind, l.role))
        .collect()
    };
    let mut current = ConflictLineRole::Unknown;
    for (i, (raw, kind, role)) in lines.into_iter().enumerate() {
      if kind == ConflictLineKind::Marker {
        current = role;
      }
      let side = if kind == ConflictLineKind::Content {
        role
      } else {
        current
      };
      let skip = kind == ConflictLineKind::Content
        && ((role == ConflictLineRole::Left && left_deleted)
          || (role == ConflictLineRole::Right && right_deleted));
      if skip {
        continue;
      }
      let bg = match side {
        ConflictLineRole::Left => Some(tw::color(tw::RED_500).opacity(0.2)),
        ConflictLineRole::Right => Some(tw::color(tw::EMERALD_500).opacity(0.2)),
        ConflictLineRole::Base => Some(tw::color(tw::AMBER_500).opacity(0.2)),
        _ => None,
      };
      let label = (kind == ConflictLineKind::Marker)
        .then(|| section_label(region, role))
        .flatten();
      let label_el = label.map(|label| {
        let (fg, bg) = match label {
          "Side #1" => (
            shade(tw::RED_700, tw::RED_300),
            tw::color(tw::RED_500).opacity(0.2),
          ),
          "Base" => (
            shade(tw::AMBER_700, tw::AMBER_300),
            tw::color(tw::AMBER_500).opacity(0.2),
          ),
          _ => (
            shade(tw::EMERALD_700, tw::EMERALD_300),
            tw::color(tw::EMERALD_500).opacity(0.2),
          ),
        };
        ui::text(div(), TextSize::Xs)
          .debug_selector(move || format!("conflict-label-{label}"))
          .mr_2()
          .px(zpx(6.))
          .py(zpx(2.))
          .rounded(zpx(4.))
          .font_family(theme::SANS_FONT)
          .font_weight(FontWeight::MEDIUM)
          .bg(bg)
          .text_color(fg)
          .child(label)
      });
      let line_id = format!("conflict-line-{}-{i}-{side:?}", conflict.id);
      pre = pre.child(
        div()
          .debug_selector(move || line_id)
          .flex()
          .when_some(bg, |this, bg| this.bg(bg))
          .when(kind == ConflictLineKind::Marker, |this| {
            this.text_color(p.muted_foreground)
          })
          .children(label_el)
          .child(div().flex_1().min_w_0().child(self.marked_text(
            search::MatchKey::Conflict(conflict.id.clone(), i),
            &raw,
            None,
          ))),
      );
      let deleted_notice = |side: &'static str, fg: Hsla, border: Hsla, bg: Hsla| {
        div()
          .debug_selector(|| "conflict-deleted-side".into())
          .my_1()
          .rounded(zpx(8.))
          .border_1()
          .border_color(border)
          .bg(bg)
          .px_3()
          .py_2()
          .font_family(theme::SANS_FONT)
          .text_color(fg)
          .child(format!("{side} deleted this file"))
      };
      if kind == ConflictLineKind::Marker && role == ConflictLineRole::Start && left_deleted {
        pre = pre.child(deleted_notice(
          "Side #1",
          shade(tw::RED_700, tw::RED_300),
          tw::color(tw::RED_500).opacity(0.4),
          tw::color(tw::RED_500).opacity(0.1),
        ));
      }
      if kind == ConflictLineKind::Marker && role == ConflictLineRole::Separator && right_deleted {
        pre = pre.child(deleted_notice(
          "Side #2",
          shade(tw::EMERALD_700, tw::EMERALD_300),
          tw::color(tw::EMERALD_500).opacity(0.4),
          tw::color(tw::EMERALD_500).opacity(0.1),
        ));
      }
    }
    let number = if region.total_conflicts > 0 {
      format!("Conflict {} of {}", conflict.number, region.total_conflicts)
    } else {
      format!("Conflict {}", conflict.number)
    };
    div()
      .debug_selector({
        let id = format!("conflict-card-{}", conflict.id);
        move || id
      })
      .bg(p.destructive.opacity(0.1))
      .border_y_1()
      .border_color(p.destructive.opacity(0.4))
      .px_4()
      .py_3()
      .flex()
      .flex_col()
      .gap_2()
      .font_family(theme::SANS_FONT)
      .child(
        ui::text(div(), TextSize::Xs)
          .flex()
          .items_center()
          .gap_2()
          .text_color(p.muted_foreground)
          .child(
            div()
              .font_weight(FontWeight::MEDIUM)
              .text_color(p.destructive)
              .child(number),
          )
          .child(format!("lines {}–{}", region.start_line, region.end_line)),
      )
      .child(pre)
      .children(self.note_section(conflict, p, cx))
  }

  fn dialog(
    &mut self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    if !self.discard_dialog {
      return None;
    }
    let viewport = window.viewport_size();
    let n = self.staging.unstaged(self.files()).len();
    let panel = div()
      .id("discard-all-dialog")
      .debug_selector(|| "discard-all-dialog".into())
      .w_full()
      .max_w(zpx(512.))
      .p_6()
      .rounded(zpx(16.))
      .border_1()
      .border_color(p.border.opacity(0.5))
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_2xl()
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        if e.keystroke.key == "escape" {
          cx.stop_propagation();
          view.discard_dialog = false;
          cx.notify();
        }
      }))
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .child(
        div()
          .flex()
          .flex_col()
          .gap(zpx(6.))
          .child(
            ui::text(div(), TextSize::Lg)
              .line_height(zpx(15.))
              .font_weight(FontWeight::SEMIBOLD)
              .child("Discard all changes?"),
          )
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child(format!(
                "This will discard all {n} unsaved {}. You can restore them from the Undo button on \
                 the toast that follows.",
                if n == 1 { "change" } else { "changes" }
              )),
          ),
      )
      .child(
        div()
          .mt_4()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("discard-all-cancel", p)
              .variant(Variant::Ghost)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| {
                view.discard_dialog = false;
                cx.notify();
              })),
          )
          .child(
            Button::new("discard-all-confirm", p)
              .variant(Variant::Destructive)
              .label("Discard all changes")
              .on_click(cx.listener(|view, _, _, cx| view.discard_all(cx))),
          ),
      );
    Some(
      deferred(
        anchored().position(point(px(0.), px(0.))).child(
          div()
            .id("discard-all-backdrop")
            .w(viewport.width)
            .h(viewport.height)
            .p_4()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.4))
            .occlude()
            .on_mouse_down(
              MouseButton::Left,
              cx.listener(|view, _, _, cx| {
                view.discard_dialog = false;
                cx.notify();
              }),
            )
            .child(panel),
        ),
      )
      .with_priority(2),
    )
  }
}

/// `Skeleton`: `animate-pulse rounded-md bg-primary/10`.
fn skeleton(p: &Palette) -> Div {
  div().rounded(zpx(8.)).bg(p.primary.opacity(0.1))
}

/// `HunkContextLineRow`: a line read by "Show more lines".
fn context_row(
  n: i64,
  line: &str,
  language: Option<&'static str>,
  p: &Palette,
) -> impl IntoElement {
  let number = if n > 0 { n.to_string() } else { String::new() };
  let border = p.border.opacity(0.4);
  div()
    .flex()
    .child(
      div()
        .w_24()
        .flex_none()
        .flex()
        .items_center()
        .border_r_1()
        .border_color(border)
        .text_color(p.muted_foreground)
        .child(
          div()
            .w_10()
            .mr_1()
            .flex()
            .justify_end()
            .child(number.clone()),
        )
        .child(div().w_10().flex().justify_end().child(number)),
    )
    .child(div().w_6().flex_none())
    .child(div().w_5().flex_none())
    .child(
      div()
        .flex_1()
        .min_w_0()
        .px_2()
        .py(zpx(2.))
        .text_color(p.muted_foreground)
        .child(highlight::styled(
          if line.is_empty() { " " } else { line },
          language,
          p.dark,
        )),
    )
}
