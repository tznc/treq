//! The Changes tab's review (`useLineSelection`, `useComments`, `useReview`,
//! `ReviewActionBar`): selecting diff lines, comments on lines, files and
//! conflicts, the pending review the backend keeps, and sending it to an
//! agent through React.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{ops::Range, time::Duration};

use super::*;
use crate::core::review::{selected_range, selected_text, word_at, TextPoint};
use crate::gpui_shell::screen::{
  changes_review::{
    commented_text, comments_under, compose_suggestion_comment, count_label, is_outdated,
    line_has_comment, line_location, number_and_side, outdated_label, outdated_markdown,
    parse_suggestion, pending_for_line, pending_for_selection, review_markdown, stale_names,
    suggestion_lines, ConflictNote, LineComment, Pending, Side, Stored, FILE_HUNK,
  },
  commits_data::{selection, LineRef},
};
use gpui::{MouseDownEvent, MouseMoveEvent, Point, Task, TextLayout};

/// How long edits wait before the review is written (`useDebounce(…, 500)`).
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// A diff line as drawn: its file, place and bounds.
pub(in crate::gpui_shell::screen) type DrawnLine = (String, LineRef, Bounds<Pixels>);

/// The diff lines picked by dragging in the gutter (`diffLineSelection`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gpui_shell::screen) struct LineSelection {
  pub path: String,
  pub anchor: LineRef,
  /// The end that moves: the dragged-to line, or the keyboard's cursor.
  pub head: LineRef,
  pub lines: Vec<LineRef>,
  pub dragging: bool,
}

/// Code picked by dragging over the diff's text, apart from the gutter's
/// line range: what Copy takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gpui_shell::screen) struct TextSelection {
  pub path: String,
  pub anchor: TextPoint,
  pub head: TextPoint,
  pub dragging: bool,
}

/// Each drawn line's text layout, for finding the character under the mouse.
pub(in crate::gpui_shell::screen) type Layouts =
  Rc<RefCell<HashMap<(String, LineRef), TextLayout>>>;

/// A text box and the subscription that keeps its text.
pub(in crate::gpui_shell::screen) struct Input {
  pub state: Entity<TextareaState>,
  _changes: Subscription,
}

/// Which box an input belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
  Draft,
  Edit,
  Note,
  Summary,
  Suggestion,
}

#[derive(Default)]
pub(in crate::gpui_shell::screen) struct Review {
  pub selection: Option<LineSelection>,
  pub text: Option<TextSelection>,
  pub(super) layouts: Layouts,
  /// Where the pointer is during a drag, wherever it is in the window.
  pub(super) pointer: Option<Point<Pixels>>,
  /// Scrolls the diff while a drag holds the pointer at or past its edge.
  pub(super) autoscroll: Option<Task<()>>,
  /// Scroll the selection's head into view, within this many frames.
  pub reveal: u8,
  /// Where the selection's head was last drawn.
  pub head_drawn: Rc<RefCell<Option<DrawnLine>>>,
  /// A comment box closed: the diff takes the keyboard back.
  pub refocus: bool,
  /// The next read shows as is: it follows this view's own write to the
  /// checkout, which must not wait behind the stale banner.
  pub apply_next: bool,
  pub comments: Vec<LineComment>,
  /// Whether each comment is outdated, as of the layout it was worked out
  /// at: by id, with the lines it covers.
  outdated: RefCell<HashMap<String, (u64, u32, u32, bool)>>,
  /// Where the comment box is open.
  pub draft: Option<Pending>,
  pub draft_text: String,
  pub(super) draft_input: Option<Input>,
  /// "Suggest change"'s lines, as edited; the comment takes them as a
  /// suggestion block when it is added.
  pub suggestion_text: Option<String>,
  pub(super) suggestion_input: Option<Input>,
  /// The comment, or the conflict's note, being edited.
  pub editing: Option<String>,
  pub edit_text: String,
  pub(super) edit_input: Option<Input>,
  pub notes: Vec<ConflictNote>,
  /// The conflict whose note box is open.
  pub note_open: Option<String>,
  pub note_text: String,
  note_input: Option<Input>,
  pub summary: String,
  summary_input: Option<Input>,
  pub finish_open: bool,
  pub use_idle: bool,
  pub sending: bool,
  pub discard_dialog: bool,
  /// `hasUserAddedComments`.
  pub user_added: bool,
  /// Files that changed since the review started, and the read parked
  /// until Reload.
  pub stale: Vec<String>,
  pub parked: Option<Snapshot>,
  /// Drop comments whose lines are gone once the reloaded diffs are read.
  pub prune: bool,
  /// The workspace whose kept review was read.
  hydrated: Option<(String, i64)>,
  save: Option<Task<()>>,
}

impl Review {
  fn stored(&self) -> Stored {
    Stored {
      comments: self.comments.clone(),
      summary: self.summary.clone(),
      notes: self.notes.clone(),
    }
  }
}

fn now() -> String {
  chrono::Utc::now().to_rfc3339()
}

fn new_id() -> String {
  uuid::Uuid::new_v4().to_string()
}

impl ChangesView {
  /// Reads the kept review once per workspace.
  pub(super) fn hydrate_review(&mut self, cx: &mut Context<Self>) {
    let Some(id) = self.parent.workspace_id() else {
      return;
    };
    let key = (self.parent.repo_path.clone(), id);
    if self.review.hydrated.as_ref() == Some(&key) {
      return;
    }
    self.review.hydrated = Some(key);
    self.run(
      move |host, parent| host.load_review(&parent.repo_path, id),
      |view, result, _| match result {
        Ok(Some(review)) => {
          let stored = Stored::parse(&review);
          if !stored.is_empty() {
            view.review.comments = stored.comments;
            view.review.summary = stored.summary;
            view.review.notes = stored.notes;
            view.review.user_added = true;
          }
        }
        Ok(None) => {}
        Err(err) => log::warn!("Failed to load pending review: {err}"),
      },
      cx,
    );
  }

  /// `isInReviewMode`: a refreshed file list waits behind the stale banner.
  pub(super) fn in_review(&self) -> bool {
    self.review.user_added
      || self.review.draft.is_some()
      || self.review.finish_open
      || !self.review.summary.trim().is_empty()
  }

  /// Writes the review after edits settle, or forgets it once empty.
  fn save_review(&mut self, cx: &mut Context<Self>) {
    if self.read_only() {
      return;
    }
    let Some(id) = self.parent.workspace_id() else {
      return;
    };
    let stored = self.review.stored();
    if stored.is_empty() {
      self.review.save = None;
      self.run(
        move |host, parent| host.clear_review(&parent.repo_path, id),
        |_, result, _| {
          if let Err(err) = result {
            log::warn!("Failed to clear pending review: {err}");
          }
        },
        cx,
      );
      return;
    }
    self.review.save = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(SAVE_DELAY).await;
      this
        .update(cx, |view, cx| {
          let (comments, summary, notes) = stored.to_save();
          view.run(
            move |host, parent| {
              host.save_review(
                &parent.repo_path,
                id,
                &comments,
                summary.as_deref(),
                notes.as_deref(),
              )
            },
            |_, result, _| {
              if let Err(err) = result {
                log::warn!("Failed to save pending review: {err}");
              }
            },
            cx,
          );
        })
        .ignore_closed();
    }));
  }

  // Line selection.

  pub(super) fn press_line(&mut self, path: &str, at: LineRef, cx: &mut Context<Self>) {
    self.review.text = None;
    self.review.selection = Some(LineSelection {
      path: path.into(),
      anchor: at,
      head: at,
      lines: vec![at],
      dragging: true,
    });
    cx.notify();
  }

  /// Shift-click: moves the selection's head to `at`, keeping its anchor, and
  /// keeps dragging from there. Without a selection in `path`, a plain press.
  pub(super) fn extend_line(&mut self, path: &str, at: LineRef, cx: &mut Context<Self>) {
    match self.review.selection.as_mut().filter(|s| s.path == path) {
      Some(s) => s.dragging = true,
      None => return self.press_line(path, at, cx),
    }
    self.drag_line(path, at, cx);
  }

  pub(super) fn drag_line(&mut self, path: &str, at: LineRef, cx: &mut Context<Self>) {
    let Some(s) = self.review.selection.as_ref() else {
      return;
    };
    if !s.dragging || s.path != path {
      return;
    }
    let anchor = s.anchor;
    let Some(hunks) = self.file_hunks(path).map(|d| d.hunks) else {
      return;
    };
    let lines = selection(&hunks, anchor, at);
    if let Some(s) = self.review.selection.as_mut() {
      if s.lines != lines {
        s.head = at;
        s.lines = lines;
        cx.notify();
      }
    }
  }

  pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
    self.review.pointer = None;
    self.review.autoscroll = None;
    if let Some(s) = self.review.selection.as_mut().filter(|s| s.dragging) {
      s.dragging = false;
      cx.notify();
    }
    if let Some(t) = self.review.text.as_mut().filter(|t| t.dragging) {
      t.dragging = false;
      cx.notify();
    }
  }

  pub(super) fn clear_selection(&mut self, cx: &mut Context<Self>) {
    let had = self.review.selection.take().is_some() | self.review.text.take().is_some();
    if had {
      cx.notify();
    }
  }

  // Text selection.

  /// The byte of `at`'s code under `position`, once the line was drawn.
  fn text_index(&self, path: &str, at: LineRef, position: Point<Pixels>) -> Option<usize> {
    let layouts = self.review.layouts.borrow();
    let layout = layouts.get(&(path.to_string(), at))?;
    Some(layout.index_for_position(position).unwrap_or_else(|i| i))
  }

  /// A press on the code: one click starts a text selection, two pick the
  /// word and three the line.
  pub(super) fn press_text(
    &mut self,
    path: &str,
    at: LineRef,
    position: Point<Pixels>,
    clicks: usize,
    cx: &mut Context<Self>,
  ) {
    let Some(i) = self.text_index(path, at, position) else {
      return;
    };
    let code = self
      .file_hunks(path)
      .and_then(|d| d.hunks.get(at.0)?.lines.get(at.1).cloned())
      .map(|t| t.get(1..).unwrap_or("").to_string())
      .unwrap_or_default();
    let (from, to) = match clicks {
      2 => {
        let w = word_at(&code, i);
        (w.start, w.end)
      }
      n if n >= 3 => (0, code.len()),
      _ => (i, i),
    };
    self.review.selection = None;
    self.review.text = Some(TextSelection {
      path: path.into(),
      anchor: (at, from),
      head: (at, to),
      dragging: clicks <= 1,
    });
    cx.notify();
  }

  pub(super) fn drag_text(
    &mut self,
    path: &str,
    at: LineRef,
    position: Point<Pixels>,
    cx: &mut Context<Self>,
  ) {
    if !self
      .review
      .text
      .as_ref()
      .is_some_and(|t| t.dragging && t.path == path)
    {
      return;
    }
    let Some(i) = self.text_index(path, at, position) else {
      return;
    };
    if let Some(t) = self.review.text.as_mut() {
      if t.head != (at, i) {
        t.head = (at, i);
        cx.notify();
      }
    }
  }

  /// Whether a line or text selection is being dragged.
  pub(super) fn dragging(&self) -> bool {
    self.review.selection.as_ref().is_some_and(|s| s.dragging)
      || self.review.text.as_ref().is_some_and(|t| t.dragging)
  }

  /// The pointer moved during a drag: keeps where it is, and scrolls the diff
  /// while it is at or past the diff's top or bottom.
  pub(super) fn drag_pointer(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
    if !self.dragging() {
      return;
    }
    self.review.pointer = Some(position);
    if self.review.autoscroll.is_some()
      || super::keys::autoscroll_step(position.y, self.diff.list.viewport_bounds()) == px(0.)
    {
      return;
    }
    self.review.autoscroll = Some(cx.spawn(async move |this, cx| loop {
      cx.background_executor()
        .timer(Duration::from_millis(16))
        .await;
      let more = this
        .update(cx, |view, cx| view.autoscroll_tick(cx))
        .unwrap_or(false);
      if !more {
        break;
      }
    }));
  }

  /// One step of a drag's scroll: moves the drag to the line now under the
  /// pointer, then scrolls on. `false` once there is nothing more to do.
  fn autoscroll_tick(&mut self, cx: &mut Context<Self>) -> bool {
    let Some(pointer) = self.review.pointer.filter(|_| self.dragging()) else {
      self.review.autoscroll = None;
      return false;
    };
    let path = match (&self.review.selection, &self.review.text) {
      (Some(s), _) if s.dragging => s.path.clone(),
      (_, Some(t)) => t.path.clone(),
      _ => return false,
    };
    if let Some(at) = self.line_under(&path, pointer.y) {
      self.drag_line(&path, at, cx);
      self.drag_text(&path, at, pointer, cx);
    }
    let step = super::keys::autoscroll_step(pointer.y, self.diff.list.viewport_bounds());
    if step == px(0.) {
      self.review.autoscroll = None;
      return false;
    }
    self.diff.list.scroll_by(step);
    cx.notify();
    true
  }

  /// The drawn line of `path` nearest `y`, as of the last frame.
  fn line_under(&self, path: &str, y: Pixels) -> Option<LineRef> {
    let layouts = self.review.layouts.borrow();
    layouts
      .iter()
      .filter(|((p, _), _)| p == path)
      .map(|((_, at), layout)| {
        let b = layout.bounds();
        let off = if y < b.top() {
          b.top() - y
        } else if y > b.bottom() {
          y - b.bottom()
        } else {
          px(0.)
        };
        (off, *at)
      })
      .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
      .map(|(_, at)| at)
  }

  /// The selected code, as Copy takes it; `None` when nothing is selected.
  pub(super) fn selected_code(&self) -> Option<String> {
    let t = self.review.text.as_ref()?;
    let hunks = self.file_hunks(&t.path)?.hunks;
    let text = selected_text(&hunks, t.anchor, t.head);
    (!text.is_empty()).then_some(text)
  }

  /// The selection's comment target, `None` when it mixes sides.
  pub(super) fn selection_target(&self) -> Option<Pending> {
    let s = self
      .review
      .selection
      .as_ref()
      .filter(|s| !s.lines.is_empty())?;
    let hunks = self.file_hunks(&s.path)?.hunks;
    pending_for_selection(&s.path, &hunks, &s.lines)
  }

  /// The + button: the selection when there is one, else this line.
  pub(super) fn add_line_comment(&mut self, path: &str, at: LineRef, cx: &mut Context<Self>) {
    let has_selection = self
      .review
      .selection
      .as_ref()
      .is_some_and(|s| !s.lines.is_empty());
    let draft = if has_selection {
      self.selection_target()
    } else {
      self
        .file_hunks(path)
        .and_then(|d| pending_for_line(path, &d.hunks, at))
    };
    if let Some(draft) = draft {
      self.open_draft(draft, cx);
    }
  }

  pub(super) fn open_draft(&mut self, draft: Pending, cx: &mut Context<Self>) {
    self.review.draft = Some(draft);
    self.review.draft_text.clear();
    self.review.draft_input = None;
    cx.notify();
  }

  /// The file header's comment button.
  pub(super) fn add_file_comment(&mut self, path: &str, cx: &mut Context<Self>) {
    self.collapsed_files.remove(path);
    self.open_draft(Pending::file(path), cx);
  }

  fn submit_draft(&mut self, cx: &mut Context<Self>) {
    let text = compose_suggestion_comment(
      &self.review.draft_text,
      self.review.suggestion_text.as_deref(),
    );
    if text.is_empty() {
      return;
    }
    self.review.suggestion_text = None;
    self.review.suggestion_input = None;
    if self.read_only() {
      if let Some(draft) = self.review.draft.clone() {
        self.submit_pr_draft(draft, text, cx);
      }
      return;
    }
    let Some(draft) = self.review.draft.take() else {
      return;
    };
    self
      .review
      .comments
      .push(draft.comment(&text, new_id(), now()));
    self.review.user_added = true;
    self.review.draft_input = None;
    self.review.draft_text.clear();
    self.review.selection = None;
    self.review.refocus = true;
    self.save_review(cx);
    cx.notify();
  }

  /// "Suggest change": opens a code editor with the commented lines, to edit
  /// into what they should read; pressed again, drops the suggestion.
  fn suggest_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.review.suggestion_text.take().is_some() {
      self.review.suggestion_input = None;
      if let Some(input) = &self.review.draft_input {
        input.state.update(cx, |s, cx| s.focus(window, cx));
      }
      cx.notify();
      return;
    }
    let Some(lines) = self.review.draft.as_ref().and_then(suggestion_lines) else {
      return;
    };
    self.review.suggestion_text = Some(lines);
    self.input_for(Field::Suggestion, window, cx);
    cx.notify();
  }

  /// "Apply suggestion": writes the suggested lines into the file, then the
  /// comment is done with.
  fn apply_suggestion(&mut self, id: &str, cx: &mut Context<Self>) {
    let Some(comment) = self.review.comments.iter().find(|c| c.id == id).cloned() else {
      return;
    };
    let Some(suggestion) = parse_suggestion(&comment.text) else {
      return;
    };
    let original = commented_text(&comment.line_content);
    let workspace = self.parent.working_directory();
    let id = id.to_string();
    self.run(
      move |host, _| {
        host.apply_suggestion(
          &workspace,
          &comment.file_path,
          (comment.start_line, comment.end_line),
          &original,
          &suggestion.suggested,
        )
      },
      move |view, result, cx| match result {
        Ok(()) => {
          view.delete_comment(&id, cx);
          view.toast(ToastTone::Success, "Suggestion applied", "");
          view.review.apply_next = true;
          view.load(cx);
        }
        Err(err) => view.toast(ToastTone::Error, "Could not apply suggestion", err),
      },
      cx,
    );
  }

  fn cancel_draft(&mut self, cx: &mut Context<Self>) {
    self.review.refocus = true;
    self.review.draft = None;
    self.review.draft_input = None;
    self.review.draft_text.clear();
    self.review.suggestion_text = None;
    self.review.suggestion_input = None;
    cx.notify();
  }

  fn delete_comment(&mut self, id: &str, cx: &mut Context<Self>) {
    if self.read_only() {
      if self.review.editing.as_deref() == Some(id) {
        self.cancel_edit(cx);
      }
      self.delete_pr_draft(id.to_string(), cx);
      return;
    }
    self.review.comments.retain(|c| c.id != id);
    if self.review.editing.as_deref() == Some(id) {
      self.cancel_edit(cx);
    }
    self.save_review(cx);
    cx.notify();
  }

  pub(super) fn start_edit(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
    self.review.editing = Some(id.into());
    self.review.edit_text = text.into();
    self.review.edit_input = None;
    cx.notify();
  }

  fn save_edit(&mut self, cx: &mut Context<Self>) {
    let text = self.review.edit_text.trim().to_string();
    if text.is_empty() {
      return;
    }
    if self.read_only() {
      if let Some(id) = self.review.editing.clone() {
        self.save_pr_edit(id, text, cx);
      }
      return;
    }
    let Some(id) = self.review.editing.take() else {
      return;
    };
    if let Some(c) = self.review.comments.iter_mut().find(|c| c.id == id) {
      c.text = text;
    } else if let Some(n) = self.review.notes.iter_mut().find(|n| n.conflict_id == id) {
      n.text = text;
    }
    self.review.edit_input = None;
    self.save_review(cx);
    cx.notify();
  }

  fn cancel_edit(&mut self, cx: &mut Context<Self>) {
    self.review.editing = None;
    self.review.edit_input = None;
    cx.notify();
  }

  // Conflict notes.

  /// The conflict card's "Add comment": one note box open at a time.
  pub(super) fn toggle_note(&mut self, conflict_id: &str, cx: &mut Context<Self>) {
    if self.review.note_open.as_deref() == Some(conflict_id) {
      self.review.note_open = None;
    } else {
      self.review.note_open = Some(conflict_id.into());
      self.review.note_text = self
        .review
        .notes
        .iter()
        .find(|n| n.conflict_id == conflict_id)
        .map(|n| n.text.clone())
        .unwrap_or_default();
    }
    self.review.note_input = None;
    cx.notify();
  }

  fn save_note(&mut self, conflict: &Conflict, cx: &mut Context<Self>) {
    let text = self.review.note_text.trim().to_string();
    if text.is_empty() {
      return;
    }
    let path = conflict
      .id
      .rsplit_once("-conflict-")
      .map_or(conflict.id.as_str(), |(p, _)| p)
      .to_string();
    let note = ConflictNote {
      id: new_id(),
      conflict_id: conflict.id.clone(),
      file_path: path,
      conflict_number: conflict.number,
      text,
      created_at: now(),
    };
    match self
      .review
      .notes
      .iter_mut()
      .find(|n| n.conflict_id == conflict.id)
    {
      Some(existing) => *existing = note,
      None => self.review.notes.push(note),
    }
    self.review.note_open = None;
    self.review.note_input = None;
    self.save_review(cx);
    cx.notify();
  }

  /// Cancel clears the note too (`onClear`).
  fn clear_note(&mut self, conflict_id: &str, cx: &mut Context<Self>) {
    self.review.notes.retain(|n| n.conflict_id != conflict_id);
    if self.review.note_open.as_deref() == Some(conflict_id) {
      self.review.note_open = None;
      self.review.note_input = None;
    }
    if self.review.editing.as_deref() == Some(conflict_id) {
      self.review.editing = None;
      self.review.edit_input = None;
    }
    self.save_review(cx);
    cx.notify();
  }

  // The review.

  /// `hasConflicts || comments.length>0`.
  pub(super) fn show_review_bar(&self) -> bool {
    !self.read_only() && !self.conflicted().is_empty() || !self.review.comments.is_empty()
  }

  fn all_conflicts(&self) -> Vec<Conflict> {
    self
      .conflicted()
      .iter()
      .filter_map(|path| self.file_hunks(path).map(|d| conflicts(path, &d.hunks)))
      .flatten()
      .collect()
  }

  fn markdown(&self) -> String {
    review_markdown(
      &self.review.comments,
      &self.review.summary,
      &self.review.notes,
      &self.all_conflicts(),
      &self.conflicted(),
    )
  }

  fn copy_review(&mut self, cx: &mut Context<Self>) {
    cx.write_to_clipboard(ClipboardItem::new_string(self.markdown()));
  }

  /// Plan or Edit: React starts the agent with the review.
  fn send_review(&mut self, plan: bool, cx: &mut Context<Self>) {
    if self.review.sending {
      return;
    }
    let markdown = self.markdown();
    self.field(
      "changes.review",
      json!({
        "markdown": markdown,
        "mode": if plan { "plan" } else { "acceptEdits" },
        "useIdleAgent": self.review.use_idle,
      }),
      cx,
    );
    if self.review.parked.is_some() {
      self.reload_stale(cx);
    }
    self.reset_review(cx);
  }

  /// Clears everything and the kept review.
  fn reset_review(&mut self, cx: &mut Context<Self>) {
    let hydrated = self.review.hydrated.take();
    let use_idle = self.review.use_idle;
    self.review = Review {
      hydrated,
      use_idle,
      ..Review::default()
    };
    self.save_review(cx);
    cx.notify();
  }

  fn discard_review(&mut self, cx: &mut Context<Self>) {
    self.reset_review(cx);
    self.toast(ToastTone::Success, "Review canceled", "");
  }

  /// A refreshed list that adds, removes or changes a file waits while a
  /// review is open; `true` when `snapshot` was parked.
  pub(super) fn park(&mut self, snapshot: &Snapshot) -> bool {
    let Some(current) = self.snapshot.as_ref().filter(|_| self.in_review()) else {
      self.review.stale.clear();
      self.review.parked = None;
      return false;
    };
    let mut stale: Vec<String> = Vec::new();
    for file in &snapshot.files {
      match current.files.iter().find(|f| f.path == file.path) {
        Some(old) if old.status == file.status => {}
        _ => stale.push(file.path.clone()),
      }
    }
    for file in &current.files {
      if !snapshot.files.iter().any(|f| f.path == file.path) {
        stale.push(file.path.clone());
      }
    }
    if stale.is_empty() {
      return false;
    }
    for path in stale {
      if !self.review.stale.contains(&path) {
        self.review.stale.push(path);
      }
    }
    self.review.parked = Some(snapshot.clone());
    true
  }

  /// A refresh that lists the same files while a review is open: their
  /// diffs are read aside, and files whose diff changed (an agent edited
  /// them) wait behind the stale banner like added or removed ones.
  pub(super) fn recheck(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
    // A file whose content id held shows the hunks it would read.
    let paths: Vec<String> = snapshot
      .files
      .iter()
      .filter(|f| draws_hunks(&f.path, &f.status))
      .map(|f| &f.path)
      .filter(|path| {
        let id = snapshot.content_ids.get(*path);
        id.is_none() || self.hunks.get(*path).and_then(|d| d.content_id.as_ref()) != id
      })
      .cloned()
      .collect();
    if paths.is_empty() {
      return;
    }
    let token = snapshot.token.clone();
    self.run(
      move |host, parent| {
        host.file_hunks(
          &parent.repo_path,
          parent.workspace_id(),
          paths,
          token.as_deref(),
        )
      },
      move |view, result, cx| {
        let read = match result {
          Ok(read) => read,
          Err(err) => return log::warn!("Failed to recheck file hunks: {err}"),
        };
        let mut changed = false;
        for file in read.files {
          let shown = view
            .hunks
            .get(&file.path)
            .filter(|d| !d.loading)
            .map(|d| hunks_hash(&d.hunks));
          if shown.is_some_and(|hash| hash != hunks_hash(&file.hunks)) {
            changed = true;
            if !view.review.stale.contains(&file.path) {
              view.review.stale.push(file.path);
            }
          }
        }
        if changed {
          view.review.parked = Some(snapshot);
          cx.notify();
        }
      },
      cx,
    );
  }

  /// The stale banner's Reload.
  fn reload_stale(&mut self, cx: &mut Context<Self>) {
    let Some(snapshot) = self.review.parked.take() else {
      return;
    };
    self.review.stale.clear();
    self.review.prune = true;
    self.apply(snapshot, cx);
  }

  /// After a Reload's diffs are read: comments whose file or hunk is gone.
  pub(super) fn prune_comments(&mut self, cx: &mut Context<Self>) {
    if !std::mem::take(&mut self.review.prune) {
      return;
    }
    let listed: HashSet<String> = self.files().iter().map(|f| f.path.clone()).collect();
    let before = self.review.comments.len();
    let hunks = self.hunks.clone();
    self.review.comments.retain(|c| {
      listed.contains(&c.file_path)
        && (c.is_file()
          || hunks
            .get(&c.file_path)
            .is_some_and(|d| d.hunks.iter().any(|h| h.id == c.hunk_id)))
    });
    let dropped = before - self.review.comments.len();
    if dropped > 0 {
      self.toast(
        ToastTone::Warning,
        "Comments discarded",
        format!(
          "{} discarded (lines changed)",
          count_label(dropped, "outdated comment")
        ),
      );
      self.save_review(cx);
    }
  }

  fn outdated(&self, path: &str) -> Vec<&LineComment> {
    let data = self.file_hunks(path).filter(|d| !d.loading);
    self
      .review
      .comments
      .iter()
      .filter(|c| c.file_path == path)
      .filter(|c| is_outdated(c, data.as_ref().map(|d| d.hunks.as_slice())))
      .collect()
  }

  fn copy_outdated(&self, cx: &mut App) {
    let all: Vec<&LineComment> = self
      .review
      .comments
      .iter()
      .filter(|c| {
        is_outdated(
          c,
          self
            .file_hunks(&c.file_path)
            .filter(|d| !d.loading)
            .as_ref()
            .map(|d| d.hunks.as_slice()),
        )
      })
      .collect();
    cx.write_to_clipboard(ClipboardItem::new_string(outdated_markdown(&all)));
    self.toast(
      ToastTone::Success,
      &format!("{} copied", count_label(all.len(), "outdated comment")),
      "",
    );
  }

  /// Right-click with lines selected (`ContextMenu`).
  pub(super) fn line_menu(&self, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
    let Some(target) = self.selection_target() else {
      return;
    };
    let entity = cx.entity().downgrade();
    let add = {
      let (entity, target) = (entity.clone(), target.clone());
      move |cx: &mut App| {
        entity
          .update(cx, |view, cx| view.open_draft(target.clone(), cx))
          .ignore_closed();
      }
    };
    let location = line_location(&target);
    let lines = target.line_content.join("\n");
    let copy = |text: String, description: &'static str| {
      let entity = entity.clone();
      move |cx: &mut App| {
        entity
          .update(cx, |view, cx| {
            view.copy(text.clone(), description.into(), cx)
          })
          .ignore_closed();
      }
    };
    let items = vec![
      NativeMenuItem::item("Add comment", add).icon(MenuIcon::MessageSquare),
      NativeMenuItem::item("Copy line location", copy(location, "Line location copied"))
        .icon(MenuIcon::Copy),
      NativeMenuItem::item("Copy lines", copy(lines, "Lines copied")).icon(MenuIcon::Copy),
    ];
    self.popup(
      PopupRequest::ShowMenu {
        entries: crate::gpui_shell::popup::native_entries(items),
        at,
        dark: self.dark,
      },
      cx,
    );
  }

  // Inputs.

  fn input_for(
    &mut self,
    field: Field,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Option<Entity<TextareaState>> {
    let (slot, text, placeholder, rows) = match field {
      Field::Draft if self.review.draft.is_some() => (
        &self.review.draft_input,
        self.review.draft_text.clone(),
        "Add a comment…",
        3,
      ),
      Field::Edit if self.review.editing.is_some() => (
        &self.review.edit_input,
        self.review.edit_text.clone(),
        "",
        3,
      ),
      Field::Note if self.review.note_open.is_some() => (
        &self.review.note_input,
        self.review.note_text.clone(),
        "Add a comment…",
        3,
      ),
      Field::Summary if self.review.finish_open => (
        &self.review.summary_input,
        self.review.summary.clone(),
        "Add a summary comment (optional)…",
        3,
      ),
      Field::Suggestion => {
        let lines = self.review.suggestion_text.as_ref()?;
        (
          &self.review.suggestion_input,
          lines.clone(),
          "Suggested lines",
          lines.lines().count().max(1),
        )
      }
      _ => return None,
    };
    if let Some(input) = slot {
      return Some(input.state.clone());
    }
    let state = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(rows)
        .placeholder(placeholder)
        .default_value(text)
    });
    let changes = cx.subscribe(&state, move |view, state, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        let value = state.read(cx).value().to_string();
        match field {
          Field::Draft => view.review.draft_text = value,
          Field::Edit => view.review.edit_text = value,
          Field::Note => view.review.note_text = value,
          Field::Summary => {
            view.review.summary = value;
            view.save_review(cx);
          }
          Field::Suggestion => view.review.suggestion_text = Some(value),
        }
        cx.notify();
      }
    });
    if field != Field::Summary {
      state.update(cx, |s, cx| s.focus(window, cx));
    }
    let input = Input {
      state: state.clone(),
      _changes: changes,
    };
    match field {
      Field::Draft => self.review.draft_input = Some(input),
      Field::Edit => self.review.edit_input = Some(input),
      Field::Note => self.review.note_input = Some(input),
      Field::Summary => self.review.summary_input = Some(input),
      Field::Suggestion => self.review.suggestion_input = Some(input),
    }
    Some(state)
  }

  /// Builds the boxes that are open, so render can draw them.
  pub(super) fn ensure_review_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    for field in [
      Field::Draft,
      Field::Edit,
      Field::Note,
      Field::Summary,
      Field::Suggestion,
    ] {
      self.input_for(field, window, cx);
    }
    if self.review.suggestion_text.is_none() {
      self.review.suggestion_input = None;
    }
    if !self.review.finish_open {
      self.review.summary_input = None;
    }
  }

  // Drawing.

  /// [`is_outdated`] for `comment` against `hunks`, worked out once per
  /// layout: it reads the hunk's lines, and every row it could sit under
  /// asks.
  pub(super) fn is_outdated_in(&self, comment: &LineComment, hunks: &[JjDiffHunk]) -> bool {
    let known = (self.layout_gen, comment.start_line, comment.end_line);
    let cached = self.review.outdated.borrow().get(&comment.id).copied();
    if let Some((generation, start, end, outdated)) = cached {
      if (generation, start, end) == known {
        return outdated;
      }
    }
    let outdated = is_outdated(comment, Some(hunks));
    self
      .review
      .outdated
      .borrow_mut()
      .insert(comment.id.clone(), (known.0, known.1, known.2, outdated));
    outdated
  }

  /// `HunkDiffLine`'s row.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  pub(super) fn diff_line(
    &self,
    path: &str,
    hunks: &[JjDiffHunk],
    h: usize,
    l: usize,
    numbers: (Option<u32>, Option<u32>),
    changed: &[Range<usize>],
    language: Option<&'static str>,
    conflicted: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    let hunk = &hunks[h];
    let line = &hunk.lines[l];
    let (old, new) = numbers;
    let selected = self
      .review
      .selection
      .as_ref()
      .is_some_and(|s| s.path == path && s.lines.contains(&(h, l)));
    let head = self
      .review
      .selection
      .as_ref()
      .is_some_and(|s| s.path == path && s.head == (h, l));
    let kind = line_kind(line);
    let has_comment = line_has_comment(
      &self.review.comments,
      path,
      hunk,
      number_and_side(numbers, l),
      &|c| self.is_outdated_in(c, hunks),
    );
    let bg = row_fill(selected, has_comment, kind, p);
    // The characters this line changed against its pair, a shade deeper.
    let mark = match kind {
      LineKind::Removed => Tone::GitClosed.color(p),
      _ => Tone::GitOpen.color(p),
    }
    .opacity(0.45);
    // The text selection first: where it overlaps a changed run, it shows.
    let picked = self
      .review
      .text
      .as_ref()
      .filter(|t| t.path == path)
      .and_then(|t| selected_range(hunks, t.anchor, t.head, (h, l)))
      .filter(|r| !r.is_empty());
    let changed: Vec<(Range<usize>, Hsla)> = picked
      .map(|r| (r, p.selected()))
      .into_iter()
      .chain(changed.iter().map(|r| (r.clone(), mark)))
      .collect();
    let marker = match kind {
      LineKind::Added => "+",
      LineKind::Removed => "-",
      LineKind::Context => " ",
    };
    let multi = self
      .review
      .selection
      .as_ref()
      .is_some_and(|s| s.lines.len() > 1);
    let border = p.border.opacity(0.4);
    let dragging = self.review.selection.as_ref().is_some_and(|s| s.dragging);
    let text = line.get(1..).filter(|t| !t.is_empty()).unwrap_or(" ");
    let at = (h, l);
    let (press, drag, add) = (path.to_string(), path.to_string(), path.to_string());
    let (code_path, layout_path) = (path.to_string(), path.to_string());
    let layouts = self.review.layouts.clone();
    let head_drawn = head.then(|| (self.review.head_drawn.clone(), path.to_string()));
    div()
      .id(SharedString::from(format!("diff-line-{path}-{h}-{l}")))
      .group("diff-line")
      .relative()
      .flex()
      .when_some(head_drawn, |this, (drawn, path)| {
        this.child(
          gpui::canvas(
            move |bounds, _, _| {
              *drawn.borrow_mut() = Some((path.clone(), (h, l), bounds));
            },
            |_, _, _, _| {},
          )
          .absolute()
          .top_0()
          .left_0()
          .size_full(),
        )
      })
      .when_some(bg, |this, bg| this.bg(bg))
      .when(!conflicted, |this| {
        this.on_mouse_move(cx.listener(move |view, e: &MouseMoveEvent, _, cx| {
          view.drag_line(&drag, at, cx);
          view.drag_text(&drag, at, e.position, cx);
        }))
      })
      .child(
        div()
          .id(SharedString::from(format!("gutter-{path}-{h}-{l}")))
          .debug_selector({
            let id = format!("gutter-{path}-{h}-{l}");
            move || id
          })
          .w_24()
          .flex_none()
          .flex()
          .items_center()
          .gap_1()
          .border_r_1()
          .border_color(border)
          .text_color(p.muted_foreground)
          .cursor_pointer()
          .when(gutter_hovers(selected, dragging), |this| {
            this.hover(move |s| s.bg(p.hover()))
          })
          .when(!conflicted, |this| {
            this.on_mouse_down(
              MouseButton::Left,
              cx.listener(move |view, e: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&view.focus, cx);
                if e.modifiers.shift {
                  view.extend_line(&press, at, cx);
                } else {
                  view.press_line(&press, at, cx);
                }
              }),
            )
          })
          .child(
            div()
              .w_10()
              .mr_1()
              .flex()
              .justify_end()
              .child(old.map(|n| n.to_string()).unwrap_or_default()),
          )
          .child(
            div()
              .debug_selector({
                let id = format!("line-new-{path}-{h}-{l}");
                move || id
              })
              .w_10()
              .flex()
              .justify_end()
              .child(new.map(|n| n.to_string()).unwrap_or_default()),
          ),
      )
      .child(
        div()
          .w_6()
          .flex_none()
          .flex()
          .items_center()
          .justify_center()
          .when(!conflicted, |this| {
            this.child(
              self.tip(
                format!("add-comment-tip-{path}-{h}-{l}"),
                if multi {
                  "Add comment to selected lines"
                } else {
                  "Add comment"
                },
                div()
                  .id(SharedString::from(format!("add-comment-{path}-{h}-{l}")))
                  .debug_selector({
                    let id = format!("add-comment-{path}-{h}-{l}");
                    move || id
                  })
                  .role(gpui::Role::Button)
                  .aria_label(format!(
                    "Add comment on {path} line {}",
                    new.or(old).unwrap_or_default()
                  ))
                  .test_support()
                  .p_0p5()
                  .rounded(radius::SM)
                  .bg(p.primary)
                  .cursor_pointer()
                  .opacity(0.)
                  .group_hover("diff-line", |s| s.opacity(1.))
                  .child(ui::icon(theme::ICON_PLUS, 12., p.primary_foreground))
                  .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                  .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.add_line_comment(&add, at, cx);
                  })),
                cx,
              ),
            )
          }),
      )
      .child(
        div()
          .w_5()
          .flex_none()
          .flex()
          .justify_center()
          .child(marker),
      )
      .child(
        div()
          .id(SharedString::from(format!("code-{path}-{h}-{l}")))
          .debug_selector({
            let id = format!("code-{path}-{h}-{l}");
            move || id
          })
          .flex_1()
          .min_w_0()
          .px_2()
          .py_0p5()
          .cursor_text()
          .when(!conflicted, |this| {
            this.on_mouse_down(
              MouseButton::Left,
              cx.listener(move |view, e: &MouseDownEvent, window, cx| {
                window.focus(&view.focus, cx);
                view.press_text(&code_path, at, e.position, e.click_count, cx);
              }),
            )
          })
          .child(self.marked_text(
            super::search::MatchKey::Line(path.to_string(), h, l),
            text,
            language,
            &changed,
            Some(Rc::new(move |layout| {
              layouts
                .borrow_mut()
                .insert((layout_path.clone(), at), layout);
            })),
          )),
      )
  }

  /// What follows a diff line: its comments and the comment box.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  pub(super) fn line_extras(
    &self,
    path: &str,
    hunks: &[JjDiffHunk],
    h: usize,
    l: usize,
    numbers: (Option<u32>, Option<u32>),
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Vec<gpui::AnyElement> {
    let mut out = Vec::new();
    let under = comments_under(
      &self.review.comments,
      path,
      &hunks[h],
      number_and_side(numbers, l),
      &|c| self.is_outdated_in(c, hunks),
    );
    if !under.is_empty() {
      let mut list = div()
        .bg(comment_strip_fill(p))
        .border_y_1()
        .border_color(p.border.opacity(0.4))
        .px_4()
        .py_2()
        .flex()
        .flex_col()
        .gap_2()
        .font_family(theme::SANS_FONT);
      for c in under {
        list = list.child(self.comment_card(&c.id, &c.text, "inline-comment-card", p, cx));
      }
      out.push(list.into_any_element());
    }
    out.extend(self.line_threads(path, hunks, h, l, numbers, p, cx));
    let here = self
      .review
      .draft
      .as_ref()
      .is_some_and(|d| d.file_path == path && d.at == Some((h, l)) && d.hunk_id == hunks[h].id);
    if here {
      out.push(self.comment_input(p, cx).into_any_element());
    }
    out
  }

  /// A comment, or its edit box (`inline-comment-card`, `file-comment-card`).
  fn comment_card(
    &self,
    id: &str,
    text: &str,
    selector: &'static str,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::AnyElement {
    // Comment text reads at the diff's code size, so it follows zoom with it.
    let box_ = || {
      ui::text(div(), TextSize::Code)
        .rounded(radius::MD)
        .p_3()
        .border_1()
        .border_color(p.border.opacity(0.6))
    };
    if self.review.editing.as_deref() == Some(id) {
      let discard = id.to_string();
      return box_()
        .bg(p.background)
        .child(self.edit_box(
          p,
          cx.listener(move |view, _, _, cx| {
            if view.review.notes.iter().any(|n| n.conflict_id == discard) {
              view.clear_note(&discard, cx);
            } else {
              view.delete_comment(&discard, cx);
            }
          }),
          cx,
        ))
        .into_any_element();
    }
    let (edit, delete, body) = (id.to_string(), id.to_string(), text.to_string());
    let muted = p.muted_foreground;
    let card = box_()
      .id(SharedString::from(format!("comment-{id}")))
      .debug_selector(move || selector.into())
      .role(gpui::Role::Button)
      .aria_label("Edit comment")
      .test_support()
      .cursor_pointer()
      .hover(|s| s.bg(p.hover()))
      .on_click(cx.listener(move |view, _, _, cx| view.start_edit(&edit, &body, cx)))
      .child(
        div()
          .flex()
          .items_start()
          .gap_2()
          .child(self.comment_body(id, text, p, cx))
          .child(
            self.tip(
              format!("delete-comment-tip-{id}"),
              "Delete comment",
              div()
                .id(SharedString::from(format!("delete-comment-{id}")))
                .debug_selector(|| "delete-comment".into())
                .flex_none()
                .p_1()
                .rounded(radius::SM)
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover()))
                .child(ui::icon(theme::ICON_X, 12., muted))
                .on_click(cx.listener(move |view, _, _, cx| {
                  cx.stop_propagation();
                  if view.review.notes.iter().any(|n| n.conflict_id == delete) {
                    view.clear_note(&delete, cx);
                  } else {
                    view.delete_comment(&delete, cx);
                  }
                })),
              cx,
            ),
          ),
      );
    self
      .tip(format!("edit-comment-tip-{id}"), "Click to edit", card, cx)
      .into_any_element()
  }

  /// A comment's text, with its suggestion drawn as a diff against the
  /// lines it replaces and an Apply button.
  fn comment_body(&self, id: &str, text: &str, p: &Palette, cx: &mut Context<Self>) -> Div {
    let comment = self
      .review
      .comments
      .iter()
      .find(|c| c.id == id)
      .filter(|c| !c.is_file() && c.line_side != Some(Side::Old));
    let Some((comment, suggestion)) = comment.zip(parse_suggestion(text)) else {
      return div().flex_1().whitespace_normal().child(text.to_string());
    };
    let original = commented_text(&comment.line_content).join("\n");
    let apply = id.to_string();
    div()
      .flex_1()
      .min_w_0()
      .flex()
      .flex_col()
      .gap_2()
      .when(!suggestion.before.is_empty(), |this| {
        this.child(div().whitespace_normal().child(suggestion.before.clone()))
      })
      .child(threads::suggestion_box(
        Some(&original),
        &suggestion.suggested,
        p,
      ))
      .when(!suggestion.after.is_empty(), |this| {
        this.child(div().whitespace_normal().child(suggestion.after.clone()))
      })
      .when(!self.read_only(), |this| {
        this.child(
          div().flex().justify_end().child(
            Button::new("apply-suggestion", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Apply suggestion")
              .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation();
                view.apply_suggestion(&apply, cx);
              })),
          ),
        )
      })
  }

  /// `CommentEditInput`: Discard, Cancel, Save.
  fn edit_box(
    &self,
    p: &Palette,
    on_discard: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let empty = self.review.edit_text.trim().is_empty();
    div()
      .debug_selector(|| "comment-edit".into())
      .flex()
      .flex_col()
      .gap_2()
      .font_family(theme::SANS_FONT)
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        if ui::is_escape(k) {
          cx.stop_propagation();
          view.cancel_edit(cx);
        } else if k.key == "enter"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
        {
          cx.stop_propagation();
          view.save_edit(cx);
        }
      }))
      .children(
        self
          .review
          .edit_input
          .as_ref()
          .map(|i| ui::text(Textarea::new(&i.state), TextSize::Code)),
      )
      .child(
        div()
          .flex()
          .justify_between()
          .gap_2()
          .child(
            Button::new("comment-discard", p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .text_color(p.destructive)
              .label("Discard")
              .on_click(on_discard),
          )
          .child(
            div()
              .flex()
              .gap_2()
              .child(
                Button::new("comment-edit-cancel", p)
                  .variant(Variant::Ghost)
                  .size(Size::Sm)
                  .label("Cancel")
                  .on_click(cx.listener(|view, _, _, cx| view.cancel_edit(cx))),
              )
              .child(
                Button::new("comment-save", p)
                  .size(Size::Sm)
                  .disabled(empty)
                  .label("Save")
                  .on_click(cx.listener(|view, _, _, cx| view.save_edit(cx))),
              ),
          ),
      )
  }

  /// `CommentInput`.
  pub(super) fn comment_input(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let suggesting = self.review.suggestion_text.is_some();
    let empty = self.review.draft_text.trim().is_empty() && !suggesting;
    let label = self.review.draft.as_ref().and_then(|d| d.label());
    let can_suggest = self
      .review
      .draft
      .as_ref()
      .is_some_and(|d| suggestion_lines(d).is_some());
    ui::text(div(), TextSize::Code)
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
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        if ui::is_escape(k) {
          cx.stop_propagation();
          view.cancel_draft(cx);
        } else if k.key == "enter"
          && (crate::gpui_shell::keys::held(k.modifiers).platform
            || crate::gpui_shell::keys::held(k.modifiers).control)
        {
          cx.stop_propagation();
          view.submit_draft(cx);
        }
      }))
      .children(label.map(|label| div().mb_2().text_color(p.muted_foreground).child(label)))
      .children(self.review.draft.as_ref().and_then(|d| {
        let quote = d.quote.as_ref()?;
        Some(
          div()
            .mb_2()
            .border_l_2()
            .border_color(Tone::Primary.border(p))
            .pl_3()
            .italic()
            .text_color(p.muted_foreground)
            .child(
              ui::text(div(), TextSize::Xs)
                .font_weight(FontWeight::MEDIUM)
                .not_italic()
                .mb_0p5()
                .child(format!("Quoting @{}", quote.author)),
            )
            .child(d.line_content.join(" ")),
        )
      }))
      .children(self.review.draft_input.as_ref().map(|i| {
        div()
          .mb_2()
          .child(ui::text(Textarea::new(&i.state), TextSize::Code))
      }))
      .children(self.suggestion_editor(p))
      .child(
        div()
          .flex()
          .justify_end()
          .gap_2()
          .when(can_suggest, |this| {
            this
              .child(
                Button::new("suggest-change", p)
                  .variant(Variant::Ghost)
                  .icon(theme::ICON_FILE_DIFF, 14.)
                  .label(if suggesting {
                    "Remove suggestion"
                  } else {
                    "Suggest change"
                  })
                  .on_click(cx.listener(|view, _, window, cx| view.suggest_change(window, cx))),
              )
              .child(div().flex_1())
          })
          .child(
            Button::new("comment-cancel", p)
              .variant(Variant::Ghost)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.cancel_draft(cx))),
          )
          .child(
            Button::new("comment-submit", p)
              .disabled(empty)
              .label("Add comment")
              .on_click(cx.listener(|view, _, _, cx| view.submit_draft(cx))),
          ),
      )
  }

  /// "Suggest change"'s editor: the lines in a code field, and below it a
  /// preview of what changes against the lines as they are.
  fn suggestion_editor(&self, p: &Palette) -> Option<impl IntoElement> {
    let input = self.review.suggestion_input.as_ref()?;
    let suggested = self.review.suggestion_text.as_deref()?;
    let original = commented_text(&self.review.draft.as_ref()?.line_content);
    let line = |marker: &'static str, text: String, tone: Tone| {
      div()
        .flex()
        .gap_2()
        .px_2()
        .bg(tone.tint(p))
        .whitespace_nowrap()
        .child(div().flex_none().text_color(tone.text(p)).child(marker))
        .child(text)
    };
    let preview = div()
      .debug_selector(|| "suggestion-preview".into())
      .rounded(radius::SM)
      .border_1()
      .border_color(p.border)
      .overflow_hidden()
      .font_family(theme::MONO_FONT)
      .children(
        original
          .into_iter()
          .map(|l| line("-", l, Tone::Danger))
          .chain(
            suggested
              .split('\n')
              .map(|l| line("+", l.to_string(), Tone::Success)),
          ),
      );
    Some(
      div()
        .debug_selector(|| "suggestion-editor".into())
        .mb_2()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
          ui::text(div(), TextSize::Xs)
            .text_color(p.muted_foreground)
            .child("Suggested change"),
        )
        .child(
          div()
            .rounded(radius::SM)
            .border_1()
            .border_color(p.border)
            .bg(p.background)
            .font_family(theme::MONO_FONT)
            .child(ui::text(Textarea::new(&input.state), TextSize::Code)),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .text_color(p.muted_foreground)
            .child("Preview"),
        )
        .child(ui::text(preview, TextSize::Code)),
    )
  }

  /// Whether `path` shows its file comments section.
  pub(super) fn file_comment_activity(&self, path: &str) -> bool {
    self
      .review
      .comments
      .iter()
      .any(|c| c.file_path == path && c.is_file())
      || self
        .review
        .draft
        .as_ref()
        .is_some_and(|d| d.file_path == path && d.hunk_id == FILE_HUNK)
  }

  /// `FileCommentSection`.
  pub(super) fn file_comments(
    &self,
    path: &str,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    if !self.file_comment_activity(path) {
      return None;
    }
    let mut section = div()
      .border_b_1()
      .border_color(p.border)
      .bg(p.muted.opacity(0.2))
      .px_4()
      .py_3()
      .flex()
      .flex_col()
      .gap_3()
      .font_family(theme::SANS_FONT);
    for c in self
      .review
      .comments
      .iter()
      .filter(|c| c.file_path == path && c.is_file())
    {
      section = section.child(self.comment_card(&c.id, &c.text, "file-comment-card", p, cx));
    }
    if self
      .review
      .draft
      .as_ref()
      .is_some_and(|d| d.file_path == path && d.hunk_id == FILE_HUNK)
    {
      section = section.child(self.comment_input(p, cx));
    }
    Some(section)
  }

  /// The amber "Outdated" block at the top of a file's diff.
  pub(super) fn outdated_comments(
    &self,
    path: &str,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    let outdated = self.outdated(path);
    if outdated.is_empty() {
      return None;
    }
    let tone = Tone::Warning;
    let mut section = div()
      .debug_selector(|| "outdated-comments".into())
      .border_b_1()
      .border_color(tone.border(p))
      .bg(tone.tint(p))
      .px_4()
      .py_3()
      .flex()
      .flex_col()
      .gap_3();
    for c in outdated {
      let id = c.id.clone();
      section = section.child(
        div()
          .bg(p.background)
          .rounded(radius::MD)
          .p_3()
          .border_1()
          .border_color(tone.border(p))
          .child(
            div()
              .flex()
              .items_center()
              .justify_between()
              .mb_2()
              .font_family(theme::SANS_FONT)
              .child(
                div()
                  .flex()
                  .items_center()
                  .gap_2()
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .px_1p5()
                      .py_0p5()
                      .rounded(radius::SM)
                      .bg(tone.tint(p))
                      .text_color(tone.text(p))
                      .font_weight(FontWeight::MEDIUM)
                      .child("Outdated"),
                  )
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .text_color(p.muted_foreground)
                      .child(outdated_label(c)),
                  ),
              )
              .child(
                div()
                  .id(SharedString::from(format!("delete-outdated-{id}")))
                  .p_1()
                  .rounded(radius::SM)
                  .cursor_pointer()
                  .hover(move |s| s.bg(p.hover()))
                  .child(ui::icon(theme::ICON_X, 12., p.muted_foreground))
                  .on_click(cx.listener(move |view, _, _, cx| view.delete_comment(&id, cx))),
              ),
          )
          .when(!c.line_content.is_empty(), |this| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .bg(p.muted.opacity(0.6))
                .rounded(radius::SM)
                .px_2()
                .py_1()
                .mb_2()
                .font_family(theme::MONO_FONT)
                .children(c.line_content.iter().map(|l| div().child(l.clone()))),
            )
          })
          .child(div().font_family(theme::SANS_FONT).child(c.text.clone())),
      );
    }
    Some(section)
  }

  /// The conflict card's resolution note and its box.
  pub(super) fn note_section(
    &self,
    conflict: &Conflict,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Vec<gpui::AnyElement> {
    let mut out = Vec::new();
    let id = conflict.id.clone();
    out.push(
      div()
        .mt_2()
        .flex()
        .justify_end()
        .child(
          Button::new(SharedString::from(format!("conflict-comment-{id}")), p)
            .variant(Variant::Secondary)
            .size(Size::Sm)
            .label("Add comment")
            .on_click(cx.listener(move |view, _, _, cx| view.toggle_note(&id, cx))),
        )
        .into_any_element(),
    );
    if let Some(note) = self
      .review
      .notes
      .iter()
      .find(|n| n.conflict_id == conflict.id)
    {
      out.push(
        div()
          .px_3()
          .py_2()
          .bg(p.muted.opacity(0.4))
          .border_t_1()
          .border_color(p.border)
          .font_family(theme::SANS_FONT)
          .child(
            div()
              .text_color(p.muted_foreground)
              .mb_1()
              .child("Resolution note:"),
          )
          .child(self.comment_card(&note.conflict_id, &note.text, "conflict-note-card", p, cx))
          .into_any_element(),
      );
    }
    if self.review.note_open.as_deref() == Some(conflict.id.as_str()) {
      let empty = self.review.note_text.trim().is_empty();
      let path = conflict
        .id
        .rsplit_once("-conflict-")
        .map_or(conflict.id.as_str(), |(p, _)| p);
      let region = &conflict.region;
      let label = if region.start_line == region.end_line {
        format!("{path}:L{}", region.start_line)
      } else {
        format!("{path}:L{}-{}", region.start_line, region.end_line)
      };
      let (save, clear) = (conflict.clone(), conflict.id.clone());
      out.push(
        div()
          .debug_selector(|| "conflict-comment-input".into())
          .bg(p.muted.opacity(0.6))
          .border_y_1()
          .border_color(p.border.opacity(0.4))
          .px_4()
          .py_3()
          .font_family(theme::SANS_FONT)
          .child(div().mb_2().text_color(p.muted_foreground).child(label))
          .children(self.review.note_input.as_ref().map(|i| {
            div()
              .mb_2()
              .child(ui::text(Textarea::new(&i.state), TextSize::Code))
          }))
          .child(
            div()
              .flex()
              .justify_end()
              .gap_2()
              .child(
                Button::new("conflict-comment-cancel", p)
                  .variant(Variant::Ghost)
                  .size(Size::Sm)
                  .label("Cancel")
                  .on_click(cx.listener(move |view, _, _, cx| view.clear_note(&clear, cx))),
              )
              .child(
                Button::new("conflict-comment-save", p)
                  .size(Size::Sm)
                  .disabled(empty)
                  .label("Add comment")
                  .on_click(cx.listener(move |view, _, _, cx| view.save_note(&save, cx))),
              ),
          )
          .into_any_element(),
      );
    }
    out
  }

  /// `ReviewActionBar` B: the pending count, Start Review, Discard and
  /// Finish review.
  pub(super) fn review_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let conflicts = !self.conflicted().is_empty();
    let total = self.review.comments.len() + self.review.notes.len();
    let send_bg = if conflicts { p.destructive } else { p.primary };
    div()
      .debug_selector(|| "review-action-bar".into())
      .flex()
      .items_center()
      .justify_between()
      .px_4()
      .py_2()
      .bg(p.muted.opacity(0.8))
      .border_b_1()
      .border_color(p.border)
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::icon(
            theme::ICON_MESSAGE,
            16.,
            if conflicts { p.destructive } else { p.primary },
          ))
          .child(
            div()
              .text_color(p.muted_foreground)
              .child(format!("{} pending", count_label(total, "comment"))),
          )
          .children(self.agent_count(p)),
      )
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(self.start_review_button(p, cx))
          .child(
            Button::new("discard-review", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Discard pending comments…")
              .on_click(cx.listener(|view, _, _, cx| {
                view.review.discard_dialog = true;
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
                  .bg(send_bg)
                  .icon(theme::ICON_SEND, 12.)
                  .label(if conflicts {
                    "Resolve conflicts…"
                  } else {
                    "Finish review"
                  })
                  .on_click(cx.listener(|view, _, _, cx| {
                    view.review.finish_open = !view.review.finish_open;
                    cx.notify();
                  })),
              )
              .child(self.anchor("finish-review".into())),
          ),
      )
  }

  pub(super) fn start_review_button(&self, p: &Palette, cx: &mut Context<Self>) -> Button {
    Button::new("start-agent-review", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .gap_2()
      .disabled(self.starting_review)
      .icon(theme::ICON_BOT, 12.)
      .label("Start review")
      .on_click(cx.listener(|view, _, _, cx| view.start_review(cx)))
  }

  /// `ReviewActionBar` D.
  pub(super) fn stale_banner(
    &self,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    if self.review.stale.is_empty() {
      return None;
    }
    let tone = Tone::Warning;
    let n = self.review.stale.len();
    let outdated = self
      .review
      .comments
      .iter()
      .filter(|c| {
        is_outdated(
          c,
          self
            .file_hunks(&c.file_path)
            .filter(|d| !d.loading)
            .as_ref()
            .map(|d| d.hunks.as_slice()),
        )
      })
      .count();
    let fg = tone.text(p);
    let button = |id: &'static str, icon: &'static str, label: String| {
      Button::new(id, p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .gap_1p5()
        .border_color(tone.border(p))
        .text_color(fg)
        .icon(icon, 12.)
        .label(label)
    };
    Some(
      div()
        .debug_selector(|| "stale-banner".into())
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .py_2()
        .bg(tone.tint(p))
        .border_b_1()
        .border_color(tone.border(p))
        .child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .child(ui::icon(theme::ICON_TRIANGLE_ALERT, 16., tone.text(p)))
            .child(div().text_color(fg).child(format!(
              "{} changed since you started reviewing",
              count_label(n, "file")
            )))
            .child(
              div()
                .text_color(tone.text(p).opacity(0.8))
                .child(stale_names(&self.review.stale)),
            ),
        )
        .child(
          div()
            .flex()
            .items_center()
            .gap_2()
            .when(outdated > 0, |this| {
              this.child(ui::copy_feedback::copyable("copy-outdated-feedback", {
                let (p, copy) = (*p, cx.listener(|view, _, _, cx| view.copy_outdated(cx)));
                move |copied| {
                  Button::new("copy-outdated", &p)
                    .variant(Variant::Outline)
                    .size(Size::Sm)
                    .gap_1p5()
                    .border_color(tone.border(&p))
                    .text_color(fg)
                    .copy_icon(copied, 12.)
                    .label(format!("Copy Outdated ({outdated})"))
                    .on_click(copy)
                    .into_any_element()
                }
              }))
            })
            .child(
              button("reload-stale", theme::ICON_REFRESH, "Reload".into())
                .on_click(cx.listener(|view, _, _, cx| view.reload_stale(cx))),
            ),
        ),
    )
  }

  /// The Finish review popover, under its button.
  pub(super) fn finish_popover(
    &self,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    if !self.review.finish_open {
      return None;
    }
    let anchor = self.anchors.borrow().get("finish-review").copied()?;
    let conflicts = !self.conflicted().is_empty();
    let total = self.review.comments.len() + self.review.notes.len();
    let sending = self.review.sending;
    let width = 320.;
    let at = point(anchor.right() - px(width), anchor.bottom() + px(4.));
    let checkbox = {
      let checked = self.review.use_idle;
      div()
        .id("use-idle-agent")
        .debug_selector(|| "use-idle-agent".into())
        .flex()
        .items_center()
        .gap_1p5()
        .cursor_pointer()
        .child(
          div()
            .size(zpx(14.))
            .rounded(radius::SM)
            .border_1()
            .border_color(if checked { p.primary } else { p.border })
            .bg(if checked { p.primary } else { p.background })
            .flex()
            .items_center()
            .justify_center()
            .when(checked, |this| {
              this.child(ui::icon(theme::ICON_CHECK, 10., p.primary_foreground))
            }),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .text_color(p.muted_foreground)
            .child("Send to idle agent if one is available"),
        )
        .on_click(cx.listener(|view, _, _, cx| {
          view.review.use_idle = !view.review.use_idle;
          cx.notify();
        }))
    };
    let panel = div()
      .id("finish-review-popover")
      .debug_selector(|| "finish-review-popover".into())
      .w(zpx(width))
      .relative()
      .p_4()
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_md()
      .occlude()
      .on_mouse_down_out(cx.listener(|view, _, _, cx| {
        view.review.finish_open = false;
        cx.notify();
      }))
      .child(
        div().absolute().top_2().right_2().child(
          Button::new("finish-review-close", p)
            .aria_label("Close")
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .px_1()
            .disabled(sending)
            .icon(theme::ICON_X, 16.)
            .on_click(cx.listener(|view, _, _, cx| {
              view.review.finish_open = false;
              cx.notify();
            })),
        ),
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_3()
          .child(
            div()
              .child(
                div()
                  .font_weight(FontWeight::MEDIUM)
                  .mb_1()
                  .child(if conflicts {
                    "Resolve conflicts"
                  } else {
                    "Finish your review"
                  }),
              )
              .child(div().text_color(p.muted_foreground).child(format!(
                "{} will be submitted.",
                count_label(total, "comment")
              ))),
          )
          .children(
            self
              .review
              .summary_input
              .as_ref()
              .map(|i| Textarea::new(&i.state)),
          )
          .child(checkbox)
          .child(
            div()
              .flex()
              .justify_between()
              .gap_2()
              .child(
                ui::copy_feedback::copyable("copy-review-feedback", {
                  let (p, copy) = (*p, cx.listener(|view, _, _, cx| view.copy_review(cx)));
                  move |copied| {
                    Button::new("copy-review", &p)
                      .variant(Variant::Outline)
                      .size(Size::Sm)
                      .gap_2()
                      .disabled(sending)
                      .copy_icon(copied, 12.)
                      .label(if copied.copied { "Copied" } else { "Copy" })
                      .on_click(copy)
                      .into_any_element()
                  }
                })
                .disabled(sending),
              )
              .child(
                div()
                  .flex()
                  .gap_2()
                  .child(
                    Button::new("review-plan", p)
                      .variant(Variant::Secondary)
                      .size(Size::Sm)
                      .disabled(sending)
                      .label("Plan")
                      .on_click(cx.listener(|view, _, _, cx| view.send_review(true, cx))),
                  )
                  .child(
                    Button::new("review-edit", p)
                      .size(Size::Sm)
                      .disabled(sending)
                      .label("Edit")
                      .on_click(cx.listener(|view, _, _, cx| view.send_review(false, cx))),
                  ),
              ),
          ),
      );
    Some(deferred(anchored().position(at).child(panel)).with_priority(1))
  }

  /// `ReviewActionBar` C: "Discard review?".
  pub(super) fn review_dialog(
    &self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    if !self.review.discard_dialog {
      return None;
    }
    let n = self.review.comments.len();
    let close = |view: &mut Self, cx: &mut Context<Self>| {
      view.review.discard_dialog = false;
      cx.notify();
    };
    Some(
      ui::Dialog::new("discard-review-dialog", p)
        .overlay(window.viewport_size())
        .width(512.)
        .title(format!(
          "Discard {n} pending {}?",
          if n == 1 { "comment" } else { "comments" }
        ))
        .description("They are deleted from this machine. This can't be undone.")
        .on_close(ui::handler(cx, move |view, _, cx| close(view, cx)))
        .on_primary(ui::handler(cx, |view, _, cx| view.discard_review(cx)))
        .footer([
          Button::new("keep-reviewing", p)
            .variant(Variant::Outline)
            .label("Cancel")
            .on_click(cx.listener(move |view, _, _, cx| close(view, cx)))
            .into_any_element(),
          Button::new("confirm-discard-review", p)
            .variant(Variant::Destructive)
            .label("Discard comments")
            .on_click(cx.listener(|view, _, _, cx| view.discard_review(cx)))
            .into_any_element(),
        ]),
    )
  }
}

/// A diff row's fill. Selection wins, so a multi-line selection reads as one
/// band; then a comment's amber, marking its lines like GitHub; then the
/// add/remove tint.
pub(super) fn row_fill(
  selected: bool,
  commented: bool,
  kind: LineKind,
  p: &Palette,
) -> Option<Hsla> {
  if selected {
    return Some(p.selected());
  }
  if commented {
    return Some(p.commented());
  }
  match kind {
    LineKind::Added => Some(Tone::GitOpen.tint(p)),
    LineKind::Removed => Some(Tone::GitClosed.tint(p)),
    LineKind::Context => None,
  }
}

/// Behind the comments under a line: the same amber as the commented rows,
/// so the band runs on into the comments.
pub(super) fn comment_strip_fill(p: &Palette) -> Hsla {
  p.commented()
}

/// Whether the gutter shows its hover fill. The fill sits on the gutter cell,
/// above the row's own fill, so on a selected row (the drag head is always
/// under the cursor) it would cut a grey gap into the selection band.
pub(super) fn gutter_hovers(selected: bool, dragging: bool) -> bool {
  !selected && !dragging
}

#[cfg(test)]
mod row_paint_tests {
  use super::*;

  #[test]
  fn selection_beats_diff_tint_on_every_kind() {
    let p = theme::palette(true);
    for kind in [LineKind::Added, LineKind::Removed, LineKind::Context] {
      assert_eq!(row_fill(true, false, kind, &p), Some(p.selected()));
    }
  }

  #[test]
  fn unselected_rows_keep_their_diff_tint() {
    let p = theme::palette(true);
    assert_eq!(
      row_fill(false, false, LineKind::Added, &p),
      Some(Tone::GitOpen.tint(&p))
    );
    assert_eq!(
      row_fill(false, false, LineKind::Removed, &p),
      Some(Tone::GitClosed.tint(&p))
    );
    assert_eq!(row_fill(false, false, LineKind::Context, &p), None);
  }

  #[test]
  fn a_commented_row_is_tinted_over_its_diff_colour() {
    let p = theme::palette(true);
    for kind in [LineKind::Added, LineKind::Removed, LineKind::Context] {
      assert_eq!(
        row_fill(false, true, kind, &p),
        Some(Tone::Warning.tint(&p))
      );
    }
  }

  #[test]
  fn the_comment_strip_matches_its_commented_rows() {
    for dark in [true, false] {
      let p = theme::palette(dark);
      assert_eq!(
        Some(comment_strip_fill(&p)),
        row_fill(false, true, LineKind::Added, &p)
      );
    }
  }

  #[test]
  fn selection_beats_the_comment_tint() {
    let p = theme::palette(true);
    assert_eq!(
      row_fill(true, true, LineKind::Added, &p),
      Some(p.selected())
    );
  }

  #[test]
  fn gutter_hover_never_paints_over_a_selection_or_a_drag() {
    assert!(gutter_hovers(false, false));
    assert!(!gutter_hovers(true, false), "selected row under the cursor");
    assert!(!gutter_hovers(true, true), "drag head");
    assert!(!gutter_hovers(false, true), "rows crossed mid-drag");
  }
}
