//! Modal dialogs and confirmations.

use std::{cell::Cell, rc::Rc, time::Instant};

use gpui::{
  anchored, canvas, deferred, div, point, prelude::*, px, AnyElement, App, DispatchPhase, EntityId,
  FocusHandle, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Size,
  Window,
};

use gpui_kit::base::TestSupportExt as _;

use crate::{text, theme, theme::Palette, zpx, Button, IconButton, TextSize, Variant};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A sheet's title and the body text under it, one step apart.
pub const SHEET_TITLE_PX: f32 = 18.;
pub const SHEET_BODY_PX: f32 = 16.;

/// A drag down past this fraction of a sheet's height closes it.
const DISMISS_FRACTION: f32 = 0.25;
/// A shorter drag closes it when this fast (px/s) and at least
/// [`FLICK_MIN`] long.
const FLICK_SPEED: f32 = 1000.;
const FLICK_MIN: f32 = 32.;

/// The drag-down-to-dismiss gesture on a sheet's grabber and title.
#[derive(Default)]
struct SheetDrag {
  /// Where the press started (window y) and when.
  start: Cell<Option<(f32, Instant)>>,
  /// How far the panel follows the finger down.
  offset: Cell<f32>,
  /// The panel's height at the last paint.
  height: Cell<f32>,
}

/// A view's method as a dialog handler, for [`Dialog::on_close`] and
/// [`Dialog::on_primary`]: it runs on the view while the view lives.
pub fn handler<V: 'static>(
  cx: &gpui::Context<V>,
  f: impl Fn(&mut V, &mut Window, &mut gpui::Context<V>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
  let view = cx.weak_entity();
  move |window, cx| {
    view.update(cx, |view, cx| f(view, window, cx)).ok();
  }
}

/// A dialog on the scrim. It fills its parent, as at the root of a modal
/// window, or with [`Dialog::overlay`] covers the whole window from
/// anywhere in a view. With an action pair the
/// footer holds `[Cancel] [Verb]` and there is no X; without one, an X
/// sits top-right. Escape and a click on the scrim close it; Enter, or
/// Ctrl/Cmd+Enter from a multi-line field, runs the primary action.
#[derive(IntoElement)]
pub struct Dialog {
  id: SharedString,
  palette: Palette,
  viewport: Option<Size<Pixels>>,
  priority: usize,
  width: f32,
  height: Option<f32>,
  relative: Option<(f32, Option<f32>)>,
  title: Option<SharedString>,
  description: Option<SharedString>,
  header: Option<AnyElement>,
  children: Vec<AnyElement>,
  footer: Vec<AnyElement>,
  focus: Option<FocusHandle>,
  on_close: Option<Handler>,
  on_primary: Option<Handler>,
  primary_disabled: bool,
  sheet: Option<Pixels>,
}

impl Dialog {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      viewport: None,
      priority: 2,
      width: 440.,
      height: None,
      relative: None,
      title: None,
      description: None,
      header: None,
      children: Vec::new(),
      footer: Vec::new(),
      focus: None,
      on_close: None,
      on_primary: None,
      primary_disabled: false,
      sheet: None,
    }
  }

  /// Presents it as a phone's bottom sheet: the full width of its parent
  /// along the bottom edge, `bottom_inset` above the home indicator, with
  /// a grabber. Dragging the grabber or title down closes it, as does the
  /// scrim. The title is a step larger than the description, and the
  /// footer's buttons stack full width, the verb on top.
  pub fn sheet(mut self, bottom_inset: Pixels) -> Self {
    self.sheet = Some(bottom_inset);
    self
  }

  pub fn width(mut self, width: f32) -> Self {
    self.width = width;
    self
  }

  /// A fixed height, for a body that scrolls or splits, such as a list
  /// beside its detail. Without one the dialog fits its content.
  pub fn height(mut self, height: f32) -> Self {
    self.height = Some(height);
    self
  }

  /// A wide body that grows with the window, such as a list beside its
  /// detail: `width`, and `height` if given, as fractions of the window.
  /// It replaces [`Dialog::width`] and [`Dialog::height`].
  pub fn relative_size(mut self, width: f32, height: Option<f32>) -> Self {
    self.relative = Some((width, height));
    self
  }

  /// With [`Dialog::overlay`], how high it stacks: a dialog opened from
  /// another takes a higher one. 2 by default.
  pub fn priority(mut self, priority: usize) -> Self {
    self.priority = priority;
    self
  }

  /// Replaces the title and description block with `header`, for a header
  /// that holds more than text, such as a title beside a badge. The title,
  /// if set, still names the dialog.
  pub fn header(mut self, header: impl IntoElement) -> Self {
    self.header = Some(header.into_any_element());
    self
  }

  /// The verb is disabled or busy: Enter does nothing.
  pub fn primary_disabled(mut self, disabled: bool) -> Self {
    self.primary_disabled = disabled;
    self
  }

  /// Draws it above everything, its scrim covering `viewport`, the
  /// window's size.
  pub fn overlay(mut self, viewport: Size<Pixels>) -> Self {
    self.viewport = Some(viewport);
    self
  }

  pub fn title(mut self, title: impl Into<SharedString>) -> Self {
    self.title = Some(title.into());
    self
  }

  pub fn description(mut self, description: impl Into<SharedString>) -> Self {
    self.description = Some(description.into());
    self
  }

  /// The footer's buttons, Cancel first and the verb last.
  pub fn footer(mut self, buttons: impl IntoIterator<Item = AnyElement>) -> Self {
    self.footer.extend(buttons);
    self
  }

  /// Takes Escape and Enter while `focus`, or something in the dialog, is
  /// focused. Without one, the dialog keeps its own handle and focuses it
  /// as it opens, unless something inside it already has focus.
  pub fn focus(mut self, focus: &FocusHandle) -> Self {
    self.focus = Some(focus.clone());
    self
  }

  /// Closes it: Escape, the scrim, and the X when there is no footer.
  pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_close = Some(Rc::new(f));
    self
  }

  /// What Enter runs: the footer's verb.
  pub fn on_primary(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_primary = Some(Rc::new(f));
    self
  }
}

impl ParentElement for Dialog {
  fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
    self.children.extend(elements);
  }
}

impl RenderOnce for Dialog {
  fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
    if let Some(inset) = self.sheet {
      return self.render_sheet(inset, window, cx);
    }
    let p = self.palette;
    let selector = self.id.to_string();
    let close_on_scrim = self.on_close.clone();
    let close_on_key = self.on_close.clone();
    let primary_on_key = self.on_primary.clone().filter(|_| !self.primary_disabled);
    let focus = self.focus.clone().unwrap_or_else(|| {
      window
        .use_keyed_state(
          SharedString::from(format!("{}-focus", self.id)),
          cx,
          |window, cx| {
            let focus = cx.focus_handle();
            let opening = focus.clone();
            // Once the frame is drawn, so focus a screen put on a field
            // inside the dialog stays there.
            window.defer(cx, move |window, cx| {
              if !opening.contains_focused(window, cx) {
                opening.focus(window, cx);
              }
            });
            focus
          },
        )
        .read(cx)
        .clone()
    });
    let x = self
      .on_close
      .clone()
      .filter(|_| self.footer.is_empty())
      .map(|close| {
        IconButton::new(format!("{}-close", self.id), theme::ICON_X, "Close", &p)
          .on_click(move |_, window, cx| close(window, cx))
      });
    let has_header = self.header.is_some();
    let panel = text(div(), TextSize::Sm)
      .id(SharedString::from(format!("{}-panel", self.id)))
      .debug_selector(move || selector)
      .role(gpui::Role::Dialog)
      .when_some(self.title.clone(), |el, title| el.aria_label(title))
      .test_support()
      .relative()
      .map(|el| match self.relative {
        Some((w, h)) => el
          .w(gpui::relative(w))
          .when_some(h, |el, h| el.h(gpui::relative(h))),
        None => el
          .w(zpx(self.width))
          .when_some(self.height, |el, h| el.h(zpx(h))),
      })
      .max_w_full()
      .max_h_full()
      .font_family(theme::SANS_FONT)
      .p_5()
      .flex()
      .flex_col()
      .gap_4()
      .rounded(zpx(theme::RADIUS_LG))
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_xl()
      // A click inside doesn't reach the scrim.
      .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .when_some(self.header, |el, header| {
        el.child(div().when(x.is_some(), |el| el.pr_8()).child(header))
      })
      .when(
        !has_header && (self.title.is_some() || self.description.is_some()),
        |el| {
          el.child(
            div()
              .flex()
              .flex_col()
              .gap_1()
              .when(x.is_some(), |el| el.pr_8())
              .when_some(self.title, |el, title| {
                el.child(
                  text(div(), TextSize::Lg)
                    .id(SharedString::from(format!("{}-title", self.id)))
                    .role(gpui::Role::Heading)
                    .aria_label(title.clone())
                    .test_support()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(title),
                )
              })
              .when_some(self.description, |el, description| {
                el.child(
                  text(div(), TextSize::Sm)
                    .text_color(p.muted_foreground)
                    .child(description),
                )
              }),
          )
        },
      )
      .children(self.children)
      .when(!self.footer.is_empty(), |el| {
        el.child(div().flex().justify_end().gap_2().children(self.footer))
      })
      .when_some(x, |el, x| {
        el.child(div().absolute().top_3().right_3().child(x))
      });
    let viewport = self.viewport;
    let priority = self.priority;
    let scrim = div()
      .id(SharedString::from(format!("{}-scrim", self.id)))
      .map(|el| match viewport {
        Some(size) => el.w(size.width).h(size.height),
        None => el.size_full(),
      })
      .p_4()
      .flex()
      .items_center()
      .justify_center()
      .bg(p.scrim())
      .occlude()
      .track_focus(&focus)
      .on_key_down(move |event: &KeyDownEvent, window, cx| {
        let m = event.keystroke.modifiers;
        let plain = !(m.control || m.alt || m.shift || m.platform || m.function);
        let secondary = !(m.alt || m.shift || m.function);
        let handler = match event.keystroke.key.as_str() {
          "escape" if plain => close_on_key.as_ref(),
          "enter" if secondary => primary_on_key.as_ref(),
          _ => None,
        };
        if let Some(handler) = handler {
          cx.stop_propagation();
          handler(window, cx);
        }
      })
      .when_some(close_on_scrim, |el, close| {
        el.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
          close(window, cx)
        })
      })
      .child(panel);
    match viewport {
      Some(_) => deferred(anchored().position(point(px(0.), px(0.))).child(scrim))
        .with_priority(priority)
        .into_any_element(),
      None => scrim.into_any_element(),
    }
  }
}

impl Dialog {
  fn render_sheet(self, inset: Pixels, window: &mut Window, cx: &mut App) -> AnyElement {
    let p = self.palette;
    let id = self.id.clone();
    let selector = id.to_string();
    let drag = window
      .use_keyed_state(SharedString::from(format!("{id}-drag")), cx, |_, _| {
        Rc::new(SheetDrag::default())
      })
      .read(cx)
      .clone();
    // The view that draws the sheet re-renders as the panel follows the
    // finger.
    let view = window.current_view();
    let close: Handler = self.on_close.clone().unwrap_or_else(|| Rc::new(|_, _| {}));
    let close_on_scrim = close.clone();
    let close_on_key = close.clone();
    let title_selector = format!("{id}-title");
    let description_selector = format!("{id}-description");
    let handle = div()
      .id(SharedString::from(format!("{id}-handle")))
      .flex_none()
      .flex()
      .flex_col()
      // Capture phase: a press here is the sheet's drag, not a scroll or a
      // tap in it.
      .capture_any_mouse_down({
        let drag = drag.clone();
        move |e, _, cx| {
          if e.button == MouseButton::Left {
            drag
              .start
              .set(Some((f32::from(e.position.y), Instant::now())));
            cx.stop_propagation();
          }
        }
      })
      .child(
        div().flex().justify_center().pt_2().child(
          div()
            .debug_selector(|| "sheet-grabber".into())
            .w(px(36.))
            .h(px(4.))
            .rounded_full()
            .bg(p.border),
        ),
      )
      .when_some(self.title.clone(), |el, title| {
        el.child(
          div()
            .id(SharedString::from(format!("{id}-title")))
            .debug_selector(move || title_selector)
            .role(gpui::Role::Heading)
            .aria_label(title.clone())
            .test_support()
            .px_4()
            .pt_2()
            .pb_1()
            .text_size(px(SHEET_TITLE_PX))
            .line_height(px(SHEET_TITLE_PX + 5.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(p.foreground)
            .truncate()
            .child(title),
        )
      });
    let has_body = self.description.is_some() || !self.footer.is_empty();
    let body = div()
      .flex_none()
      .flex()
      .flex_col()
      .px_4()
      .pt_1()
      .gap_4()
      .when_some(self.description, |el, description| {
        el.child(
          div()
            .debug_selector(move || description_selector)
            .text_size(px(SHEET_BODY_PX))
            .line_height(px(SHEET_BODY_PX + 5.))
            .text_color(p.muted_foreground)
            .child(description),
        )
      })
      .when(!self.footer.is_empty(), |el| {
        // The verb on top, nearest the thumb's reach from the title.
        el.child(
          div()
            .flex()
            .flex_col()
            .gap_2()
            .children(self.footer.into_iter().rev()),
        )
      });
    let panel = div()
      .id(SharedString::from(format!("{id}-panel")))
      .debug_selector(move || selector)
      .role(gpui::Role::Dialog)
      .when_some(self.title, |el, title| el.aria_label(title))
      .test_support()
      .occlude()
      .relative()
      .top(px(drag.offset.get()))
      .w_full()
      .max_h(gpui::relative(0.85))
      .overflow_y_scroll()
      .flex()
      .flex_col()
      .font_family(theme::SANS_FONT)
      .bg(p.background)
      .text_color(p.foreground)
      .rounded_t(px(theme::RADIUS_LG))
      .pb(inset + px(16.))
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .child(drag_tracker(drag, view, close.clone()))
      .child(handle)
      // A tall sheet scrolls; its parts keep their height.
      .children(
        self
          .children
          .into_iter()
          .map(|child| div().flex_none().child(child)),
      )
      .when(has_body, |el| el.child(body));
    div()
      .id(SharedString::from(format!("{id}-scrim")))
      .size_full()
      .flex()
      .flex_col()
      .justify_end()
      .bg(p.scrim())
      // The tap that closes the sheet must not reach the page below.
      .occlude()
      .when_some(self.focus, |el, focus| el.track_focus(&focus))
      .on_key_down(move |event: &KeyDownEvent, window, cx| {
        if event.keystroke.key == "escape" {
          cx.stop_propagation();
          close_on_key(window, cx);
        }
      })
      .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        close_on_scrim(window, cx)
      })
      .child(panel)
      .into_any_element()
  }
}

/// Records the sheet's height and, while a drag runs, moves the panel with
/// the pointer anywhere in the window and settles the drag on release. It
/// listens in the capture phase, on every paint: on a phone the bubble
/// phase does not see these events, and a cached frame replays the
/// listeners it was painted with.
fn drag_tracker(drag: Rc<SheetDrag>, view: EntityId, on_close: Handler) -> impl IntoElement {
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
          cx.notify(view);
          window.refresh();
        }
      });
      let on_close = on_close.clone();
      let drag = drag.clone();
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
        cx.notify(view);
        window.refresh();
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

/// A confirmation: the title is the question, the message says what will
/// happen, and the buttons are `[Cancel] [Verb]`, never Yes/No/OK. Enter
/// confirms and Escape cancels.
#[derive(IntoElement)]
pub struct ConfirmDialog {
  id: SharedString,
  palette: Palette,
  viewport: Option<Size<Pixels>>,
  question: SharedString,
  message: Option<SharedString>,
  verb: SharedString,
  destructive: bool,
  focus: Option<FocusHandle>,
  on_confirm: Option<Handler>,
  on_cancel: Option<Handler>,
  sheet: Option<Pixels>,
}

impl ConfirmDialog {
  /// `verb` labels the confirm button with what it does, such as
  /// "Initialize" or "Delete workspace".
  pub fn new(
    id: impl Into<SharedString>,
    question: impl Into<SharedString>,
    verb: impl Into<SharedString>,
    palette: &Palette,
  ) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      viewport: None,
      question: question.into(),
      message: None,
      verb: verb.into(),
      destructive: false,
      focus: None,
      on_confirm: None,
      on_cancel: None,
      sheet: None,
    }
  }

  /// See [`Dialog::sheet`]: the buttons stack full width at touch height.
  pub fn sheet(mut self, bottom_inset: Pixels) -> Self {
    self.sheet = Some(bottom_inset);
    self
  }

  pub fn message(mut self, message: impl Into<SharedString>) -> Self {
    self.message = Some(message.into());
    self
  }

  /// See [`Dialog::overlay`].
  pub fn overlay(mut self, viewport: Size<Pixels>) -> Self {
    self.viewport = Some(viewport);
    self
  }

  /// The verb destroys something: the danger variant.
  pub fn destructive(mut self) -> Self {
    self.destructive = true;
    self
  }

  pub fn focus(mut self, focus: &FocusHandle) -> Self {
    self.focus = Some(focus.clone());
    self
  }

  pub fn on_confirm(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_confirm = Some(Rc::new(f));
    self
  }

  pub fn on_cancel(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_cancel = Some(Rc::new(f));
    self
  }
}

impl RenderOnce for ConfirmDialog {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let sheet = self.sheet;
    let height = if sheet.is_some() {
      crate::press::MIN_TOUCH_TARGET
    } else {
      theme::CONTROL_LG
    };
    let cancel = self.on_cancel.clone();
    let confirm = self.on_confirm.clone();
    let cancel_button = Button::new(format!("{}-cancel", self.id), &p)
      .variant(Variant::Outline)
      .label("Cancel")
      .when(sheet.is_some(), |b| b.size(crate::Size::Lg).w_full())
      // A finger's target is in points, not rems.
      .map(|b| match sheet {
        Some(_) => b.h(px(height)),
        None => b.h(zpx(height)),
      })
      .when_some(cancel.clone(), |b, f| b.on_click(move |_, w, cx| f(w, cx)));
    let confirm_button = Button::new(format!("{}-confirm", self.id), &p)
      .variant(if self.destructive {
        Variant::Destructive
      } else {
        Variant::Default
      })
      .label(self.verb)
      .when(sheet.is_some(), |b| b.size(crate::Size::Lg).w_full())
      // A finger's target is in points, not rems.
      .map(|b| match sheet {
        Some(_) => b.h(px(height)),
        None => b.h(zpx(height)),
      })
      .when_some(confirm.clone(), |b, f| b.on_click(move |_, w, cx| f(w, cx)));
    Dialog::new(self.id, &p)
      .when_some(self.viewport, |d, viewport| d.overlay(viewport))
      .when_some(sheet, |d, inset| d.sheet(inset))
      .width(400.)
      .title(self.question)
      .when_some(self.message, |d, message| d.description(message))
      .footer([
        cancel_button.into_any_element(),
        confirm_button.into_any_element(),
      ])
      .when_some(self.focus, |d, focus| d.focus(&focus))
      .when_some(cancel, |d, f| d.on_close(move |w, cx| f(w, cx)))
      .when_some(confirm, |d, f| d.on_primary(move |w, cx| f(w, cx)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn viewport() -> Size<Pixels> {
    gpui::size(px(800.), px(600.))
  }

  #[test]
  fn test_dialog_builder() {
    let p = theme::palette(false);
    let dialog = Dialog::new("settings", &p)
      .overlay(viewport())
      .width(520.)
      .title("Settings")
      .description("Changes apply at once.")
      .child("Body")
      .on_close(|_, _| {});
    assert_eq!(dialog.width, 520.);
    assert!(dialog.footer.is_empty() && dialog.on_close.is_some());
  }

  #[test]
  fn test_confirm_dialog_builder() {
    let p = theme::palette(true);
    let confirm = ConfirmDialog::new(
      "init-repo",
      "Initialize a Git repository here?",
      "Initialize",
      &p,
    )
    .overlay(viewport())
    .message("Treq needs one to create workspaces.")
    .destructive()
    .on_confirm(|_, _| {})
    .on_cancel(|_, _| {});
    assert!(confirm.destructive);
    assert_eq!(confirm.verb.as_ref(), "Initialize");
  }

  #[test]
  fn test_dialog_layout_builder() {
    let p = theme::palette(false);
    let dialog = Dialog::new("stash", &p)
      .overlay(viewport())
      .priority(3)
      .height(480.)
      .title("Stashes")
      .header(div().child("Custom header"))
      .primary_disabled(true);
    assert_eq!(dialog.priority, 3);
    assert_eq!(dialog.height, Some(480.));
    assert!(dialog.header.is_some() && dialog.primary_disabled);
  }

  #[test]
  fn test_dialog_relative_size_builder() {
    let p = theme::palette(false);
    let dialog = Dialog::new("history", &p).relative_size(0.65, Some(0.75));
    assert_eq!(dialog.relative, Some((0.65, Some(0.75))));
  }

  mod keys {
    use std::cell::RefCell;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    #[derive(Default)]
    struct Log(RefCell<Vec<&'static str>>);

    struct Host {
      log: Rc<Log>,
      busy: bool,
    }

    impl Render for Host {
      fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        let close = self.log.clone();
        let primary = self.log.clone();
        div().size_full().child(
          Dialog::new("keys", &p)
            .title("Rename workspace")
            .primary_disabled(self.busy)
            .footer([div().child("Rename").into_any_element()])
            .on_close(move |_, _| close.0.borrow_mut().push("close"))
            .on_primary(move |_, _| primary.0.borrow_mut().push("primary")),
        )
      }
    }

    fn mount(cx: &mut TestAppContext, busy: bool) -> (VisualTestContext, Rc<Log>) {
      let log = Rc::new(Log::default());
      let window = cx.add_window({
        let log = log.clone();
        move |_, _| Host { log, busy }
      });
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      cx.run_until_parked();
      cx.update(|window, cx| window.draw(cx).clear(cx));
      (cx, log)
    }

    /// With no focus handle of its own, the dialog takes focus as it opens,
    /// so Escape and Enter reach it.
    #[gpui::test]
    fn a_dialog_without_a_focus_handle_hears_escape_and_enter(cx: &mut TestAppContext) {
      let (mut cx, log) = mount(cx, false);
      cx.simulate_keystrokes("escape");
      cx.simulate_keystrokes("enter");
      assert_eq!(*log.0.borrow(), ["close", "primary"]);
    }

    /// Ctrl or Cmd+Enter submits too, as it does from a multi-line field;
    /// Shift+Enter and a modified Escape don't.
    #[gpui::test]
    fn secondary_enter_runs_the_primary(cx: &mut TestAppContext) {
      let (mut cx, log) = mount(cx, false);
      cx.simulate_keystrokes("ctrl-enter");
      cx.simulate_keystrokes("shift-enter");
      cx.simulate_keystrokes("ctrl-escape");
      assert_eq!(*log.0.borrow(), ["primary"]);
    }

    /// While the verb is busy or disabled, Enter does nothing.
    #[gpui::test]
    fn enter_skips_a_disabled_primary(cx: &mut TestAppContext) {
      let (mut cx, log) = mount(cx, true);
      cx.simulate_keystrokes("enter");
      cx.simulate_keystrokes("escape");
      assert_eq!(*log.0.borrow(), ["close"]);
    }
  }

  mod sheet {
    use std::cell::Cell;

    use gpui::{Modifiers, MouseButton, Point, TestAppContext, VisualTestContext};

    use super::*;

    struct Host {
      closed: Rc<Cell<bool>>,
    }

    impl Render for Host {
      fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        let closed = self.closed.clone();
        div().size_full().child(
          Dialog::new("sheet", &p)
            .sheet(px(0.))
            .title("Delete workspace?")
            .description("Its files are removed.")
            .on_close(move |_, _| closed.set(true))
            .child(div().h(px(250.))),
        )
      }
    }

    fn mount(cx: &mut TestAppContext) -> (VisualTestContext, Rc<Cell<bool>>) {
      let closed = Rc::new(Cell::new(false));
      let window = cx.add_window({
        let closed = closed.clone();
        move |_, _| Host { closed }
      });
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      cx.run_until_parked();
      cx.update(|window, cx| window.draw(cx).clear(cx));
      (cx, closed)
    }

    /// Drags from the grabber down by `dy`, in four moves.
    fn drag(cx: &mut VisualTestContext, dy: f32) {
      let panel = cx.debug_bounds("sheet").expect("sheet panel");
      let start = Point::new(panel.center().x, panel.origin.y + px(10.));
      cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
      for step in 1..=4 {
        let at = Point::new(start.x, start.y + px(dy * step as f32 / 4.));
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
      }
      let end = Point::new(start.x, start.y + px(dy));
      cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    }

    /// A sheet spans the window's width along its bottom edge.
    #[gpui::test]
    fn a_sheet_is_anchored_to_the_bottom(cx: &mut TestAppContext) {
      let (mut cx, _) = mount(cx);
      let viewport = cx.update(|window, _| window.viewport_size());
      let panel = cx.debug_bounds("sheet").expect("sheet panel");
      assert_eq!(panel.bottom(), viewport.height);
      assert_eq!(panel.size.width, viewport.width);
      assert!(cx.debug_bounds("sheet-grabber").is_some());
    }

    /// Dragging the grabber down past a quarter of the sheet closes it.
    #[gpui::test]
    fn a_long_drag_down_closes_the_sheet(cx: &mut TestAppContext) {
      let (mut cx, closed) = mount(cx);
      drag(&mut cx, 200.);
      assert!(closed.get(), "sheet should close");
    }

    /// A short, slow drag snaps back and leaves the sheet open.
    #[gpui::test]
    fn a_short_drag_leaves_the_sheet_open(cx: &mut TestAppContext) {
      let (mut cx, closed) = mount(cx);
      drag(&mut cx, 20.);
      assert!(!closed.get(), "sheet should stay open");
    }

    /// The title outranks the body text it introduces.
    #[gpui::test]
    fn the_title_is_larger_than_the_body(cx: &mut TestAppContext) {
      let (mut cx, _) = mount(cx);
      let title = cx.debug_bounds("sheet-title").expect("title");
      let body = cx.debug_bounds("sheet-description").expect("description");
      assert!(title.size.height > body.size.height);
      const { assert!(SHEET_TITLE_PX > SHEET_BODY_PX) };
    }

    #[test]
    fn test_confirm_sheet_builder() {
      let p = theme::palette(false);
      let confirm =
        ConfirmDialog::new("delete", "Delete workspace?", "Delete workspace", &p).sheet(px(34.));
      assert_eq!(confirm.sheet, Some(px(34.)));
    }
  }
}
