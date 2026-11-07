//! The signed-in user's GitHub token, for calling GitHub as the user from
//! the desktop and mobile apps.
//!
//! `integration-proxy`'s `/github/user-token` returns a GitHub App user
//! access token (it refreshes the grant kept in Vault). The token is cached
//! per Supabase user until five minutes before it expires, so most GitHub
//! calls make no proxy call at all.

use std::sync::Arc;

use serde::Deserialize;

use super::supabase::Supabase;

/// A cached token is reused while it has at least this long left.
pub const REUSE_MARGIN_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTokenError {
  /// Not signed in to treq.
  SignedOut,
  /// The GitHub App link is missing or expired; the user must reconnect.
  Reconnect(String),
  /// The proxy could not be reached or answered something else.
  Unavailable(String),
}

impl std::fmt::Display for GithubTokenError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::SignedOut => f.write_str("Sign in to treq to use GitHub"),
      Self::Reconnect(message) | Self::Unavailable(message) => f.write_str(message),
    }
  }
}

#[derive(Debug, Clone, Deserialize)]
struct Issued {
  token: String,
  /// RFC 3339, or null for a token that does not expire.
  expires_at: Option<String>,
}

#[derive(Debug, Clone)]
struct Cached {
  user_id: String,
  token: String,
  /// Unix seconds; `None` never expires.
  expires_at: Option<i64>,
}

pub struct GithubUserTokens {
  supabase: Arc<Supabase>,
  cached: tokio::sync::Mutex<Option<Cached>>,
}

impl GithubUserTokens {
  pub fn new(supabase: Arc<Supabase>) -> Self {
    Self {
      supabase,
      cached: tokio::sync::Mutex::new(None),
    }
  }

  /// A token for the signed-in user at `now` (Unix seconds).
  pub async fn token(&self, now: i64) -> Result<String, GithubTokenError> {
    let user_id = self
      .supabase
      .auth
      .session()
      .map(|session| session.user.id)
      .ok_or(GithubTokenError::SignedOut)?;
    let mut cached = self.cached.lock().await;
    if let Some(hit) = cached.as_ref().filter(|c| {
      c.user_id == user_id && c.expires_at.is_none_or(|at| at - now > REUSE_MARGIN_SECS)
    }) {
      return Ok(hit.token.clone());
    }
    let (status, body) = self
      .supabase
      .invoke_raw(
        "integration-proxy/github/user-token",
        &serde_json::json!({}),
      )
      .await
      .map_err(|e| GithubTokenError::Unavailable(e.friendly()))?;
    let message = || {
      body.get("error").and_then(|e| e.as_str()).map_or_else(
        || format!("integration-proxy answered {status}"),
        String::from,
      )
    };
    if status == 401 && body.get("code").and_then(|c| c.as_str()) == Some("github_reconnect") {
      return Err(GithubTokenError::Reconnect(message()));
    }
    if status != 200 {
      return Err(GithubTokenError::Unavailable(message()));
    }
    let issued: Issued = serde_json::from_value(body)
      .map_err(|e| GithubTokenError::Unavailable(format!("integration-proxy answer: {e}")))?;
    let expires_at = match issued.expires_at.as_deref() {
      None => None,
      Some(at) => Some(
        chrono::DateTime::parse_from_rfc3339(at)
          .map_err(|e| GithubTokenError::Unavailable(format!("bad expires_at {at}: {e}")))?
          .timestamp(),
      ),
    };
    *cached = Some(Cached {
      user_id,
      token: issued.token.clone(),
      expires_at,
    });
    Ok(issued.token)
  }

  /// Forgets the cached token, after GitHub refused it.
  pub async fn forget(&self) {
    *self.cached.lock().await = None;
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;
  use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  const ROUTE: &str = "/functions/v1/integration-proxy/github/user-token";
  // 2027-01-01T08:00:00Z
  const EXPIRY: i64 = 1_798_790_400;

  async fn signed_in(server: &MockServer, user_id: &str) -> Arc<Supabase> {
    Mock::given(method("POST"))
      .and(path("/functions/v1/exchange-desktop-token"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "access_token": format!("jwt-{user_id}"),
        "refresh_token": "r",
        "expires_in": 3_600,
        "user": { "id": user_id },
      })))
      .mount(server)
      .await;
    let (supabase, _) = client(&server.uri());
    supabase.auth.exchange_token("t").await.unwrap();
    Arc::new(supabase)
  }

  fn issued(token: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
      .set_body_json(json!({ "token": token, "expires_at": "2027-01-01T08:00:00Z" }))
  }

  #[tokio::test]
  async fn fetches_once_with_the_session_then_serves_the_cache() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path(ROUTE))
      .and(header("authorization", "Bearer jwt-u1"))
      .respond_with(issued("ghu_1"))
      .expect(1)
      .mount(&server)
      .await;
    let tokens = GithubUserTokens::new(signed_in(&server, "u1").await);
    assert_eq!(tokens.token(EXPIRY - 3_600).await, Ok("ghu_1".into()));
    assert_eq!(tokens.token(EXPIRY - 600).await, Ok("ghu_1".into()));
  }

  #[tokio::test]
  async fn fetches_again_within_five_minutes_of_expiry_or_after_forget() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path(ROUTE))
      .respond_with(issued("ghu_1"))
      .expect(3)
      .mount(&server)
      .await;
    let tokens = GithubUserTokens::new(signed_in(&server, "u1").await);
    tokens.token(EXPIRY - 3_600).await.unwrap();
    tokens.token(EXPIRY - 200).await.unwrap();
    tokens.forget().await;
    tokens.token(EXPIRY - 3_600).await.unwrap();
  }

  #[tokio::test]
  async fn a_token_that_never_expires_is_kept() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path(ROUTE))
      .respond_with(
        ResponseTemplate::new(200).set_body_json(json!({ "token": "ghu_1", "expires_at": null })),
      )
      .expect(1)
      .mount(&server)
      .await;
    let tokens = GithubUserTokens::new(signed_in(&server, "u1").await);
    tokens.token(0).await.unwrap();
    tokens.token(EXPIRY * 2).await.unwrap();
  }

  #[tokio::test]
  async fn says_when_the_user_must_reconnect_or_sign_in() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path(ROUTE))
      .respond_with(ResponseTemplate::new(401).set_body_json(json!({
        "error": "GitHub authorization expired. Reconnect the treq GitHub App from the dashboard.",
        "code": "github_reconnect",
      })))
      .mount(&server)
      .await;
    let tokens = GithubUserTokens::new(signed_in(&server, "u1").await);
    assert_eq!(
      tokens.token(0).await,
      Err(GithubTokenError::Reconnect(
        "GitHub authorization expired. Reconnect the treq GitHub App from the dashboard.".into()
      ))
    );

    let (signed_out, _) = client(&server.uri());
    let tokens = GithubUserTokens::new(Arc::new(signed_out));
    assert_eq!(tokens.token(0).await, Err(GithubTokenError::SignedOut));
  }

  #[tokio::test]
  async fn reports_other_failures_as_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path(ROUTE))
      .respond_with(
        ResponseTemplate::new(502)
          .set_body_json(json!({ "error": "Failed to refresh the GitHub token" })),
      )
      .mount(&server)
      .await;
    let tokens = GithubUserTokens::new(signed_in(&server, "u1").await);
    assert_eq!(
      tokens.token(0).await,
      Err(GithubTokenError::Unavailable(
        "Failed to refresh the GitHub token".into()
      ))
    );
  }
}
