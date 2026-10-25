//! Deleting the account: the danger row at the bottom of the Account tab,
//! and the dialog it opens. The user types their email to enable the
//! destructive button. The server cancels the subscription and destroys the
//! Treq Cloud machines, then Rust signs out; a refusal (such as being the
//! only owner of an organization with other members) shows its sentence in
//! the dialog.

use tauri_plugin_gpui::gpui::{div, prelude::*, Context, FocusHandle, Focusable as _, Window};

use super::{setting_row, SettingsView};
use crate::gpui_shell::{
  form,
  native::ToastTone,
  screen::{settings::toast, ScreenAction},
  theme::Palette,
  ui::{self, Button, Size, TextSize, Variant},
};

/// The press ids and field the dialog uses.
const ASK: &str = "account.delete.ask";
const CANCEL: &str = "account.delete.cancel";
const CONFIRM: &str = "account.delete.confirm";
pub(super) const FIELD: &str = "account.delete.confirmText";

fn press(id: &str) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

/// What the user types when the account has no email.
const FALLBACK_CONFIRM_TEXT: &str = "delete my account";

/// The open dialog's own state.
#[derive(Default)]
pub(super) struct Deletion {
  typed: String,
  deleting: bool,
  error: Option<String>,
  /// Takes Enter and Escape; the field takes focus on the first draw.
  focus: Option<FocusHandle>,
}

/// The dialog, as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountDeletion {
  /// What the user types to confirm: their email.
  pub confirm_text: String,
  pub typed: String,
  pub deleting: bool,
  /// Why the last attempt failed, in the server's words.
  pub error: Option<String>,
}

impl AccountDeletion {
  /// The destructive button is on: the email matches and nothing runs.
  pub fn confirmable(&self) -> bool {
    !self.deleting && self.typed.trim().eq_ignore_ascii_case(&self.confirm_text)
  }
}

impl Deletion {
  pub(super) fn drawn(&self, email: Option<&str>) -> AccountDeletion {
    AccountDeletion {
      confirm_text: email.unwrap_or(FALLBACK_CONFIRM_TEXT).to_string(),
      typed: self.typed.clone(),
      deleting: self.deleting,
      error: self.error.clone(),
    }
  }
}

impl SettingsView {
  /// The dialog's buttons; false for the rest.
  pub(super) fn press_deletion(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
    let deletion = &mut self.account.deletion;
    match id {
      ASK => *deletion = Some(Deletion::default()),
      CANCEL if !deletion.as_ref().is_some_and(|d| d.deleting) => *deletion = None,
      CANCEL => {}
      CONFIRM => self.delete_account(cx),
      _ => return false,
    }
    true
  }

  pub(super) fn set_deletion_text(&mut self, text: String) {
    if let Some(deletion) = &mut self.account.deletion {
      deletion.typed = text;
      deletion.error = None;
    }
  }

  fn delete_account(&mut self, cx: &mut Context<Self>) {
    let email = self
      .account
      .rust
      .user
      .as_ref()
      .and_then(|u| u.email.clone());
    let Some(deletion) = &mut self.account.deletion else {
      return;
    };
    if !deletion.drawn(email.as_deref()).confirmable() {
      return;
    }
    deletion.deleting = true;
    deletion.error = None;
    self.spawn(
      cx,
      |host| host.delete_account(),
      |view, result, _| match result {
        Ok(()) => {
          view.account.deletion = None;
          view.host.notify(toast(
            ToastTone::Success,
            "Account deleted",
            "Repositories and workspaces on this computer are unchanged.",
          ));
        }
        Err(error) => {
          if let Some(deletion) = &mut view.account.deletion {
            deletion.deleting = false;
            deletion.error = Some(if error.status == 0 {
              format!("Treq could not reach the server: {}", error.message)
            } else {
              error.message
            });
          }
        }
      },
    );
  }

  /// "Delete Account…", in its own bordered row under Sign Out.
  pub(super) fn deletion_row(&self, p: &Palette) -> impl IntoElement {
    div()
      .border_1()
      .border_color(ui::Tone::Danger.border(p))
      .rounded(ui::radius::MD)
      .px_4()
      .child(setting_row(
        "Delete account",
        "Permanently deletes your Treq account and cancels your subscription.",
        Button::new("account-delete", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .text_color(p.destructive)
          .label("Delete Account…")
          .on_click(self.press(ASK)),
        p,
      ))
  }

  /// The confirmation over the whole window.
  pub(in crate::gpui_shell::screen) fn deletion_dialog(
    &mut self,
    d: &AccountDeletion,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let input = self
      .controls
      .input(FIELD, &d.typed, &d.confirm_text, window, cx);
    let focus = match self.account.deletion.as_mut() {
      Some(Deletion {
        focus: Some(focus), ..
      }) => focus.clone(),
      state => {
        let focus = cx.focus_handle();
        input.read(cx).focus_handle(cx).focus(window, cx);
        if let Some(state) = state {
          state.focus = Some(focus.clone());
        }
        focus
      }
    };
    let (cancel, confirm) = (self.sink.clone(), self.sink.clone());
    ui::Dialog::new("account-delete-dialog", p)
      .overlay(window.viewport_size())
      .width(460.)
      .title("Delete your Treq account?")
      .description(
        "Your Pro or Team subscription is cancelled now, and your Treq Cloud machines and their disks are deleted. Repositories and workspaces on this computer are not touched.",
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_1p5()
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child(format!("Type {} to confirm.", d.confirm_text)),
          )
          .child(form::input_field(
            "account-delete-confirm-text",
            &input,
            false,
            d.deleting,
          )),
      )
      .when_some(d.error.clone(), |dialog, error| {
        dialog.child(
          ui::Alert::new("account-delete-error", "Your account was not deleted", p)
            .severity(ui::Severity::Danger)
            .message(error),
        )
      })
      .footer([
        Button::new("account-delete-cancel", p)
          .variant(Variant::Outline)
          .disabled(d.deleting)
          .label("Cancel")
          .on_click(self.press(CANCEL))
          .into_any_element(),
        Button::new("account-delete-confirm", p)
          .variant(Variant::Destructive)
          .disabled(!d.confirmable())
          .when(d.deleting, |b| {
            b.child(ui::spinner(
              "account-delete-spinner",
              14.,
              p.destructive_foreground,
            ))
          })
          .label("Delete Account")
          .on_click(self.press(CONFIRM))
          .into_any_element(),
      ])
      .focus(&focus)
      .on_close(move |_, cx| cancel(press(CANCEL), cx))
      .on_primary(move |_, cx| confirm(press(CONFIRM), cx))
  }
}
