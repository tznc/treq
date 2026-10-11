//! Small layout pieces shared by the screens, sized for touch.

use std::sync::{Arc, LazyLock};

use gpui_kit::{
  component::{h_flex, v_flex, ActiveTheme, IconName},
  *,
};

use super::theme::BRAND_BLUE;

// The blue "T" mark, shared with the website and desktop app.
const LOGO_PNG: &[u8] = include_bytes!("../../../assets/logo.imageset/logo@3x.png");

// Decoded once: GPUI caches the texture by the image's id.
static LOGO: LazyLock<Arc<Image>> =
  LazyLock::new(|| Arc::new(Image::from_bytes(ImageFormat::Png, LOGO_PNG.to_vec())));

/// The Treq mark, `size` logical pixels square.
pub fn logo(size: f32) -> Img {
  img(LOGO.clone()).size(px(size))
}

/// The lowercase "treq" wordmark in bold brand-blue monospace.
pub fn wordmark(size: f32, cx: &App) -> Div {
  div()
    .font_family(cx.theme().mono_font_family.clone())
    .font_weight(FontWeight::BOLD)
    .text_size(px(size))
    .line_height(relative(1.))
    .text_color(rgb(BRAND_BLUE))
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
