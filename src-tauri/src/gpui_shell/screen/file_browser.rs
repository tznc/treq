//! The Code tab's file browser (`FileBrowser`): the workspace's file tree,
//! the open file with its syntax highlighted and its working-copy changes
//! marked, Cmd+F search, and review comments kept while browsing.
//!
//! Drawn from the backend directly ([`file_browser_data`]). React keeps
//! whether it is open and which file or folder it opens on; Back hands the
//! tab back to it, and a finished review goes to it to start the agent.
//!
//! [`file_browser_data`]: super::file_browser_data

mod review;

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use std::{
  cell::RefCell,
  collections::{HashMap, HashSet},
  ops::Range,
  rc::Rc,
  sync::Arc,
  time::Duration,
};

use gpui_kit::component::input::{Escape, InputEvent, InputState, Textarea, TextareaState};
use serde::Deserialize;
use serde_json::json;
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, list, point, prelude::*, px, uniform_list, App, Bounds,
  ClipboardItem, Context, Div, Entity, FontWeight, Hsla, KeyDownEvent, ListAlignment, ListOffset,
  ListState, MouseButton, MouseDownEvent, Pixels, SharedString, Stateful, Subscription, Task,
  UniformListScrollHandle, Window,
};

use super::{
  changes::search::match_ranges,
  changes_data::{ChangedFile, EditorApps},
  changes_review::{count_label, review_markdown, LineComment},
  code::Parent,
  file_browser_data::{
    comment_counts, comment_target, dir_status, file_name, filter_hidden, flatten, is_binary,
    is_markdown, line_marks, line_number_width, parse_review, readme, relative, review_to_save,
    status_title, too_large, FileBrowserHost, LineMarks, TreeRow, COMMENT_HUNK,
  },
  highlight, ActionSink, ScreenAction,
};
use crate::{
  core::WorkspaceEntry,
  gpui_shell::{
    native::{NativeEvent, ToastTone},
    popup::{self, MenuIcon, NativeMenuItem, PopupRequest, PopupSink, Side, TooltipContent},
    theme::{self, Palette},
    time,
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
};

/// `useDebounce(searchQuery, 150)`.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
/// How long edits wait before the review is written (`useDebounce(…, 500)`).
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// How long "Copied" shows.
const COPIED_FOR: Duration = Duration::from_millis(2000);
/// `TextSize::Code`'s size and line height.
const CODE_FONT: f32 = 12.;
const LINE_HEIGHT: f32 = 18.;
/// JetBrains Mono's advance, for `ch` widths.
const CH: f32 = CODE_FONT * 0.6;

/// Which file or folder React opened the browser on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  /// `initialSelectedFile`: a full path.
  #[serde(default)]
  pub selected_file: Option<String>,
  /// `initialExpandedDir`: a full path.
  #[serde(default)]
  pub expanded_dir: Option<String>,
}

/// What React opens the browser on, and the files' revision
/// (`changesRevision`), bumped when they change.
pub struct Opening {
  pub route: Route,
  pub revision: u64,
}

/// The open file's text, split as React splits it.
#[derive(Debug, Clone, Default)]
pub(super) struct Opened {
  pub content: String,
  pub lines: Rc<Vec<String>>,
}

/// The comment box: what it covers, and whether it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Draft {
  pub start: u32,
  pub end: u32,
  pub content: Vec<String>,
}

/// A text box and the subscription that keeps its text.
struct TextInput {
  state: Entity<TextareaState>,
  _changes: Subscription,
}

/// What a copy button copied.
#[derive(Debug, Clone, Copy)]
enum Copied {
  Contents,
  Path,
  Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
  Draft,
  Edit,
  Summary,
}

#[derive(Default)]
pub(super) struct Search {
  pub open: bool,
  query: String,
  pub applied: String,
  pub current: usize,
  /// Every match: its line (0-based) and byte range.
  pub matches: Vec<(usize, Range<usize>)>,
  input: Option<Entity<InputState>>,
  focus: bool,
  debounce: Option<Task<()>>,
  _subscription: Option<Subscription>,
}

pub struct FileBrowserView {
  host: Arc<dyn FileBrowserHost>,
  react: ActionSink,
  popups: PopupSink,
  tips: Tooltips,
  pub(super) parent: Parent,
  route: Route,
  revision: u64,
  dark: bool,
  editors: EditorApps,
  /// The root's entries, `None` until read.
  pub(super) root: Option<Vec<WorkspaceEntry>>,
  cache: HashMap<String, Vec<WorkspaceEntry>>,
  pub(super) expanded: HashSet<String>,
  /// `flatten` of the three fields above, dropped whenever one changes.
  tree_rows: Option<Rc<Vec<TreeRow>>>,
  pub(super) changed: Vec<ChangedFile>,
  tree_scroll: UniformListScrollHandle,
  pub(super) selected: Option<String>,
  modified_at: Option<String>,
  loading_file: bool,
  pub(super) opened: Option<Opened>,
  language: Option<&'static str>,
  pub(super) marks: LineMarks,
  /// Bumped per file read, so a slower earlier read is dropped.
  reads: u64,
  copied: bool,
  copied_path: bool,
  /// The Markdown file whose preview shows.
  preview: Option<String>,
  list: ListState,
  /// What the rows' heights depend on, to measure them again on change.
  rows_key: Vec<(u32, String, usize)>,
  /// The lines picked by dragging (`lineSelection`) and the drag's anchor.
  pub(super) selection: Option<(u32, u32)>,
  anchor: Option<u32>,
  selecting: bool,
  // The review.
  pub(super) comments: Vec<LineComment>,
  pub(super) summary: String,
  pub(super) draft: Option<Draft>,
  pub(super) draft_open: bool,
  draft_text: String,
  draft_input: Option<TextInput>,
  editing: Option<String>,
  edit_text: String,
  edit_input: Option<TextInput>,
  summary_input: Option<TextInput>,
  pub(super) finish_open: bool,
  use_idle: bool,
  copied_review: bool,
  pub(super) discard_dialog: bool,
  hydrated: Option<(String, i64)>,
  pub(super) search: Search,
  anchors: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
  _save: Option<Task<()>>,
  _copied_reset: Option<Task<()>>,
  _find_key: Option<Subscription>,
}

impl HasTooltips for FileBrowserView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl FileBrowserView {
  pub fn new(
    parent: Parent,
    Opening { route, revision }: Opening,
    host: Arc<dyn FileBrowserHost>,
    react: ActionSink,
    popups: PopupSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let editors = host.editor_apps();
    let mut this = Self {
      host,
      react,
      tips: Tooltips::new(popups.clone(), dark),
      popups,
      parent,
      route: Route::default(),
      revision,
      dark,
      editors,
      root: None,
      cache: HashMap::new(),
      tree_rows: None,
      expanded: HashSet::new(),
      changed: Vec::new(),
      tree_scroll: UniformListScrollHandle::new(),
      selected: None,
      modified_at: None,
      loading_file: false,
      opened: None,
      language: None,
      marks: LineMarks::default(),
      reads: 0,
      copied: false,
      copied_path: false,
      preview: None,
      list: ListState::new(0, ListAlignment::Top, px(400.)),
      rows_key: Vec::new(),
      selection: None,
      anchor: None,
      selecting: false,
      comments: Vec::new(),
      summary: String::new(),
      draft: None,
      draft_open: false,
      draft_text: String::new(),
      draft_input: None,
      editing: None,
      edit_text: String::new(),
      edit_input: None,
      summary_input: None,
      finish_open: false,
      use_idle: false,
      copied_review: false,
      discard_dialog: false,
      hydrated: None,
      search: Search::default(),
      anchors: Rc::default(),
      _save: None,
      _copied_reset: None,
      _find_key: None,
    };
    this.start(route, cx);
    this
  }

  /// Reads the tree, the changed files and the kept review, and opens
  /// what `route` asks for.
  fn start(&mut self, route: Route, cx: &mut Context<Self>) {
    let base = self.base();
    self.expanded = HashSet::from([base]);
    self.tree_rows = None;
    self.load_root(cx);
    self.load_changed(cx);
    self.hydrate_review(cx);
    self.apply_route(route, cx);
  }

  pub fn set_parent(
    &mut self,
    parent: Parent,
    Opening { route, revision }: Opening,
    dark: bool,
    cx: &mut Context<Self>,
  ) {
    if self.dark != dark {
      self.dark = dark;
      self.tips.dark = dark;
      cx.notify();
    }
    if self.parent.key() != parent.key() {
      let (host, react, popups) = (self.host.clone(), self.react.clone(), self.popups.clone());
      let opening = Opening { route, revision };
      *self = Self::new(parent, opening, host, react, popups, dark, cx);
      cx.notify();
      return;
    }
    let refresh = self.revision != revision;
    self.parent = parent;
    self.revision = revision;
    if refresh {
      self.refresh(cx);
    }
    self.apply_route(route, cx);
  }

  /// `initialSelectedFile` and `initialExpandedDir`, when they change.
  fn apply_route(&mut self, route: Route, cx: &mut Context<Self>) {
    if self.route == route {
      return;
    }
    let old = std::mem::replace(&mut self.route, route.clone());
    if route.selected_file != old.selected_file {
      if let Some(path) = route.selected_file {
        self.select_file(&path, cx);
      }
    }
    if route.expanded_dir != old.expanded_dir {
      if let Some(dir) = route.expanded_dir {
        self.load_dir(dir, true, true, cx);
      }
    }
  }

  /// The workspace's files changed: read the open file, the expanded
  /// directories and the changed files again.
  fn refresh(&mut self, cx: &mut Context<Self>) {
    if let Some(path) = self.selected.clone() {
      self.select_file(&path, cx);
    }
    let dirs: Vec<String> = self.expanded.iter().cloned().collect();
    for dir in dirs {
      if dir == self.base() {
        self.load_root(cx);
      } else {
        self.load_dir(dir, true, false, cx);
      }
    }
    self.load_changed(cx);
  }

  /// `basePath`: the workspace's directory.
  pub(super) fn base(&self) -> String {
    self.parent.working_directory()
  }

  /// Runs `work` off the main thread, then `apply`s it unless the browser
  /// moved to another workspace meanwhile.
  fn run<T: Send + 'static>(
    &self,
    work: impl FnOnce(&dyn FileBrowserHost) -> T + Send + 'static,
    apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    cx: &mut Context<Self>,
  ) {
    let host = self.host.clone();
    let key = self.parent.key();
    cx.spawn(async move |this, cx| {
      let value = cx
        .background_spawn(async move { work(host.as_ref()) })
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

  fn toast(&self, tone: ToastTone, title: &str, description: impl Into<String>) {
    super::workspace_data::WorkspaceHost::notify(
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
    cx.defer(move |cx| react(action, cx));
  }

  fn popup(&self, request: PopupRequest, cx: &mut App) {
    let popups = self.popups.clone();
    cx.defer(move |cx| popups(request, cx));
  }

  // The tree.

  fn load_root(&mut self, cx: &mut Context<Self>) {
    let base = self.base();
    self.run(
      move |host| host.list_directory(&base).map(filter_hidden),
      |view, result, cx| match result {
        Ok(entries) => {
          let first = view.root.is_none();
          view.cache.insert(view.base(), entries.clone());
          view.root = Some(entries);
          view.tree_rows = None;
          if first {
            view.open_readme(cx);
          }
        }
        Err(err) => {
          log::warn!("Failed to list directory: {err}");
          if view.root.is_none() {
            view.root = Some(Vec::new());
            view.tree_rows = None;
          }
        }
      },
      cx,
    );
  }

  /// Opens the root's README unless React asked for a file or one is open.
  fn open_readme(&mut self, cx: &mut Context<Self>) {
    if self.route.selected_file.is_some() || self.selected.is_some() {
      return;
    }
    let root = self.root.clone().unwrap_or_default();
    match readme(&root) {
      Some(entry) => {
        let path = entry.path.clone();
        self.select_file(&path, cx);
      }
      None => {
        self.selected = None;
        self.modified_at = None;
        self.opened = None;
      }
    }
  }

  /// Reads `dir` (again, when `force`), then expands it when `expand`.
  fn load_dir(&mut self, dir: String, force: bool, expand: bool, cx: &mut Context<Self>) {
    if !force && self.cache.contains_key(&dir) {
      if expand {
        self.expanded.insert(dir);
        self.tree_rows = None;
        cx.notify();
      }
      return;
    }
    let path = dir.clone();
    self.run(
      move |host| host.list_directory(&path).map(filter_hidden),
      move |view, result, _| {
        let entries = match result {
          Ok(entries) => entries,
          Err(err) => {
            view.toast(ToastTone::Error, "Failed to load directory", err);
            Vec::new()
          }
        };
        view.cache.insert(dir.clone(), entries);
        view.tree_rows = None;
        if expand {
          view.expanded.insert(dir);
        }
      },
      cx,
    );
  }

  fn load_changed(&mut self, cx: &mut Context<Self>) {
    let (repo, workspace_id) = (self.parent.repo_path.clone(), self.parent.workspace_id());
    self.run(
      move |host| host.changed_files(&repo, workspace_id),
      |view, result, _| {
        view.changed = result
          .map(|files| ChangedFile::parse(&files))
          .unwrap_or_default();
      },
      cx,
    );
  }

  fn click_dir(&mut self, path: String, cx: &mut Context<Self>) {
    if self.expanded.remove(&path) {
      self.tree_rows = None;
      cx.notify();
    } else {
      self.load_dir(path, false, true, cx);
    }
  }

  fn rel(&self, path: &str) -> String {
    relative(&self.base(), path)
  }

  /// The file's working-copy status.
  fn file_status(&self, path: &str) -> Option<&str> {
    let rel = self.rel(path);
    self
      .changed
      .iter()
      .find(|f| f.path == rel)
      .map(|f| f.status.as_str())
  }

  // The open file.

  /// `handleFileClick`.
  pub(super) fn select_file(&mut self, path: &str, cx: &mut Context<Self>) {
    let refreshing = self.selected.as_deref() == Some(path);
    if !refreshing {
      self.close_search(cx);
      self.selection = None;
      self.anchor = None;
      self.draft = None;
      self.draft_open = false;
      self.draft_input = None;
    }
    self.selected = Some(path.to_string());
    self.reads += 1;
    let read = self.reads;
    let file = path.to_string();
    self.run(
      move |host| host.modified_at(&file),
      move |view, modified, _| {
        if view.reads == read {
          view.modified_at = modified;
        }
      },
      cx,
    );
    if is_binary(path) {
      self.opened = None;
      self.marks = LineMarks::default();
      self.loading_file = false;
      cx.notify();
      return;
    }
    if !refreshing {
      self.loading_file = true;
    }
    let rel = self.rel(path);
    let hunks = self.changed.iter().any(|f| f.path == rel);
    let (repo, workspace_id) = (self.parent.repo_path.clone(), self.parent.workspace_id());
    let file = path.to_string();
    self.run(
      move |host| {
        let content = host.read_file(&file)?;
        if too_large(&content) || !hunks {
          return Ok((content, None));
        }
        let read = host
          .file_hunks(&repo, workspace_id, vec![rel.clone()])
          .map(|batch| {
            batch
              .files
              .into_iter()
              .find(|f| f.path == rel)
              .map(|f| f.hunks)
              .unwrap_or_default()
          });
        Ok::<_, String>((content, Some(read)))
      },
      move |view, result, cx| {
        if view.reads != read {
          return;
        }
        view.loading_file = false;
        match result {
          Ok((content, _)) if too_large(&content) => {
            view.toast(
              ToastTone::Warning,
              "File too large",
              "Files larger than 1MB cannot be displayed.",
            );
            view.selected = None;
            view.opened = None;
          }
          Ok((content, hunks)) => {
            view.marks = match hunks {
              Some(Ok(hunks)) => line_marks(&hunks),
              Some(Err(err)) => {
                log::warn!("Failed to load jj hunks: {err}");
                LineMarks::default()
              }
              None => LineMarks::default(),
            };
            view.show_content(content, refreshing, cx);
          }
          Err(err) => {
            view.toast(ToastTone::Error, "Failed to read file", err);
            view.opened = Some(Opened::default());
            view.marks = LineMarks::default();
          }
        }
      },
      cx,
    );
    cx.notify();
  }

  fn show_content(&mut self, content: String, refreshing: bool, cx: &mut Context<Self>) {
    let lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    let count = lines.len();
    let old = self.opened.as_ref().map_or(0, |o| o.lines.len());
    self.language = self.selected.as_deref().and_then(highlight::language);
    self.opened = Some(Opened {
      content,
      lines: Rc::new(lines),
    });
    if refreshing {
      self.list.splice(0..old, count);
    } else {
      self.list.reset(count);
    }
    self.rows_key.clear();
    self.index_matches();
    cx.notify();
  }

  /// Shows "Copied" on `what` for a while.
  fn copy_flag(&mut self, what: Copied, cx: &mut Context<Self>) {
    match what {
      Copied::Contents => self.copied = true,
      Copied::Path => self.copied_path = true,
      Copied::Review => self.copied_review = true,
    }
    self._copied_reset = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(COPIED_FOR).await;
      let _ = this.update(cx, |view, cx| {
        view.copied = false;
        view.copied_path = false;
        view.copied_review = false;
        cx.notify();
      });
    }));
    cx.notify();
  }

  fn copy_contents(&mut self, cx: &mut Context<Self>) {
    let Some(opened) = self.opened.as_ref() else {
      return;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(opened.content.clone()));
    self.copy_flag(Copied::Contents, cx);
  }

  fn copy_path(&mut self, cx: &mut Context<Self>) {
    let Some(path) = self.selected.clone() else {
      return;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(self.rel(&path)));
    self.copy_flag(Copied::Path, cx);
  }

  // Line selection and comments.

  fn press_line(&mut self, line: u32, e: &MouseDownEvent, cx: &mut Context<Self>) {
    if e.click_count >= 2 {
      return;
    }
    self.selecting = true;
    self.anchor = Some(line);
    self.selection = Some((line, line));
    self.draft_open = false;
    self.draft_input = None;
    cx.notify();
  }

  fn drag_line(&mut self, line: u32, cx: &mut Context<Self>) {
    let (true, Some(anchor)) = (self.selecting, self.anchor) else {
      return;
    };
    let next = Some((anchor.min(line), anchor.max(line)));
    if self.selection != next {
      self.selection = next;
      cx.notify();
    }
  }

  fn end_drag(&mut self, cx: &mut Context<Self>) {
    if self.selecting {
      self.selecting = false;
      cx.notify();
    }
  }

  /// The + button (`handleAddComment`).
  pub(super) fn add_comment(&mut self, line: Option<u32>, cx: &mut Context<Self>) {
    let Some(opened) = self.opened.as_ref() else {
      return;
    };
    let Some((start, end, content)) = comment_target(&opened.lines, self.selection, line) else {
      return;
    };
    if start == end && line.is_some() {
      self.selection = Some((start, end));
    }
    self.draft = Some(Draft {
      start,
      end,
      content,
    });
    self.draft_open = true;
    self.draft_input = None;
    cx.notify();
  }

  fn submit_draft(&mut self, cx: &mut Context<Self>) {
    let text = self.draft_text.trim().to_string();
    let (Some(draft), Some(path)) = (self.draft.take(), self.selected.clone()) else {
      return;
    };
    // The review is kept per workspace; the home repository has none.
    if !text.is_empty() && self.parent.workspace_id().is_some() {
      self.comments.push(LineComment {
        id: uuid::Uuid::new_v4().to_string(),
        file_path: self.rel(&path),
        hunk_id: COMMENT_HUNK.into(),
        start_line: draft.start,
        end_line: draft.end,
        line_content: draft.content,
        text,
        created_at: chrono::Utc::now().to_rfc3339(),
        line_side: None,
        source: None,
        github_author: None,
        github_avatar_url: None,
        github_comment_url: None,
      });
      self.save_review(cx);
    }
    self.draft_open = false;
    self.draft_input = None;
    self.selection = None;
    self.draft_text.clear();
    cx.notify();
  }

  fn cancel_draft(&mut self, cx: &mut Context<Self>) {
    self.draft = None;
    self.draft_open = false;
    self.draft_input = None;
    self.draft_text.clear();
    cx.notify();
  }

  fn delete_comment(&mut self, id: &str, cx: &mut Context<Self>) {
    self.comments.retain(|c| c.id != id);
    if self.editing.as_deref() == Some(id) {
      self.editing = None;
      self.edit_input = None;
    }
    self.save_review(cx);
    cx.notify();
  }

  fn start_edit(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
    self.editing = Some(id.into());
    self.edit_text = text.into();
    self.edit_input = None;
    cx.notify();
  }

  fn save_edit(&mut self, cx: &mut Context<Self>) {
    let text = self.edit_text.trim().to_string();
    if text.is_empty() {
      return;
    }
    let Some(id) = self.editing.take() else {
      return;
    };
    if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
      c.text = text;
    }
    self.edit_input = None;
    self.save_review(cx);
    cx.notify();
  }

  fn cancel_edit(&mut self, cx: &mut Context<Self>) {
    self.editing = None;
    self.edit_input = None;
    cx.notify();
  }

  // The kept review.

  fn hydrate_review(&mut self, cx: &mut Context<Self>) {
    let Some(id) = self.parent.workspace_id() else {
      return;
    };
    let key = (self.parent.repo_path.clone(), id);
    if self.hydrated.as_ref() == Some(&key) {
      return;
    }
    self.hydrated = Some(key);
    let repo = self.parent.repo_path.clone();
    self.run(
      move |host| host.load_browser_review(&repo, id),
      |view, result, _| match result {
        Ok(Some(review)) => {
          let (comments, summary) = parse_review(&review);
          view.comments = comments;
          view.summary = summary;
        }
        Ok(None) => {}
        Err(err) => log::warn!("Failed to load file browser review: {err}"),
      },
      cx,
    );
  }

  /// Writes the review after edits settle, or forgets it once empty.
  fn save_review(&mut self, cx: &mut Context<Self>) {
    let Some(id) = self.parent.workspace_id() else {
      return;
    };
    let repo = self.parent.repo_path.clone();
    if self.comments.is_empty() && self.summary.trim().is_empty() {
      self._save = None;
      self.run(
        move |host| host.clear_browser_review(&repo, id),
        |_, result, _| {
          if let Err(err) = result {
            log::warn!("Failed to clear file browser review: {err}");
          }
        },
        cx,
      );
      return;
    }
    let (comments, summary) = review_to_save(&self.comments, &self.summary);
    self._save = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(SAVE_DELAY).await;
      let _ = this.update(cx, |view, cx| {
        view.run(
          move |host| host.save_browser_review(&repo, id, &comments, summary.as_deref()),
          |_, result, _| {
            if let Err(err) = result {
              log::warn!("Failed to save file browser review: {err}");
            }
          },
          cx,
        );
      });
    }));
  }

  fn markdown(&self) -> String {
    review_markdown(&self.comments, &self.summary, &[], &[], &[])
  }

  fn copy_review(&mut self, cx: &mut Context<Self>) {
    cx.write_to_clipboard(ClipboardItem::new_string(self.markdown()));
    self.copy_flag(Copied::Review, cx);
  }

  /// Plan or Edit: React starts the agent with the review.
  fn send_review(&mut self, plan: bool, cx: &mut Context<Self>) {
    self.send_react(
      ScreenAction::Field {
        field: "fileBrowser.review".into(),
        value: json!({
          "markdown": self.markdown(),
          "mode": if plan { "plan" } else { "acceptEdits" },
          "useIdleAgent": self.use_idle,
        }),
      },
      cx,
    );
    self.reset_review(cx);
  }

  fn reset_review(&mut self, cx: &mut Context<Self>) {
    self.comments.clear();
    self.summary.clear();
    self.summary_input = None;
    self.finish_open = false;
    self.discard_dialog = false;
    self.editing = None;
    self.edit_input = None;
    self.save_review(cx);
    cx.notify();
  }

  fn discard_review(&mut self, cx: &mut Context<Self>) {
    self.reset_review(cx);
    self.toast(ToastTone::Success, "Review canceled", "");
  }

  fn input_for(
    &mut self,
    field: Field,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<Entity<TextareaState>> {
    let (slot, text, placeholder) = match field {
      Field::Draft if self.draft_open && self.draft.is_some() => (
        &self.draft_input,
        self.draft_text.clone(),
        "Add a comment...",
      ),
      Field::Edit if self.editing.is_some() => (&self.edit_input, self.edit_text.clone(), ""),
      Field::Summary if self.finish_open => (
        &self.summary_input,
        self.summary.clone(),
        "Add a summary comment (optional)...",
      ),
      _ => return None,
    };
    if let Some(input) = slot {
      return Some(input.state.clone());
    }
    let state = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(3)
        .placeholder(placeholder)
        .default_value(text)
    });
    let changes = cx.subscribe(&state, move |view, state, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        let value = state.read(cx).value().to_string();
        match field {
          Field::Draft => view.draft_text = value,
          Field::Edit => view.edit_text = value,
          Field::Summary => {
            view.summary = value;
            view.save_review(cx);
          }
        }
        cx.notify();
      }
    });
    if field != Field::Summary {
      state.update(cx, |s, cx| s.focus(window, cx));
    }
    let input = TextInput {
      state: state.clone(),
      _changes: changes,
    };
    match field {
      Field::Draft => self.draft_input = Some(input),
      Field::Edit => self.edit_input = Some(input),
      Field::Summary => self.summary_input = Some(input),
    }
    Some(state)
  }

  fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    for field in [Field::Draft, Field::Edit, Field::Summary] {
      self.input_for(field, window, cx);
    }
    if !self.finish_open {
      self.summary_input = None;
    }
  }

  /// The open file's comments.
  fn file_comments(&self) -> Vec<&LineComment> {
    let Some(path) = self.selected.as_deref() else {
      return Vec::new();
    };
    let rel = self.rel(path);
    self
      .comments
      .iter()
      .filter(|c| c.file_path == rel)
      .collect()
  }

  /// Measures rows again when what follows a line changed.
  fn sync_rows(&mut self) {
    let mut key: Vec<(u32, String, usize)> = self
      .file_comments()
      .iter()
      .map(|c| (c.end_line, c.id.clone(), c.text.len()))
      .collect();
    if let Some(d) = self.draft.as_ref().filter(|_| self.draft_open) {
      key.push((d.end, "draft".into(), 0));
    }
    if let Some(id) = &self.editing {
      key.push((0, format!("edit-{id}"), 0));
    }
    if key != self.rows_key {
      self.rows_key = key;
      let count = self.list.item_count();
      self.list.remeasure_items(0..count);
    }
  }

  // Search.

  fn watch_find_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self._find_key.is_some() {
      return;
    }
    let id = window.window_handle().window_id();
    self._find_key = Some(cx.observe_keystrokes(move |view, event, window, cx| {
      let k = &event.keystroke;
      if window.window_handle().window_id() == id
        && k.key == "f"
        && (crate::gpui_shell::keys::held(k.modifiers).platform
          || crate::gpui_shell::keys::held(k.modifiers).control)
        && !crate::gpui_shell::keys::held(k.modifiers).shift
        && !crate::gpui_shell::keys::held(k.modifiers).alt
      {
        view.open_search(cx);
      }
    }));
  }

  /// Cmd+F, while a text file is open.
  pub(super) fn open_search(&mut self, cx: &mut Context<Self>) {
    if self.selected.as_deref().is_none_or(is_binary) {
      return;
    }
    self.search.open = true;
    self.search.focus = true;
    cx.notify();
  }

  fn close_search(&mut self, cx: &mut Context<Self>) {
    self.search = Search::default();
    cx.notify();
  }

  fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
    self.search.query = query.clone();
    self.search.debounce = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(SEARCH_DEBOUNCE).await;
      let _ = this.update(cx, |view, cx| {
        view.search.applied = query;
        view.search.current = 0;
        view.index_matches();
        cx.notify();
      });
    }));
    cx.notify();
  }

  fn index_matches(&mut self) {
    let query = self.search.applied.clone();
    self.search.matches = match self.opened.as_ref() {
      Some(opened) if !query.is_empty() => opened
        .lines
        .iter()
        .enumerate()
        .flat_map(|(i, line)| match_ranges(line, &query).into_iter().map(move |r| (i, r)))
        .collect(),
      _ => Vec::new(),
    };
    if self.search.current >= self.search.matches.len() {
      self.search.current = 0;
    }
  }

  pub(super) fn step_match(&mut self, delta: isize, cx: &mut Context<Self>) {
    let total = self.search.matches.len() as isize;
    if total == 0 {
      return;
    }
    self.search.current = (self.search.current as isize + delta).rem_euclid(total) as usize;
    let line = self.search.matches[self.search.current].0;
    let half = (f32::from(self.list.viewport_bounds().size.height) / LINE_HEIGHT / 2.) as usize;
    self.list.scroll_to(ListOffset {
      item_ix: line.saturating_sub(half),
      offset_in_item: px(0.),
    });
    cx.notify();
  }

  fn ensure_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.search.open {
      return;
    }
    let input = match &self.search.input {
      Some(input) => input.clone(),
      None => {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        self.search._subscription = Some(cx.subscribe(
          &input,
          |view, input, event: &InputEvent, cx| match event {
            InputEvent::Change => {
              let query = input.read(cx).value().to_string();
              view.set_query(query, cx);
            }
            InputEvent::PressEnter { shift, .. } => {
              view.step_match(if *shift { -1 } else { 1 }, cx)
            }
            _ => {}
          },
        ));
        self.search.input = Some(input.clone());
        input
      }
    };
    if std::mem::take(&mut self.search.focus) {
      input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
      });
    }
  }

  // Menus.

  fn copy_toast(&self, text: String, title: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    self.toast(ToastTone::Success, &title, "");
  }

  /// `FileTreeContextMenu`.
  fn entry_menu(&self, path: &str, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
    let rel = self.rel(path);
    let full = path.to_string();
    let entity = cx.entity().downgrade();
    let copy = |text: String, title: String| {
      let entity = entity.clone();
      move |cx: &mut App| {
        let _ = entity.update(cx, |view, cx| {
          view.copy_toast(text.clone(), title.clone(), cx)
        });
      }
    };
    let mut open = vec![{
      let (host, full) = (self.host.clone(), full.clone());
      NativeMenuItem::item(crate::core::context_menu::REVEAL_LABEL, move |_| {
        host.reveal(&full)
      })
      .icon(MenuIcon::FolderOpen)
    }];
    for (on, scheme, label) in [
      (self.editors.cursor, "cursor", "Open in Cursor"),
      (self.editors.vscode, "vscode", "Open in VSCode"),
      (self.editors.zed, "zed", "Open in Zed"),
    ] {
      if on {
        let (host, full) = (self.host.clone(), full.clone());
        open.push(NativeMenuItem::item(label, move |_| {
          host.open_url(&format!("{scheme}://file/{full}"))
        }));
      }
    }
    let items = vec![
      NativeMenuItem::item(
        "Copy relative path",
        copy(rel, "Relative path copied".to_string()),
      )
      .icon(MenuIcon::Copy),
      NativeMenuItem::item("Copy full path", copy(full, "Full path copied".to_string()))
        .icon(MenuIcon::Copy),
      NativeMenuItem::Separator,
      NativeMenuItem::Submenu {
        text: "Open in...".into(),
        icon: Some(MenuIcon::FolderOpen),
        detail: None,
        items: open,
      },
    ];
    self.popup(
      PopupRequest::ShowMenu {
        entries: popup::native_entries(items),
        at,
        dark: self.dark,
      },
      cx,
    );
  }

  fn tip(
    &self,
    key: impl Into<SharedString>,
    text: impl Into<SharedString>,
    side: Side,
    el: impl IntoElement,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    self
      .tips
      .wrap(key, side, Some(TooltipContent::Text(text.into())), el, cx)
  }

  /// Records its parent's bounds as `key`'s, for a popover below it.
  fn anchor(&self, key: &'static str) -> impl IntoElement {
    let anchors = self.anchors.clone();
    gpui::canvas(
      move |b, _, _| {
        anchors.borrow_mut().insert(key.into(), b);
      },
      |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
  }
}

impl Render for FileBrowserView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    self.watch_find_key(window, cx);
    self.ensure_search_input(window, cx);
    self.ensure_inputs(window, cx);
    self.sync_rows();
    let p = theme::palette(self.dark);
    let header = self.header(&p, cx);
    let tree = self.tree(&p, cx);
    let content = self.content(&p, cx);
    let finish = self.finish_popover(&p, cx);
    let dialog = self.discard_dialog(window, &p, cx);
    ui::text(div(), TextSize::Sm)
      .debug_selector(|| "file-browser".into())
      .size_full()
      .flex()
      .flex_col()
      .overflow_hidden()
      .bg(p.background)
      .text_color(p.foreground)
      .on_mouse_up(
        MouseButton::Left,
        cx.listener(|view, _, _, cx| view.end_drag(cx)),
      )
      .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|view, _, _, cx| view.end_drag(cx)),
      )
      .child(header)
      .child(
        div()
          .flex_1()
          .flex()
          .min_h_0()
          .overflow_hidden()
          .child(tree)
          .child(
            div()
              .flex_1()
              .min_w_0()
              .h_full()
              .bg(p.background)
              .child(content),
          ),
      )
      .children(finish)
      .children(dialog)
  }
}

impl FileBrowserView {
  /// Back, and the review's controls.
  fn header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let n = self.comments.len();
    let muted = p.muted_foreground;
    let fg = p.foreground;
    let back = ui::text(div(), TextSize::Sm)
      .id("file-browser-back")
      .debug_selector(|| "file-browser-back".into())
      .flex()
      .items_center()
      .gap_1()
      .text_color(muted)
      .cursor_pointer()
      .hover(move |s| s.text_color(fg))
      .child(ui::icon(theme::ICON_CHEVRON_LEFT, 16., muted))
      .child("Back")
      .on_click(cx.listener(|view, _, _, cx| {
        view.send_react(
          ScreenAction::Press {
            id: "fileBrowser.back".into(),
          },
          cx,
        )
      }));
    div()
      .flex_none()
      .flex()
      .items_center()
      .justify_between()
      .gap_3()
      .px_4()
      .pt_3()
      .pb_2()
      .border_b_1()
      .border_color(p.border)
      .child(div().child(back))
      .when(n > 0, |this| {
        this.child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .child(
              ui::text(div(), TextSize::Sm)
                .debug_selector(|| "file-browser-pending".into())
                .flex()
                .items_center()
                .gap_1p5()
                .text_color(muted)
                .child(ui::icon(theme::ICON_MESSAGE, 16., p.primary))
                .child(format!("{} pending", count_label(n, "comment"))),
            )
            .child(
              Button::new("discard-review", p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .label("Discard")
                .on_click(cx.listener(|view, _, _, cx| {
                  view.discard_dialog = true;
                  cx.notify();
                })),
            )
            .child(
              div()
                .relative()
                .child(
                  Button::new("finish-review", p)
                    .size(Size::Sm)
                    .gap_2()
                    .icon(theme::ICON_SEND, 12.)
                    .label("Finish review")
                    .on_click(cx.listener(|view, _, _, cx| {
                      view.finish_open = !view.finish_open;
                      cx.notify();
                    })),
                )
                .child(self.anchor("finish-review")),
            ),
        )
      })
  }

  fn tree(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let panel = div()
      .w_72()
      .flex_none()
      .h_full()
      .border_r_1()
      .border_color(p.border)
      .bg(p.sidebar)
      .overflow_hidden();
    let Some(root) = self.root.as_ref() else {
      return panel.child(
        div()
          .debug_selector(|| "file-tree-loading".into())
          .flex()
          .items_center()
          .justify_center()
          .p_4()
          .child(ui::spinner("file-tree-spin", 24., p.muted_foreground)),
      );
    };
    let rows = self
      .tree_rows
      .get_or_insert_with(|| Rc::new(flatten(root, &self.expanded, &self.cache)))
      .clone();
    let count = rows.len();
    panel.child(
      uniform_list(
        "file-tree",
        count,
        cx.processor(move |view, range: Range<usize>, _, cx| {
          let p = theme::palette(view.dark);
          range
            .filter_map(|i| rows.get(i))
            .map(|row| view.tree_row(&row.entry, row.depth, &p, cx))
            .collect::<Vec<_>>()
        }),
      )
      .track_scroll(&self.tree_scroll)
      .size_full(),
    )
  }

  fn tree_row(
    &self,
    entry: &WorkspaceEntry,
    depth: usize,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::AnyElement {
    let id = SharedString::from(format!("file-tree-{}", self.rel(&entry.path)));
    let selector = id.to_string();
    let hover = p.muted.opacity(0.6);
    let mono = |text: &str, color: Hsla| {
      ui::text(div(), TextSize::Sm)
        .font_family(theme::MONO_FONT)
        .min_w_0()
        .truncate()
        .text_color(color)
        .child(text.to_string())
    };
    let menu_path = entry.path.clone();
    let row = div()
      .id(id)
      .debug_selector(move || selector)
      .w_full()
      .flex()
      .items_center()
      .gap_2()
      .px_2()
      .py_1()
      .pl(zpx(depth as f32 * 16. + 8.))
      .rounded(radius::MD)
      .cursor_pointer()
      .hover(move |s| s.bg(hover))
      .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |view, e: &MouseDownEvent, _, cx| {
          view.entry_menu(&menu_path, e.position, cx)
        }),
      );
    let path = entry.path.clone();
    if entry.is_directory {
      let open = self.expanded.contains(&entry.path);
      let rel = self.rel(&entry.path);
      let status = dir_status(&self.changed, &rel);
      let color = status.map_or(p.foreground, |s| status_tone(s).text(p));
      return row
        .child(ui::icon(
          if open {
            theme::ICON_FOLDER_OPEN
          } else {
            theme::ICON_FOLDER
          },
          16.,
          p.foreground,
        ))
        .child(mono(&entry.name, color).font_weight(FontWeight::MEDIUM))
        .when(status.is_some(), |this| {
          this.child(div().ml_auto().flex_none().child(self.tip(
            format!("dir-changes-{rel}"),
            "Contains modified files",
            Side::Top,
            div().size_2().rounded_full().bg(ui::Tone::Warning.color(p)),
            cx,
          )))
        })
        .on_click(cx.listener(move |view, _, _, cx| view.click_dir(path.clone(), cx)))
        .into_any_element();
    }
    let rel = self.rel(&entry.path);
    let status = self.file_status(&entry.path).map(str::to_string);
    let count = comment_counts(&self.comments)
      .get(&rel)
      .copied()
      .unwrap_or(0);
    let color = status
      .as_deref()
      .map_or(p.foreground, |s| status_tone(s).text(p));
    let selected = self.selected.as_deref() == Some(entry.path.as_str());
    let primary = p.primary;
    row
      .when(selected, |this| this.bg(primary.opacity(0.1)))
      .child(ui::icon(theme::ICON_FILE_TEXT, 16., p.muted_foreground))
      .child(mono(&entry.name, color))
      .when(count > 0, |this| {
        this.child(
          div().ml_auto().flex_none().child(
            self.tip(
              format!("comments-{rel}"),
              format!(
                "{count} pending review comment{}",
                if count == 1 { "" } else { "s" }
              ),
              Side::Top,
              ui::text(div(), TextSize::Xs)
                .flex()
                .items_center()
                .gap_0p5()
                .text_color(primary)
                .child(ui::icon(theme::ICON_MESSAGE, 12., primary))
                .child(count.to_string()),
              cx,
            ),
          ),
        )
      })
      .when_some(status, |this, status| {
        let bg = status_dot(&status).map_or(p.muted, |tone| tone.color(p));
        this.child(div().ml_auto().flex_none().child(self.tip(
          format!("status-{rel}"),
          status_title(&status),
          Side::Top,
          div().size_2().rounded_full().bg(bg),
          cx,
        )))
      })
      .on_click(cx.listener(move |view, _, _, cx| view.select_file(&path, cx)))
      .into_any_element()
  }

  fn centered(p: &Palette) -> Div {
    div()
      .size_full()
      .flex()
      .items_center()
      .justify_center()
      .text_color(p.muted_foreground)
  }

  /// `FileContentView`.
  fn content(&mut self, p: &Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
    let Some(path) = self.selected.clone() else {
      return Self::centered(p)
        .debug_selector(|| "file-browser-empty".into())
        .child("Select a file to view its contents")
        .into_any_element();
    };
    if is_binary(&path) {
      return Self::centered(p)
        .debug_selector(|| "file-browser-binary".into())
        .flex_col()
        .gap_2()
        .child(ui::icon(theme::ICON_CIRCLE_ALERT, 32., p.muted_foreground))
        .child("Binary file - cannot display")
        .child(ui::text(div(), TextSize::Sm).child(file_name(&path).to_string()))
        .into_any_element();
    }
    if self.loading_file {
      return Self::centered(p)
        .debug_selector(|| "file-browser-loading".into())
        .child(ui::spinner("file-spin", 24., p.muted_foreground))
        .into_any_element();
    }
    let Some(opened) = self.opened.clone().filter(|o| !o.content.is_empty()) else {
      return Self::centered(p)
        .debug_selector(|| "file-browser-failed".into())
        .child("Failed to load file content")
        .into_any_element();
    };
    let markdown = is_markdown(&path);
    let previewing = markdown && self.preview.as_deref() == Some(path.as_str());
    let body = if previewing {
      div()
        .id("markdown-preview")
        .debug_selector(|| "file-markdown-preview".into())
        .size_full()
        .overflow_y_scroll()
        .bg(p.background)
        .px_8()
        .py_6()
        .child(super::code::markdown(
          "file-markdown",
          &opened.content,
          14.,
          self.dark,
        ))
        .into_any_element()
    } else {
      div()
        .size_full()
        .relative()
        .child(
          ui::text(div(), TextSize::Sm)
            .size_full()
            .font_family(theme::MONO_FONT)
            .child(
              list(
                self.list.clone(),
                cx.processor(|view, ix: usize, _, cx| view.code_row(ix, cx)),
              )
              .size_full(),
            ),
        )
        .children(self.search_bar(p, cx))
        .into_any_element()
    };
    div()
      .debug_selector(|| "file-content".into())
      .size_full()
      .flex()
      .flex_col()
      .bg(theme::palette(self.dark).code_background())
      .child(self.content_header(&path, markdown, previewing, p, cx))
      .child(div().flex_1().min_h_0().overflow_hidden().child(body))
      .into_any_element()
  }

  fn content_header(
    &self,
    path: &str,
    markdown: bool,
    previewing: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let rel = self.rel(path);
    let muted = p.muted_foreground;
    let fg = p.foreground;
    let left = div()
      .flex()
      .items_center()
      .gap_3()
      .min_w_0()
      .child(
        ui::text(div(), TextSize::Sm)
          .debug_selector(|| "file-content-path".into())
          .font_family(theme::MONO_FONT)
          .text_color(muted)
          .child(rel),
      )
      .child(
        self.tip(
          "copy-file-path",
          "Copy file path",
          Side::Top,
          copy_button(
            "copy-file-path",
            "Copy file path",
            self.copied_path,
            false,
            p,
          )
          .on_click(cx.listener(|view, _, _, cx| view.copy_path(cx))),
          cx,
        ),
      );
    let mut right = div().flex().flex_none().items_center().gap_2();
    if let Some(modified) = &self.modified_at {
      right = right.child(
        self.tip(
          "file-modified",
          time::full(modified),
          Side::Top,
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "file-modified".into())
            .text_color(muted.opacity(0.9))
            .hover(move |s| s.text_color(muted))
            .child(format!("Modified {}", time::relative(modified))),
          cx,
        ),
      );
    }
    if markdown {
      let tab = |id: &'static str, icon: &'static str, label: &'static str, active: bool| {
        let color = if active { fg } else { muted };
        ui::text(div(), TextSize::Xs)
          .id(id)
          .debug_selector(move || id.into())
          .flex()
          .items_center()
          .gap_1()
          .px_2()
          .py_0p5()
          .rounded(radius::MD)
          .font_weight(FontWeight::MEDIUM)
          .cursor_pointer()
          .text_color(color)
          .when(active, |this| this.bg(p.background).shadow_sm())
          .child(ui::icon(icon, 14., color))
          .child(label)
      };
      let file = path.to_string();
      right = right.child(
        div()
          .flex()
          .gap_0p5()
          .p_0p5()
          .rounded(radius::MD)
          .bg(p.muted)
          .child(
            tab("markdown-code", theme::ICON_CODE, "Code", !previewing).on_click(cx.listener(
              |view, _, _, cx| {
                view.preview = None;
                cx.notify();
              },
            )),
          )
          .child(
            tab(
              "markdown-preview-tab",
              theme::ICON_EYE,
              "Preview",
              previewing,
            )
            .on_click(cx.listener(move |view, _, _, cx| {
              view.preview = Some(file.clone());
              cx.notify();
            })),
          ),
      );
    }
    right = right.child(
      self.tip(
        "copy-file-contents",
        "Copy file contents",
        Side::Top,
        copy_button(
          "copy-file-contents",
          "Copy file contents",
          self.copied,
          true,
          p,
        )
        .on_click(cx.listener(|view, _, _, cx| view.copy_contents(cx))),
        cx,
      ),
    );
    div()
      .flex_none()
      .flex()
      .items_center()
      .justify_between()
      .px_4()
      .pt_2()
      .pb_2()
      .border_b_1()
      .border_color(p.border)
      .child(left)
      .child(right)
  }

  /// A source line, and the comments under it (`FileBrowserListRow`).
  fn code_row(&mut self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
    let p = theme::palette(self.dark);
    let Some(opened) = self.opened.clone() else {
      return div().into_any_element();
    };
    let Some(text) = opened.lines.get(ix) else {
      return div().into_any_element();
    };
    let n = ix as u32 + 1;
    let added = self.marks.added.contains(&n);
    let deleted = self.marks.deleted_after.contains(&n);
    let comments: Vec<LineComment> = self.file_comments().into_iter().cloned().collect();
    let has_comment = comments
      .iter()
      .any(|c| n >= c.start_line && n <= c.end_line);
    let selected = self.selection.is_some_and(|(s, e)| n >= s && n <= e);
    let width = line_number_width(opened.lines.len());
    let bg = if selected {
      Some(p.selected())
    } else if added {
      Some(ui::Tone::GitOpen.tint(&p))
    } else {
      None
    };
    let hover = p.muted.opacity(0.3);
    let selecting = self.selecting;
    let accent = p.primary;
    let accent_hover = p.primary.opacity(0.85);
    let line = ui::text(div(), TextSize::Code)
      .id(SharedString::from(format!("code-line-{n}")))
      .debug_selector(move || format!("code-line-{n}"))
      .group("code-line")
      .relative()
      .flex()
      .items_start()
      .overflow_hidden()
      .min_h(zpx(18.))
      .when_some(bg, |this, bg| this.bg(bg))
      .when(bg.is_none(), |this| this.hover(move |s| s.bg(hover)))
      .on_mouse_move(cx.listener(move |view, _, _, cx| view.drag_line(n, cx)))
      .when(added, |this| {
        this.child(
          div()
            .absolute()
            .left_0()
            .top_0()
            .w_1()
            .h_full()
            .bg(ui::Tone::GitOpen.color(&p)),
        )
      })
      .when(deleted, |this| {
        this.child(
          div()
            .debug_selector(move || format!("deletion-marker-{n}"))
            .absolute()
            .left_0()
            .bottom(zpx(-1.5))
            .w_2()
            .h(zpx(3.))
            .bg(ui::Tone::GitClosed.color(&p)),
        )
      })
      .child(
        div()
          .id(SharedString::from(format!("code-gutter-{n}")))
          .debug_selector(move || format!("code-gutter-{n}"))
          .relative()
          .flex()
          .flex_none()
          .items_start()
          // Room for the add-comment button's overhang past the number.
          .pr_1()
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, e: &MouseDownEvent, _, cx| view.press_line(n, e, cx)),
          )
          .when(has_comment, |this| {
            this.child(
              div()
                .ml_1()
                .h(zpx(LINE_HEIGHT))
                .flex()
                .items_center()
                .child(ui::icon(theme::ICON_MESSAGE, 12., p.primary)),
            )
          })
          .child(
            div()
              .min_w(zpx(width as f32 * CH))
              .pr_2()
              .pl(zpx(if added { 4. } else { 2. }))
              .flex()
              .justify_end()
              .text_color(p.muted_foreground.opacity(0.5))
              .child(n.to_string()),
          )
          .child(
            div()
              // Over the number's padding: a negative margin collapses the gutter.
              .absolute()
              .top_0()
              .right_0()
              .size_5()
              .flex()
              .items_center()
              .justify_center()
              .when(!selecting, |this| {
                this.child(
                  self.tip(
                    format!("add-comment-tip-{n}"),
                    "Add comment",
                    Side::Top,
                    div()
                      .id(SharedString::from(format!("add-comment-{n}")))
                      .debug_selector(move || format!("add-comment-{n}"))
                      .size_5()
                      .flex()
                      .items_center()
                      .justify_center()
                      .rounded(radius::SM)
                      .bg(accent)
                      .cursor_pointer()
                      .opacity(0.)
                      .group_hover("code-line", |s| s.opacity(1.))
                      .hover(move |s| s.bg(accent_hover))
                      .child(ui::icon(theme::ICON_PLUS, 16., p.primary_foreground))
                      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                      .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.add_comment(Some(n), cx);
                      })),
                    cx,
                  ),
                )
              }),
          ),
      )
      .child(div().flex_1().min_w_0().child(self.marked_line(ix, text)));
    let under: Vec<&LineComment> = comments.iter().filter(|c| c.end_line == n).collect();
    let input_here = self.draft_open && self.draft.as_ref().is_some_and(|d| d.end == n);
    if under.is_empty() && !input_here {
      return line.into_any_element();
    }
    div()
      .child(line)
      .child(self.comment_section(&under, input_here, &p, cx))
      .into_any_element()
  }

  /// A line's text, highlighted, with its search matches marked.
  fn marked_line(&self, ix: usize, text: &str) -> gpui::AnyElement {
    if self.search.applied.is_empty() || !self.search.matches.iter().any(|(l, _)| *l == ix) {
      return highlight::styled(text, self.language, self.dark).into_any_element();
    }
    let primary = theme::palette(self.dark).primary;
    let marks: Vec<(Range<usize>, Hsla)> = self
      .search
      .matches
      .iter()
      .enumerate()
      .filter(|(_, (l, _))| *l == ix)
      .map(|(i, (_, r))| {
        let current = i == self.search.current;
        (r.clone(), primary.opacity(if current { 0.6 } else { 0.3 }))
      })
      .collect();
    highlight::styled_marked(text, self.language, self.dark, &marks).into_any_element()
  }
}

/// The change a file status letter stands for, as the file row's name colour.
fn status_tone(status: &str) -> ui::Tone {
  match status {
    "A" => ui::Tone::Success,
    "D" => ui::Tone::Danger,
    _ => ui::Tone::Warning,
  }
}

/// The dot after a file's name; `None` for a neutral one.
fn status_dot(status: &str) -> Option<ui::Tone> {
  match status {
    "??" | "A" => Some(ui::Tone::Success),
    "D" => Some(ui::Tone::Danger),
    "M" => Some(ui::Tone::Warning),
    "R" => Some(ui::Tone::Primary),
    _ => None,
  }
}

/// A copy button that shows a check for a moment after it copied.
fn copy_button(
  id: &'static str,
  label: &'static str,
  copied: bool,
  bordered: bool,
  p: &Palette,
) -> ui::IconButton {
  let icon = if copied {
    theme::ICON_CHECK
  } else {
    theme::ICON_COPY
  };
  let button = ui::IconButton::new(id, icon, label, p).size(ui::IconButtonSize::Small);
  if bordered {
    button.outline()
  } else {
    button
  }
}
