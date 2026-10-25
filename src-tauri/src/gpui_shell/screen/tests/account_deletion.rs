//! Deleting the account from the Account tab: the danger row, the dialog
//! that wants the email typed, and what the server's answer does.

use gpui::TestAppContext;

use super::*;

fn signed_in(cx: &mut TestAppContext, host: FakeHost) -> (Harness, Arc<FakeHost>) {
  let host = Arc::new(host);
  let mut h = Harness::with_host(cx, account_route("account"), host.clone());
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": pro() }),
  ));
  (h, host)
}

/// The dialog as last drawn, if open.
fn deletion(h: &mut Harness) -> Option<settings::AccountDeletion> {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => signed_in.deletion,
    other => panic!("signed in: {other:?}"),
  }
}

fn type_email(h: &mut Harness, text: &str) {
  h.click("account-delete-confirm-text");
  h.cx.simulate_input(text);
}

#[gpui::test]
fn deleting_the_account_waits_for_the_email(cx: &mut TestAppContext) {
  let (mut h, host) = signed_in(cx, FakeHost::default());
  assert_eq!(deletion(&mut h), None);
  h.click("account-delete");
  assert!(h.drawn("account-delete-dialog"));
  let open = deletion(&mut h).expect("dialog open");
  assert_eq!(open.confirm_text, "ada@example.com");
  assert!(!open.confirmable());

  h.click("account-delete-confirm");
  type_email(&mut h, "ada@example.org");
  assert!(!deletion(&mut h).expect("dialog open").confirmable());
  assert!(!host.writes().contains(&"delete account".to_string()));

  // "ada@example.org" becomes "ada@example.com ", which counts.
  h.cx.simulate_keystrokes("backspace backspace backspace");
  h.cx.simulate_input("com ");
  assert!(deletion(&mut h).expect("dialog open").confirmable());
  h.click("account-delete-confirm");
  h.cx.run_until_parked();
  assert_eq!(deletion(&mut h), None);
  assert!(!h.drawn("account-delete-dialog"));
  let writes = host.writes();
  let from = writes
    .iter()
    .position(|w| w == "delete account")
    .expect("deleted");
  assert_eq!(
    writes[from..],
    ["delete account", "notify toast Account deleted"]
  );
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn a_refused_deletion_shows_the_server_sentence(cx: &mut TestAppContext) {
  let sentence = "You are the only owner of Acme, which has other members. Make another member an owner or remove the other members, then delete your account.";
  let (mut h, host) = signed_in(
    cx,
    FakeHost {
      deletion_error: Some(crate::core::supabase::ApiError {
        status: 409,
        code: "sole_owner".into(),
        message: sentence.into(),
        correlation_id: None,
      }),
      ..FakeHost::default()
    },
  );
  h.click("account-delete");
  type_email(&mut h, "ada@example.com");
  h.click("account-delete-confirm");
  h.cx.run_until_parked();
  let refused = deletion(&mut h).expect("the dialog stays open");
  assert_eq!(refused.error.as_deref(), Some(sentence));
  assert!(!refused.deleting);
  assert!(h.drawn("account-delete-error"));
  assert!(!host.writes().iter().any(|w| w.starts_with("notify")));
  assert!(!host.writes().contains(&"sign out".to_string()));
}

#[gpui::test]
fn cancelling_keeps_the_account(cx: &mut TestAppContext) {
  let (mut h, host) = signed_in(cx, FakeHost::default());
  h.click("account-delete");
  type_email(&mut h, "ada@example.com");
  h.click("account-delete-cancel");
  assert!(!h.drawn("account-delete-dialog"));
  assert_eq!(deletion(&mut h), None);
  h.click("account-delete");
  assert_eq!(deletion(&mut h).expect("reopened").typed, "");
  assert!(!host.writes().contains(&"delete account".to_string()));
}
