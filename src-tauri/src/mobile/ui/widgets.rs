//! Small layout pieces shared by the screens, sized for touch.

use std::sync::{Arc, LazyLock};

use gpui_kit::{
  component::{
    button::Button,
    text::SelectionFormat,
    text::{TextView, TextViewStyle},
    v_flex, ActiveTheme, IconName,
  },
  div, img, px, relative, AnyElement, App, ClickEvent, Div, ElementId, FontWeight, Image,
  ImageFormat, Img, InteractiveElement, IntoElement, ListAlignment, ListState, ParentElement,
  SharedString, StatefulInteractiveElement, Styled, Window,
};

use gpui_kit::base::TestSupportExt as _;
use treq_ui::press::{self, pressable, Press};
use treq_ui::{EmptyState, Row, RowDensity, SectionHeader};

// The blue "T" mark, shared with the website and desktop app.
const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/logo.imageset/logo@3x.png");

// Decoded once: GPUI caches the texture by the image's id.
static LOGO: LazyLock<Arc<Image>> =
  LazyLock::new(|| Arc::new(Image::from_bytes(ImageFormat::Png, LOGO_PNG.to_vec())));

/// The Treq mark, `size` logical pixels square.
pub fn logo(size: f32) -> Img {
  img(LOGO.clone()).size(px(size))
}

/// The lowercase "treq" wordmark in bold primary-blue monospace.
pub fn wordmark(size: f32, cx: &App) -> Div {
  div()
    .font_family(cx.theme().mono_font_family.clone())
    .font_weight(FontWeight::BOLD)
    .text_size(px(size))
    .line_height(relative(1.))
    .text_color(treq_ui::theme::current(cx).primary)
    .child("treq")
}

/// Minimum height of a tappable row: treq-ui's touch row.
pub const ROW_H: f32 = treq_ui::TOUCH_ROW_HEIGHT;

/// Height of a full-width call-to-action button (gpui-kit's large is 32px).
pub const CTA_H: f32 = treq_ui::theme::CTA_HEIGHT;

/// `el` at caption size, the smallest text on a phone (13px).
pub fn caption<E: Styled>(el: E) -> E {
  let size = treq_ui::theme::Density::Touch.caption();
  el.text_size(px(size)).line_height(px(size + 5.))
}

/// [`caption`] as a method, for builder chains.
pub trait Caption: Styled + Sized {
  fn caption(self) -> Self {
    caption(self)
  }
}
impl<E: Styled> Caption for E {}

/// The bar pinned under a page that holds its main action.
pub fn bottom_bar(cx: &App) -> Div {
  treq_ui::touch::bottom_bar(&treq_ui::theme::current(cx))
}

/// A titled group of content, under a [`treq_ui::SectionHeader`].
pub fn section(title: impl Into<SharedString>, cx: &App) -> Div {
  section_with(title, |header| header, cx)
}

/// [`section`] with a header `build` adds a count or an action to.
pub fn section_with(
  title: impl Into<SharedString>,
  build: impl FnOnce(SectionHeader) -> SectionHeader,
  cx: &App,
) -> Div {
  let title: SharedString = title.into();
  let id = SharedString::from(format!("section-{}", slug(&title)));
  v_flex().gap_2().child(build(SectionHeader::new(
    id,
    title,
    &treq_ui::theme::current(cx),
  )))
}

/// `title` in lowercase words joined by dashes, for an element id.
fn slug(title: &str) -> String {
  title
    .split(|c: char| !c.is_alphanumeric())
    .filter(|w| !w.is_empty())
    .map(str::to_lowercase)
    .collect::<Vec<_>>()
    .join("-")
}

/// A bordered surface.
pub fn card(cx: &App) -> Div {
  treq_ui::touch::grouped(&treq_ui::theme::current(cx))
}

/// A tappable list row with a title, optional detail line and a chevron:
/// a touch [`Row`] with a divider, as the rows of a [`card`] are.
pub fn row(
  id: impl Into<SharedString>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  cx: &App,
) -> Row {
  row_with(id, title, detail, None, cx)
}

/// [`row`] with an extra control before the chevron.
pub fn row_with(
  id: impl Into<SharedString>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  trailing: Option<AnyElement>,
  cx: &App,
) -> Row {
  row_full(id, None, title, detail, trailing, cx)
}

/// [`row_with`] with a status (a dot or spinner) before the title.
pub fn row_full(
  id: impl Into<SharedString>,
  leading: Option<AnyElement>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  trailing: Option<AnyElement>,
  cx: &App,
) -> Row {
  let chevron = div()
    .text_color(cx.theme().muted_foreground)
    .child(IconName::ChevronRight);
  touch_row(id, leading, title, detail, trailing, cx).trailing(chevron)
}

/// One option of a single choice: no chevron (it goes nowhere), a check
/// and the selected fill on the chosen one.
pub fn choice(
  id: impl Into<SharedString>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  selected: bool,
  cx: &App,
) -> Row {
  let row = treq_ui::touch::choice(id, title, selected, &treq_ui::theme::current(cx));
  match detail {
    Some(detail) => row.meta(detail),
    None => row,
  }
}

fn touch_row(
  id: impl Into<SharedString>,
  leading: Option<AnyElement>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  trailing: Option<AnyElement>,
  cx: &App,
) -> Row {
  let mut row = Row::new(id, &treq_ui::theme::current(cx))
    .density(RowDensity::Touch)
    .divider(true)
    .title(title);
  if let Some(detail) = detail {
    row = row.meta(detail);
  }
  if let Some(leading) = leading {
    row = row.leading(leading);
  }
  if let Some(trailing) = trailing {
    row = row.trailing(trailing);
  }
  row
}

/// The state of a page-long virtualized list, which only builds and lays
/// out the rows near the screen.
pub fn list_state() -> ListState {
  ListState::new(0, ListAlignment::Top, px(ROW_H * 4.))
}

/// Sets the number of items without forgetting the measured heights (or the
/// scroll position) of the ones that stay.
pub fn resize(state: &ListState, count: usize) {
  let old = state.item_count();
  if count > old {
    state.splice(old..old, count - old);
  } else if count < old {
    state.splice(count..old, 0);
  }
}

/// One item of a bordered card that spans several list items: the side
/// borders of every item, the top of the `first` and the bottom of the `last`
/// with its rounded corners.
pub fn card_item(child: impl IntoElement, first: bool, last: bool, cx: &App) -> Div {
  treq_ui::touch::grouped_item(child, first, last, &treq_ui::theme::current(cx))
}

pub fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
  div()
    .text_sm()
    .text_color(cx.theme().muted_foreground)
    .child(text.into())
}

/// An error, selectable with a long press so it can be copied whole.
pub fn error_text(text: impl Into<SharedString>, cx: &App) -> Div {
  let text: SharedString = text.into();
  let id = SharedString::from(format!("error-{:x}", hash(&text)));
  // An alert, so a screen reader announces the error as it appears.
  let alert = div()
    .id(SharedString::from(format!("{id}-alert")))
    .role(gpui_kit::Role::Alert)
    .aria_label(text.clone())
    .child(
      TextView::markdown(id, escape_markdown(&text))
        .selectable(true)
        .selection_format(SelectionFormat::Plain),
    )
    .test_support();
  div().text_sm().text_color(cx.theme().danger).child(alert)
}

fn hash(text: &str) -> u64 {
  use std::hash::{Hash, Hasher};
  let mut hasher = std::collections::hash_map::DefaultHasher::new();
  text.hash(&mut hasher);
  hasher.finish()
}

/// `text` as Markdown that renders as itself.
fn escape_markdown(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  for c in text.chars() {
    if c.is_ascii_punctuation() {
      out.push('\\');
    }
    out.push(c);
  }
  out
}

/// Touch sizing and press feedback for a button.
pub trait TouchButton {
  /// At least 44pt square, and darkened while a finger is on it.
  fn touch(self) -> Press;
}

impl TouchButton for Button {
  fn touch(mut self) -> Press {
    let id = self
      .interactivity()
      .element_id
      .clone()
      .unwrap_or_else(|| "button".into());
    let min = px(press::MIN_TOUCH_TARGET);
    pressable(id, self.min_h(min).min_w(min)).rounded(px(6.))
  }
}

/// Monospace text that keeps whitespace, for diffs and logs.
pub fn mono(text: impl Into<SharedString>, cx: &App) -> Div {
  div()
    .font_family(cx.theme().mono_font_family.clone())
    .text_size(cx.theme().mono_font_size)
    .whitespace_nowrap()
    .child(text.into())
}

/// Vertical page content with standard padding.
pub fn page() -> Div {
  v_flex().gap_5().p_4()
}

/// A first load: skeleton rows the height of the rows they stand in for,
/// shown only once the load runs past the spinner delay.
pub fn skeleton(id: impl Into<SharedString>, rows: usize, cx: &App) -> AnyElement {
  let id: SharedString = id.into();
  let palette = treq_ui::theme::current(cx);
  let selector = format!("{id}-skeleton");
  treq_ui::delayed(
    SharedString::from(format!("{id}-delay")),
    div().debug_selector(move || selector).px_4().child(
      div()
        .id(SharedString::from(format!("{id}-skeleton")))
        .role(gpui_kit::Role::ProgressIndicator)
        .aria_label("Loading")
        .child(treq_ui::Skeleton::new(id, rows, &palette).row_height(ROW_H))
        .test_support(),
    ),
  )
  .into_any_element()
}

/// Nothing to show: a title and, when it helps, one sentence.
pub fn empty(id: impl Into<SharedString>, title: impl Into<SharedString>, cx: &App) -> EmptyState {
  EmptyState::new(id, title, &treq_ui::theme::current(cx))
}

/// A failed load: the readable sentence for `raw` and a Retry button.
pub fn failed(
  id: impl Into<SharedString>,
  raw: &str,
  retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
  cx: &App,
) -> EmptyState {
  let id: SharedString = id.into();
  let problem = crate::mobile::errors::describe(raw);
  empty(id.clone(), problem.message, cx)
    .error(true)
    .icon(treq_ui::theme::ICON_CIRCLE_ALERT)
    .action(
      Button::new(SharedString::from(format!("{id}-retry")))
        .outline()
        .label("Retry")
        .on_click(retry)
        .touch(),
    )
}

/// GitHub-flavoured Markdown, as issue and pull request text is written;
/// selectable, so a phrase can be copied.
pub fn markdown(id: impl Into<ElementId>, text: &str, cx: &App) -> Div {
  let style = TextViewStyle {
    is_dark: cx.theme().mode.is_dark(),
    ..TextViewStyle::default()
  };
  div().child(
    TextView::markdown(id, SharedString::from(text.to_string()))
      .style(style)
      .selectable(true),
  )
}

/// A 412x892 window showing the view `build` makes, drawn once and past
/// the spinner delay.
/// The Filters button's label: the set filters' count beside it.
pub fn filters_label(count: usize) -> String {
  if count == 0 {
    "Filters".into()
  } else {
    format!("Filters · {count}")
  }
}

/// A list's filter row, as desktop's GitHub toolbar: the state as a value
/// with a caret, opening a menu of states, and Filters, opening the
/// advanced filters.
pub fn filter_row(
  id: &str,
  state: &str,
  on_state: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
  count: usize,
  on_filters: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
  gpui_kit::component::h_flex()
    .gap_2()
    .child(
      Button::new(SharedString::from(format!("{id}-state")))
        .outline()
        .h(px(treq_ui::theme::TOUCH_TARGET))
        .label(state.to_string())
        .dropdown_caret(true)
        .accessibility_label(format!("State: {state}"))
        .on_click(on_state)
        .touch(),
    )
    .child(
      Button::new(SharedString::from(format!("{id}-filters")))
        .outline()
        .h(px(treq_ui::theme::TOUCH_TARGET))
        .label(filters_label(count))
        .on_click(on_filters)
        .touch(),
    )
}

/// What a Filters sheet entry does with the value picked (`None` for Any).
pub type OnPick = std::rc::Rc<dyn Fn(Option<String>, &mut App)>;

/// A Filters sheet entry: "Author: ada", opening a menu of "Any" and
/// `values` that hands the pick to `on_pick` (`None` for Any).
pub fn filter_field(
  id: &'static str,
  name: &'static str,
  current: Option<String>,
  values: Vec<String>,
  on_pick: OnPick,
) -> super::sheet::MenuEntry {
  let label = format!("{name}: {}", current.as_deref().unwrap_or("Any"));
  super::sheet::MenuEntry::new(id, label, move |_, cx| {
    let entries = std::iter::once(None)
      .chain(values.iter().cloned().map(Some))
      .enumerate()
      .map(|(i, value)| {
        let on_pick = on_pick.clone();
        let selected = value == current;
        let label = value.clone().unwrap_or_else(|| "Any".into());
        super::sheet::MenuEntry::new(format!("{id}-{i}"), label, move |_, cx| {
          on_pick(value.clone(), cx)
        })
        .selected(selected)
      })
      .collect();
    super::sheet::menu(name, entries, cx);
  })
}

/// What the composer bar reads: an invitation, or the first line of the
/// text not yet sent (cut at 40 characters).
pub fn composer_hint(text: &str) -> String {
  let line = text.trim().lines().next().unwrap_or("").trim();
  if line.is_empty() {
    return "Add a comment…".into();
  }
  if line.chars().count() > 40 {
    format!("{}…", line.chars().take(40).collect::<String>())
  } else {
    line.to_string()
  }
}

/// The one-line bar at the foot of an issue or pull request: tapping it
/// opens the composer sheet. It shows unsent text, so none is lost from view.
pub fn composer_bar(
  id: &str,
  text: &str,
  on_tap: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
  cx: &App,
) -> Press {
  let theme = cx.theme();
  let unsent = !text.trim().is_empty();
  let hint = composer_hint(text);
  Button::new(SharedString::from(id.to_string()))
    .outline()
    .w_full()
    .h(px(treq_ui::theme::TOUCH_TARGET))
    .rounded(px(22.))
    .justify_start()
    .accessibility_label(if unsent {
      format!("Continue comment: {hint}")
    } else {
      hint.clone()
    })
    .child(
      div()
        .flex_1()
        .min_w_0()
        .px_1()
        .text_left()
        .truncate()
        .text_color(if unsent {
          theme.foreground
        } else {
          theme.muted_foreground
        })
        .child(hint),
    )
    .on_click(on_tap)
    .touch()
}

#[cfg(test)]
pub(crate) fn mount_for_test<V: gpui_kit::Render>(
  cx: &mut gpui_kit::TestAppContext,
  build: impl FnOnce(&mut Window, &mut gpui_kit::Context<V>) -> V + 'static,
) -> gpui_kit::AnyWindowHandle {
  use gpui_kit::test::TestWindowExt as _;
  use gpui_kit::{size, AppContext as _, Bounds, Point, WindowBounds, WindowOptions};
  cx.update(super::theme::init);
  let window = cx.update(|cx| {
    gpui_kit::open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
          origin: Point::default(),
          size: size(px(412.), px(892.)),
        })),
        ..Default::default()
      },
      cx,
      move |window, cx| cx.new(|cx| build(window, cx)),
    )
    .expect("open test window")
    .0
  });
  cx.update_window(window, |_, w, cx| w.render_frame(cx))
    .unwrap();
  cx.executor()
    .advance_clock(std::time::Duration::from_millis(400));
  cx.run_until_parked();
  cx.update_window(window, |_, w, cx| w.render_frame(cx))
    .unwrap();
  window
}

#[cfg(test)]
mod tests {
  #[core::prelude::v1::test]
  fn the_composer_bar_invites_a_comment_or_shows_the_unsent_text() {
    assert_eq!(super::composer_hint(""), "Add a comment…");
    assert_eq!(super::composer_hint("  \n "), "Add a comment…");
    assert_eq!(
      super::composer_hint("  Shipped in #418\nmore"),
      "Shipped in #418"
    );
    let long = "x".repeat(80);
    assert_eq!(super::composer_hint(&long).chars().count(), 41);
    assert!(super::composer_hint(&long).ends_with('…'));
  }

  #[core::prelude::v1::test]
  fn the_filters_button_counts_set_filters() {
    assert_eq!(super::filters_label(0), "Filters");
    assert_eq!(super::filters_label(2), "Filters · 2");
  }

  use std::{cell::Cell, rc::Rc};

  use gpui_kit::base::TextSelection;
  use gpui_kit::{
    component::{
      button::{Button, ButtonVariants as _},
      Sizable as _,
    },
    div, point, px, size, AnyWindowHandle, AppContext as _, Bounds, Context, IntoElement,
    ParentElement as _, PlatformInput, Point, Render, Styled as _, TestAppContext, TouchEvent,
    TouchId, TouchPhase, Window, WindowBounds, WindowOptions,
  };

  use gpui_kit::test::TestWindowExt as _;

  use super::TouchButton as _;

  const ERROR: &str = "The host refused the connection.";
  /// Where the error starts, below the 44pt button.
  const ERROR_Y: f32 = 100.;

  struct Host {
    clicks: Rc<Cell<usize>>,
  }

  impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
      let clicks = self.clicks.clone();
      div()
        .child(
          Button::new("small")
            .ghost()
            .small()
            .label("Go")
            .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
            .touch(),
        )
        .child(div().h(px(ERROR_Y - 44.)))
        .child(super::error_text(ERROR, cx))
    }
  }

  fn mount(cx: &mut TestAppContext) -> (AnyWindowHandle, Rc<Cell<usize>>) {
    cx.update(super::super::theme::init);
    let clicks = Rc::new(Cell::new(0));
    let window = cx.update(|cx| {
      let clicks = clicks.clone();
      gpui_kit::open_window(
        WindowOptions {
          window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(412.), px(892.)),
          })),
          ..Default::default()
        },
        cx,
        move |_, cx| cx.new(|_| Host { clicks }),
      )
      .expect("open test window")
      .0
    });
    cx.update_window(window, |_, w, cx| w.render_frame(cx))
      .unwrap();
    (window, clicks)
  }

  fn touch(cx: &mut TestAppContext, window: AnyWindowHandle, phase: TouchPhase, x: f32, y: f32) {
    cx.update_window(window, |_, w, cx| {
      w.dispatch_event(
        PlatformInput::Touch(TouchEvent {
          id: TouchId(1),
          phase,
          position: point(px(x), px(y)),
          predicted_position: None,
          force: None,
        }),
        cx,
      );
    })
    .unwrap();
  }

  /// A small button still takes a tap anywhere in its 44pt target.
  #[gpui_kit::test]
  fn a_tap_inside_the_touch_target_clicks(cx: &mut TestAppContext) {
    let (window, clicks) = mount(cx);
    touch(cx, window, TouchPhase::Started, 20., 40.);
    touch(cx, window, TouchPhase::Ended, 20., 40.);
    assert_eq!(clicks.get(), 1);
  }

  /// A finger that moves past the slop is a scroll, not a tap.
  #[gpui_kit::test]
  fn a_drag_does_not_click(cx: &mut TestAppContext) {
    let (window, clicks) = mount(cx);
    touch(cx, window, TouchPhase::Started, 20., 20.);
    touch(cx, window, TouchPhase::Moved, 20., 60.);
    touch(cx, window, TouchPhase::Ended, 20., 60.);
    assert_eq!(clicks.get(), 0);
  }

  /// A long press on an error selects its text.
  #[gpui_kit::test]
  fn a_long_press_selects_error_text(cx: &mut TestAppContext) {
    let (window, _) = mount(cx);
    touch(cx, window, TouchPhase::Started, 20., ERROR_Y + 8.);
    cx.executor()
      .advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
    touch(cx, window, TouchPhase::Ended, 20., ERROR_Y + 8.);
    let selected = cx
      .update_window(window, |_, w, cx| TextSelection::selected_text(w, cx))
      .unwrap();
    assert!(
      !selected.is_empty() && ERROR.contains(selected.trim()),
      "selected {selected:?}"
    );
  }

  /// A screen reader announces an error as it appears.
  #[gpui_kit::test]
  fn error_text_is_an_alert_named_by_its_text(cx: &mut TestAppContext) {
    use gpui_testing_library::prelude::*;
    let (window, _) = mount(cx);
    let screen = gpui_testing_library::Screen::new(cx, window);
    expect(screen.get_by_role(by::role(gpui_kit::Role::Alert).name(ERROR))).to_be_visible();
  }
}
