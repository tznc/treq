//! Every interactive control is a tab stop and activates from the keyboard
//! with the keys its role expects; a click leaves focus where it was, so
//! the focus ring shows only for keyboard focus.

use std::{cell::RefCell, rc::Rc};

use gpui::{
  div, prelude::*, Entity, FocusHandle, Modifiers, Role, TestAppContext, VisualTestContext, Window,
};

use crate::{theme, Button, Checkbox, IconButton, Picker, Row, Tab, Tabs};

type Log = Rc<RefCell<Vec<String>>>;

struct Host {
  start: FocusHandle,
  log: Log,
}

fn note(log: &Log, what: &str) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) {
  let (log, what) = (log.clone(), what.to_string());
  move |_, _, _| log.borrow_mut().push(what.clone())
}

impl Render for Host {
  fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
    let p = theme::palette(false);
    let log = self.log.clone();
    div()
      .size_full()
      // Tab outside a dialog: the window's next tab stop.
      .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
        if event.keystroke.key == "tab" {
          window.focus_next(cx);
        }
      })
      .flex()
      .flex_col()
      .child(div().id("start").track_focus(&self.start).child("Start"))
      .child(
        Button::new("btn", &p)
          .label("Save")
          .on_click(note(&log, "btn")),
      )
      .child(IconButton::new("icon", theme::ICON_CHECK, "Done", &p).on_click(note(&log, "icon")))
      .child(
        Picker::new("pick", &p)
          .label("Branch")
          .on_click(note(&log, "pick")),
      )
      .child(
        Checkbox::new("check", &p)
          .label("Assigned to me")
          .on_click(note(&log, "check")),
      )
      .child(
        Row::new("row", &p)
          .title("Open")
          .on_click(note(&log, "row")),
      )
      .child(
        Row::new("check-row", &p)
          .role(Role::CheckBox)
          .title("#5 Change")
          .on_click(note(&log, "check-row")),
      )
      .child(
        Tabs::segmented("tabs", &p)
          .tab(Tab::new("a", "A"))
          .tab(Tab::new("b", "B"))
          .on_select({
            let log = log.clone();
            move |ix, _, _| log.borrow_mut().push(format!("tab {ix}"))
          }),
      )
  }
}

fn draw(cx: &mut VisualTestContext) {
  cx.update(|window, cx| window.draw(cx).clear(cx));
  cx.run_until_parked();
  cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn mount(cx: &mut TestAppContext) -> (VisualTestContext, Entity<Host>, Log) {
  cx.update(theme::init);
  let log = Log::default();
  let window = cx.add_window({
    let log = log.clone();
    move |_, cx| Host {
      start: cx.focus_handle().tab_stop(true),
      log,
    }
  });
  let host = window.root(cx).unwrap();
  let mut cx = VisualTestContext::from_window(*window, cx);
  cx.update(|window, _| window.activate_window());
  draw(&mut cx);
  cx.update(|window, cx| host.read(cx).start.clone().focus(window, cx));
  draw(&mut cx);
  (cx, host, log)
}

/// From the start, Tab `tabs` times, then press `key`: what ran.
fn press(cx: &mut TestAppContext, tabs: usize, key: &str) -> Vec<String> {
  let (mut cx, _, log) = mount(cx);
  for _ in 0..tabs {
    cx.simulate_keystrokes("tab");
    draw(&mut cx);
  }
  // A native keyboard click is a key down then its key up.
  let keystroke = gpui::Keystroke::parse(key).unwrap();
  cx.simulate_event(gpui::KeyDownEvent {
    keystroke: keystroke.clone(),
    is_held: false,
    prefer_character_input: false,
  });
  cx.simulate_event(gpui::KeyUpEvent { keystroke });
  draw(&mut cx);
  let ran = log.borrow().clone();
  ran
}

#[gpui::test]
fn tab_reaches_a_button_and_enter_or_space_presses_it(cx: &mut TestAppContext) {
  assert_eq!(press(cx, 1, "enter"), ["btn"]);
  assert_eq!(press(cx, 1, "space"), ["btn"]);
}

#[gpui::test]
fn tab_reaches_an_icon_button(cx: &mut TestAppContext) {
  assert_eq!(press(cx, 2, "enter"), ["icon"]);
}

#[gpui::test]
fn tab_reaches_a_picker_trigger(cx: &mut TestAppContext) {
  assert_eq!(press(cx, 3, "space"), ["pick"]);
}

#[gpui::test]
fn tab_reaches_a_checkbox_and_space_toggles_it(cx: &mut TestAppContext) {
  assert_eq!(press(cx, 4, "space"), ["check"]);
  assert!(press(cx, 4, "enter").is_empty(), "Enter doesn't toggle");
}

#[gpui::test]
fn tab_reaches_a_clickable_row_and_enter_opens_it(cx: &mut TestAppContext) {
  assert_eq!(press(cx, 5, "enter"), ["row"]);
  assert!(press(cx, 5, "space").is_empty(), "Space doesn't open a row");
}

#[gpui::test]
fn a_checkbox_row_toggles_with_space(cx: &mut TestAppContext) {
  assert_eq!(press(cx, 6, "space"), ["check-row"]);
}

#[gpui::test]
fn tab_reaches_the_tab_list_and_arrows_pick_a_tab(cx: &mut TestAppContext) {
  assert_eq!(press(cx, 7, "right"), ["tab 1"]);
}

/// A click runs the control but leaves focus where it was, so no ring.
#[gpui::test]
fn a_click_leaves_focus_where_it_was(cx: &mut TestAppContext) {
  let (mut cx, host, log) = mount(cx);
  for selector in ["check", "row", "check-row", "a"] {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(&mut cx);
    let start = cx.update(|window, cx| host.read(cx).start.is_focused(window));
    assert!(start, "a click on {selector} took focus");
  }
  assert_eq!(log.borrow().len(), 4);
}
