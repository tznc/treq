//! The remote setup dialog in small windows and with oversized input: its
//! actions stay on screen, long errors are clamped, and a hostname longer
//! than DNS allows is refused before probing.

use gpui::TestAppContext;

use super::{dialogs::ssh_key, *};

fn own_vm(cx: &mut TestAppContext, host_key: Result<String, String>) -> (Harness, Arc<FakeHost>) {
  let host = FakeHost {
    ssh_keys: vec![ssh_key("id_ed25519")],
    host_key: Some(host_key),
    ..FakeHost::default()
  };
  let (mut h, host) = remote_harness(cx, host, setup_route(true, serde_json::json!({})));
  h.click("remote-setup-own-vm");
  (h, host)
}

fn on_screen(h: &mut Harness, selector: &'static str, (w, hh): (f32, f32)) {
  h.cx.run_until_parked();
  let b = h
    .cx
    .debug_bounds(selector)
    .unwrap_or_else(|| panic!("{selector} is not drawn"));
  assert!(
    b.left() >= gpui::px(0.)
      && b.top() >= gpui::px(0.)
      && b.right() <= gpui::px(w)
      && b.bottom() <= gpui::px(hh),
    "{selector} {b:?} off a {w}x{hh} window"
  );
}

#[gpui::test]
fn own_vm_actions_stay_on_screen_under_a_huge_error(cx: &mut TestAppContext) {
  let (mut h, _) = own_vm(cx, Err("x".repeat(10_000)));
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  assert!(h.drawn("remote-probe-error"));
  for size in [(1280., 800.), (320., 240.)] {
    h.cx
      .simulate_resize(gpui::size(gpui::px(size.0), gpui::px(size.1)));
    on_screen(&mut h, "remote-continue", size);
    on_screen(&mut h, "remote-setup-back", size);
  }
}

#[gpui::test]
fn a_long_error_is_clamped_until_shown_in_full(cx: &mut TestAppContext) {
  let (mut h, _) = own_vm(cx, Err("connection refused ".repeat(500)));
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  h.cx.run_until_parked();
  let clamped = h
    .cx
    .debug_bounds("remote-probe-error")
    .expect("error drawn");
  assert!(clamped.size.height < gpui::px(140.), "{clamped:?}");
  h.click("remote-probe-error-more");
  let full = h
    .cx
    .debug_bounds("remote-probe-error")
    .expect("error drawn");
  assert!(full.size.height > clamped.size.height, "{full:?}");
}

#[gpui::test]
fn a_hostname_longer_than_253_chars_disables_continue(cx: &mut TestAppContext) {
  let (mut h, host) = own_vm(cx, Ok("SHA256:abc123".into()));
  type_into(&mut h, "remote-hostname", &"a".repeat(254));
  type_into(&mut h, "remote-username", "me");
  assert!(h.drawn("remote-hostname-too-long"));
  h.click("remote-continue");
  h.cx.run_until_parked();
  assert!(host.writes().is_empty(), "{:?}", host.writes());
}

#[gpui::test]
fn editing_a_field_clears_a_stale_error(cx: &mut TestAppContext) {
  let (mut h, _) = own_vm(cx, Err("connection refused".into()));
  type_into(&mut h, "remote-hostname", "10.0.0.4");
  type_into(&mut h, "remote-username", "me");
  h.click("remote-continue");
  assert!(h.drawn("remote-probe-error"));
  type_into(&mut h, "remote-username", "x");
  assert!(!h.drawn("remote-probe-error"));
}
