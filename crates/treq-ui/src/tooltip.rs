//! Hover tooltips for [`crate::Button::tooltip`]: after [`DELAY`] the text
//! shows beside the hovered button, and it goes on leave or press.
//!
//! The shell draws them (in popup windows) and installs, per window, the
//! [`TooltipHost`] that does with [`set_host`]; a window without one shows
//! none.

use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  rc::Rc,
  time::Duration,
};

use gpui::{App, Bounds, Pixels, SharedString, Window, WindowId};

/// base-ui's tooltips open after 200ms (`TooltipProvider delay={200}`).
pub const DELAY: Duration = Duration::from_millis(200);

/// Which side of the button a tooltip shows on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
  #[default]
  Bottom,
  /// For a button near the window's foot, such as a composer's Send.
  Top,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TooltipEvent {
  Show {
    anchor: Bounds<Pixels>,
    text: SharedString,
    side: Side,
  },
  Hide,
}

/// Draws one window's tooltips.
pub type TooltipHost = Rc<dyn Fn(TooltipEvent, &mut App)>;

/// The button whose tooltip is pending or shown.
struct Current {
  window: WindowId,
  key: SharedString,
  text: SharedString,
  side: Side,
  shown: bool,
  /// Tells a stale timer from this hover's.
  hover: u64,
}

thread_local! {
  static HOSTS: RefCell<HashMap<WindowId, TooltipHost>> = RefCell::default();
  static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) };
  static HOVERS: Cell<u64> = const { Cell::new(0) };
  /// Where each button with a tooltip was last drawn.
  static BOUNDS: RefCell<HashMap<(WindowId, SharedString), Bounds<Pixels>>> = RefCell::default();
}

/// Installs `host` to draw `window`'s tooltips, replacing the one before.
pub fn set_host(window: &Window, host: TooltipHost) {
  let id = window.window_handle().window_id();
  HOSTS.with(|hosts| hosts.borrow_mut().insert(id, host));
}

fn send(window: WindowId, event: TooltipEvent, cx: &mut App) {
  let host = HOSTS.with(|hosts| hosts.borrow().get(&window).cloned());
  if let Some(host) = host {
    host(event, cx);
  }
}

/// Ends the current tooltip, hiding it if it shows.
fn end(cx: &mut App) {
  if let Some(current) = CURRENT.with(|c| c.borrow_mut().take()) {
    if current.shown {
      send(current.window, TooltipEvent::Hide, cx);
    }
  }
}

/// The pointer entered (`hovered`) or left the button `key`.
pub(crate) fn hover(
  key: &SharedString,
  text: &SharedString,
  side: Side,
  hovered: bool,
  window: &Window,
  cx: &mut App,
) {
  let id = window.window_handle().window_id();
  let is_current = || {
    CURRENT.with(|c| {
      c.borrow()
        .as_ref()
        .is_some_and(|c| c.window == id && c.key == *key)
    })
  };
  if !hovered {
    if is_current() {
      end(cx);
    }
    return;
  }
  // A repeat enter, as a popup window opening sends, keeps it.
  if is_current() {
    return;
  }
  end(cx);
  let hover = HOVERS.with(|h| {
    h.set(h.get() + 1);
    h.get()
  });
  CURRENT.with(|c| {
    *c.borrow_mut() = Some(Current {
      window: id,
      key: key.clone(),
      text: text.clone(),
      side,
      shown: false,
      hover,
    })
  });
  cx.spawn(async move |cx| {
    cx.background_executor().timer(DELAY).await;
    cx.update(|cx| show(hover, cx));
  })
  .detach();
}

fn show(hover: u64, cx: &mut App) {
  let pending = CURRENT.with(|c| {
    let mut c = c.borrow_mut();
    let current = c.as_mut().filter(|c| c.hover == hover && !c.shown)?;
    current.shown = true;
    Some((
      current.window,
      current.key.clone(),
      current.text.clone(),
      current.side,
    ))
  });
  let Some((window, key, text, side)) = pending else {
    return;
  };
  let anchor = BOUNDS.with(|b| b.borrow().get(&(window, key)).copied());
  if let Some(anchor) = anchor {
    send(window, TooltipEvent::Show { anchor, text, side }, cx);
  }
}

/// A press on the button `key` hides its tooltip, as base-ui's does.
pub(crate) fn press(key: &SharedString, window: &Window, cx: &mut App) {
  let id = window.window_handle().window_id();
  if CURRENT.with(|c| {
    c.borrow()
      .as_ref()
      .is_some_and(|c| c.window == id && c.key == *key)
  }) {
    end(cx);
  }
}

/// Records where the button `key` was drawn, for its tooltip's anchor.
pub(crate) fn record(key: SharedString, bounds: Bounds<Pixels>, window: &Window) {
  let id = window.window_handle().window_id();
  BOUNDS.with(|b| b.borrow_mut().insert((id, key), bounds));
}

#[cfg(test)]
mod tests {
  use gpui::{div, prelude::*, Role, TestAppContext, Window};
  use gpui_testing_library::{prelude::*, Node, Screen};

  use super::*;
  use crate::{theme, Button, IconButton};

  type Log = Rc<RefCell<Vec<TooltipEvent>>>;

  struct Host {
    log: Log,
  }

  impl Render for Host {
    fn render(&mut self, window: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
      let log = self.log.clone();
      set_host(
        window,
        Rc::new(move |event, _| log.borrow_mut().push(event)),
      );
      let p = theme::palette(false);
      div()
        .size_full()
        .flex()
        .gap_4()
        .p_4()
        .child(
          Button::new("edit", &p)
            .aria_label("Edit title")
            .icon(theme::ICON_PENCIL, 14.)
            .tooltip("Edit title"),
        )
        .child(IconButton::new("close", theme::ICON_X, "Close", &p))
        .child(
          IconButton::new("send", theme::ICON_SEND, "Send", &p)
            .tooltip("Send (Ctrl+Enter)")
            .tooltip_side(Side::Top),
        )
        .child(IconButton::new("own", theme::ICON_X, "Own tooltip", &p).on_hover(|_, _, _| {}))
    }
  }

  fn mount(cx: &mut TestAppContext) -> (Screen, Log) {
    cx.update(theme::init);
    let log = Log::default();
    let window = cx.add_window({
      let log = log.clone();
      move |_, _| Host { log }
    });
    let screen = Screen::new(cx, window);
    screen.redraw();
    (screen, log)
  }

  fn button(screen: &Screen, name: &str) -> Node {
    screen.get_by_role(by::role(Role::Button).name(name))
  }

  fn wait(screen: &Screen, ms: u64) {
    screen.executor().advance_clock(Duration::from_millis(ms));
    screen.executor().run_until_parked();
  }

  #[gpui::test]
  fn hovering_shows_the_text_beside_the_button_after_the_delay(cx: &mut TestAppContext) {
    let (screen, log) = mount(cx);
    let edit = button(&screen, "Edit title");
    screen.user().hover(&edit);
    wait(&screen, 100);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    wait(&screen, 200);
    assert_eq!(
      *log.borrow(),
      [TooltipEvent::Show {
        anchor: edit.bounds(),
        text: "Edit title".into(),
        side: Side::Bottom,
      }]
    );
  }

  #[gpui::test]
  fn leaving_hides_it(cx: &mut TestAppContext) {
    let (screen, log) = mount(cx);
    let edit = button(&screen, "Edit title");
    screen.user().hover(&edit);
    wait(&screen, 300);
    screen.user().unhover(&edit);
    assert_eq!(log.borrow().last(), Some(&TooltipEvent::Hide));
  }

  #[gpui::test]
  fn leaving_before_the_delay_shows_nothing(cx: &mut TestAppContext) {
    let (screen, log) = mount(cx);
    let edit = button(&screen, "Edit title");
    screen.user().hover(&edit);
    screen.user().unhover(&edit);
    wait(&screen, 300);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
  }

  #[gpui::test]
  fn a_press_hides_it(cx: &mut TestAppContext) {
    let (screen, log) = mount(cx);
    let edit = button(&screen, "Edit title");
    screen.user().hover(&edit);
    wait(&screen, 300);
    screen.user().click(&edit);
    assert_eq!(log.borrow().last(), Some(&TooltipEvent::Hide));
  }

  #[gpui::test]
  fn an_icon_button_shows_its_label(cx: &mut TestAppContext) {
    let (screen, log) = mount(cx);
    screen.user().hover(&button(&screen, "Close"));
    wait(&screen, 300);
    assert!(
      matches!(&log.borrow()[..], [TooltipEvent::Show { text, .. }] if text == "Close"),
      "{:?}",
      log.borrow()
    );
  }

  #[gpui::test]
  fn a_tooltip_shows_on_the_side_asked(cx: &mut TestAppContext) {
    let (screen, log) = mount(cx);
    screen.user().hover(&button(&screen, "Send"));
    wait(&screen, 300);
    assert!(
      matches!(&log.borrow()[..], [TooltipEvent::Show { text, side: Side::Top, .. }] if text == "Send (Ctrl+Enter)"),
      "{:?}",
      log.borrow()
    );
  }

  #[gpui::test]
  fn an_icon_button_with_its_own_hover_shows_none(cx: &mut TestAppContext) {
    let (screen, log) = mount(cx);
    screen.user().hover(&button(&screen, "Own tooltip"));
    wait(&screen, 300);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
  }
}
