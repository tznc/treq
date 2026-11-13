//! The Commits tab's find bar: Cmd+F opens it, matches in the commit
//! messages and the open diffs are marked, and Enter steps through them.

use std::ops::Range;

use gpui::Hsla;
use gpui_kit::component::input::Escape;

use super::*;
use crate::gpui_shell::screen::changes::search::{match_count, match_ranges};

/// Where a match is: a commit's headline, or a line of its open diff.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Hit {
  Message(String),
  Line(String, String, LineRef),
}

#[derive(Default)]
pub(in crate::gpui_shell::screen) struct Find {
  pub(super) open: bool,
  query: String,
  pub(in crate::gpui_shell::screen) current: usize,
  pub(in crate::gpui_shell::screen) total: usize,
  /// Matches by place: the first's index overall, how many, and its row.
  index: HashMap<Hit, (usize, usize, usize)>,
  /// The rows changed since `index` was built.
  pub(super) stale: bool,
  input: Option<Entity<InputState>>,
  focus: bool,
  reveal: bool,
  _subscription: Option<Subscription>,
  /// Cmd+F anywhere in the window, while the tab shows.
  _find_key: Option<Subscription>,
}

impl CommitsView {
  fn tip(
    &self,
    key: String,
    text: &'static str,
    el: impl IntoElement,
    cx: &mut Context<Self>,
  ) -> Stateful<Div> {
    self.tips.wrap(
      key,
      Side::Top,
      Some(TooltipContent::Text(text.into())),
      el,
      cx,
    )
  }

  /// Listens for Cmd+F in this view's window, focused or not.
  pub(super) fn watch_find_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.find._find_key.is_some() || self.hidden_at.is_some() {
      return;
    }
    let id = window.window_handle().window_id();
    self.find._find_key = Some(cx.observe_keystrokes(move |view, event, window, cx| {
      let k = &event.keystroke;
      let held = crate::gpui_shell::keys::held(k.modifiers);
      if window.window_handle().window_id() == id
        && k.key == "f"
        && (held.platform || held.control)
        && !held.shift
        && !held.alt
        && view.hidden_at.is_none()
      {
        view.find.open = true;
        view.find.focus = true;
        cx.notify();
      }
    }));
  }

  /// A hidden tab stops listening, and listens again once drawn.
  pub(super) fn unwatch_find_key(&mut self) {
    self.find._find_key = None;
  }

  fn close_find(&mut self, cx: &mut Context<Self>) {
    self.find = Find {
      _find_key: self.find._find_key.take(),
      ..Find::default()
    };
    cx.notify();
  }

  fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
    let total = self.find.total as isize;
    if total == 0 {
      return;
    }
    self.find.current = (self.find.current as isize + delta).rem_euclid(total) as usize;
    self.find.reveal = true;
    cx.notify();
  }

  /// Builds the input once the bar opens, and focuses it when asked.
  pub(super) fn ensure_find_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.find.open {
      return;
    }
    let input = match &self.find.input {
      Some(input) => input.clone(),
      None => {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        self.find._subscription = Some(cx.subscribe(
          &input,
          |view, input, event: &InputEvent, cx| match event {
            InputEvent::Change => {
              view.find.query = input.read(cx).value().to_string();
              view.find.current = 0;
              view.find.stale = true;
              cx.notify();
            }
            InputEvent::PressEnter { shift, .. } => view.step_find(if *shift { -1 } else { 1 }, cx),
            _ => {}
          },
        ));
        self.find.input = Some(input.clone());
        input
      }
    };
    if std::mem::take(&mut self.find.focus) {
      input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
      });
    }
  }

  /// Every match, in the list's order, once the rows or the query changed.
  pub(super) fn index_find(&mut self) {
    if !std::mem::take(&mut self.find.stale) {
      return;
    }
    self.find.index.clear();
    self.find.total = 0;
    if !self.find.open || self.find.query.is_empty() {
      return;
    }
    let query = self.find.query.clone();
    let (mut index, mut total) = (HashMap::new(), 0);
    self.searchable(|row, hit, text| {
      let n = match_count(text, &query);
      if n > 0 {
        index.insert(hit, (total, n, row));
        total += n;
      }
    });
    self.find.index = index;
    self.find.total = total;
    if self.find.current >= total {
      self.find.current = 0;
    }
  }

  /// Scrolls the current match's row to the top once stepped to.
  pub(super) fn reveal_find(&mut self) {
    if !std::mem::take(&mut self.find.reveal) {
      return;
    }
    let current = self.find.current;
    if let Some(&(_, _, row)) = self
      .find
      .index
      .values()
      .find(|(first, n, _)| (*first..first + n).contains(&current))
    {
      self.scroll_to_row(row);
    }
  }

  /// `text`'s matches at `hit`, the current one stronger.
  pub(super) fn find_marks(&self, hit: &Hit, text: &str, p: &Palette) -> Vec<(Range<usize>, Hsla)> {
    let Some(&(first, _, _)) = self.find.index.get(hit) else {
      return Vec::new();
    };
    match_ranges(text, &self.find.query)
      .into_iter()
      .enumerate()
      .map(|(i, r)| {
        let current = first + i == self.find.current;
        (r, p.primary.opacity(if current { 0.6 } else { 0.3 }))
      })
      .collect()
  }

  /// The bar, top right of the list.
  pub(super) fn find_bar(&self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    if !self.find.open {
      return None;
    }
    let input = self.find.input.clone()?;
    let empty = self.find.query.trim().is_empty();
    let shown = if self.find.total > 0 {
      self.find.current + 1
    } else {
      0
    };
    let button = |id: &'static str,
                  icon,
                  label: &'static str,
                  tip: &'static str,
                  delta: isize,
                  cx: &mut Context<Self>| {
      self.tip(
        format!("{id}-tip"),
        tip,
        ui::IconButton::new(id, icon, label, p)
          .size(ui::IconButtonSize::Small)
          .disabled(empty)
          .when(!empty, |this| {
            this.on_click(cx.listener(move |view, _, _, cx| view.step_find(delta, cx)))
          }),
        cx,
      )
    };
    let prev = button(
      "commits-search-prev",
      theme::ICON_CHEVRON_UP,
      "Previous match",
      "Previous (Shift+Enter)",
      -1,
      cx,
    );
    let next = button(
      "commits-search-next",
      theme::ICON_CHEVRON_DOWN,
      "Next match",
      "Next (Enter)",
      1,
      cx,
    );
    let close = self.tip(
      "commits-search-close-tip".into(),
      "Close (Esc)",
      ui::IconButton::new("commits-search-close", theme::ICON_X, "Close search", p)
        .size(ui::IconButtonSize::Small)
        .on_click(cx.listener(|view, _, _, cx| view.close_find(cx))),
      cx,
    );
    Some(
      div()
        .id("commits-search")
        .debug_selector(|| "commits-search".into())
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
          view.close_find(cx);
          cx.stop_propagation();
        }))
        .child(
          ui::text(div(), TextSize::Sm)
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
            .debug_selector(|| "commits-search-count".into())
            .px_2()
            .whitespace_nowrap()
            .text_color(p.muted_foreground)
            .child(format!("{shown} of {}", self.find.total)),
        )
        .child(prev)
        .child(next)
        .child(close),
    )
  }
}
