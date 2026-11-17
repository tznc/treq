//! Round 7 measurements of the Supabase client: request and connection
//! counts, and refresh serialization. All
//! `#[ignore]`d; run with
//! `cargo test --lib core::cloud_perf -- --ignored --nocapture --test-threads=1`.

use std::sync::{
  atomic::{AtomicUsize, Ordering},
  Arc, Mutex,
};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use wiremock::{
  matchers::{method, path},
  Mock, MockServer, ResponseTemplate,
};

use super::{
  account,
  cloud::{AccountSnapshot, Cloud, CloudEffects, ManagedEndpointEvent},
  remote::MachineUsageReport,
  remote_control_plane::SshEndpoint,
  remote_local_keys::LocalSshIdentity,
  remote_ssh_transport::CutoffReason,
  supabase::test_support::client,
};

#[derive(Default)]
struct Quiet {
  changed: AtomicUsize,
  tokens: Mutex<Vec<Option<String>>>,
}

#[async_trait]
impl CloudEffects for Quiet {
  fn changed(&self, _: &AccountSnapshot) {
    self.changed.fetch_add(1, Ordering::SeqCst);
  }
  fn access_token(&self, _: &str, token: Option<&str>) {
    self.tokens.lock().unwrap().push(token.map(str::to_string));
  }
  fn managed_endpoint(&self, _: ManagedEndpointEvent) {}
  async fn cut_off(&self, _: &str, _: CutoffReason) {}
  async fn cut_off_managed(&self) {}
  async fn clear_cutoff(&self, _: &str) {}
  fn local_identities(&self) -> Result<Vec<LocalSshIdentity>, String> {
    Ok(Vec::new())
  }
  fn read_public_key(&self, _: &str) -> Result<String, String> {
    Err("none".into())
  }
  async fn machine_usage(&self, _: &SshEndpoint) -> Result<MachineUsageReport, String> {
    Err("none".into())
  }
}

fn token_body(access: &str, expires_in: i64) -> Value {
  json!({
    "access_token": access, "refresh_token": "r1", "expires_in": expires_in,
    "user": {"id": "u1", "email": "ada@example.com"}
  })
}

const STORED: &str = r#"{"accessToken":"a0","refreshToken":"r0"}"#;

/// Forwards TCP to `target` and counts the connections it accepted.
async fn counting_proxy(target: std::net::SocketAddr) -> (String, Arc<AtomicUsize>) {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();
  let count = Arc::new(AtomicUsize::new(0));
  let seen = count.clone();
  tokio::spawn(async move {
    loop {
      let Ok((mut inbound, _)) = listener.accept().await else {
        return;
      };
      seen.fetch_add(1, Ordering::SeqCst);
      tokio::spawn(async move {
        if let Ok(mut outbound) = TcpStream::connect(target).await {
          let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
          let _ = outbound.shutdown().await;
        }
      });
    }
  });
  (format!("http://{addr}"), count)
}

async fn summarize(server: &MockServer) -> Vec<String> {
  server
    .received_requests()
    .await
    .unwrap()
    .iter()
    .map(|r| format!("{} {}", r.method, r.url.path()))
    .collect()
}

/// Startup: restore (refresh) + subscription read; then window focus events.
#[tokio::test]
#[ignore]
async fn r7_startup_and_focus_request_counts() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/auth/v1/token"))
    .respond_with(ResponseTemplate::new(200).set_body_json(token_body("a1", 3600)))
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path("/rest/v1/subscriptions"))
    .respond_with(
      ResponseTemplate::new(200)
        .set_body_json(json!([{"status": "active", "plan": "pro", "current_period_end": null}])),
    )
    .mount(&server)
    .await;
  let (proxy, connections) = counting_proxy(*server.address()).await;
  let (supabase, store) = client(&proxy);
  *store.0.lock().unwrap() = Some(STORED.into());
  let fx = Arc::new(Quiet::default());
  let cloud = Cloud::new(Arc::new(supabase), fx.clone());

  let t = Instant::now();
  cloud.start().await;
  println!(
    "start(): {:?}, requests {:?}, tcp connections {}",
    t.elapsed(),
    summarize(&server).await,
    connections.load(Ordering::SeqCst)
  );

  // `commands::cloud::window_focused` spawns exactly this on every Focused(true).
  for _ in 0..10 {
    cloud.refresh_subscription().await;
  }
  let all = summarize(&server).await;
  println!(
    "after 10 focus events: {} requests, {} tcp connections, effects.changed called {} times",
    all.len(),
    connections.load(Ordering::SeqCst),
    fx.changed.load(Ordering::SeqCst)
  );

  // Four concurrent reads, as parallel fetches would issue them: HTTP/1.1
  // opens one connection each.
  let before = connections.load(Ordering::SeqCst);
  let reads = (0..4).map(|_| account::subscription(&cloud.supabase));
  let _ = futures::future::join_all(reads).await;
  println!(
    "4 concurrent selects: {} new tcp connections",
    connections.load(Ordering::SeqCst) - before
  );
}

/// A slow refresh holds every Supabase call behind it.
#[tokio::test]
#[ignore]
async fn r7_refresh_lock_holds_every_caller() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/auth/v1/token"))
    .respond_with(
      ResponseTemplate::new(200)
        .set_body_json(token_body("a1", 3600))
        .set_delay(Duration::from_millis(1500)),
    )
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path("/rest/v1/subscriptions"))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
    .mount(&server)
    .await;
  let (supabase, store) = client(&server.uri());
  // restore() makes a session that expires at once, so the next calls refresh.
  *store.0.lock().unwrap() = Some(STORED.into());
  Mock::given(method("POST"))
    .and(path("/functions/v1/exchange-desktop-token"))
    .respond_with(ResponseTemplate::new(200).set_body_json(token_body("a0", 30)))
    .mount(&server)
    .await;
  supabase.auth.exchange_token("t").await.unwrap();
  let t = Instant::now();
  let reads = (0..4).map(|i| {
    let supabase = &supabase;
    async move {
      let _ = account::subscription(supabase).await;
      (i, t.elapsed())
    }
  });
  let done = futures::future::join_all(reads).await;
  println!("4 callers behind a 1.5 s refresh: {done:?}");
  println!("requests: {:?}", summarize(&server).await);
}

/// Refresh that never answers: callers wait on the 160 s client timeout.
#[tokio::test]
#[ignore]
async fn r7_hung_refresh_blocks_callers() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/functions/v1/exchange-desktop-token"))
    .respond_with(ResponseTemplate::new(200).set_body_json(token_body("a0", 30)))
    .mount(&server)
    .await;
  Mock::given(method("POST"))
    .and(path("/auth/v1/token"))
    .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3600)))
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path("/rest/v1/subscriptions"))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
    .mount(&server)
    .await;
  let (supabase, _) = client(&server.uri());
  supabase.auth.exchange_token("t").await.unwrap();
  let t = Instant::now();
  let waits = (0..3).map(|i| {
    let supabase = &supabase;
    async move {
      let r = tokio::time::timeout(Duration::from_secs(5), account::subscription(supabase)).await;
      (i, r.is_ok(), t.elapsed())
    }
  });
  let done = futures::future::join_all(waits).await;
  println!("3 Supabase calls behind a refresh that never answers (5 s cap here; the client cap is 160 s): {done:?}");
  println!(
    "requests that reached the server: {:?}",
    summarize(&server).await
  );
}
