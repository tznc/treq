//! Editing an inline review comment in the Changes tab
//! (`review/comments-inline.test.tsx`): Save, Cancel, Escape and Discard.

use gpui::TestAppContext;

use super::*;

/// A Changes tab with comment "Nit" on `a.rs`, open in its edit box.
fn editing(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  settle_save(&mut h);
  h.click("inline-comment-card");
  assert!(h.drawn("comment-edit"));
  h.click("comment-edit");
  (h, host)
}

fn saved(host: &FakeHost) -> String {
  host.review.lock().unwrap().clone().unwrap_or_default()
}

#[gpui::test]
fn editing_a_comment_offers_save_cancel_and_discard(cx: &mut TestAppContext) {
  let (mut h, _) = editing(cx);
  for selector in ["comment-save", "comment-edit-cancel", "comment-discard"] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("inline-comment-card"), "the edit box replaces it");
}

#[gpui::test]
fn cancelling_an_edit_restores_the_comment(cx: &mut TestAppContext) {
  let (mut h, host) = editing(cx);
  h.cx.simulate_input(" changed");
  h.click("comment-edit-cancel");
  settle_save(&mut h);
  assert!(!h.drawn("comment-edit"));
  assert!(h.drawn("inline-comment-card"));
  let saved = saved(&host);
  assert!(
    saved.contains("\"Nit\"") && !saved.contains("changed"),
    "{saved}"
  );
}

#[gpui::test]
fn escape_cancels_an_edit(cx: &mut TestAppContext) {
  let (mut h, host) = editing(cx);
  h.cx.simulate_input(" changed");
  h.cx.simulate_keystrokes("escape");
  settle_save(&mut h);
  assert!(!h.drawn("comment-edit"));
  assert!(h.drawn("inline-comment-card"));
  assert!(!saved(&host).contains("changed"));
}

#[gpui::test]
fn an_emptied_comment_cannot_be_saved(cx: &mut TestAppContext) {
  let (mut h, host) = editing(cx);
  h.cx.simulate_keystrokes("backspace backspace backspace");
  h.click("comment-save");
  settle_save(&mut h);
  assert!(h.drawn("comment-edit"), "still editing");
  assert!(saved(&host).contains("\"Nit\""));
}

#[gpui::test]
fn discarding_from_the_edit_box_deletes_the_comment(cx: &mut TestAppContext) {
  let (mut h, host) = editing(cx);
  h.click("comment-discard");
  settle_save(&mut h);
  assert!(!h.drawn("comment-edit"));
  assert!(!h.drawn("inline-comment-card"));
  assert!(!saved(&host).contains("Nit"), "{}", saved(&host));
}
