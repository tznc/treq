#![allow(clippy::expect_used, clippy::string_slice, reason = "test code")]
//! Live acceptance for the managed-instance flow against a dedicated test
//! project whose Edge Functions provision real Fly Sprites.
//!
//! Desktop and mobile share this flow: the same `remote-instance` and
//! `remote-ssh-trust` calls, and the same `treq_lib` relay transport, typed
//! commands, PTY channel and credential cutoff. Only key storage differs per
//! client, so this one suite covers both. See `SPRITES_E2E.md` for local runs.
//!
//! Gated on `TREQ_REMOTE_E2E=1` plus `TREQ_REMOTE_E2E_NATIVE=1` and the
//! Supabase test-project credentials. Missing credentials skip (via
//! `#[ignore]` + an inner gate), they never count as passing acceptance.
//!
//! Run:
//! `TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_NATIVE=1 cargo test --test remote_e2e_native -- --ignored --test-threads=1`

#![allow(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::print_stderr,
  reason = "test code reports progress on stderr"
)]

use std::time::Duration;

use russh::keys::PrivateKey;
use serde_json::Value;
use treq_lib::core::remote::TreqCommandRequest;
use treq_lib::core::remote_control_plane::{
  SshAuthentication, SshEndpoint, SshEndpointSource, TrustedHostKey,
};
use treq_lib::core::remote_ssh_transport::{
  exec_command, CancellationToken, CutoffReason, ExecLimits, RemotePtyChannel, SshConnectionPool,
  SshTransportError,
};

const LIVE_IGNORE: &str =
  "live native SSH; TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_NATIVE=1 cargo test --test remote_e2e_native -- --ignored";

struct NativeCfg {
  url: String,
  anon_key: String,
  service_role_key: String,
}

fn native_cfg() -> Option<NativeCfg> {
  if std::env::var("TREQ_REMOTE_E2E").as_deref() != Ok("1") {
    return None;
  }
  if std::env::var("TREQ_REMOTE_E2E_NATIVE").as_deref() != Ok("1") {
    eprintln!(
      "[remote-e2e-native] SKIP: TREQ_REMOTE_E2E_NATIVE=1 not set. \
       Native certificate authentication is not proven by an ordinary wake or control-plane-only test."
    );
    return None;
  }
  let url = std::env::var("SUPABASE_TEST_URL").ok()?;
  let anon_key = std::env::var("SUPABASE_TEST_ANON_KEY").ok()?;
  let service_role_key = std::env::var("SUPABASE_TEST_SERVICE_ROLE_KEY").ok()?;
  Some(NativeCfg {
    url,
    anon_key,
    service_role_key,
  })
}

macro_rules! require_native {
  () => {
    match native_cfg() {
      Some(cfg) => cfg,
      None => {
        eprintln!(
          "[remote-e2e-native] SKIP {}: missing native live credentials",
          module_path!()
        );
        return;
      }
    }
  };
}

fn e2e_tag() -> String {
  format!("treq-e2e-{}", uuid::Uuid::new_v4())
}

async fn json_post(
  url: &str,
  token: &str,
  extra_headers: &[(&str, &str)],
  body: Value,
) -> (u16, Value) {
  let mut request = reqwest::Client::new()
    .post(url)
    .json(&body)
    .header("Content-Type", "application/json");
  for (name, value) in extra_headers {
    request = request.header(*name, *value);
  }
  request = request.bearer_auth(token);
  let response = request.send().await.expect("http send");
  let status = response.status().as_u16();
  let json: Value = response.json().await.unwrap_or(Value::Null);
  if let Some(id) = json.get("correlation_id").and_then(|v| v.as_str()) {
    eprintln!("[remote-e2e-native] correlation_id={id}");
  }
  (status, json)
}

/// `ensure`, `wake` and `reprovision` answer 402 `pro_required` without Pro.
/// Grants it through the RPCs the Stripe webhook calls, as
/// `scripts/service-qa/billing.ts` does.
async fn grant_pro(cfg: &NativeCfg, user_id: &str, suffix: &str) {
  let customer_id = format!("cus_e2e{suffix}");
  let period_end = chrono::Utc::now() + chrono::Duration::days(30);
  for (name, args) in [
    (
      "billing_record_checkout_completed",
      serde_json::json!({
        "p_event_id": format!("evt_e2e{suffix}a"),
        "p_customer_id": customer_id,
        "p_owner_type": "user",
        "p_owner_id": user_id,
      }),
    ),
    (
      "billing_record_subscription_event",
      serde_json::json!({
        "p_event_id": format!("evt_e2e{suffix}b"),
        "p_event_type": "customer.subscription.created",
        "p_event_created": chrono::Utc::now().to_rfc3339(),
        "p_subscription_id": format!("sub_e2e{suffix}"),
        "p_customer_id": customer_id,
        "p_plan": "pro",
        "p_status": "active",
        "p_current_period_end": period_end.to_rfc3339(),
        "p_cancel_at_period_end": false,
        "p_trial_end": null,
      }),
    ),
  ] {
    let (status, body) = json_post(
      &format!("{}/rest/v1/rpc/{name}", cfg.url.trim_end_matches('/')),
      &cfg.service_role_key,
      &[("apikey", cfg.service_role_key.as_str())],
      args,
    )
    .await;
    assert!(
      status == 200 && body == serde_json::json!("applied"),
      "{name} failed: HTTP {status} {body}"
    );
  }
}

fn write_client_key(dir: &std::path::Path) -> (String, String) {
  use getrandom::SysRng;
  use rand_core::UnwrapErr;
  let key = PrivateKey::random(&mut UnwrapErr(SysRng), russh::keys::Algorithm::Ed25519).unwrap();
  let path = dir.join("id_e2e");
  let pem = key
    .to_openssh(russh::keys::ssh_key::LineEnding::LF)
    .unwrap();
  std::fs::write(&path, pem.as_bytes()).unwrap();
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
  }
  let public = key.public_key().to_openssh().expect("openssh public key");
  (path.to_string_lossy().into_owned(), public)
}

async fn exec_typed(
  pool: &SshConnectionPool,
  endpoint: &SshEndpoint,
  request: TreqCommandRequest,
) -> Value {
  let args = request.cli_args().expect("typed args");
  let output = exec_command(
    pool,
    endpoint,
    &args,
    ExecLimits {
      deadline: Duration::from_secs(60),
      max_output_bytes: 8 * 1024 * 1024,
    },
    &CancellationToken::new(),
  )
  .await
  .unwrap_or_else(|err| panic!("exec {:?} failed: {err:?}", args));
  serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
    panic!(
      "stdout was not JSON: {}",
      String::from_utf8_lossy(&output.stdout)
    )
  })
}

#[test]
fn native_suite_is_ignored_without_opt_in() {
  if std::env::var("TREQ_REMOTE_E2E_NATIVE").as_deref() != Ok("1") {
    eprintln!("[remote-e2e-native] SKIP without TREQ_REMOTE_E2E_NATIVE=1");
  }
}

#[tokio::test]
#[ignore = "live native SSH; TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_NATIVE=1 cargo test --test remote_e2e_native -- --ignored"]
async fn native_certificate_auth_two_repos_mutations_pty_reconnect_and_reprovision_trust() {
  let _ = LIVE_IGNORE;
  let cfg = require_native!();
  let tag = e2e_tag();
  let email = format!("{tag}@e2e.treq.invalid");
  let password = uuid::Uuid::new_v4().to_string();
  let admin = reqwest::Client::new();

  let created = admin
    .post(format!(
      "{}/auth/v1/admin/users",
      cfg.url.trim_end_matches('/')
    ))
    .header("apikey", &cfg.service_role_key)
    .bearer_auth(&cfg.service_role_key)
    .json(&serde_json::json!({
      "email": email,
      "password": password,
      "email_confirm": true
    }))
    .send()
    .await
    .expect("create user");
  assert!(
    created.status().is_success(),
    "admin create user failed: {}",
    created.text().await.unwrap_or_default()
  );
  let created_json: Value = created.json().await.expect("create user json");
  let user_id = created_json["id"].as_str().unwrap_or_default().to_string();
  let billing_suffix = uuid::Uuid::new_v4().simple().to_string()[..16].to_string();

  let sign_in = admin
    .post(format!(
      "{}/auth/v1/token?grant_type=password",
      cfg.url.trim_end_matches('/')
    ))
    .header("apikey", &cfg.anon_key)
    .json(&serde_json::json!({ "email": email, "password": password }))
    .send()
    .await
    .expect("sign in");
  let session: Value = sign_in.json().await.expect("session json");
  let access = session["access_token"]
    .as_str()
    .expect("access_token")
    .to_string();

  struct UserCleanup {
    url: String,
    service_role_key: String,
    user_id: String,
    access: String,
    anon_key: String,
    billing_suffix: String,
  }
  impl Drop for UserCleanup {
    fn drop(&mut self) {
      let url = self.url.clone();
      let key = self.service_role_key.clone();
      let user_id = self.user_id.clone();
      let access = self.access.clone();
      let anon = self.anon_key.clone();
      let billing_suffix = self.billing_suffix.clone();
      let _ = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
          .enable_all()
          .build()
          .expect("cleanup rt");
        rt.block_on(async move {
          let _ = json_post(
            &format!("{}/functions/v1/remote-instance", url.trim_end_matches('/')),
            &access,
            &[("apikey", anon.as_str())],
            serde_json::json!({ "action": "delete", "idempotency_key": format!("treq-e2e-{}", uuid::Uuid::new_v4()) }),
          )
          .await;
          // Deleting the user does not cascade to billing rows.
          for path in [
            format!("billing_subscriptions?owner_id=eq.{user_id}"),
            format!("billing_customers?owner_id=eq.{user_id}"),
            format!("billing_events?stripe_event_id=in.(evt_e2e{billing_suffix}a,evt_e2e{billing_suffix}b)"),
          ] {
            let _ = reqwest::Client::new()
              .delete(format!("{}/rest/v1/{path}", url.trim_end_matches('/')))
              .header("apikey", &key)
              .bearer_auth(&key)
              .send()
              .await;
          }
          let _ = reqwest::Client::new()
            .delete(format!("{}/auth/v1/admin/users/{user_id}", url.trim_end_matches('/')))
            .header("apikey", &key)
            .bearer_auth(&key)
            .send()
            .await;
        });
      })
      .join();
    }
  }
  let cleanup = UserCleanup {
    url: cfg.url.clone(),
    service_role_key: cfg.service_role_key.clone(),
    user_id,
    access: access.clone(),
    anon_key: cfg.anon_key.clone(),
    billing_suffix: billing_suffix.clone(),
  };
  grant_pro(&cfg, &cleanup.user_id, &billing_suffix).await;

  let instance_url = format!(
    "{}/functions/v1/remote-instance",
    cfg.url.trim_end_matches('/')
  );
  let trust_url = format!(
    "{}/functions/v1/remote-ssh-trust",
    cfg.url.trim_end_matches('/')
  );
  let api_headers = [("apikey", cfg.anon_key.as_str())];

  let (st, empty_json) = json_post(
    &instance_url,
    &access,
    &api_headers,
    serde_json::json!({ "action": "status" }),
  )
  .await;
  assert_eq!(st, 200, "status failed: {empty_json}");
  assert!(
    empty_json["instance"].is_null(),
    "a new user must have no instance: {empty_json}"
  );

  let (ensure_status, ensure_json) = json_post(
    &instance_url,
    &access,
    &api_headers,
    // No region or size: Sprites size every VM themselves.
    serde_json::json!({
      "action": "ensure",
      "idempotency_key": e2e_tag()
    }),
  )
  .await;
  assert_eq!(ensure_status, 200, "ensure failed: {ensure_json}");

  let deadline = std::time::Instant::now() + Duration::from_secs(10 * 60);
  loop {
    let (st, status_json) = json_post(
      &instance_url,
      &access,
      &api_headers,
      serde_json::json!({ "action": "status" }),
    )
    .await;
    assert_eq!(st, 200);
    let status = status_json["instance"]["status"].as_str().unwrap_or("");
    if status == "ready" {
      break;
    }
    if status == "failed" || status == "degraded" {
      panic!("instance never ready: {status_json}");
    }
    if std::time::Instant::now() > deadline {
      panic!("timed out waiting for ready: {status_json}");
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
  }

  let dir = tempfile::tempdir().expect("tempdir");
  let (key_path, public_line) = write_client_key(dir.path());
  // Clients retry registration with a stable idempotency key; a replay must
  // return the same key rather than registering a second one.
  let register = serde_json::json!({
    "action": "register_client_key",
    "idempotency_key": e2e_tag(),
    "public_key": format!("{public_line} {tag}@e2e")
  });
  let (reg_status, reg_json) = json_post(&trust_url, &access, &api_headers, register.clone()).await;
  assert_eq!(reg_status, 200, "register_client_key failed: {reg_json}");
  let key_id = reg_json["key"]["id"].as_str().expect("key id").to_string();
  let (replay_status, replay_json) = json_post(&trust_url, &access, &api_headers, register).await;
  assert_eq!(
    replay_status, 200,
    "register_client_key replay failed: {replay_json}"
  );
  // A replay answers with the user's key list instead of the single key.
  let replayed_ids: Vec<&str> = replay_json["key"]["id"]
    .as_str()
    .into_iter()
    .chain(
      replay_json["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| k["id"].as_str()),
    )
    .collect();
  assert_eq!(
    replayed_ids,
    vec![key_id.as_str()],
    "a replayed registration must resolve the same single key: {replay_json}"
  );
  let instance_id = json_post(
    &instance_url,
    &access,
    &api_headers,
    serde_json::json!({ "action": "status" }),
  )
  .await
  .1["instance"]["instance_id"]
    .as_str()
    .expect("instance id")
    .to_string();

  let (issue_status, issue_json) = json_post(
    &trust_url,
    &access,
    &api_headers,
    serde_json::json!({
      "action": "issue_certificate",
      "instance_id": instance_id,
      "key_id": key_id
    }),
  )
  .await;
  assert_eq!(issue_status, 200, "issue_certificate failed: {issue_json}");
  let cert_line = issue_json["certificate"].as_str().expect("certificate");

  let endpoint_json = &issue_json["endpoint"];
  let host_keys = host_keys_from(endpoint_json);
  assert!(
    !host_keys.is_empty(),
    "certificate response must include trusted host keys"
  );

  let mut endpoint = SshEndpoint {
    id: endpoint_json["id"]
      .as_str()
      .unwrap_or("e2e-endpoint")
      .to_string(),
    instance_id: Some(instance_id.clone()),
    source: SshEndpointSource::Managed {
      provider: "fly_sprites".to_string(),
      generation: endpoint_json["source"]["generation"].as_u64().unwrap_or(0),
    },
    hostname: endpoint_json["hostname"]
      .as_str()
      .expect("hostname")
      .to_string(),
    port: endpoint_json["port"].as_u64().unwrap_or(22) as u16,
    username: endpoint_json["username"]
      .as_str()
      .unwrap_or("treq")
      .to_string(),
    host_keys,
    authentication: SshAuthentication::Certificate {
      key_reference: key_path.clone(),
      certificate: Some(cert_line.to_string()),
    },
    // Sprites have no raw TCP ingress: the response routes SSH through the
    // `remote-ssh-relay` Edge Function.
    transport: serde_json::from_value(endpoint_json["transport"].clone()).unwrap_or_default(),
  };

  let pool = SshConnectionPool::new();
  pool.set_relay_access_token(Some(access.clone()));
  let repo_a = "/home/treq/e2e-repo-a".to_string();
  let repo_b = "/home/treq/e2e-repo-b".to_string();
  exec_typed(
    &pool,
    &endpoint,
    TreqCommandRequest::InitRepo {
      repo: repo_a.clone(),
      idempotency_key: e2e_tag(),
    },
  )
  .await;
  exec_typed(
    &pool,
    &endpoint,
    TreqCommandRequest::InitRepo {
      repo: repo_b.clone(),
      idempotency_key: e2e_tag(),
    },
  )
  .await;
  let inspect_a = exec_typed(
    &pool,
    &endpoint,
    TreqCommandRequest::InspectRepository {
      repo: repo_a.clone(),
    },
  )
  .await;
  let inspect_b = exec_typed(
    &pool,
    &endpoint,
    TreqCommandRequest::InspectRepository {
      repo: repo_b.clone(),
    },
  )
  .await;
  assert!(
    !inspect_a.is_null() && !inspect_b.is_null(),
    "both repositories must inspect"
  );

  exec_typed(
    &pool,
    &endpoint,
    TreqCommandRequest::CreateWorkspace {
      repo: repo_a.clone(),
      branch_name: format!("feat/{tag}"),
      source_branch: None,
      metadata: None,
      idempotency_key: e2e_tag(),
    },
  )
  .await;
  let workspaces = exec_typed(
    &pool,
    &endpoint,
    TreqCommandRequest::ListWorkspaces {
      repo: repo_a.clone(),
    },
  )
  .await;
  assert!(
    workspaces.to_string().contains("feat/"),
    "workspace list should include the created workspace: {workspaces}"
  );

  let pty = RemotePtyChannel::open(
    &pool,
    &endpoint,
    "xterm",
    80,
    24,
    Some("bash -lc 'cd /home/treq/e2e-repo-a && pwd'"),
  )
  .await
  .expect("pty open in selected workspace");
  let _ = pty.read_chunk().await;
  pty.close().await.expect("pty close");

  let agent = TreqCommandRequest::AgentStatus {
    repo: repo_a.clone(),
    workspace: format!("feat/{tag}"),
  };
  let agent_args = agent.cli_args().expect("agent args");
  let _ = exec_command(
    &pool,
    &endpoint,
    &agent_args,
    ExecLimits::default(),
    &CancellationToken::new(),
  )
  .await;

  // Silent renewal: the same action with `renewal: true`, swapped into the
  // same endpoint id while the pool stays open.
  let (renew_status, renew_json) = json_post(
    &trust_url,
    &access,
    &api_headers,
    serde_json::json!({
      "action": "issue_certificate",
      "instance_id": instance_id,
      "key_id": key_id,
      "renewal": true
    }),
  )
  .await;
  assert_eq!(renew_status, 200, "renewal failed: {renew_json}");
  assert_eq!(
    renew_json["endpoint"]["id"].as_str(),
    Some(endpoint.id.as_str()),
    "renewal must keep the endpoint id"
  );
  assert!(
    renew_json["expires_at"].as_str().unwrap_or_default()
      >= issue_json["expires_at"].as_str().unwrap_or_default(),
    "renewal moved expiry backwards: {renew_json}"
  );
  endpoint.authentication = SshAuthentication::Certificate {
    key_reference: key_path.clone(),
    certificate: Some(
      renew_json["certificate"]
        .as_str()
        .expect("renewed certificate")
        .to_string(),
    ),
  };
  exec_typed(
    &pool,
    &endpoint,
    TreqCommandRequest::ListWorkspaces {
      repo: repo_a.clone(),
    },
  )
  .await;

  // Plain reconnect: a fresh connection pool reaches the same repository.
  drop(pool);
  let pool2 = SshConnectionPool::new();
  pool2.set_relay_access_token(Some(access.clone()));
  let inspect_again = exec_typed(
    &pool2,
    &endpoint,
    TreqCommandRequest::InspectRepository {
      repo: repo_a.clone(),
    },
  )
  .await;
  assert!(!inspect_again.is_null());
  drop(pool2);

  // Normal managed lifecycle recovery (acceptance criterion 10). Sprites
  // have no suspend call; they pause about 30 seconds after the last
  // activity and drop open TCP connections when they do. With every pool
  // closed, wait out the idle window, then wake through the control plane
  // and reconnect. The repository must still be there: nothing was
  // recreated and nothing was re-registered.
  let idle = idle_pause_wait();
  eprintln!("[remote-e2e-native] idling {idle:?} so the Sprite pauses");
  tokio::time::sleep(idle).await;
  let (_, paused_json) = json_post(
    &instance_url,
    &access,
    &api_headers,
    serde_json::json!({ "action": "status" }),
  )
  .await;
  eprintln!(
    "[remote-e2e-native] status after idle: {}",
    paused_json["instance"]["status"]
  );
  let provider_id_before = paused_json["instance"]["provider_resource_id"].clone();
  let (wake_status, wake_json) = json_post(
    &instance_url,
    &access,
    &api_headers,
    serde_json::json!({ "action": "wake", "idempotency_key": e2e_tag() }),
  )
  .await;
  assert_eq!(wake_status, 200, "wake failed: {wake_json}");
  let (_, woken_json) = json_post(
    &instance_url,
    &access,
    &api_headers,
    serde_json::json!({ "action": "status" }),
  )
  .await;
  assert_eq!(
    woken_json["instance"]["status"], "ready",
    "instance must be ready after wake: {woken_json}"
  );
  assert_eq!(
    woken_json["instance"]["provider_resource_id"], provider_id_before,
    "wake must not recreate the VM"
  );
  let pool_after_wake = SshConnectionPool::new();
  pool_after_wake.set_relay_access_token(Some(access.clone()));
  let inspect_after_wake = exec_typed(
    &pool_after_wake,
    &endpoint,
    TreqCommandRequest::InspectRepository {
      repo: repo_a.clone(),
    },
  )
  .await;
  assert!(
    !inspect_after_wake.is_null(),
    "repository must be reachable after wake without re-registration"
  );
  drop(pool_after_wake);

  // Repair. Sprites repair in place, so the provider identity and the
  // generation stay the same and so do the VM's SSH host keys. If a future
  // provider does replace the VM, the generation must increase and the
  // control plane must hand out the new host keys at that generation. In
  // both cases the client pins exactly what the control plane returns.
  let generation_before = managed_generation(&endpoint);
  let (repro_status, repro_json) = json_post(
    &instance_url,
    &access,
    &api_headers,
    serde_json::json!({
      "action": "reprovision",
      "idempotency_key": e2e_tag()
    }),
  )
  .await;
  assert_eq!(repro_status, 200, "reprovision failed: {repro_json}");

  let (issue2_status, issue2) = json_post(
    &trust_url,
    &access,
    &api_headers,
    serde_json::json!({
      "action": "issue_certificate",
      "instance_id": instance_id,
      "key_id": key_id
    }),
  )
  .await;
  assert_eq!(
    issue2_status, 200,
    "reissue after reprovision failed: {issue2}"
  );
  let generation_after = issue2["endpoint"]["source"]["generation"]
    .as_u64()
    .unwrap_or(generation_before);
  let host_keys_after = host_keys_from(&issue2["endpoint"]);
  assert!(
    !host_keys_after.is_empty(),
    "reissued certificate response must include trusted host keys"
  );
  let fingerprints = |keys: &[TrustedHostKey]| {
    let mut fps: Vec<String> = keys.iter().map(|k| k.fingerprint_sha256.clone()).collect();
    fps.sort();
    fps
  };
  if generation_after == generation_before {
    assert_eq!(
      fingerprints(&endpoint.host_keys),
      fingerprints(&host_keys_after),
      "an in-place repair must not change host trust at the same generation"
    );
  } else {
    assert!(
      generation_after > generation_before,
      "generation must never go backwards: {generation_before} -> {generation_after}"
    );
  }

  endpoint.authentication = SshAuthentication::Certificate {
    key_reference: key_path.clone(),
    certificate: Some(
      issue2["certificate"]
        .as_str()
        .expect("reissued certificate")
        .to_string(),
    ),
  };
  endpoint.hostname = issue2["endpoint"]["hostname"]
    .as_str()
    .unwrap_or(&endpoint.hostname)
    .to_string();
  endpoint.host_keys = host_keys_after;
  endpoint.source = SshEndpointSource::Managed {
    provider: "fly_sprites".to_string(),
    generation: generation_after,
  };
  let pool3 = SshConnectionPool::new();
  pool3.set_relay_access_token(Some(access.clone()));
  let after_repro = exec_typed(
    &pool3,
    &endpoint,
    TreqCommandRequest::InspectRepository {
      repo: repo_a.clone(),
    },
  )
  .await;
  assert!(
    !after_repro.is_null(),
    "repository must survive an in-place repair"
  );

  // Revoked key. The renewal refusal is what both clients classify as a
  // key-revoked cutoff (409 "Key has been revoked"); the pool then refuses
  // further commands locally and the client reports the cutoff.
  let (revoke_status, revoke_json) = json_post(
    &trust_url,
    &access,
    &api_headers,
    serde_json::json!({
      "action": "revoke_client_key",
      "key_id": key_id,
      "idempotency_key": e2e_tag()
    }),
  )
  .await;
  assert_eq!(
    revoke_status, 200,
    "revoke_client_key failed: {revoke_json}"
  );
  let (refused_status, refused_json) = json_post(
    &trust_url,
    &access,
    &api_headers,
    serde_json::json!({
      "action": "issue_certificate",
      "instance_id": instance_id,
      "key_id": key_id,
      "renewal": true
    }),
  )
  .await;
  assert!(
    refused_status == 409 && refused_json.to_string().to_lowercase().contains("revoked"),
    "renewal with a revoked key must be refused as revoked: HTTP {refused_status} {refused_json}"
  );
  pool3
    .force_cutoff(&endpoint.id, CutoffReason::KeyRevoked)
    .await;
  let refused = exec_command(
    &pool3,
    &endpoint,
    &TreqCommandRequest::InspectRepository { repo: repo_a }
      .cli_args()
      .expect("typed args"),
    ExecLimits::default(),
    &CancellationToken::new(),
  )
  .await;
  assert!(
    matches!(
      refused,
      Err(SshTransportError::CredentialCutOff {
        reason: CutoffReason::KeyRevoked,
        ..
      })
    ),
    "a cut-off endpoint must refuse commands locally: {refused:?}"
  );
  let (cutoff_status, cutoff_json) = json_post(
    &trust_url,
    &access,
    &api_headers,
    serde_json::json!({
      "action": "report_cutoff",
      "instance_id": instance_id,
      "endpoint_id": endpoint.id,
      "reason": "key_revoked"
    }),
  )
  .await;
  assert_eq!(cutoff_status, 200, "report_cutoff failed: {cutoff_json}");
  drop(cleanup);
}

/// How long the lifecycle step leaves the VM idle before waking it. The
/// provider pauses an idle Sprite after about 30 seconds; the default adds
/// margin. Overridable with `TREQ_REMOTE_E2E_IDLE_PAUSE_WAIT_SECS`.
fn idle_pause_wait() -> Duration {
  Duration::from_secs(
    std::env::var("TREQ_REMOTE_E2E_IDLE_PAUSE_WAIT_SECS")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(90),
  )
}

fn managed_generation(endpoint: &SshEndpoint) -> u64 {
  match &endpoint.source {
    SshEndpointSource::Managed { generation, .. } => *generation,
    _ => 0,
  }
}

fn host_keys_from(endpoint_json: &Value) -> Vec<TrustedHostKey> {
  endpoint_json["host_keys"]
    .as_array()
    .cloned()
    .unwrap_or_default()
    .into_iter()
    .map(|row| TrustedHostKey {
      algorithm: row["algorithm"]
        .as_str()
        .unwrap_or("ssh-ed25519")
        .to_string(),
      fingerprint_sha256: row["fingerprint_sha256"]
        .as_str()
        .unwrap_or_default()
        .to_string(),
      comment: row["comment"].as_str().map(str::to_string),
    })
    .collect()
}
