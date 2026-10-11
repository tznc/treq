//! Small layout pieces shared by the screens, sized for touch.

use gpui_kit::{
  component::{h_flex, v_flex, ActiveTheme, IconName},
  *,
};

/// Minimum height of a tappable row.
pub const ROW_H: f32 = 56.;

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
  let theme = cx.theme();
  h_flex()
    .id(id)
    .min_h(px(ROW_H))
    .px_4()
    .py_2()
    .gap_3()
    .items_center()
    .border_b_1()
    .border_color(theme.border)
    .active(|s| s.bg(theme.accent))
    .child(
      v_flex()
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
    .child(
      div()
        .text_color(theme.muted_foreground)
        .child(IconName::ChevronRight),
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
  div().py_6().flex().justify_center().child(muted(text, cx))
}
