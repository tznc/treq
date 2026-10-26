//! Bottom sheets: one at a time, over the whole window, above a scrim that
//! closes it. Menus (a list of actions) and confirmations are sheets; so is
//! any view a screen opens with [`open`].

use std::{rc::Rc, time::Duration};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Sizable,
  },
  div, px, relative, AnyView, App, AppContext, Context, Entity, Global, InteractiveElement,
  IntoElement, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled,
  WeakEntity, Window,
};

use super::widgets;

/// What a sheet action does when tapped. The sheet closes first.
pub type Action = Rc<dyn Fn(&mut Window, &mut App)>;

/// The open sheet, owned by the root view.
pub struct Sheet {
  pub title: Option<SharedString>,
  pub view: AnyView,
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
            view: view.into(),
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
  let theme = cx.theme();
  let palette = treq_ui::theme::current(cx);
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
        .on_click(|_, _, cx| close(cx)),
    )
    .child(
      v_flex()
        .id("sheet")
        .occlude()
        .max_h(relative(0.85))
        .overflow_y_scroll()
        .bg(theme.background)
        .rounded_t(theme.radius_lg)
        .pb(bottom_inset + px(16.))
        .child(
          div()
            .flex()
            .flex_none()
            .justify_center()
            .pt_2()
            .child(div().w(px(36.)).h(px(4.)).rounded_full().bg(theme.border)),
        )
        .when_some(sheet.title.clone(), |panel, title| {
          panel.child(
            div()
              .flex_none()
              .px_4()
              .pt_2()
              .pb_1()
              .text_sm()
              .text_color(theme.muted_foreground)
              .truncate()
              .child(title),
          )
        })
        // A tall sheet scrolls; its parts keep their height.
        .child(div().flex_none().child(sheet.view.clone())),
    )
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
      .child(widgets::muted(self.message.clone(), cx).text_base())
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
              }),
          )
          .child(
            Button::new("cancel")
              .outline()
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .label("Cancel")
              .on_click(|_, _, cx| close(cx)),
          ),
      )
  }
}
