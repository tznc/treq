//! The title bar found by role, pressed as a user would.

use std::{cell::RefCell, rc::Rc};

use gpui::{Role, TestAppContext};
use gpui_testing_library::{prelude::*, ByRole};

use super::*;
use crate::gpui_shell::theme;

#[derive(Default)]
struct FakeWindow {
  calls: RefCell<Vec<&'static str>>,
}

impl TitleBarHost for FakeWindow {
  fn start_move(&self, _: &mut Window) {
    self.calls.borrow_mut().push("move");
  }

  fn double_click(&self, _: &mut Window) {
    self.calls.borrow_mut().push("zoom");
  }
}

struct View {
  host: Rc<FakeWindow>,
}

impl gpui::Render for View {
  fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
    div()
      .size_full()
      .child(drag_area_with("title-bar", self.host.clone()))
  }
}

fn screen(cx: &mut TestAppContext) -> (gpui_testing_library::Screen, Rc<FakeWindow>) {
  cx.update(theme::init);
  let host = Rc::new(FakeWindow::default());
  let view_host = host.clone();
  let (_, cx) = cx.add_window_view(|_, _| View { host: view_host });
  cx.run_until_parked();
  let window = cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(cx, window);
  screen.redraw();
  (screen, host)
}

fn bar() -> ByRole {
  by::role(Role::TitleBar).name("Title bar")
}

#[gpui::test]
fn pressing_the_title_bar_moves_the_window(cx: &mut TestAppContext) {
  let (screen, host) = screen(cx);
  screen.user().click(&screen.get_by_role(bar()));
  assert_eq!(*host.calls.borrow(), ["move"]);
}

#[gpui::test]
fn double_clicking_the_title_bar_zooms(cx: &mut TestAppContext) {
  let (screen, host) = screen(cx);
  screen.user().dbl_click(&screen.get_by_role(bar()));
  // The first press of a double-click starts a move, as on a native bar.
  assert_eq!(*host.calls.borrow(), ["move", "zoom"]);
}
