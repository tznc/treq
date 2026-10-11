//! Live acceptance for the mobile app's managed-instance flow against a
//! dedicated Supabase test project whose Edge Functions talk to a dedicated
//! Fly Sprites test organization.
//!
//! It drives the app's own Rust control-plane client (`control_plane::Client`)
//! and treq's SSH transport the way `managed::connect` and `remote.rs` do:
//! status, wake, device-key registration (and its idempotent replay),
//! certificate issue, typed commands over the relay, renewal on the same
//! connection, and the renewal refusal `managed::classify_renewal_error`
//! must map to a key-revoked cutoff. The device key normally lives in the
//! OS keystore; here it is a temporary file, so the suite runs on a desktop.
//!
//! Gated on `TREQ_REMOTE_E2E=1`, `TREQ_REMOTE_E2E_MOBILE=1` and the Supabase
//! test-project credentials. Without them the test prints SKIP and passes
//! without asserting anything, which is not an acceptance pass.
//!
//! Run (see `SPRITES_E2E.md`):
//! `TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_MOBILE=1 cargo test --test sprites_e2e -- --ignored --nocapture`

use std::time::{Duration, Instant};

use russh::keys::PrivateKey;
use serde_json::{json, Value};
use treq_gpui_lib::{
  control_plane::{Client, FunctionError},
  managed,
};
use treq_lib::core::{
  remote::{
    execute_remote_command, retry_after_reconnect, MutationRetryOutcome, TreqCommandRequest,
  },
  remote_control_plane::{IssueCertificateResponse, SshAuthentication, SshEndpoint, SshTransport},
  remote_ssh_transport::{CancellationToken, CutoffReason, ExecLimits, SshConnectionPool},
};

const READY_TIMEOUT: Duration = Duration::from_secs(10 * 60);

struct Cfg {
  url: String,
  anon_key: String,
  service_role_key: String,
}

fn cfg() -> Option<Cfg> {
  if std::env::var("TREQ_REMOTE_E2E").as_deref() != Ok("1") {
    eprintln!("[mobile-sprites-e2e] SKIP: TREQ_REMOTE_E2E=1 not set");
    return None;
  }
  if std::env::var("TREQ_REMOTE_E2E_MOBILE").as_deref() != Ok("1") {
    eprintln!("[mobile-sprites-e2e] SKIP: TREQ_REMOTE_E2E_MOBILE=1 not set");
    return None;
  }
  let var = |name: &str| {
    let value = std::env::var(name).ok().filter(|v| !v.is_empty());
    if value.is_none() {
      eprintln!("[mobile-sprites-e2e] SKIP: {name} not set");
    }
    value
  };
  Some(Cfg {
    url: var("SUPABASE_TEST_URL")?.trim_end_matches('/').to_string(),
    anon_key: var("SUPABASE_TEST_ANON_KEY")?,
    service_role_key: var("SUPABASE_TEST_SERVICE_ROLE_KEY")?,
  })
}

fn e2e_tag() -> String {
  format!("treq-e2e-{}", uuid::Uuid::new_v4())
}

fn step(text: &str) {
  eprintln!("[mobile-sprites-e2e] {text}");
}

/// A disposable, Pro-entitled test user. Dropping it deletes the managed
/// instance (and so the Sprite), the billing rows and the user, whether
/// the test passed or panicked.
struct TestUser {
  cfg: Cfg,
  id: String,
  access_token: String,
  customer_id: String,
  /// Distinguishes this user's billing customer, subscription and events.
  billing_suffix: String,
}

impl TestUser {
  async fn create(cfg: Cfg) -> Self {
    let http = reqwest::Client::new();
    let tag = e2e_tag();
    let email = format!("{tag}@e2e.treq.invalid");
    let password = uuid::Uuid::new_v4().to_string();
    let created: Value = admin(&http, &cfg, "POST", "/auth/v1/admin/users")
      .json(&json!({ "email": email, "password": password, "email_confirm": true }))
      .send()
      .await
      .expect("create user")
      .error_for_status()
      .expect("admin create user")
      .json()
      .await
      .expect("create user json");
    let id = created["id"].as_str().expect("user id").to_string();
    let suffix = uuid::Uuid::new_v4().simple().to_string()[..16].to_string();
    let mut user = Self {
      cfg,
      id,
      access_token: String::new(),
      customer_id: format!("cus_e2e{suffix}"),
      billing_suffix: suffix,
    };
    // From here on, Drop cleans up even if a later step panics.
    user.grant_pro(&http).await;
    let session: Value = http
      .post(format!(
        "{}/auth/v1/token?grant_type=password",
        user.cfg.url
      ))
      .header("apikey", &user.cfg.anon_key)
      .json(&json!({ "email": email, "password": password }))
      .send()
      .await
      .expect("sign in")
      .error_for_status()
      .expect("sign in status")
      .json()
      .await
      .expect("session json");
    user.access_token = session["access_token"]
      .as_str()
      .expect("access_token")
      .to_string();
    user
  }

  /// `ensure`, `wake` and `reprovision` answer 402 `pro_required` without
  /// Pro. Grants it through the same RPCs the Stripe webhook calls, as
  /// `scripts/service-qa/billing.ts` does.
  async fn grant_pro(&self, http: &reqwest::Client) {
    let suffix = &self.billing_suffix;
    let rpc = |name: &str, args: Value| {
      admin(http, &self.cfg, "POST", &format!("/rest/v1/rpc/{name}")).json(&args)
    };
    let mapped: Value = rpc(
      "billing_record_checkout_completed",
      json!({
        "p_event_id": format!("evt_e2e{suffix}a"),
        "p_customer_id": self.customer_id,
        "p_owner_type": "user",
        "p_owner_id": self.id,
      }),
    )
    .send()
    .await
    .expect("checkout rpc")
    .json()
    .await
    .expect("checkout rpc json");
    assert_eq!(mapped, json!("applied"), "checkout mapping: {mapped}");
    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let created: Value = rpc(
      "billing_record_subscription_event",
      json!({
        "p_event_id": format!("evt_e2e{suffix}b"),
        "p_event_type": "customer.subscription.created",
        "p_event_created": chrono::Utc::now().to_rfc3339(),
        "p_subscription_id": format!("sub_e2e{suffix}"),
        "p_customer_id": self.customer_id,
        "p_plan": "pro",
        "p_status": "active",
        "p_current_period_end": period_end.to_rfc3339(),
        "p_cancel_at_period_end": false,
        "p_trial_end": null,
      }),
    )
    .send()
    .await
    .expect("subscription rpc")
    .json()
    .await
    .expect("subscription rpc json");
    assert_eq!(created, json!("applied"), "subscription: {created}");
  }

  fn client(&self) -> Client {
    Client {
      http: reqwest::Client::new(),
      url: self.cfg.url.clone(),
      anon_key: self.cfg.anon_key.clone(),
      token: self.access_token.clone(),
    }
  }

  /// remote-instance/remote-ssh-trust calls the mobile app does not make
  /// (setup and teardown happen on desktop).
  async fn call(&self, function: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
      .post(format!("{}/functions/v1/{function}", self.cfg.url))
      .bearer_auth(&self.access_token)
      .header("apikey", &self.cfg.anon_key)
      .json(&body)
      .send()
      .await
      .expect("edge function call");
    let status = response.status().as_u16();
    let json = response.json().await.unwrap_or(Value::Null);
    if let Some(id) = json["correlation_id"].as_str() {
      step(&format!("{function} correlation_id={id}"));
    }
    (status, json)
  }
}

impl Drop for TestUser {
  fn drop(&mut self) {
    let url = self.cfg.url.clone();
    let anon = self.cfg.anon_key.clone();
    let service = self.cfg.service_role_key.clone();
    let (id, access, suffix) = (
      self.id.clone(),
      self.access_token.clone(),
      self.billing_suffix.clone(),
    );
    // Drop runs inside the test's runtime, so clean up on a fresh one.
    let _ = std::thread::spawn(move || {
      let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("cleanup runtime");
      rt.block_on(async move {
        let http = reqwest::Client::new();
        let cfg = Cfg {
          url,
          anon_key: anon,
          service_role_key: service,
        };
        if !access.is_empty() {
          let deleted = http
            .post(format!("{}/functions/v1/remote-instance", cfg.url))
            .bearer_auth(&access)
            .header("apikey", &cfg.anon_key)
            .json(&json!({ "action": "delete", "idempotency_key": e2e_tag() }))
            .send()
            .await;
          eprintln!(
            "[mobile-sprites-e2e] cleanup: delete instance -> {:?}",
            deleted.map(|r| r.status())
          );
        }
        for path in [
          format!("/rest/v1/billing_subscriptions?owner_id=eq.{id}"),
          format!("/rest/v1/billing_customers?owner_id=eq.{id}"),
          format!("/rest/v1/billing_events?stripe_event_id=in.(evt_e2e{suffix}a,evt_e2e{suffix}b)"),
        ] {
          let _ = admin(&http, &cfg, "DELETE", &path).send().await;
        }
        let user = admin(&http, &cfg, "DELETE", &format!("/auth/v1/admin/users/{id}"))
          .send()
          .await;
        eprintln!(
          "[mobile-sprites-e2e] cleanup: delete user -> {:?}",
          user.map(|r| r.status())
        );
      });
    })
    .join();
  }
}

fn admin(http: &reqwest::Client, cfg: &Cfg, method: &str, path: &str) -> reqwest::RequestBuilder {
  let url = format!("{}{path}", cfg.url);
  let request = match method {
    "DELETE" => http.delete(url),
    _ => http.post(url),
  };
  request
    .header("apikey", &cfg.service_role_key)
    .bearer_auth(&cfg.service_role_key)
}

/// A device key, as the keystore would hold it, written to a 0600 file.
fn device_key(dir: &std::path::Path) -> (String, String, String) {
  use getrandom::SysRng;
  use rand_core::UnwrapErr;
  let key = PrivateKey::random(&mut UnwrapErr(SysRng), russh::keys::Algorithm::Ed25519).unwrap();
  let path = dir.join("device_key");
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
  let fingerprint = key
    .public_key()
    .fingerprint(russh::keys::HashAlg::Sha256)
    .to_string();
  (path.to_string_lossy().into_owned(), public, fingerprint)
}

/// `managed::with_device_certificate`, with a file instead of the keystore.
fn endpoint_with_certificate(response: IssueCertificateResponse, key_path: &str) -> SshEndpoint {
  let mut endpoint = response.endpoint;
  endpoint.authentication = SshAuthentication::Certificate {
    key_reference: key_path.to_string(),
    certificate: Some(response.certificate),
  };
  endpoint
}

async fn wait_ready(client: &Client) -> String {
  let deadline = Instant::now() + READY_TIMEOUT;
  loop {
    let instance = client
      .instance_status()
      .await
      .unwrap_or_else(|e| panic!("status: {e}"))
      .expect("an instance after ensure");
    match instance.status.as_str() {
      "ready" => return instance.instance_id,
      "failed" | "deleted" | "degraded" => panic!("instance never became ready: {instance:?}"),
      status if Instant::now() > deadline => panic!("timed out waiting for ready (last: {status})"),
      _ => tokio::time::sleep(Duration::from_secs(5)).await,
    }
  }
}

fn limits() -> ExecLimits {
  ExecLimits {
    deadline: Duration::from_secs(120),
    max_output_bytes: 8 * 1024 * 1024,
  }
}

async fn read(
  pool: &SshConnectionPool,
  endpoint: &SshEndpoint,
  request: TreqCommandRequest,
) -> Value {
  execute_remote_command::<Value>(
    pool,
    endpoint,
    request.clone(),
    limits(),
    &CancellationToken::new(),
  )
  .await
  .unwrap_or_else(|e| panic!("read {request:?}: {e}"))
}

async fn mutate(pool: &SshConnectionPool, endpoint: &SshEndpoint, request: TreqCommandRequest) {
  let outcome = retry_after_reconnect::<Value, _>(
    pool,
    endpoint,
    request.clone(),
    limits(),
    &CancellationToken::new(),
    |_| {},
  )
  .await
  .unwrap_or_else(|e| panic!("mutate {request:?}: {e}"));
  assert!(
    matches!(
      outcome,
      MutationRetryOutcome::Applied(_) | MutationRetryOutcome::AlreadyApplied
    ),
    "{request:?} did not apply"
  );
}

#[test]
fn mobile_sprites_suite_is_ignored_without_opt_in() {
  if std::env::var("TREQ_REMOTE_E2E_MOBILE").as_deref() != Ok("1") {
    eprintln!("[mobile-sprites-e2e] SKIP without TREQ_REMOTE_E2E_MOBILE=1");
  }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live Sprites; TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_MOBILE=1 cargo test --test sprites_e2e -- --ignored"]
async fn mobile_managed_flow_against_real_sprites() {
  let Some(cfg) = cfg() else { return };
  let started = Instant::now();
  let user = TestUser::create(cfg).await;
  step(&format!("created Pro test user {}", user.id));
  let client = user.client();

  // A fresh account: the app shows "no managed instance yet".
  assert!(
    client.instance_status().await.expect("status").is_none(),
    "a new user must have no instance"
  );

  // Desktop sets the instance up; the phone only connects to it.
  step("ensure (desktop's part): provisioning a Sprite");
  let (status, body) = user
    .call(
      "remote-instance",
      json!({ "action": "ensure", "idempotency_key": e2e_tag() }),
    )
    .await;
  assert_eq!(status, 200, "ensure: {body}");
  let instance_id = wait_ready(&client).await;
  step(&format!(
    "instance {instance_id} ready after {:?}",
    started.elapsed()
  ));

  // managed::issue: register the device key, replay the registration, issue.
  let dir = tempfile::tempdir().expect("tempdir");
  let (key_path, public_key, fingerprint) = device_key(dir.path());
  let idempotency = format!("register:{fingerprint}");
  let key = client
    .register_client_key(&public_key, managed::DEVICE_KEY_COMMENT, &idempotency)
    .await
    .unwrap_or_else(|e| panic!("register_client_key: {e}"));
  let replay = client
    .register_client_key(&public_key, managed::DEVICE_KEY_COMMENT, &idempotency)
    .await
    .unwrap_or_else(|e| panic!("register_client_key replay: {e}"));
  assert_eq!(
    replay.id, key.id,
    "an idempotent replay must resolve the same key"
  );
  step(&format!("device key {} registered", key.id));

  let issued = client
    .issue_certificate(&instance_id, &key.id, false)
    .await
    .unwrap_or_else(|e| panic!("issue_certificate: {e}"));
  let first_expiry = issued.expires_at.clone();
  let mut endpoint = endpoint_with_certificate(issued, &key_path);
  assert!(
    matches!(endpoint.transport, SshTransport::Relay { .. }),
    "a Sprite is reachable only through the relay, got {:?}",
    endpoint.transport
  );
  assert!(
    !endpoint.host_keys.is_empty(),
    "the endpoint must pin host keys"
  );

  // managed::activate: the relay authenticates with the Supabase session.
  let pool = SshConnectionPool::new();
  pool.set_relay_access_token(Some(user.access_token.clone()));
  let repo = format!("/home/treq/{}", e2e_tag());
  step("running typed commands over the relay");
  mutate(
    &pool,
    &endpoint,
    TreqCommandRequest::InitRepo {
      repo: repo.clone(),
      idempotency_key: e2e_tag(),
    },
  )
  .await;
  let inspected = read(
    &pool,
    &endpoint,
    TreqCommandRequest::InspectRepository { repo: repo.clone() },
  )
  .await;
  assert!(!inspected.is_null(), "inspect returned null");
  mutate(
    &pool,
    &endpoint,
    TreqCommandRequest::CreateWorkspace {
      repo: repo.clone(),
      branch_name: "mobile-e2e".into(),
      source_branch: None,
      metadata: None,
      idempotency_key: e2e_tag(),
    },
  )
  .await;
  let workspaces = read(
    &pool,
    &endpoint,
    TreqCommandRequest::ListWorkspaces { repo: repo.clone() },
  )
  .await;
  assert!(
    workspaces.to_string().contains("mobile-e2e"),
    "new workspace missing from {workspaces}"
  );

  // managed::renewal_loop: renew and swap the certificate into the same
  // endpoint id, keeping the pooled connection.
  step("renewing the certificate");
  let renewed = client
    .issue_certificate(&instance_id, &key.id, true)
    .await
    .unwrap_or_else(|e| panic!("renewal: {e}"));
  assert!(
    renewed.expires_at >= first_expiry,
    "renewal moved expiry backwards"
  );
  let renewed = endpoint_with_certificate(renewed, &key_path);
  assert_eq!(renewed.id, endpoint.id, "renewal must keep the endpoint id");
  endpoint = renewed;
  read(
    &pool,
    &endpoint,
    TreqCommandRequest::ListWorkspaces { repo: repo.clone() },
  )
  .await;

  // managed::connect's wake path, then reconnect on a fresh pool with files intact.
  step("waking the instance");
  client
    .wake(&instance_id)
    .await
    .unwrap_or_else(|e| panic!("wake: {e}"));
  assert_eq!(
    wait_ready(&client).await,
    instance_id,
    "wake must keep the instance"
  );
  let reconnected = SshConnectionPool::new();
  reconnected.set_relay_access_token(Some(user.access_token.clone()));
  let after_wake = read(
    &reconnected,
    &endpoint,
    TreqCommandRequest::ListWorkspaces { repo: repo.clone() },
  )
  .await;
  assert!(
    after_wake.to_string().contains("mobile-e2e"),
    "files lost across wake: {after_wake}"
  );

  // A revoked device key: the renewal refusal must classify as a
  // key-revoked cutoff, which the app reports.
  step("revoking the device key");
  let (status, body) = user
    .call(
      "remote-ssh-trust",
      json!({ "action": "revoke_client_key", "key_id": key.id, "idempotency_key": e2e_tag() }),
    )
    .await;
  assert_eq!(status, 200, "revoke_client_key: {body}");
  let refusal: FunctionError = client
    .issue_certificate(&instance_id, &key.id, true)
    .await
    .expect_err("renewal with a revoked key must be refused");
  assert_eq!(
    managed::classify_renewal_error(&refusal),
    Some(CutoffReason::KeyRevoked),
    "unexpected classification for {refusal}"
  );
  client
    .report_cutoff(
      &instance_id,
      &endpoint.id,
      managed::reason_code(CutoffReason::KeyRevoked),
    )
    .await
    .unwrap_or_else(|e| panic!("report_cutoff: {e}"));

  step(&format!("passed in {:?}", started.elapsed()));
  drop(user);
}
