//! The Commits tab's find matches: the shared find bar
//! ([`crate::gpui_shell::screen::find`]) marks them in the commit messages
//! and the open diffs.

use std::ops::Range;

use gpui::Hsla;

use super::*;
use crate::gpui_shell::screen::find::{self, match_count, Find, Findable};

/// Where a match is: a commit's headline, or a line of its open diff.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Hit {
  Message(String),
  Line(String, String, LineRef),
}

/// The matches of the find bar's query in the list.
#[derive(Default)]
pub(super) struct Hits {
  /// Matches by place: the first's index overall, how many, and its row.
  index: HashMap<Hit, (usize, usize, usize)>,
  /// The rows or the query changed since `index` was built.
  pub(super) stale: bool,
  /// Scroll the current match's row into view.
  reveal: bool,
}

impl Findable for CommitsView {
  fn find(&mut self) -> &mut Find {
    &mut self.search
  }

  fn can_find(&self) -> bool {
    self.hidden_at.is_none()
  }

  /// Only open diffs are searched, and only the last few stay read.
  fn find_instantly(&self) -> bool {
    true
  }

  fn query_applied(&mut self, _cx: &mut Context<Self>) {
    self.hits.stale = true;
  }

  fn reveal_current(&mut self, _cx: &mut Context<Self>) {
    self.hits.reveal = true;
  }
}

impl CommitsView {
  /// Every match, in the list's order, once the rows or the query changed.
  pub(super) fn index_find(&mut self) {
    if !std::mem::take(&mut self.hits.stale) {
      return;
    }
    self.hits.index.clear();
    let query = self.search.applied.clone();
    if query.is_empty() {
      self.search.set_total(0);
      return;
    }
    let (mut index, mut total) = (HashMap::new(), 0);
    self.searchable(|row, hit, text| {
      let n = match_count(text, &query);
      if n > 0 {
        index.insert(hit, (total, n, row));
        total += n;
      }
    });
    self.hits.index = index;
    self.search.set_total(total);
  }

  /// Scrolls the current match's row to the top once stepped to.
  pub(super) fn reveal_find(&mut self) {
    if !std::mem::take(&mut self.hits.reveal) {
      return;
    }
    if let Some(&(_, _, row)) = self
      .hits
      .index
      .values()
      .find(|(first, n, _)| self.search.current_in(*first, *n).is_some())
    {
      self.scroll_to_row(row);
    }
  }

  /// `text`'s matches at `hit`, the current one stronger.
  pub(super) fn find_marks(&self, hit: &Hit, text: &str, p: &Palette) -> Vec<(Range<usize>, Hsla)> {
    match self.hits.index.get(hit) {
      Some(&(first, _, _)) => self.search.marks(text, first, p.primary),
      None => Vec::new(),
    }
  }

  /// The bar, top right of the list.
  pub(super) fn find_bar(
    &mut self,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    find::bar(self, "commits-search", p, cx)
  }
}
