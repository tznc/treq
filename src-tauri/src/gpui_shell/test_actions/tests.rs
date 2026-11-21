//! Selector actions go through the element's gpui-testing-library node.

use std::{cell::Cell, rc::Rc};

use gpui::{
  div, px, Context, InteractiveElement, IntoElement, ParentElement, Render, Role,
  StatefulInteractiveElement, Styled, TestAppContext, Window,
};
use gpui_kit::base::TestSupportExt;

use super::*;

/// A button that counts its clicks, with a node when `testable`.
struct View {
  clicks: Rc<Cell<u32>>,
  testable: bool,
  opacity: f32,
}

impl Render for View {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let clicks = self.clicks.clone();
    let button = div()
      .id("go")
      .size(px(40.))
      .opacity(self.opacity)
      .debug_selector(|| "go".into())
      .on_click(move |_, _, _| clicks.set(clicks.get() + 1));
    let button = if self.testable {
      button
        .role(Role::Button)
        .aria_label("Go")
        .test_support()
        .into_any_element()
    } else {
      button.into_any_element()
    };
    div().size_full().child(button)
  }
}

fn window(
  cx: &mut TestAppContext,
  testable: bool,
  opacity: f32,
) -> (&mut VisualTestContext, Rc<Cell<u32>>) {
  let clicks = Rc::new(Cell::new(0));
  let view_clicks = clicks.clone();
  let (_, cx) = cx.add_window_view(move |_, _| View {
    clicks: view_clicks,
    testable,
    opacity,
  });
  cx.run_until_parked();
  (cx, clicks)
}

#[gpui::test]
fn a_node_is_clicked(cx: &mut TestAppContext) {
  let (cx, clicks) = window(cx, true, 1.);
  act(cx, "go", Action::Click);
  assert_eq!(clicks.get(), 1);
}

#[gpui::test]
#[should_panic(expected = "not visible")]
fn a_transparent_node_cannot_be_clicked(cx: &mut TestAppContext) {
  let (cx, _) = window(cx, true, 0.);
  act(cx, "go", Action::Click);
}

#[gpui::test]
fn a_selector_without_a_node_is_clicked_at_its_bounds(cx: &mut TestAppContext) {
  let (cx, clicks) = window(cx, false, 1.);
  act(cx, "go", Action::Click);
  assert_eq!(clicks.get(), 1);
}

#[gpui::test]
#[should_panic(expected = "missing is not drawn")]
fn a_missing_selector_panics(cx: &mut TestAppContext) {
  let (cx, _) = window(cx, true, 1.);
  act(cx, "missing", Action::Click);
}

/// A hidden heading with the id `go`, and a plain field whose selector is `go`.
struct SharedId {
  clicks: Rc<Cell<u32>>,
}

impl Render for SharedId {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let clicks = self.clicks.clone();
    div()
      .size_full()
      .child(
        div()
          .id("go")
          .size(px(40.))
          .opacity(0.)
          .role(Role::Heading)
          .aria_label("Title")
          .test_support(),
      )
      .child(
        div()
          .id("field")
          .size(px(40.))
          .debug_selector(|| "go".into())
          .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
      )
  }
}

#[gpui::test]
fn a_node_that_only_shares_the_selectors_id_is_not_used(cx: &mut TestAppContext) {
  let clicks = Rc::new(Cell::new(0));
  let view_clicks = clicks.clone();
  let (_, cx) = cx.add_window_view(move |_, _| SharedId {
    clicks: view_clicks,
  });
  act(cx, "go", Action::Click);
  assert_eq!(clicks.get(), 1);
}
