//! Helpers shared by the GPUI test harnesses: the window as
//! gpui-testing-library sees it, and actions on elements named by their debug
//! selector.

use gpui::{ElementId, Modifiers, MouseButton, Role, VisualTestContext};
use gpui_testing_library::{by, normalize, prelude::*, Screen};

/// A test harness that owns a window. Implement `cx`; the rest is shared.
pub(crate) trait WindowHarness {
  fn cx(&mut self) -> &mut VisualTestContext;

  /// The window as the user sees it, once everything pending has run.
  fn rendered(&mut self) -> Screen {
    rendered(self.cx())
  }

  /// Whether the user sees an element of `role` named `name`.
  fn shows(&mut self, role: Role, name: &str) -> bool {
    !self
      .rendered()
      .query_all_by_role(by::role(role).name(normalize(name)))
      .is_empty()
  }

  fn click(&mut self, selector: &'static str) {
    act(self.cx(), selector, Action::Click);
  }

  fn right_click(&mut self, selector: &'static str) {
    act(self.cx(), selector, Action::RightClick);
  }

  fn hover(&mut self, selector: &'static str) {
    act(self.cx(), selector, Action::Hover);
  }
}

/// The window of `cx` as the user sees it, once everything pending has run.
pub(crate) fn rendered(cx: &mut VisualTestContext) -> Screen {
  cx.run_until_parked();
  let window = cx.update(|window, _| window.window_handle());
  let screen = Screen::new(cx, window);
  screen.redraw();
  screen
}

/// What to do to the element.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Action {
  Click,
  RightClick,
  Hover,
}

/// Does `action` to the element `selector` names, as the user would: through
/// its gpui-testing-library node, which must be visible, at the centre of what
/// shows of it.
///
/// The node is the one with the selector's element id and painted bounds:
/// another element can share the id, such as a dialog's heading and a field
/// in it. A selector with no node (see the `gpui-debug-selector-needs-test-node`
/// lint) is acted on at the centre of its painted bounds instead.
pub(crate) fn act(cx: &mut VisualTestContext, selector: &'static str, action: Action) {
  let screen = rendered(cx);
  let bounds = cx
    .debug_bounds(selector)
    .unwrap_or_else(|| panic!("{selector} is not drawn"));
  let id = ElementId::Name(selector.into());
  let node = screen
    .query_all_by_element_id(by::element_id(id))
    .into_iter()
    .find(|node| node.snapshot().bounds() == bounds);
  if let Some(node) = node {
    let user = screen.user();
    match action {
      Action::Click => user.click(&node),
      Action::RightClick => user.right_click(&node),
      Action::Hover => user.hover(&node),
    }
    return;
  }
  let at = bounds.center();
  match action {
    Action::Click => cx.simulate_click(at, Modifiers::none()),
    Action::RightClick => {
      cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
      cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    }
    Action::Hover => cx.simulate_mouse_move(at, None, Modifiers::none()),
  }
  cx.run_until_parked();
}

#[cfg(test)]
mod tests;
