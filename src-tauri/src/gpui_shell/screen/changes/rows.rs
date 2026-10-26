//! The diff as a virtual list. Every file card is cut into rows (its head,
//! a hunk's header, its lines, the context lines read around it and its
//! conflict cards), and a [`gpui::list`] builds and lays out only the rows
//! on screen, so a frame costs the same for five lines or five thousand.
//!
//! What a row draws is derived from the view each frame; only line numbers
//! and the character-level marks are kept between frames, per file, until
//! its diff changes.

use std::{
  collections::{BTreeSet, HashMap, HashSet},
  ops::Range,
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use tauri_plugin_gpui::gpui::{
  self, div, list, prelude::*, px, AnyElement, Context, ListAlignment, ListOffset, ListState,
  MouseButton, Pixels, SharedString, Stateful, Window,
};

use super::{context_row, ChangesView, Dir};
use crate::gpui_shell::ui::radius;
use crate::{
  gpui_shell::{
    screen::{
      changes_data::{
        body, conflict_lines, conflicts, context_row_numbers, is_collapsed, line_counts, Body,
        BodyInput, Conflict, FileHunks,
      },
      changes_review::FILE_HUNK,
      commits_data::{line_number, line_numbers},
      highlight, intraline,
    },
    theme::{self, Palette},
    ui::{self, TextSize},
  },
  jj::JjDiffHunk,
};

/// A file card in the diff.
pub(super) struct Card {
  pub(super) path: String,
  pub(super) status: String,
  pub(super) old_path: Option<String>,
  pub(super) committed: bool,
  pub(super) conflicted: bool,
  pub(super) body: Body,
  pub(super) shut: bool,
  regions: Vec<Conflict>,
  prepared: Option<Rc<Prepared>>,
}

/// One row of the list, by card index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
  /// The card's header and file comments, and everything below them when
  /// the card shows no hunks.
  Head(usize, Body),
  /// A hunk's `@@` header, and whether it reads more lines above.
  HunkHeader(usize, usize, bool),
  /// A line "Show more lines" read, by hunk, side and index.
  Context(usize, usize, Dir, usize),
  /// A diff line, with its comments and comment box.
  Line(usize, usize, usize),
  /// A conflict region, by its index in the card's regions.
  Conflict(usize, usize),
  /// "Show more lines below" under a hunk.
  Below(usize, usize),
  /// The space under the last card.
  End,
}

impl Row {
  fn card(self) -> Option<usize> {
    match self {
      Row::Head(c, _)
      | Row::HunkHeader(c, ..)
      | Row::Context(c, ..)
      | Row::Line(c, ..)
      | Row::Conflict(c, _)
      | Row::Below(c, _) => Some(c),
      Row::End => None,
    }
  }
}

/// Line numbers and character marks of one file's hunks.
struct Prepared {
  /// The hunks these were derived from; a reload swaps the `Arc`.
  source: Option<Arc<Vec<JjDiffHunk>>>,
  numbers: Vec<Vec<(Option<u32>, Option<u32>)>>,
  changed: Vec<HashMap<usize, Vec<Range<usize>>>>,
}

impl Prepared {
  fn new(source: Option<&Arc<Vec<JjDiffHunk>>>) -> Self {
    let hunks = source.map_or(&[][..], |h| h.as_slice());
    Self {
      source: source.cloned(),
      numbers: hunks.iter().map(line_numbers).collect(),
      changed: hunks
        .iter()
        .map(|h| intraline::changed_ranges(&h.lines))
        .collect(),
    }
  }

  fn matches(&self, source: Option<&Arc<Vec<JjDiffHunk>>>) -> bool {
    match (&self.source, source) {
      (Some(a), Some(b)) => Arc::ptr_eq(a, b),
      (None, None) => true,
      _ => false,
    }
  }
}

/// The list and the rows it was last told about.
pub(super) struct DiffRows {
  pub(super) list: ListState,
  pub(super) rows: Vec<Row>,
  pub(super) cards: Vec<Card>,
  prepared: HashMap<(String, bool), Rc<Prepared>>,
  /// The rem size the rows were measured at.
  rem: Pixels,
  /// What the rows were laid out from, so an unchanged frame keeps them.
  key: Option<Key>,
  /// Bumped each time the rows are laid out again: the find index and the
  /// placed threads are derived from the same inputs and follow it.
  pub(super) revision: u64,
  /// How long the last layout of the rows took.
  pub(super) layout_cost: Duration,
}

/// Every input of `layout_rows`. The snapshot, the hunks and the lines read
/// around them are too large to compare, so `ChangesView::layout_gen`
/// stands for them: it is bumped wherever any of them changes.
#[derive(PartialEq)]
struct Key {
  generation: u64,
  pr_error: bool,
  changeset_expanded: bool,
  show_committed: Option<bool>,
  /// Generated files the filter keeps because they are staged; `None`
  /// with the filter off.
  kept_generated: Option<BTreeSet<String>>,
  collapsed: HashSet<String>,
  expanded_large: HashSet<String>,
  loaded_generated: HashSet<String>,
  /// Files with a file comment or one being written.
  file_comments: BTreeSet<String>,
}

impl Default for DiffRows {
  fn default() -> Self {
    Self {
      // Draws a screen's worth above and below, so a scroll or the next
      // arrow key rarely meets an unmeasured row.
      list: ListState::new(0, ListAlignment::Top, px(800.)),
      rows: Vec::new(),
      cards: Vec::new(),
      prepared: HashMap::new(),
      rem: px(0.),
      key: None,
      revision: 0,
      layout_cost: Duration::ZERO,
    }
  }
}

/// A card's identity, which a row's index stands for.
fn card_key(cards: &[Card], row: Row) -> Option<(&str, bool)> {
  row
    .card()
    .and_then(|c| cards.get(c))
    .map(|c| (c.path.as_str(), c.committed))
}

impl ChangesView {
  /// The diff a card shows: the branch's for a file in
  /// [`Snapshot::shows_branch_diff`], else the working copy's, else the
  /// branch's.
  pub(super) fn shown_hunks(&self, path: &str) -> Option<&FileHunks> {
    let branch = self
      .snapshot
      .as_ref()
      .and_then(|s| s.committed_hunks.get(path));
    if self
      .snapshot
      .as_ref()
      .is_some_and(|s| s.shows_branch_diff(path))
    {
      return branch.or_else(|| self.hunks.get(path));
    }
    self.hunks.get(path).or(branch)
  }

  fn layout_key(&self) -> Key {
    let review = &self.review;
    Key {
      generation: self.layout_gen,
      pr_error: self.pr.as_ref().is_some_and(|pr| pr.error.is_some()),
      changeset_expanded: self.changeset_expanded,
      show_committed: self.options.show_committed,
      kept_generated: self.hide_generated.then(|| {
        self
          .snapshot
          .iter()
          .flat_map(|s| &s.generated)
          .filter(|p| self.staging.staged.contains(*p))
          .cloned()
          .collect()
      }),
      collapsed: self.collapsed_files.clone(),
      expanded_large: self.expanded_large.clone(),
      loaded_generated: self.loaded_generated.clone(),
      file_comments: review
        .comments
        .iter()
        .filter(|c| c.is_file())
        .map(|c| &c.file_path)
        .chain(
          review
            .draft
            .iter()
            .filter(|d| d.hunk_id == FILE_HUNK)
            .map(|d| &d.file_path),
        )
        .cloned()
        .collect(),
    }
  }

  /// The cards and rows the diff shows now. None while the tab shows a
  /// placeholder instead (loading, no changes, the large-changeset gate).
  fn layout_rows(&mut self) -> (Vec<Card>, Vec<Row>) {
    let Some(snapshot) = self.snapshot.as_ref() else {
      return Default::default();
    };
    if self.pr.as_ref().is_some_and(|pr| pr.error.is_some()) || self.large_changeset_gated() {
      return Default::default();
    }
    let conflicted: HashSet<String> = self.conflicted().into_iter().collect();
    let mut files: Vec<(String, String, Option<String>, bool)> = snapshot
      .files
      .iter()
      .filter(|f| !self.hides(&f.path))
      .map(|f| (f.path.clone(), f.status.clone(), f.old_path.clone(), false))
      .collect();
    files.extend(
      self
        .visible_committed()
        .into_iter()
        .map(|f| (f.path, f.status, f.previous_path, true)),
    );
    let mut prepared = std::mem::take(&mut self.diff.prepared);
    let mut kept = HashMap::new();
    let mut cards = Vec::with_capacity(files.len());
    let mut rows = Vec::new();
    for (path, status, old_path, committed) in files {
      let data = self.shown_hunks(&path);
      let input = BodyInput {
        path: &path,
        status: &status,
        renamed: old_path.is_some(),
        data,
        collapsed: self.collapsed_files.contains(&path),
        comment_activity: self.file_comment_activity(&path),
        expanded_large: self.expanded_large.contains(&path),
        generated: self.is_generated(&path),
        loaded_generated: self.loaded_generated.contains(&path),
      };
      let shut = is_collapsed(&input);
      let kind = body(&input);
      let c = cards.len();
      rows.push(Row::Head(c, kind));
      let mut card = Card {
        conflicted: conflicted.contains(&path),
        path,
        status,
        old_path,
        committed,
        body: kind,
        shut,
        regions: Vec::new(),
        prepared: None,
      };
      if kind == Body::Hunks {
        let hunks = data.map(|d| d.hunks.as_slice()).unwrap_or_default();
        let key = (card.path.clone(), committed);
        let source = data.map(|d| &d.hunks);
        let prep = match prepared.remove(&key) {
          Some(p) if p.matches(source) => p,
          _ => Rc::new(Prepared::new(source)),
        };
        card.regions = conflicts(&card.path, hunks);
        self.hunk_rows(c, &card, hunks, &prep, &mut rows);
        card.prepared = Some(prep.clone());
        kept.insert(key, prep);
      }
      cards.push(card);
    }
    self.diff.prepared = kept;
    if !rows.is_empty() {
      rows.push(Row::End);
    }
    (cards, rows)
  }

  /// The rows of a card's hunks, as `hunks` drew them.
  fn hunk_rows(
    &self,
    c: usize,
    card: &Card,
    hunks: &[JjDiffHunk],
    prep: &Prepared,
    rows: &mut Vec<Row>,
  ) {
    let path = card.path.as_str();
    let in_region = conflict_lines(&card.regions);
    let mut drawn_regions: HashSet<usize> = HashSet::new();
    let read = |h: usize, dir: Dir| {
      self
        .context
        .get(&(path.to_string(), h, dir))
        .map_or(0, Vec::len)
    };
    for (h, hunk) in hunks.iter().enumerate() {
      let before = read(h, Dir::Before);
      let (above, below) = self.context_ranges(path, hunks, h);
      // Once the lines above meet the previous hunk's, or the file's start,
      // the header no longer separates anything.
      let joined = above.is_none()
        && (before > 0
          || h
            .checked_sub(1)
            .is_some_and(|prev| read(prev, Dir::After) > 0));
      if !joined {
        rows.push(Row::HunkHeader(c, h, above.is_some()));
      }
      rows.extend((0..before).map(|i| Row::Context(c, h, Dir::Before, i)));
      for l in 0..hunk.lines.len() {
        let new = prep.numbers[h].get(l).and_then(|n| n.1);
        if let Some(region) = new.and_then(|n| in_region.get(&(n as usize)).copied()) {
          if card.regions[region].region.start_line == new.unwrap_or(0) as usize
            && drawn_regions.insert(region)
          {
            rows.push(Row::Conflict(c, region));
          }
          continue;
        }
        rows.push(Row::Line(c, h, l));
      }
      rows.extend((0..read(h, Dir::After)).map(|i| Row::Context(c, h, Dir::After, i)));
      if below.is_some() {
        rows.push(Row::Below(c, h));
      }
    }
  }

  /// Brings the list up to date with the rows, keeping the measured
  /// heights of the rows that did not change.
  pub(super) fn sync_rows(&mut self, window: &mut Window) {
    let rem = window.rem_size();
    if rem != self.diff.rem {
      self.diff.rem = rem;
      self.diff.list.remeasure();
    }
    let key = self.layout_key();
    if self.diff.key.as_ref() == Some(&key) {
      return;
    }
    let started = Instant::now();
    let (cards, rows) = self.layout_rows();
    self.diff.layout_cost = started.elapsed();
    self.diff.key = Some(key);
    self.diff.revision += 1;
    let old = (&self.diff.rows, &self.diff.cards);
    let same = |a: Row, b: Row| a == b && card_key(old.1, a) == card_key(&cards, b);
    if old.0.len() == rows.len() && old.0.iter().zip(&rows).all(|(a, b)| same(*a, *b)) {
      self.diff.cards = cards;
      return;
    }
    let prefix = old
      .0
      .iter()
      .zip(&rows)
      .take_while(|(a, b)| same(**a, **b))
      .count();
    let suffix = old
      .0
      .iter()
      .rev()
      .zip(rows.iter().rev())
      .take(old.0.len().min(rows.len()) - prefix)
      .take_while(|(a, b)| same(**a, **b))
      .count();
    self
      .diff
      .list
      .splice(prefix..old.0.len() - suffix, rows.len() - prefix - suffix);
    self.diff.rows = rows;
    self.diff.cards = cards;
  }

  /// How many times the rows were laid out.
  #[cfg(test)]
  pub(in crate::gpui_shell::screen) fn layout_revision(&self) -> u64 {
    self.diff.revision
  }

  /// The row of a file's card.
  pub(super) fn card_row(&self, path: &str) -> Option<usize> {
    self
      .diff
      .rows
      .iter()
      .position(|r| matches!(r, Row::Head(c, _) if self.diff.cards[*c].path == path))
  }

  /// The row of a diff line.
  pub(super) fn line_row(&self, path: &str, (h, l): (usize, usize)) -> Option<usize> {
    self.diff.rows.iter().position(
      |r| matches!(r, Row::Line(c, rh, rl) if (*rh, *rl) == (h, l) && self.diff.cards[*c].path == path),
    )
  }

  /// The row of a conflict card, by the conflict's id.
  pub(super) fn conflict_row(&self, id: &str) -> Option<usize> {
    self.diff.rows.iter().position(
      |r| matches!(r, Row::Conflict(c, region) if self.diff.cards[*c].regions[*region].id == id),
    )
  }

  /// Puts row `ix` at the top of the list unless some of it already shows.
  /// The rows between may not be measured yet, so this is the one place
  /// a row can be scrolled to exactly; callers fine-tune once it is drawn.
  pub(super) fn reveal_row(&self, ix: usize) {
    let list = &self.diff.list;
    let view = list.viewport_bounds();
    let shows = list
      .bounds_for_item(ix)
      .is_some_and(|b| b.bottom() > view.top() && b.top() < view.bottom());
    if !shows {
      self.scroll_to_row(ix);
    }
  }

  /// Puts a card's top, and the gap above it, at the top of the list.
  pub(super) fn scroll_to_row(&self, ix: usize) {
    self.diff.list.scroll_to(ListOffset {
      item_ix: ix,
      offset_in_item: px(0.),
    });
  }

  /// The diff's list.
  pub(super) fn diff_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .debug_selector(|| "changes-diff-list".into())
      .size_full()
      .child(
        list(
          self.diff.list.clone(),
          cx.processor(|view, ix, window, cx| view.row(ix, window, cx)),
        )
        .size_full(),
      )
  }

  fn row(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let Some(&row) = self.diff.rows.get(ix) else {
      return div().into_any_element();
    };
    let p = theme::palette(self.dark);
    let Some(c) = row.card() else {
      // The last card's gap, then room to scroll it up.
      return div().pt_4().pb_32().into_any_element();
    };
    let card = |r: Option<&Row>| r.and_then(|r| r.card());
    let top = !matches!(ix.checked_sub(1).and_then(|i| self.diff.rows.get(i)), Some(r) if card(Some(r)) == Some(c));
    let bottom = card(self.diff.rows.get(ix + 1)) != Some(c);
    let path = self.diff.cards[c].path.clone();
    let content = match row {
      Row::Head(..) => self.card_head(c, &p, cx),
      _ => ui::text(div(), TextSize::Code)
        .font_family(theme::MONO_FONT)
        .child(self.body_row(row, &p, cx))
        .into_any_element(),
    };
    div()
      .when(top, |this| this.pt_4())
      .child(
        div()
          .when(top, |this| {
            let id = format!("file-card-{path}");
            this
              .debug_selector(move || id)
              .border_t_1()
              .rounded_t(radius::LG)
          })
          .when(bottom, |this| this.border_b_1().rounded_b(radius::LG))
          .border_x_1()
          .border_color(p.border)
          .overflow_hidden()
          .bg(p.background)
          .child(content),
      )
      .into_any_element()
  }

  /// A row below a card's head.
  fn body_row(&self, row: Row, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
    let (c, h) = match row {
      Row::HunkHeader(c, h, _) | Row::Context(c, h, ..) | Row::Line(c, h, _) | Row::Below(c, h) => {
        (c, h)
      }
      Row::Conflict(c, region) => {
        return self
          .conflict_card(&self.diff.cards[c].regions[region], p, cx)
          .into_any_element();
      }
      Row::Head(..) | Row::End => return div().into_any_element(),
    };
    let card = &self.diff.cards[c];
    let path = card.path.as_str();
    let hunks = self
      .shown_hunks(path)
      .map(|d| d.hunks.as_slice())
      .unwrap_or_default();
    let Some(hunk) = hunks.get(h) else {
      return div().into_any_element();
    };
    let language = highlight::language(path);
    let border = p.border.opacity(0.4);
    let muted = p.muted_foreground;
    let gutter_cols = |el: Stateful<gpui::Div>| {
      el.child(div().w_24().flex_none().border_r_1().border_color(border))
        .child(div().w_6().flex_none())
        .child(div().w_5().flex_none())
    };
    match row {
      Row::HunkHeader(_, _, expandable) => {
        let file = path.to_string();
        let hover = p.muted.opacity(0.8);
        gutter_cols(
          div()
            .id(SharedString::from(format!("hunk-header-{path}-{h}")))
            .debug_selector({
              let id = format!("hunk-header-{path}-{h}");
              move || id
            })
            .flex()
            .bg(p.muted.opacity(0.6))
            .when(expandable, |this| {
              this
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |view, _, _, cx| {
                  view.expand_context(&file, h, Dir::Before, false, cx)
                }))
            }),
        )
        .child(
          div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .px_2()
            .py_0p5()
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
            })
            // Between hunks the row above offers it, for the same gap.
            .when_some(
              (h == 0)
                .then(|| self.expand_all_range(path, hunks, h, Dir::Before))
                .flatten()
                .map(|(start, end)| end + 1 - start),
              |this, count| this.child(self.expand_all_button(path, h, Dir::Before, count, cx)),
            ),
        )
        .into_any_element()
      }
      Row::Context(_, _, dir, i) => {
        let lines = self
          .context
          .get(&(path.to_string(), h, dir))
          .map(Vec::as_slice)
          .unwrap_or_default();
        let line = lines.get(i).map_or("", String::as_str);
        let numbers = context_row_numbers(&hunk.header, dir == Dir::After, lines.len(), i);
        let side = if dir == Dir::After { "after" } else { "before" };
        context_row(
          format!("context-new-{path}-{h}-{side}-{i}"),
          numbers,
          line,
          language,
          p,
        )
        .into_any_element()
      }
      Row::Line(_, _, l) => {
        // Numbered with the card's other lines, not by counting up to this one.
        let numbers = card
          .prepared
          .as_ref()
          .and_then(|prep| prep.numbers.get(h)?.get(l).copied())
          .unwrap_or_else(|| line_number(hunk, l));
        let changed = card
          .prepared
          .as_ref()
          .and_then(|prep| prep.changed.get(h)?.get(&l))
          .map_or(&[][..], Vec::as_slice);
        div()
          .child(
            self
              .diff_line(
                path,
                hunks,
                h,
                l,
                numbers,
                changed,
                language,
                card.conflicted,
                p,
                cx,
              )
              .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, e: &gpui::MouseDownEvent, _, cx| view.line_menu(e.position, cx)),
              ),
          )
          .children(self.line_extras(path, hunks, h, l, numbers, p, cx))
          .into_any_element()
      }
      Row::Below(..) => {
        let file = path.to_string();
        let hover = p.muted.opacity(0.5);
        gutter_cols(
          div()
            .id(SharedString::from(format!("expand-after-{path}-{h}")))
            .debug_selector({
              let id = format!("expand-after-{path}-{h}");
              move || id
            })
            .flex()
            .bg(p.muted.opacity(0.3))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |view, _, _, cx| {
              view.expand_context(&file, h, Dir::After, false, cx)
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
            .py_0p5()
            .text_color(muted)
            .child(ui::icon(theme::ICON_CHEVRON_DOWN, 12., muted))
            .child("Show more lines below")
            .when_some(
              self
                .expand_all_range(path, hunks, h, Dir::After)
                .map(|(start, end)| end + 1 - start),
              |this, count| this.child(self.expand_all_button(path, h, Dir::After, count, cx)),
            ),
        )
        .into_any_element()
      }
      Row::Head(..) | Row::Conflict(..) | Row::End => div().into_any_element(),
    }
  }

  /// "Expand all (N lines)": reads the whole gap in one click.
  fn expand_all_button(
    &self,
    path: &str,
    h: usize,
    dir: Dir,
    count: usize,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let id = match dir {
      Dir::Before => format!("expand-all-above-{path}-{h}"),
      Dir::After => format!("expand-all-{path}-{h}"),
    };
    let file = path.to_string();
    let hover = p.foreground;
    ui::text(div(), TextSize::Xs)
      .id(SharedString::from(id.clone()))
      .debug_selector(move || id)
      .ml_3()
      .flex_none()
      .text_color(p.muted_foreground)
      .cursor_pointer()
      .hover(move |s| s.text_color(hover))
      .on_click(cx.listener(move |view, _, _, cx| {
        // The row behind it reads twenty lines.
        cx.stop_propagation();
        view.expand_context(&file, h, dir, true, cx);
      }))
      .child(format!("Expand all ({count} lines)"))
  }

  /// Added and removed lines of a card, for its header.
  pub(super) fn card_counts(&self, c: usize) -> (usize, usize) {
    self
      .shown_hunks(&self.diff.cards[c].path)
      .filter(|d| !d.loading)
      .map_or((0, 0), |d| line_counts(&d.hunks))
  }
}
