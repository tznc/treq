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
