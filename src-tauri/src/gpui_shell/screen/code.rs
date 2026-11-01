//! The workspace Code tab (`ShowWorkspace`'s Overview): the workspace's
//! title, its action row (Agent, alerts, Go to file), description and stack, its files and README, and its commit
//! history in a column on the right.
//!
//! Drawn from the backend directly ([`code_data`]). What stays React's goes
//! back to it as screen actions: opening a file or folder in the file
//! browser, the Commits and Changes tabs, Cmd+P's picker, moving to another
//! workspace, and the agent terminal a new task starts in.
//!
//! [`code_data`]: super::code_data

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::{zpx, DismissExt as _};
use crate::result_ext::ResultExt;
use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  ops::Range,
  rc::Rc,
  sync::{Arc, LazyLock},
  time::Duration,
};

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
  input::{Enter, Escape, InputEvent, Textarea, TextareaState},
  text::{TextView, TextViewStyle},
};
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, prelude::*, px, relative, rems, Animation, AnimationExt as _,
  AnyElement, App, Context, Div, Entity, FontWeight, Hsla, Pixels, Role, ScrollHandle,
  SharedString, Stateful, Subscription, Window,
};

use super::{
  code_data::{
    self, description, diff_stats, display_title, group_by_day, headline, history_commits,
    looks_like_markdown, task_settings, workspace_stack, Agent, AlertState, CodeAlert, CodeHost,
    Overview, TaskSettings, REMOTE_FILE_TREE_REASON,
  },
  merge::full_path,
  prefetch, ActionSink, ScreenAction,
};
use crate::{
  core::{
    pr_sync::{self, SyncState},
    rebase_guard::RebaseHold,
    running_checks::{CheckRun, ChecksTarget},
    WorkspaceEntry,
  },
  gpui_shell::{
    checks_run,
    native::{NativeEvent, ToastTone},
    popup::{PopupSink, Side, TooltipContent},
    theme::{self, Palette},
    time,
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::{JjLogCommit, JjLogResult},
  local_db::{PrSyncRecord, Workspace},
};

/// `LinearCommitHistory`'s page size.
const HISTORY_PAGE: usize = 14;
const STACK_DOCS_PATH: &str = "/docs/concepts/workspaces#stacks-and-rebasing";
/// A root of at most this many entries draws every row.
const ENTRIES_ALL: usize = 200;
/// How often a pending or running setup script's status is read again.
const SETUP_POLL: Duration = Duration::from_millis(1500);
/// Rows drawn past each edge of the viewport.
const ENTRIES_OVERSCAN: usize = 20;

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
    self.repo_path == other.repo_path
      && self.remote == other.remote
      && self.conflicted_files == other.conflicted_files
      && self.default_branch == other.default_branch
      && self.revision == other.revision
      && self.workspace == other.workspace
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
#[derive(Debug, Clone, PartialEq)]
pub struct StackRow {
  pub workspace: Workspace,
  pub current: bool,
  pub stats: (u32, u32),
}

/// What [`CodeView::load`] reads, all at once.
struct Read {
  overview: Overview,
  setup: Option<crate::core::SetupScriptStatus>,
  /// The Logs preview is on, so the setup alert can open its logs.
  logs: bool,
  history: Result<JjLogResult, String>,
  stack: Option<Vec<StackRow>>,
  settings: TaskSettings,
  pr_sync: PrSyncRead,
  rebase_hold: Option<RebaseHold>,
  branch_deleted: bool,
}

/// The workspace's PR sync record, and when its read started: reads land out
/// of order, and only one that started later may replace what shows.
type PrSyncRead = (u64, Option<PrSyncRecord>);

fn read_pr_sync(host: &dyn CodeHost, parent: &Parent) -> PrSyncRead {
  static STARTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
  let started = STARTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  let record = parent
    .workspace_id()
    .and_then(|id| host.pr_sync(&parent.repo_path, id));
  (started, record)
}

impl Read {
  /// Reads everything the tab shows for `parent`, `limit` commits of the
  /// home repository's history. Blocks.
  fn of(host: &dyn CodeHost, parent: &Parent, limit: usize) -> Self {
    // Independent reads: run them side by side.
    std::thread::scope(|scope| {
      let overview = scope.spawn(|| host.overview(&parent.repo_path, parent.workspace_id()));
      let history = scope.spawn(|| read_history(host, parent, limit));
      let stack = scope.spawn(|| read_stack(host, parent));
      let setup = scope.spawn(|| {
        parent
          .workspace_id()
          .and_then(|id| host.setup(&parent.repo_path, id))
      });
      let settings = task_settings(host, &parent.repo_path);
      let pr_sync = read_pr_sync(host, parent);
      let rebase_hold = parent
        .workspace_id()
        .and_then(|id| host.rebase_hold(&parent.repo_path, id));
      let branch_deleted = parent
        .workspace_id()
        .is_some_and(|id| host.branch_deleted(&parent.repo_path, id));
      let logs = host.logs_enabled();
      Read {
        overview: Overview::or_unavailable(super::joined(overview)),
        logs,
        setup: super::joined(setup),
        history: super::joined(history),
        stack: super::joined(stack),
        settings,
        pr_sync,
        rebase_hold,
        branch_deleted,
      }
    })
  }
}

/// The commit history column: `limit` commits of the home repository, a
/// page of a workspace's.
fn read_history(host: &dyn CodeHost, parent: &Parent, limit: usize) -> Result<JjLogResult, String> {
  let home = parent.workspace.is_none();
  host.commits(
    &parent.repo_path,
    parent.workspace_id(),
    true,
    Some(if home { limit } else { HISTORY_PAGE }),
  )
}

/// The workspace's stack with each row's diff stats; `None` at home.
fn read_stack(host: &dyn CodeHost, parent: &Parent) -> Option<Vec<StackRow>> {
  let current = parent.workspace.as_ref()?;
  let workspaces = host.workspaces(&parent.repo_path).ok()?;
  let stack = workspace_stack(&workspaces, current.id, &parent.default_branch)?;
  // Each row's commits are an independent read: read them all at once.
  Some(std::thread::scope(|scope| {
    #[expect(
      clippy::needless_collect,
      reason = "every read starts before the first is joined"
    )]
    let reads: Vec<_> = stack
      .into_iter()
      .map(|(workspace, current)| {
        let id = workspace.id;
        let stats = scope.spawn(move || {
          host
            .commits(&parent.repo_path, Some(id), false, None)
            .map(|log| diff_stats(&log.commits))
            .unwrap_or_default()
        });
        (workspace, current, stats)
      })
      .collect();
    reads
      .into_iter()
      .map(|(workspace, current, stats)| StackRow {
        workspace,
        current,
        stats: super::joined(stats),
      })
      .collect()
  }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
  /// The title editor's Save options.
  SaveTitle,
  /// The description editor's Save options.
  SaveDescription,
}

/// Which side of a local draft the title and description card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AboutTab {
  #[default]
  Draft,
  GitHub,
}

pub struct CodeView {
  host: Arc<dyn CodeHost>,
  react: ActionSink,
  pub(super) parent: Parent,
  /// The workspace the tab moves to once [`CodeView::load`] reads it.
  next: Option<Parent>,
  /// Other workspaces read ahead of a switch to them (`prefetch`).
  prefetched: prefetch::Lru<(String, Option<i64>), (Parent, Read)>,
  dark: bool,
  tips: Tooltips,
  pub(super) overview: Option<Overview>,
  /// Shared, so a frame takes it without copying every commit.
  pub(super) history: Option<Rc<JjLogResult>>,
  pub(super) history_limit: usize,
  history_loading: bool,
  /// Why the last commit read failed, while there is no history to show.
  history_error: Option<String>,
  /// The failed read's message, read-only so it can be copied into a report.
  history_error_text: Option<Entity<TextareaState>>,
  /// Whether "Show error" has the error's popover open.
  history_error_open: bool,
  /// Whether the alerts button has its popover open.
  status_open: bool,
  pub(super) stack: Option<Vec<StackRow>>,
  pub(super) settings: Option<TaskSettings>,
  pub(super) menu: Option<Menu>,
  /// The title being edited, and its editor.
  pub(super) title_draft: Option<Entity<TextareaState>>,
  saving_title: bool,
  /// The description being edited, and its editor.
  pub(super) description_draft: Option<Entity<TextareaState>>,
  saving_description: bool,
  /// The PR text last copied into the workspace, as the latest-started
  /// read found it; [`Self::pr_sync`] checks the shown workspace against it.
  pr_sync: PrSyncRead,
  /// Why the workspace's last rebase was held back (`core::rebase_guard`).
  rebase_hold: Option<RebaseHold>,
  /// The workspace's branch is gone, as after its PR merged.
  branch_deleted: bool,
  resyncing: bool,
  /// The card's tab while a local draft differs from the pull request, and
  /// the PR's title and body as the GitHub tab last read them.
  about_tab: AboutTab,
  pr_text: Option<(String, String)>,
  syncing_submodule: bool,
  /// The workspace setup script's status, and whether the Logs tab is
  /// there to show its output.
  setup: Option<crate::core::SetupScriptStatus>,
  logs_enabled: bool,
  /// The file list shows its untouched entries.
  show_untouched: bool,
  /// The tab's scroll, and where the root's entry rows sit in it.
  scroll: ScrollHandle,
  entry_rows: Rc<EntryRows>,
  _subscriptions: Vec<Subscription>,
}

/// Where the root's entry rows sit, measured as they draw, so a root of
/// thousands of entries draws only the rows near the viewport: drawing all
/// of 5,000 took most of a second a frame (debug).
#[derive(Default)]
struct EntryRows {
  /// A row's height with its top border (every row but the first).
  pitch: Cell<Option<Pixels>>,
  /// The first row's top in the scrolled content.
  top: Cell<Option<Pixels>>,
}

impl EntryRows {
  /// The rows of `count` to draw while `scroll` shows what it does.
  fn shown(&self, count: usize, scroll: &ScrollHandle) -> Range<usize> {
    if count <= ENTRIES_ALL {
      return 0..count;
    }
    let (Some(pitch), Some(top)) = (self.pitch.get(), self.top.get()) else {
      return 0..ENTRIES_ALL;
    };
    let pitch = f32::from(pitch).max(1.);
    // A scroll past the end draws where layout clamps it, not a frame later.
    let offset = (-scroll.offset().y).min(scroll.max_offset().y);
    let from = f32::from(offset - top);
    let to = from + f32::from(scroll.bounds().size.height);
    let first = ((from / pitch).floor().max(0.) as usize).saturating_sub(ENTRIES_OVERSCAN);
    let last = ((to / pitch).ceil().max(0.) as usize + ENTRIES_OVERSCAN).min(count);
    first.min(last)..last
  }
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
      next: None,
      prefetched: prefetch::Lru::new(2),
      dark,
      tips: Tooltips::new(popups, dark),
      overview: None,
      history: None,
      history_limit: HISTORY_PAGE,
      history_loading: false,
      history_error: None,
      history_error_text: None,
      history_error_open: false,
      status_open: false,
      stack: None,
      settings: None,
      menu: None,
      title_draft: None,
      saving_title: false,
      description_draft: None,
      saving_description: false,
      pr_sync: (0, None),
      rebase_hold: None,
      branch_deleted: false,
      resyncing: false,
      about_tab: AboutTab::Draft,
      pr_text: None,
      syncing_submodule: false,
      setup: None,
      logs_enabled: false,
      show_untouched: false,
      scroll: ScrollHandle::new(),
      entry_rows: Rc::default(),
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
    let current = self.next.as_ref().unwrap_or(&self.parent);
    if current == &parent {
      return;
    }
    // The conflicted files or the workspace's record changing alone (the
    // header's read landing after the window's listing) don't read again.
    let reread = current.key() != parent.key()
      || current.revision != parent.revision
      || current.default_branch != parent.default_branch;
    // A new title or description (an edit, or the PR's text synced in)
    // can start or stop following the pull request.
    let text = |p: &Parent| {
      p.workspace
        .as_ref()
        .map(|w| (w.title.clone(), w.description.clone()))
    };
    let resync = !reread && text(current) != text(&parent);
    if parent.key() != self.parent.key() {
      // Another workspace: keep drawing this one until the new one is read,
      // then swap them in one frame (`apply`), so the tab never draws empty.
      // Read ahead, it swaps now, and reads again behind.
      let now = cx.background_executor().now();
      let ahead = self
        .prefetched
        .take_fresh(&parent.key(), now)
        .filter(|(read, _)| {
          read.revision == parent.revision && read.default_branch == parent.default_branch
        });
      self.next = Some(parent);
      if let Some((read_for, read)) = ahead {
        self.apply(read_for, read, cx);
      }
    } else {
      self.next = None;
      self.parent = parent;
      cx.notify();
    }
    if reread {
      self.load(cx);
    } else if resync {
      self.read(read_pr_sync, |view, read, _| view.take_pr_sync(read), cx);
    }
  }

  /// Reads everything the tab shows off the main thread and applies it in
  /// one go, so the tab never draws half of a read.
  fn load(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    let parent = self.next.clone().unwrap_or_else(|| self.parent.clone());
    let limit = if self.next.is_some() {
      HISTORY_PAGE
    } else {
      self.history_limit
    };
    self.history_loading = true;
    cx.spawn(async move |this, cx| {
      let read = cx
        .background_spawn(async move {
          let read = Read::of(host.as_ref(), &parent, limit);
          (parent, read)
        })
        .await;
      this
        .update(cx, |view, cx| view.apply(read.0, read.1, cx))
        .ignore_closed();
    })
    .detach();
  }

  /// Reads workspace `parent` ahead of a switch to it, which then draws at
  /// once (`set_parent`). Nothing on screen changes.
  pub(super) fn prefetch(&mut self, parent: Parent, cx: &mut Context<Self>) {
    let now = cx.background_executor().now();
    let current = self.next.as_ref().unwrap_or(&self.parent);
    if parent.key() == current.key() || self.prefetched.has_fresh(&parent.key(), now) {
      return;
    }
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let read = cx
        .background_spawn(async move {
          let read = Read::of(host.as_ref(), &parent, HISTORY_PAGE);
          (parent, read)
        })
        .await;
      this
        .update(cx, |view, cx| {
          let now = cx.background_executor().now();
          view.prefetched.insert(read.0.key(), now, read);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// It shows its workspace's read, with no other workspace's on the way.
  pub(super) fn ready(&self) -> bool {
    self.next.is_none() && self.overview.is_some()
  }

  /// Reads the tab again, drawing what it has meanwhile.
  pub(super) fn revalidate(&mut self, cx: &mut Context<Self>) {
    self.load(cx);
  }

  /// Shows what [`Self::load`] read for `parent`, unless the tab moved on to
  /// another workspace meanwhile.
  fn apply(&mut self, parent: Parent, read: Read, cx: &mut Context<Self>) {
    let moved = match &self.next {
      Some(next) if next.key() == parent.key() => {
        self.parent = self.next.take().unwrap_or(parent);
        self.history_limit = HISTORY_PAGE;
        self.title_draft = None;
        self.description_draft = None;
        self.about_tab = AboutTab::Draft;
        self.pr_text = None;
        self.menu = None;
        self.history = None;
        self.history_error = None;
        self.history_error_open = false;
        self.show_untouched = false;
        true
      }
      None if self.parent.key() == parent.key() => false,
      _ => return,
    };
    // A read again that found what shows already (a revalidation) draws
    // nothing new.
    let unchanged = !moved
      && !(self.history_loading && self.history_limit > HISTORY_PAGE)
      && self.overview.as_ref() == Some(&read.overview)
      && self.history.as_deref() == read.history.as_ref().ok()
      && self.history_error.is_none()
      && self.stack == read.stack
      && self.settings.as_ref() == Some(&read.settings)
      && self.pr_sync.1 == read.pr_sync.1
      && self.rebase_hold == read.rebase_hold
      && self.branch_deleted == read.branch_deleted
      && self.setup == read.setup
      && self.logs_enabled == read.logs;
    self.overview = Some(read.overview);
    self.logs_enabled = read.logs;
    self.set_setup(read.setup, cx);
    self.set_history(read.history);
    self.stack = read.stack;
    self.settings = Some(read.settings);
    self.take_pr_sync(read.pr_sync);
    self.rebase_hold = read.rebase_hold;
    self.branch_deleted = read.branch_deleted;
    if !unchanged {
      cx.notify();
    }
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

  /// Shows the setup script's status; reads it again while it has not
  /// finished.
  fn set_setup(&mut self, setup: Option<crate::core::SetupScriptStatus>, cx: &mut Context<Self>) {
    let unfinished = setup
      .as_ref()
      .is_some_and(|s| s.configured && matches!(s.status.as_str(), "pending" | "running"));
    self.setup = setup;
    if unfinished {
      let key = self.parent.key();
      cx.spawn(async move |this, cx| {
        cx.background_executor().timer(SETUP_POLL).await;
        this
          .update(cx, |view, cx| {
            if view.parent.key() == key {
              view.load_setup(cx);
            }
          })
          .ignore_closed();
      })
      .detach();
    }
  }

  fn load_setup(&mut self, cx: &mut Context<Self>) {
    self.read(
      |host, parent| {
        parent
          .workspace_id()
          .and_then(|id| host.setup(&parent.repo_path, id))
      },
      |view, setup, cx| view.set_setup(setup, cx),
      cx,
    );
  }

  /// Runs the setup script again, then reads its status.
  fn rerun_setup(&mut self, cx: &mut Context<Self>) {
    let Some(workspace) = self.parent.workspace.clone() else {
      return;
    };
    let target = ChecksTarget {
      repo_path: self.parent.repo_path.clone(),
      workspace_id: workspace.id,
      workspace_path: full_path(&workspace),
    };
    let (host, view) = (self.host.clone(), cx.weak_entity());
    let repo_path = target.repo_path.clone();
    checks_run::start(
      cx,
      target,
      CheckRun::Setup,
      move |target, _| host.run_setup(target),
      move |_, cx| {
        view
          .update(cx, |view, cx| {
            tell(
              view.host.as_ref(),
              NativeEvent::WorkspaceChanged { repo_path },
            );
            view.load_setup(cx);
          })
          .ignore_closed();
      },
    );
    cx.notify();
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
      move |host, parent| read_history(host, parent, limit),
      |view, history, _| view.set_history(history),
      cx,
    );
  }

  /// Shows a commit read: the history, or why it failed while there is none.
  fn set_history(&mut self, history: Result<JjLogResult, String>) {
    self.history_loading = false;
    match history {
      Ok(history) => {
        self.history = Some(Rc::new(history));
        self.history_error = None;
        self.history_error_open = false;
      }
      Err(err) => {
        log::warn!("Failed to load commits: {err}");
        // Keep what was read before; only an empty view says it failed.
        if self.history.is_none() {
          self.history_error = Some(err);
        }
      }
    }
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

  fn edit_title(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
    let draft = cx.new(|cx| {
      TextareaState::new(window, cx)
        .auto_grow(1, 5)
        .placeholder("Workspace title")
        .default_value(text)
    });
    draft.update(cx, |d, cx| d.focus(window, cx));
    // Save is disabled while the title is blank.
    self
      ._subscriptions
      .push(cx.subscribe(&draft, |_, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
          cx.notify();
        }
      }));
    self.title_draft = Some(draft);
    cx.notify();
  }

  /// Saves the title being edited; on a workspace that follows its pull
  /// request, to the PR too unless `draft` keeps it local until a push.
  fn save_title(&mut self, draft: bool, cx: &mut Context<Self>) {
    let (Some(editor), Some(ws)) = (&self.title_draft, &self.parent.workspace) else {
      return;
    };
    let text = editor.read(cx).value().trim().to_string();
    if text.is_empty() || self.saving_title {
      return;
    }
    let (id, to_pr) = (ws.id, !draft && self.pr_sync.1.is_some());
    self.saving_title = true;
    self.menu = None;
    self.read(
      move |host, parent| {
        host.set_title(&parent.repo_path, id, &text)?;
        Ok(
          to_pr
            .then(|| host.push_to_pr(&parent.repo_path, id).err())
            .flatten(),
        )
      },
      |view, result, _| {
        view.saving_title = false;
        if view.saved("title", result) {
          view.title_draft = None;
        }
      },
      cx,
    );
  }

  /// Reports a save: `Ok(Some(err))` saved it here but not to the PR, so it
  /// stays a local draft. Returns whether the edit landed.
  fn saved(&self, what: &str, result: Result<Option<String>, String>) -> bool {
    match result {
      Ok(pr_error) => {
        tell(
          self.host.as_ref(),
          NativeEvent::WorkspaceChanged {
            repo_path: self.parent.repo_path.clone(),
          },
        );
        if let Some(err) = pr_error {
          self.toast(
            ToastTone::Error,
            "Saved as a local draft",
            format!("The pull request wasn't updated: {err}"),
          );
        }
        true
      }
      Err(err) => {
        self.toast(ToastTone::Error, &format!("Failed to save {what}"), err);
        false
      }
    }
  }

  fn resync_from_pr(&mut self, cx: &mut Context<Self>) {
    let Some(ws) = &self.parent.workspace else {
      return;
    };
    if self.resyncing {
      return;
    }
    let id = ws.id;
    self.resyncing = true;
    self.read(
      move |host, parent| host.resync_from_pr(&parent.repo_path, id),
      |view, result, _| {
        view.resyncing = false;
        match result {
          Ok(()) => tell(
            view.host.as_ref(),
            NativeEvent::WorkspaceChanged {
              repo_path: view.parent.repo_path.clone(),
            },
          ),
          Err(err) => view.toast(
            ToastTone::Error,
            "Failed to resync from the pull request",
            err,
          ),
        }
      },
      cx,
    );
  }

  /// Keeps `read` unless a read that started later already landed.
  fn take_pr_sync(&mut self, read: PrSyncRead) {
    if read.0 >= self.pr_sync.0 {
      self.pr_sync = read;
    }
  }

  /// Which of the shown workspace's fields still follow its pull request.
  fn pr_sync(&self) -> Option<SyncState> {
    let workspace = self.parent.workspace.as_ref()?;
    Some(pr_sync::state(workspace, self.pr_sync.1.as_ref()?))
  }

  /// The card's footer on a workspace opened from a pull request: a check
  /// while title and description match the PR, else that a local draft
  /// waits for the next push, with Sync to take the PR's text back.
  fn pr_footer(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
    let sync = self.pr_sync()?;
    let note = |id: &'static str, text: String| {
      ui::text(div(), TextSize::Xs)
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Label)
        .aria_label(SharedString::from(text.clone()))
        .test_support()
        .text_color(p.muted_foreground)
        .child(text)
    };
    let footer = div()
      .flex()
      .flex_wrap()
      .items_center()
      .justify_between()
      .gap_2()
      .pt_3()
      .border_t_1()
      .border_color(p.border);
    if sync.title && sync.description {
      return Some(
        footer
          .child(
            div()
              .flex()
              .items_center()
              .gap_1p5()
              .child(ui::icon(theme::ICON_CHECK, 14., p.success))
              .child(note(
                "workspace-description-synced",
                format!("Synced with PR #{}", sync.pr_number),
              )),
          )
          .into_any_element(),
      );
    }
    Some(
      footer
        .child(note(
          "workspace-pr-draft",
          format!(
            "Local draft, sent to PR #{} on your next push",
            sync.pr_number
          ),
        ))
        .child(
          Button::new("workspace-description-resync", p)
            .variant(Variant::Warning)
            .size(Size::Sm)
            .disabled(self.resyncing)
            .label("Sync")
            .on_click(cx.listener(|view, _, _, cx| view.resync_from_pr(cx))),
        )
        .into_any_element(),
    )
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

  /// Saves the description being edited, as [`Self::save_title`] does.
  fn save_description(&mut self, draft: bool, cx: &mut Context<Self>) {
    let (Some(editor), Some(ws)) = (&self.description_draft, &self.parent.workspace) else {
      return;
    };
    let text = editor.read(cx).value().to_string();
    let (id, to_pr) = (ws.id, !draft && self.pr_sync.1.is_some());
    self.saving_description = true;
    self.menu = None;
    self.read(
      move |host, parent| {
        host.set_description(&parent.repo_path, id, &text)?;
        Ok(
          to_pr
            .then(|| host.push_to_pr(&parent.repo_path, id).err())
            .flatten(),
        )
      },
      |view, result, _| {
        view.saving_description = false;
        if view.saved("description", result) {
          view.description_draft = None;
        }
      },
      cx,
    );
  }

  /// An editor's Save: on a workspace that follows its pull request, a split
  /// button whose menu saves a local draft instead.
  fn save_button(
    &self,
    id: &'static str,
    menu: Menu,
    disabled: bool,
    p: &Palette,
    save: fn(&mut Self, bool, &mut Context<Self>),
    cx: &mut Context<Self>,
  ) -> AnyElement {
    if self.pr_sync.1.is_none() {
      return Button::new(id, p)
        .size(Size::Sm)
        .disabled(disabled)
        .label("Save")
        .on_click(cx.listener(move |view, _, _, cx| save(view, false, cx)))
        .into_any_element();
    }
    let open = self.menu == Some(menu) && !disabled;
    let draft_id = SharedString::from(format!("{id}-draft"));
    let panel = menu_panel("workspace-save-menu", p)
      .top(zpx(32.))
      .right_0()
      .w(zpx(240.))
      .child(
        ui::MenuItem::new(draft_id, "Save as local draft", p)
          .description("Keep it here; it goes to the pull request on your next push")
          .on_click(cx.listener(move |view, _, _, cx| save(view, true, cx))),
      );
    div()
      .relative()
      .child(
        ui::SplitButton::new(id, "Save", p)
          .menu_label("More save options")
          .disabled(disabled)
          .open(open)
          .on_click(cx.listener(move |view, _, _, cx| save(view, false, cx)))
          .on_menu(cx.listener(move |view, _, _, cx| view.toggle_menu(menu, cx))),
      )
      .when(open, |this| {
        this.child(deferred(self.close_menu_on_outside(panel, cx)).with_priority(1))
      })
      .into_any_element()
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

  fn close_menu_on_outside(&self, panel: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
    panel.dismiss_with("code-menu", cx, |view, cx| {
      let open = view.menu.take().is_some();
      if open {
        cx.notify();
      }
      open
    })
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
    Animation::new(Duration::from_secs(2))
      .repeat()
      .with_max_fps(ui::PULSE_FPS),
    |el, delta| el.opacity(1. - 0.5 * (1. - (2. * delta - 1.).abs())),
  )
}

/// `rounded-lg border` around a section.
fn section(p: &Palette) -> Div {
  div().rounded(radius::LG).border_1().border_color(p.border)
}

/// A dropdown's panel (`DropdownMenuContent`), drawn over the tab.
pub(super) fn menu_panel(id: &'static str, p: &Palette) -> Stateful<Div> {
  div()
    .id(id)
    .debug_selector(move || id.into())
    .absolute()
    .p_1()
    .rounded(radius::MD)
    .border_1()
    .border_color(p.border)
    .bg(p.popover)
    .text_color(p.popover_foreground)
    .shadow_md()
    .occlude()
}

pub(super) fn agent_icon(agent: Agent) -> &'static str {
  match agent {
    Agent::Claude => theme::ICON_AGENT_CLAUDE,
    Agent::Codex => theme::ICON_AGENT_CODEX,
    Agent::Cursor => theme::ICON_AGENT_CURSOR,
    Agent::Copilot => theme::ICON_AGENT_COPILOT,
  }
}

/// `MarkdownContent`'s prose, at the type scale step nearest `size` (`Lg`
/// for `prose`, `Sm` for `prose-sm`); the step sets the line height.
pub(super) fn markdown(id: impl Into<SharedString>, text: &str, size: f32, dark: bool) -> Div {
  markdown_view(id, text, size, dark, false)
}

/// [`markdown`] that fills its parent and scrolls itself, drawing only the
/// blocks on screen: for a whole document.
pub(super) fn scrolling_markdown(
  id: impl Into<SharedString>,
  text: &str,
  size: f32,
  dark: bool,
) -> Div {
  markdown_view(id, text, size, dark, true).size_full()
}

fn markdown_text_size(size: f32) -> TextSize {
  match size {
    s if s <= 11. => TextSize::Xs,
    s if s <= 12. => TextSize::Code,
    s if s <= 14. => TextSize::Sm,
    s if s <= 17. => TextSize::Lg,
    _ => TextSize::Xl,
  }
}

fn markdown_view(
  id: impl Into<SharedString>,
  text: &str,
  size: f32,
  dark: bool,
  scrollable: bool,
) -> Div {
  let id: SharedString = id.into();
  let text_size = markdown_text_size(size);
  let size = text_size.metrics().0;
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
  ui::text(div(), text_size).child(
    TextView::markdown(id, prepared_markdown(text))
      .style(style)
      .selectable(true)
      .scrollable(scrollable),
  )
}

/// The texts [`prepared_markdown`] keeps; a page shows only a few.
const PREPARED_MAX: usize = 64;

thread_local! {
  /// Markdown texts as drawn, by their source.
  static PREPARED: RefCell<HashMap<String, SharedString>> = RefCell::default();
}

/// `text` as `MarkdownContent` draws it: HTML comments never show, and
/// shortcodes show as emoji, as on GitHub. Kept by source, since every frame
/// draws it again: a long README cost a regex pass, `emojify` and a copy per
/// frame, and the same string lets `TextView` skip comparing the text.
fn prepared_markdown(text: &str) -> SharedString {
  if let Some(hit) = PREPARED.with(|p| p.borrow().get(text).cloned()) {
    return hit;
  }
  #[expect(clippy::expect_used, reason = "constant pattern")]
  static HTML_COMMENT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<!--.*?-->").expect("regex"));
  let prepared: SharedString =
    super::code_data::emojify(&HTML_COMMENT.replace_all(text, "")).into();
  PREPARED.with(|p| {
    let mut p = p.borrow_mut();
    if p.len() >= PREPARED_MAX {
      p.clear();
    }
    p.insert(text.to_string(), prepared.clone());
  });
  prepared
}

impl Render for CodeView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    let p = theme::palette(self.dark);
    let left = div()
      .id("code-scroll")
      .track_scroll(&self.scroll)
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
          .children(self.about(&p, window, cx))
          .child(self.action_row(&p, cx))
          .children(self.stack_panel(&p, cx))
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
          .child(self.history(&p, window, cx)),
      )
  }
}

/// The icon and colour an alert of `level` shows, as [`ui::Alert`] draws it.
fn severity_icon(level: ui::Severity, p: &Palette) -> (&'static str, gpui::Hsla) {
  match level {
    ui::Severity::Info => (theme::ICON_INFO, p.primary),
    ui::Severity::Warning => (theme::ICON_TRIANGLE_ALERT, p.warning),
    ui::Severity::Danger => (theme::ICON_CIRCLE_ALERT, p.destructive),
  }
}

/// Why the last rebase skipped this workspace, or ran onto a target it
/// could not fetch. Clears once a rebase passes its checks.
fn rebase_alert(hold: &RebaseHold, p: &Palette) -> ui::Alert {
  match hold {
    RebaseHold::ParentDirty { branch } => ui::Alert::new(
      "code-rebase-held-parent-dirty",
      format!("Rebase skipped: {branch} has uncommitted changes"),
      p,
    )
    .severity(ui::Severity::Warning)
    .message(format!(
      "Commit or discard the changes in {branch}; the next rebase picks this workspace up."
    )),
    RebaseHold::TargetDiverged { bookmark } => ui::Alert::new(
      "code-rebase-held-diverged",
      format!("Rebase skipped: {bookmark} diverged from origin"),
      p,
    )
    .severity(ui::Severity::Warning)
    .message(format!(
      "Local {bookmark} and origin/{bookmark} each have commits the other lacks. \
       Push {bookmark}, or reset it to origin/{bookmark}, then rebase."
    )),
    RebaseHold::StaleTarget { .. } => {
      ui::Alert::new("code-rebase-stale-target", "Rebased without fetching", p)
        .severity(ui::Severity::Info)
        .message("Fetching origin failed, so the rebase used the last-fetched target.")
    }
  }
}

fn skeleton(p: &Palette) -> impl IntoElement {
  let bar = |h: f32| div().h(zpx(h)).rounded(radius::SM).bg(p.muted.opacity(0.4));
  div()
    .debug_selector(|| "workspace-overview-skeleton".into())
    .flex()
    .flex_col()
    .gap_4()
    .child(pulse(
      div()
        .h(zpx(40.))
        .rounded(radius::LG)
        .bg(p.muted.opacity(0.5)),
      "sk-1",
    ))
    .child(pulse(
      div()
        .h(zpx(40.))
        .rounded(radius::LG)
        .bg(p.muted.opacity(0.5)),
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
            .rounded(radius::SM)
            .bg(p.muted.opacity(0.5)),
          "sk-3",
        ))
        .child(pulse(bar(12.), "sk-4"))
        .child(pulse(bar(12.).w(relative(11. / 12.)), "sk-5"))
        .child(pulse(bar(12.).w(relative(10. / 12.)), "sk-6")),
    )
}

impl CodeView {
  /// The tab's alerts, as [`code_data::code_alerts`] gathers them.
  pub(super) fn alerts(&self, cx: &App) -> Vec<CodeAlert> {
    let workspace_id = self.parent.workspace_id();
    let setup_rerunning = workspace_id.is_some_and(|id| {
      checks_run::keys(cx, &self.parent.repo_path).contains(&CheckRun::Setup.key(id))
    });
    code_data::code_alerts(&AlertState {
      conflicted_files: &self.parent.conflicted_files,
      rebase_hold: self.rebase_hold.as_ref(),
      setup: self.setup.as_ref(),
      in_workspace: workspace_id.is_some(),
      setup_rerunning,
      branch_deleted: self
        .branch_deleted
        .then(|| {
          self
            .parent
            .workspace
            .as_ref()
            .map(|w| w.branch_name.as_str())
        })
        .flatten(),
    })
  }

  /// One row under the title card: Agent on the left; the alerts button
  /// and Go to file on the right.
  fn action_row(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let react = self.react.clone();
    let agent = Button::new("code-agent", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .gap_1p5()
      .icon(theme::ICON_BOT, 14.)
      .label("Agent")
      .on_click(move |_, _, cx| {
        let react = react.clone();
        cx.defer(move |cx| {
          react(
            ScreenAction::Press {
              id: "code.agentPrompt".into(),
            },
            cx,
          )
        });
      });
    div()
      .debug_selector(|| "code-action-row".into())
      .flex()
      .items_center()
      .gap_2()
      .child(agent)
      .child(div().flex_1())
      .children(self.status_button(p, cx))
      .child(self.go_to_file(p))
  }

  /// Every alert behind one button that counts them by severity; it opens
  /// a popover with each alert and its actions. Hidden with no alerts.
  fn status_button(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let alerts = self.alerts(cx);
    if alerts.is_empty() {
      self.status_open = false;
      return None;
    }
    let counts = code_data::severity_counts(&alerts);
    let name = counts
      .iter()
      .map(|(level, n)| {
        let what = match level {
          ui::Severity::Danger => "error",
          ui::Severity::Warning => "warning",
          ui::Severity::Info => "info",
        };
        format!("{n} {what}")
      })
      .collect::<Vec<_>>()
      .join(", ");
    let mut button = Button::new("code-status", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .gap_2()
      .aria_label(format!("Alerts: {name}"));
    for (level, n) in counts {
      let (icon, color) = severity_icon(level, p);
      button = button.child(
        div()
          .flex()
          .items_center()
          .gap_1()
          .child(ui::icon(icon, 14., color))
          .child(n.to_string()),
      );
    }
    // A press outside the open popover closes it, so this only opens.
    let button = button.on_click(cx.listener(|view, _, _, cx| {
      view.status_open = true;
      cx.notify();
    }));
    let popover = self.status_open.then(|| {
      let items: Vec<AnyElement> = alerts.iter().map(|a| self.alert(a, p, cx)).collect();
      ui::Popover::new("code-status-popover", p)
        .w(zpx(420.))
        .gap_2()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .dismiss_with("code-status", cx, |view, cx| {
          let open = std::mem::take(&mut view.status_open);
          if open {
            cx.notify();
          }
          open
        })
        .children(items)
    });
    Some(div().relative().child(button).children(popover))
  }

  fn alert(&self, alert: &CodeAlert, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
    match alert {
      CodeAlert::Conflicts { count } => self.conflicts(*count, p).into_any_element(),
      CodeAlert::BranchDeleted { branch } => {
        self.branch_deleted_alert(branch, p).into_any_element()
      }
      CodeAlert::Rebase(hold) => rebase_alert(hold, p).into_any_element(),
      CodeAlert::Setup {
        status,
        running,
        run_id,
      } => self
        .setup_alert(status, *running, *run_id, alert.severity(), p, cx)
        .into_any_element(),
    }
  }

  fn conflicts(&self, count: usize, p: &Palette) -> impl IntoElement {
    let react = self.react.clone();
    ui::Alert::new(
      "code-conflicts",
      format!(
        "{count} {} {} conflicts",
        if count == 1 { "file" } else { "files" },
        if count == 1 { "has" } else { "have" }
      ),
      p,
    )
    .severity(ui::Severity::Danger)
    .message("Resolve them before you push or merge.")
    .action(
      Button::new("code-view-conflicts", p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .label("Resolve…")
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
    )
  }

  /// The workspace's branch was deleted, usually by a fetch after its pull
  /// request merged: the work has landed, so offer to archive it.
  fn branch_deleted_alert(&self, branch: &str, p: &Palette) -> impl IntoElement {
    let react = self.react.clone();
    ui::Alert::new("code-branch-deleted", "Branch deleted on remote", p)
      .severity(ui::Severity::Warning)
      .message(format!(
        "{branch} no longer exists, usually because its pull request merged. \
           Archive the workspace once you no longer need it."
      ))
      .action(
        Button::new("code-branch-deleted-archive", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .label("Archive")
          .on_click(move |_, _, cx| {
            let react = react.clone();
            cx.defer(move |cx| {
              react(
                ScreenAction::Press {
                  id: "workspace.archive".into(),
                },
                cx,
              )
            });
          }),
      )
  }

  /// The workspace setup script: its state, re-run, and a link to its
  /// output in the Logs tab once it has run.
  fn setup_alert(
    &self,
    status: &str,
    running: bool,
    run_id: Option<i64>,
    severity: ui::Severity,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let (title, message) = match status {
      _ if running && status != "pending" => (
        "Running the workspace setup script",
        "Checks run once it finishes.",
      ),
      "pending" => (
        "The workspace setup script has not run yet",
        "Checks run once it finishes.",
      ),
      "failed" => ("Workspace setup script failed", "Checks can still run."),
      _ => ("Workspace setup script passed", "Checks can run."),
    };
    let logs = (self.logs_enabled && run_id.is_some()).then(|| {
      let react = self.react.clone();
      Button::new("code-setup-logs", p)
        .variant(Variant::Ghost)
        .size(Size::Sm)
        .label("View logs")
        .on_click(move |_, _, cx| {
          let react = react.clone();
          cx.defer(move |cx| {
            react(
              ScreenAction::Field {
                field: "workspace.openLogs".into(),
                value: "setup-script".into(),
              },
              cx,
            )
          });
        })
    });
    ui::Alert::new("code-setup", title, p)
      .severity(severity)
      .message(message)
      .actions(logs)
      .action(
        Button::new("code-setup-rerun", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .disabled(running)
          .label("Re-run setup script")
          .on_click(cx.listener(|view, _, _, cx| view.rerun_setup(cx))),
      )
  }

  /// The workspace's full title, wrapped rather than truncated, with an
  /// inline editor behind its pencil.
  /// The workspace's title and description in one card, with the pull
  /// request's sync state in its footer. While a local draft differs from
  /// the PR, tabs switch between the draft and what GitHub has.
  fn about(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    let draft = self
      .pr_sync()
      .is_some_and(|sync| !(sync.title && sync.description));
    if !draft {
      // The next draft opens on itself.
      self.about_tab = AboutTab::Draft;
    }
    let card = section(p)
      .debug_selector(|| "workspace-about".into())
      .px_4()
      .py_3()
      .flex()
      .flex_col()
      .gap_3()
      .when(draft, |card| card.child(self.about_tabs(p, cx)));
    let card = if draft && self.about_tab == AboutTab::GitHub {
      card.child(self.github_text(p))
    } else {
      card
        .child(self.title(p, cx)?)
        .children(self.description(p, window, cx))
    };
    Some(card.children(self.pr_footer(p, cx)))
  }

  fn about_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let view = cx.weak_entity();
    ui::Tabs::new("workspace-about-tabs", p)
      .style(ui::TabsStyle::Underline)
      .tab(ui::Tab::new("workspace-about-tab-draft", "Local draft"))
      .tab(ui::Tab::new("workspace-about-tab-github", "GitHub"))
      .selected(match self.about_tab {
        AboutTab::Draft => 0,
        AboutTab::GitHub => 1,
      })
      .on_select(move |ix, _, cx| {
        let tab = [AboutTab::Draft, AboutTab::GitHub][ix];
        view
          .update(cx, |view, cx| view.select_about_tab(tab, cx))
          .ignore_closed();
      })
  }

  /// Shows `tab`; the GitHub tab reads the PR's text again on the way.
  fn select_about_tab(&mut self, tab: AboutTab, cx: &mut Context<Self>) {
    self.about_tab = tab;
    cx.notify();
    if tab == AboutTab::GitHub {
      self.read(
        |host, parent| {
          parent
            .workspace
            .as_ref()
            .and_then(|ws| host.pr_text(&parent.repo_path, ws))
        },
        |view, text, _| view.pr_text = text,
        cx,
      );
    }
  }

  /// The pull request's title and body as GitHub has them, read-only.
  fn github_text(&self, p: &Palette) -> impl IntoElement {
    let panel = div()
      .debug_selector(|| "workspace-about-github".into())
      .flex()
      .flex_col()
      .gap_3();
    let Some((title, body)) = self.pr_text.clone() else {
      return panel.child(
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child("Reading the pull request…"),
      );
    };
    panel
      .child(
        ui::text(div(), TextSize::Xl)
          .font_weight(FontWeight::SEMIBOLD)
          .text_color(p.foreground)
          .whitespace_normal()
          .child(title),
      )
      .child(if body.is_empty() {
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child("No description")
          .into_any_element()
      } else if looks_like_markdown(&body) {
        markdown("workspace-about-github-md", &body, 14., self.dark).into_any_element()
      } else {
        ui::text(div(), TextSize::Sm)
          .whitespace_normal()
          .child(body)
          .into_any_element()
      })
  }

  fn title(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let title = display_title(self.parent.workspace.as_ref()?);
    let heading = |el: Div| {
      ui::text(el, TextSize::Xl)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.foreground)
    };
    let panel = div().debug_selector(|| "workspace-title".into());
    let Some(draft) = self.title_draft.clone() else {
      return Some(
        panel
          .flex()
          .items_start()
          .gap_2()
          .child(
            heading(div())
              .flex_1()
              .min_w_0()
              .whitespace_normal()
              .debug_selector(|| "workspace-title-text".into())
              .child(title.clone()),
          )
          .child(
            Button::new("workspace-title-edit", p)
              .aria_label("Edit workspace title")
              .variant(Variant::Ghost)
              .size(Size::Icon)
              .flex_none()
              .w(zpx(20.))
              .h(zpx(20.))
              .child(ui::icon(theme::ICON_PENCIL, 16., p.muted_foreground))
              .on_click(
                cx.listener(move |view, _, window, cx| view.edit_title(title.clone(), window, cx)),
              ),
          )
          .into_any_element(),
      );
    };
    let saving = self.saving_title;
    let empty = draft.read(cx).value().trim().is_empty();
    Some(
      panel
        .child(
          div()
            .id("workspace-title-editor")
            .flex()
            .flex_col()
            .gap_2()
            .capture_action(cx.listener(|view, action: &Enter, _, cx| {
              if action.secondary {
                view.save_title(false, cx);
                cx.stop_propagation();
              }
            }))
            .capture_action(cx.listener(|view, _: &Escape, _, cx| {
              view.title_draft = None;
              cx.notify();
              cx.stop_propagation();
            }))
            .child(
              heading(div())
                .debug_selector(|| "workspace-title-input".into())
                .child(Textarea::new(&draft)),
            )
            .child(
              div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                  Button::new("workspace-title-cancel", p)
                    .variant(Variant::Ghost)
                    .size(Size::Sm)
                    .disabled(saving)
                    .label("Cancel")
                    .on_click(cx.listener(|view, _, _, cx| {
                      view.title_draft = None;
                      cx.notify();
                    })),
                )
                .child(self.save_button(
                  "workspace-title-save",
                  Menu::SaveTitle,
                  saving || empty,
                  p,
                  Self::save_title,
                  cx,
                )),
            ),
        )
        .into_any_element(),
    )
  }

  fn description(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    let text = self.parent.workspace.as_ref().and_then(description)?;
    let card = div()
      .debug_selector(|| "workspace-description".into())
      .relative();
    let Some(draft) = self.description_draft.clone() else {
      let shown = if looks_like_markdown(&text) {
        div()
          .pr_8()
          .child(markdown("workspace-description-md", &text, 14., self.dark))
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
            div().absolute().top_0().right_0().child(
              Button::new("workspace-description-edit", p)
                .aria_label("Edit description")
                .variant(Variant::Ghost)
                .size(Size::Icon)
                .w(zpx(20.))
                .h(zpx(20.))
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
                view.save_description(false, cx);
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
                .child(self.save_button(
                  "workspace-description-save",
                  Menu::SaveDescription,
                  saving,
                  p,
                  Self::save_description,
                  cx,
                )),
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
      .gap_1p5()
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
    ui::Row::new(id, p)
      .label(title.clone())
      .selected(row.current)
      .relative()
      .w_full()
      .items_start()
      .gap_3()
      .py_2()
      .mx_neg_2()
      .rounded(radius::MD)
      .leading(stack_dot(row.current, p))
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
              .mt_0p5()
              .flex_wrap()
              .child(time)
              .when(ins > 0 || del > 0, |this| {
                this.child(div().ml_auto().child(loc_indicator(
                  ins,
                  del,
                  max,
                  (plus_ch, minus_ch),
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
    div().flex_none().w(zpx(280.)).child(
      div()
        .id("code-go-to-file")
        .debug_selector(|| "code-go-to-file".into())
        .role(gpui::Role::Button)
        .aria_label("Go to file")
        .test_support()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .border_1()
        .border_color(p.border)
        .rounded(radius::LG)
        .bg(p.background)
        .hover(move |s| s.bg(p.hover()))
        .cursor_pointer()
        .w_full()
        .child(ui::icon(theme::ICON_SEARCH, 16., p.muted_foreground))
        .child(
          ui::text(div(), TextSize::Sm)
            .flex_1()
            .text_color(p.muted_foreground)
            .child("Go to file"),
        )
        .child(ui::Kbd::new("secondary-p", p))
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

  /// Records row `i`'s pitch (any row but the first) and, from the first row drawn (`first`),
  /// where row 0 sits in the scrolled content. A new measure draws the view again, which
  /// nothing else would while the screen is cached.
  fn measure_entry_row(&self, i: usize, first: usize, cx: &Context<Self>) -> impl IntoElement {
    let (rows, scroll) = (self.entry_rows.clone(), self.scroll.clone());
    let view = cx.weak_entity();
    gpui::canvas(
      move |b, _, cx| {
        let before = (rows.pitch.get(), rows.top.get());
        if i > 0 {
          rows.pitch.set(Some(b.size.height));
        }
        if i == first {
          let above = match (first, rows.pitch.get()) {
            (0, _) | (_, None) => px(0.),
            (n, Some(pitch)) => pitch * n as f32 - px(1.),
          };
          let top = b.top() - above - scroll.bounds().top() - scroll.offset().y;
          rows.top.set(Some(top));
        }
        if (rows.pitch.get(), rows.top.get()) != before {
          if let Some(view) = view.upgrade() {
            cx.defer(move |cx| view.update(cx, |_, cx| cx.notify()));
          }
        }
      },
      |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
  }

  fn files(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let all = self
      .overview
      .as_ref()
      .map_or(&[][..], |o| o.entries.as_slice());
    // Untouched entries fold behind a toggle, below the changed ones;
    // submodule rows carry their own controls, and an entry without a status
    // may have changed, so both stay up top.
    let untouched =
      |e: &WorkspaceEntry| e.submodule_pin.is_none() && e.status.as_deref() == Some("untouched");
    let (rest, mut entries): (Vec<&WorkspaceEntry>, Vec<&WorkspaceEntry>) =
      all.iter().partition(|e| untouched(e));
    let changed = entries.len();
    let untouched_count = rest.len();
    let show_untouched = self.show_untouched;
    if show_untouched {
      entries.extend(rest);
    }
    let font = self.settings.as_ref().map_or(14., |s| s.font_size);
    let home = self.parent.workspace.is_none();
    let muted_fg = p.muted_foreground;
    let mut list = section(p).flex().flex_col();
    let loaded = self.overview.is_some();
    let empty = all.is_empty();
    let unavailable = self.overview.as_ref().is_some_and(|o| o.unavailable);
    let count = entries.len();
    let shown = self.entry_rows.shown(count, &self.scroll);
    let pitch = self.entry_rows.pitch.get().unwrap_or_default();
    // Rows not drawn keep their room: the first is a border shorter.
    if shown.start > 0 {
      list = list.child(div().h(pitch * shown.start as f32 - px(1.)));
    }
    let mut toggle = (untouched_count > 0).then(|| {
      self
        .unchanged_toggle(untouched_count, changed > 0, p, cx)
        .into_any_element()
    });
    for (i, entry) in entries.iter().enumerate().take(shown.end).skip(shown.start) {
      if i == changed {
        list = list.children(toggle.take());
      }
      let pin = entry.submodule_pin.clone();
      let submodule = pin.is_some();
      let icon = match (&pin, entry.is_directory) {
        (Some(_), _) => (theme::ICON_FOLDER_GIT, ui::Tone::Primary.color(p)),
        (None, true) => (theme::ICON_FOLDER, ui::Tone::Primary.color(p)),
        (None, false) => (theme::ICON_FILE, p.muted_foreground),
      };
      let pip = match entry.status.as_deref() {
        Some("conflict") => Some((ui::Tone::Danger.color(p), "Conflicted")),
        Some("committed") => Some((ui::Tone::Primary.color(p), "Committed")),
        Some("workingCopy") => Some((ui::Tone::Warning.color(p), "Working copy change")),
        _ => None,
      };
      // The pip after the name carries the status; the name stays neutral.
      let name_color = match pip {
        Some(_) => p.foreground,
        None => p.muted_foreground.opacity(0.7),
      };
      let pip_id = SharedString::from(format!("code-entry-status-{}", entry.name));
      // A row has something above it: an earlier row, or the toggle.
      let divided = i > 0 || (i >= changed && untouched_count > 0);
      let id = SharedString::from(format!("code-entry-{}", entry.name));
      let selector = if submodule {
        format!("submodule-row-{}", entry.name)
      } else {
        id.to_string()
      };
      let open = (*entry).clone();
      let synced = entry.submodule_synced.unwrap_or(false);
      let path = entry.name.clone();
      let syncing = self.syncing_submodule;
      list =
        list.child(
          div()
            .debug_selector(move || selector)
            .when(divided, |this| this.border_t_1().border_color(p.border))
            .when(
              count > ENTRIES_ALL && (i == shown.start || i == 1),
              |this| {
                this
                  .relative()
                  .child(self.measure_entry_row(i, shown.start, cx))
              },
            )
            .flex()
            .items_center()
            .hover(move |s| s.bg(p.hover()))
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
                      this.child(div().flex_none().text_color(muted_fg).child(format!(
                        "\u{a0}@ {}",
                        pin.chars().take(7).collect::<String>()
                      )))
                    }),
                )
                .children(pip.map(|(color, label)| {
                  div()
                    .id(pip_id.clone())
                    .role(Role::Image)
                    .aria_label(label)
                    .test_support()
                    .size_2()
                    .flex_none()
                    .rounded_full()
                    .bg(color)
                }))
                .on_click(cx.listener(move |view, _, _, cx| view.open_entry(&open, cx))),
            )
            .when(home && submodule, |this| {
              this.child(
                ui::text(div(), TextSize::Xs)
                  .id(SharedString::from(format!("submodule-sync-{path}")))
                  .flex()
                  .items_center()
                  .gap_1p5()
                  .flex_none()
                  .pr_4()
                  .pl_2()
                  .text_color(p.muted_foreground)
                  .cursor_pointer()
                  .when(syncing, |this| this.opacity(0.5))
                  .child(ui::Checkbox::new(format!("submodule-sync-box-{path}"), p).checked(synced))
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
    // The toggle closes the list when nothing drawn follows it; one scrolled
    // above the drawn rows stays undrawn with them.
    if changed >= shown.end {
      list = list.children(toggle);
    }
    if shown.end < count {
      list = list.child(div().h(pitch * (count - shown.end) as f32));
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

  /// The row that shows or hides the file list's `count` untouched entries.
  fn unchanged_toggle(
    &self,
    count: usize,
    divided: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let show = self.show_untouched;
    let muted_fg = p.muted_foreground;
    let label = if show {
      "Hide unchanged files".to_string()
    } else if count == 1 {
      "Show 1 unchanged file".to_string()
    } else {
      format!("Show {count} unchanged files")
    };
    ui::text(div(), TextSize::Xs)
      .id("code-unchanged-toggle")
      .debug_selector(|| "code-unchanged-toggle".into())
      .role(Role::Button)
      .aria_label(label.clone())
      .aria_expanded(show)
      .test_support()
      .when(divided, |this| this.border_t_1().border_color(p.border))
      .flex()
      .items_center()
      .gap_2()
      .px_4()
      .py_1p5()
      .text_color(muted_fg)
      .cursor_pointer()
      .hover(move |s| s.bg(p.hover()))
      .child(ui::icon(
        if show {
          theme::ICON_CHEVRON_DOWN
        } else {
          theme::ICON_CHEVRON_RIGHT
        },
        12.,
        muted_fg,
      ))
      .child(label)
      .on_click(cx.listener(|view, _, _, cx| {
        view.show_untouched = !view.show_untouched;
        cx.notify();
      }))
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
          .child(markdown("code-readme-md", &text, 16., self.dark))
          .into_any_element(),
        None => ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .text_center()
          .py_4()
          .child("No README.md found")
          .into_any_element(),
      })
  }

  /// The read-only, wrapping text area that shows a failed commit read.
  fn history_error_text(
    &mut self,
    err: &str,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Entity<TextareaState> {
    if let Some(text) = self.history_error_text.clone() {
      if text.read(cx).value() != err {
        text.update(cx, |s, cx| s.set_value(err.to_string(), window, cx));
      }
      return text;
    }
    let text = cx.new(|cx| {
      let mut state = TextareaState::new(window, cx)
        .auto_grow(2, 12)
        .default_value(err.to_string());
      state.set_readonly(true, cx);
      state
    });
    self.history_error_text = Some(text.clone());
    text
  }

  /// The failed read's message in a popover under "Show error", selectable
  /// so it can be copied into a bug report.
  fn history_error_popover(
    &mut self,
    err: &str,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let text = self.history_error_text(err, window, cx);
    let close = |view: &mut Self, cx: &mut Context<Self>| {
      view.history_error_open = false;
      cx.notify();
    };
    deferred(
      anchored().snap_to_window_with_margin(px(8.)).child(
        div()
          .id("code-history-error-popover")
          .debug_selector(|| "code-history-error-popover".into())
          .mt_1()
          .w(zpx(360.))
          .p_2()
          .rounded(radius::MD)
          .border_1()
          .border_color(p.border)
          .bg(p.popover)
          .text_color(p.popover_foreground)
          .shadow_md()
          .occlude()
          .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .on_mouse_down_out(cx.listener(move |view, _, _, cx| close(view, cx)))
          .capture_action(cx.listener(move |view, _: &Escape, _, cx| {
            close(view, cx);
            cx.stop_propagation();
          }))
          .child(
            ui::text(
              Textarea::new(&text)
                .readonly(true)
                .aria_label("Commit read error")
                .px_3()
                .py_2(),
              TextSize::Code,
            )
            .font_family(theme::MONO_FONT),
          ),
      ),
    )
    .with_priority(1)
  }

  fn history(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let Some(log) = self.history.clone() else {
      let status = match self.history_error.clone() {
        Some(err) => div()
          .id("code-history-error")
          .debug_selector(|| "code-history-error".into())
          .role(gpui::Role::Status)
          .aria_label("Couldn't load commits")
          .test_support()
          .w_full()
          .flex()
          .flex_col()
          .items_center()
          .gap_1()
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child("Couldn't load commits"),
          )
          .child(
            div()
              .child(
                Button::new("code-history-show-error", p)
                  .variant(Variant::Outline)
                  .size(Size::Sm)
                  .label("Show error")
                  // A press outside the open popover closes it, so this only opens.
                  .on_click(cx.listener(|view, _, _, cx| {
                    view.history_error_open = true;
                    cx.notify();
                  })),
              )
              .when(self.history_error_open, |el| {
                el.child(self.history_error_popover(&err, p, window, cx))
              }),
          )
          .into_any_element(),
        None => ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child("Loading commits…")
          .into_any_element(),
      };
      return div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .child(status)
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
    for (label, commits) in group_by_day(own.iter().copied()) {
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
        div().mt_3().pl_7().child(
          ui::Row::load_more("history-load-more", loading, p)
            .title("Load more commits")
            .when(!loading, |row| {
              row.on_click(cx.listener(|view, _, _, cx| {
                view.history_limit += HISTORY_PAGE;
                view.load_history(cx);
                cx.notify();
              }))
            }),
        ),
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
        {
          let text = format!("On {} (read-only)", log.target_branch);
          div().child(
            div()
              .id("history-divider")
              .role(Role::Heading)
              .aria_label(text.clone())
              .child(text)
              .test_support(),
          )
        }
      };
      track = track.child(separator(label, p));
      for (label, commits) in group_by_day(target.iter().copied()) {
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
    let warning = ui::Tone::Warning;
    let yellow_bg = warning.tint(p);
    let yellow_border = warning.border(p);
    let dot = if tentative {
      warning.color(p)
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
    let (green, red) = (ui::Tone::GitOpen.text(p), ui::Tone::GitClosed.text(p));
    let change = commit.change_id.clone();
    let id = SharedString::from(format!("commit-item-{}", commit.commit_id));
    let selector = id.to_string();
    let column = p.background.blend(p.muted.opacity(0.2));
    let body = div()
      .id(id)
      .debug_selector(move || selector)
      .flex_1()
      .min_w_0()
      .pt_0p5()
      .rounded(radius::MD)
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
      .hover(move |s| s.bg(p.hover()))
      .child(ui::text(div(), TextSize::Sm).truncate().child(title))
      .when_some(
        match &kind {
          Kind::Tentative(label) if !label.is_empty() => Some(label.clone()),
          _ => None,
        },
        |this, label| {
          this.child(
            ui::text(div(), TextSize::Sm)
              .mt_0p5()
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
          .mt_0p5()
          .flex_wrap()
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
      .rounded(radius::MD)
      .hover(move |s| s.bg(p.hover()))
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

/// The dashed `Diverged from` / `On main (read-only)` rule.
fn separator(label: Div, p: &Palette) -> impl IntoElement {
  // `border-t border-dashed`, as Chromium dashes a 1px border.
  let dashes = || {
    div()
      .flex_1()
      .h(zpx(1.))
      .overflow_hidden()
      .flex()
      .gap_1()
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

/// `WorkspaceLocIndicator`: `+N [bar] -M` in columns `plus_ch`/`minus_ch`
/// characters wide, so the bars line up down a stack.
fn loc_indicator(
  insertions: u32,
  deletions: u32,
  max: u32,
  (plus_ch, minus_ch): (usize, usize),
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
  let (added, removed) = (ui::Tone::GitOpen, ui::Tone::GitClosed);
  let (green, red) = (added.color(p), removed.color(p));
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
    .gap_1p5()
    .child(
      count(format!("+{insertions}"), added.text(p), plus_ch)
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
    .child(count(format!("-{deletions}"), removed.text(p), minus_ch))
}
