//! A small chevron button that sits after a group's name and folds the
//! group: pointing down while open, right while shut.
//!
//! Use it where the header's text must line up with the rows above it and
//! its children must show their indent from it, as in the sidebar's
//! repository groups. A leading chevron would push the text right of
//! everything else; [`crate::SectionHeader::collapsible`] does that and
//! suits headers that need no such alignment.
//!
//! Put the name in a `min_w_0().truncate()` element before the toggle:
//! a long name then gives way with an ellipsis and the chevron keeps its
//! size, never clipped or pushed out of the row. The toggle is `flex_none`.

use crate::SelectorExt as _;
use std::rc::Rc;

use gpui::{div, prelude::*, App, ClickEvent, SharedString, Window};

use crate::theme::{self, Palette};

type ToggleFn = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The trailing fold chevron of the group `name`.
#[derive(IntoElement)]
pub struct FoldToggle {
  id: SharedString,
  name: SharedString,
  collapsed: bool,
  palette: Palette,
  on_toggle: Option<ToggleFn>,
}

impl FoldToggle {
  pub fn new(
    id: impl Into<SharedString>,
    name: impl Into<SharedString>,
    collapsed: bool,
    palette: &Palette,
  ) -> Self {
    Self {
      id: id.into(),
      name: name.into(),
      collapsed,
      palette: *palette,
      on_toggle: None,
    }
  }

  /// What a click does. The click stops here, so the row around the
  /// chevron doesn't also take it.
  pub fn on_toggle(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_toggle = Some(Rc::new(f));
    self
  }

  /// What a press does, for a screen reader.
  fn label(&self) -> SharedString {
    let verb = if self.collapsed { "Expand" } else { "Collapse" };
    format!("{verb} {}", self.name).into()
  }

  fn chevron(&self) -> &'static str {
    if self.collapsed {
      theme::ICON_CHEVRON_RIGHT
    } else {
      theme::ICON_CHEVRON_DOWN
    }
  }
}

impl RenderOnce for FoldToggle {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let label = self.label();
    let chevron = self.chevron();
    let on_toggle = self.on_toggle;
    div()
      .selector(self.id)
      .role(gpui::Role::Button)
      .aria_label(label)
      .flex()
      .flex_none()
      .items_center()
      .justify_center()
      .size(crate::zpx(18.))
      .rounded_md()
      .cursor_pointer()
      .hover(move |s| s.bg(p.hover()))
      .child(crate::icon(chevron, 14., p.muted_foreground))
      .on_click(move |event, window, cx| {
        cx.stop_propagation();
        if let Some(f) = &on_toggle {
          f(event, window, cx);
        }
      })
  }
}

#[cfg(test)]
mod tests {
  use std::{cell::Cell, rc::Rc};

  use gpui::{div, prelude::*, px, Context, TestAppContext, VisualTestContext, Window};

  use super::FoldToggle;
  use crate::theme;

  #[test]
  fn the_label_names_what_a_press_does() {
    let p = theme::palette(false);
    assert_eq!(
      FoldToggle::new("t", "docs-site", false, &p)
        .label()
        .as_ref(),
      "Collapse docs-site"
    );
    assert_eq!(
      FoldToggle::new("t", "docs-site", true, &p).label().as_ref(),
      "Expand docs-site"
    );
  }

  #[test]
  fn the_chevron_points_down_while_open_and_right_while_shut() {
    let p = theme::palette(false);
    assert_eq!(
      FoldToggle::new("t", "a", false, &p).chevron(),
      theme::ICON_CHEVRON_DOWN
    );
    assert_eq!(
      FoldToggle::new("t", "a", true, &p).chevron(),
      theme::ICON_CHEVRON_RIGHT
    );
  }

  struct Host {
    toggles: Rc<Cell<u32>>,
    row_clicks: Rc<Cell<u32>>,
  }
  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      let toggles = self.toggles.clone();
      let row_clicks = self.row_clicks.clone();
      div()
        .id("row")
        .w(px(300.))
        .flex()
        .child("docs-site")
        .child(
          FoldToggle::new("fold", "docs-site", false, &p)
            .on_toggle(move |_, _, _| toggles.set(toggles.get() + 1)),
        )
        .on_click(move |_, _, _| row_clicks.set(row_clicks.get() + 1))
    }
  }

  /// A click folds the group and doesn't reach the row around it.
  #[gpui::test]
  fn a_click_toggles_without_selecting_the_row(cx: &mut TestAppContext) {
    let toggles = Rc::new(Cell::new(0));
    let row_clicks = Rc::new(Cell::new(0));
    let window = cx.add_window({
      let (toggles, row_clicks) = (toggles.clone(), row_clicks.clone());
      move |_, _| Host {
        toggles,
        row_clicks,
      }
    });
    let mut cx = VisualTestContext::from_window(*window, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds("fold").expect("the toggle is drawn");
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    assert_eq!(toggles.get(), 1);
    assert_eq!(row_clicks.get(), 0);
  }
}
