use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn account_signs_in_through_the_browser(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let route = account_route("account");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(
    serde_json::json!({ "signInError": "token expired" }),
  ));
  let account = drawn_settings(&mut h).account.expect("account tab");
  let settings::AccountTab::SignedOut { error, .. } = account else {
    panic!("signed out: {account:?}");
  };
  assert_eq!(error.as_deref(), Some("Sign-in failed: token expired"));
  h.click("account-sign-in");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["open http://localhost:3001/sign-in?source=desktop"]
  );
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn account_shows_loading_and_signing_in(cx: &mut TestAppContext) {
  let route = account_route("account");
  let mut h = Harness::new(cx, route);
  h.set_account(account_state(serde_json::json!({ "loading": true })));
  assert_eq!(
    drawn_settings(&mut h).account,
    Some(settings::AccountTab::Loading)
  );
  h.set_account(account_state(serde_json::json!({ "signingIn": true })));
  assert_eq!(
    drawn_settings(&mut h).account,
    Some(settings::AccountTab::SigningIn)
  );
  assert!(h.drawn("account-signing-in"));
}

#[gpui::test]
fn account_trades_a_pasted_callback_token(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let route = account_route("account");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(serde_json::json!({})));
  settings_act(
    &mut h,
    field("account.callbackUrl", "treq://auth/callback?token=".into()),
  );
  h.click("account-dev-callback-go");
  let account = drawn_settings(&mut h).account.expect("account tab");
  let settings::AccountTab::SignedOut { dev, .. } = account else {
    panic!("signed out");
  };
  assert_eq!(
    dev.expect("dev builds offer it").error.as_deref(),
    Some("No token found in URL")
  );
  assert!(h.sent().is_empty());

  settings_act(
    &mut h,
    field(
      "account.callbackUrl",
      " treq://auth/callback?token=abc&x=1 ".into(),
    ),
  );
  h.click("account-dev-callback-go");
  h.cx.run_until_parked();
  settings_act(&mut h, field("account.callbackUrl", "expired".into()));
  h.click("account-dev-callback-go");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["exchange abc", "exchange expired"]);
  assert!(h.sent().is_empty());
  let Some(settings::AccountTab::SignedOut { dev, .. }) = drawn_settings(&mut h).account else {
    panic!("signed out");
  };
  let dev = dev.expect("dev builds offer it");
  assert!(!dev.loading);
  assert_eq!(dev.error.as_deref(), Some("Token exchange failed"));
}

#[gpui::test]
fn account_shows_the_plan_and_signs_out(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut user = ada();
  user["avatarUrl"] = "https://example.com/ada.png".into();
  let route = account_route("account");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(
    serde_json::json!({ "user": user, "subscription": pro() }),
  ));
  let account = drawn_settings(&mut h).account.expect("account tab");
  let settings::AccountTab::SignedIn(signed_in) = account else {
    panic!("signed in: {account:?}");
  };
  assert_eq!(signed_in.name, "Ada");
  assert_eq!(signed_in.email.as_deref(), Some("ada@example.com"));
  assert!(signed_in.pro);
  assert_eq!(
    signed_in.status,
    Some(("Active".into(), settings::StatusTone::Good))
  );
  assert_eq!(
    signed_in.period,
    Some(("Renews on".into(), "11/1/2026".into()))
  );
  assert_eq!(signed_in.manage_label, "Manage Subscription");

  h.click("account-manage-subscription");
  h.click("account-sign-out");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "fetch https://example.com/ada.png",
      "open http://localhost:3001/dashboard",
      "sign out"
    ]
  );
  assert!(host.calls().contains(&"read cloud workspace".to_string()));
  assert!(h.sent().is_empty());
}

fn delete_dialog(h: &mut Harness) -> Option<settings::DeleteAccountDialog> {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => signed_in.delete_account,
    other => panic!("signed in: {other:?}"),
  }
}

#[gpui::test]
fn account_deletes_only_after_the_email_is_typed(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_as_ada(cx, host.clone());
  assert_eq!(delete_dialog(&mut h), None);

  h.click("account-delete");
  assert!(h.drawn("account-delete-dialog"));
  let dialog = delete_dialog(&mut h).expect("asked");
  assert!(!dialog.can_confirm);

  h.click("account-delete-confirm");
  settings_act(
    &mut h,
    field("account.delete.email", "eve@example.com".into()),
  );
  assert!(!delete_dialog(&mut h).unwrap().can_confirm);
  h.click("account-delete-confirm");
  h.cx.run_until_parked();
  assert!(!host.writes().contains(&"delete account".to_string()));

  settings_act(
    &mut h,
    field("account.delete.email", " ADA@example.com ".into()),
  );
  assert!(delete_dialog(&mut h).unwrap().can_confirm);
  h.click("account-delete-confirm");
  h.cx.run_until_parked();
  assert_eq!(
    host
      .writes()
      .iter()
      .filter(|w| *w == "delete account")
      .count(),
    1
  );
  assert_eq!(delete_dialog(&mut h), None);
  assert!(h.sent().is_empty());
}

#[gpui::test]
fn account_deletion_the_server_refuses_shows_why_and_keeps_the_dialog(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    account_deletion: Some(crate::core::supabase::ApiError {
      status: 409,
      code: "last_owner".into(),
      message: "You are the last owner of Acme. Make another member an owner, or delete the organization, before deleting your account.".into(),
      correlation_id: None,
    }),
    ..FakeHost::default()
  });
  let mut h = signed_in_as_ada(cx, host.clone());
  h.click("account-delete");
  settings_act(
    &mut h,
    field("account.delete.email", "ada@example.com".into()),
  );
  h.click("account-delete-confirm");
  h.cx.run_until_parked();

  let dialog = delete_dialog(&mut h).expect("still open");
  assert!(!dialog.deleting);
  assert!(dialog.can_confirm);
  assert_eq!(
    dialog.error.as_deref(),
    Some("You are the last owner of Acme. Make another member an owner, or delete the organization, before deleting your account.")
  );
  assert!(host.writes().contains(&"delete account".to_string()));
}

fn farewell(h: &mut Harness) -> bool {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedOut { farewell, .. }) => farewell,
    other => panic!("signed out: {other:?}"),
  }
}

#[gpui::test]
fn account_deletion_toasts_and_says_goodbye_once_signed_out(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_as_ada(cx, host.clone());
  h.click("account-delete");
  settings_act(
    &mut h,
    field("account.delete.email", "ada@example.com".into()),
  );
  h.click("account-delete-confirm");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"notify toast Account deleted".to_string()));

  // The core signs out after deleting; the tab follows the account.
  h.set_account(account_state(serde_json::json!({})));
  assert!(farewell(&mut h));
  assert!(h.drawn("account-farewell"));

  h.click("account-farewell-dismiss");
  assert!(!farewell(&mut h));
  assert!(!h.drawn("account-farewell"));
}

#[gpui::test]
fn a_refused_account_deletion_neither_toasts_nor_says_goodbye(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    account_deletion: Some(crate::core::supabase::ApiError::new("last_owner", "No")),
    ..FakeHost::default()
  });
  let mut h = signed_in_as_ada(cx, host.clone());
  h.click("account-delete");
  settings_act(
    &mut h,
    field("account.delete.email", "ada@example.com".into()),
  );
  h.click("account-delete-confirm");
  h.cx.run_until_parked();
  assert!(!host.writes().iter().any(|w| w.starts_with("notify toast")));
  h.set_account(account_state(serde_json::json!({})));
  assert!(!farewell(&mut h));
}

#[gpui::test]
fn account_deletion_cancel_closes_without_deleting(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_as_ada(cx, host.clone());
  h.click("account-delete");
  settings_act(
    &mut h,
    field("account.delete.email", "ada@example.com".into()),
  );
  h.click("account-delete-cancel");
  h.cx.run_until_parked();
  assert_eq!(delete_dialog(&mut h), None);
  assert!(!h.drawn("account-delete-dialog"));
  assert!(!host.writes().contains(&"delete account".to_string()));

  // Asking again starts from an empty field.
  h.click("account-delete");
  assert!(!delete_dialog(&mut h).unwrap().can_confirm);
}

#[gpui::test]
fn account_offers_pro_on_the_free_plan(cx: &mut TestAppContext) {
  let route = account_route("account");
  let mut h = Harness::new(cx, route);
  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "subscription": { "plan": "free", "status": "canceled", "currentPeriodEnd": null },
  })));
  let Some(settings::AccountTab::SignedIn(signed_in)) = drawn_settings(&mut h).account else {
    panic!("signed in");
  };
  assert!(!signed_in.pro);
  assert_eq!(
    signed_in.status,
    Some(("Canceling".into(), settings::StatusTone::Warn))
  );
  assert_eq!(signed_in.period, None);
  assert_eq!(signed_in.manage_label, "Upgrade to Pro");
}

fn cloud(patch: serde_json::Value) -> serde_json::Value {
  let mut cloud = serde_json::json!({
    "instance": null,
    "endpoint": null,
    "provisioningStage": null,
    "provisioningError": null,
    "usage": null,
    "usageError": null,
    "submitting": false,
  });
  if let (Some(cloud), Some(patch)) = (cloud.as_object_mut(), patch.as_object()) {
    for (key, value) in patch {
      cloud.insert(key.clone(), value.clone());
    }
  }
  cloud
}

fn signed_in_with_cloud(
  cx: &mut TestAppContext,
  host: Arc<FakeHost>,
  cloud: serde_json::Value,
) -> Harness {
  let mut h = Harness::with_host(cx, account_route("account"), host);
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": pro(), "cloud": cloud }),
  ));
  h
}

fn cloud_card(h: &mut Harness) -> settings::CloudCard {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => signed_in.cloud.expect("cloud card"),
    other => panic!("signed in: {other:?}"),
  }
}

#[gpui::test]
fn cloud_workspace_creates_through_rust(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host.clone(), cloud(serde_json::json!({})));
  let card = cloud_card(&mut h);
  assert_eq!(card.instance, None);
  assert_eq!(
    card.create.as_ref().map(|c| c.label.as_str()),
    Some("Create cloud workspace")
  );
  h.click("cloud-create");
  assert_eq!(host.writes(), ["cloud Provision"]);
  assert!(h.sent().is_empty());

  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "cloud": cloud(serde_json::json!({ "provisioningStage": "Requesting provisioning..." })),
  })));
  let create = cloud_card(&mut h).create.expect("create");
  assert_eq!(create.label, "Creating...");
  assert!(create.disabled && create.spinning);
  assert_eq!(create.stage.as_deref(), Some("Requesting provisioning..."));
}

#[gpui::test]
fn cloud_workspace_shows_usage_and_deletes_after_confirming(cx: &mut TestAppContext) {
  let ready = cloud(serde_json::json!({
    "instance": { "status": "ready", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "endpoint": { "username": "treq", "hostname": "box.treq.dev", "port": 2222 },
    "usage": { "repositoryCount": 3, "workspaceCount": 7, "diskUsedBytes": 1932735283u64 },
  }));
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host.clone(), ready);
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
  let instance = cloud_card(&mut h).instance.expect("instance");
  assert_eq!(instance.label, "Ready");
  assert_eq!(instance.endpoint.as_deref(), Some("treq@box.treq.dev:2222"));
  assert!(!instance.repairable);
  assert_eq!(
    instance.usage,
    Some(settings::CloudUsage::Report {
      summary: "7 workspaces across 3 repositories".into(),
      size: "1.8 GB of 5 GB".into(),
      percent: 36,
    })
  );
  assert!(!h.drawn("cloud-repair"));
  h.click("cloud-open-repositories");
  h.click("cloud-delete");
  assert!(h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete-dialog-cancel");
  assert!(!h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete");
  h.click("cloud-delete-dialog-confirm");
  assert!(!h.drawn("cloud-delete-dialog"));
  assert_eq!(host.writes(), ["cloud OpenRepositories", "cloud Delete"]);
  assert_eq!(h.sent(), [press("settings.close")]);
}

#[gpui::test]
fn cloud_workspace_warns_when_a_lapse_will_delete_it(cx: &mut TestAppContext) {
  let lapsed = cloud(serde_json::json!({
    "instance": {
      "status": "suspended",
      "providerResourceId": "m-1",
      "diskQuotaGb": 5,
      "lapsedAt": "2026-11-01T12:00:00Z",
    },
  }));
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host, lapsed);
  let instance = cloud_card(&mut h).instance.expect("instance");
  assert_eq!(
    instance.deletion.as_deref(),
    Some("Pro ended, so this workspace will be deleted on 12/1/2026. Resubscribe to keep it.")
  );
  assert!(h.drawn("cloud-deletion-warning"));
}

#[gpui::test]
fn cloud_workspace_without_a_lapse_shows_no_deletion(cx: &mut TestAppContext) {
  let kept = cloud(serde_json::json!({
    "instance": { "status": "ready", "providerResourceId": "m-1", "diskQuotaGb": 5 },
  }));
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host, kept);
  assert_eq!(
    cloud_card(&mut h).instance.expect("instance").deletion,
    None
  );
  assert!(!h.drawn("cloud-deletion-warning"));
}

#[gpui::test]
fn cloud_workspace_repairs_and_upsells_pro(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let degraded = cloud(serde_json::json!({
    "instance": { "status": "degraded", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "provisioningError": "[pro_required] Cloud workspaces need Pro.\nhttp 402",
  }));
  let mut h = signed_in_with_cloud(cx, host.clone(), degraded);
  let card = cloud_card(&mut h);
  assert!(card.instance.expect("instance").repairable);
  assert_eq!(
    card.pro_required.as_deref(),
    Some("Cloud workspaces need Pro.")
  );
  assert_eq!(card.error, None);
  h.click("cloud-repair");
  h.click("cloud-upgrade");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    host.writes(),
    ["cloud Repair", "open http://localhost:3001/dashboard"]
  );

  h.set_account(account_state(serde_json::json!({
    "user": ada(),
    "cloud": cloud(serde_json::json!({
      "instance": { "status": "failed", "providerResourceId": null, "diskQuotaGb": 5 },
      "provisioningError": "[http_502] exec failed\nstack",
    })),
  })));
  let card = cloud_card(&mut h);
  assert_eq!(card.error.as_deref(), Some("[http_502] exec failed"));
  assert_eq!(
    card.create.map(|c| c.label),
    Some("Retry cloud workspace creation".into())
  );
}

#[gpui::test]
fn integrations_sign_in_when_signed_out(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.linearIntegration",
    "false",
  )]));
  let route = account_route("integrations");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(serde_json::json!({})));
  let integrations = drawn_settings(&mut h).integrations.expect("integrations");
  assert_eq!(integrations.github, settings::GithubSection::SignedOut);
  assert_eq!(integrations.linear, None);
  h.click("integrations-sign-in");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["open http://localhost:3001/sign-in?source=desktop"]
  );
}

#[gpui::test]
fn integrations_list_public_repositories_on_the_free_plan(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::with_settings(&[(
    "feature_preview.linearIntegration",
    "false",
  )]));
  let repo = |id, name: &str, private| crate::core::account::GithubRepository {
    id,
    full_name: name.into(),
    private,
  };
  let host = Arc::new(FakeHost {
    github_repos: Some(Ok(vec![
      repo(1, "acme/public", false),
      repo(2, "acme/private", true),
    ])),
    ..Arc::into_inner(host).expect("one owner")
  });
  let route = account_route("integrations");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(serde_json::json!({ "user": ada() })));
  let integrations = drawn_settings(&mut h).integrations.expect("integrations");
  assert_eq!(
    integrations.github,
    settings::GithubSection::SignedIn {
      scope: "Public repositories".into(),
      description: "1 enabled.".into(),
      repositories: vec![("acme/public".into(), false)],
    }
  );
  h.click("github-manage");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["open http://localhost:3001/dashboard?tab=integrations"]
  );
}

fn linear_harness(
  cx: &mut TestAppContext,
  pro: bool,
  key: Option<&str>,
) -> (Harness, Arc<FakeHost>) {
  let mut settings = vec![("feature_preview.linearIntegration", "true")];
  settings.extend(key.map(|key| ("/repo:linear_api_key", key)));
  let host = Arc::new(FakeHost::with_settings(&settings));
  let subscription = if pro {
    self::pro()
  } else {
    serde_json::Value::Null
  };
  let route = account_route("integrations");
  let mut h = Harness::with_host(cx, route, host.clone());
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": subscription }),
  ));
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
  (h, host)
}

fn linear_section(h: &mut Harness) -> settings::LinearSection {
  drawn_settings(h)
    .integrations
    .expect("integrations")
    .linear
    .expect("linear")
}

#[gpui::test]
fn linear_saves_an_api_key_to_the_repository(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, false, None);
  let linear = linear_section(&mut h);
  assert_eq!(linear.description, "Free plan: provide your API key");
  assert_eq!(linear.edit_label, "Add API Key");
  assert!(!linear.oauth);
  h.click("linear-api-key-edit");
  assert!(linear_section(&mut h).editing);
  settings_act(&mut h, field("linear.apiKey", "lin_api_1".into()));
  h.click("linear-api-key-save");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    [
      "set repo /repo linear_api_key=lin_api_1",
      "notify repoSettingsChanged",
      "notify toast API Key saved",
    ]
  );
  let linear = linear_section(&mut h);
  assert!(!linear.editing);
  assert_eq!(linear.description, "Direct API access configured");
  assert_eq!(linear.edit_label, "Update API Key");
}

#[gpui::test]
fn linear_connects_via_oauth_on_pro(cx: &mut TestAppContext) {
  let (mut h, host) = linear_harness(cx, true, Some("lin_api_1"));
  let linear = linear_section(&mut h);
  assert!(linear.oauth);
  assert!(linear.key_priority_note);
  assert_eq!(
    linear.oauth_description,
    "API key takes priority, but you can also connect via OAuth"
  );
  h.click("linear-oauth");
  h.cx.run_until_parked();
  assert!(h.sent().is_empty());
  assert_eq!(
    host.writes(),
    ["linear oauth", "open https://linear.app/oauth/authorize"]
  );
  assert!(!linear_section(&mut h).oauth_loading);
}

#[gpui::test]
fn linear_oauth_refused_for_want_of_pro_offers_the_upgrade(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost {
    oauth: Some(Err(crate::core::supabase::ApiError {
      status: 402,
      code: "pro_required".into(),
      message: "Pro".into(),
      correlation_id: None,
    })),
    ..FakeHost::with_settings(&[("feature_preview.linearIntegration", "true")])
  });
  let mut h = Harness::with_host(cx, account_route("integrations"), host.clone());
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": pro() }),
  ));
  h.click("linear-oauth");
  h.cx.run_until_parked();
  assert_eq!(
    host.writes(),
    ["linear oauth", "notify toast Linear OAuth needs Pro"]
  );
}

#[gpui::test]
fn the_cloud_delete_confirmation_cancels_on_escape_and_deletes_on_enter(cx: &mut TestAppContext) {
  let ready = cloud(serde_json::json!({
    "instance": { "status": "ready", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "endpoint": { "username": "treq", "hostname": "box.treq.dev", "port": 2222 },
  }));
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host.clone(), ready);
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
  h.click("cloud-delete");
  assert!(h.drawn("cloud-delete-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(!h.drawn("cloud-delete-dialog"));
  assert_eq!(host.writes(), ["cloud Delete"]);
}

#[gpui::test]
fn the_account_delete_dialog_cancels_on_escape(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_as_ada(cx, host);
  h.click("account-delete");
  assert!(h.drawn("account-delete-dialog"));
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert_eq!(delete_dialog(&mut h), None);
}

#[gpui::test]
fn the_account_delete_dialog_deletes_on_enter_once_the_email_is_typed(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_as_ada(cx, host.clone());
  h.click("account-delete");
  // Enter waits for the email.
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(!host.writes().contains(&"delete account".to_string()));
  settings_act(
    &mut h,
    field("account.delete.email", "ada@example.com".into()),
  );
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"delete account".to_string()));
}
