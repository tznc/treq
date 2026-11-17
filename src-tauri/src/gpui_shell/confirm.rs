//! In-app confirmations ([`ui::ConfirmDialog`]): the title is the question,
//! the message says what will happen, and the buttons are Cancel and the
//! verb. Enter confirms; Escape and a click on the scrim cancel. A view
//! asks one question at a time: asking again while one is open does
//! nothing, so a second click on Delete can't stack another dialog.

use crate::result_ext::ResultExt;
use std::rc::Rc;

use tauri_plugin_gpui::gpui::{prelude::*, App, Context, FocusHandle, Window};

use super::{theme::Palette, ui};

/// The dialog's id; its buttons are `confirm-cancel` and `confirm-confirm`.
pub const ID: &str = "confirm";

/// What a confirmation says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
  pub question: String,
  pub message: String,
  /// The confirm button: what it does, such as "Delete workspace".
  pub verb: String,
  pub destructive: bool,
}

impl Question {
  pub fn new(
    question: impl Into<String>,
    message: impl Into<String>,
    verb: impl Into<String>,
  ) -> Self {
    Self {
      question: question.into(),
      message: message.into(),
      verb: verb.into(),
      destructive: false,
    }
  }

  /// The verb destroys something.
  pub fn destructive(mut self) -> Self {
    self.destructive = true;
    self
  }

  /// The dialog over the whole window, taking Enter and Escape while
  /// `focus` is focused; `answer` hears true for the verb.
  pub fn confirm_dialog(
    &self,
    focus: &FocusHandle,
    window: &Window,
    p: &Palette,
    answer: impl Fn(bool, &mut Window, &mut App) + 'static,
  ) -> ui::ConfirmDialog {
    let yes = Rc::new(answer);
    let no = yes.clone();
    ui::ConfirmDialog::new(ID, self.question.clone(), self.verb.clone(), p)
      .overlay(window.viewport_size())
      .message(self.message.clone())
      .when(self.destructive, |d| d.destructive())
      .focus(focus)
      .on_confirm(move |w, cx| yes(true, w, cx))
      .on_cancel(move |w, cx| no(false, w, cx))
  }
}

/// What a view's confirmation runs once confirmed.
type Confirmed<V> = Box<dyn FnOnce(&mut V, &mut Context<V>)>;

/// A view's open confirmation, and what its verb runs.
pub struct Pending<V> {
  question: Question,
  confirmed: Confirmed<V>,
  focus: Option<FocusHandle>,
}

/// Where a view keeps its open confirmation.
pub type Slot<V> = fn(&mut V) -> &mut Option<Pending<V>>;

/// Opens `question` in `slot`, unless one is already open; the verb runs
/// `confirmed`.
pub fn ask<V: 'static>(
  slot: &mut Option<Pending<V>>,
  question: Question,
  confirmed: impl FnOnce(&mut V, &mut Context<V>) + 'static,
  cx: &mut Context<V>,
) {
  if slot.is_none() {
    *slot = Some(Pending {
      question,
      confirmed: Box::new(confirmed),
      focus: None,
    });
    cx.notify();
  }
}

/// Draws `slot`'s confirmation, if one is open, focusing it as it opens so
/// it hears Enter and Escape.
pub fn render<V: 'static>(
  slot: Slot<V>,
  view: &mut V,
  window: &mut Window,
  p: &Palette,
  cx: &mut Context<V>,
) -> Option<ui::ConfirmDialog> {
  let pending = slot(view).as_mut()?;
  let focus = pending
    .focus
    .get_or_insert_with(|| {
      let focus = cx.focus_handle();
      focus.focus(window, cx);
      focus
    })
    .clone();
  let this = cx.weak_entity();
  Some(
    pending
      .question
      .confirm_dialog(&focus, window, p, move |yes, _, cx| {
        this
          .update(cx, |view, cx| answer(slot, view, yes, cx))
          .ignore_closed();
      }),
  )
}

/// Closes `slot`'s confirmation; the verb (`yes`) runs its work.
pub fn answer<V: 'static>(slot: Slot<V>, view: &mut V, yes: bool, cx: &mut Context<V>) {
  let Some(pending) = slot(view).take() else {
    return;
  };
  if yes {
    (pending.confirmed)(view, cx);
  }
  cx.notify();
}
