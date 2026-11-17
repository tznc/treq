//! Press and hold: a finger's long press, or a held mouse button on a
//! desktop build, runs an action instead of the tap underneath.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::{
  AnyElement, App, Bounds, Element, ElementId, Empty, GlobalElementId, Hitbox, HitboxBehavior,
  InspectorElementId, IntoElement, LayoutId, LongPressEvent, MouseButton, MouseDownEvent,
  MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, Task, TouchDragEvent, TouchPhase,
  Window,
};

/// How long a mouse button stays down before it counts as a hold.
const HOLD_DELAY: Duration = Duration::from_millis(500);

/// How far a pointer may drift before a press stops counting as a hold.
const HOLD_SLOP: f32 = 10.;

/// Wraps `child` so a hold on it runs `on_hold`. The tap that ends a hold
/// doesn't reach `child`.
pub fn hold(
  id: impl Into<ElementId>,
  child: impl IntoElement,
  on_hold: impl Fn(&mut Window, &mut App) + 'static,
) -> Hold {
  Hold {
    id: id.into(),
    child: Some(child.into_any_element()),
    on_hold: Rc::new(on_hold),
  }
}

pub struct Hold {
  id: ElementId,
  child: Option<AnyElement>,
  on_hold: super::sheet::Action,
}

#[derive(Default)]
struct Press {
  start: Option<Point<Pixels>>,
  /// The press became a hold, so its release isn't a tap.
  held: bool,
  timer: Option<Task<()>>,
}

impl Press {
  fn cancel(&mut self) {
    self.start = None;
    self.timer = None;
  }
}

impl IntoElement for Hold {
  type Element = Self;

  fn into_element(self) -> Self {
    self
  }
}

impl Element for Hold {
  type RequestLayoutState = AnyElement;
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
  ) -> (LayoutId, AnyElement) {
    let mut child = self
      .child
      .take()
      .unwrap_or_else(|| Empty.into_any_element());
    (child.request_layout(window, cx), child)
  }

  fn prepaint(
    &mut self,
    _: Option<&GlobalElementId>,
    _: Option<&InspectorElementId>,
    bounds: Bounds<Pixels>,
    child: &mut AnyElement,
    window: &mut Window,
    cx: &mut App,
  ) -> Hitbox {
    child.prepaint(window, cx);
    window.insert_hitbox(bounds, HitboxBehavior::Normal)
  }

  fn paint(
    &mut self,
    id: Option<&GlobalElementId>,
    _: Option<&InspectorElementId>,
    _: Bounds<Pixels>,
    child: &mut AnyElement,
    hitbox: &mut Hitbox,
    window: &mut Window,
    cx: &mut App,
  ) {
    #[expect(clippy::expect_used, reason = "`Hold::id` always returns an id")]
    let press = window.with_element_state(id.expect("Hold has an id"), |state, _| {
      let state: Rc<RefCell<Press>> = state.unwrap_or_default();
      (state.clone(), state)
    });

    // A finger's long press, recognized by the platform.
    window.on_mouse_event({
      let (press, hitbox, on_hold) = (press.clone(), hitbox.clone(), self.on_hold.clone());
      move |event: &LongPressEvent, phase, window, cx| {
        if phase.bubble()
          && event.phase == TouchPhase::Started
          && !window.default_prevented()
          && hitbox.is_hovered(window)
        {
          window.prevent_default();
          cx.stop_propagation();
          let mut press = press.borrow_mut();
          press.cancel();
          press.held = true;
          drop(press);
          on_hold(window, cx);
        }
      }
    });

    // A held mouse button. Capture phase, so the child still sees the press.
    window.on_mouse_event({
      let (press, hitbox, on_hold) = (press.clone(), hitbox.clone(), self.on_hold.clone());
      move |event: &MouseDownEvent, phase, window, cx| {
        if !phase.capture() {
          return;
        }
        let mut state = press.borrow_mut();
        state.held = false;
        state.cancel();
        if event.button != MouseButton::Left || !hitbox.is_hovered(window) {
          return;
        }
        state.start = Some(event.position);
        let (press, on_hold) = (press.clone(), on_hold.clone());
        state.timer = Some(window.spawn(cx, async move |cx| {
          cx.background_executor().timer(HOLD_DELAY).await;
          let fire = {
            let mut press = press.borrow_mut();
            let fire = press.start.take().is_some();
            press.held |= fire;
            fire
          };
          if fire {
            cx.update(|window, cx| {
              on_hold(window, cx);
              window.refresh();
            })
            .ok();
          }
        }));
      }
    });

    window.on_mouse_event({
      let press = press.clone();
      move |event: &MouseMoveEvent, phase, _, _| {
        let mut press = press.borrow_mut();
        let drifted = press
          .start
          .is_some_and(|start| (event.position - start).magnitude() > HOLD_SLOP as f64);
        if phase.capture() && drifted {
          press.cancel();
        }
      }
    });

    // Scrolling isn't holding.
    window.on_mouse_event({
      let press = press.clone();
      move |_: &TouchDragEvent, phase, _, _| {
        if phase.capture() {
          press.borrow_mut().cancel();
        }
      }
    });
    window.on_mouse_event({
      let press = press.clone();
      move |_: &ScrollWheelEvent, phase, _, _| {
        if phase.capture() {
          press.borrow_mut().cancel();
        }
      }
    });

    // The release that ends a hold is not a tap on the child.
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
      if !phase.capture() {
        return;
      }
      let mut press = press.borrow_mut();
      press.cancel();
      if std::mem::take(&mut press.held) {
        cx.stop_propagation();
      }
    });

    child.paint(window, cx);
  }
}
