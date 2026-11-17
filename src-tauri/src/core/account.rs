//! What the signed-in account reads from Supabase: its subscription, the
//! GitHub repositories the Treq app can see, and Linear OAuth
//! (`subscriptions`, `github_repositories` and the
//! `create-linear-oauth-intent` function).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::supabase::{ApiError, Supabase};

/// A row of `public.subscriptions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
  /// `canceled` means set to cancel at the period end, still Pro until then.
  pub status: String,
  pub plan: String,
  #[serde(default)]
  pub current_period_end: Option<String>,
}

impl Subscription {
  /// `isProSubscription`: statuses that still carry Pro.
  pub fn is_pro(&self) -> bool {
    self.plan == "pro"
      && ["trialing", "active", "past_due", "canceled"].contains(&self.status.as_str())
  }
}

/// The account's subscription; `None` without a row.
pub async fn subscription(supabase: &Supabase) -> Result<Option<Subscription>, ApiError> {
  supabase.select_single("subscriptions", "select=*").await
}

/// A row of `public.github_repositories`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GithubRepository {
  pub id: i64,
  pub full_name: String,
  pub private: bool,
}

pub async fn github_repositories(supabase: &Supabase) -> Result<Vec<GithubRepository>, ApiError> {
  supabase
    .select(
      "github_repositories",
      "select=id,full_name,private,default_branch,installation_id",
    )
    .await
}

/// Where Linear's OAuth page for this account is.
pub async fn linear_oauth_url(supabase: &Supabase) -> Result<String, ApiError> {
  let data: Value = supabase
    .invoke("create-linear-oauth-intent", &json!({}))
    .await?;
  data["authorization_url"]
    .as_str()
    .filter(|url| !url.is_empty())
    .map(str::to_string)
    .ok_or_else(|| ApiError::new("no_url", "No authorization URL returned"))
}

#[cfg(test)]
mod tests {
  use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  #[test]
  fn pro_covers_every_paid_status() {
    let sub = |plan: &str, status: &str| Subscription {
      plan: plan.into(),
      status: status.into(),
      current_period_end: None,
    };
    assert!(sub("pro", "canceled").is_pro());
    assert!(!sub("pro", "inactive").is_pro());
    assert!(!sub("free", "active").is_pro());
  }

  #[tokio::test]
  async fn reads_the_linear_oauth_url_and_github_repositories() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/create-linear-oauth-intent"))
      .respond_with(
        ResponseTemplate::new(200)
          .set_body_json(json!({"authorization_url": "https://linear.app/oauth"})),
      )
      .mount(&server)
      .await;
    Mock::given(method("GET"))
      .and(path("/rest/v1/github_repositories"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"id": 7, "full_name": "o/r", "private": true, "default_branch": "main", "installation_id": 1}
      ])))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    assert_eq!(
      linear_oauth_url(&supabase).await.unwrap(),
      "https://linear.app/oauth"
    );
    assert_eq!(
      github_repositories(&supabase).await.unwrap(),
      vec![GithubRepository {
        id: 7,
        full_name: "o/r".into(),
        private: true
      }]
    );
  }
}
