//! Naming an element for tests.

use gpui::{Element, Hitbox, InteractiveElement, SharedString, Stateful};
use gpui_kit::base::{ObservedElement, TestSupportExt as _};

/// Names an element for tests: `name` is its element id and debug selector,
/// and gpui-testing-library observes it (in test builds), so a test finds it
/// by role, name or id rather than by its painted bounds.
pub trait SelectorExt:
  Element<PrepaintState = Option<Hitbox>> + InteractiveElement + Sized
{
  fn selector(self, name: impl Into<SharedString>) -> ObservedElement<Stateful<Self>> {
    let name = name.into();
    let selector = name.to_string();
    self
      .id(name)
      .debug_selector(move || selector)
      .test_support()
  }
}

impl<E: Element<PrepaintState = Option<Hitbox>> + InteractiveElement> SelectorExt for E {}

#[cfg(test)]
mod tests {
  use gpui::{
    div, px, Context, IntoElement, ParentElement, Render, Styled, TestAppContext, Window,
  };
  use gpui_testing_library::{prelude::*, Screen};

  use super::*;

  struct View;

  impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      div().size_full().child(div().selector("go").size(px(10.)))
    }
  }

  #[gpui::test]
  fn a_selector_is_both_the_debug_selector_and_a_test_node(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| View);
    cx.run_until_parked();
    assert!(cx.debug_bounds("go").is_some());
    let window = cx.update(|window, _| window.window_handle());
    let screen = Screen::new(cx, window);
    screen.redraw();
    assert!(screen.query_by_element_id("go").is_some());
  }
}
