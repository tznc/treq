//! The tab as a virtual list. The banners, each day's label, each commit
//! and, in an open commit's card, each file row, hunk header and diff line
//! are rows, and a [`gpui::list`] builds and lays out only the rows on
//! screen, so a frame costs the same with a five-line commit open or a
//! five-thousand-line one.
//!
//! Rows are derived from the view each frame. The space between them is
//! what the nested layout's collapsing margins made it, and each row in the
//! timeline draws its own stretch of the rule.

use crate::gpui_shell::ui::radius;
use std::{collections::HashSet, rc::Rc};

use tauri_plugin_gpui::gpui::{
  self, div, list, prelude::*, px, rems, AnyElement, Context, ListAlignment, ListOffset, ListState,
  MouseButton, MouseMoveEvent, Pixels, SharedString, Window,
};

use super::{CommitsView, Kind};
use crate::{
  gpui_shell::{
    screen::{
      code_data::group_by_day,
      commits_data::{line_kind, line_numbers, History, LineKind},
      highlight,
    },
    theme::{self, Palette},
    ui::{self, zpx},
  },
  jj::{JjDiffHunk, JjLogCommit},
};

/// A commit the list shows, and where.
pub(super) struct Entry {
  pub(super) commit: JjLogCommit,
  pub(super) kind: Kind,
  pub(super) label: Option<String>,
}

/// One row of the list. Commits are by entry, files by their index in the
/// commit's `committed_files`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
  /// "Shift to now" and the conflicts banner, over the timeline.
  Top,
  /// A day's label, by its index in the labels.
  Day(usize),
  /// A commit's row, and when it is open its actions, and its whole card
  /// when that shows no files.
  Commit(usize),
  /// A file's row in an open commit's card.
  File(usize, usize),
  HunkHeader(usize, usize, usize),
  Line(usize, usize, usize, usize),
  /// The comment box under the line it is on.
  CommentBox(usize, usize),
  /// "Load diff" for a file whose diff was held back.
  Deferred(usize, usize),
  /// A file with no diff to show.
  Missing(usize, usize),
  /// "There are no commits within this workspace branch yet."
  OwnEmpty,
  /// "Load more commits", of the target's commits or not.
  LoadMore(bool),
  /// The rule and "Recent on {target}".
  Divider,
  /// The space under the last row.
  End,
}

impl Row {
  fn entry(self) -> Option<usize> {
    match self {
      Row::Commit(e)
      | Row::File(e, _)
      | Row::HunkHeader(e, ..)
      | Row::Line(e, ..)
      | Row::CommentBox(e, _)
      | Row::Deferred(e, _)
      | Row::Missing(e, _) => Some(e),
      _ => None,
    }
  }

  /// A row inside an open commit's card.
  fn in_card(self) -> bool {
    self.entry().is_some() && !matches!(self, Row::Commit(_))
  }
}

/// The list and the rows it was last told about. A row's gap is the space
/// above it, in quarter rems.
pub(super) struct Feed {
  pub(super) list: ListState,
  rows: Vec<(Row, u8)>,
  entries: Vec<Entry>,
  days: Vec<String>,
  /// The banners' state: "Shift to now" shown, and the change ids of the
  /// conflicted commits.
  shift: bool,
  conflicted: Vec<String>,
  /// The rem size the rows were measured at.
  rem: Pixels,
  /// What the rows were laid out from, so an unchanged frame keeps them.
  key: Option<Key>,
}

/// Every input of `layout_rows`. The history and the diffs are too large to
/// compare, so `CommitsView::layout_gen` stands for them: it is bumped
/// wherever either one changes.
#[derive(PartialEq)]
struct Key {
  generation: u64,
  home: bool,
  home_limit: usize,
  target_limit: usize,
  hide_tentative: bool,
  removed: HashSet<String>,
  expanded: HashSet<String>,
  /// The commit, file and line the comment box is under.
  comment: Option<(String, String, (usize, usize))>,
  /// Day labels are relative ("Today"), so a new day relabels them.
  today: chrono::NaiveDate,
}

impl Feed {
  pub(super) fn shift(&self) -> bool {
    self.shift
  }

  pub(super) fn conflicted(&self) -> &[String] {
    &self.conflicted
  }
}

impl Default for Feed {
  fn default() -> Self {
    Self {
      list: ListState::new(0, ListAlignment::Top, px(800.)),
      rows: Vec::new(),
      entries: Vec::new(),
      days: Vec::new(),
      shift: false,
      conflicted: Vec::new(),
      rem: px(0.),
      key: None,
    }
  }
}

/// Rows with the gap above each: of two margins that meet, the larger.
#[derive(Default)]
struct Rows {
  rows: Vec<(Row, u8)>,
  pending: u8,
}

impl Rows {
  fn push(&mut self, row: Row, top: u8, bottom: u8) {
    self.rows.push((row, self.pending.max(top)));
    self.pending = bottom;
  }
}

/// What a row stands for, besides its indices.
fn ident<'a>(
  entries: &'a [Entry],
  days: &'a [String],
  row: Row,
) -> Option<(&'a str, Option<Kind>)> {
  match row {
    Row::Day(d) => days.get(d).map(|d| (d.as_str(), None)),
    _ => row
      .entry()
      .and_then(|e| entries.get(e))
      .map(|e| (e.commit.commit_id.as_str(), Some(e.kind))),
  }
}

impl CommitsView {
  fn layout_key(&self) -> Key {
    Key {
      generation: self.layout_gen,
      home: self.home(),
      home_limit: self.home_limit,
      target_limit: self.target_limit,
      hide_tentative: self.hide_tentative,
      removed: self.removed.clone(),
      expanded: self.expanded.clone(),
      comment: self
        .comment
        .as_ref()
        .map(|c| (c.commit_id.clone(), c.target.file_path.clone(), c.target.at)),
      today: chrono::Local::now().date_naive(),
    }
  }

  /// The commits the list shows, and the banners over them.
  fn layout_rows(&mut self) -> Feed {
    let mut feed = Feed::default();
    let Some(history) = self.history.as_ref() else {
      return feed;
    };
    let home = self.home();
    let own: Vec<JjLogCommit> = history
      .own
      .iter()
      .filter(|c| !self.removed.contains(&c.commit_id))
      .cloned()
      .collect();
    let tentative = history
      .tentative
      .clone()
      .filter(|_| !home && !self.hide_tentative);
    let visible = History {
      own: own.clone(),
      ..history.clone()
    };
    if visible.is_empty() {
      return feed;
    }
    feed.shift = !home && visible.has_mutable();
    feed.conflicted = visible
      .resolvable(home)
      .into_iter()
      .map(|c| c.change_id)
      .collect();
    let target = history.target.clone();
    let shows_target = visible.shows_target();
    let mut rows = Rows::default();
    let banner = if !feed.conflicted.is_empty() {
      4
    } else if feed.shift {
      3
    } else {
      0
    };
    rows.push(Row::Top, 0, banner);
    if let Some((commit, label)) = tentative {
      self.commit_rows(&mut feed, &mut rows, commit, Kind::Tentative, Some(label));
      rows.pending = rows.pending.max(3);
    }
    let mut first = true;
    for (label, commits) in group_by_day(own.iter().cloned()) {
      rows.push(Row::Day(feed.days.len()), 5, 0);
      feed.days.push(label);
      for commit in commits {
        self.commit_rows(&mut feed, &mut rows, commit, Kind::Own { first }, None);
        first = false;
      }
    }
    if home && own.len() + 1 >= self.home_limit {
      rows.push(Row::LoadMore(false), 3, 0);
    }
    if shows_target {
      if own.is_empty() {
        rows.push(Row::OwnEmpty, 0, 3);
      }
      rows.push(Row::Divider, 4, 2);
      for (label, commits) in group_by_day(target.iter().cloned()) {
        rows.push(Row::Day(feed.days.len()), 5, 0);
        feed.days.push(label);
        for commit in commits {
          self.commit_rows(&mut feed, &mut rows, commit, Kind::Target, None);
        }
      }
      if target.len() >= self.target_limit {
        rows.push(Row::LoadMore(true), 3, 0);
      }
    }
    rows.push(Row::End, 0, 0);
    feed.rows = rows.rows;
    feed
  }

  /// A commit's row, then, when it is open and its diff lists files, each
  /// row of its card.
  fn commit_rows(
    &mut self,
    feed: &mut Feed,
    rows: &mut Rows,
    commit: JjLogCommit,
    kind: Kind,
    label: Option<String>,
  ) {
    let e = feed.entries.len();
    let expanded = self.expanded.contains(&commit.commit_id);
    let comment_at = self
      .comment
      .as_ref()
      .filter(|c| c.commit_id == commit.commit_id)
      .map(|c| (c.target.file_path.clone(), c.target.at));
    let state = self.diffs.get_mut(&commit.commit_id);
    feed.entries.push(Entry {
      commit,
      kind,
      label,
    });
    if !expanded {
      rows.push(Row::Commit(e), 0, 0);
      return;
    }
    let Some(state) = state.filter(|s| s.split()) else {
      rows.push(Row::Commit(e), 0, 3);
      return;
    };
    // The actions' own margin is the gap above the card.
    rows.push(Row::Commit(e), 0, 0);
    let Some(diff) = state.diff.as_ref() else {
      return;
    };
    for (f, file) in diff.committed_files.iter().enumerate() {
      rows.push(Row::File(e, f), 0, 0);
      if state.collapsed.contains(&file.path) {
        continue;
      }
      match diff.hunks_by_file.iter().find(|d| d.path == file.path) {
        Some(file_diff) => {
          if !state.numbers.contains_key(&file.path) {
            let numbers = file_diff.hunks.iter().map(line_numbers).collect();
            state.numbers.insert(file.path.clone(), Rc::new(numbers));
          }
          let comment = comment_at
            .as_ref()
            .filter(|(path, _)| *path == file.path)
            .map(|(_, at)| *at);
          for (h, hunk) in file_diff.hunks.iter().enumerate() {
            rows.push(Row::HunkHeader(e, f, h), 0, 0);
            for l in 0..hunk.lines.len() {
              rows.push(Row::Line(e, f, h, l), 0, 0);
              if comment == Some((h, l)) {
                rows.push(Row::CommentBox(e, f), 0, 0);
              }
            }
          }
        }
        None if file.diff_deferred => rows.push(Row::Deferred(e, f), 0, 0),
        None => rows.push(Row::Missing(e, f), 0, 0),
      }
    }
    rows.pending = 3;
  }

  /// Brings the list up to date with the rows, keeping the measured
  /// heights of the rows that did not change.
  pub(super) fn sync_rows(&mut self, window: &mut Window) {
    let rem = window.rem_size();
    if rem != self.feed.rem {
      self.feed.rem = rem;
      self.feed.list.remeasure();
    }
    let key = self.layout_key();
    if self.feed.key.as_ref() == Some(&key) {
      return;
    }
    let mut feed = self.layout_rows();
    feed.key = Some(key);
    let old = &self.feed;
    let same = |a: (Row, u8), b: (Row, u8)| {
      a == b && ident(&old.entries, &old.days, a.0) == ident(&feed.entries, &feed.days, b.0)
    };
    let (n, m) = (old.rows.len(), feed.rows.len());
    let prefix = old
      .rows
      .iter()
      .zip(&feed.rows)
      .take_while(|(a, b)| same(**a, **b))
      .count();
    if prefix < n.max(m) {
      let suffix = old
        .rows
        .iter()
        .rev()
        .zip(feed.rows.iter().rev())
        .take(n.min(m) - prefix)
        .take_while(|(a, b)| same(**a, **b))
        .count();
      self
        .feed
        .list
        .splice(prefix..n - suffix, m - prefix - suffix);
    }
    feed.list = self.feed.list.clone();
    feed.rem = self.feed.rem;
    self.feed = feed;
  }

  /// Nothing to list: no commits, or none read yet.
  pub(super) fn feed_is_empty(&self) -> bool {
    self.feed.rows.is_empty()
  }

  /// The row of the commit with this change id.
  pub(super) fn commit_row(&self, change_id: &str) -> Option<(usize, String)> {
    self
      .feed
      .rows
      .iter()
      .enumerate()
      .find_map(|(ix, (row, _))| match row {
        Row::Commit(e) => {
          let commit = &self.feed.entries[*e].commit;
          (commit.change_id == change_id).then(|| (ix, commit.commit_id.clone()))
        }
        _ => None,
      })
  }

  /// Puts row `ix` at the top of the list.
  pub(super) fn scroll_to_row(&self, ix: usize) {
    self.feed.list.scroll_to(ListOffset {
      item_ix: ix,
      offset_in_item: px(0.),
    });
  }

  /// The tab's list.
  pub(super) fn feed_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .id("commits-scroll")
      .debug_selector(|| "commits-list".into())
      .size_full()
      .child(
        list(
          self.feed.list.clone(),
          cx.processor(|view, ix, _, cx| view.row(ix, cx)),
        )
        .size_full(),
      )
  }

  fn row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
    let Some(&(row, gap)) = self.feed.rows.get(ix) else {
      return div().into_any_element();
    };
    let p = theme::palette(self.dark);
    let n = self.feed.rows.len();
    let content = match row {
      Row::Top => {
        return div()
          .px_4()
          .pt_4()
          .child(self.banners(&p, cx))
          .into_any_element()
      }
      Row::End => return div().pt(rems(gap as f32 * 0.25)).pb_4().into_any_element(),
      Row::Day(d) => ui::text(div(), ui::TextSize::Sm)
        .pb_1()
        .pl_7()
        .text_color(p.muted_foreground)
        .child(self.feed.days[d].clone())
        .into_any_element(),
      Row::Commit(e) => {
        let entry = &self.feed.entries[e];
        let (commit, kind, label) = (entry.commit.clone(), entry.kind, entry.label.clone());
        self
          .commit(&commit, kind, label.as_ref(), &p, cx)
          .into_any_element()
      }
      Row::OwnEmpty => div()
        .debug_selector(|| "own-commits-empty".into())
        .pl_7()
        .text_color(p.muted_foreground)
        .child(if self.home() {
          "There are no commits on this branch yet."
        } else {
          "There are no commits within this workspace branch yet."
        })
        .into_any_element(),
      Row::LoadMore(target) => self.load_more_row(target, &p, cx).into_any_element(),
      Row::Divider => {
        let branch = self
          .history
          .as_ref()
          .map(|h| h.target_branch.clone())
          .unwrap_or_default();
        div()
          // `border-t`, drawn as a rule: an empty div paints no border.
          .child(div().pb_4().child(div().h(zpx(1.)).bg(p.border).mx_2()))
          .child(
            ui::text(div(), ui::TextSize::Sm)
              .debug_selector(|| "target-divider".into())
              .font_weight(gpui::FontWeight::SEMIBOLD)
              .text_color(p.muted_foreground)
              .pl_7()
              .child(format!("Recent on {branch}")),
          )
          .into_any_element()
      }
      _ => self.card_row(ix, row, &p, cx),
    };
    // The timeline's rule, from 0.5rem into the first row to 0.5rem above
    // the last one's end.
    let first = ix == 1;
    let last = ix + 2 == n;
    let gap = gap as f32 * 0.25;
    div()
      .px_4()
      .child(
        div()
          .relative()
          .pt(rems(gap))
          .child(
            div()
              .absolute()
              .left(zpx(7.))
              .top(rems(if first { gap + 0.5 } else { 0. }))
              .bottom(rems(if last { 0.5 } else { 0. }))
              .w(zpx(2.))
              .bg(p.border),
          )
          .child(content),
      )
      .into_any_element()
  }

  /// A row of an open commit's card, its side borders and, first and last,
  /// its rounded top or bottom.
  fn card_row(&mut self, ix: usize, row: Row, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
    let top = matches!(row, Row::File(_, 0));
    let bottom = !self
      .feed
      .rows
      .get(ix + 1)
      .is_some_and(|(next, _)| next.in_card() && next.entry() == row.entry());
    let content = match row {
      Row::File(e, f) => self.file_row_at(e, f, p, cx),
      _ => div()
        .bg(p.muted.opacity(0.2))
        .child(self.body_row(row, p, cx))
        .into_any_element(),
    };
    div()
      .ml_7()
      .border_x_1()
      .border_color(p.border)
      .when(top, |this| this.border_t_1().rounded_t(radius::MD))
      .when(bottom, |this| this.border_b_1().rounded_b(radius::MD))
      .overflow_hidden()
      .child(content)
      .into_any_element()
  }

  fn file_row_at(&mut self, e: usize, f: usize, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
    let commit = self.feed.entries[e].commit.clone();
    let Some(state) = self.diffs.get(&commit.commit_id) else {
      return div().into_any_element();
    };
    let Some(file) = state
      .diff
      .as_ref()
      .and_then(|d| d.committed_files.get(f))
      .cloned()
    else {
      return div().into_any_element();
    };
    let open = !state.collapsed.contains(&file.path);
    div()
      .when(f > 0, |this| this.border_t_1().border_color(p.border))
      .child(self.file_row(&commit, &file, open, p, cx))
      .into_any_element()
  }

  /// A row under a file's row: a hunk's header, a line, the comment box,
  /// "Load diff" or "No diff content available."
  fn body_row(&mut self, row: Row, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
    let (Row::HunkHeader(e, f, _)
    | Row::Line(e, f, ..)
    | Row::CommentBox(e, f)
    | Row::Deferred(e, f)
    | Row::Missing(e, f)) = row
    else {
      return div().into_any_element();
    };
    let commit = self.feed.entries[e].commit.clone();
    let Some(state) = self.diffs.get(&commit.commit_id) else {
      return div().into_any_element();
    };
    let Some(diff) = state.diff.as_ref() else {
      return div().into_any_element();
    };
    let Some(file) = diff.committed_files.get(f) else {
      return div().into_any_element();
    };
    let path = file.path.clone();
    let hunks = diff
      .hunks_by_file
      .iter()
      .find(|d| d.path == path)
      .map(|d| d.hunks.as_slice())
      .unwrap_or_default();
    match row {
      Row::HunkHeader(_, _, h) => div()
        .font_family(theme::MONO_FONT)
        .child(
          div()
            .px_3()
            .py_0p5()
            .bg(p.muted.opacity(0.6))
            .text_color(p.muted_foreground)
            .border_t_1()
            .border_color(p.border)
            .child(hunks.get(h).map(|h| h.header.clone()).unwrap_or_default()),
        )
        .into_any_element(),
      Row::Line(_, _, h, l) => {
        let Some(hunk) = hunks.get(h) else {
          return div().into_any_element();
        };
        let numbers = state.numbers.get(&path).cloned();
        let at = numbers
          .and_then(|n| n.get(h)?.get(l).copied())
          .unwrap_or((None, None));
        let line = self.line(&commit, &path, hunk, (h, l), at, p, cx);
        div()
          .font_family(theme::MONO_FONT)
          .child(line)
          .into_any_element()
      }
      Row::CommentBox(..) => self.comment_box(p, cx).into_any_element(),
      Row::Deferred(_, f) => self.deferred_file(&commit, f, p, cx),
      _ => div()
        .border_t_1()
        .border_color(p.border)
        .px_3()
        .py_3()
        .text_color(p.muted_foreground)
        .child("No diff content available.")
        .into_any_element(),
    }
  }

  /// `HunkView`'s line: its numbers, the comment button, the sign and the
  /// highlighted text.
  #[allow(
    clippy::too_many_arguments,
    reason = "the row needs its commit, file, hunk and position"
  )]
  fn line(
    &self,
    commit: &JjLogCommit,
    path: &str,
    hunk: &JjDiffHunk,
    at: (usize, usize),
    (old, new): (Option<u32>, Option<u32>),
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let (h, l) = at;
    let line = hunk.lines.get(l).map_or("", String::as_str);
    let selected = self
      .selection
      .as_ref()
      .is_some_and(|s| s.commit_id == commit.commit_id && s.path == path && s.lines.contains(&at));
    let gutter_fg = p.muted_foreground.opacity(0.6);
    let gutter_hover = p.muted.opacity(0.5);
    let kind = line_kind(line);
    let bg = if selected {
      Some(ui::Tone::Primary.tint(p))
    } else {
      match kind {
        LineKind::Added => Some(ui::Tone::GitOpen.tint(p)),
        LineKind::Removed => Some(ui::Tone::GitClosed.tint(p)),
        LineKind::Context => None,
      }
    };
    let prefix = match kind {
      LineKind::Added => "+",
      LineKind::Removed => "-",
      LineKind::Context => " ",
    };
    let gutter = |n: Option<u32>, which: &'static str, cx: &mut Context<Self>| {
      let (id, file) = (commit.commit_id.clone(), path.to_string());
      div()
        .id(SharedString::from(format!("gutter-{which}-{h}-{l}")))
        .w_10()
        .flex_none()
        .pr_1()
        .flex()
        .justify_end()
        .text_color(gutter_fg)
        .cursor_pointer()
        .hover(move |s| s.bg(gutter_hover))
        .child(n.map(|n| n.to_string()).unwrap_or_default())
        .on_mouse_down(
          MouseButton::Left,
          cx.listener(move |view, e: &gpui::MouseDownEvent, _, cx| {
            view.press_line(&id, &file, at, e.modifiers.shift, cx);
          }),
        )
    };
    let plus = {
      let (c, file) = (commit.clone(), path.to_string());
      div()
        .w_5()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
          div()
            .id(SharedString::from(format!("add-comment-{h}-{l}")))
            .debug_selector(move || format!("add-comment-{h}-{l}"))
            .p_px()
            .rounded(radius::SM)
            .bg(p.primary)
            .opacity(0.)
            .group_hover("diff-line", |s| s.opacity(1.))
            .cursor_pointer()
            .child(ui::icon(theme::ICON_PLUS, 12., p.primary_foreground))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |view, _, _, cx| {
              cx.stop_propagation();
              view.add_comment(&c, &file, at, cx);
            })),
        )
    };
    let (id, file) = (commit.commit_id.clone(), path.to_string());
    let selector = format!("commit-line-{path}-{h}-{l}");
    div()
      .id(SharedString::from(format!("diff-line-{h}-{l}")))
      .debug_selector(move || selector)
      .group("diff-line")
      .flex()
      .when_some(bg, |this, bg| this.bg(bg))
      .on_mouse_move(cx.listener(move |view, _: &MouseMoveEvent, _, cx| {
        view.drag_to(&id, &file, at, cx);
      }))
      .child(gutter(old, "old", cx))
      .child(gutter(new, "new", cx))
      .child(plus)
      .child(
        div()
          .w_4()
          .flex_none()
          .flex()
          .justify_center()
          .text_color(gutter_fg)
          .child(prefix),
      )
      .child(
        div()
          .flex_1()
          .min_w_0()
          .overflow_hidden()
          .whitespace_nowrap()
          .child(highlight::styled(
            line.get(1..).unwrap_or(""),
            highlight::language(path),
            p.dark,
          )),
      )
  }
}
