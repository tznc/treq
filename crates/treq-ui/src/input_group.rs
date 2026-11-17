//! A control with icon buttons joined to its end, inside one border
//! (Bootstrap's input group). For a field whose value comes from an action,
//! such as a picker, rather than only from typing.

use gpui::{div, prelude::*, AnyElement, App, SharedString, Window};

use crate::{theme, theme::Palette, zpx};

/// One border around a control and the buttons appended to it, with a
/// divider before each button. Pass the control without its own border
/// (gpui-kit's `Input::appearance(false)`), and [`crate::IconButton`]s, whose
/// label names the action, as the addons. On a phone prefer this to a text
/// button beside the field.
#[derive(IntoElement)]
pub struct InputGroup {
  id: SharedString,
  palette: Palette,
  control: Option<AnyElement>,
  addons: Vec<AnyElement>,
}

impl InputGroup {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      control: None,
      addons: Vec::new(),
    }
  }

  /// The field itself, which takes the remaining width.
  pub fn control(mut self, control: impl IntoElement) -> Self {
    self.control = Some(control.into_any_element());
    self
  }

  /// A button at the end, after any added before it.
  pub fn append(mut self, addon: impl IntoElement) -> Self {
    self.addons.push(addon.into_any_element());
    self
  }
}

impl RenderOnce for InputGroup {
  fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = self.palette;
    let height = theme::Density::current(cx).control_height();
    let selector = self.id.to_string();
    let id = self.id.clone();
    div()
      .id(self.id)
      .debug_selector(move || selector)
      .flex()
      .min_w_0()
      .h(zpx(height))
      .rounded(zpx(theme::RADIUS_MD))
      .border_1()
      .border_color(p.input)
      .bg(p.background)
      .overflow_hidden()
      .child(
        div()
          .flex_1()
          .min_w_0()
          .h_full()
          .flex()
          .items_center()
          .children(self.control),
      )
      .children(self.addons.into_iter().enumerate().map(move |(i, addon)| {
        let selector = format!("{id}-addon-{i}");
        div()
          .debug_selector(move || selector)
          .flex_none()
          .h_full()
          .flex()
          .items_center()
          .justify_center()
          .border_l_1()
          .border_color(p.input)
          .child(addon)
      }))
  }
}

#[cfg(test)]
mod tests {
  use gpui::{px, TestAppContext, VisualTestContext};

  use super::*;

  struct Host;

  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      div().w(px(300.)).child(
        InputGroup::new("path", &p)
          .control(
            div()
              .debug_selector(|| "field".into())
              .w_full()
              .child("~/code"),
          )
          .append(crate::IconButton::new(
            "browse",
            theme::ICON_FOLDER_OPEN,
            "Browse",
            &p,
          )),
      )
    }
  }

  fn mount(cx: &mut TestAppContext, density: theme::Density) -> VisualTestContext {
    cx.update(|cx| {
      theme::init(cx);
      theme::set_density(density, cx);
    });
    let window = cx.add_window(|_, _| Host);
    let mut cx = VisualTestContext::from_window(*window, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx
  }

  #[gpui::test]
  fn the_button_is_joined_to_the_field_at_the_end(cx: &mut TestAppContext) {
    let mut cx = mount(cx, theme::Density::Desktop);
    let group = cx.debug_bounds("path").unwrap();
    let field = cx.debug_bounds("field").unwrap();
    let addon = cx.debug_bounds("path-addon-0").unwrap();
    assert_eq!(group.size.height, px(theme::CONTROL_MD));
    // No gap: the addon starts where the field ends, flush to the border.
    assert_eq!(addon.origin.x, field.origin.x + field.size.width);
    assert_eq!(
      addon.origin.x + addon.size.width,
      group.origin.x + group.size.width - px(1.)
    );
  }

  #[gpui::test]
  fn on_touch_the_group_is_a_44_target(cx: &mut TestAppContext) {
    let mut cx = mount(cx, theme::Density::Touch);
    let group = cx.debug_bounds("path").unwrap();
    let addon = cx.debug_bounds("path-addon-0").unwrap();
    assert_eq!(group.size.height, px(theme::TOUCH_TARGET));
    assert!(addon.size.width >= px(theme::TOUCH_TARGET));
  }
}
