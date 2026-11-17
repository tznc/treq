//! A virtual list's rows. A screen derives what each row stands for every
//! frame; [`Rows::sync`] tells the [`ListState`] which rows changed, so the
//! heights it measured for the others are kept, and a [`gpui::list`] lays
//! out only the rows on screen.

use tauri_plugin_gpui::gpui::{px, ListAlignment, ListState, Pixels, Window};

pub(in crate::gpui_shell) struct Rows<T> {
  pub(in crate::gpui_shell) list: ListState,
  rows: Vec<T>,
  /// The rem size the rows were measured at.
  rem: Pixels,
}

impl<T> Default for Rows<T> {
  fn default() -> Self {
    Self {
      // Draws a screen's worth above and below, so a scroll rarely meets
      // an unmeasured row.
      list: ListState::new(0, ListAlignment::Top, px(800.)),
      rows: Vec::new(),
      rem: px(0.),
    }
  }
}

impl<T> Rows<T> {
  /// Takes the rows the screen shows now. `same` says whether a row still
  /// draws what it drew, so its height holds.
  pub(in crate::gpui_shell) fn sync(
    &mut self,
    window: &Window,
    rows: Vec<T>,
    same: impl Fn(&T, &T) -> bool,
  ) {
    let rem = window.rem_size();
    if rem != self.rem {
      self.rem = rem;
      self.list.remeasure();
    }
    let (n, m) = (self.rows.len(), rows.len());
    let prefix = self
      .rows
      .iter()
      .zip(&rows)
      .take_while(|(a, b)| same(a, b))
      .count();
    if prefix < n.max(m) {
      let suffix = self
        .rows
        .iter()
        .rev()
        .zip(rows.iter().rev())
        .take(n.min(m) - prefix)
        .take_while(|(a, b)| same(a, b))
        .count();
      self.list.splice(prefix..n - suffix, m - prefix - suffix);
    }
    self.rows = rows;
  }

  pub(in crate::gpui_shell) fn get(&self, ix: usize) -> Option<&T> {
    self.rows.get(ix)
  }

  pub(in crate::gpui_shell) fn len(&self) -> usize {
    self.rows.len()
  }
}

/// A long comment thread shows its first comment and its newest ones, with
/// the rest behind "Show N earlier comments".
const FOLDED_KEEP_FIRST: usize = 1;
const FOLDED_KEEP_LAST: usize = 3;
/// A fold hides at least this many comments, so its row never stands in for
/// a single comment.
const FOLD_MIN_HIDDEN: usize = 2;

/// The comments of a thread of `len` that show: `Some(ix)` is a comment and
/// `None` the fold's row, which stands for the comments between. A
/// `expanded` or short thread shows all. Folded comments are not built.
pub(super) fn thread(len: usize, expanded: bool) -> Vec<Option<usize>> {
  if expanded || len < FOLDED_KEEP_FIRST + FOLDED_KEEP_LAST + FOLD_MIN_HIDDEN {
    return (0..len).map(Some).collect();
  }
  (0..FOLDED_KEEP_FIRST)
    .map(Some)
    .chain([None])
    .chain((len - FOLDED_KEEP_LAST..len).map(Some))
    .collect()
}

/// How many comments the fold's row hides.
pub(super) fn hidden(len: usize) -> usize {
  len - FOLDED_KEEP_FIRST - FOLDED_KEEP_LAST
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_long_thread_folds_its_middle() {
    assert_eq!(thread(8, false), [Some(0), None, Some(5), Some(6), Some(7)]);
    assert_eq!(hidden(8), 4);
    assert_eq!(thread(8, true).len(), 8);
    // Folding one comment would only trade it for a row.
    assert_eq!(thread(5, false).len(), 5);
    assert_eq!(thread(6, false).len(), 5);
    assert!(thread(0, false).is_empty());
  }
}
