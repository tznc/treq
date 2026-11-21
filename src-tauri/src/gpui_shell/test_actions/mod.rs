//! Test actions on an element named by its debug selector.

use gpui::{Modifiers, VisualTestContext};

/// What to do to the element.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Action {
  Click,
  RightClick,
  Hover,
}

/// Does `action` to the element `selector` names.
pub(crate) fn act(cx: &mut VisualTestContext, selector: &str, action: Action) {
  cx.run_until_parked();
  let at = cx
    .debug_bounds(selector)
    .unwrap_or_else(|| panic!("{selector} is not drawn"))
    .center();
  match action {
    Action::Click => cx.simulate_click(at, Modifiers::none()),
    Action::RightClick | Action::Hover => unimplemented!(),
  }
  cx.run_until_parked();
}

#[cfg(test)]
mod tests;
