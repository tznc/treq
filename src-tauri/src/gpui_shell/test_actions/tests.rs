//! Selector actions go through the element's gpui-testing-library node.

use std::{cell::Cell, rc::Rc};

use gpui::{div, px, Context, IntoElement, ParentElement, Render, Role, Styled, TestAppContext, Window};
use gpui::{InteractiveElement, StatefulInteractiveElement};
use gpui_kit::base::TestSupportExt;

use super::*;

/// A button `height` tall in a 20px box that clips it, with a node when
/// `testable`.
struct View {
  clicks: Rc<Cell<u32>>,
  height: f32,
  testable: bool,
}

impl Render for View {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let clicks = self.clicks.clone();
    let button = div()
      .id("go")
      .h(px(self.height))
      .w(px(40.))
      .debug_selector(|| "go".into())
      .on_click(move |_, _, _| clicks.set(clicks.get() + 1));
    let button = if self.testable {
      button.role(Role::Button).aria_label("Go").test_support().into_any_element()
    } else {
      button.into_any_element()
    };
    div().size_full().child(div().h(px(20.)).overflow_hidden().child(button))
  }
}

fn window(cx: &mut TestAppContext, height: f32, testable: bool) -> (&mut VisualTestContext, Rc<Cell<u32>>) {
  let clicks = Rc::new(Cell::new(0));
  let view_clicks = clicks.clone();
  let (_, cx) = cx.add_window_view(move |_, _| View { clicks: view_clicks, height, testable });
  cx.run_until_parked();
  (cx, clicks)
}

#[gpui::test]
fn a_partly_clipped_element_is_clicked_where_it_shows(cx: &mut TestAppContext) {
  // Painted 100px tall, so the centre of its bounds is clipped out.
  let (cx, clicks) = window(cx, 100., true);
  act(cx, "go", Action::Click);
  assert_eq!(clicks.get(), 1);
}

#[gpui::test]
#[should_panic(expected = "not visible")]
fn a_clipped_out_element_cannot_be_clicked(cx: &mut TestAppContext) {
  let (cx, _) = window(cx, 0., true);
  act(cx, "go", Action::Click);
}

#[gpui::test]
fn a_selector_without_a_node_is_clicked_at_its_bounds(cx: &mut TestAppContext) {
  let (cx, clicks) = window(cx, 20., false);
  act(cx, "go", Action::Click);
  assert_eq!(clicks.get(), 1);
}

#[gpui::test]
#[should_panic(expected = "missing is not drawn")]
fn a_missing_selector_panics(cx: &mut TestAppContext) {
  let (cx, _) = window(cx, 20., true);
  act(cx, "missing", Action::Click);
}
