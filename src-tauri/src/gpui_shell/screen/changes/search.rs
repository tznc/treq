//! The Changes tab's find bar (`useDiffSearch`, `SearchOverlay`): Cmd+F
//! opens it, matches are marked in the diff and Enter steps through them.

use crate::gpui_shell::ui::radius;
use crate::result_ext::ResultExt;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyElement, StyledText, Task};
use gpui_kit::component::input::{Escape, InputState};

use super::*;

/// `useDebounce(searchQuery, 150)`.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Where a match is: a diff line, or a line of a conflict region.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::gpui_shell::screen) enum MatchKey {
  Line(String, usize, usize),
  Conflict(String, usize),
}

/// One file's inputs to the match index: what decides the lines searched.
struct Indexed {
  path: String,
  conflicted: bool,
  body: Body,
  hunks: Option<Arc<Vec<JjDiffHunk>>>,
}

impl PartialEq for Indexed {
  fn eq(&self, other: &Self) -> bool {
    self.path == other.path
      && self.conflicted == other.conflicted
      && self.body == other.body
      && match (&self.hunks, &other.hunks) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
      }
  }
}

/// A match's line and where it was drawn.
type Drawn = (MatchKey, Bounds<Pixels>);

#[derive(Default)]
pub(in crate::gpui_shell::screen) struct Search {
  pub(super) open: bool,
  query: String,
  /// The query after the debounce: what is matched.
  pub(super) applied: String,
  pub(in crate::gpui_shell::screen) current: usize,
  /// Matches by line: the first's index overall and how many.
  index: HashMap<MatchKey, (usize, usize)>,
  pub(in crate::gpui_shell::screen) total: usize,
  /// What `index` was built from, to skip rebuilding it.
  indexed: Option<(String, Vec<Indexed>)>,
  input: Option<Entity<InputState>>,
  focus: bool,
  /// Scroll the current match into view once it is drawn.
  reveal: bool,
  /// Where the current match was last drawn.
  target: Rc<RefCell<Option<Drawn>>>,
  debounce: Option<Task<()>>,
  _subscription: Option<Subscription>,
  /// Cmd+F anywhere in the window (`useKeyboardShortcut`).
  _find_key: Option<Subscription>,
}

/// Byte ranges of `query` in `text`, ignoring case, left to right and
/// not overlapping (a `gi` regex of the escaped query).
pub(in crate::gpui_shell::screen) fn match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
  let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
  if q.is_empty() {
    return Vec::new();
  }
  let chars: Vec<(usize, char)> = text.char_indices().collect();
  let mut out = Vec::new();
  let mut i = 0;
  while i < chars.len() {
    let (mut qi, mut j) = (0, i);
    let mut ok = true;
    while ok && qi < q.len() {
      let Some(&(_, c)) = chars.get(j) else {
        ok = false;
        break;
      };
      for lower in c.to_lowercase() {
        if q.get(qi) == Some(&lower) {
          qi += 1;
        } else {
          ok = false;
          break;
        }
      }
      j += 1;
    }
    if ok {
      let end = chars.get(j).map_or(text.len(), |c| c.0);
      out.push(chars[i].0..end);
      i = j;
    } else {
      i += 1;
    }
  }
  out
}

/// How many `match_ranges` `text` has. ASCII text and queries, the usual
/// case, are counted in place: the index counts every line every frame.
fn match_count(text: &str, query: &str) -> usize {
  if !(text.is_ascii() && query.is_ascii()) {
    return match_ranges(text, query).len();
  }
  let (text, query) = (text.as_bytes(), query.as_bytes());
  if query.is_empty() {
    return 0;
  }
  let (mut i, mut n) = (0, 0);
  while i + query.len() <= text.len() {
    if text[i..i + query.len()].eq_ignore_ascii_case(query) {
      n += 1;
      i += query.len();
    } else {
      i += 1;
    }
  }
  n
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

impl ChangesView {
  /// Listens for Cmd+F in this view's window, focused or not.
  pub(super) fn watch_find_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.search._find_key.is_some() {
      return;
    }
    let id = window.window_handle().window_id();
    self.search._find_key = Some(cx.observe_keystrokes(move |view, event, window, cx| {
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

  /// Cmd+F: opens the bar, or selects its text again.
  pub(super) fn open_search(&mut self, cx: &mut Context<Self>) {
    self.search.open = true;
    self.search.focus = true;
    cx.notify();
  }

  pub(super) fn close_search(&mut self, cx: &mut Context<Self>) {
    let input = self.search.input.take();
    self.search = Search {
      target: self.search.target.clone(),
      _find_key: self.search._find_key.take(),
      ..Search::default()
    };
    drop(input);
    cx.notify();
  }

  fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
    self.search.query = query.clone();
    self.search.current = 0;
    self.search.debounce = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DEBOUNCE).await;
      this
        .update(cx, |view, cx| {
          view.search.applied = query;
          view.search.current = 0;
          cx.notify();
        })
        .ignore_closed();
    }));
    cx.notify();
  }

  pub(super) fn step_match(&mut self, delta: isize, cx: &mut Context<Self>) {
    let total = self.search.total as isize;
    if total == 0 {
      return;
    }
    self.search.current = (self.search.current as isize + delta).rem_euclid(total) as usize;
    self.search.reveal = true;
    cx.notify();
  }

  /// Builds the input once the bar opens, and focuses it when asked.
  pub(super) fn ensure_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

  /// `searchData`: every match, in React's order (conflicts, then the
  /// working copy's files, then the committed ones shown).
  pub(super) fn index_matches(&mut self) {
    let query = self.search.applied.clone();
    if query.is_empty() {
      self.search.index.clear();
      self.search.total = 0;
      self.search.indexed = None;
      return;
    }
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
    let inputs: Vec<Indexed> = files
      .iter()
      .zip(&bodies)
      .map(|((path, _, _), body)| Indexed {
        path: path.clone(),
        conflicted: conflicted.contains(path),
        body: *body,
        hunks: self.shown_hunks(path).map(|d| d.hunks.clone()),
      })
      .collect();
    if self
      .search
      .indexed
      .as_ref()
      .is_some_and(|(q, last)| *q == query && *last == inputs)
    {
      return;
    }
    self.search.indexed = Some((query.clone(), inputs));
    self.search.index.clear();
    self.search.total = 0;
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
    self.search.index = index;
    self.search.total = total;
    if self.search.current >= total {
      self.search.current = 0;
    }
  }

  /// The current match's offset within `key`'s line, if it is there.
  fn current_in(&self, key: &MatchKey) -> Option<usize> {
    let (first, n) = *self.search.index.get(key)?;
    let current = self.search.current;
    (current >= first && current < first + n).then(|| current - first)
  }

  /// A line's text with its matches marked (`mark.search-match`).
  pub(super) fn marked_text(
    &self,
    key: MatchKey,
    text: &str,
    language: Option<&'static str>,
    changed: &[(Range<usize>, Hsla)],
  ) -> AnyElement {
    if !self.search.index.contains_key(&key) {
      return highlight::styled_marked(text, language, self.dark, changed).into_any_element();
    }
    let current = self.current_in(&key);
    let primary = theme::palette(self.dark).primary;
    // Matches first: where one overlaps a changed run, it shows.
    let marks: Vec<(Range<usize>, Hsla)> = match_ranges(text, &self.search.applied)
      .into_iter()
      .enumerate()
      .map(|(i, r)| {
        (
          r,
          primary.opacity(if Some(i) == current { 0.6 } else { 0.3 }),
        )
      })
      .chain(changed.iter().cloned())
      .collect();
    let styled: StyledText = highlight::styled_marked(text, language, self.dark, &marks);
    if current.is_none() {
      return styled.into_any_element();
    }
    let target = self.search.target.clone();
    div()
      .relative()
      .child(styled)
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
    if !self.search.reveal {
      return;
    }
    let Some(key) = self
      .search
      .index
      .iter()
      .find(|(k, _)| self.current_in(k).is_some())
      .map(|(k, _)| k.clone())
    else {
      self.search.reveal = false;
      return;
    };
    let drawn = self.search.target.borrow().clone();
    match drawn {
      Some((k, line)) if k == key => {
        let view = self.diff.list.viewport_bounds();
        self.diff.list.scroll_by(line.center().y - view.center().y);
        self.search.reveal = false;
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
  pub(super) fn search_bar(&self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    if !self.search.open {
      return None;
    }
    let input = self.search.input.clone()?;
    let empty = self.search.query.trim().is_empty();
    let shown = if self.search.total > 0 {
      self.search.current + 1
    } else {
      0
    };
    let muted = p.muted_foreground;
    Some(
      div()
        .id("diff-search")
        .debug_selector(|| "diff-search".into())
        .absolute()
        .top_2()
        .right_2()
        .bg(p.background)
        .border_1()
        .border_color(p.border)
        .rounded(radius::MD)
        .shadow_lg()
        .p_0p5()
        .flex()
        .items_center()
        .gap_0p5()
        .capture_action(cx.listener(|view, _: &Escape, _, cx| {
          view.close_search(cx);
          cx.stop_propagation();
        }))
        .child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "diff-search-input".into())
            .h_6()
            .w_48()
            .px_2()
            .flex()
            .items_center()
            .rounded(radius::MD)
            .border_1()
            .border_color(p.input)
            .child(ui::input(&input).appearance(false).bordered(false)),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .debug_selector(|| "diff-search-count".into())
            .px_2()
            .whitespace_nowrap()
            .text_color(muted)
            .child(format!("{shown} of {}", self.search.total)),
        )
        .child(
          self.tip(
            "diff-search-prev-tip".into(),
            "Previous (Shift+Enter)",
            ui::IconButton::new(
              "diff-search-prev",
              theme::ICON_CHEVRON_UP,
              "Previous match",
              p,
            )
            .size(ui::IconButtonSize::Small)
            .disabled(empty)
            .when(!empty, |this| {
              this.on_click(cx.listener(|view, _, _, cx| view.step_match(-1, cx)))
            }),
            cx,
          ),
        )
        .child(
          self.tip(
            "diff-search-next-tip".into(),
            "Next (Enter)",
            ui::IconButton::new(
              "diff-search-next",
              theme::ICON_CHEVRON_DOWN,
              "Next match",
              p,
            )
            .size(ui::IconButtonSize::Small)
            .disabled(empty)
            .when(!empty, |this| {
              this.on_click(cx.listener(|view, _, _, cx| view.step_match(1, cx)))
            }),
            cx,
          ),
        )
        .child(
          self.tip(
            "diff-search-close-tip".into(),
            "Close (Esc)",
            ui::IconButton::new("diff-search-close", theme::ICON_X, "Close search", p)
              .size(ui::IconButtonSize::Small)
              .on_click(cx.listener(|view, _, _, cx| view.close_search(cx))),
            cx,
          ),
        ),
    )
  }
}

#[cfg(test)]
mod tests {
  use super::{match_count, match_ranges};

  #[test]
  fn matches_ignore_case_and_do_not_overlap() {
    assert_eq!(match_ranges("Logout logout", "LOG"), vec![0..3, 7..10]);
    assert_eq!(match_ranges("aaaa", "aa"), vec![0..2, 2..4]);
    assert_eq!(
      match_ranges("abc", ""),
      Vec::<std::ops::Range<usize>>::new()
    );
    assert_eq!(
      match_ranges("ab", "abc"),
      Vec::<std::ops::Range<usize>>::new()
    );
  }

  #[test]
  fn match_count_agrees_with_match_ranges() {
    for (text, query) in [
      ("Logout logout", "LOG"),
      ("aaaa", "aa"),
      ("aaa", "aa"),
      ("abc", ""),
      ("ab", "abc"),
      ("é x É", "é"),
      ("let value_1 = VALUE_10", "value_1"),
    ] {
      assert_eq!(
        match_count(text, query),
        match_ranges(text, query).len(),
        "{text:?} / {query:?}"
      );
    }
  }

  #[test]
  fn matches_keep_byte_offsets_past_wide_characters() {
    assert_eq!(match_ranges("é x É", "é"), vec![0..2, 5..7]);
  }
}
