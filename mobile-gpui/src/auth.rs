//! Supabase sign-in, ported from the desktop frontend's `authStore`.
//!
//! Sign-in opens `{web}/sign-in?source=desktop` in the browser. The web app
//! returns through `treq://auth/callback?token=<one-time token>`, which is
//! exchanged for a session by the `exchange-desktop-token` Edge Function.
//! Unlike the web client, every refreshed token pair is written back to the
//! `supabase_session` setting, so a restore never holds a rotated-out
//! refresh token.

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
  backend::{self, AppEvent},
  config,
};

const SESSION_SETTING: &str = "supabase_session";
/// Refresh this long before the access token expires (supabase-js uses 90 s).
const REFRESH_MARGIN_SECS: i64 = 90;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct User {
  pub id: String,
  #[serde(default)]
  pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
  pub access_token: String,
  pub refresh_token: String,
  /// Epoch seconds.
  pub expires_at: i64,
  pub user: User,
}

#[derive(Default)]
pub struct AuthState(Mutex<Option<Session>>);

/// The persisted form, shared with the desktop frontend's format.
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

#[derive(Deserialize)]
struct ErrorBody {
  #[serde(default)]
  error: Option<String>,
  #[serde(default)]
  error_description: Option<String>,
  #[serde(default)]
  msg: Option<String>,
}

fn now() -> i64 {
  chrono::Utc::now().timestamp()
}

/// `true` when `expires_at` is close enough that the token must be refreshed.
pub fn needs_refresh(expires_at: i64, now: i64) -> bool {
  expires_at - now <= REFRESH_MARGIN_SECS
}

pub fn sign_in_url() -> String {
  format!("{}/sign-in?source=desktop", config::env().web_url)
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

async fn error_message(response: reqwest::Response, fallback: &str) -> String {
  let status = response.status();
  let body = response.json::<ErrorBody>().await.ok();
  body
    .and_then(|b| b.error_description.or(b.error).or(b.msg))
    .unwrap_or_else(|| format!("{fallback} (HTTP {status})"))
}

impl AuthState {
  pub async fn session(&self) -> Option<Session> {
    self.0.lock().await.clone()
  }

  async fn set(&self, session: Option<Session>) {
    let backend = backend::get();
    let stored = session.as_ref().map(|s| {
      serde_json::to_string(&StoredSession {
        access_token: s.access_token.clone(),
        refresh_token: s.refresh_token.clone(),
      })
      .unwrap_or_default()
    });
    backend.set_setting(SESSION_SETTING, stored.as_deref().unwrap_or(""));
    let email = session
      .as_ref()
      .map(|s| s.user.email.clone().unwrap_or_else(|| s.user.id.clone()));
    *self.0.lock().await = session;
    backend.emit(AppEvent::Auth(email));
  }

  /// Exchanges the one-time token from the sign-in deep link.
  pub async fn exchange_token(&self, token: &str) -> Result<(), String> {
    let env = config::env();
    let response = backend::get()
      .http
      .post(format!(
        "{}/functions/v1/exchange-desktop-token",
        env.supabase.url
      ))
      .bearer_auth(&env.supabase.anon_key)
      .header("apikey", &env.supabase.anon_key)
      .json(&json!({ "token": token }))
      .send()
      .await
      .map_err(|e| format!("Token exchange failed: {e}"))?;
    if !response.status().is_success() {
      return Err(error_message(response, "Token exchange failed").await);
    }
    let tokens = response
      .json::<TokenResponse>()
      .await
      .map_err(|e| format!("Token exchange failed: {e}"))?;
    self.set(Some(tokens.into_session(now()))).await;
    Ok(())
  }

  async fn refresh(refresh_token: &str) -> Result<Session, String> {
    let env = config::env();
    let response = backend::get()
      .http
      .post(format!(
        "{}/auth/v1/token?grant_type=refresh_token",
        env.supabase.url
      ))
      .header("apikey", &env.supabase.anon_key)
      .json(&json!({ "refresh_token": refresh_token }))
      .send()
      .await
      .map_err(|e| format!("Session refresh failed: {e}"))?;
    if !response.status().is_success() {
      return Err(error_message(response, "Session refresh failed").await);
    }
    response
      .json::<TokenResponse>()
      .await
      .map(|tokens| tokens.into_session(now()))
      .map_err(|e| format!("Session refresh failed: {e}"))
  }

  /// Restores the persisted session on launch. Always refreshes, which
  /// both validates the session and rotates the refresh token.
  pub async fn restore(&self) -> Result<(), String> {
    let Some(stored) = backend::get().setting(SESSION_SETTING) else {
      return Ok(());
    };
    let Ok(stored) = serde_json::from_str::<StoredSession>(&stored) else {
      return Ok(());
    };
    match Self::refresh(&stored.refresh_token).await {
      Ok(session) => {
        self.set(Some(session)).await;
        Ok(())
      }
      Err(error) => {
        log::warn!("restoring the Supabase session failed: {error}");
        Err(error)
      }
    }
  }

  /// A valid access token, refreshed first when it is about to expire.
  pub async fn access_token(&self) -> Option<String> {
    let mut guard = self.0.lock().await;
    let session = guard.as_ref()?.clone();
    if !needs_refresh(session.expires_at, now()) {
      return Some(session.access_token);
    }
    drop(guard);
    match Self::refresh(&session.refresh_token).await {
      Ok(fresh) => {
        let token = fresh.access_token.clone();
        self.set(Some(fresh)).await;
        Some(token)
      }
      Err(error) => {
        log::warn!("refreshing the Supabase session failed: {error}");
        guard = self.0.lock().await;
        guard
          .as_ref()
          .filter(|s| s.expires_at > now())
          .map(|s| s.access_token.clone())
      }
    }
  }

  pub async fn sign_out(&self) {
    if let Some(session) = self.session().await {
      let env = config::env();
      let _ = backend::get()
        .http
        .post(format!("{}/auth/v1/logout", env.supabase.url))
        .bearer_auth(&session.access_token)
        .header("apikey", &env.supabase.anon_key)
        .send()
        .await;
    }
    self.set(None).await;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
}
