//! Small layout pieces shared by the screens, sized for touch.

use std::sync::{Arc, LazyLock};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::Button,
    h_flex,
    text::SelectionFormat,
    text::{TextView, TextViewStyle},
    v_flex, ActiveTheme, IconName,
  },
  div, img, px, relative, AnyElement, App, Div, ElementId, FontWeight, Image, ImageFormat, Img,
  InteractiveElement, IntoElement, ListAlignment, ListState, ParentElement, SharedString, Stateful,
  StatefulInteractiveElement, Styled,
};

use treq_ui::press::{self, press_overlay, pressable, Press};

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

/// Minimum height of a tappable row.
pub const ROW_H: f32 = 56.;

/// Height of a full-width call-to-action button (gpui-kit's large is 32px).
pub const CTA_H: f32 = 52.;

/// A titled group of content.
pub fn section(title: impl Into<SharedString>, cx: &App) -> Div {
  v_flex().gap_2().child(
    div()
      .text_xs()
      .font_weight(FontWeight::SEMIBOLD)
      .text_color(cx.theme().muted_foreground)
      .child(title.into().to_uppercase()),
  )
}

/// A bordered surface.
pub fn card(cx: &App) -> Div {
  v_flex()
    .rounded(cx.theme().radius_lg)
    .border_1()
    .border_color(cx.theme().border)
    .bg(cx.theme().background)
    .overflow_hidden()
    // A card with overflow_hidden may otherwise shrink below its rows.
    .flex_shrink_0()
}

/// A tappable list row with a title, optional detail line and a chevron.
pub fn row(
  id: impl Into<ElementId>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  cx: &App,
) -> Stateful<Div> {
  row_with(id, title, detail, None, cx)
}

/// [`row`] with an extra control before the chevron.
pub fn row_with(
  id: impl Into<ElementId>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  trailing: Option<AnyElement>,
  cx: &App,
) -> Stateful<Div> {
  row_full(id, None, title, detail, trailing, cx)
}

/// [`row_with`] with a status (a dot or spinner) before the title.
pub fn row_full(
  id: impl Into<ElementId>,
  leading: Option<AnyElement>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  trailing: Option<AnyElement>,
  cx: &App,
) -> Stateful<Div> {
  let chevron = div()
    .text_color(cx.theme().muted_foreground)
    .child(IconName::ChevronRight)
    .into_any_element();
  row_base(id, leading, title, detail, trailing, Some(chevron), cx)
}

/// One option of a single choice: no chevron (it goes nowhere), a check
/// and the selected background on the chosen one.
pub fn choice(
  id: impl Into<ElementId>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  selected: bool,
  cx: &App,
) -> Stateful<Div> {
  let check = div()
    .w(px(16.))
    .when(selected, |c| c.child(IconName::Check))
    .into_any_element();
  row_base(id, None, title, detail, None, Some(check), cx).when(selected, |row| {
    row.bg(treq_ui::theme::current(cx).selected())
  })
}

fn row_base(
  id: impl Into<ElementId>,
  leading: Option<AnyElement>,
  title: impl Into<SharedString>,
  detail: Option<SharedString>,
  trailing: Option<AnyElement>,
  end: Option<AnyElement>,
  cx: &App,
) -> Stateful<Div> {
  let theme = cx.theme();
  h_flex()
    .id(id)
    .min_h(px(ROW_H))
    .px_4()
    .py_2()
    .gap_3()
    .items_center()
    .relative()
    .active(|s| s.bg(theme.accent))
    .child(press_overlay())
    // The divider sits just below the row: on the next row's top edge, or
    // on the card's bottom border under the last row. It stops short of
    // the card's rounded corners, which the card's mask doesn't clip to.
    .child(
      div()
        .absolute()
        .bottom(px(-1.))
        .left(theme.radius_lg)
        .right(theme.radius_lg)
        .h(px(1.))
        .bg(theme.border),
    )
    .children(leading)
    // A block column: the title and detail stack without a nested flex
    // container, which costs far less to lay out in a long list.
    .child(
      div()
        .flex_1()
        .min_w_0()
        .child(div().truncate().child(title.into()))
        .children(detail.map(|d| {
          div()
            .text_sm()
            .truncate()
            .text_color(theme.muted_foreground)
            .child(d)
        })),
    )
    .children(trailing)
    .children(end)
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
  let theme = cx.theme();
  div().px_4().child(
    v_flex()
      .border_l_1()
      .border_r_1()
      .border_color(theme.border)
      .bg(theme.background)
      .overflow_hidden()
      .when(first, |c| {
        c.border_t_1()
          .rounded_tl(theme.radius_lg)
          .rounded_tr(theme.radius_lg)
      })
      .when(last, |c| {
        c.border_b_1()
          .rounded_bl(theme.radius_lg)
          .rounded_br(theme.radius_lg)
      })
      .child(child),
  )
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
  div().text_sm().text_color(cx.theme().danger).child(
    TextView::markdown(id, escape_markdown(&text))
      .selectable(true)
      .selection_format(SelectionFormat::Plain),
  )
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

pub fn centered_message(text: impl Into<SharedString>, cx: &App) -> Div {
  div().py_6().px_4().text_center().child(muted(text, cx))
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

#[cfg(test)]
mod tests {
  use std::{cell::Cell, rc::Rc};

  use gpui_kit::{
    component::{
      button::{Button, ButtonVariants as _},
      Sizable as _,
    },
    div, point, px, size, AnyWindowHandle, AppContext as _, Bounds, Context, IntoElement,
    ParentElement as _, PlatformInput, Point, Render, TestAppContext, TouchEvent, TouchId,
    TouchPhase, Window, WindowBounds, WindowOptions,
  };

  use gpui_kit::test::TestWindowExt as _;

  use super::TouchButton as _;

  struct Host {
    clicks: Rc<Cell<usize>>,
  }

  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      let clicks = self.clicks.clone();
      div().child(
        Button::new("small")
          .ghost()
          .small()
          .label("Go")
          .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
          .touch(),
      )
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
}
