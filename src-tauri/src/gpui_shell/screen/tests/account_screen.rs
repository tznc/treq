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
  h.click("cloud-delete-cancel");
  assert!(!h.drawn("cloud-delete-dialog"));
  h.click("cloud-delete");
  h.click("cloud-delete-confirm");
  assert!(!h.drawn("cloud-delete-dialog"));
  assert_eq!(host.writes(), ["cloud OpenRepositories", "cloud Delete"]);
  assert_eq!(h.sent(), [press("settings.close")]);
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
fn cloud_workspace_lists_background_processes_to_kill_or_keep_alive(cx: &mut TestAppContext) {
  let stops_at = (chrono::Utc::now() + chrono::Duration::seconds(12 * 60 + 30)).to_rfc3339();
  let ready = cloud(serde_json::json!({
    "instance": { "status": "ready", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "activity": {
      "stops_at": stops_at,
      "idle_timeout_min": 15,
      "processes": [
        {"pid": 500, "command": "node server.js", "origin": "orphan", "cpu": 12.5, "ports": [3000], "pinned": false},
        {"pid": 410, "command": "npm run dev", "origin": "agent", "cpu": 3, "ports": [], "pinned": true},
        {"pid": 100, "command": "/usr/sbin/sshd", "origin": "service", "cpu": 0, "ports": [2222], "pinned": false}
      ],
      "pins": [{"id": "pin-1", "pid": 410, "command": "npm run dev", "pattern": null, "expires_at": stops_at}]
    },
  }));
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(cx, host.clone(), ready);
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1600.)));
  let card = cloud_card(&mut h);
  assert_eq!(card.idle_stop.as_deref(), Some("Stops in 12 min if idle"));
  assert_eq!(
    card.processes,
    [
      settings::ProcessRow {
        pid: 500,
        command: "node server.js".into(),
        detail: "Background · 12.5% CPU · :3000".into(),
        pinned: false,
        pin_id: None,
        killable: true,
      },
      settings::ProcessRow {
        pid: 410,
        command: "npm run dev".into(),
        detail: "Agent · 3.0% CPU".into(),
        pinned: true,
        pin_id: Some("pin-1".into()),
        killable: true,
      },
      settings::ProcessRow {
        pid: 100,
        command: "/usr/sbin/sshd".into(),
        detail: "Service · 0.0% CPU · :2222".into(),
        pinned: false,
        pin_id: None,
        killable: false,
      },
    ]
  );
  assert!(h.drawn("cloud-processes"));
  assert!(!h.drawn("cloud-process-kill-100"));
  assert!(!h.drawn("cloud-process-pin-100"));
  h.click("cloud-process-kill-500");
  h.click("cloud-process-pin-500");
  h.click("cloud-process-unpin-410");
  assert_eq!(
    host.writes(),
    [
      "cloud KillProcess { pid: 500 }",
      "cloud PinProcess { pid: 500 }",
      "cloud UnpinProcess { pin_id: \"pin-1\" }",
    ]
  );
}

#[gpui::test]
fn cloud_workspace_without_a_probe_shows_no_processes(cx: &mut TestAppContext) {
  let ready = cloud(serde_json::json!({
    "instance": { "status": "suspended", "providerResourceId": "m-1", "diskQuotaGb": 5 },
    "activity": { "stops_at": null, "idle_timeout_min": 15, "processes": [], "pins": [] },
  }));
  let mut h = signed_in_with_cloud(cx, Arc::new(FakeHost::default()), ready);
  let card = cloud_card(&mut h);
  assert_eq!(card.idle_stop, None);
  assert!(card.processes.is_empty());
  assert!(!h.drawn("cloud-processes"));
}

fn tiers() -> serde_json::Value {
  serde_json::json!([
    {"id": "s", "label": "Small", "vcpu": 2, "ram_gib": 4, "disk_gb": 10, "regions": [
      {"region": "eu_west", "machine_type_id": "boat:small", "hourly_usd": 0.0216},
      {"region": "us_east", "machine_type_id": "daytona:s", "hourly_usd": 0.03}
    ]},
    {"id": "m", "label": "Medium", "vcpu": 4, "ram_gib": 8, "disk_gb": 20, "regions": [
      {"region": "us_east", "machine_type_id": "daytona:m", "hourly_usd": 0.06}
    ]}
  ])
}

#[gpui::test]
fn cloud_workspace_creates_with_the_picked_size(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(
    cx,
    host.clone(),
    cloud(serde_json::json!({ "tiers": tiers(), "selectedTier": "m" })),
  );
  let card = cloud_card(&mut h);
  assert_eq!(
    card.sizes,
    [
      settings::SizeOption {
        id: "s".into(),
        label: "Small".into(),
        spec: "2 vCPU · 4 GiB RAM · 10 GB disk".into(),
        price: Some("$0.0216/hour".into()),
        selected: false,
        current: false,
      },
      settings::SizeOption {
        id: "m".into(),
        label: "Medium".into(),
        spec: "4 vCPU · 8 GiB RAM · 20 GB disk".into(),
        price: Some("$0.0600/hour".into()),
        selected: true,
        current: false,
      },
    ]
  );
  assert!(!card.resize);
  h.click("cloud-tier-s");
  h.click("cloud-create");
  assert_eq!(
    host.writes(),
    ["cloud SelectTier { tier_id: \"s\" }", "cloud Provision"]
  );
}

#[gpui::test]
fn cloud_workspace_resizes_after_confirming_the_unpushed_work_it_loses(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let instance = serde_json::json!({
    "status": "ready", "providerResourceId": "m-1", "diskQuotaGb": 10,
    "tierId": "s", "region": "us_east"
  });
  let mut h = signed_in_with_cloud(
    cx,
    host.clone(),
    cloud(serde_json::json!({
      "instance": instance, "tiers": tiers(), "selectedTier": "m",
      "billing": { "billedUsd": 1.234, "hours": 41.5 },
    })),
  );
  let card = cloud_card(&mut h);
  assert_eq!(card.sizes[0].price.as_deref(), Some("$0.0300/hour"));
  assert!(card.sizes[0].current);
  assert!(card.resize);
  assert_eq!(
    card.billing.as_deref(),
    Some("This month: $1.23 for 41.5 hours")
  );
  h.click("cloud-resize");

  set_cloud(
    &mut h,
    serde_json::json!({
      "instance": instance, "tiers": tiers(), "selectedTier": "m",
      "resizePreflight": ["m", {"requires_ack": true, "repos": [
        {"path": "acme/api", "vcs": "jj", "dirty": 0, "unpushed": 2, "agents": 0, "error": null}
      ]}],
    }),
  );
  let card = cloud_card(&mut h);
  assert_eq!(
    card.resize_confirm,
    Some(vec!["acme/api: 2 unpushed commits".to_string()])
  );
  assert!(h.drawn("cloud-resize-confirm-alert"));
  h.click("cloud-resize-cancel");
  h.click("cloud-resize-confirm");
  assert_eq!(
    host.writes(),
    [
      "cloud Resize { acknowledged_data_loss: false }",
      "cloud CancelResize",
      "cloud Resize { acknowledged_data_loss: true }",
    ]
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

fn spend(cap: f64, spent: f64, state: &str) -> serde_json::Value {
  serde_json::json!({
    "cap_usd": cap, "spent_usd": spent,
    "month_start": "2026-10-01T00:00:00+00:00", "state": state
  })
}

/// Signs Ada in on Pro with the cloud workspace `patch` describes.
fn set_cloud(h: &mut Harness, patch: serde_json::Value) {
  h.set_account(account_state(
    serde_json::json!({ "user": ada(), "subscription": pro(), "cloud": cloud(patch) }),
  ));
}

fn signed_in_tab(h: &mut Harness) -> settings::SignedIn {
  match drawn_settings(h).account {
    Some(settings::AccountTab::SignedIn(signed_in)) => *signed_in,
    other => panic!("signed in: {other:?}"),
  }
}

#[gpui::test]
fn cloud_card_shows_spend_against_the_cap(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(
    cx,
    host,
    cloud(serde_json::json!({
      "billing": { "billedUsd": 12.0, "hours": 3.0 },
      "spend": spend(50., 12.345, "ok"),
    })),
  );
  let card = cloud_card(&mut h);
  assert_eq!(
    card.billing.as_deref(),
    Some("This month: $12.35 of $50.00")
  );
  assert_eq!(
    card.spend_state,
    Some(crate::core::cloud_catalog::SpendState::Ok)
  );
  assert_eq!(card.spend_note, None);
  assert!(h.drawn("cloud-billing"));
}

#[gpui::test]
fn cloud_card_warns_at_80_percent_and_explains_a_reached_cap(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(
    cx,
    host,
    cloud(serde_json::json!({ "spend": spend(50., 41., "warning") })),
  );
  let card = cloud_card(&mut h);
  assert_eq!(
    card.spend_state,
    Some(crate::core::cloud_catalog::SpendState::Warning)
  );
  assert_eq!(
    card.spend_note.as_deref(),
    Some("You've used 82% of your monthly cap. Workspaces stop when you reach it.")
  );

  set_cloud(
    &mut h,
    serde_json::json!({
      "instance": { "status": "suspended", "providerResourceId": "m-1", "diskQuotaGb": 5 },
      "spend": spend(50., 50.2, "capped"),
      "provisioningError": "[spend_cap_reached] You reached your monthly cloud spend cap of $50.00. Raise the cap to keep working.\nHTTP 402",
    }),
  );
  let card = cloud_card(&mut h);
  assert_eq!(
    card.spend_state,
    Some(crate::core::cloud_catalog::SpendState::Capped)
  );
  assert_eq!(
    card.spend_note.as_deref(),
    Some("You reached your monthly cap. Your cloud workspaces are stopped until you raise the cap or the month ends (UTC).")
  );
  assert_eq!(
    card.spend_cap_reached.as_deref(),
    Some("You reached your monthly cloud spend cap of $50.00. Raise the cap to keep working.")
  );
  assert_eq!(card.error, None);
  assert!(h.drawn("cloud-spend-capped"));
  // Raise cap opens the cap editor.
  h.click("cloud-spend-cap-raise");
  let editor = signed_in_tab(&mut h).spend_cap.expect("cap editor");
  assert!(editor.editing);
  assert_eq!(editor.draft, "50");
}

#[gpui::test]
fn the_spend_cap_is_changed_in_the_app(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(
    cx,
    host.clone(),
    cloud(serde_json::json!({ "spend": spend(50., 10., "ok") })),
  );
  let editor = signed_in_tab(&mut h).spend_cap.expect("cap editor");
  assert_eq!(editor.label, "Monthly spend cap: $50.00");
  assert!(!editor.editing);
  h.click("cloud-spend-cap-edit");
  settings_act(&mut h, field("cloud.spendCap", "4".into()));
  h.click("cloud-spend-cap-save");
  assert_eq!(
    signed_in_tab(&mut h).spend_cap.unwrap().error.as_deref(),
    Some("Enter an amount of at least $5.")
  );
  assert!(host.writes().is_empty());
  settings_act(&mut h, field("cloud.spendCap", "$80".into()));
  h.click("cloud-spend-cap-save");
  assert_eq!(host.writes(), ["cloud SetSpendCap { cap_usd: 80.0 }"]);
  assert!(!signed_in_tab(&mut h).spend_cap.unwrap().editing);

  // The server's refusal shows under the editor.
  set_cloud(
    &mut h,
    serde_json::json!({
      "spend": spend(50., 10., "ok"),
      "spendCapError": "That cap is above the maximum allowed.",
    }),
  );
  assert_eq!(
    signed_in_tab(&mut h).spend_cap.unwrap().error.as_deref(),
    Some("That cap is above the maximum allowed.")
  );
}

#[gpui::test]
fn stored_notifications_show_and_dismiss(cx: &mut TestAppContext) {
  let host = Arc::new(FakeHost::default());
  let mut h = signed_in_with_cloud(
    cx,
    host.clone(),
    cloud(serde_json::json!({ "notifications": [{
      "id": "n1", "kind": "spend_cap_warning", "period": "2026-10",
      "title": "Cloud spend at 80% of your cap",
      "body": "You spent $40.00 of your $50.00 monthly cap.",
      "created_at": "2026-10-28T12:00:00+00:00"
    }] })),
  );
  let card = cloud_card(&mut h);
  let notice = &card.notifications[0];
  assert_eq!(
    (
      notice.id.as_str(),
      notice.title.as_str(),
      notice.body.as_str()
    ),
    (
      "n1",
      "Cloud spend at 80% of your cap",
      "You spent $40.00 of your $50.00 monthly cap."
    )
  );
  h.click("cloud-notification-dismiss-n1");
  assert_eq!(host.writes(), ["cloud DismissNotification { id: \"n1\" }"]);
}

// -- Bring your own sandbox (BYOS) ------------------------------------------

fn byos_host(on: bool) -> Arc<FakeHost> {
  Arc::new(FakeHost::with_settings(&[(
    "feature_preview.byos",
    if on { "true" } else { "false" },
  )]))
}

fn provider_keys(h: &mut Harness) -> Option<settings::ProviderKeysSection> {
  signed_in_tab(h).provider_keys
}

fn boat_key() -> serde_json::Value {
  serde_json::json!([{ "id": "c1", "provider_kind": "boat", "validated_at": "2026-10-28T00:00:00Z" }])
}

#[gpui::test]
fn byos_is_hidden_with_its_preview_off(cx: &mut TestAppContext) {
  let mut h = signed_in_with_cloud(
    cx,
    byos_host(false),
    cloud(serde_json::json!({ "providerKeys": boat_key(), "selectedProvider": "boat" })),
  );
  assert_eq!(provider_keys(&mut h), None);
  assert!(!h.drawn("byos-provider-keys"));
  let card = cloud_card(&mut h);
  assert!(card.placements.is_empty());
  assert_eq!(card.byos_note, None);
  assert!(!h.drawn("cloud-placement-managed"));
}

#[gpui::test]
fn byos_adds_and_removes_provider_keys(cx: &mut TestAppContext) {
  let host = byos_host(true);
  let mut h = signed_in_with_cloud(
    cx,
    host.clone(),
    cloud(serde_json::json!({ "providerKeys": boat_key() })),
  );
  let keys = provider_keys(&mut h).expect("provider keys with the preview on");
  assert!(h.drawn("byos-provider-keys"));
  assert_eq!(
    keys.rows,
    [settings::ProviderKeyRow {
      kind: "boat".into(),
      name: "Boat".into(),
      status: "Key saved".into(),
      invalid: false,
    }]
  );
  assert_eq!(
    keys.addable,
    [
      ("daytona".to_string(), "Daytona".to_string()),
      ("fly_machines".to_string(), "Fly Machines".to_string())
    ]
  );
  h.click("byos-key-add-daytona");
  assert_eq!(
    provider_keys(&mut h).unwrap().editing.as_deref(),
    Some("daytona")
  );
  settings_act(&mut h, field("byos.key", "dt-secret".into()));
  h.click("byos-key-save");
  h.click("byos-key-remove-boat");
  assert_eq!(
    host.writes(),
    [
      "cloud SaveProviderKey { kind: \"daytona\", key: <redacted> }",
      "cloud RemoveProviderKey { kind: \"boat\" }",
    ]
  );
  assert_eq!(
    provider_keys(&mut h).unwrap().editing,
    None,
    "saving closes the field"
  );
}

#[gpui::test]
fn byos_shows_a_key_the_provider_rejected(cx: &mut TestAppContext) {
  let mut h = signed_in_with_cloud(
    cx,
    byos_host(true),
    cloud(serde_json::json!({
      "providerKeys": [{ "id": "c1", "provider_kind": "daytona", "invalid_at": "2026-10-28T00:00:00Z" }],
      "providerKeyError": "Daytona rejected this API key. Check it and try again.",
      "provisioningError": "[provider_credential_invalid] provider credential is invalid\nHTTP 409",
    })),
  );
  let keys = provider_keys(&mut h).unwrap();
  assert!(keys.rows[0].invalid);
  assert_eq!(keys.rows[0].status, "Rejected by Daytona. Replace the key.");
  assert_eq!(
    keys.error.as_deref(),
    Some("Daytona rejected this API key. Check it and try again.")
  );
  let card = cloud_card(&mut h);
  assert_eq!(
    card.credential_invalid.as_deref(),
    Some("Your provider rejected the saved API key. Replace it under Provider keys.")
  );
  assert_eq!(card.error, None);
}

#[gpui::test]
fn byos_create_offers_your_own_provider_account(cx: &mut TestAppContext) {
  let host = byos_host(true);
  let byos_catalog = serde_json::json!([
    { "provider_kind": "boat", "tier_id": "s", "hourly_usd": 0.0009, "provider_hourly_usd": 0.018 },
    { "provider_kind": "boat", "tier_id": "m", "hourly_usd": 0.0018, "provider_hourly_usd": 0.036 }
  ]);
  let mut h = signed_in_with_cloud(
    cx,
    host.clone(),
    cloud(serde_json::json!({
      "tiers": tiers(), "selectedTier": "s", "providerKeys": boat_key(), "byosCatalog": byos_catalog,
    })),
  );
  let card = cloud_card(&mut h);
  assert_eq!(
    card.placements,
    [
      settings::PlacementOption {
        kind: None,
        label: "treq managed".into(),
        selected: true
      },
      settings::PlacementOption {
        kind: Some("boat".into()),
        label: "Your Boat account".into(),
        selected: false
      },
    ]
  );
  assert_eq!(card.byos_note, None);
  h.click("cloud-placement-boat");
  h.click("cloud-placement-managed");
  assert_eq!(
    host.writes(),
    [
      "cloud SelectProvider { kind: Some(\"boat\") }",
      "cloud SelectProvider { kind: None }",
    ]
  );

  set_cloud(
    &mut h,
    serde_json::json!({
      "tiers": tiers(), "selectedTier": "s", "providerKeys": boat_key(),
      "byosCatalog": byos_catalog, "selectedProvider": "boat",
    }),
  );
  let card = cloud_card(&mut h);
  assert!(card.placements[1].selected);
  assert_eq!(card.sizes.len(), 2);
  assert_eq!(card.sizes[0].price.as_deref(), Some("$0.0009/hour"));
  assert_eq!(
    card.byos_note.as_deref(),
    Some("Boat bills you directly for the machine ($0.0180/hour for Small). treq charges 5% of Boat's price.")
  );
}
