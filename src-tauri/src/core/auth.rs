//! The Supabase session: sign-in through the website, restore on launch,
//! refresh and sign-out. Desktop and mobile share it.
//!
//! Sign-in opens `{web}/sign-in?source=desktop` in the browser. The website
//! returns through `treq://auth/callback?token=<one-time token>`, which the
//! `exchange-desktop-token` Edge Function trades for a session. Every
//! refreshed token pair is written back to the `supabase_session` setting,
//! so a restore never presents a rotated-out refresh token.
//!
//! Callers watch the session with [`Auth::subscribe`]: the desktop app hands
//! each new access token to the SSH relay and the Linear proxy, and mirrors
//! the session to its webview.

use crate::result_ext::ResultExt;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::supabase::{self, ApiError, SupabaseEnv};
use crate::lock_ext::LockExt;

/// The setting the session's tokens are kept under, in the frontend's
/// format (`{"accessToken", "refreshToken"}`).
pub const SESSION_SETTING: &str = "supabase_session";
/// Refresh this long before the access token expires (supabase-js uses 90 s).
const REFRESH_MARGIN_SECS: i64 = 90;

/// Where the session's tokens persist.
pub trait SessionStore: Send + Sync {
  fn load(&self) -> Option<String>;
  /// Saves `value`; an empty string clears it.
  fn save(&self, value: &str);
}

/// GoTrue's user, with the fields Treq reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
  pub id: String,
  #[serde(default)]
  pub email: Option<String>,
  #[serde(default)]
  pub user_metadata: Value,
}

impl User {
  fn metadata(&self, keys: &[&str]) -> Option<String> {
    keys
      .iter()
      .find_map(|key| self.user_metadata[key].as_str().filter(|v| !v.is_empty()))
      .map(str::to_string)
  }

  /// What the Account tab calls the user (`full_name`, `name`, or "User").
  pub fn display_name(&self) -> String {
    self
      .metadata(&["full_name", "name"])
      .unwrap_or_else(|| "User".into())
  }

  pub fn avatar_url(&self) -> Option<String> {
    self.metadata(&["avatar_url", "picture"])
  }
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
  pub access_token: String,
  pub refresh_token: String,
  /// Epoch seconds.
  pub expires_at: i64,
  pub user: User,
}

/// Leaves the tokens out, so a logged session cannot leak them.
impl std::fmt::Debug for Session {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Session")
      .field("access_token", &"<redacted>")
      .field("refresh_token", &"<redacted>")
      .field("expires_at", &self.expires_at)
      .field("user", &self.user)
      .finish()
  }
}

/// Everything there is to show about signing in.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
  /// The stored session is still being restored.
  pub loading: bool,
  /// A sign-in token from a `treq://auth/callback` link is being exchanged.
  pub signing_in: bool,
  /// Why the last token exchange failed; cleared when a new one starts.
  pub sign_in_error: Option<String>,
  pub session: Option<Session>,
}

impl Default for AuthStatus {
  fn default() -> Self {
    Self {
      loading: true,
      signing_in: false,
      sign_in_error: None,
      session: None,
    }
  }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSession {
  access_token: String,
  refresh_token: String,
}

#[derive(Deserialize)]
struct TokenResponse {
  access_token: String,
  refresh_token: String,
  #[serde(default)]
  expires_in: Option<i64>,
  #[serde(default)]
  expires_at: Option<i64>,
  user: User,
}

impl TokenResponse {
  fn into_session(self, now: i64) -> Session {
    Session {
      expires_at: self
        .expires_at
        .unwrap_or_else(|| now + self.expires_in.unwrap_or(3600)),
      access_token: self.access_token,
      refresh_token: self.refresh_token,
      user: self.user,
    }
  }
}

fn now() -> i64 {
  chrono::Utc::now().timestamp()
}

/// `true` when `expires_at` is close enough that the token must be refreshed.
pub fn needs_refresh(expires_at: i64, now: i64) -> bool {
  expires_at - now <= REFRESH_MARGIN_SECS
}

/// The website's sign-in page for `web_url`.
pub fn sign_in_url(web_url: &str) -> String {
  format!("{web_url}/sign-in?source=desktop")
}

/// Extracts the one-time token from a `treq://auth/callback?token=...` link.
pub fn callback_token(link: &str) -> Option<String> {
  let url = url::Url::parse(link).ok()?;
  if url.scheme() != "treq" || url.host_str() != Some("auth") || url.path() != "/callback" {
    return None;
  }
  url
    .query_pairs()
    .find(|(key, _)| key == "token")
    .map(|(_, value)| value.into_owned())
    .filter(|token| !token.is_empty())
}

type Listener = Arc<dyn Fn(&AuthStatus) + Send + Sync>;

pub struct Auth {
  url: String,
  anon_key: String,
  http: reqwest::Client,
  store: Arc<dyn SessionStore>,
  status: Mutex<AuthStatus>,
  /// Held across a refresh, so two callers never spend one refresh token.
  refreshing: tokio::sync::Mutex<()>,
  listeners: Mutex<Vec<Listener>>,
}

impl Auth {
  pub fn new(env: &SupabaseEnv, http: reqwest::Client, store: Arc<dyn SessionStore>) -> Self {
    Self {
      url: env.url.trim_end_matches('/').to_string(),
      anon_key: env.anon_key.clone(),
      http,
      store,
      status: Mutex::new(AuthStatus::default()),
      refreshing: tokio::sync::Mutex::new(()),
      listeners: Mutex::new(Vec::new()),
    }
  }

  pub fn status(&self) -> AuthStatus {
    self.status.lock_or_recover().clone()
  }

  pub fn session(&self) -> Option<Session> {
    self.status.lock_or_recover().session.clone()
  }

  /// Calls `listener` with the status after every change.
  pub fn subscribe(&self, listener: impl Fn(&AuthStatus) + Send + Sync + 'static) {
    self.listeners.lock_or_recover().push(Arc::new(listener));
  }

  fn update(&self, f: impl FnOnce(&mut AuthStatus)) {
    let status = {
      let mut status = self.status.lock_or_recover();
      f(&mut status);
      status.clone()
    };
    let listeners = self.listeners.lock_or_recover().clone();
    for listener in listeners {
      listener(&status);
    }
  }

  /// Makes `session` current and persists its tokens.
  fn set_session(&self, session: Option<Session>) {
    let stored = session
      .as_ref()
      .map(|s| {
        serde_json::to_string(&StoredSession {
          access_token: s.access_token.clone(),
          refresh_token: s.refresh_token.clone(),
        })
        .unwrap_or_default()
      })
      .unwrap_or_default();
    self.store.save(&stored);
    self.update(|status| status.session = session);
  }

  async fn token_request(&self, url: String, body: Value) -> Result<Session, ApiError> {
    let response = self
      .http
      .post(url)
      .bearer_auth(&self.anon_key)
      .header("apikey", &self.anon_key)
      .json(&body)
      .send()
      .await
      .map_err(ApiError::transport)?;
    let tokens: TokenResponse = supabase::read(response).await?;
    Ok(tokens.into_session(now()))
  }

  async fn refresh(&self, refresh_token: &str) -> Result<Session, ApiError> {
    self
      .token_request(
        format!("{}/auth/v1/token?grant_type=refresh_token", self.url),
        json!({ "refresh_token": refresh_token }),
      )
      .await
  }

  /// Exchanges the one-time token from the sign-in deep link.
  pub async fn exchange_token(&self, token: &str) -> Result<(), String> {
    self.update(|status| {
      status.signing_in = true;
      status.sign_in_error = None;
    });
    let result = self
      .token_request(
        format!("{}/functions/v1/exchange-desktop-token", self.url),
        json!({ "token": token }),
      )
      .await;
    match result {
      Ok(session) => {
        self.set_session(Some(session));
        self.update(|status| status.signing_in = false);
        Ok(())
      }
      Err(error) => {
        let message = if error.status == 0 {
          format!("Token exchange failed: {}", error.message)
        } else {
          error.message
        };
        self.update(|status| {
          status.signing_in = false;
          status.sign_in_error = Some(message.clone());
        });
        Err(message)
      }
    }
  }

  /// Restores the persisted session on launch. Always refreshes, which both
  /// validates the session and rotates the refresh token. A failed refresh
  /// leaves the stored tokens for the next launch.
  pub async fn restore(&self) -> Result<(), String> {
    let stored = self
      .store
      .load()
      .and_then(|raw| serde_json::from_str::<StoredSession>(&raw).ok());
    let result = match stored {
      None => Ok(()),
      Some(stored) => {
        let _guard = self.refreshing.lock().await;
        match self.refresh(&stored.refresh_token).await {
          Ok(session) => {
            self.set_session(Some(session));
            Ok(())
          }
          Err(error) => {
            log::warn!("restoring the Supabase session failed: {error}");
            Err(error.to_string())
          }
        }
      }
    };
    self.update(|status| status.loading = false);
    result
  }

  /// A valid access token, refreshed first when it is about to expire.
  pub async fn access_token(&self) -> Option<String> {
    let session = self.session()?;
    if !needs_refresh(session.expires_at, now()) {
      return Some(session.access_token);
    }
    let _guard = self.refreshing.lock().await;
    // Another caller may have refreshed while this one waited.
    let session = self.session()?;
    if !needs_refresh(session.expires_at, now()) {
      return Some(session.access_token);
    }
    match self.refresh(&session.refresh_token).await {
      Ok(fresh) => {
        let token = fresh.access_token.clone();
        self.set_session(Some(fresh));
        Some(token)
      }
      Err(error) => {
        log::warn!("refreshing the Supabase session failed: {error}");
        // A refused refresh token ends the session; a network failure keeps
        // it until the access token actually expires.
        if (400..500).contains(&error.status) {
          self.set_session(None);
          return None;
        }
        self
          .session()
          .filter(|s| s.expires_at > now())
          .map(|s| s.access_token)
      }
    }
  }

  /// Ends the session on the server (best effort) and forgets it here.
  pub async fn sign_out(&self) {
    if let Some(session) = self.session() {
      self
        .http
        .post(format!("{}/auth/v1/logout", self.url))
        .bearer_auth(&session.access_token)
        .header("apikey", &self.anon_key)
        .send()
        .await
        .ignore_closed();
    }
    self.set_session(None);
  }
}

#[cfg(test)]
mod tests {
  use wiremock::{
    matchers::{body_json, header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  #[test]
  fn parses_callback_links() {
    assert_eq!(
      callback_token("treq://auth/callback?token=abc").as_deref(),
      Some("abc")
    );
    assert_eq!(callback_token("treq://auth/callback?token="), None);
    assert_eq!(callback_token("treq://auth/other?token=abc"), None);
    assert_eq!(callback_token("https://auth/callback?token=abc"), None);
  }

  #[test]
  fn debug_output_leaves_out_the_tokens() {
    let session = Session {
      access_token: "secret-access".into(),
      refresh_token: "secret-refresh".into(),
      expires_at: 1,
      user: User {
        id: "u".into(),
        email: None,
        user_metadata: Value::Null,
      },
    };
    let debug = format!("{session:?}");
    assert!(!debug.contains("secret-"), "{debug}");
    assert!(debug.contains("expires_at: 1"), "{debug}");
  }

  #[test]
  fn refreshes_inside_the_margin() {
    assert!(needs_refresh(1_000, 1_000 - 30));
    assert!(!needs_refresh(1_000, 1_000 - 600));
  }

  #[test]
  fn computes_expiry_from_expires_in() {
    let tokens: TokenResponse = serde_json::from_value(json!({
        "access_token": "a", "refresh_token": "r", "expires_in": 3600,
        "user": {"id": "u", "email": "e@x"}
    }))
    .unwrap();
    assert_eq!(tokens.into_session(100).expires_at, 3700);
  }

  #[test]
  fn names_the_user_from_metadata() {
    let user: User = serde_json::from_value(json!({
      "id": "u", "user_metadata": {"name": "Ada", "picture": "https://x/a.png"}
    }))
    .unwrap();
    assert_eq!(user.display_name(), "Ada");
    assert_eq!(user.avatar_url().as_deref(), Some("https://x/a.png"));
    let bare: User = serde_json::from_value(json!({"id": "u"})).unwrap();
    assert_eq!(bare.display_name(), "User");
    assert_eq!(bare.avatar_url(), None);
  }

  fn tokens(access: &str, refresh: &str, expires_in: i64) -> Value {
    json!({
      "access_token": access, "refresh_token": refresh, "expires_in": expires_in,
      "user": {"id": "u1", "email": "ada@example.com"}
    })
  }

  async fn refresh_mock(server: &MockServer, from: &str, to: Value) {
    Mock::given(method("POST"))
      .and(path("/auth/v1/token"))
      .and(query_param("grant_type", "refresh_token"))
      .and(body_json(json!({"refresh_token": from})))
      .respond_with(ResponseTemplate::new(200).set_body_json(to))
      .mount(server)
      .await;
  }

  #[tokio::test]
  async fn restore_refreshes_and_persists_the_rotated_token() {
    let server = MockServer::start().await;
    refresh_mock(&server, "r0", tokens("a1", "r1", 3600)).await;
    let (supabase, store) = client(&server.uri());
    store.save(r#"{"accessToken":"a0","refreshToken":"r0"}"#);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    supabase.auth.subscribe(move |status| {
      log.lock().unwrap().push((
        status.loading,
        status.session.as_ref().map(|s| s.access_token.clone()),
      ))
    });

    supabase.auth.restore().await.unwrap();

    assert_eq!(
      store.load().as_deref(),
      Some(r#"{"accessToken":"a1","refreshToken":"r1"}"#)
    );
    let status = supabase.auth.status();
    assert!(!status.loading);
    assert_eq!(
      status.session.unwrap().user.email.as_deref(),
      Some("ada@example.com")
    );
    assert_eq!(
      *seen.lock().unwrap(),
      vec![
        (true, Some("a1".to_string())),
        (false, Some("a1".to_string()))
      ]
    );
  }

  #[tokio::test]
  async fn restore_without_a_stored_session_only_stops_loading() {
    let (supabase, _) = client("http://127.0.0.1:9");
    supabase.auth.restore().await.unwrap();
    assert_eq!(
      supabase.auth.status(),
      AuthStatus {
        loading: false,
        ..AuthStatus::default()
      }
    );
  }

  #[tokio::test]
  async fn a_failed_restore_keeps_the_stored_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/auth/v1/token"))
      .respond_with(
        ResponseTemplate::new(400)
          .set_body_json(json!({"error_description": "Invalid Refresh Token"})),
      )
      .mount(&server)
      .await;
    let (supabase, store) = client(&server.uri());
    let stored = r#"{"accessToken":"a0","refreshToken":"r0"}"#;
    store.save(stored);
    let error = supabase.auth.restore().await.unwrap_err();
    assert!(error.contains("Invalid Refresh Token"));
    assert_eq!(store.load().as_deref(), Some(stored));
    assert_eq!(supabase.auth.session(), None);
  }

  #[tokio::test]
  async fn exchanges_the_deep_link_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/exchange-desktop-token"))
      .and(header("apikey", "anon"))
      .and(body_json(json!({"token": "once"})))
      .respond_with(ResponseTemplate::new(200).set_body_json(tokens("a1", "r1", 3600)))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/exchange-desktop-token"))
      .and(body_json(json!({"token": "stale"})))
      .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "expired"})))
      .mount(&server)
      .await;
    let (supabase, store) = client(&server.uri());

    let error = supabase.auth.exchange_token("stale").await.unwrap_err();
    assert_eq!(error, "expired");
    assert_eq!(
      supabase.auth.status().sign_in_error.as_deref(),
      Some("expired")
    );

    supabase.auth.exchange_token("once").await.unwrap();
    let status = supabase.auth.status();
    assert!(!status.signing_in);
    assert_eq!(status.sign_in_error, None);
    assert_eq!(status.session.unwrap().access_token, "a1");
    assert!(store.load().unwrap().contains("r1"));
  }

  #[tokio::test]
  async fn access_token_refreshes_once_near_expiry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/exchange-desktop-token"))
      .respond_with(ResponseTemplate::new(200).set_body_json(tokens("a1", "r1", 30)))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/auth/v1/token"))
      .and(body_json(json!({"refresh_token": "r1"})))
      .respond_with(ResponseTemplate::new(200).set_body_json(tokens("a2", "r2", 3600)))
      .expect(1)
      .mount(&server)
      .await;
    let (supabase, store) = client(&server.uri());
    supabase.auth.exchange_token("t").await.unwrap();

    let (first, second) = tokio::join!(supabase.auth.access_token(), supabase.auth.access_token());
    assert_eq!(first.as_deref(), Some("a2"));
    assert_eq!(second.as_deref(), Some("a2"));
    assert!(store.load().unwrap().contains("r2"));
  }

  #[tokio::test]
  async fn a_refused_refresh_signs_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/exchange-desktop-token"))
      .respond_with(ResponseTemplate::new(200).set_body_json(tokens("a1", "r1", 30)))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/auth/v1/token"))
      .respond_with(ResponseTemplate::new(400).set_body_json(json!({"msg": "revoked"})))
      .mount(&server)
      .await;
    let (supabase, store) = client(&server.uri());
    supabase.auth.exchange_token("t").await.unwrap();
    assert_eq!(supabase.auth.access_token().await, None);
    assert_eq!(supabase.auth.session(), None);
    assert_eq!(store.load(), None);
  }

  #[tokio::test]
  async fn sign_out_ends_the_server_session_and_clears_the_store() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/exchange-desktop-token"))
      .respond_with(ResponseTemplate::new(200).set_body_json(tokens("a1", "r1", 3600)))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/auth/v1/logout"))
      .and(header("authorization", "Bearer a1"))
      .respond_with(ResponseTemplate::new(204))
      .expect(1)
      .mount(&server)
      .await;
    let (supabase, store) = client(&server.uri());
    supabase.auth.exchange_token("t").await.unwrap();
    supabase.auth.sign_out().await;
    assert_eq!(supabase.auth.session(), None);
    assert_eq!(store.load(), None);
  }
}
