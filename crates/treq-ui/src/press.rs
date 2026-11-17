//! Press feedback that shows on touch.
//!
//! GPUI's `.active()` style lasts from mouse down to mouse up. A finger tap
//! reaches GPUI as both at once, on release, so the style never paints. A
//! [`Press`] darkens from the moment the finger lands, through GPUI's
//! touch-start offer (`TouchDragEvent` with `Started`), and stays dark for at
//! least [`MIN_VISIBLE`] so a quick tap still shows. A press with the mouse
//! works the same way.

use std::{
  cell::RefCell,
  rc::Rc,
  time::{Duration, Instant},
};

use gpui::{
  fill, px, AnyElement, App, Bounds, Element, ElementId, Empty, GlobalElementId, Hitbox,
  HitboxBehavior, InspectorElementId, IntoElement, LayoutId, LongPressEvent, MouseButton,
  MouseDownEvent, MouseUpEvent, Pixels, ScrollWheelEvent, Style, Task, TouchDragEvent, TouchPhase,
  Window,
};

/// The shortest time a press stays visible, however quick the tap.
pub const MIN_VISIBLE: Duration = Duration::from_millis(100);

/// The smallest touch target, in logical pixels (Apple's 44pt).
pub const MIN_TOUCH_TARGET: f32 = 44.;

/// When a press shows and when it stops.
#[derive(Default, Debug)]
pub struct PressState {
  pressed_at: Option<Instant>,
}

impl PressState {
  pub fn is_pressed(&self) -> bool {
    self.pressed_at.is_some()
  }

  pub fn press(&mut self, now: Instant) {
    self.pressed_at = Some(now);
  }

  /// A scroll or a long press took the touch: it won't be a tap.
  pub fn cancel(&mut self) {
    self.pressed_at = None;
  }

  /// How long the press should stay visible after a release at `now`.
  /// `None` when nothing is pressed.
  pub fn release_delay(&self, now: Instant) -> Option<Duration> {
    let shown = now.saturating_duration_since(self.pressed_at?);
    Some(MIN_VISIBLE.saturating_sub(shown))
  }
}

/// Wraps `child` so a press on it darkens it. `id` keeps the press apart
/// from its siblings'; the child's own id will do.
pub fn pressable(id: impl Into<ElementId>, child: impl IntoElement) -> Press {
  Press {
    id: id.into(),
    child: Some(child.into_any_element()),
    overlay: false,
    radius: px(0.),
    on_change: None,
  }
}

/// The same feedback as [`pressable`], as an absolutely positioned child
/// that fills a `relative()` parent. For a parent whose builder can't be
/// wrapped, like a row returned for the caller to add `on_click` to.
pub fn press_overlay() -> Press {
  Press {
    id: "press".into(),
    child: None,
    overlay: true,
    radius: px(0.),
    on_change: None,
  }
}

pub struct Press {
  id: ElementId,
  child: Option<AnyElement>,
  overlay: bool,
  radius: Pixels,
  on_change: Option<OnChange>,
}

impl Press {
  /// Rounds the darkened area to match the pressed element's corners.
  pub fn rounded(mut self, radius: Pixels) -> Self {
    self.radius = radius;
    self
  }

  /// Runs `f` with `true` when the press shows and `false` when it goes.
  pub fn on_change(mut self, f: impl Fn(bool) + 'static) -> Self {
    self.on_change = Some(Rc::new(f));
    self
  }
}

type OnChange = Rc<dyn Fn(bool)>;

/// The press, its pending release and who hears about changes.
type Shared = Rc<RefCell<(PressState, Option<Task<()>>, Option<OnChange>)>>;

impl IntoElement for Press {
  type Element = Self;

  fn into_element(self) -> Self {
    self
  }
}

impl Element for Press {
  type RequestLayoutState = Option<AnyElement>;
  type PrepaintState = Hitbox;

  fn id(&self) -> Option<ElementId> {
    Some(self.id.clone())
  }

  fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
    None
  }

  fn request_layout(
    &mut self,
    _: Option<&GlobalElementId>,
    _: Option<&InspectorElementId>,
    window: &mut Window,
    cx: &mut App,
  ) -> (LayoutId, Option<AnyElement>) {
    if self.overlay {
      let style = Style {
        position: gpui::Position::Absolute,
        inset: gpui::Edges::all(px(0.).into()),
        ..Style::default()
      };
      return (window.request_layout(style, None, cx), None);
    }
    let mut child = self
      .child
      .take()
      .unwrap_or_else(|| Empty.into_any_element());
    (child.request_layout(window, cx), Some(child))
  }

  fn prepaint(
    &mut self,
    _: Option<&GlobalElementId>,
    _: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    child: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
  ) -> Hitbox {
    if let Some(child) = child {
      child.prepaint(window, cx);
    }
    window.insert_hitbox(bounds, HitboxBehavior::Normal)
  }

  fn paint(
    &mut self,
    id: Option<&GlobalElementId>,
    _: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    child: &mut Option<AnyElement>,
    hitbox: &mut Hitbox,
    window: &mut Window,
    cx: &mut App,
  ) {
    let state = window.with_element_state(id.expect("Press has an id"), |state, _| {
      let state: Shared = state.unwrap_or_default();
      (state.clone(), state)
    });
    state.borrow_mut().2 = self.on_change.clone();

    if let Some(child) = child {
      child.paint(window, cx);
    }
    if state.borrow().0.is_pressed() {
      let color = crate::theme::current(cx).foreground.opacity(0.12);
      window.paint_quad(fill(bounds, color).corner_radii(self.radius));
    }

    // GPUI offers every new touch as a drag at touch-down, before it knows
    // whether it is a tap: the earliest a finger can be seen.
    window.on_mouse_event({
      let (state, hitbox) = (state.clone(), hitbox.clone());
      move |event: &TouchDragEvent, phase, window, _| {
        if phase.bubble() && event.phase == TouchPhase::Started && hitbox.is_hovered(window) {
          press(&state, window);
        }
      }
    });
    window.on_mouse_event({
      let (state, hitbox) = (state.clone(), hitbox.clone());
      move |event: &MouseDownEvent, phase, window, _| {
        if phase.bubble() && event.button == MouseButton::Left && hitbox.is_hovered(window) {
          press(&state, window);
        }
      }
    });
    // A pan or a long press means the touch isn't a tap.
    window.on_mouse_event({
      let state = state.clone();
      move |event: &ScrollWheelEvent, phase, window, _| {
        if phase.capture() && event.touch_phase == TouchPhase::Started {
          cancel(&state, window);
        }
      }
    });
    window.on_mouse_event({
      let state = state.clone();
      move |event: &LongPressEvent, phase, window, _| {
        if phase.capture() && event.phase != TouchPhase::Moved {
          cancel(&state, window);
        }
      }
    });
    window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
      if phase.capture() {
        release(&state, window, cx);
      }
    });
  }
}

fn press(state: &Shared, window: &mut Window) {
  let mut state = state.borrow_mut();
  let was = state.0.is_pressed();
  state.0.press(Instant::now());
  state.1 = None;
  window.refresh();
  if !was {
    notify(&state.2, true);
  }
}

fn notify(on_change: &Option<OnChange>, pressed: bool) {
  if let Some(f) = on_change {
    f(pressed);
  }
}

fn cancel(state: &Shared, window: &mut Window) {
  let mut state = state.borrow_mut();
  if state.0.is_pressed() {
    state.0.cancel();
    state.1 = None;
    window.refresh();
    notify(&state.2, false);
  }
}

fn release(state: &Shared, window: &mut Window, cx: &mut App) {
  let Some(delay) = state.borrow().0.release_delay(Instant::now()) else {
    return;
  };
  let shared = state.clone();
  let task = window.spawn(cx, async move |cx| {
    cx.background_executor().timer(delay).await;
    let on_change = {
      let mut shared = shared.borrow_mut();
      shared.0.cancel();
      shared.2.clone()
    };
    notify(&on_change, false);
    cx.update(|window, _| window.refresh()).ok();
  });
  state.borrow_mut().1 = Some(task);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_quick_tap_stays_visible_for_the_minimum() {
    let start = Instant::now();
    let mut state = PressState::default();
    state.press(start);
    assert!(state.is_pressed());
    assert_eq!(
      state.release_delay(start + Duration::from_millis(30)),
      Some(Duration::from_millis(70))
    );
  }

  #[test]
  fn a_long_press_releases_at_once() {
    let start = Instant::now();
    let mut state = PressState::default();
    state.press(start);
    assert_eq!(
      state.release_delay(start + Duration::from_millis(300)),
      Some(Duration::ZERO)
    );
  }

  #[test]
  fn a_cancelled_press_has_nothing_to_release() {
    let mut state = PressState::default();
    state.press(Instant::now());
    state.cancel();
    assert!(!state.is_pressed());
    assert_eq!(state.release_delay(Instant::now()), None);
  }

  mod touch {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui::{
      div, point, px, Context, IntoElement, ParentElement as _, PlatformInput, Render, Styled as _,
      TestAppContext, TouchEvent, TouchId, TouchPhase, VisualTestContext, Window,
    };

    use super::super::{pressable, MIN_VISIBLE};

    struct Host {
      changes: Rc<RefCell<Vec<bool>>>,
    }

    impl Render for Host {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let changes = self.changes.clone();
        div().size_full().child(
          pressable("b", div().w(px(100.)).h(px(44.)))
            .on_change(move |pressed| changes.borrow_mut().push(pressed)),
        )
      }
    }

    fn mount(cx: &mut TestAppContext) -> (VisualTestContext, Rc<RefCell<Vec<bool>>>) {
      let changes = Rc::new(RefCell::new(Vec::new()));
      let window = cx.add_window({
        let changes = changes.clone();
        move |_, _| Host { changes }
      });
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      (cx, changes)
    }

    fn touch(cx: &mut VisualTestContext, phase: TouchPhase, x: f32, y: f32) {
      cx.update(|window, cx| {
        window.dispatch_event(
          PlatformInput::Touch(TouchEvent {
            id: TouchId(1),
            phase,
            position: point(px(x), px(y)),
            predicted_position: None,
            force: None,
          }),
          cx,
        );
      });
    }

    /// The finger landing shows the press, before GPUI knows it's a tap,
    /// and a quick tap stays visible for `MIN_VISIBLE`.
    #[gpui::test]
    fn a_touch_shows_the_press_from_touch_down(cx: &mut TestAppContext) {
      let (mut cx, changes) = mount(cx);
      touch(&mut cx, TouchPhase::Started, 50., 20.);
      assert_eq!(*changes.borrow(), [true]);
      touch(&mut cx, TouchPhase::Ended, 50., 20.);
      assert_eq!(*changes.borrow(), [true], "still shown right after the tap");
      cx.executor()
        .advance_clock(MIN_VISIBLE + Duration::from_millis(1));
      cx.run_until_parked();
      assert_eq!(*changes.borrow(), [true, false]);
    }

    /// A finger that scrolls was never a tap: the press goes at once.
    #[gpui::test]
    fn a_scroll_cancels_the_press(cx: &mut TestAppContext) {
      let (mut cx, changes) = mount(cx);
      touch(&mut cx, TouchPhase::Started, 50., 20.);
      touch(&mut cx, TouchPhase::Moved, 50., 80.);
      assert_eq!(*changes.borrow(), [true, false]);
    }

    /// A touch elsewhere doesn't press it.
    #[gpui::test]
    fn a_touch_outside_does_not_press(cx: &mut TestAppContext) {
      let (mut cx, changes) = mount(cx);
      touch(&mut cx, TouchPhase::Started, 50., 200.);
      touch(&mut cx, TouchPhase::Ended, 50., 200.);
      assert!(changes.borrow().is_empty());
    }
  }
}
