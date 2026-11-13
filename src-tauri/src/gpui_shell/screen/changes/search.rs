//! The Changes tab's find matches (`useDiffSearch`): the shared find bar
//! ([`crate::gpui_shell::screen::find`]) marks them in the diff.

use std::ops::Range;

use gpui::{AnyElement, StyledText};

use super::*;
use crate::gpui_shell::screen::find::{self, match_count, Find, Findable};

/// Diffs under this many lines are searched on every key: indexing one costs
/// well under a frame, so waiting for a pause only delays the highlights.
const INSTANT_LINES: usize = 10_000;

/// Where a match is: a diff line, or a line of a conflict region.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::gpui_shell::screen) enum MatchKey {
  Line(String, usize, usize),
  Conflict(String, usize),
}

/// A match's line and where it was drawn.
type Drawn = (MatchKey, Bounds<Pixels>);

/// The matches of the find bar's query in the diff.
#[derive(Default)]
pub(in crate::gpui_shell::screen) struct Matches {
  /// Matches by line: the first's index overall and how many.
  index: HashMap<MatchKey, (usize, usize)>,
  /// The query and the rows' revision `index` was built from: the rows are
  /// laid out from everything that decides the lines searched.
  indexed: Option<(String, u64)>,
  /// Scroll the current match into view once it is drawn.
  reveal: bool,
  /// Where the current match was last drawn.
  target: Rc<RefCell<Option<Drawn>>>,
}

/// A conflict region's lines as React searches them.
pub(in crate::gpui_shell::screen) fn conflict_texts(conflict: &Conflict) -> Vec<String> {
  if conflict.region.lines.is_empty() {
    conflict
      .region
      .content
      .split('\n')
      .map(str::to_string)
      .collect()
  } else {
    conflict
      .region
      .lines
      .iter()
      .map(|l| l.raw.clone())
      .collect()
  }
}

impl Findable for ChangesView {
  fn find(&mut self) -> &mut Find {
    &mut self.search
  }

  fn find_instantly(&self) -> bool {
    let lines: usize = self
      .hunks
      .values()
      .map(|data| data.hunks.iter().map(|h| h.lines.len()).sum::<usize>())
      .sum();
    lines < INSTANT_LINES
  }

  fn reveal_current(&mut self, _cx: &mut Context<Self>) {
    self.matches.reveal = true;
  }
}

impl ChangesView {
  /// `searchData`: every match, in React's order (conflicts, then the
  /// working copy's files, then the committed ones shown).
  pub(super) fn index_matches(&mut self) {
    if self.search.applied.is_empty() {
      self.matches.index.clear();
      self.matches.indexed = None;
      self.search.set_total(0);
      return;
    }
    let revision = self.diff.revision;
    if self
      .matches
      .indexed
      .as_ref()
      .is_some_and(|(q, r)| *q == self.search.applied && *r == revision)
    {
      return;
    }
    let query = self.search.applied.clone();
    let conflicted: HashSet<String> = self.conflicted().into_iter().collect();
    let mut files: Vec<(String, String, bool)> = self
      .listed()
      .iter()
      .map(|f| (f.path.clone(), f.status.clone(), f.old_path.is_some()))
      .collect();
    let working = files.len();
    files.extend(
      self
        .visible_committed()
        .into_iter()
        .map(|f| (f.path, f.status, f.previous_path.is_some())),
    );
    let bodies: Vec<Body> = files
      .iter()
      .map(|(path, status, renamed)| {
        body(&BodyInput {
          path,
          status,
          renamed: *renamed,
          data: self.shown_hunks(path),
          collapsed: self.collapsed_files.contains(path),
          comment_activity: self.file_comment_activity(path),
          expanded_large: self.expanded_large.contains(path),
          generated: self.is_generated(path),
          loaded_generated: self.loaded_generated.contains(path),
        })
      })
      .collect();
    self.matches.indexed = Some((query.clone(), revision));
    let mut total = 0;
    let mut index = HashMap::new();
    let mut add = |key: MatchKey, text: &str, total: &mut usize| {
      let n = match_count(text, &query);
      if n > 0 {
        index.insert(key, (*total, n));
        *total += n;
      }
    };
    for (path, _, _) in &files[..working] {
      if !conflicted.contains(path) {
        continue;
      }
      let hunks = self.hunks.get(path).map_or(&[][..], |d| &d.hunks);
      for conflict in conflicts(path, hunks) {
        for (i, text) in conflict_texts(&conflict).iter().enumerate() {
          add(MatchKey::Conflict(conflict.id.clone(), i), text, &mut total);
        }
      }
    }
    for ((path, _, _), body) in files.iter().zip(&bodies) {
      if *body != Body::Hunks {
        continue;
      }
      let data = self.shown_hunks(path);
      let hunks = data.map_or(&[][..], |d| &d.hunks);
      let in_region = conflict_lines(&conflicts(path, hunks));
      for (h, hunk) in hunks.iter().enumerate() {
        let numbers = line_numbers(hunk);
        for (l, line) in hunk.lines.iter().enumerate() {
          let new = numbers.get(l).and_then(|n| n.1);
          if new.is_some_and(|n| in_region.contains_key(&(n as usize))) {
            continue;
          }
          add(
            MatchKey::Line(path.clone(), h, l),
            line.get(1..).unwrap_or(""),
            &mut total,
          );
        }
      }
    }
    self.matches.index = index;
    self.search.set_total(total);
  }

  /// The current match's offset within `key`'s line, if it is there.
  fn current_in(&self, key: &MatchKey) -> Option<usize> {
    let (first, n) = *self.matches.index.get(key)?;
    self.search.current_in(first, n)
  }

  /// A line's text with its matches marked (`mark.search-match`).
  pub(super) fn marked_text(
    &self,
    key: MatchKey,
    text: &str,
    language: Option<&'static str>,
    changed: &[(Range<usize>, Hsla)],
    record: Option<RecordLayout>,
  ) -> AnyElement {
    let Some(&(first, _)) = self.matches.index.get(&key) else {
      let styled = highlight::styled_marked(text, language, self.dark, changed);
      return with_layout(styled, record).into_any_element();
    };
    let current = self.current_in(&key);
    let primary = theme::palette(self.dark).primary;
    // Matches first: where one overlaps a changed run, it shows.
    let marks: Vec<(Range<usize>, Hsla)> = self
      .search
      .marks(text, first, primary)
      .into_iter()
      .chain(changed.iter().cloned())
      .collect();
    let styled: StyledText = highlight::styled_marked(text, language, self.dark, &marks);
    if current.is_none() {
      return with_layout(styled, record).into_any_element();
    }
    let target = self.matches.target.clone();
    with_layout(styled, record)
      .child(
        gpui::canvas(
          move |bounds, _, _| {
            *target.borrow_mut() = Some((key.clone(), bounds));
          },
          |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
      )
      .into_any_element()
  }

  /// Scrolls the current match to the middle once it was drawn.
  pub(super) fn reveal_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.matches.reveal {
      return;
    }
    let Some(key) = self
      .matches
      .index
      .iter()
      .find(|(k, _)| self.current_in(k).is_some())
      .map(|(k, _)| k.clone())
    else {
      self.matches.reveal = false;
      return;
    };
    let drawn = self.matches.target.borrow().clone();
    match drawn {
      Some((k, line)) if k == key => {
        let view = self.diff.list.viewport_bounds();
        self.diff.list.scroll_by(line.center().y - view.center().y);
        self.matches.reveal = false;
        cx.notify();
      }
      _ => {
        // Off screen, the match's row is not drawn until scrolled to.
        let row = match &key {
          MatchKey::Line(path, h, l) => self.line_row(path, (*h, *l)),
          MatchKey::Conflict(id, _) => self.conflict_row(id),
        };
        if let Some(ix) = row {
          self.reveal_row(ix);
        }
        cx.on_next_frame(window, |_, _, cx| cx.notify())
      }
    }
  }

  /// `SearchOverlay`, top right of the diff.
  pub(super) fn search_bar(
    &mut self,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<impl IntoElement> {
    find::bar(self, "diff-search", p, cx)
  }
}

/// Takes a drawn line's text layout, once it is laid out and placed.
pub(super) type RecordLayout = Rc<dyn Fn(gpui::TextLayout)>;

/// `styled` in a box that hands its layout to `record` at prepaint, when
/// asking it where a point falls is safe.
fn with_layout(styled: StyledText, record: Option<RecordLayout>) -> Div {
  let layout = styled.layout().clone();
  div()
    .relative()
    .child(styled)
    .when_some(record, |this, record| {
      this.child(
        gpui::canvas(move |_, _, _| record(layout.clone()), |_, _, _, _| {})
          .absolute()
          .size_0(),
      )
    })
}

#[cfg(test)]
mod tests {
  use super::match_count;

  /// Round 7 measurement of the find bar's index pass: `match_count` over
  /// every diff line plus an entry per matching line, as `index_matches`
  /// does it. `cargo test --lib find_index_cost -- --ignored --nocapture`.
  #[test]
  #[ignore = "perf measurement"]
  fn find_index_cost() {
    for lines in [3_000usize, 20_000, 100_000] {
      let text: Vec<String> = (0..lines)
        .map(|i| {
          format!(
            "    let value_{i} = compute_{}(\"item {i}\", {}).map(|v| v + {}); // note {i}",
            i % 6,
            i * 3,
            i * 3
          )
        })
        .collect();
      for query in ["v", "value_1", "compute_3"] {
        let started = std::time::Instant::now();
        let mut index = std::collections::HashMap::new();
        let mut total = 0;
        for (l, line) in text.iter().enumerate() {
          let n = match_count(line, query);
          if n > 0 {
            index.insert(("src/module.rs".to_string(), 0usize, l), (total, n));
            total += n;
          }
        }
        println!(
          "{lines} lines, query {query:?}: {:?} ({} lines match)",
          started.elapsed(),
          index.len()
        );
      }
    }
  }
}
