//! Hover tooltips drawn in popup windows, as base-ui's `Tooltip` shows
//! them: after a delay, next to the hovered element, gone on a click.
//!
//! A view keeps a [`Tooltips`] and wraps elements with [`Tooltips::wrap`];
//! [`Tooltips::sync`] at the start of its render closes the popup once no
//! tooltip shows.

use crate::result_ext::ResultExt;
use std::{cell::RefCell, rc::Rc, time::Duration};

use tauri_plugin_gpui::gpui::{self, div, prelude::*, App, Context, Div, SharedString, Stateful};

use super::popup::{PopupRequest, PopupSink, Side, TooltipContent};

/// React's base-ui tooltips open after 200ms (`TooltipProvider delay={200}`).
pub const DELAY: Duration = Duration::from_millis(200);

/// A view that shows tooltips.
pub trait HasTooltips: 'static + Sized {
  fn tooltips(&mut self) -> &mut Tooltips;
}

pub struct Tooltips {
  popups: PopupSink,
  pub dark: bool,
  /// The element whose tooltip is pending or shown (`true`).
  current: Option<(SharedString, bool)>,
  /// The tooltip whose popup is open.
  sent: Rc<RefCell<Option<SharedString>>>,
}

impl Tooltips {
  pub fn new(popups: PopupSink, dark: bool) -> Self {
    Self {
      popups,
      dark,
      current: None,
      sent: Rc::default(),
    }
  }

  /// Closes the tooltip popup once no tooltip shows.
  pub fn sync(&self, cx: &mut App) {
    let shown = matches!(self.current, Some((_, true)));
    if !shown && self.sent.borrow_mut().take().is_some() {
      (self.popups)(PopupRequest::HideTooltip, cx);
    }
  }

  fn hover<V: HasTooltips>(key: SharedString, hovered: bool, view: &mut V, cx: &mut Context<V>) {
    let tips = view.tooltips();
    if !hovered {
      if tips.current.as_ref().is_some_and(|(k, _)| *k == key) {
        tips.current = None;
        cx.notify();
      }
      return;
    }
    tips.current = Some((key.clone(), false));
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(DELAY).await;
      this
        .update(cx, |view, cx| {
          if let Some((k, shown)) = view.tooltips().current.as_mut() {
            if *k == key && !*shown {
              *shown = true;
              cx.notify();
            }
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Wraps `el` so hovering it shows `content` on `side` of it. `key`
  /// identifies the element among the view's tooltips.
  pub fn wrap<V: HasTooltips>(
    &self,
    key: impl Into<SharedString>,
    side: Side,
    content: Option<TooltipContent>,
    el: impl IntoElement,
    cx: &mut Context<V>,
  ) -> Stateful<Div> {
    let key: SharedString = key.into();
    let shown = matches!(&self.current, Some((k, true)) if *k == key);
    let wrapper = div()
      .id(SharedString::from(format!("{key}-tooltip")))
      .relative()
      .child(el);
    let Some(content) = content else {
      return wrapper;
    };
    let hover_key = key.clone();
    wrapper
      .on_hover(cx.listener(move |view, hovered: &bool, _, cx| {
        Self::hover(hover_key.clone(), *hovered, view, cx);
      }))
      .on_mouse_down(
        gpui::MouseButton::Left,
        cx.listener(|view, _, _, cx| {
          if view.tooltips().current.take().is_some() {
            cx.notify();
          }
        }),
      )
      .when(shown, |this| {
        let (sent, popups, dark) = (self.sent.clone(), self.popups.clone(), self.dark);
        this.child(
          gpui::canvas(
            move |anchor, _, cx| {
              if sent.borrow().as_ref() != Some(&key) {
                *sent.borrow_mut() = Some(key);
                popups(
                  PopupRequest::ShowTooltip {
                    anchor,
                    side,
                    content,
                    dark,
                  },
                  cx,
                );
              }
            },
            |_, _, _, _| {},
          )
          .absolute()
          .top_0()
          .left_0()
          .size_full(),
        )
      })
  }
}
