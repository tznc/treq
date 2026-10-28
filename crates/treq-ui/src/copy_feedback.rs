//! Feedback for a copy button: its icon turns into a green check for
//! [`COPIED_FOR`] after a copy, then back.

use std::time::{Duration, Instant};

use gpui::{
  ease_out_quint, prelude::*, size, Animation, AnimationExt, AnyElement, ElementId, Hsla,
  Transformation,
};

use crate::theme;

/// How long the swap between the copy icon and the check takes.
pub const SWAP_FOR: Duration = Duration::from_millis(180);

/// How long the check shows after a copy.
pub const COPIED_FOR: Duration = Duration::from_millis(2000);

/// When a copy button last copied.
#[derive(Default, Debug, Clone, Copy)]
pub struct CopyFeedback {
  copied_at: Option<Instant>,
}

impl CopyFeedback {
  pub fn copy(&mut self, now: Instant) {
    self.copied_at = Some(now);
  }

  pub fn is_copied(&self, now: Instant) -> bool {
    self
      .copied_at
      .is_some_and(|at| now.saturating_duration_since(at) < COPIED_FOR)
  }
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
  let id = ElementId::NamedChild(
    std::sync::Arc::new(id.into()),
    if copied { "check" } else { "copy" }.into(),
  );
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shows_the_check_after_a_copy_then_reverts() {
    let start = Instant::now();
    let mut feedback = CopyFeedback::default();
    assert!(!feedback.is_copied(start));
    feedback.copy(start);
    assert!(feedback.is_copied(start));
    assert!(feedback.is_copied(start + COPIED_FOR - Duration::from_millis(1)));
    assert!(!feedback.is_copied(start + COPIED_FOR));
  }

  #[test]
  fn a_second_copy_restarts_the_timer() {
    let start = Instant::now();
    let mut feedback = CopyFeedback::default();
    feedback.copy(start);
    feedback.copy(start + Duration::from_millis(1500));
    assert!(feedback.is_copied(start + COPIED_FOR));
  }
}
