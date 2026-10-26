//! The diff from the keyboard: Up and Down move a line cursor through
//! every drawn file, Shift extends the selection within the file, Left
//! opens the comment box on it, Enter edits the comment under it and
//! Escape clears it.

use super::review::LineSelection;
use super::*;
use crate::gpui_shell::screen::{
  changes_review::{edge_line, pending_for_line, step_line},
  commits_data::selection,
};

/// How many frames a moved cursor waits to be drawn before it is scrolled to.
const REVEAL_FRAMES: u8 = 3;
/// Room kept above and below a revealed line.
const REVEAL_MARGIN: f32 = 48.;
/// How close to the diff's top or bottom a drag starts scrolling it.
pub(super) const AUTOSCROLL_EDGE: Pixels = px(32.);
/// The most a drag scrolls the diff per tick.
pub(super) const AUTOSCROLL_MAX: Pixels = px(64.);

/// How far a drag with the pointer at `y` scrolls the diff this tick: nothing
/// away from the edges, then more the further into or past an edge it is.
pub(super) fn autoscroll_step(y: Pixels, view: Bounds<Pixels>) -> Pixels {
  let top = view.top() + AUTOSCROLL_EDGE;
  let bottom = view.bottom() - AUTOSCROLL_EDGE;
  let past = if y < top {
    y - top
  } else if y > bottom {
    y - bottom
  } else {
    return px(0.);
  };
  past.clamp(-AUTOSCROLL_MAX, AUTOSCROLL_MAX)
}

impl ChangesView {
  /// The keys the diff takes while it has focus; `true` when handled.
  pub(super) fn diff_key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
    match key {
      "down" | "up" => self.move_line(key == "down", shift, cx),
      "left" if !shift => self.comment_selection(cx),
      "enter" if !shift => return self.edit_cursor_comment(cx),
      _ => return false,
    }
    true
  }

  /// The files whose diff lines are drawn, in the order they are drawn.
  fn line_files(&self) -> Vec<String> {
    let Some(snapshot) = self.snapshot.as_ref() else {
      return Vec::new();
    };
    let conflicted: HashSet<String> = self.conflicted().into_iter().collect();
    let committed = self.visible_committed();
    snapshot
      .files
      .iter()
      .filter(|f| !self.hides(&f.path))
      .map(|f| (f.path.as_str(), f.status.as_str(), f.old_path.is_some()))
      .chain(committed.iter().map(|f| {
        (
          f.path.as_str(),
          f.status.as_str(),
          f.previous_path.is_some(),
        )
      }))
      .filter(|(path, status, renamed)| {
        let data = self.file_hunks(path);
        !conflicted.contains(*path)
          && body(&BodyInput {
            path,
            status,
            renamed: *renamed,
            data: data.as_ref(),
            collapsed: self.collapsed_files.contains(*path),
            comment_activity: self.file_comment_activity(path),
            expanded_large: self.expanded_large.contains(*path),
            generated: self.is_generated(path),
            loaded_generated: self.loaded_generated.contains(*path),
          }) == Body::Hunks
      })
      .map(|(path, _, _)| path.to_string())
      .collect()
  }

  fn lines_of(&self, path: &str) -> Vec<JjDiffHunk> {
    self
      .file_hunks(path)
      .map(|d| d.hunks.to_vec())
      .unwrap_or_default()
  }

  /// Up or Down: the cursor moves a line, into the next file at a file's
  /// edge. With Shift the selection grows from its anchor instead, within
  /// its file, as comments take one file's lines.
  fn move_line(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
    let files = self.line_files();
    let current = self
      .review
      .selection
      .as_ref()
      .filter(|s| files.contains(&s.path))
      .map(|s| (s.path.clone(), s.anchor, s.head));
    let next = match &current {
      None => self
        .active
        .clone()
        .filter(|p| files.contains(p))
        .or_else(|| files.first().cloned())
        .and_then(|p| edge_line(&self.lines_of(&p), true).map(|at| (p, at))),
      Some((path, _, head)) => match step_line(&self.lines_of(path), *head, down) {
        Some(at) => Some((path.clone(), at)),
        None if extend => None,
        None => {
          let i = files.iter().position(|p| p == path).unwrap_or(0);
          let neighbour = if down {
            files.get(i + 1)
          } else {
            i.checked_sub(1).and_then(|i| files.get(i))
          };
          neighbour.and_then(|p| edge_line(&self.lines_of(p), down).map(|at| (p.clone(), at)))
        }
      },
    };
    let Some((path, head)) = next else {
      return;
    };
    let anchor = match current {
      Some((p, anchor, _)) if extend && p == path => anchor,
      _ => head,
    };
    let lines = selection(&self.lines_of(&path), anchor, head);
    self.active = Some(path.clone());
    self.review.selection = Some(LineSelection {
      path,
      anchor,
      head,
      lines,
      dragging: false,
    });
    self.review.reveal = REVEAL_FRAMES;
    cx.notify();
  }

  /// Left: the comment box on the selected lines, or on the cursor's line
  /// when the selection spans both sides.
  fn comment_selection(&mut self, cx: &mut Context<Self>) {
    let Some(s) = self.review.selection.as_ref() else {
      return;
    };
    let (path, head) = (s.path.clone(), s.head);
    let draft = self.selection_target().or_else(|| {
      self
        .file_hunks(&path)
        .and_then(|d| pending_for_line(&path, &d.hunks, head))
    });
    if let Some(draft) = draft {
      self.open_draft(draft, cx);
    }
  }

  /// Enter: edits the pending comment drawn under the cursor's line.
  fn edit_cursor_comment(&mut self, cx: &mut Context<Self>) -> bool {
    let Some(s) = self.review.selection.as_ref() else {
      return false;
    };
    let (path, head) = (s.path.clone(), s.head);
    let Some(d) = self.file_hunks(&path) else {
      return false;
    };
    let Some(hunk) = d.hunks.get(head.0) else {
      return false;
    };
    let under = crate::core::review::comments_under(
      &self.review.comments,
      &path,
      hunk,
      crate::core::review::line_at(hunk, head.1),
      &|c| self.is_outdated_in(c, &d.hunks),
    );
    let Some((id, text)) = under.first().map(|c| (c.id.clone(), c.text.clone())) else {
      return false;
    };
    self.start_edit(&id, &text, cx);
    true
  }

  /// Scrolls the cursor's line into view once it is drawn.
  pub(super) fn reveal_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.review.reveal == 0 {
      return;
    }
    let Some(s) = self.review.selection.as_ref() else {
      self.review.reveal = 0;
      return;
    };
    let drawn = self.review.head_drawn.borrow().clone();
    match drawn {
      Some((path, at, line)) if path == s.path && at == s.head => {
        let view = self.diff.list.viewport_bounds();
        let margin = px(REVEAL_MARGIN);
        let delta = if line.top() < view.top() + margin {
          line.top() - view.top() - margin
        } else if line.bottom() > view.bottom() - margin {
          line.bottom() - view.bottom() + margin
        } else {
          px(0.)
        };
        if delta != px(0.) {
          self.diff.list.scroll_by(delta);
          cx.notify();
        }
        self.review.reveal = 0;
      }
      _ => {
        // Off screen, the line's row is not drawn until scrolled to.
        if let Some(ix) = self.line_row(&s.path, s.head) {
          self.reveal_row(ix);
        }
        self.review.reveal -= 1;
        cx.on_next_frame(window, |_, _, cx| cx.notify());
      }
    }
  }
}

#[cfg(test)]
mod autoscroll_tests {
  use super::*;

  fn view() -> Bounds<Pixels> {
    Bounds::new(
      gpui::point(px(0.), px(100.)),
      gpui::size(px(500.), px(400.)),
    )
  }

  #[test]
  fn no_scroll_away_from_the_edges() {
    assert_eq!(autoscroll_step(px(300.), view()), px(0.));
    assert_eq!(autoscroll_step(px(100.) + AUTOSCROLL_EDGE, view()), px(0.));
  }

  #[test]
  fn scrolls_up_near_and_above_the_top_faster_the_further_out() {
    let near = autoscroll_step(px(110.), view());
    let out = autoscroll_step(px(60.), view());
    assert!(near < px(0.), "{near:?}");
    assert!(out < near, "{out:?} vs {near:?}");
    assert_eq!(autoscroll_step(px(-5000.), view()), -AUTOSCROLL_MAX);
  }

  #[test]
  fn the_edge_zone_alone_scrolls_a_line_or_more_per_tick() {
    // A window's pointer rarely gets past its edge, so the zone must move.
    let at_edge = view().bottom() - px(4.);
    assert!(autoscroll_step(at_edge, view()) >= px(24.));
  }

  #[test]
  fn scrolls_down_near_and_below_the_bottom() {
    assert!(autoscroll_step(px(490.), view()) > px(0.));
    assert_eq!(autoscroll_step(px(9000.), view()), AUTOSCROLL_MAX);
  }
}
