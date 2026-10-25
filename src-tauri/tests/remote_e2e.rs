#![expect(clippy::expect_used, reason = "test code")]
//! Real-provider end-to-end tests for the managed-compute provider adapter
//! (prds/remote-development.md, "Engineering and operational requirements":
//! real-provider integration tests use isolated test resources and safe
//! cleanup).
//!
//! Everything in this file talks to the *real* Fly Sprites API. Nothing here
//! is mocked. It is the non-mocked counterpart to the `wiremock` unit tests
//! in `src-tauri/src/core/remote_provider_sprites.rs`.
//!
//! ## Running this suite
//!
//! Every live test here is gated on real credentials. With no credentials
//! set, `cargo test --test remote_e2e` compiles the harness and runs only the
//! local (non-ignored) gate tests. Live tests are `#[ignore]` so missing
//! credentials are an explicit skip, not a passing acceptance run. Run them
//! with:
//! `TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1`
//! See `remote_e2e_README.md`.
//!
//! Environment variables:
//!
//! - `TREQ_REMOTE_E2E=1`: explicit opt-in. Nothing runs without it, so a
//!   stray token in a shared CI environment can never trigger spend.
//! - `TREQ_SPRITES_TEST_API_TOKEN`: a Sprites API token for a dedicated test
//!   organization. Sprites tokens are organization-scoped, so this must
//!   never be a token for an organization that holds real user Sprites.
//! - `TREQ_SPRITES_TEST_API_URL`: optional, defaults to `https://api.sprites.dev`.
//! - `TREQ_REMOTE_E2E_IDLE_PAUSE_TIMEOUT_SECS`: optional upper bound on how
//!   long the suspend/wake test waits for the Sprite to pause on its own
//!   (default 600).

#![expect(clippy::print_stderr, reason = "test code reports progress on stderr")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

use serde_json::Value;
use treq_lib::core::remote_provider::{
  CreateInstanceRequest, ManagedComputeProvider, ManagedInstanceState, ProviderError, RegionCode,
  ReplaceInstanceRequest, SizePreset,
};
use treq_lib::core::remote_provider_sprites::{SpritesConfig, SpritesProvider};

/// Every resource this suite creates carries this prefix so a cleanup pass
/// (`scripts/remote-e2e-cleanup.ts`) can find and remove it by name,
/// independent of which test created it or whether that test's own
/// compensating cleanup ran.
pub const E2E_TAG_PREFIX: &str = "treq-e2e-";

/// The adapter names a Sprite `dev-treq-<owner>`, and this suite uses an
/// e2e tag as the owner. The cleanup script matches on exactly this shape.
const E2E_SPRITE_NAME_PREFIX: &str = "dev-treq-treq-e2e-";

const DEFAULT_API_URL: &str = "https://api.sprites.dev";

const LIVE: &str =
  "live Sprites; TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1";

/// Hard cap on how many test Sprites this suite will have alive at once,
/// enforced in-process via `ConcurrencyGuard`. Overridable for a wider test
/// organization via `TREQ_REMOTE_E2E_MAX_CONCURRENCY`. The default is small
/// because every Sprite here is a real billable resource.
fn max_concurrent_instances() -> usize {
  std::env::var("TREQ_REMOTE_E2E_MAX_CONCURRENCY")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(2)
}

/// Sprites pause about 30 seconds after the last activity, then fall from
/// warm to cold on their own. The bound is generous so that a slow provider
/// day does not flake the nightly run, and finite so a Sprite that never
/// pauses fails the test instead of hanging CI.
fn idle_pause_timeout() -> Duration {
  Duration::from_secs(
    std::env::var("TREQ_REMOTE_E2E_IDLE_PAUSE_TIMEOUT_SECS")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(600),
  )
}

static ACTIVE_INSTANCES: AtomicUsize = AtomicUsize::new(0);

/// RAII concurrency-cap guard. Acquired before any real `create_instance`
/// call and held for the lifetime of that Sprite. Dropping it (including on
/// panic) releases the slot so a failed test cannot wedge the cap.
struct ConcurrencyGuard;

impl ConcurrencyGuard {
  fn acquire() -> Self {
    loop {
      let current = ACTIVE_INSTANCES.load(Ordering::SeqCst);
      if current >= max_concurrent_instances() {
        panic!(
          "remote e2e concurrency cap ({}) reached; refusing to provision another real test Sprite",
          max_concurrent_instances()
        );
      }
      if ACTIVE_INSTANCES
        .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
      {
        return Self;
      }
    }
  }
}

impl Drop for ConcurrencyGuard {
  fn drop(&mut self) {
    ACTIVE_INSTANCES.fetch_sub(1, Ordering::SeqCst);
  }
}

/// Compensating cleanup for one Sprite. Deletes it on drop, which covers
/// test success, test failure, and panic unwind. `scripts/remote-e2e-cleanup.ts`
/// is the backstop for anything this misses (SIGKILL, runner loss).
struct InstanceCleanupGuard {
  config: SpritesConfig,
  provider_resource_id: Option<String>,
  _concurrency: ConcurrencyGuard,
}

impl InstanceCleanupGuard {
  fn new(config: &SpritesConfig, provider_resource_id: String) -> Self {
    Self {
      config: config.clone(),
      provider_resource_id: Some(provider_resource_id),
      _concurrency: ConcurrencyGuard::acquire(),
    }
  }
}

impl Drop for InstanceCleanupGuard {
  fn drop(&mut self) {
    let Some(id) = self.provider_resource_id.take() else {
      return;
    };
    // Drop cannot await, and the test's runtime may already be shutting
    // down during unwind, so run the delete on a throwaway runtime in its
    // own thread. Errors are logged and never panicked on: a cleanup
    // failure must not hide the original test failure.
    let config = self.config.clone();
    let target = id.clone();
    let result = std::thread::spawn(move || {
      let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build cleanup runtime");
      rt.block_on(async move {
        let provider =
          SpritesProvider::new(config).expect("failed to rebuild provider for cleanup");
        provider.delete_instance(&target).await
      })
    })
    .join();

    match result {
      Ok(Ok(())) => eprintln!("[remote-e2e cleanup] deleted Sprite {id}"),
      Ok(Err(err)) => eprintln!(
        "[remote-e2e cleanup] FAILED to delete Sprite {id}: {err:?} - \
         scripts/remote-e2e-cleanup.ts must catch this on its next run"
      ),
      Err(_) => eprintln!(
        "[remote-e2e cleanup] cleanup thread panicked for Sprite {id} - \
         scripts/remote-e2e-cleanup.ts must catch this on its next run"
      ),
    }
  }
}

/// Central skip gate. Every live test calls this first and returns early
/// when it is `None`, so a test body never asserts anything without
/// credentials.
fn e2e_config() -> Option<SpritesConfig> {
  static PRINT_BANNER: Once = Once::new();

  if std::env::var("TREQ_REMOTE_E2E").as_deref() != Ok("1") {
    PRINT_BANNER.call_once(|| {
      eprintln!(
        "[remote-e2e] SKIP: TREQ_REMOTE_E2E=1 not set. Real-API tests in \
         remote_e2e.rs do not run. See src-tauri/tests/remote_e2e_README.md."
      );
    });
    return None;
  }
  let api_token = std::env::var("TREQ_SPRITES_TEST_API_TOKEN")
    .ok()
    .filter(|v| !v.is_empty())?;
  let base_url = std::env::var("TREQ_SPRITES_TEST_API_URL")
    .ok()
    .filter(|v| !v.is_empty())
    .unwrap_or_else(|| DEFAULT_API_URL.to_string());
  Some(SpritesConfig {
    base_url,
    api_token,
    request_timeout: Duration::from_secs(60),
  })
}

macro_rules! require_e2e {
  () => {
    match e2e_config() {
      Some(cfg) => cfg,
      None => {
        eprintln!(
          "[remote-e2e] SKIP {}: missing TREQ_REMOTE_E2E=1 / TREQ_SPRITES_TEST_API_TOKEN",
          module_path!()
        );
        return;
      }
    }
  };
}

fn e2e_tag() -> String {
  format!("{}{}", E2E_TAG_PREFIX, uuid::Uuid::new_v4())
}

/// Sprites ignore region and machine size: the platform sizes every Sprite
/// itself. The request type still carries both fields, so this suite passes
/// fixed placeholders and never asserts on them.
fn create_request(owner_user_id: String) -> CreateInstanceRequest {
  CreateInstanceRequest {
    // Not a real Supabase user id. The raw adapter only uses it to derive
    // the Sprite name, which keeps the e2e tag greppable for cleanup.
    owner_user_id,
    region: RegionCode::UsEast,
    size_preset: SizePreset::Small,
    manifest_version: 1,
    idempotency_key: e2e_tag(),
  }
}

/// Creates one tagged Sprite and its cleanup guard.
async fn create_tagged(
  cfg: &SpritesConfig,
  provider: &SpritesProvider,
) -> (
  treq_lib::core::remote_provider::ProviderInstance,
  InstanceCleanupGuard,
) {
  let instance = provider
    .create_instance(create_request(e2e_tag()))
    .await
    .expect("create_instance should succeed against the real Sprites API");
  let guard = InstanceCleanupGuard::new(cfg, instance.provider_resource_id.clone());
  (instance, guard)
}

/// Polls the adapter until the Sprite is usable. A Sprite that is still
/// being created maps to Provisioning. A new Sprite with no work yet may
/// already report `cold` (mapped to Suspended); that is usable too, because
/// the next command wakes it. Tests that need it running run a command.
async fn wait_until_ready(provider: &SpritesProvider, name: &str) {
  let deadline = Instant::now() + Duration::from_secs(5 * 60);
  loop {
    let snapshot = provider
      .get_instance(name)
      .await
      .expect("get_instance while waiting for readiness");
    match snapshot.state {
      ManagedInstanceState::Ready | ManagedInstanceState::Suspended => return,
      ManagedInstanceState::Failed | ManagedInstanceState::Degraded => {
        panic!("Sprite {name} never became ready: {:?}", snapshot.state)
      }
      _ => {}
    }
    assert!(
      Instant::now() < deadline,
      "Sprite {name} did not become ready within 5 minutes (last state {:?})",
      snapshot.state
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
  }
}

fn http() -> reqwest::Client {
  reqwest::Client::builder()
    .timeout(Duration::from_secs(120))
    .build()
    .expect("http client")
}

fn sprite_url(cfg: &SpritesConfig, name: &str) -> String {
  format!(
    "{}/v1/sprites/{}",
    cfg.base_url.trim_end_matches('/'),
    urlencoding::encode(name)
  )
}

fn request_id(response: &reqwest::Response) -> String {
  response
    .headers()
    .get("fly-request-id")
    .or_else(|| response.headers().get("x-request-id"))
    .and_then(|v| v.to_str().ok())
    .unwrap_or("(none)")
    .to_string()
}

/// Reads the raw vendor record for a Sprite. The adapter maps both `warm`
/// and `running` to Ready, because a warm Sprite resumes in well under a
/// second. The suspend/wake test needs the finer vendor status to prove the
/// Sprite really paused and really resumed.
async fn vendor_sprite(cfg: &SpritesConfig, name: &str) -> Value {
  let response = http()
    .get(sprite_url(cfg, name))
    .bearer_auth(&cfg.api_token)
    .send()
    .await
    .expect("GET sprite");
  let rid = request_id(&response);
  let status = response.status();
  let body: Value = response.json().await.unwrap_or(Value::Null);
  assert!(
    status.is_success(),
    "GET sprite {name} failed: {status} request_id={rid} body={body}"
  );
  body
}

fn vendor_status(sprite: &Value) -> String {
  sprite["status"].as_str().unwrap_or_default().to_string()
}

/// Runs a command in the Sprite through the vendor's non-TTY exec endpoint
/// and returns its raw response body. This is the reachability check the
/// provider itself offers; the managed SSH path is covered separately by
/// `remote_e2e_native.rs`.
async fn exec(cfg: &SpritesConfig, name: &str, argv: &[&str]) -> String {
  let mut query: Vec<(&str, &str)> = argv.iter().map(|arg| ("cmd", *arg)).collect();
  if let Some(first) = argv.first() {
    query.push(("path", first));
  }
  let response = http()
    .post(format!("{}/exec", sprite_url(cfg, name)))
    .bearer_auth(&cfg.api_token)
    .query(&query)
    .send()
    .await
    .expect("POST exec");
  let rid = request_id(&response);
  let status = response.status();
  let body = response.text().await.unwrap_or_default();
  assert!(
    status.is_success(),
    "exec {argv:?} on {name} failed: {status} request_id={rid} body={body}"
  );
  body
}

/// Lists Sprite names that start with `prefix`, following pagination.
async fn list_sprite_names(cfg: &SpritesConfig, prefix: &str) -> Vec<String> {
  let mut names = Vec::new();
  let mut continuation: Option<String> = None;
  loop {
    let mut query = vec![
      ("prefix", prefix.to_string()),
      ("max_results", "500".to_string()),
    ];
    if let Some(token) = &continuation {
      query.push(("continuation_token", token.clone()));
    }
    let response = http()
      .get(format!("{}/v1/sprites", cfg.base_url.trim_end_matches('/')))
      .bearer_auth(&cfg.api_token)
      .query(&query)
      .send()
      .await
      .expect("GET sprites");
    let rid = request_id(&response);
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    assert!(
      status.is_success(),
      "list sprites failed: {status} request_id={rid} body={body}"
    );
    for sprite in body["sprites"].as_array().cloned().unwrap_or_default() {
      if let Some(name) = sprite["name"].as_str() {
        names.push(name.to_string());
      }
    }
    continuation = body["next_continuation_token"]
      .as_str()
      .filter(|_| body["has_more"].as_bool() == Some(true))
      .map(str::to_string);
    if continuation.is_none() {
      return names;
    }
  }
}

// ---------------------------------------------------------------------------
// Local gate tests. These always run and never touch the network.
// ---------------------------------------------------------------------------

#[test]
fn e2e_tag_shape_matches_cleanup_script() {
  let tag = e2e_tag();
  assert!(tag.starts_with(E2E_TAG_PREFIX));
  assert_eq!(tag.len(), E2E_TAG_PREFIX.len() + 36);
  // The adapter derives `dev-treq-<owner>`; the cleanup script only deletes
  // names of exactly this shape, and Sprite names are capped at 63 chars.
  let name = format!("dev-treq-{tag}");
  assert!(name.starts_with(E2E_SPRITE_NAME_PREFIX));
  assert!(name.len() <= 63);
}

#[test]
fn live_suite_is_ignored_without_explicit_opt_in() {
  if std::env::var("TREQ_REMOTE_E2E").as_deref() != Ok("1") {
    eprintln!("[remote-e2e] SKIP live tests: not opted in. Run with {LIVE}");
  }
}

// ---------------------------------------------------------------------------
// Managed setup (acceptance criterion 1): provisioning is idempotent,
// reaches readiness, and the Sprite is reachable.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "live Sprites; TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1"]
async fn provisions_a_sprite_that_becomes_ready_and_runs_commands() {
  let cfg = require_e2e!();
  let provider = SpritesProvider::new(cfg.clone()).expect("failed to build provider");

  let (instance, _cleanup) = create_tagged(&cfg, &provider).await;
  assert!(
    instance
      .provider_resource_id
      .starts_with(E2E_SPRITE_NAME_PREFIX),
    "unexpected Sprite name {}",
    instance.provider_resource_id
  );
  assert!(!matches!(instance.state, ManagedInstanceState::Failed));

  // Every lifecycle call must be traceable in the vendor's logs.
  let request_id = provider
    .last_request_id()
    .expect("the Sprites API should return a request id header");
  assert!(!request_id.is_empty());
  eprintln!("[remote-e2e] create_instance request id: {request_id}");

  wait_until_ready(&provider, &instance.provider_resource_id).await;

  let marker = e2e_tag();
  let out = exec(
    &cfg,
    &instance.provider_resource_id,
    &["echo", marker.as_str()],
  )
  .await;
  assert!(
    out.contains(&marker),
    "exec output should echo the marker; got {out:?}"
  );
}

#[tokio::test]
#[ignore = "live Sprites; TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1"]
async fn repeated_and_concurrent_creates_for_one_owner_resolve_to_one_sprite() {
  let cfg = require_e2e!();
  let provider = SpritesProvider::new(cfg.clone()).expect("failed to build provider");

  // The adapter derives the Sprite name from the owner, so the owner is the
  // idempotency identity. A fresh idempotency key per call models a client
  // that retried after losing the first response.
  let owner = e2e_tag();
  let first = provider
    .create_instance(create_request(owner.clone()))
    .await
    .expect("first create_instance should succeed");
  let _cleanup = InstanceCleanupGuard::new(&cfg, first.provider_resource_id.clone());

  let mut handles = Vec::new();
  for _ in 0..3 {
    let cfg = cfg.clone();
    let request = create_request(owner.clone());
    handles.push(tokio::spawn(async move {
      SpritesProvider::new(cfg)
        .unwrap()
        .create_instance(request)
        .await
    }));
  }
  for handle in handles {
    let outcome = handle.await.expect("task panicked");
    let instance = outcome.unwrap_or_else(|err| {
      panic!(
        "a repeated create for the same owner must resolve to the existing Sprite, got {err:?}"
      )
    });
    assert_eq!(
      instance.provider_resource_id, first.provider_resource_id,
      "a repeated create must return the same Sprite, not a new one"
    );
  }

  let matching = list_sprite_names(&cfg, &first.provider_resource_id).await;
  assert_eq!(
    matching
      .iter()
      .filter(|name| **name == first.provider_resource_id)
      .count(),
    1,
    "exactly one Sprite must exist for the owner, found {matching:?}"
  );
}

// ---------------------------------------------------------------------------
// Normal managed lifecycle recovery (acceptance criterion 10): a paused
// Sprite is woken and reconnected without being recreated.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "live Sprites; TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1"]
async fn idle_paused_sprite_wakes_and_reconnects_without_recreation() {
  let cfg = require_e2e!();
  let provider = SpritesProvider::new(cfg.clone()).expect("failed to build provider");

  let (instance, _cleanup) = create_tagged(&cfg, &provider).await;
  let name = instance.provider_resource_id.clone();
  wait_until_ready(&provider, &name).await;

  // Leave durable state behind so the reconnect step can prove it reached
  // the same Sprite rather than a fresh one with the same name.
  let marker = e2e_tag();
  exec(
    &cfg,
    &name,
    &[
      "sh",
      "-c",
      &format!("printf %s {marker} > \"$HOME/.treq-e2e-marker\""),
    ],
  )
  .await;
  let identity_before = vendor_sprite(&cfg, &name).await;

  // Sprites have no suspend endpoint. They pause on their own once idle, so
  // stop all activity and poll metadata (which is not activity) until the
  // vendor reports `warm` or `cold`.
  let deadline = Instant::now() + idle_pause_timeout();
  let mut paused_status = String::new();
  while Instant::now() < deadline {
    let status = vendor_status(&vendor_sprite(&cfg, &name).await);
    if status == "warm" || status == "cold" {
      paused_status = status;
      break;
    }
    tokio::time::sleep(Duration::from_secs(10)).await;
  }
  assert!(
    !paused_status.is_empty(),
    "Sprite {name} did not pause within {:?} of going idle",
    idle_pause_timeout()
  );
  eprintln!("[remote-e2e] Sprite {name} paused as {paused_status}");

  // Wake through the adapter only. If wake were a no-op (for example a
  // metadata read), the Sprite would stay paused and this check would fail.
  provider
    .wake_instance(&name)
    .await
    .expect("wake_instance on a paused Sprite should succeed");
  // Allow a few seconds for the vendor's status record to catch up. These
  // metadata reads are not activity, so they cannot wake the Sprite
  // themselves and cannot mask a no-op wake.
  let wake_deadline = Instant::now() + Duration::from_secs(20);
  let mut status_after_wake = vendor_status(&vendor_sprite(&cfg, &name).await);
  while status_after_wake != "running" && Instant::now() < wake_deadline {
    tokio::time::sleep(Duration::from_secs(2)).await;
    status_after_wake = vendor_status(&vendor_sprite(&cfg, &name).await);
  }
  assert_eq!(
    status_after_wake, "running",
    "wake_instance must resume a paused Sprite, but it is still {status_after_wake}"
  );
  let after = provider
    .get_instance(&name)
    .await
    .expect("get_instance after wake");
  assert_eq!(after.state, ManagedInstanceState::Ready);

  // Reconnect: same vendor identity, same durable filesystem.
  let identity_after = vendor_sprite(&cfg, &name).await;
  assert_eq!(
    identity_before["id"], identity_after["id"],
    "wake must not recreate the Sprite"
  );
  assert_eq!(identity_before["created_at"], identity_after["created_at"]);
  let read_back = exec(&cfg, &name, &["sh", "-c", "cat \"$HOME/.treq-e2e-marker\""]).await;
  assert!(
    read_back.contains(&marker),
    "the file written before the pause must survive wake; got {read_back:?}"
  );
}

#[tokio::test]
#[ignore = "live Sprites; TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1"]
async fn wake_on_a_running_sprite_is_a_harmless_no_op() {
  let cfg = require_e2e!();
  let provider = SpritesProvider::new(cfg.clone()).expect("failed to build provider");

  let (instance, _cleanup) = create_tagged(&cfg, &provider).await;
  wait_until_ready(&provider, &instance.provider_resource_id).await;
  // Make sure it is running right now, well inside the idle window.
  exec(&cfg, &instance.provider_resource_id, &["true"]).await;

  // Clients call wake before reconnecting without knowing whether the Sprite
  // paused, so waking a running Sprite must succeed and change nothing.
  provider
    .wake_instance(&instance.provider_resource_id)
    .await
    .expect("wake_instance on a running Sprite should succeed");
  let after = provider
    .get_instance(&instance.provider_resource_id)
    .await
    .expect("get_instance after wake");
  assert_eq!(after.provider_resource_id, instance.provider_resource_id);
  assert_eq!(after.state, ManagedInstanceState::Ready);
}

// ---------------------------------------------------------------------------
// Repair: reprovisioning a Sprite repairs it in place. The Sprite's
// filesystem is the user's durable environment, so repair must keep the
// same provider identity (and therefore the same generation) and the same
// files. Host trust across repair is covered in remote_e2e_native.rs.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "live Sprites; TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1"]
async fn reprovision_repairs_the_sprite_in_place() {
  let cfg = require_e2e!();
  let provider = SpritesProvider::new(cfg.clone()).expect("failed to build provider");

  let (original, _cleanup) = create_tagged(&cfg, &provider).await;
  let name = original.provider_resource_id.clone();
  wait_until_ready(&provider, &name).await;
  let marker = e2e_tag();
  exec(
    &cfg,
    &name,
    &[
      "sh",
      "-c",
      &format!("printf %s {marker} > \"$HOME/.treq-e2e-marker\""),
    ],
  )
  .await;
  let identity_before = vendor_sprite(&cfg, &name).await;

  let replaced = provider
    .replace_instance(ReplaceInstanceRequest {
      provider_resource_id: name.clone(),
      region: RegionCode::UsEast,
      size_preset: SizePreset::Small,
      manifest_version: 2,
      idempotency_key: e2e_tag(),
    })
    .await
    .expect("replace_instance should succeed against the real Sprites API");

  // The control plane bumps the generation only when the provider resource
  // id changes. For Sprites it must not change.
  assert_eq!(replaced.provider_resource_id, name);
  assert!(!matches!(replaced.state, ManagedInstanceState::Failed));
  let identity_after = vendor_sprite(&cfg, &name).await;
  assert_eq!(identity_before["id"], identity_after["id"]);
  let read_back = exec(&cfg, &name, &["sh", "-c", "cat \"$HOME/.treq-e2e-marker\""]).await;
  assert!(
    read_back.contains(&marker),
    "repair must keep the Sprite's files; got {read_back:?}"
  );
}

// ---------------------------------------------------------------------------
// Teardown and orphan-resource detection: deleting a Sprite removes it from
// the provider's inventory, and a repeated delete is safe.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "live Sprites; TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1"]
async fn delete_removes_the_sprite_from_provider_inventory() {
  let cfg = require_e2e!();
  let provider = SpritesProvider::new(cfg.clone()).expect("failed to build provider");

  let (instance, mut cleanup) = create_tagged(&cfg, &provider).await;
  let name = instance.provider_resource_id.clone();

  provider
    .delete_instance(&name)
    .await
    .expect("delete_instance should succeed");
  // Already deleted, so the guard must not delete again.
  cleanup.provider_resource_id = None;

  let after_delete = provider.get_instance(&name).await;
  assert!(
    matches!(after_delete, Err(ProviderError::NotFound)),
    "a deleted Sprite must not still be visible to get_instance: {after_delete:?}"
  );
  let listed = list_sprite_names(&cfg, &name).await;
  assert!(
    !listed.contains(&name),
    "a deleted Sprite must not appear in the inventory listing that orphan cleanup scans: {listed:?}"
  );
  provider
    .delete_instance(&name)
    .await
    .expect("deleting an already-deleted Sprite must be idempotent");
}
