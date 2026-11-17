//! A shell window's toasts, drawn in that window above its screen and
//! dialogs, in its bottom-left corner. `toasts` keeps each window's list.

use std::rc::Rc;

use tauri_plugin_gpui::gpui::{self, div, prelude::*, svg, AnyElement, App, SharedString};

use super::{
  popup::{is_compact, Toast, ToastKind},
  theme::{self, Palette},
  ui::{self, zpx, TextSize},
};

/// A click on toast `id`: its action (`true`) or close button.
pub type ToastClick = Rc<dyn Fn(&str, bool, &mut App)>;

const INSET: f32 = 16.;
const GAP: f32 = 8.;
const MIN_WIDTH: f32 = 320.;
const MAX_WIDTH: f32 = 384.;

fn clipboard_text(toast: &Toast) -> String {
  match &toast.description {
    Some(description) => format!("{}: {}", toast.title, description),
    None => toast.title.clone(),
  }
}

/// The toast stack, `fixed bottom-4 left-4 flex flex-col gap-2`; `None`
/// without toasts.
pub fn toast_layer(toasts: &[Toast], palette: Palette, on_click: ToastClick) -> Option<AnyElement> {
  if toasts.is_empty() {
    return None;
  }
  let stack = ui::text(div(), TextSize::Sm)
    .id("toasts")
    .absolute()
    .bottom(zpx(INSET))
    .left(zpx(INSET))
    // Clicks on a toast stay off the screen and dialogs below it.
    .occlude()
    .flex()
    .flex_col()
    .gap(zpx(GAP))
    .font_family(theme::SANS_FONT)
    .children(
      toasts
        .iter()
        .map(|toast| item(toast, palette, on_click.clone())),
    );
  Some(stack.into_any_element())
}

/// A `ToastItem`: `flex items-start gap-3 rounded-lg border bg-popover p-4
/// shadow-lg`.
fn item(toast: &Toast, palette: Palette, on_click: ToastClick) -> AnyElement {
  let (icon, icon_color) = match toast.kind {
    ToastKind::Error => (theme::ICON_CIRCLE_ALERT, palette.destructive),
    ToastKind::Info => (theme::ICON_INFO, palette.primary),
    ToastKind::Success => (theme::ICON_CIRCLE_CHECK, palette.toast_success),
    ToastKind::Warning => (theme::ICON_TRIANGLE_ALERT, palette.toast_warning),
  };
  let compact = is_compact(toast);
  let selector = format!("toast-{}", toast.id);
  let close_selector = format!("toast-close-{}", toast.id);
  let (action_id, close_id) = (toast.id.clone(), toast.id.clone());
  let on_action = on_click.clone();
  div()
    .id(SharedString::from(selector.clone()))
    .debug_selector(move || selector)
    .flex()
    .items_start()
    .gap_3()
    .p_4()
    .min_w(zpx(if compact { 0. } else { MIN_WIDTH }))
    .max_w(zpx(MAX_WIDTH))
    .rounded_lg()
    .shadow_lg()
    .border_1()
    .border_color(palette.border)
    .bg(palette.popover)
    .text_color(palette.popover_foreground)
    .child(
      svg()
        .path(icon)
        .size(zpx(16.))
        .when(!compact, |this| this.mt_0p5())
        .flex_none()
        .text_color(icon_color),
    )
    .child(
      div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(
          ui::text(div(), TextSize::Sm)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(SharedString::from(toast.title.clone())),
        )
        .when_some(toast.description.clone(), |this, description| {
          this.child(
            div()
              .text_color(palette.muted_foreground)
              .child(SharedString::from(description)),
          )
        })
        .when_some(toast.action.clone(), |this, action| {
          // `mt-1 font-medium text-primary underline`
          this.child(
            div()
              .id("action")
              .role(gpui::Role::Button)
              .aria_label(SharedString::from(action.clone()))
              .mt_1()
              .font_weight(gpui::FontWeight::MEDIUM)
              .text_color(palette.primary)
              .underline()
              .hover(|style| style.text_decoration_none())
              .cursor_pointer()
              .child(SharedString::from(action))
              .on_click(move |_, _, cx| on_action(&action_id, true, cx)),
          )
        }),
    )
    .when(toast.kind == ToastKind::Error, |this| {
      // Copies the error so it can be pasted into a report or a search.
      this.child(ui::copy_feedback::CopyButton::new(
        SharedString::from(format!("toast-copy-{}", toast.id)),
        clipboard_text(toast),
        &palette,
      ))
    })
    .when(!compact, |this| {
      // `rounded-md p-1 text-muted-foreground/70 hover:bg-accent
      // hover:text-foreground`
      this.child(
        div()
          .id("close")
          .debug_selector(move || close_selector)
          .role(gpui::Role::Button)
          .aria_label("Dismiss")
          .group("close")
          .flex_none()
          .p_1()
          .rounded_md()
          .hover(|style| style.bg(palette.hover()))
          .cursor_pointer()
          .child(
            svg()
              .path(theme::ICON_X)
              .size(zpx(16.))
              .text_color(palette.muted_foreground.opacity(0.7))
              .group_hover("close", |style| style.text_color(palette.foreground)),
          )
          .on_click(move |_, _, cx| on_click(&close_id, false, cx)),
      )
    })
    .into_any_element()
}

#[cfg(test)]
mod tests {
  use std::cell::{Cell, RefCell};

  use gpui::{Modifiers, TestAppContext, Window};

  use super::*;

  /// A window with a toast over a full-window dialog scrim.
  struct Host {
    toasts: Vec<Toast>,
    dialog: bool,
    clicks: Rc<RefCell<Vec<(String, bool)>>>,
    scrim_clicks: Rc<Cell<usize>>,
  }

  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
      let clicks = self.clicks.clone();
      let scrim_clicks = self.scrim_clicks.clone();
      div()
        .size_full()
        .relative()
        .when(self.dialog, |this| {
          this.child(
            div()
              .id("scrim")
              .debug_selector(|| "scrim".into())
              .absolute()
              .inset_0()
              .occlude()
              .on_click(move |_, _, _| scrim_clicks.set(scrim_clicks.get() + 1)),
          )
        })
        .children(toast_layer(
          &self.toasts,
          theme::palette(true),
          Rc::new(move |id, action, _| clicks.borrow_mut().push((id.to_string(), action))),
        ))
    }
  }

  fn toast() -> Toast {
    Toast {
      id: "1".into(),
      title: "JJ Error".into(),
      description: Some("Failed to resolve revset".into()),
      kind: ToastKind::Error,
      action: None,
    }
  }

  #[gpui::test]
  fn toasts_stay_in_their_window_over_and_after_a_dialog(cx: &mut TestAppContext) {
    cx.update(theme::init);
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let scrim_clicks = Rc::new(Cell::new(0));
    let (host, cx) = cx.add_window_view(|_, _| Host {
      toasts: vec![toast()],
      dialog: true,
      clicks: clicks.clone(),
      scrim_clicks: scrim_clicks.clone(),
    });
    cx.run_until_parked();
    let toast_bounds = cx.debug_bounds("toast-1").expect("toast over the dialog");
    let window = cx.debug_bounds("scrim").expect("scrim");
    assert!(toast_bounds.left() > window.left());
    assert!(toast_bounds.bottom() < window.bottom());

    // Closing the dialog leaves the toast where it was.
    host.update(cx, |host, cx| {
      host.dialog = false;
      cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("toast-1"), Some(toast_bounds));

    // Its close button takes the click; nothing below it does.
    host.update(cx, |host, cx| {
      host.dialog = true;
      cx.notify();
    });
    cx.run_until_parked();
    let close = cx.debug_bounds("toast-close-1").expect("close button");
    cx.simulate_click(close.center(), Modifiers::none());
    assert_eq!(*clicks.borrow(), vec![("1".to_string(), false)]);
    assert_eq!(scrim_clicks.get(), 0);
  }
}
