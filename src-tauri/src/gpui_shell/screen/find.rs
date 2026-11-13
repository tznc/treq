//! The find bar shared by the code views (`useDiffSearch`, `SearchOverlay`):
//! Cmd+F opens it, the view marks the query's matches, and Enter steps
//! through them. A view keeps a [`Find`] and says, through [`Findable`],
//! how it counts and shows its matches.

use std::ops::Range;
use std::time::Duration;

use gpui_kit::component::input::{Escape, InputEvent, InputState};
use tauri_plugin_gpui::gpui::{
  div, prelude::*, Context, Div, Entity, Hsla, SharedString, Stateful, Subscription, Task, Window,
};

use crate::gpui_shell::{
  popup::{Side, TooltipContent},
  theme::{self, Palette},
  tooltips::HasTooltips,
  ui::{self, radius, TextSize},
};
use crate::result_ext::ResultExt;

/// `useDebounce(searchQuery, 150)`, for text too long to search on every key.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// The bar's state: whether it shows, the query, and where in the matches.
#[derive(Default)]
pub(in crate::gpui_shell::screen) struct Find {
  pub(in crate::gpui_shell::screen) open: bool,
  /// What is typed.
  pub(in crate::gpui_shell::screen) query: String,
  /// The query after the debounce: what is matched.
  pub(in crate::gpui_shell::screen) applied: String,
  pub(in crate::gpui_shell::screen) current: usize,
  /// How many matches the view counted for `applied`.
  pub(in crate::gpui_shell::screen) total: usize,
  input: Option<Entity<InputState>>,
  focus: bool,
  debounce: Option<Task<()>>,
  _subscription: Option<Subscription>,
  /// Cmd+F anywhere in the window, while the view shows.
  _find_key: Option<Subscription>,
}

/// A view with a find bar.
pub(in crate::gpui_shell::screen) trait Findable:
  HasTooltips + Sized + 'static
{
  fn find(&mut self) -> &mut Find;
  /// Whether Cmd+F opens the bar now.
  fn can_find(&self) -> bool {
    true
  }
  /// Match on every key rather than after a pause: the text is short.
  fn find_instantly(&self) -> bool {
    false
  }
  /// The applied query changed: count the matches into `Find::total`.
  fn query_applied(&mut self, _cx: &mut Context<Self>) {}
  /// The current match changed: bring it into view.
  fn reveal_current(&mut self, _cx: &mut Context<Self>) {}
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
/// case, are counted in place: an index counts every line.
pub(in crate::gpui_shell::screen) fn match_count(text: &str, query: &str) -> usize {
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

impl Find {
  /// The current match's offset within a run of matches starting at
  /// `first`, `n` long, if it is in it.
  pub(in crate::gpui_shell::screen) fn current_in(&self, first: usize, n: usize) -> Option<usize> {
    (first..first + n)
      .contains(&self.current)
      .then(|| self.current - first)
  }

  /// `text`'s matches, the first of them overall at `first`, painted; the
  /// current one stronger.
  pub(in crate::gpui_shell::screen) fn marks(
    &self,
    text: &str,
    first: usize,
    primary: Hsla,
  ) -> Vec<(Range<usize>, Hsla)> {
    match_ranges(text, &self.applied)
      .into_iter()
      .enumerate()
      .map(|(i, r)| {
        let current = first + i == self.current;
        (r, primary.opacity(if current { 0.6 } else { 0.3 }))
      })
      .collect()
  }

  /// Sets the count, keeping the current match within it.
  pub(in crate::gpui_shell::screen) fn set_total(&mut self, total: usize) {
    self.total = total;
    if self.current >= total {
      self.current = 0;
    }
  }
}

/// Listens for Cmd+F in the view's window, focused or not.
pub(in crate::gpui_shell::screen) fn watch_key<V: Findable>(
  view: &mut V,
  window: &mut Window,
  cx: &mut Context<V>,
) {
  if view.find()._find_key.is_some() {
    return;
  }
  let id = window.window_handle().window_id();
  view.find()._find_key = Some(
    cx.observe_keystrokes(move |view: &mut V, event, window, cx| {
      let k = &event.keystroke;
      let held = crate::gpui_shell::keys::held(k.modifiers);
      if window.window_handle().window_id() == id
        && k.key == "f"
        && (held.platform || held.control)
        && !held.shift
        && !held.alt
      {
        open(view, cx);
      }
    }),
  );
}

/// Cmd+F is the shown view's: a hidden one stops listening, and listens
/// again once drawn.
pub(in crate::gpui_shell::screen) fn unwatch_key<V: Findable>(view: &mut V) {
  view.find()._find_key = None;
}

/// Opens the bar, or selects its text again.
pub(in crate::gpui_shell::screen) fn open<V: Findable>(view: &mut V, cx: &mut Context<V>) {
  if !view.can_find() {
    return;
  }
  let find = view.find();
  find.open = true;
  find.focus = true;
  cx.notify();
}

pub(in crate::gpui_shell::screen) fn close<V: Findable>(view: &mut V, cx: &mut Context<V>) {
  let find = view.find();
  *find = Find {
    _find_key: find._find_key.take(),
    ..Find::default()
  };
  view.query_applied(cx);
  cx.notify();
}

fn set_query<V: Findable>(view: &mut V, query: String, cx: &mut Context<V>) {
  // Enter in the input reports a change too: the same text keeps its place.
  if view.find().query == query {
    return;
  }
  view.find().query = query.clone();
  view.find().current = 0;
  if view.find_instantly() {
    view.find().debounce = None;
    view.find().applied = query;
    view.query_applied(cx);
    cx.notify();
    return;
  }
  view.find().debounce = Some(cx.spawn(async move |this, cx| {
    cx.background_executor().timer(DEBOUNCE).await;
    this
      .update(cx, |view: &mut V, cx| {
        view.find().applied = query;
        view.find().current = 0;
        view.query_applied(cx);
        cx.notify();
      })
      .ignore_closed();
  }));
  cx.notify();
}

pub(in crate::gpui_shell::screen) fn step<V: Findable>(
  view: &mut V,
  delta: isize,
  cx: &mut Context<V>,
) {
  let find = view.find();
  let total = find.total as isize;
  if total == 0 {
    return;
  }
  find.current = (find.current as isize + delta).rem_euclid(total) as usize;
  view.reveal_current(cx);
  cx.notify();
}

/// Builds the input once the bar opens, and focuses it when asked.
pub(in crate::gpui_shell::screen) fn ensure_input<V: Findable>(
  view: &mut V,
  window: &mut Window,
  cx: &mut Context<V>,
) {
  if !view.find().open {
    return;
  }
  let input = match &view.find().input {
    Some(input) => input.clone(),
    None => {
      let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
      let subscription =
        cx.subscribe(
          &input,
          |view: &mut V, input, event: &InputEvent, cx| match event {
            InputEvent::Change => {
              let query = input.read(cx).value().to_string();
              set_query(view, query, cx);
            }
            InputEvent::PressEnter { shift, .. } => step(view, if *shift { -1 } else { 1 }, cx),
            _ => {}
          },
        );
      let find = view.find();
      find._subscription = Some(subscription);
      find.input = Some(input.clone());
      input
    }
  };
  if std::mem::take(&mut view.find().focus) {
    input.update(cx, |input, cx| {
      input.focus(window, cx);
      input.select_all(window, cx);
    });
  }
}

/// The bar, top right of the view's content; its parts' selectors start
/// with `id` (`{id}-input`, `{id}-count`, `{id}-prev`, `{id}-next`,
/// `{id}-close`).
pub(in crate::gpui_shell::screen) fn bar<V: Findable>(
  view: &mut V,
  id: &'static str,
  p: &Palette,
  cx: &mut Context<V>,
) -> Option<Stateful<Div>> {
  let find = view.find();
  if !find.open {
    return None;
  }
  let input = find.input.clone()?;
  let empty = find.query.trim().is_empty();
  let total = find.total;
  let shown = if total > 0 { find.current + 1 } else { 0 };
  let mut button =
    |part: &str, icon, label: &'static str, tip: &'static str, delta: Option<isize>| {
      let button = ui::IconButton::new(SharedString::from(format!("{id}-{part}")), icon, label, p)
        .size(ui::IconButtonSize::Small);
      let button = match delta {
        Some(delta) => button.disabled(empty).when(!empty, |this| {
          this.on_click(cx.listener(move |view, _, _, cx| step(view, delta, cx)))
        }),
        None => button.on_click(cx.listener(|view, _, _, cx| close(view, cx))),
      };
      view.tooltips().wrap(
        format!("{id}-{part}-tip"),
        Side::Top,
        Some(TooltipContent::Text(tip.into())),
        button,
        cx,
      )
    };
  let prev = button(
    "prev",
    theme::ICON_CHEVRON_UP,
    "Previous match",
    "Previous (Shift+Enter)",
    Some(-1),
  );
  let next = button(
    "next",
    theme::ICON_CHEVRON_DOWN,
    "Next match",
    "Next (Enter)",
    Some(1),
  );
  let close_button = button("close", theme::ICON_X, "Close search", "Close (Esc)", None);
  Some(
    div()
      .id(id)
      .debug_selector(move || id.into())
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
      .font_family(theme::SANS_FONT)
      .capture_action(cx.listener(|view, _: &Escape, _, cx| {
        close(view, cx);
        cx.stop_propagation();
      }))
      .child(
        ui::text(div(), TextSize::Sm)
          .debug_selector(move || format!("{id}-input"))
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
        // Short label (['count']): 11px is fine.
        // ast-grep-ignore: gpui-text-xs-short-label-only
        ui::text(div(), TextSize::Xs)
          .debug_selector(move || format!("{id}-count"))
          .px_2()
          .whitespace_nowrap()
          .text_color(p.muted_foreground)
          .child(format!("{shown} of {total}")),
      )
      .child(prev)
      .child(next)
      .child(close_button),
  )
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
