//! Bottom sheets: one at a time, over the whole window, above a scrim that
//! closes it. Menus (a list of actions) and confirmations are sheets; so is
//! any view a screen opens with [`open`].

use std::{rc::Rc, time::Duration};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  div, AnyView, App, AppContext, Context, Entity, Global, InteractiveElement, IntoElement,
  ParentElement, Pixels, Render, SharedString, Styled, WeakEntity, Window,
};
use treq_ui::{ConfirmDialog, Dialog};

/// What a sheet action does when tapped. The sheet closes first.
pub type Action = treq_ui::touch::Action;

/// The open sheet, owned by the root view.
pub struct Sheet {
  pub title: Option<SharedString>,
  pub(super) body: Body,
}

/// What a sheet holds: a screen's view, or a confirmation.
pub(super) enum Body {
  View(AnyView),
  Confirm(Confirm),
}

struct Slot {
  root: WeakEntity<super::Root>,
}
impl Global for Slot {}

pub(super) fn install(root: WeakEntity<super::Root>, cx: &mut App) {
  cx.set_global(Slot { root });
}

/// Opens the sheet `build` creates, replacing any open one.
pub fn open<V: Render>(
  title: Option<SharedString>,
  cx: &mut App,
  build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
) {
  let Some(root) = cx.try_global::<Slot>().map(|s| s.root.clone()) else {
    return;
  };
  // Deferred: callers are usually inside another entity's update.
  cx.defer(move |cx| {
    let Some(entity) = root.upgrade() else { return };
    let window = cx.global::<super::Nav>().window;
    window
      .update(cx, |_, window, cx| {
        let view = build(window, cx);
        entity.update(cx, |root, cx| {
          root.sheet = Some(Sheet {
            title,
            body: Body::View(view.into()),
          });
          cx.notify();
        });
      })
      .ok();
  });
}

pub fn close(cx: &mut App) {
  let Some(root) = cx.try_global::<Slot>().map(|s| s.root.clone()) else {
    return;
  };
  cx.defer(move |cx| {
    root.update(cx, dismiss).ok();
  });
}

/// How long taps are ignored after a sheet closes. A double tap on a sheet's
/// button would otherwise land its second tap on the page below (a row, or
/// the page's own button in the same spot).
const TAP_GUARD: Duration = Duration::from_millis(350);

/// Closes the open sheet, if any, and ignores taps for [`TAP_GUARD`].
pub(super) fn dismiss(root: &mut super::Root, cx: &mut Context<super::Root>) {
  if root.sheet.take().is_none() {
    return;
  }
  root.tap_guard = true;
  cx.spawn(async move |root, cx| {
    cx.background_executor().timer(TAP_GUARD).await;
    root
      .update(cx, |root, cx| {
        root.tap_guard = false;
        cx.notify();
      })
      .ok();
  })
  .detach();
  cx.notify();
}

/// Swallows taps while [`TAP_GUARD`] runs.
pub(super) fn tap_guard() -> impl IntoElement {
  div().id("sheet-tap-guard").absolute().inset_0().occlude()
}

/// The sheet over `content`: a scrim that closes it and a panel along the
/// bottom edge.
pub(super) fn render(sheet: &Sheet, bottom_inset: Pixels, cx: &App) -> impl IntoElement {
  render_with(sheet, bottom_inset, Rc::new(|_, cx| close(cx)), cx)
}

/// [`render`], closing through `on_close` (the scrim tap, a drag down and
/// a confirmation's Cancel).
fn render_with(
  sheet: &Sheet,
  bottom_inset: Pixels,
  on_close: Action,
  cx: &App,
) -> impl IntoElement {
  let palette = treq_ui::theme::current(cx);
  let content = match &sheet.body {
    Body::View(view) => Dialog::new("sheet", &palette)
      .sheet(bottom_inset)
      .when_some(sheet.title.clone(), |d, title| d.title(title))
      .on_close(move |window, cx| on_close(window, cx))
      .child(view.clone())
      .into_any_element(),
    Body::Confirm(c) => {
      let action = c.action.clone();
      let cancel = on_close.clone();
      ConfirmDialog::new(
        "sheet",
        sheet.title.clone().unwrap_or_default(),
        c.confirm_label.clone(),
        &palette,
      )
      .sheet(bottom_inset)
      .message(c.message.clone())
      .when(c.danger, |d| d.destructive())
      .on_confirm(move |window, cx| {
        close(cx);
        action(window, cx);
      })
      .on_cancel(move |window, cx| cancel(window, cx))
      .into_any_element()
    }
  };
  div().absolute().inset_0().child(content)
}

/// One row of a [`Menu`].
pub type MenuEntry = treq_ui::touch::SheetAction;

/// A list of actions, as a sheet: the phone's context menu.
pub struct Menu {
  entries: Vec<MenuEntry>,
}

/// Opens a menu sheet.
pub fn menu(title: impl Into<SharedString>, entries: Vec<MenuEntry>, cx: &mut App) {
  open(Some(title.into()), cx, move |_, cx| {
    cx.new(|_| Menu { entries })
  });
}

impl Render for Menu {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let palette = treq_ui::theme::current(cx);
    treq_ui::touch::ActionSheet::new(self.entries.clone(), &palette).on_dismiss(|_, cx| close(cx))
  }
}

/// A confirmation: what will happen, and the one button that does it.
pub(super) struct Confirm {
  message: SharedString,
  confirm_label: SharedString,
  danger: bool,
  action: Action,
}

/// Opens a confirmation sheet. `confirm_label` names the action ("Delete
/// workspace"), never "OK".
pub fn confirm(
  title: impl Into<SharedString>,
  message: impl Into<SharedString>,
  confirm_label: impl Into<SharedString>,
  danger: bool,
  action: impl Fn(&mut Window, &mut App) + 'static,
  cx: &mut App,
) {
  let Some(root) = cx.try_global::<Slot>().map(|s| s.root.clone()) else {
    return;
  };
  let sheet = Sheet {
    title: Some(title.into()),
    body: Body::Confirm(Confirm {
      message: message.into(),
      confirm_label: confirm_label.into(),
      danger,
      action: Rc::new(action),
    }),
  };
  // Deferred: callers are usually inside another entity's update.
  cx.defer(move |cx| {
    root
      .update(cx, |root, cx| {
        root.sheet = Some(sheet);
        cx.notify();
      })
      .ok();
  });
}

#[cfg(test)]
mod tests {
  use gpui_kit::MouseButton;
  use gpui_kit::{
    div, px, size, AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Point,
    Render, Styled as _, TestAppContext, Window, WindowBounds, WindowOptions,
  };
  use gpui_testing_library::{prelude::*, PointerAction};
  use std::{cell::Cell, rc::Rc};

  struct Empty;
  impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      div()
    }
  }

  struct Host(super::Sheet, Rc<Cell<bool>>);
  impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
      let closed = self.1.clone();
      div().size_full().child(super::render_with(
        &self.0,
        px(0.),
        Rc::new(move |_, _| closed.set(true)),
        cx,
      ))
    }
  }

  /// A 300px-tall sheet titled "Delete workspace?"; the flag is set when it
  /// closes.
  fn open_sheet(cx: &mut TestAppContext) -> (gpui_testing_library::Screen, Rc<Cell<bool>>) {
    cx.update(super::super::theme::init);
    let closed = Rc::new(Cell::new(false));
    let flag = closed.clone();
    let window = cx.update(|cx| {
      gpui_kit::open_window(
        WindowOptions {
          window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(412.), px(892.)),
          })),
          ..Default::default()
        },
        cx,
        |_, cx| {
          let view = cx.new(|_| Tall).into();
          cx.new(|_| {
            Host(
              super::Sheet {
                title: Some("Delete workspace?".into()),
                body: super::Body::View(view),
              },
              flag,
            )
          })
        },
      )
      .expect("open test window")
      .0
    });
    let screen = gpui_testing_library::Screen::new(cx, window);
    screen.redraw();
    (screen, closed)
  }

  struct Tall;
  impl Render for Tall {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      div().h(px(250.))
    }
  }

  /// Drags from the grabber down by `dy` pixels.
  fn drag_handle(screen: &gpui_testing_library::Screen, dy: f32) {
    let sheet = screen.get_by_role(by::role(gpui_kit::Role::Dialog));
    let b = sheet.bounds();
    let start = Point::new(b.center().x, b.origin.y + px(10.));
    let mut actions = vec![
      PointerAction::MoveToPosition(start),
      PointerAction::Press(MouseButton::Left),
    ];
    for step in 1..=4 {
      actions.push(PointerAction::MoveToPosition(Point::new(
        start.x,
        start.y + px(dy * step as f32 / 4.),
      )));
    }
    actions.push(PointerAction::Release(MouseButton::Left));
    screen.user().pointer(actions);
  }

  /// The title outranks the body text it introduces.
  #[test]
  fn the_title_is_larger_than_the_body() {
    const { assert!(treq_ui::dialog::SHEET_TITLE_PX > treq_ui::dialog::SHEET_BODY_PX) };
  }

  /// Dragging the title down past a quarter of the sheet closes it.
  #[gpui_kit::test]
  fn a_long_drag_down_closes_the_sheet(cx: &mut TestAppContext) {
    let (screen, closed) = open_sheet(cx);
    drag_handle(&screen, 200.);
    assert!(closed.get(), "sheet should close");
  }

  /// A short, slow drag snaps back and leaves the sheet open.
  #[gpui_kit::test]
  fn a_short_drag_leaves_the_sheet_open(cx: &mut TestAppContext) {
    let (screen, closed) = open_sheet(cx);
    drag_handle(&screen, 20.);
    assert!(!closed.get(), "sheet should stay open");
    expect(screen.get_by_role(by::role(gpui_kit::Role::Dialog).name("Delete workspace?")))
      .to_be_visible();
  }

  /// A screen reader reads the sheet as a dialog, named by its title.
  #[gpui_kit::test]
  fn the_sheet_is_a_dialog_named_by_its_title(cx: &mut TestAppContext) {
    cx.update(super::super::theme::init);
    let window = cx.update(|cx| {
      gpui_kit::open_window(
        WindowOptions {
          window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(412.), px(892.)),
          })),
          ..Default::default()
        },
        cx,
        |_, cx| {
          let view = cx.new(|_| Empty).into();
          cx.new(|_| {
            Host(
              super::Sheet {
                title: Some("Delete workspace?".into()),
                body: super::Body::View(view),
              },
              Rc::default(),
            )
          })
        },
      )
      .expect("open test window")
      .0
    });
    let screen = gpui_testing_library::Screen::new(cx, window);
    screen.redraw();
    expect(screen.get_by_role(by::role(gpui_kit::Role::Dialog).name("Delete workspace?")))
      .to_be_visible();
  }
}
