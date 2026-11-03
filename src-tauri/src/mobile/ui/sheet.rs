//! Bottom sheets: one at a time, over the whole window, above a scrim that
//! closes it. Menus (a list of actions) and confirmations are sheets; so is
//! any view a screen opens with [`open`].

use crate::mobile::ui::widgets::TouchButton as _;
use std::{
  cell::Cell,
  rc::Rc,
  time::{Duration, Instant},
};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  canvas,
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Sizable, StyledExt as _,
  },
  div, px, relative, AnyView, App, AppContext, Context, DispatchPhase, Entity, Global,
  InteractiveElement, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement,
  Pixels, Render, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window,
};

use gpui_kit::base::TestSupportExt as _;

use super::widgets;

/// What a sheet action does when tapped. The sheet closes first.
pub type Action = Rc<dyn Fn(&mut Window, &mut App)>;

/// The open sheet, owned by the root view.
pub struct Sheet {
  pub title: Option<SharedString>,
  pub view: AnyView,
  pub(super) drag: Rc<Drag>,
}

/// The drag-down-to-dismiss gesture on the grabber and title.
#[derive(Default)]
pub(super) struct Drag {
  /// Where the press started (window y) and when.
  start: Cell<Option<(f32, Instant)>>,
  /// How far the panel follows the finger down.
  offset: Cell<f32>,
  /// The panel's height at the last paint.
  height: Cell<f32>,
}

/// A drag past this fraction of the sheet's height closes it.
const DISMISS_FRACTION: f32 = 0.25;
/// A shorter drag closes it when this fast (px/s) and at least
/// [`FLICK_MIN`] long.
const FLICK_SPEED: f32 = 1000.;
const FLICK_MIN: f32 = 32.;

/// Font sizes of the sheet's title and of a confirmation's body: the title
/// is one step up from the body.
pub(super) const TITLE_PX: f32 = 18.;
pub(super) const BODY_PX: f32 = 16.;

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
            view: view.into(),
            drag: Rc::default(),
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

/// [`render`], closing through `on_close` (the scrim tap and a drag down).
fn render_with(
  sheet: &Sheet,
  bottom_inset: Pixels,
  on_close: Action,
  cx: &App,
) -> impl IntoElement {
  let theme = cx.theme();
  let palette = treq_ui::theme::current(cx);
  let drag = sheet.drag.clone();
  let scrim_close = on_close.clone();
  div()
    .absolute()
    .inset_0()
    .flex()
    .flex_col()
    .justify_end()
    .child(
      div()
        .id("sheet-scrim")
        .absolute()
        .inset_0()
        // The tap that closes the sheet must not reach the page below.
        .occlude()
        .bg(palette.scrim())
        .on_click(move |_, window, cx| scrim_close(window, cx)),
    )
    .child(
      v_flex()
        .id("sheet")
        .occlude()
        .relative()
        .top(px(drag.offset.get()))
        .max_h(relative(0.85))
        .overflow_y_scroll()
        .bg(theme.background)
        .rounded_t(theme.radius_lg)
        .pb(bottom_inset + px(16.))
        .child(drag_tracker(drag.clone(), on_close))
        .child(
          v_flex()
            .id("sheet-handle")
            .flex_none()
            // Capture phase: in the app, a press in the sheet does not reach bubble-phase handlers.
            .capture_any_mouse_down({
              let drag = drag.clone();
              move |e, _, cx| {
                if e.button == MouseButton::Left {
                  drag
                    .start
                    .set(Some((f32::from(e.position.y), Instant::now())));
                  // The drag is the sheet's, not a scroll or a tap.
                  cx.stop_propagation();
                }
              }
            })
            .child(
              div()
                .flex()
                .justify_center()
                .pt_2()
                .child(div().w(px(36.)).h(px(4.)).rounded_full().bg(theme.border)),
            )
            .when_some(sheet.title.clone(), |handle, title| {
              handle.child(
                div()
                  .px_4()
                  .pt_2()
                  .pb_1()
                  .text_size(px(TITLE_PX))
                  .font_semibold()
                  .text_color(theme.foreground)
                  .truncate()
                  .child(title),
              )
            }),
        )
        // A tall sheet scrolls; its parts keep their height.
        .child(div().flex_none().child(sheet.view.clone()))
        .role(gpui_kit::Role::Dialog)
        .when_some(sheet.title.clone(), |panel, title| panel.aria_label(title))
        .test_support(),
    )
}

/// Re-renders the root, which reads the drag offset, and repaints.
fn redraw(window: &mut Window, cx: &mut App) {
  if let Some(root) = cx.try_global::<Slot>().and_then(|s| s.root.upgrade()) {
    root.update(cx, |_, cx| cx.notify());
  }
  window.refresh();
}

/// Records the panel's height and, while a drag runs, follows the pointer
/// anywhere in the window and settles the drag on release. It listens in the
/// capture phase, on every paint: in the app the bubble phase does not see
/// these events, and a cached frame replays the listeners it was painted with.
fn drag_tracker(drag: Rc<Drag>, on_close: Action) -> impl IntoElement {
  let measure = drag.clone();
  canvas(
    move |bounds, _, _| measure.height.set(f32::from(bounds.size.height)),
    move |_, _, window, _| {
      let moving = drag.clone();
      window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
          return;
        }
        if let Some((y, _)) = moving.start.get() {
          moving.offset.set((f32::from(e.position.y) - y).max(0.));
          cx.stop_propagation();
          redraw(window, cx);
        }
      });
      let on_close = on_close.clone();
      window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
          return;
        }
        let Some((y, at)) = drag.start.take() else {
          return;
        };
        cx.stop_propagation();
        // The panel's offset is where the last move left it.
        let distance = (f32::from(e.position.y) - y).max(drag.offset.get()).max(0.);
        let speed = distance / at.elapsed().as_secs_f32().max(1e-3);
        drag.offset.set(0.);
        redraw(window, cx);
        let far = distance >= drag.height.get() * DISMISS_FRACTION;
        let flick = distance >= FLICK_MIN && speed >= FLICK_SPEED;
        if far || flick {
          on_close(window, cx);
        }
      });
    },
  )
  .absolute()
  .inset_0()
}

/// One row of a [`Menu`].
#[derive(Clone)]
pub struct MenuEntry {
  pub id: SharedString,
  pub label: SharedString,
  pub danger: bool,
  pub action: Action,
}

impl MenuEntry {
  pub fn new(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    action: impl Fn(&mut Window, &mut App) + 'static,
  ) -> Self {
    Self {
      id: id.into(),
      label: label.into(),
      danger: false,
      action: Rc::new(action),
    }
  }

  pub fn danger(mut self) -> Self {
    self.danger = true;
    self
  }
}

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
    let theme = cx.theme();
    v_flex().children(self.entries.iter().map(|entry| {
      let action = entry.action.clone();
      h_flex()
        .id(entry.id.clone())
        .min_h(px(widgets::ROW_H - 8.))
        .px_4()
        .items_center()
        .active(|s| s.bg(theme.accent))
        .when(entry.danger, |row| row.text_color(theme.danger))
        .child(entry.label.clone())
        .on_click(move |_, window, cx| {
          close(cx);
          action(window, cx);
        })
    }))
  }
}

/// A confirmation: what will happen, and the one button that does it.
pub struct Confirm {
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
  let (message, confirm_label) = (message.into(), confirm_label.into());
  let action: Action = Rc::new(action);
  open(Some(title.into()), cx, move |_, cx| {
    cx.new(|_| Confirm {
      message,
      confirm_label,
      danger,
      action,
    })
  });
}

impl Render for Confirm {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let action = self.action.clone();
    v_flex()
      .px_4()
      .pt_1()
      .gap_4()
      .child(widgets::muted(self.message.clone(), cx).text_size(px(BODY_PX)))
      .child(
        v_flex()
          .gap_2()
          .child(
            Button::new("confirm")
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .label(self.confirm_label.clone())
              .map(|b| if self.danger { b.danger() } else { b.primary() })
              .on_click(move |_, window, cx| {
                close(cx);
                action(window, cx);
              })
              .touch(),
          )
          .child(
            Button::new("cancel")
              .outline()
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .label("Cancel")
              .on_click(|_, _, cx| close(cx))
              .touch(),
          ),
      )
  }
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
                view,
                drag: Rc::default(),
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
    const { assert!(super::TITLE_PX > super::BODY_PX) };
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
                view,
                drag: Rc::default(),
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
