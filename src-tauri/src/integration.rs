//! GitHub tokens for agents in a cloud sandbox, through the
//! `integration-proxy` Edge Function.
//!
//! Machine setup writes `/etc/treq/integration.json` (placement id, proxy URL
//! and the placement's integration secret). From it the `treq github`
//! commands sign a short-lived request token, ask the proxy for installation
//! tokens for the repositories in hand, and cache them per repository until
//! shortly before they expire. The token format matches
//! `supabase/functions/integration-proxy/lib.ts`, which checks the same test
//! vector.

use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "/etc/treq/integration.json";

/// How long a signed request token lives; the proxy accepts at most 300s.
pub const REQUEST_TOKEN_TTL_SECS: u64 = 240;

/// A cached token is reused while it has at least this long left.
pub const REUSE_MARGIN_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IntegrationConfig {
  pub placement_id: String,
  pub proxy_url: String,
  pub secret: String,
}

/// One installation token from `/github/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoToken {
  pub repositories: Vec<String>,
  pub token: String,
  pub expires_at: String,
}

/// Tokens by lowercase `owner/name`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCache {
  pub tokens: HashMap<String, CachedToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedToken {
  pub token: String,
  /// Unix seconds.
  pub expires_at: i64,
}

pub fn read_config(_path: &Path) -> Result<IntegrationConfig, String> {
  unimplemented!()
}

/// `it1.<placement_id>.<expiry>.<mac>`, `mac` lowercase hex HMAC-SHA256 over
/// `it1.<placement_id>.<expiry>` keyed with the integration secret.
pub fn request_token(_secret: &str, _placement_id: &str, _expiry: u64) -> String {
  unimplemented!()
}

impl TokenCache {
  /// A cached token for `repository` with enough time left at `now`.
  pub fn fresh(&self, _repository: &str, _now: i64) -> Option<&str> {
    unimplemented!()
  }

  /// Records every repository each token covers.
  pub fn insert(&mut self, _tokens: &[RepoToken]) {
    unimplemented!()
  }
}

/// Asks the proxy for tokens for `repositories`, signing with `config`.
pub async fn fetch_tokens(
  _client: &reqwest::Client,
  _config: &IntegrationConfig,
  _repositories: &[String],
  _now: u64,
) -> Result<Vec<RepoToken>, String> {
  unimplemented!()
}

/// The `owner/name` a git credential request (`protocol=…`, `host=…`,
/// `path=…` lines) asks about, when it is for github.com over https.
pub fn credential_repository(_request: &str) -> Option<String> {
  unimplemented!()
}

/// The git credential helper's answer for `token`.
pub fn credential_answer(token: &str) -> String {
  format!("username=x-access-token\npassword={token}\n")
}

/// The repository a `gh` invocation names with `-R`/`--repo`, if any.
pub fn gh_repo_flag(_args: &[String]) -> Option<String> {
  unimplemented!()
}

#[cfg(test)]
mod tests {
  use super::*;

  const SECRET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  const PLACEMENT: &str = "11111111-1111-1111-1111-111111111111";
  // integration-proxy's lib.test.ts checks the same vector.
  const VECTOR: &str = "it1.11111111-1111-1111-1111-111111111111.1800000300.f02f52f8f67b6774a7f2ec2bc93e2211b6f76df76e5be3af643f60b4d0380535";

  #[test]
  fn request_token_matches_the_proxy() {
    assert_eq!(request_token(SECRET, PLACEMENT, 1_800_000_300), VECTOR);
  }

  #[test]
  fn reads_the_config_machine_setup_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("integration.json");
    std::fs::write(
      &path,
      r#"{"placement_id":"pl-1","proxy_url":"https://p.supabase.co/functions/v1/integration-proxy","secret":"s"}"#,
    )
    .unwrap();
    assert_eq!(
      read_config(&path).unwrap(),
      IntegrationConfig {
        placement_id: "pl-1".into(),
        proxy_url: "https://p.supabase.co/functions/v1/integration-proxy".into(),
        secret: "s".into(),
      }
    );
    let missing = read_config(&dir.path().join("none.json")).unwrap_err();
    assert!(missing.contains("not a treq cloud sandbox"), "{missing}");
  }

  #[test]
  fn cache_reuses_a_token_until_five_minutes_before_expiry_for_every_repo_it_covers() {
    let mut cache = TokenCache::default();
    cache.insert(&[RepoToken {
      repositories: vec!["Tznc/Treq".into(), "tznc/biz-tools".into()],
      token: "ghs_1".into(),
      expires_at: "2027-01-01T01:00:00Z".into(),
    }]);
    let expiry = 1_798_765_200; // 2027-01-01T01:00:00Z
    assert_eq!(cache.fresh("tznc/treq", expiry - 600), Some("ghs_1"));
    assert_eq!(cache.fresh("TZNC/BIZ-TOOLS", expiry - 600), Some("ghs_1"));
    assert_eq!(cache.fresh("tznc/treq", expiry - 200), None);
    assert_eq!(cache.fresh("acme/app", expiry - 600), None);
  }

  #[tokio::test]
  async fn fetch_tokens_signs_the_request_and_reads_the_tokens() {
    use wiremock::{
      matchers::{body_json, header, method, path},
      Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    let token = request_token(SECRET, PLACEMENT, 1_000 + REQUEST_TOKEN_TTL_SECS);
    Mock::given(method("POST"))
      .and(path("/functions/v1/integration-proxy/github/token"))
      .and(header("authorization", format!("Bearer {token}").as_str()))
      .and(body_json(serde_json::json!({ "repositories": ["tznc/treq"] })))
      .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "tokens": [{ "repositories": ["tznc/treq"], "token": "ghs_1", "expires_at": "2027-01-01T00:00:00Z" }],
        "errors": [],
      })))
      .expect(1)
      .mount(&server)
      .await;
    let config = IntegrationConfig {
      placement_id: PLACEMENT.into(),
      proxy_url: format!("{}/functions/v1/integration-proxy", server.uri()),
      secret: SECRET.into(),
    };
    let tokens = fetch_tokens(
      &reqwest::Client::new(),
      &config,
      &["tznc/treq".into()],
      1_000,
    )
    .await
    .unwrap();
    assert_eq!(tokens[0].token, "ghs_1");
  }

  #[tokio::test]
  async fn fetch_tokens_reports_why_a_repository_was_refused() {
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
        "tokens": [],
        "errors": [{ "repository": "tznc/treq", "code": "github_permission_denied",
                     "error": "The sandbox owner cannot push to this repository" }],
      })))
      .mount(&server)
      .await;
    let config = IntegrationConfig {
      placement_id: PLACEMENT.into(),
      proxy_url: server.uri(),
      secret: SECRET.into(),
    };
    let err = fetch_tokens(
      &reqwest::Client::new(),
      &config,
      &["tznc/treq".into()],
      1_000,
    )
    .await
    .unwrap_err();
    assert!(err.contains("cannot push to this repository"), "{err}");
  }

  #[test]
  fn credential_requests_name_the_repository() {
    let request = "protocol=https\nhost=github.com\npath=tznc/treq.git\n";
    assert_eq!(credential_repository(request).as_deref(), Some("tznc/treq"));
    assert_eq!(
      credential_repository("protocol=https\nhost=github.com\npath=tznc/treq\n").as_deref(),
      Some("tznc/treq")
    );
    // No path (useHttpPath off), another host, or plain http: not ours.
    assert_eq!(
      credential_repository("protocol=https\nhost=github.com\n"),
      None
    );
    assert_eq!(
      credential_repository("protocol=https\nhost=gitlab.com\npath=a/b\n"),
      None
    );
    assert_eq!(
      credential_repository("protocol=http\nhost=github.com\npath=a/b\n"),
      None
    );
    assert_eq!(
      credential_answer("ghs_1"),
      "username=x-access-token\npassword=ghs_1\n"
    );
  }

  #[test]
  fn gh_repo_flag_reads_both_spellings() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
      gh_repo_flag(&args(&["pr", "list", "-R", "tznc/treq"])).as_deref(),
      Some("tznc/treq")
    );
    assert_eq!(
      gh_repo_flag(&args(&["pr", "list", "--repo", "tznc/treq"])).as_deref(),
      Some("tznc/treq")
    );
    assert_eq!(
      gh_repo_flag(&args(&["pr", "list", "--repo=tznc/treq"])).as_deref(),
      Some("tznc/treq")
    );
    assert_eq!(gh_repo_flag(&args(&["pr", "list"])), None);
  }
}
