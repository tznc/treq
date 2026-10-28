//! Feedback for a copy button: its icon turns into a green check for
//! [`COPIED_FOR`] after a copy, then back.

use std::time::Duration;

use gpui::{
  div, ease_out_quint, prelude::*, size, Animation, AnimationExt, AnyElement, App, ClipboardItem,
  Context, ElementId, Entity, Hsla, MouseButton, SharedString, Task, Transformation, Window,
};

use crate::theme::{self, Palette};

/// How long the swap between the copy icon and the check takes.
pub const SWAP_FOR: Duration = Duration::from_millis(180);

/// How long the check shows after a copy.
pub const COPIED_FOR: Duration = Duration::from_millis(2000);

/// Whether a copy button just copied. Keep one per button with [`state`].
#[derive(Default)]
pub struct CopyFeedback {
  copied: bool,
  /// A left press landed on a [`Copyable`]; its release copies.
  pressed: bool,
  reset: Option<Task<()>>,
}

impl CopyFeedback {
  pub fn is_copied(&self) -> bool {
    self.copied
  }

  /// Shows the check, for [`COPIED_FOR`] from the latest copy.
  pub fn copy(&mut self, cx: &mut Context<Self>) {
    self.copied = true;
    // Replacing the task drops the earlier timer, so the latest copy wins.
    self.reset = Some(cx.spawn(async move |this, cx| {
      cx.background_executor().timer(COPIED_FOR).await;
      let _ = this.update(cx, |this, cx| {
        this.copied = false;
        cx.notify();
      });
    }));
    cx.notify();
  }
}

/// The copy feedback of the button `id`, kept while it renders. The view
/// rendering it redraws when the check shows or reverts.
fn state(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Entity<CopyFeedback> {
  window.use_keyed_state(id, cx, |_, _| CopyFeedback::default())
}

/// The copy icon, or the check in `success` while `copied`. Each swap fades
/// and scales the new icon in. `id` keeps the animation apart from other
/// copy buttons'.
pub fn copy_icon(
  id: impl Into<ElementId>,
  copied: bool,
  size_px: f32,
  idle: Hsla,
  success: Hsla,
) -> AnyElement {
  let (path, color) = if copied {
    (theme::ICON_CHECK, success)
  } else {
    (theme::ICON_COPY, idle)
  };
  // A new id per state restarts the animation on every swap.
  let state = if copied { "check" } else { "copy" };
  let id = ElementId::NamedChild(std::sync::Arc::new(id.into()), state.into());
  crate::icon(path, size_px, color)
    .with_animation(
      id,
      Animation::new(SWAP_FOR).with_easing(ease_out_quint()),
      move |svg, delta| {
        let scale = 0.6 + 0.4 * delta;
        svg
          .opacity(delta)
          .with_transformation(Transformation::scale(size(scale, scale)))
      },
    )
    .into_any_element()
}

/// Wraps a copy control so a click on it shows the check: `render` draws
/// it, told whether it just copied. The control keeps its own click handler
/// to do the copy; the wrapper sees the click even when that handler stops
/// propagation. `id` must be unique among its siblings.
pub fn copyable(
  id: impl Into<ElementId>,
  render: impl FnOnce(bool) -> AnyElement + 'static,
) -> Copyable {
  Copyable {
    id: id.into(),
    disabled: false,
    render: Box::new(render),
  }
}

#[derive(IntoElement)]
pub struct Copyable {
  id: ElementId,
  disabled: bool,
  render: Box<dyn FnOnce(bool) -> AnyElement>,
}

impl Copyable {
  /// A disabled control copies nothing, so its click shows no check.
  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }
}

impl RenderOnce for Copyable {
  fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let feedback = state(self.id.clone(), window, cx);
    let copied = feedback.read(cx).is_copied();
    let (down, up) = (feedback.clone(), feedback);
    // Capture phase: the control's own handler may stop the click bubbling.
    div()
      .id(self.id)
      .child((self.render)(copied))
      .when(!self.disabled, |this| {
        this
          .capture_any_mouse_down(move |event, _, cx| {
            if event.button == MouseButton::Left {
              down.update(cx, |f, _| f.pressed = true);
            }
          })
          .capture_any_mouse_up(move |event, _, cx| {
            if event.button == MouseButton::Left {
              up.update(cx, |f, cx| {
                if std::mem::take(&mut f.pressed) {
                  f.copy(cx);
                }
              });
            }
          })
      })
  }
}

/// An icon-only copy button: copies `text` and shows the check.
#[derive(IntoElement)]
pub struct CopyButton {
  id: ElementId,
  text: SharedString,
  size: f32,
  palette: Palette,
}

impl CopyButton {
  pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      text: text.into(),
      size: 16.,
      palette: *palette,
    }
  }

  pub fn size(mut self, size: f32) -> Self {
    self.size = size;
    self
  }
}

impl RenderOnce for CopyButton {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let (id, p, size) = (self.id, self.palette, self.size);
    let text = self.text;
    copyable(id.clone(), move |copied| {
      div()
        .id("copy")
        .flex_none()
        .p_1()
        .rounded_md()
        .hover(|style| style.bg(p.hover()))
        .cursor_pointer()
        .child(copy_icon(
          id,
          copied,
          size,
          p.muted_foreground.opacity(0.7),
          p.success,
        ))
        .on_click(move |_, _, cx| {
          cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        })
        .into_any_element()
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use gpui::TestAppContext;

  #[gpui::test]
  fn shows_the_check_after_a_copy_then_reverts(cx: &mut TestAppContext) {
    let feedback = cx.new(|_| CopyFeedback::default());
    assert!(!feedback.read_with(cx, |f, _| f.is_copied()));
    feedback.update(cx, |f, cx| f.copy(cx));
    assert!(feedback.read_with(cx, |f, _| f.is_copied()));
    cx.executor()
      .advance_clock(COPIED_FOR - Duration::from_millis(1));
    cx.run_until_parked();
    assert!(feedback.read_with(cx, |f, _| f.is_copied()));
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert!(!feedback.read_with(cx, |f, _| f.is_copied()));
  }

  mod copyable {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{
      div, point, px, Context, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _,
      Render, StatefulInteractiveElement as _, Styled as _, TestAppContext, VisualTestContext,
      Window,
    };

    use super::super::{copyable, COPIED_FOR};

    struct Host {
      seen: Rc<RefCell<Vec<bool>>>,
      disabled: bool,
    }

    impl Render for Host {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let seen = self.seen.clone();
        div().size_full().child(
          copyable("c", move |copied| {
            seen.borrow_mut().push(copied);
            div()
              .id("inner")
              .w(px(40.))
              .h(px(40.))
              // Like a copy button inside a clickable row.
              .on_click(|_, _, cx| cx.stop_propagation())
              .into_any_element()
          })
          .disabled(self.disabled),
        )
      }
    }

    fn mount(
      cx: &mut TestAppContext,
      disabled: bool,
    ) -> (VisualTestContext, Rc<RefCell<Vec<bool>>>) {
      let seen = Rc::new(RefCell::new(Vec::new()));
      let window = cx.add_window({
        let seen = seen.clone();
        move |_, _| Host { seen, disabled }
      });
      let cx = VisualTestContext::from_window(*window, cx);
      cx.run_until_parked();
      (cx, seen)
    }

    #[gpui::test]
    fn a_click_shows_the_check_then_it_reverts(cx: &mut TestAppContext) {
      let (mut cx, seen) = mount(cx, false);
      assert_eq!(seen.borrow().last(), Some(&false));
      cx.simulate_click(point(px(20.), px(20.)), Modifiers::none());
      cx.run_until_parked();
      assert_eq!(seen.borrow().last(), Some(&true));
      cx.executor().advance_clock(COPIED_FOR);
      cx.run_until_parked();
      assert_eq!(seen.borrow().last(), Some(&false));
    }

    #[gpui::test]
    fn a_disabled_control_shows_no_check(cx: &mut TestAppContext) {
      let (mut cx, seen) = mount(cx, true);
      cx.simulate_click(point(px(20.), px(20.)), Modifiers::none());
      cx.run_until_parked();
      assert!(!seen.borrow().contains(&true));
    }
  }

  #[gpui::test]
  fn a_second_copy_restarts_the_timer(cx: &mut TestAppContext) {
    let feedback = cx.new(|_| CopyFeedback::default());
    feedback.update(cx, |f, cx| f.copy(cx));
    cx.executor().advance_clock(Duration::from_millis(1500));
    feedback.update(cx, |f, cx| f.copy(cx));
    cx.executor().advance_clock(Duration::from_millis(1500));
    cx.run_until_parked();
    assert!(feedback.read_with(cx, |f, _| f.is_copied()));
  }
}
