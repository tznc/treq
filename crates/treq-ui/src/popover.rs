//! Panels anchored to the control that opened them.

use gpui::{
  anchored, deferred, div, prelude::*, AnyElement, App, Div, Interactivity, SharedString, Stateful,
  Window,
};

use crate::{theme, theme::Palette, zpx};

/// An overlay surface drawn above everything, at its place in the layout:
/// put it in a `relative()` trigger's parent. Close it on Escape and a
/// click outside with [`crate::DismissExt::dismiss_with`], which works on
/// it directly. No popover opens another.
/// The surface of a panel that floats in the middle of the window without
/// being a dialog, such as the command palette or a file finder: large
/// radius, border and the deepest shadow. The caller picks its fill. A
/// dialog uses [`crate::Dialog`], which draws its own.
pub fn floating<E: Styled>(el: E, p: &Palette) -> E {
  el.rounded(zpx(theme::RADIUS_LG))
    .border_1()
    .border_color(p.border)
    .shadow_2xl()
}

#[derive(IntoElement)]
pub struct Popover {
  base: Stateful<Div>,
  palette: Palette,
  children: Vec<AnyElement>,
}

impl Popover {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    let id: SharedString = id.into();
    let selector = id.to_string();
    Self {
      base: div().id(id).debug_selector(move || selector),
      palette: *palette,
      children: Vec::new(),
    }
  }
}

impl ParentElement for Popover {
  fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
    self.children.extend(elements);
  }
}

impl Styled for Popover {
  fn style(&mut self) -> &mut gpui::StyleRefinement {
    self.base.style()
  }
}

impl InteractiveElement for Popover {
  fn interactivity(&mut self) -> &mut Interactivity {
    self.base.interactivity()
  }
}

impl StatefulInteractiveElement for Popover {}

impl RenderOnce for Popover {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let surface = self
      .base
      .role(gpui::Role::Dialog)
      .flex()
      .flex_col()
      .p_1()
      .min_w(zpx(160.))
      .rounded(zpx(theme::RADIUS_MD))
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_md()
      .occlude()
      .children(self.children);
    deferred(
      anchored()
        .snap_to_window_with_margin(gpui::px(8.))
        .child(div().mt_1().child(surface)),
    )
    .with_priority(1)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_popover_builder() {
    let p = theme::palette(false);
    let popover = Popover::new("target-menu", &p).w(zpx(240.)).child("Body");
    assert_eq!(popover.children.len(), 1);
  }

  #[test]
  fn test_floating_surface_has_a_border_radius_and_shadow() {
    let p = theme::palette(true);
    let mut surface = floating(div(), &p);
    let style = surface.style();
    assert!(style.box_shadow.as_ref().is_some_and(|s| !s.is_empty()));
    assert_eq!(style.border_color, Some(p.border));
    assert!(style.corner_radii.top_left.is_some());
  }
}
