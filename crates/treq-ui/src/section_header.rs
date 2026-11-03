//! The header over a group of rows or content: a title in small capitals,
//! an optional [`Count`] of what it holds and at most one action.

use gpui::{div, prelude::*, AnyElement, App, SharedString, Window};
use gpui_kit::base::TestSupportExt as _;

use crate::{badge::caption, theme::Palette, Count};

/// A section's header. The title is written in sentence case and shows in
/// capitals; a screen reader reads it as written, as a heading.
#[derive(IntoElement)]
pub struct SectionHeader {
  id: SharedString,
  palette: Palette,
  title: SharedString,
  count: Option<usize>,
  action: Option<AnyElement>,
}

impl SectionHeader {
  pub fn new(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      title: title.into(),
      count: None,
      action: None,
    }
  }

  /// How many things the section holds.
  pub fn count(mut self, count: usize) -> Self {
    self.count = Some(count);
    self
  }

  /// The section's one action, at the end of the line. A second call
  /// replaces the first.
  pub fn action(mut self, action: impl IntoElement) -> Self {
    self.action = Some(action.into_any_element());
    self
  }
}

/// The title as it shows: in capitals.
fn display_title(title: &str) -> SharedString {
  title.to_uppercase().into()
}

impl RenderOnce for SectionHeader {
  fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = self.palette;
    let selector = self.id.to_string();
    let count_selector = format!("{}-count", self.id);
    div()
      .flex()
      .items_center()
      .gap_2()
      .child(
        caption(div(), cx)
          .id(self.id.clone())
          .debug_selector(move || selector)
          .role(gpui::Role::Heading)
          .aria_label(self.title.clone())
          .test_support()
          .min_w_0()
          .truncate()
          .font_weight(gpui::FontWeight::SEMIBOLD)
          .text_color(p.muted_foreground)
          .child(display_title(&self.title)),
      )
      .when_some(self.count, |el, count| {
        el.child(
          div()
            .debug_selector(move || count_selector)
            .child(Count::new(count, &p)),
        )
      })
      .when_some(self.action, |el, action| {
        el.child(div().flex_1()).child(action)
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme;

  #[test]
  fn test_section_header_builder() {
    let p = theme::palette(false);
    let header = SectionHeader::new("ssh-hosts", "SSH hosts", &p)
      .count(3)
      .action(gpui::div())
      .action(gpui::div());
    assert_eq!(header.title.as_ref(), "SSH hosts");
    assert_eq!(header.count, Some(3));
    // At most one action: a second replaces the first.
    assert!(header.action.is_some());
  }

  mod render {
    use gpui::{div, prelude::*, px, Context, TestAppContext, VisualTestContext, Window};

    use crate::{theme, SectionHeader};

    struct Host;
    impl Render for Host {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        div()
          .w(px(300.))
          .child(SectionHeader::new("hosts", "SSH hosts", &p).count(2))
      }
    }

    /// A screen reader reads it as a heading named by its title, as
    /// written, though it shows in capitals.
    #[gpui::test]
    fn the_header_is_a_heading_named_by_its_title(cx: &mut TestAppContext) {
      let window = cx.add_window(|_, _| Host);
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      assert!(cx.debug_bounds("hosts").is_some());
      assert!(cx.debug_bounds("hosts-count").is_some());
      assert_eq!(
        super::super::display_title("SSH hosts").as_ref(),
        "SSH HOSTS"
      );
    }
  }
}
