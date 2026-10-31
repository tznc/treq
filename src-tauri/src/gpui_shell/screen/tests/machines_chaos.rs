//! Chaos tests for the cloud workspace provisioning UX (the setup dialog's
//! "Treq-managed cloud workspace" card). The host's provisioning runs off
//! the main thread and reports back through Rust's account, so the fake
//! "in flight", "failed" and "never resolves" states are account states.

use gpui::{
  proptest::{collection::vec, prelude::*},
  Role, TestAppContext,
};
use gpui_testing_library::prelude::*;

use super::*;
use crate::gpui_shell::fuzz;

fn view(h: &mut Harness) -> gpui_testing_library::Screen {
  h.cx.run_until_parked();
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  screen
}

fn provisions(host: &FakeHost) -> usize {
  host.writes().iter().filter(|w| *w == "cloud Provision").count()
}

/// Whether the create button reads as running (the spinner has no selector).
fn creating(h: &mut Harness) -> bool {
  view(h)
    .query_by_role(by::role(Role::Button).name("Creating..."))
    .is_some()
}

fn cloud(cloud: serde_json::Value) -> serde_json::Value {
  account_state(serde_json::json!({ "user": ada(), "cloud": cloud }))
}

fn managed(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  remote_harness(cx, FakeHost::default(), setup_route(false, serde_json::json!({})))
}

#[gpui::test]
#[ignore = "chaos: double click provisions twice"]
fn a_double_click_on_create_provisions_once(cx: &mut TestAppContext) {
  let (mut h, host) = managed(cx);
  // Two clicks before the backend's first state update reaches the window,
  // as a double click does: provisioning is spawned, so `submitting` /
  // `provisioningStage` arrive a frame or more later.
  h.click("cloud-create");
  h.click("cloud-create");
  h.cx.run_until_parked();
  assert_eq!(provisions(&host), 1, "writes: {:?}", host.writes());
}

#[gpui::test]
fn create_is_inert_while_provisioning_never_resolves(cx: &mut TestAppContext) {
  let (mut h, host) = managed(cx);
  h.set_account(cloud(serde_json::json!({ "provisioningStage": "Requesting provisioning..." })));
  for _ in 0..5 {
    h.click("cloud-create");
  }
  h.cx.simulate_keystrokes("enter space tab enter");
  assert_eq!(provisions(&host), 0, "{:?}", host.writes());
  assert!(creating(&mut h));
}

#[gpui::test]
fn no_spinner_is_left_after_a_failure(cx: &mut TestAppContext) {
  let (mut h, host) = managed(cx);
  h.set_account(cloud(serde_json::json!({ "provisioningStage": "Waiting for the cloud workspace (provisioning)..." })));
  assert!(creating(&mut h));
  h.set_account(cloud(serde_json::json!({
    "provisioningError": "Timed out waiting for the cloud workspace.",
    "instance": { "status": "failed", "providerResourceId": null, "diskQuotaGb": 10 },
  })));
  assert!(!creating(&mut h));
  let s = view(&mut h);
  expect(s.get_by_role(by::role(Role::Button).name("Retry cloud workspace creation"))).to_be_visible();
  h.click("cloud-create");
  assert_eq!(provisions(&host), 1);
}

/// What `ApiError`'s Display and the other host failures put in
/// `provisioningError`, by the failure that causes them.
const HOST_ERRORS: &[(&str, &str)] = &[
  ("network", "[network_error] error sending request for url (https://x.supabase.co/functions/v1/control-plane)"),
  ("timeout", "[network_error] operation timed out\nHTTP 0"),
  ("401", "[unauthorized] JWT expired\nHTTP 401 · Correlation ID: 7f3a"),
  ("403", "[forbidden] new row violates row-level security policy\nHTTP 403"),
  ("409 quota", "[quota_exceeded] Instance limit reached\nHTTP 409"),
  ("500", "[internal_error] {\"message\":\"upstream sprite error\"}\nHTTP 500"),
  ("malformed", "[invalid_response] expected value at line 1 column 1\nHTTP 200"),
];

#[gpui::test]
#[ignore = "chaos: raw [code] errors, no alert role"]
fn host_errors_read_as_sentences_not_codes(cx: &mut TestAppContext) {
  let (mut h, _) = managed(cx);
  let mut raw = Vec::new();
  for (case, error) in HOST_ERRORS {
    h.set_account(cloud(serde_json::json!({ "provisioningError": error })));
    let shown = error.split('\n').next().unwrap();
    let s = view(&mut h);
    // The error must not leave the button spinning or disabled.
    assert!(!creating(&mut h), "{case}");
    // What the card draws: `cloud_card`'s error line, verbatim.
    let route: settings::CloudRoute =
      serde_json::from_value(serde_json::json!({ "provisioningError": error })).unwrap();
    let drawn = settings::account::cloud_card(&route, false).error.unwrap_or_default();
    if drawn == shown && shown.starts_with('[') {
      raw.push(format!("{case}: {shown:?}"));
    }
    // It is announced, as the account deletion refusal is.
    if s.query_by_role(by::role(Role::Alert)).is_none() {
      raw.push(format!("{case}: not an alert"));
    }
  }
  assert!(raw.is_empty(), "raw / unannounced errors:\n{}", raw.join("\n"));
}

#[gpui::test]
fn extreme_error_text_stays_inside_the_dialog(cx: &mut TestAppContext) {
  let (mut h, _) = managed(cx);
  for error in [
    String::new(),
    "   ".into(),
    "\n\nsecond line only".into(),
    "خطأ في الشبكة 🚀🔥 \u{202E}reversed\u{202C} 网络错误".into(),
    "x".repeat(10_000),
    "word ".repeat(2_000),
  ] {
    h.set_account(cloud(serde_json::json!({ "provisioningError": error })));
    let dialog = h.cx.debug_bounds("remote-setup-dialog").expect("dialog");
    let card = h.cx.debug_bounds("cloud-workspace-card").expect("card");
    assert!(
      card.right() <= dialog.right() + gpui::px(1.) && card.bottom() <= dialog.bottom() + gpui::px(1.),
      "card {card:?} outside dialog {dialog:?} for {:?}",
      error.chars().take(30).collect::<String>()
    );
    assert!(h.drawn("cloud-create"));
  }
}

fn failed() -> serde_json::Value {
  cloud(serde_json::json!({
    "instance": { "status": "failed", "providerResourceId": "sprite-1", "diskQuotaGb": 10 },
    "endpoint": { "username": "treq", "hostname": "vm.example", "port": 22 },
  }))
}

#[gpui::test]
fn escape_closes_the_delete_confirmation_before_the_dialog(cx: &mut TestAppContext) {
  let (mut h, host) = managed(cx);
  h.set_account(failed());
  h.click("cloud-delete");
  assert!(h.drawn("cloud-delete-dialog"));
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert!(!h.drawn("cloud-delete-dialog"), "Escape left the confirmation open");
  assert!(h.sent().is_empty(), "Escape closed the setup dialog under the confirmation: {:?}", h.sent());
  assert!(!host.writes().contains(&"cloud Delete".to_string()));
}

#[gpui::test]
fn escape_closes_the_setup_dialog(cx: &mut TestAppContext) {
  let (mut h, _) = managed(cx);
  h.cx.simulate_keystrokes("escape");
  assert_eq!(h.sent(), vec![press("remote.setup.close")]);
}

#[gpui::test]
fn reopening_mid_provision_still_shows_it_running(cx: &mut TestAppContext) {
  let (mut h, host) = managed(cx);
  h.click("cloud-create");
  h.set_account(cloud(serde_json::json!({ "provisioningStage": "Requesting provisioning..." })));
  let setup = h.screen.read_with(&h.cx, |s, _| s.snapshot().remote.setup.clone());
  h.update(|s| s.remote.setup = None);
  assert!(!h.drawn("remote-setup-dialog"));
  h.update(|s| s.remote.setup = setup);
  assert!(creating(&mut h));
  h.click("cloud-create");
  assert_eq!(provisions(&host), 1, "{:?}", host.writes());
}

#[gpui::test]
#[ignore = "chaos: create button off-screen in small window"]
fn create_is_reachable_in_tiny_and_huge_windows(cx: &mut TestAppContext) {
  let (mut h, host) = managed(cx);
  for (w, hh) in [(320., 240.), (200., 150.), (4000., 3000.)] {
    h.cx.simulate_resize(gpui::size(gpui::px(w), gpui::px(hh)));
    h.cx.run_until_parked();
    let b = h.cx.debug_bounds("cloud-create").expect("create drawn");
    assert!(
      b.left() >= gpui::px(0.) && b.top() >= gpui::px(0.) && b.right() <= gpui::px(w) && b.bottom() <= gpui::px(hh),
      "create {b:?} off a {w}x{hh} window"
    );
  }
  h.click("cloud-create");
  assert_eq!(provisions(&host), 1);
}

// -- property: random steps --

#[derive(Debug, Clone)]
enum Step {
  /// The nth visible button, by role.
  Click(usize),
  Key(&'static str),
  /// The backend reports the nth account state.
  State(usize),
  Resize(u16, u16),
  /// The window model closes the dialog and opens it again.
  Reopen,
}

const KEYS: &[&str] = &["escape", "enter", "tab", "shift-tab", "space"];

fn states() -> Vec<(serde_json::Value, bool)> {
  // (account, provisioning in flight)
  vec![
    (cloud(serde_json::json!({})), false),
    (cloud(serde_json::json!({ "provisioningStage": "Requesting provisioning..." })), true),
    (cloud(serde_json::json!({ "submitting": true })), true),
    (cloud(serde_json::json!({ "provisioningError": "[network_error] boom\nHTTP 0" })), false),
    (failed(), false),
    (cloud(serde_json::json!({ "provisioningError": "[pro_required] Upgrade to create one." })), false),
    (cloud(serde_json::json!({
      "instance": { "status": "ready", "providerResourceId": "sprite-1", "diskQuotaGb": 10 },
      "endpoint": { "username": "treq", "hostname": "vm.example", "port": 22 },
    })), false),
    (account_state(serde_json::json!({})), false),
  ]
}

fn step() -> impl Strategy<Value = Step> {
  prop_oneof![
    5 => (0usize..12).prop_map(Step::Click),
    3 => gpui::proptest::sample::select(KEYS).prop_map(Step::Key),
    3 => (0usize..8).prop_map(Step::State),
    1 => (150u16..2400, 120u16..1600).prop_map(|(w, h)| Step::Resize(w, h)),
    1 => Just(Step::Reopen),
  ]
}

#[test]
fn provisioning_survives_chaos() {
  fuzz::check("provisioning_survives_chaos", 8, vec(step(), 1..30), |cx, steps| {
    let (mut h, host) = managed(cx);
    let states = states();
    let setup = h.screen.read_with(&h.cx, |s, _| s.snapshot().remote.setup.clone());
    let mut in_flight = false;
    let mut log = Vec::new();
    for step in &steps {
      log.push(format!("{step:?}"));
      let before = provisions(&host);
      match step {
        Step::Click(n) => {
          let s = view(&mut h);
          let buttons = s.query_all_by_role(by::role(Role::Button));
          if !buttons.is_empty() {
            s.user().click(&buttons[n % buttons.len()]);
          }
        }
        Step::Key(k) => h.cx.simulate_keystrokes(k),
        Step::State(n) => {
          in_flight = states[*n].1;
          h.set_account(states[*n].0.clone());
        }
        Step::Resize(w, hh) => h
          .cx
          .simulate_resize(gpui::size(gpui::px(*w as f32), gpui::px(*hh as f32))),
        Step::Reopen => {
          h.update(|s| s.remote.setup = None);
          h.update(|s| s.remote.setup = setup.clone());
        }
      }
      h.cx.run_until_parked();
      let added = provisions(&host) - before;
      assert!(added <= 1, "one step provisioned {added} times after {log:#?}");
      assert!(!(in_flight && added > 0), "provisioned while in flight after {log:#?}");
      let s = view(&mut h);
      let dialogs = s.query_all_by_role(by::role(Role::Dialog)).len();
      // The setup dialog, and at most its delete confirmation.
      assert!(dialogs <= 2, "{dialogs} dialogs after {log:#?}");
    }
  });
}
