//! Deleting the Treq account, from Home's Account section: a sheet that says
//! what goes and wants the email typed before its button works. The
//! `delete-account` Edge Function cancels the user's subscription and
//! destroys their Treq Cloud machines; then the app signs out and Home says
//! the account is gone. A refusal (the user is the last owner of an
//! organization) shows the server's sentence.

use gpui_kit::component::Disableable as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    v_flex, Sizable,
  },
  *,
};

use super::{load, model, sheet, widgets};
use crate::backend;

/// The message Home shows once the account is gone.
pub const DELETED: &str = "Your Treq account was deleted.";

/// Whether `typed` confirms deleting the account of `email`.
fn confirms(typed: &str, email: &str) -> bool {
  typed.trim().eq_ignore_ascii_case(email)
}

/// What the sheet shows for a failed deletion.
fn failure(error: &treq_lib::core::supabase::ApiError) -> String {
  if error.status == 0 {
    format!("Treq could not reach the server: {}", error.message)
  } else {
    error.message.clone()
  }
}

/// Opens the sheet for the account signed in as `email`.
pub fn open(email: String, cx: &mut App) {
  sheet::open(
    Some("Delete your Treq account?".into()),
    cx,
    move |window, cx| cx.new(|cx| DeleteAccountSheet::new(email, window, cx)),
  );
}

struct DeleteAccountSheet {
  email: String,
  input: Entity<InputState>,
  deleting: bool,
  error: Option<String>,
}

impl DeleteAccountSheet {
  fn new(email: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(email.clone()));
    cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        this.error = None;
        cx.notify();
      }
    })
    .detach();
    Self {
      email,
      input,
      deleting: false,
      error: None,
    }
  }

  fn confirmable(&self, cx: &App) -> bool {
    !self.deleting && confirms(&self.input.read(cx).value(), &self.email)
  }

  fn delete(&mut self, cx: &mut Context<Self>) {
    if !self.confirmable(cx) {
      return;
    }
    self.deleting = true;
    self.error = None;
    load::run(
      cx,
      async {
        let supabase = &backend::get().supabase;
        supabase
          .invoke::<serde_json::Value>("delete-account", &serde_json::json!({}))
          .await?;
        // The user no longer exists, so the session is forgotten here
        // without a logout call; `sign_out` then cuts off the machines.
        supabase.auth.forget_session();
        Ok::<(), treq_lib::core::supabase::ApiError>(())
      },
      |this, result, cx| {
        this.deleting = false;
        match result {
          Ok(()) => {
            sheet::close(cx);
            super::sign_out(cx);
            model(cx).update(cx, |m, cx| {
              m.auth_notice = Some(DELETED.into());
              cx.notify();
            });
          }
          Err(error) => this.error = Some(failure(&error)),
        }
      },
    );
    cx.notify();
  }
}

impl Render for DeleteAccountSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let confirmable = self.confirmable(cx);
    v_flex()
      .px_4()
      .pt_1()
      .gap_4()
      .child(
        widgets::muted(
          "Your Pro subscription is cancelled at once, without a refund. Your Treq Cloud machines and their disks are deleted. Billing records are kept, and your own SSH hosts are not touched. If you are the last owner of an organization, make another member an owner or delete the organization first.",
          cx,
        )
        .text_base(),
      )
      .child(
        v_flex()
          .gap_2()
          .child(widgets::muted(format!("Type {} to confirm.", self.email), cx))
          .child(Input::new(&self.input).large().disabled(self.deleting)),
      )
      .children(self.error.clone().map(|e| widgets::error_text(e, cx)))
      .child(
        v_flex()
          .gap_2()
          .child(
            Button::new("delete-account-confirm")
              .danger()
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .label("Delete account")
              .loading(self.deleting)
              .disabled(!confirmable)
              .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
          )
          .child(
            Button::new("delete-account-cancel")
              .outline()
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .label("Cancel")
              .disabled(self.deleting)
              .on_click(|_, _, cx| sheet::close(cx)),
          ),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[core::prelude::v1::test]
  fn the_email_confirms_whatever_its_case_and_spacing() {
    assert!(confirms(" Ada@Example.com ", "ada@example.com"));
    assert!(!confirms("ada@example.org", "ada@example.com"));
    assert!(!confirms("", "ada@example.com"));
  }

  #[core::prelude::v1::test]
  fn a_refusal_shows_the_server_sentence() {
    let refused = treq_lib::core::supabase::ApiError {
      status: 409,
      code: "last_owner".into(),
      message: "You are the last owner of Acme. Make another member an owner or delete Acme first."
        .into(),
      correlation_id: None,
    };
    assert_eq!(failure(&refused), refused.message);
    let offline = treq_lib::core::supabase::ApiError::new("network", "timed out");
    assert_eq!(
      failure(&offline),
      "Treq could not reach the server: timed out"
    );
  }
}
