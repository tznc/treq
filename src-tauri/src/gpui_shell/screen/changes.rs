//! The workspace Changes tab (`ChangesDiffViewer`): the sidebar with the
//! commit box and the file lists, and the diff of every changed file.
//!
//! Drawn from the backend directly ([`changes_data`]). What stays React's
//! goes back to it as screen actions: moving files to another workspace,
//! the agent terminal a review starts in, and creating the pull request.
//!
//! [`changes_data`]: super::changes_data

mod keys;
pub(in crate::gpui_shell::screen) mod pr;
mod review;
mod rows;
pub(in crate::gpui_shell::screen) mod search;
pub(super) mod threads;

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
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use serde::Deserialize;
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, point, prelude::*, px, App, Bounds, ClipboardItem, Context, Div,
  Entity, FocusHandle, FontWeight, Hsla, KeyDownEvent, MouseButton, Pixels, SharedString, Stateful,
  Subscription, Window,
};

use super::{
  changes_data::{
    body, check_message, commit_label, conflict_lines, conflicts, context_ranges, deleted_sides,
    diff_summary, discarded, draws_hunks, full_path, gap_ranges, hunks_hash, pending_label,
    review_prompt, section_label, select_all_label, split_path, stashed, total_lines,
    visible_committed_at, Body, BodyInput, ChangedFile, ChangesHost, CommitAction, Conflict,
    ContextReads, EditorApps, FileHunks, Host, List, Snapshot, Staging, BATCH, EXPAND_ALL_MAX,
    FIRST_BATCH, HOME_ENDPOINT, LARGE_CHANGESET_LINES,
  },
  code::Parent,
  commits_data::{line_kind, line_numbers, LineKind},
  highlight,
  rows::Rows as ListRows,
  ActionSink, ScreenAction,
};
use crate::{
  conflict_markers::{ConflictLineKind, ConflictLineRole},
  gpui_shell::{
    native::{self, NativeEvent, ToastAction, ToastTone},
    popup::{self, MenuIcon, NativeMenuItem, PopupRequest, PopupSink, Side, TooltipContent},
    theme::{self, Palette},
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Tone, Variant},
  },
  jj::{JjDiffHunk, JjFileChange},
};

/// Hunk batches landing closer together than this redraw once: a frame of
/// a large changeset costs more than a batch takes to read.
const BATCH_REDRAW: Duration = Duration::from_millis(100);

/// Opacity of a viewed file's name in the file lists.
const VIEWED_OPACITY: f32 = 0.5;

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

/// One row of the sidebar's virtual list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideRow {
  Filter,
  ConflictsHeader,
  /// By index into [`SideData::conflicted`].
  Conflict(usize),
  Header(List),
  /// By index into the list's files in [`SideData`].
  File(List, usize),
  CommittedHeader,
  /// By index into [`SideData::committed`].
  Committed(usize),
  /// Nothing changed, nothing committed.
  Empty,
}

/// What the sidebar's rows read, by index into the snapshot's files.
#[derive(Default)]
struct SideData {
  conflicted: Vec<String>,
  staged: Vec<usize>,
  unstaged: Vec<usize>,
  /// Into the snapshot's committed files.
  committed: Vec<usize>,
  /// The row that shows the file buttons, by index into the snapshot's files.
  last_staged: Option<usize>,
  last_changed: Option<usize>,
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
  /// The working-copy diff every file in `hunks` was read from, once all
  /// were read from the same one.
  hunks_token: Option<String>,
  /// When a hunk batch last redrew, and whether a redraw of the batches
  /// since is waiting on a timer.
  batch_drawn: Option<Instant>,
  batch_flush: bool,
  generation: u64,
  /// Bumped when the snapshot, the hunks, the lines read around them or
  /// the parent change: what the diff's rows are laid out from.
  pub(super) layout_gen: u64,
  pub(super) staging: Staging,
  pub(super) active: Option<String>,
  collapsed_sections: HashSet<Section>,
  committed_collapsed: bool,
  pub(super) collapsed_files: HashSet<String>,
  pub(super) expanded_large: HashSet<String>,
  /// Generated files whose diff "Load diff" read.
  pub(super) loaded_generated: HashSet<String>,
  /// The file list's filter: generated files left out of the list and diff.
  pub(super) hide_generated: bool,
  pub(super) changeset_expanded: bool,
  /// Files marked viewed, with the diff's hash then.
  pub(super) viewed: HashMap<String, String>,
  /// Lines read by "Show more lines", by file, hunk and side.
  pub(super) context: HashMap<(String, usize, Dir), Vec<String>>,
  /// Hunks whose last read below came back short: the file ends there.
  context_ended: HashSet<(String, usize)>,
  pub(super) message: String,
  message_input: Option<Entity<TextareaState>>,
  pub(super) message_error: Option<&'static str>,
  pub(super) pending: Option<CommitAction>,
  /// The file a discard runs on.
  file_action: Option<String>,
  pub(super) discard_dialog: bool,
  /// Focus moves into the open confirmation so Escape reaches it.
  discard_focus: Option<FocusHandle>,
  starting_review: bool,
  /// The repository has a GitHub remote, and the branch's pull request.
  github: bool,
  pr_state: Option<String>,
  editors: EditorApps,
  /// The diff's rows and the list drawing them.
  diff: rows::DiffRows,
  /// The sidebar's rows and the list drawing them.
  side: ListRows<SideRow>,
  side_data: SideData,
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
  /// Held by the diff, so it takes the arrow keys.
  focus: FocusHandle,
  /// Hidden while another tab or workspace shows.
  hidden: bool,
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
    let mut this = Self::blank(parent, options, host, react, popups, dark, cx);
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
    cx: &mut Context<Self>,
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
      hunks_token: None,
      batch_drawn: None,
      batch_flush: false,
      generation: 0,
      layout_gen: 0,
      staging: Staging::default(),
      active: None,
      collapsed_sections: HashSet::new(),
      committed_collapsed: false,
      collapsed_files: HashSet::new(),
      expanded_large: HashSet::new(),
      loaded_generated: HashSet::new(),
      hide_generated: false,
      changeset_expanded: false,
      viewed: HashMap::new(),
      context: HashMap::new(),
      context_ended: HashSet::new(),
      message: String::new(),
      message_input: None,
      message_error: None,
      pending: None,
      file_action: None,
      discard_dialog: false,
      discard_focus: None,
      starting_review: false,
      github: false,
      pr_state: None,
      editors,
      diff: rows::DiffRows::default(),
      side: ListRows::default(),
      side_data: SideData::default(),
      scroll_to: None,
      anchors: Rc::default(),
      review: review::Review::default(),
      search: search::Search::default(),
      threads: threads::Threads::default(),
      pr: None,
      focus: cx.focus_handle(),
      hidden: false,
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
    // The status's conflicts stand in until the diff lists its own.
    if moved || self.parent.conflicted_files != parent.conflicted_files {
      self.layout_gen += 1;
    }
    self.parent = parent;
    self.options = options;
    if moved {
      self.snapshot = None;
      self.hunks.clear();
      self.hunks_token = None;
      self.staging = Staging::default();
      self.active = None;
      self.collapsed_files.clear();
      self.expanded_large.clear();
      self.loaded_generated.clear();
      self.changeset_expanded = false;
      self.viewed.clear();
      self.context.clear();
      self.context_ended.clear();
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

  /// Its first read landed.
  pub(super) fn ready(&self) -> bool {
    self.snapshot.is_some()
  }

  /// Hidden while another tab or workspace shows, it stops polling its
  /// threads and hearing Cmd+F; shown again, it reads the diff, the threads and the pull
  /// request again behind what it draws.
  pub(super) fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
    if self.hidden != shown {
      return;
    }
    self.hidden = !shown;
    if !shown {
      self.stop_threads();
      self.unwatch_find_key();
      return;
    }
    self.poll_threads(cx);
    self.load_pr(cx);
    if !self.loading {
      self.load(cx);
    }
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
    self.run_changed(
      work,
      |view, value, cx| {
        apply(view, value, cx);
        true
      },
      cx,
    );
  }

  /// [`Self::run`], redrawing only when `apply` says what shows changed.
  fn run_changed<T: Send + 'static>(
    &self,
    work: impl FnOnce(&dyn ChangesHost, &Parent) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) -> bool + 'static,
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
          if view.parent.key() == key && apply(view, value, cx) {
            cx.notify();
          }
        })
        .ignore_closed();
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

  /// Whether the filter leaves `path` out. A file picked for the next
  /// commit stays listed.
  pub(super) fn hides(&self, path: &str) -> bool {
    self.hide_generated && self.is_generated(path) && !self.staging.staged.contains(path)
  }

  /// The working-copy files the list and the diff show.
  pub(super) fn listed(&self) -> Vec<ChangedFile> {
    self
      .files()
      .iter()
      .filter(|f| !self.hides(&f.path))
      .cloned()
      .collect()
  }

  /// The filter button: hiding drops hidden files from the selection.
  fn toggle_hide_generated(&mut self, cx: &mut Context<Self>) {
    self.hide_generated = !self.hide_generated;
    if self.hide_generated {
      let hidden: Vec<String> = self
        .staging
        .selected
        .iter()
        .filter(|p| self.hides(p))
        .cloned()
        .collect();
      for path in hidden {
        self.staging.selected.remove(&path);
      }
    }
    self.staging.anchor = None;
    cx.notify();
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

  /// The committed files shown: none with the Committed filter off, all
  /// with it on, else those also dirty or conflicted.
  pub(super) fn visible_committed(&self) -> Vec<JjFileChange> {
    let Some(snapshot) = &self.snapshot else {
      return Vec::new();
    };
    self
      .visible_committed_at()
      .into_iter()
      .filter_map(|ix| snapshot.committed.get(ix).cloned())
      .collect()
  }

  /// [`Self::visible_committed`], by index into the snapshot's committed
  /// files.
  fn visible_committed_at(&self) -> Vec<usize> {
    let Some(snapshot) = &self.snapshot else {
      return Vec::new();
    };
    if self.options.show_committed == Some(false) {
      return Vec::new();
    }
    let show = self.show_committed();
    // Only a filtered list needs the files that stay in it.
    let always: HashSet<String> = if show {
      HashSet::new()
    } else {
      snapshot
        .files
        .iter()
        .map(|f| f.path.clone())
        .chain(self.conflicted())
        .collect()
    };
    visible_committed_at(&snapshot.committed, show, &always)
      .into_iter()
      .filter(|&ix| !self.hides(&snapshot.committed[ix].path))
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
    let show = self.show_committed();
    // What shows now, so the read compares against it off the main thread.
    let shown = self.snapshot.clone();
    self.run_changed(
      move |host, parent| {
        // The generated-file rules don't depend on the diff: read both at once.
        std::thread::scope(|scope| {
          let rules =
            scope.spawn(|| host.generated_files(&parent.repo_path, parent.workspace_id()));
          let snapshot = match parent.workspace_id() {
            Some(id) => host
              .workspace_diff(&parent.repo_path, id)
              .map(|diff| Snapshot::from_workspace_diff(diff, show)),
            None => host
              .changed_files(&parent.repo_path, None)
              .map(|files| Snapshot::from_home(&files)),
          };
          let rules = super::joined(rules);
          snapshot.map(|s| s.with_generated(&rules).sharing_hunks(shown.as_ref()))
        })
      },
      |view, result, cx| {
        view.loading = false;
        // A refresh that reads back what shows draws no frame.
        let changed = match result {
          Ok(snapshot) if std::mem::take(&mut view.review.apply_next) => {
            view.review.stale.clear();
            view.review.parked = None;
            view.apply(snapshot, cx);
            true
          }
          Ok(snapshot) if view.park(&snapshot) => true,
          // The reviewed hunks were read from this same diff.
          Ok(snapshot)
            if view.in_review()
              && snapshot.token.is_some()
              && snapshot.token == view.hunks_token =>
          {
            false
          }
          Ok(snapshot) if view.in_review() && view.snapshot.is_some() => {
            view.recheck(snapshot, cx);
            false
          }
          Ok(snapshot) => view.apply(snapshot, cx),
          Err(err) => {
            view.toast(ToastTone::Error, "JJ Error", err);
            view.snapshot.get_or_insert_with(Snapshot::default);
            view.layout_gen += 1;
            true
          }
        };
        if std::mem::take(&mut view.reload) {
          view.load(cx);
        }
        changed
      },
      cx,
    );
  }

  /// Shows `snapshot`; `true` when that changed what is drawn. Hunks still
  /// to read redraw as they land.
  fn apply(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) -> bool {
    let marks = |view: &Self| {
      let s = &view.staging;
      let staging = s.staged.len() + s.selected.len() + s.selected_staged.len();
      (view.layout_gen, view.viewed.len(), staging)
    };
    let before = marks(self);
    self.staging.retain(&snapshot.files);
    // Generated files are read only once "Load diff" asks.
    let paths: Vec<String> = snapshot
      .files
      .iter()
      .filter(|f| draws_hunks(&f.path, &f.status))
      .map(|f| f.path.clone())
      .filter(|p| !snapshot.generated.contains(p) || self.loaded_generated.contains(p))
      .collect();
    // A refresh of an unchanged working copy (window focus, a watcher event
    // for an ignored write) has no hunk to read again.
    let current = snapshot.token.is_some()
      && snapshot.token == self.hunks_token
      && paths.len() == self.hunks.len()
      && paths.iter().all(|p| {
        self
          .hunks
          .get(p)
          .is_some_and(|d| !d.loading && d.error.is_none())
      });
    let token = snapshot.token.clone();
    // A refresh that finds the same files keeps the snapshot, and with it
    // the committed hunks the rows were laid out from.
    if self.snapshot.as_ref() != Some(&snapshot) {
      self.snapshot = Some(snapshot);
      self.layout_gen += 1;
    }
    if !current {
      self.load_hunks(paths, token, cx);
    }
    self.unmark_stale(None, cx);
    marks(self) != before
  }

  /// "Load diff" on a generated file: its diff, read now if it is the
  /// working copy's.
  fn load_generated(&mut self, path: &str, cx: &mut Context<Self>) {
    self.loaded_generated.insert(path.to_string());
    let working = self.files().iter().any(|f| f.path == path);
    if working && self.pr.is_none() && !self.hunks.contains_key(path) {
      self.layout_gen += 1;
      self.hunks.insert(
        path.to_string(),
        FileHunks {
          loading: true,
          ..FileHunks::default()
        },
      );
      let token = self.hunks_token.clone();
      self.load_batches(self.generation, vec![vec![path.to_string()]], token, cx);
    }
    cx.notify();
  }

  pub(super) fn is_generated(&self, path: &str) -> bool {
    self
      .snapshot
      .as_ref()
      .is_some_and(|s| s.generated.contains(path))
  }

  /// `loadAllFileHunks`: the first files, then the rest in batches, each
  /// shown as it arrives. What was read before stays until replaced, and
  /// files whose content id held are not read again.
  fn load_hunks(&mut self, paths: Vec<String>, token: Option<String>, cx: &mut Context<Self>) {
    self.generation += 1;
    self.hunks_token = None;
    let listed: HashSet<&String> = paths.iter().collect();
    let before = self.hunks.len();
    self.hunks.retain(|p, _| listed.contains(p));
    let kept = self.hunks.len();
    for path in &paths {
      self.hunks.entry(path.clone()).or_insert_with(|| FileHunks {
        loading: true,
        ..FileHunks::default()
      });
    }
    if self.hunks.len() != before || kept != before {
      self.layout_gen += 1;
    }
    let ids = self.snapshot.as_ref().map(|s| &s.content_ids);
    let paths: Vec<String> = paths
      .into_iter()
      .filter(|path| {
        let shown = self
          .hunks
          .get(path)
          .filter(|d| !d.loading && d.error.is_none());
        let id = ids.and_then(|ids| ids.get(path));
        id.is_none() || shown.and_then(|d| d.content_id.as_ref()) != id
      })
      .collect();
    let mut batches: Vec<Vec<String>> = Vec::new();
    let (first, rest) = paths.split_at(paths.len().min(FIRST_BATCH));
    if !first.is_empty() {
      batches.push(first.to_vec());
    }
    batches.extend(rest.chunks(BATCH).map(<[String]>::to_vec));
    self.load_batches(self.generation, batches, token, cx);
  }

  /// Reads `batches` in turn. `token` is the diff they should all read
  /// from; once every one did, it names the hunks.
  fn load_batches(
    &mut self,
    generation: u64,
    mut batches: Vec<Vec<String>>,
    mut token: Option<String>,
    cx: &mut Context<Self>,
  ) {
    if batches.is_empty() {
      self.hunks_token = token;
      // No file needed reading: what a Reload prunes is already known.
      self.prune_comments(cx);
      return;
    }
    let batch = batches.remove(0);
    // Read from the listed diff's trees, and named by its file ids.
    let (trees, ids) = match &self.snapshot {
      Some(s) => {
        let ids: HashMap<String, String> = batch
          .iter()
          .filter_map(|p| Some((p.clone(), s.content_ids.get(p)?.clone())))
          .collect();
        (s.token.clone(), ids)
      }
      None => (None, HashMap::new()),
    };
    // What the batch's files show now, so the read compares against it off
    // the main thread.
    let shown: HashMap<String, (Arc<Vec<JjDiffHunk>>, Option<String>)> = batch
      .iter()
      .filter_map(|path| {
        let d = self.hunks.get(path).filter(|d| !d.loading)?;
        Some((path.clone(), (d.hunks.clone(), d.error.clone())))
      })
      .collect();
    let read_from = trees.clone();
    self.run_changed(
      move |host, parent| {
        let read = host.file_hunks(
          &parent.repo_path,
          parent.workspace_id(),
          batch,
          read_from.as_deref(),
        )?;
        // A refresh reads every file again; one that reads the same keeps
        // its hunks, so its rows and marks are not redone.
        let files: Vec<_> = read
          .files
          .into_iter()
          .map(|file| {
            let same = shown
              .get(&file.path)
              .filter(|(hunks, error)| *error == file.error && **hunks == file.hunks)
              .map(|(hunks, _)| hunks.clone());
            let data = FileHunks {
              hunks: same.clone().unwrap_or_else(|| file.hunks.into()),
              loading: false,
              error: file.error,
              content_id: None,
            };
            (file.path, same.is_some(), data)
          })
          .collect();
        Ok::<_, String>((read.snapshot_token, files))
      },
      move |view, result, cx| {
        if view.generation != generation {
          return false;
        }
        // A file read again unchanged (a refresh) draws nothing new.
        let mut changed = HashSet::new();
        match result {
          Ok((snapshot_token, files)) => {
            // Read at another diff than listed: the ids don't name them.
            let listed = trees.as_ref() == Some(&snapshot_token);
            if token.as_ref() != Some(&snapshot_token) {
              token = None;
            }
            for (path, same, mut data) in files {
              if listed && data.error.is_none() {
                data.content_id = ids.get(&path).cloned();
              }
              // Still showing the hunks it read the same as.
              if let Some(shown) = view
                .hunks
                .get_mut(&path)
                .filter(|d| same && !d.loading && Arc::ptr_eq(&d.hunks, &data.hunks))
              {
                shown.content_id = data.content_id;
                continue;
              }
              view.layout_gen += 1;
              view.hunks.insert(path.clone(), data);
              changed.insert(path);
            }
          }
          Err(err) => {
            token = None;
            log::warn!("Failed to load file hunks: {err}");
          }
        }
        let marks = (view.viewed.len(), view.review.comments.len());
        view.unmark_stale(Some(&changed), cx);
        if batches.is_empty() {
          view.prune_comments(cx);
        }
        let last = batches.is_empty();
        view.load_batches(generation, batches, token, cx);
        let drawn = !changed.is_empty() || marks != (view.viewed.len(), view.review.comments.len());
        drawn && (last || view.batch_redraw_due(cx))
      },
      cx,
    );
  }

  /// Whether a batch landing now redraws. One landing within
  /// [`BATCH_REDRAW`] of a redraw waits, and a timer draws what landed.
  fn batch_redraw_due(&mut self, cx: &mut Context<Self>) -> bool {
    let now = Instant::now();
    let since = self.batch_drawn.map(|at| now.duration_since(at));
    // Laying out many files takes long: leave the thread time between.
    let every = BATCH_REDRAW.max(self.diff.layout_cost * 4);
    if since.is_none_or(|since| since >= every) {
      self.batch_drawn = Some(now);
      return true;
    }
    if !std::mem::replace(&mut self.batch_flush, true) {
      let wait = every.saturating_sub(since.unwrap_or_default());
      cx.spawn(async move |this, cx| {
        cx.background_executor().timer(wait).await;
        this
          .update(cx, |view, cx| {
            view.batch_flush = false;
            view.batch_drawn = Some(Instant::now());
            cx.notify();
          })
          .ignore_closed();
      })
      .detach();
    }
    false
  }

  fn load_pr(&mut self, cx: &mut Context<Self>) {
    let branch = self
      .parent
      .workspace
      .as_ref()
      .map(|w| w.branch_name.clone());
    self.run_changed(
      move |host, parent| {
        let github = host.github_remote(&parent.repo_path);
        let pr = branch
          .and_then(|b| host.pr_cache(&parent.repo_path, &b).pr.flatten())
          .map(|pr| pr.state);
        (github, pr)
      },
      |view, (github, pr), _| {
        let changed = view.github != github || view.pr_state != pr;
        view.github = github;
        view.pr_state = pr;
        changed
      },
      cx,
    );
  }

  /// The diff a file card shows: the working copy's, else the branch's.
  fn file_hunks(&self, path: &str) -> Option<FileHunks> {
    self.shown_hunks(path).cloned()
  }

  /// Viewed files whose diff changed since are viewed no longer. `read`
  /// limits the check to the files whose hunks were just replaced.
  fn unmark_stale(&mut self, read: Option<&HashSet<String>>, cx: &mut Context<Self>) {
    if self.viewed.is_empty() {
      return;
    }
    // Committed files can be marked viewed too, so they stay listed.
    let listed: HashSet<String> = self
      .snapshot
      .iter()
      .flat_map(|s| {
        let committed = s.committed.iter().map(|f| f.path.clone());
        s.files.iter().map(|f| f.path.clone()).chain(committed)
      })
      .collect();
    self.viewed.retain(|p, _| listed.contains(p));
    let stale: Vec<String> = self
      .viewed
      .iter()
      .filter(|(path, hash)| {
        !hash.is_empty()
          && read.is_none_or(|read| read.contains(*path))
          && self
            .file_hunks(path)
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
    let files = self.listed();
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
    match self.card_row(&path) {
      Some(ix) => {
        self.scroll_to_row(ix);
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
          toast_event(ToastTone::Success, "Changes restored", "")
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
    let (host, parent) = (self.host.clone(), self.parent.clone());
    let key = parent.key();
    // The commit is reported as soon as it lands; the push is a second stage
    // with its own feedback, and runs even if the tab moved on meanwhile.
    cx.spawn(async move |this, cx| {
      let (work_host, work_parent) = (host.clone(), parent.clone());
      let committed = cx
        .background_spawn(async move {
          if split {
            work_host.split(&work_parent.working_directory(), &message, staged)
          } else {
            work_host.commit(&work_parent.repo_path, work_parent.workspace_id(), &message)
          }
        })
        .await;
      let pushing = committed.is_ok() && action == CommitAction::Push;
      this
        .update(cx, |view, cx| {
          if view.parent.key() != key {
            return;
          }
          if !pushing {
            view.pending = None;
          }
          if split {
            view.staging.staged.clear();
          }
          match committed {
            Ok(result) => {
              view.toast(ToastTone::Success, "Commit created", result.trim());
              view.committed_collapsed = true;
              if action == CommitAction::Pr {
                view.send_react(
                  ScreenAction::Press {
                    id: "workspace.createPr".into(),
                  },
                  cx,
                );
              }
            }
            Err(err) => view.toast(ToastTone::Error, "Commit failed", err),
          }
          Self::changed(view.host.as_ref(), &view.parent);
          view.load(cx);
          cx.notify();
        })
        .ignore_closed();
      if !pushing {
        return;
      }
      let pushed = cx
        .background_spawn(async move { host.push_remote(&parent.repo_path, parent.workspace_id()) })
        .await;
      this
        .update(cx, |view, cx| {
          if view.parent.key() != key {
            return;
          }
          view.pending = None;
          match pushed {
            Ok(_) => view.toast(ToastTone::Success, "Pushed to remote", ""),
            Err(err) => view.toast(ToastTone::Error, "Failed to push", err),
          }
          Self::changed(view.host.as_ref(), &view.parent);
          view.load(cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
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
          let was_empty = view.message.trim().is_empty();
          view.message = state.read(cx).value().to_string();
          // The view draws only whether the message is empty, and the
          // error a keystroke clears; the box redraws itself.
          if view.message_error.take().is_some() || was_empty != view.message.trim().is_empty() {
            cx.notify();
          }
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
  /// [`context_ranges`] for hunk `h` of `path`, from the lines read so far.
  fn context_ranges(&self, path: &str, hunks: &[JjDiffHunk], h: usize) -> ContextReads {
    let read = |i: usize, dir: Dir| {
      self
        .context
        .get(&(path.to_string(), i, dir))
        .map_or(0, Vec::len)
    };
    let ended = self.context_ended.contains(&(path.to_string(), h));
    context_ranges(
      hunks,
      h,
      |i| (read(i, Dir::Before), read(i, Dir::After)),
      ended,
    )
  }

  /// [`gap_ranges`] for hunk `h` of `path`, from the lines read so far.
  fn gap_ranges(&self, path: &str, hunks: &[JjDiffHunk], h: usize) -> ContextReads {
    let read = |i: usize, dir: Dir| {
      self
        .context
        .get(&(path.to_string(), i, dir))
        .map_or(0, Vec::len)
    };
    let ended = self.context_ended.contains(&(path.to_string(), h));
    gap_ranges(
      hunks,
      h,
      |i| (read(i, Dir::Before), read(i, Dir::After)),
      ended,
    )
  }

  /// The hidden gap "Expand all" reads above or below hunk `h`, when it
  /// is small enough to offer.
  pub(super) fn expand_all_range(
    &self,
    path: &str,
    hunks: &[JjDiffHunk],
    h: usize,
    dir: Dir,
  ) -> Option<(usize, usize)> {
    let (before, after) = self.gap_ranges(path, hunks, h);
    match dir {
      Dir::Before => before,
      Dir::After => after,
    }
    .filter(|(start, end)| end + 1 - start <= EXPAND_ALL_MAX)
  }

  /// `handleExpandContext`; `all` reads the whole gap ("Expand all").
  fn expand_context(&mut self, path: &str, h: usize, dir: Dir, all: bool, cx: &mut Context<Self>) {
    // The PR's files are not checked out.
    if self.read_only() {
      return;
    }
    // A committed card's lines are read from the working copy, which only
    // matches the branch head while the file has no uncommitted edits.
    if !self.hunks.contains_key(path)
      && self
        .snapshot
        .as_ref()
        .is_some_and(|s| s.files.iter().any(|f| f.path == path))
    {
      return;
    }
    let Some(hunks) = self.shown_hunks(path).map(|d| &d.hunks) else {
      return;
    };
    if h >= hunks.len() {
      return;
    }
    let range = if all {
      self.expand_all_range(path, hunks, h, dir)
    } else {
      let (before, after) = self.context_ranges(path, hunks, h);
      match dir {
        Dir::Before => before,
        Dir::After => after,
      }
    };
    let Some((start, end)) = range else {
      return;
    };
    let key = (path.to_string(), h, dir);
    let file = path.to_string();
    self.run(
      move |host, parent| {
        host.file_lines(&parent.repo_path, parent.workspace_id(), &file, start, end)
      },
      move |view, result, _| {
        let Ok(lines) = result else {
          return;
        };
        view.layout_gen += 1;
        if dir == Dir::After && lines.len() <= end - start {
          view.context_ended.insert((key.0.clone(), h));
        }
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

  fn copy(&self, text: String, title: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    self.toast(ToastTone::Success, &title, "");
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
          view
            .update(cx, |view, _| view.open_in(scheme, &path))
            .ignore_closed();
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
    let copy = |text: String, title: String| {
      let entity = entity.clone();
      move |cx: &mut App| {
        entity
          .update(cx, |view, cx| view.copy(text.clone(), title.clone(), cx))
          .ignore_closed();
      }
    };
    let mut open = vec![{
      let (host, full) = (self.host.clone(), full.clone());
      NativeMenuItem::item(crate::core::context_menu::REVEAL_LABEL, move |_| {
        host.reveal(&full)
      })
      .icon(MenuIcon::FolderOpen)
    }];
    open.extend(self.editor_items(path, cx));
    let mut items = vec![
      NativeMenuItem::item(
        "Copy relative path",
        copy(relative, "Relative path copied".into()),
      )
      .icon(MenuIcon::Copy),
      NativeMenuItem::item("Copy full path", copy(full, "Full path copied".into()))
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
              entity
                .update(cx, |view, cx| view.move_files(paths.clone(), &target, cx))
                .ignore_closed();
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
          entity
            .update(cx, |view, cx| view.discard_files(&path, cx))
            .ignore_closed();
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
        entity
          .update(cx, |view, cx| {
            let message = view.message.trim().to_string();
            if message.is_empty() {
              view.message_error = Some("Enter a commit message.");
              cx.notify();
              return;
            }
            view.message.clear();
            view.message_input = None;
            view.commit(action, message, cx);
          })
          .ignore_closed();
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

  /// Opens the "Copy link" menu for `url` at `at`, for a right-click on
  /// what opens it.
  fn copy_link(&self, url: String, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let host = self.host.clone();
    let menu = popup::copy_link_menu(url, at, self.dark, move |e| tell(host.as_ref(), e));
    self.popup(menu, cx);
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
  p: &Palette,
) -> Stateful<Div> {
  let id: SharedString = id.into();
  let selector = id.to_string();
  div()
    .id(id)
    .debug_selector(move || selector)
    .p_1()
    .rounded(radius::SM)
    .cursor_pointer()
    .text_color(fg)
    .hover(move |s| s.bg(p.hover()))
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
    // The rows first: the threads and the find index follow their revision.
    self.sync_rows(window);
    if !self.read_only() {
      self.sync_side(window);
    }
    self.place_threads();
    self.index_matches();
    self.reveal_match(window, cx);
    // Drawn lines record their layouts afresh each frame; a scrolled-off
    // line must not be found under the pointer.
    self.review.layouts.borrow_mut().clear();
    self.reveal_line(window, cx);
    self.scroll_to_file(window, cx);
    if std::mem::take(&mut self.review.refocus) {
      window.focus(&self.focus, cx);
    }
    let p = theme::palette(self.dark);
    let root = ui::text(div(), TextSize::Sm)
      .debug_selector(|| "changes-diff-viewer".into())
      .size_full()
      .flex()
      .overflow_hidden()
      .bg(p.background)
      .text_color(p.foreground)
      .track_focus(&self.focus)
      .on_key_down(cx.listener(|view, e: &KeyDownEvent, window, cx| {
        let k = &e.keystroke;
        let held = crate::gpui_shell::keys::held(k.modifiers);
        if view.focus.is_focused(window)
          && !(held.platform || held.control || held.alt)
          && view.diff_key(&k.key, held.shift, cx)
        {
          cx.stop_propagation();
        } else if k.key == "c"
          && (held.platform || held.control)
          && !held.shift
          && view.review.text.is_some()
        {
          if let Some(text) = view.selected_code() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
          }
          cx.stop_propagation();
        } else if k.key == "a"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
          && !crate::gpui_shell::keys::held(k.modifiers).shift
          && !view.read_only()
        {
          let files = view.listed();
          view.staging.select_all(&files);
          cx.notify();
        } else if ui::is_escape(k)
          && (view.review.selection.is_some() || view.review.text.is_some())
        {
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
      )
      .when(self.dragging(), |this| {
        // A drag follows the pointer anywhere in the window, off the rows.
        let view = cx.entity().downgrade();
        this.child(
          gpui::canvas(
            |_, _, _| {},
            move |_, _, window, _| {
              window.on_mouse_event(move |e: &gpui::MouseMoveEvent, phase, _, cx| {
                if phase == gpui::DispatchPhase::Bubble {
                  view
                    .update(cx, |view, cx| view.drag_pointer(e.position, cx))
                    .ignore_closed();
                }
              });
            },
          )
          .absolute()
          .size_0(),
        )
      });
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
  /// What the sidebar's rows stand for now, and the data they read.
  fn layout_side(&self) -> (Vec<SideRow>, SideData) {
    let Some(snapshot) = self.snapshot.as_ref() else {
      return Default::default();
    };
    let mut data = SideData::default();
    let mut rows = Vec::new();
    if self.filter_shown() {
      rows.push(SideRow::Filter);
    }
    data.conflicted = self.conflicted();
    if !data.conflicted.is_empty() {
      rows.push(SideRow::ConflictsHeader);
      if !self.collapsed_sections.contains(&Section::Conflicts) {
        rows.extend((0..data.conflicted.len()).map(SideRow::Conflict));
      }
    }
    let mut listed = Vec::new();
    for (ix, file) in snapshot.files.iter().enumerate() {
      if self.hides(&file.path) {
        continue;
      }
      listed.push(ix);
      if self.staging.staged.contains(&file.path) {
        data.staged.push(ix);
      } else {
        data.unstaged.push(ix);
      }
    }
    let committed = match self.options.show_committed {
      Some(false) => 0,
      _ => snapshot.committed.len(),
    };
    if listed.is_empty() && committed == 0 {
      rows.push(SideRow::Empty);
    } else if !listed.is_empty() {
      if !data.staged.is_empty() {
        rows.push(SideRow::Header(List::Selected));
        if !self.collapsed_sections.contains(&Section::Selected) {
          rows.extend((0..data.staged.len()).map(|k| SideRow::File(List::Selected, k)));
        }
      }
      rows.push(SideRow::Header(List::Changes));
      if !self.collapsed_sections.contains(&Section::Changes) {
        rows.extend((0..data.unstaged.len()).map(|k| SideRow::File(List::Changes, k)));
      }
    }
    if committed > 0 {
      rows.push(SideRow::CommittedHeader);
      if !self.committed_collapsed {
        data.committed = self.visible_committed_at();
        rows.extend((0..data.committed.len()).map(SideRow::Committed));
      }
    }
    data.last_staged = self
      .staging
      .staged_anchor
      .and_then(|k| data.staged.get(k))
      .copied();
    data.last_changed = self.staging.anchor.and_then(|k| listed.get(k)).copied();
    (rows, data)
  }

  /// Tells the sidebar's list which rows changed.
  fn sync_side(&mut self, window: &Window) {
    let (rows, data) = self.layout_side();
    self.side_data = data;
    self.side.sync(window, rows, |a, b| a == b);
  }

  fn sidebar(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let scroll = div()
      .id("changes-sidebar-scroll")
      .flex_1()
      .min_h_0()
      .px_4()
      .pb_4();
    let list = if self.snapshot.is_none() {
      scroll.overflow_y_scroll().child(
        div()
          .size_full()
          .flex()
          .items_center()
          .justify_center()
          .child(ui::spinner("changes-sidebar-spin", 24., p.muted_foreground)),
      )
    } else if self.side.get(self.side.len().wrapping_sub(1)) == Some(&SideRow::Empty) {
      // Nothing to scroll: the empty state fills what the rows leave.
      #[expect(
        clippy::needless_collect,
        reason = "the rows borrow self, which side_row needs mutably"
      )]
      let rows: Vec<SideRow> = (0..self.side.len())
        .filter_map(|ix| self.side.get(ix).copied())
        .collect();
      scroll
        .overflow_y_scroll()
        .children(rows.into_iter().map(|row| self.side_row(row, p, cx)))
    } else {
      // Only the rows on screen are built, however many files changed.
      scroll.child(
        gpui::list(
          self.side.list.clone(),
          cx.processor(|this, ix, _, cx| {
            let p = theme::palette(this.dark);
            match this.side.get(ix).copied() {
              Some(row) => this.side_row(row, &p, cx),
              None => div().into_any_element(),
            }
          }),
        )
        .size_full(),
      )
    };
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

  /// One row of the sidebar.
  fn side_row(&mut self, row: SideRow, p: &Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
    match row {
      SideRow::Filter => self
        .filter_row(p, cx)
        .map_or_else(|| div().into_any_element(), IntoElement::into_any_element),
      SideRow::ConflictsHeader => {
        let count = self.side_data.conflicted.len();
        self.conflicts_header(count, p, cx).into_any_element()
      }
      SideRow::Conflict(i) => self.conflict_list_row(i, p, cx).into_any_element(),
      SideRow::Header(list) => {
        let count = match list {
          List::Selected => self.side_data.staged.len(),
          List::Changes => self.side_data.unstaged.len(),
        };
        self.files_header(list, count, p, cx).into_any_element()
      }
      SideRow::File(list, k) => {
        let (ids, selected, last) = match list {
          List::Selected => (
            &self.side_data.staged,
            &self.staging.selected_staged,
            self.side_data.last_staged,
          ),
          List::Changes => (
            &self.side_data.unstaged,
            &self.staging.selected,
            self.side_data.last_changed,
          ),
        };
        let Some(&ix) = ids.get(k) else {
          return div().into_any_element();
        };
        let Some(file) = self.snapshot.as_ref().and_then(|s| s.files.get(ix)) else {
          return div().into_any_element();
        };
        let (selected, last) = (selected.contains(&file.path), last == Some(ix));
        let file = file.clone();
        div()
          .when(k == 0, |this| this.pt_2())
          .child(self.file_row(list, &file, selected, last, p, cx))
          .into_any_element()
      }
      SideRow::CommittedHeader => self.committed_header(p, cx).into_any_element(),
      SideRow::Committed(i) => self.committed_row(i, p, cx).into_any_element(),
      SideRow::Empty => div()
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
        )
        .into_any_element(),
    }
  }

  /// The row above the file lists whose filter menu picks what they show:
  /// generated files and the committed group, when either applies.
  fn filter_shown(&self) -> bool {
    let generated = self
      .snapshot
      .as_ref()
      .is_some_and(|s| !s.generated.is_empty());
    generated || self.options.show_committed.is_some()
  }

  fn filter_row(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    if !self.filter_shown() {
      return None;
    }
    let filtering = self.hide_generated || self.options.show_committed == Some(false);
    let primary = p.primary;
    let filter = header_button(
      "changes-filter",
      theme::ICON_FILTER,
      if filtering {
        primary
      } else {
        p.muted_foreground
      },
      p,
    )
    .relative()
    .when(filtering, |b| b.bg(primary.opacity(0.1)))
    .role(gpui::Role::Button)
    .aria_label("Filter changes")
    .on_click(cx.listener(|view, _, _, cx| view.filter_menu(cx)))
    .test_support()
    .child(self.anchor("changes-filter".into()));
    Some(
      div()
        .pt_3()
        .child(div().flex().justify_end().child(self.tips.wrap(
          "changes-filter-tip",
          Side::Bottom,
          Some(TooltipContent::Text("Filter changes".into())),
          filter,
          cx,
        ))),
    )
  }

  /// The filter button's menu: one checkbox item per kind of file shown.
  fn filter_menu(&self, cx: &mut Context<Self>) {
    let Some(anchor) = self.anchors.borrow().get("changes-filter").copied() else {
      return;
    };
    let mut items = Vec::new();
    if self
      .snapshot
      .as_ref()
      .is_some_and(|s| !s.generated.is_empty())
    {
      let entity = cx.entity().downgrade();
      items.push(
        NativeMenuItem::item("Generated files", move |cx| {
          entity
            .update(cx, |view, cx| view.toggle_hide_generated(cx))
            .ignore_closed();
        })
        .check(!self.hide_generated),
      );
    }
    if let Some(show) = self.options.show_committed {
      let entity = cx.entity().downgrade();
      items.push(
        NativeMenuItem::item("Committed", move |cx| {
          entity
            .update(cx, |view, cx| {
              let show = view.options.show_committed.unwrap_or(show);
              view.field("changes.showCommitted", (!show).into(), cx);
            })
            .ignore_closed();
        })
        .check(show),
      );
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

  fn commit_box(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let files = self.files().len();
    let disabled = files == 0 || self.pending.is_some();
    let empty = self.message.trim().is_empty();
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
      .py_1p5()
      .gap_1p5()
      .rounded_r_none()
      .disabled(disabled || empty)
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
          .py_1p5()
          .px_1p5()
          .rounded_l_none()
          .border_l_1()
          .border_color(border)
          .disabled(disabled || empty)
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
            this
              .rounded(radius::MD)
              .border_1()
              .border_color(p.destructive)
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

  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
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

  fn conflicts_header(&mut self, count: usize, p: &Palette, cx: &mut Context<Self>) -> Div {
    let collapsed = self.collapsed_sections.contains(&Section::Conflicts);
    let header = self.section_header(
      "conflicts-section-toggle",
      "Conflicts",
      collapsed,
      p.muted_foreground,
      p,
      cx,
      |view, cx| view.toggle_section(Section::Conflicts, cx),
    );
    div().pt_4().child(
      div()
        .flex()
        .items_center()
        .justify_between()
        .text_color(p.muted_foreground)
        .child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .child(header)
            .child(ui::Dot::new(Tone::Danger, p)),
        )
        .child(ui::Count::new(count, p)),
    )
  }

  /// The `i`th conflicted file's row.
  fn conflict_list_row(&mut self, i: usize, p: &Palette, cx: &mut Context<Self>) -> Div {
    let Some(path) = self.side_data.conflicted.get(i) else {
      return div();
    };
    let active = self.active.as_deref() == Some(path.as_str());
    let (name, dir) = split_path(path);
    let file = path.clone();
    let row = ui::Row::new(SharedString::from(format!("conflict-row-{path}")), p)
      .label(path.clone())
      .selected(active)
      .gap_1()
      .px_0()
      .rounded_none()
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
              .child(name),
          )
          .when_some(dir, |this, dir| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .truncate()
                .text_color(p.muted_foreground.opacity(0.6))
                .child(dir),
            )
          }),
      )
      .trailing(div().mr_1().child(ui::Dot::new(Tone::Danger, p)))
      .on_click(cx.listener(move |view, _, _, cx| view.show_file(&file, cx)));
    div().when(i == 0, |this| this.pt_2()).child(row)
  }

  fn files_header(
    &mut self,
    list: List,
    count: usize,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    let staged = list == List::Selected;
    let section = if staged {
      Section::Selected
    } else {
      Section::Changes
    };
    let collapsed = self.collapsed_sections.contains(&section);
    let selected_len = if staged {
      self.staging.selected_staged.len()
    } else {
      self.staging.selected.len()
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
    let mut buttons = div().flex().items_center().gap_1();
    if count > 0 {
      // `onStashAll` is left out for a remote repository.
      if !staged && !self.parent.remote {
        let stash = header_button("stash-all-changes", theme::ICON_ARCHIVE, muted, p).on_click(
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
        p,
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
        let label = select_all_label(selected_len, count);
        let select = header_button("select-all-changes", theme::ICON_PLUS, muted, p).on_click(
          cx.listener(|view, _, _, cx| {
            cx.stop_propagation();
            let files = view.listed();
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
      if selected_len > 0 {
        let primary = p.primary;
        let n = selected_len;
        let arrow = div()
          .id(if staged {
            "move-selected-files"
          } else {
            "move-changes-files"
          })
          .p_1()
          .rounded(radius::SM)
          .cursor_pointer()
          .bg(p.selected())
          .hover(move |s| s.bg(p.selected_hover()))
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
        let minus = header_button("unselect-all-changes", theme::ICON_MINUS, muted, p).on_click(
          cx.listener(|view, _, _, cx| {
            cx.stop_propagation();
            view.staging.unstage_all();
            cx.notify();
          }),
        );
        buttons = buttons.child(self.tips.wrap(
          "unselect-all-tip",
          Side::Bottom,
          Some(TooltipContent::Text("Unselect all changes".into())),
          minus,
          cx,
        ));
      }
    }
    buttons = buttons.child(div().ml_1().child(ui::Count::new(count, p)));
    div()
      .id(if staged {
        "selected-section"
      } else {
        "changes-section"
      })
      .pt_4()
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
          // A click on the header clears the selection; it is not a control.
          .cursor_default()
          .child(header)
          .child(buttons)
          .on_click(cx.listener(move |view, _, _, cx| view.deselect(list, cx))),
      )
  }

  fn committed_header(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
    let collapsed = self.committed_collapsed;
    let total = self.snapshot.as_ref().map_or(0, |s| s.committed.len());
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
      .child(div().ml_1().child(ui::Count::new(total, p)))
      .on_click(cx.listener(|view, _, _, cx| {
        view.committed_collapsed = !view.committed_collapsed;
        cx.notify();
      }));
    div().pt_4().child(
      div()
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .text_color(muted)
        .child(header),
    )
  }

  /// The `i`th committed file's row.
  fn committed_row(&mut self, i: usize, p: &Palette, cx: &mut Context<Self>) -> Div {
    let muted = p.muted_foreground;
    let Some(file) = self
      .side_data
      .committed
      .get(i)
      .and_then(|&ix| self.snapshot.as_ref()?.committed.get(ix))
      .cloned()
    else {
      return div();
    };
    let (name, dir) = split_path(&file.path);
    let active = self.active.as_deref() == Some(file.path.as_str());
    let dimmed = self.viewed.contains_key(&file.path);
    let path = file.path.clone();
    let menu = file.path.clone();
    let row = ui::Row::new(
      SharedString::from(format!("committed-row-{}", file.path)),
      p,
    )
    .label(file.path.clone())
    .selected(active)
    .gap_1()
    .px_0()
    .rounded_none()
    .border_l_2()
    .border_color(gpui::transparent_black())
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
            .when(dimmed, |this| this.opacity(VIEWED_OPACITY))
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
    .trailing(div().text_color(muted).child(file.status))
    .on_mouse_down(
      MouseButton::Right,
      cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
        view.file_menu(&menu, false, e.position, cx);
      }),
    )
    .on_click(cx.listener(move |view, _, _, cx| view.show_file(&path, cx)));
    div().when(i == 0, |this| this.pt_2()).child(row)
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
    // Viewed files fade back, like GitHub's file tree.
    let dimmed = self.viewed.contains_key(&file.path) && !selected;
    let (name, dir) = split_path(&file.path);
    let path = file.path.clone();
    let name_color = p.foreground;
    let dir_color = p.muted_foreground.opacity(0.6);
    let small =
      |id: String, icon: &'static str, fg: Hsla, hidden: bool| file_action(id, icon, fg, hidden, p);
    let red = Tone::Danger.text(p);
    let status_color = p.muted_foreground;
    let mut actions = div().flex().items_center().gap_1().flex_none().child(
      div()
        .min_w(zpx(8.))
        .text_color(status_color)
        .child(file.status.clone()),
    );
    if last && selected && !staged {
      let target = path.clone();
      let tip_key = format!("discard-tip-{}", file.path);
      let el = small(
        format!("discard-{}", file.path),
        theme::ICON_UNDO,
        red,
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
        false,
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
        !selected,
      )
      .on_click(cx.listener(move |view, _, _, cx| {
        cx.stop_propagation();
        view.unstage(&target, cx);
      }));
      actions = actions.child(self.tip(tip_key, "Unstage file", el, cx));
    }
    let (click, menu) = (path.clone(), path);
    ui::Row::new(
      SharedString::from(format!(
        "{}-row-{}",
        if staged { "selected" } else { "changes" },
        file.path
      )),
      p,
    )
    .debug_selector({
      let id = format!("file-row-{}", file.path);
      move || id
    })
    .label(file.path.clone())
    .selected(selected || active)
    .group("file-row")
    .relative()
    .min_w_0()
    .gap_1()
    .px_0()
    .rounded_none()
    .border_l_2()
    .border_color(gpui::transparent_black())
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
            .when(dimmed, |this| this.opacity(VIEWED_OPACITY))
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
    .trailing(actions)
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
        .children(self.agent_count(p))
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
    let Some(snapshot) = self.snapshot.as_ref() else {
      let card = |p: &Palette| {
        div()
          .flex()
          .flex_col()
          .gap_2()
          .rounded(radius::MD)
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
    let n = snapshot.files.len();
    if n == 0 && self.visible_committed().is_empty() {
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
    if self.large_changeset_gated() {
      let total = self.changeset_lines();
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
    area.child(div().size_full().px_4().child(self.diff_list(cx)))
  }

  /// Lines of every loaded working-copy diff.
  fn changeset_lines(&self) -> usize {
    self.snapshot.as_ref().map_or(0, |snapshot| {
      total_lines(
        snapshot
          .files
          .iter()
          .filter_map(|f| self.hunks.get(&f.path)),
      )
    })
  }

  /// The diff waits behind "View changes" while it is this large.
  fn large_changeset_gated(&self) -> bool {
    !self.changeset_expanded && self.changeset_lines() > LARGE_CHANGESET_LINES
  }

  /// A card's header and file comments, then the threads and comments with
  /// no line above its hunks, or what it shows in their place.
  fn card_head(&mut self, c: usize, p: &Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
    let card = &self.diff.cards[c];
    let (path, status, old_path) = (
      card.path.clone(),
      card.status.clone(),
      card.old_path.clone(),
    );
    let (committed, shut, kind, conflicted_file) =
      (card.committed, card.shut, card.body, card.conflicted);
    let counts = self.card_counts(c);
    let header = self.file_header(
      &path,
      &status,
      old_path.as_deref(),
      committed,
      shut,
      counts,
      p,
      cx,
    );
    let card = div()
      .child(header)
      .children(self.file_comments(&path, p, cx));
    let muted = p.muted_foreground;
    let placeholder = |el: Div| div().font_family(theme::MONO_FONT).child(el);
    match kind {
      Body::Binary | Body::Deleted => card.child(placeholder(
        div()
          .when(kind == Body::Deleted, |this| {
            this.debug_selector(|| "deleted-file-placeholder".into())
          })
          .flex()
          .items_center()
          .justify_center()
          .py_8()
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
        div().px_3().py_2().text_color(p.destructive).child(
          self
            .shown_hunks(&path)
            .and_then(|d| d.error.clone())
            .unwrap_or_default(),
        ),
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
      Body::Generated => {
        let file = path.clone();
        card.child(
          div()
            .debug_selector(|| "generated-file-placeholder".into())
            .py_6()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(muted)
            .child("Generated file. Its diff is not shown by default.")
            .child(
              Button::new(SharedString::from(format!("load-generated-{path}")), p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .label("Load diff")
                .on_click(cx.listener(move |view, _, _, cx| view.load_generated(&file, cx))),
            ),
        )
      }
      Body::Large(n) => {
        let file = path.clone();
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
      Body::Hunks => card
        .children(self.unplaced_threads(&path, p, cx))
        .children(self.outdated_comments(&path, p, cx)),
      Body::Collapsed => card,
    }
    .into_any_element()
  }

  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
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
    let badge = |text: &'static str, tone: Tone| ui::Badge::new(text, p).tone(tone);
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
          .gap_1p5()
          .child(
            ui::text(div(), TextSize::Code)
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
                  view.copy(copy_path.clone(), "File path copied".into(), cx);
                })),
              cx,
            ),
          )
          .when(committed, |this| {
            this.child(
              div()
                .debug_selector(|| "committed-file-label".into())
                .child(badge("Committed", Tone::Primary)),
            )
          }),
      );
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
            .rounded(radius::SM)
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover()))
            .child(ui::icon(theme::ICON_MESSAGE_PLUS, 14., muted))
            .on_click({
              let path = path.to_string();
              cx.listener(move |view, _, _, cx| view.add_file_comment(&path, cx))
            }),
          cx,
        ),
      );
    }
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
          .py_0p5()
          .rounded(radius::SM)
          .cursor_pointer()
          .text_color(muted)
          .hover(move |s| s.bg(p.hover()))
          .child(ui::Checkbox::new(format!("viewed-box-{path}"), p).checked(viewed))
          .child("Viewed")
          .on_click(cx.listener(move |view, _, _, cx| {
            cx.stop_propagation();
            view.toggle_viewed(&file, cx);
          })),
        cx,
      ),
    );
    if renamed {
      right = right.child(badge("Renamed", Tone::Primary));
    }
    let deleted = status == "D";
    if deleted {
      right = right.child(
        div()
          .debug_selector(|| "deleted-file-badge".into())
          .child(badge("Deleted", Tone::Danger)),
      );
    }
    if super::changes_data::is_binary(path) {
      right = right.child(badge("Binary", Tone::Neutral));
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
              .text_color(Tone::GitOpen.color(p))
              .child(format!("+{additions}")),
          )
          .child(
            div()
              .text_color(Tone::GitClosed.color(p))
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
          .rounded(radius::SM)
          .cursor_pointer()
          .hover(move |s| s.bg(p.hover()))
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
  /// `InlineConflictCard`: the region's lines by side.
  fn conflict_card(
    &self,
    conflict: &Conflict,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let region = &conflict.region;
    let (left_deleted, right_deleted) = deleted_sides(region);
    let mut pre = div()
      .font_family(theme::MONO_FONT)
      .bg(p.muted.opacity(0.3))
      .p_3()
      .rounded(radius::SM);
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
        ConflictLineRole::Left => Some(Tone::Danger.tint(p)),
        ConflictLineRole::Right => Some(Tone::Success.tint(p)),
        ConflictLineRole::Base => Some(Tone::Warning.tint(p)),
        _ => None,
      };
      let label = (kind == ConflictLineKind::Marker)
        .then(|| section_label(region, role))
        .flatten();
      let label_el = label.map(|label| {
        let tone = match label {
          "Side #1" => Tone::Danger,
          "Base" => Tone::Warning,
          _ => Tone::Success,
        };
        ui::text(div(), TextSize::Xs)
          .debug_selector(move || format!("conflict-label-{label}"))
          .mr_2()
          .px_1p5()
          .py_0p5()
          .rounded(radius::SM)
          .font_family(theme::SANS_FONT)
          .font_weight(FontWeight::MEDIUM)
          .bg(tone.tint(p))
          .text_color(tone.text(p))
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
            &[],
            None,
          ))),
      );
      let deleted_notice = |side: &'static str, tone: Tone| {
        div()
          .debug_selector(|| "conflict-deleted-side".into())
          .my_1()
          .rounded(radius::MD)
          .border_1()
          .border_color(tone.border(p))
          .bg(tone.tint(p))
          .px_3()
          .py_2()
          .font_family(theme::SANS_FONT)
          .text_color(tone.text(p))
          .child(format!("{side} deleted this file"))
      };
      if kind == ConflictLineKind::Marker && role == ConflictLineRole::Start && left_deleted {
        pre = pre.child(deleted_notice("Side #1", Tone::Danger));
      }
      if kind == ConflictLineKind::Marker && role == ConflictLineRole::Separator && right_deleted {
        pre = pre.child(deleted_notice("Side #2", Tone::Success));
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
      self.discard_focus = None;
      return None;
    }
    let focus = self
      .discard_focus
      .get_or_insert_with(|| {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        focus
      })
      .clone();
    let viewport = window.viewport_size();
    let n = self.staging.unstaged(self.files()).len();
    let panel = div()
      .id("discard-all-dialog")
      .debug_selector(|| "discard-all-dialog".into())
      .track_focus(&focus)
      .w_full()
      .max_w(zpx(512.))
      .p_6()
      .rounded(radius::LG)
      .border_1()
      .border_color(p.border.opacity(0.5))
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_2xl()
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        if ui::is_escape(&e.keystroke) {
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
          .gap_1p5()
          .child(
            ui::text(div(), TextSize::Lg)
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
            .bg(p.scrim())
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
  div().rounded(radius::MD).bg(p.primary.opacity(0.1))
}

/// A file row's small stage, unstage or discard button, shown on the row's
/// hover when `hidden`.
fn file_action(
  id: String,
  icon: &'static str,
  fg: Hsla,
  hidden: bool,
  p: &Palette,
) -> Stateful<Div> {
  let selector = id.clone();
  div()
    .id(SharedString::from(id))
    .debug_selector(move || selector)
    .p_0p5()
    .rounded(radius::SM)
    .cursor_pointer()
    .when(hidden, |this| {
      this.opacity(0.).group_hover("file-row", |s| s.opacity(1.))
    })
    .hover(move |s| s.bg(p.hover()))
    .child(ui::icon(icon, 12., fg))
}

/// `HunkContextLineRow`: a line read by "Show more lines".
fn context_row(
  selector: String,
  (old, new): (i64, i64),
  line: &str,
  language: Option<&'static str>,
  p: &Palette,
) -> impl IntoElement {
  let number = |n: i64| if n > 0 { n.to_string() } else { String::new() };
  let border = p.border.opacity(0.4);
  div()
    .flex()
    // Darker than the hunk's own lines: these were read in by an expander.
    .bg(p.inset)
    .child(
      div()
        .w_24()
        .flex_none()
        .flex()
        .items_center()
        // As the hunk lines' gutter spaces its numbers.
        .gap_1()
        .border_r_1()
        .border_color(border)
        .text_color(p.muted_foreground)
        .child(div().w_10().mr_1().flex().justify_end().child(number(old)))
        .child(
          div()
            .debug_selector(move || selector)
            .w_10()
            .flex()
            .justify_end()
            .child(number(new)),
        ),
    )
    .child(div().w_6().flex_none())
    .child(div().w_5().flex_none())
    .child(
      div()
        .flex_1()
        .min_w_0()
        .px_2()
        .py_0p5()
        .text_color(p.muted_foreground)
        .child(highlight::styled(
          if line.is_empty() { " " } else { line },
          language,
          p.dark,
        )),
    )
}
