//! Small layout pieces shared by the screens, sized for touch.

use std::sync::{Arc, LazyLock};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    h_flex,
    text::{TextView, TextViewStyle},
    v_flex, ActiveTheme, IconName,
  },
  div, img, px, relative, AnyElement, App, Div, ElementId, FontWeight, Image, ImageFormat, Img,
  InteractiveElement, IntoElement, ListAlignment, ListState, ParentElement, SharedString, Stateful,
  StatefulInteractiveElement, Styled,
};

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

pub fn error_text(text: impl Into<SharedString>, cx: &App) -> Div {
  div()
    .text_sm()
    .text_color(cx.theme().danger)
    .child(text.into())
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
