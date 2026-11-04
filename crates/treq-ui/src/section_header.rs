//! The header over a group of rows or content: a title in small capitals,
//! an optional [`Count`] of what it holds and at most one action.

use std::rc::Rc;

use gpui::{div, prelude::*, AnyElement, App, ClickEvent, SharedString, Window};
use gpui_kit::base::TestSupportExt as _;

use crate::{
  badge::caption,
  theme::{self, Palette},
  Count,
};

type ToggleFn = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A collapsible section's toggle: its id, whether it is shut, and what a
/// click does.
struct Toggle {
  id: SharedString,
  collapsed: bool,
  on_toggle: ToggleFn,
}

/// A section's header. The title is written in sentence case and shows in
/// capitals; a screen reader reads it as written, as a heading.
#[derive(IntoElement)]
pub struct SectionHeader {
  id: SharedString,
  palette: Palette,
  title: SharedString,
  count: Option<usize>,
  action: Option<AnyElement>,
  icon: Option<&'static str>,
  note: Option<SharedString>,
  toggle: Option<Toggle>,
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
      icon: None,
      note: None,
      toggle: None,
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

impl SectionHeader {
  /// An icon before the title.
  pub fn icon(mut self, icon: &'static str) -> Self {
    self.icon = Some(icon);
    self
  }

  /// A muted note after the title, such as who the section is for.
  pub fn note(mut self, note: impl Into<SharedString>) -> Self {
    self.note = Some(note.into());
    self
  }

  /// Makes the section collapsible: a chevron and the title become the
  /// button `toggle_id`, which runs `on_toggle`.
  pub fn collapsible(
    mut self,
    toggle_id: impl Into<SharedString>,
    collapsed: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
  ) -> Self {
    self.toggle = Some(Toggle {
      id: toggle_id.into(),
      collapsed,
      on_toggle: Rc::new(on_toggle),
    });
    self
  }
}

/// What the toggle of the section `title` does when pressed.
fn toggle_label(title: &str, collapsed: bool) -> SharedString {
  let verb = if collapsed { "Expand" } else { "Collapse" };
  format!("{verb} {title}").into()
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
    let muted = p.muted_foreground;
    let heading = caption(div(), cx)
      .id(self.id.clone())
      .debug_selector(move || selector)
      .role(gpui::Role::Heading)
      .aria_label(self.title.clone())
      .test_support()
      .min_w_0()
      .truncate()
      .font_weight(gpui::FontWeight::SEMIBOLD)
      .text_color(muted)
      .child(display_title(&self.title));
    let lead = match self.toggle {
      Some(toggle) => {
        let toggle_selector = toggle.id.to_string();
        let on_toggle = toggle.on_toggle;
        let fg = p.foreground;
        div()
          .id(toggle.id.clone())
          .debug_selector(move || toggle_selector)
          .role(gpui::Role::Button)
          .aria_label(toggle_label(&self.title, toggle.collapsed))
          .flex()
          .items_center()
          .gap_1()
          .min_w_0()
          .cursor_pointer()
          .text_color(muted)
          .hover(move |s| s.text_color(fg))
          .child(crate::icon(
            if toggle.collapsed {
              theme::ICON_CHEVRON_RIGHT
            } else {
              theme::ICON_CHEVRON_DOWN
            },
            12.,
            muted,
          ))
          .child(heading)
          .on_click(move |event, window, cx| on_toggle(event, window, cx))
          .into_any_element()
      }
      None => heading.into_any_element(),
    };
    div()
      .flex()
      .items_center()
      .gap_2()
      .when_some(self.icon, |el, icon| el.child(crate::icon(icon, 12., muted)))
      .child(lead)
      .when_some(self.note, |el, note| {
        el.child(caption(div(), cx).truncate().text_color(muted).child(note))
      })
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

  #[test]
  fn test_section_header_icon_and_note() {
    let p = theme::palette(false);
    let header = SectionHeader::new("gh", "GitHub", &p)
      .icon(theme::ICON_GITHUB)
      .note("Signed in");
    assert_eq!(header.icon, Some(theme::ICON_GITHUB));
    assert_eq!(header.note.as_ref().map(|n| n.as_ref()), Some("Signed in"));
  }

  #[test]
  fn test_section_header_toggle_names_its_action() {
    assert_eq!(super::toggle_label("Changes", true).as_ref(), "Expand Changes");
    assert_eq!(super::toggle_label("Changes", false).as_ref(), "Collapse Changes");
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

    struct Collapsible;
    impl Render for Collapsible {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        div().w(px(300.)).child(
          SectionHeader::new("changes", "Changes", &p)
            .collapsible("changes-toggle", true, |_, _, _| {}),
        )
      }
    }

    /// A collapsible header keeps its heading and adds a toggle button.
    #[gpui::test]
    fn a_collapsible_header_has_a_toggle(cx: &mut TestAppContext) {
      let window = cx.add_window(|_, _| Collapsible);
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      assert!(cx.debug_bounds("changes").is_some());
      assert!(cx.debug_bounds("changes-toggle").is_some());
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
