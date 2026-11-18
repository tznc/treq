//! GitHub reads and writes straight from the app, as the signed-in user.
//!
//! The mobile app used to send every GitHub read and change to the connected
//! host, which ran `gh` there. With a GitHub App user token
//! ([`super::github_user_token`]) it now calls GitHub itself over HTTPS
//! ([`super::github_http`]), so the user's own access applies and their
//! actions are theirs. Only the repository's `owner/name` still comes from
//! the host, from its git remote (`GitRemoteInfo`, no `gh`).
//!
//! `None` means "use the host as before": signed out, GitHub not linked or
//! linked token expired, the proxy unreachable, or a host too old to answer
//! `GitRemoteInfo`.

use async_trait::async_trait;
use serde_json::Value;

use super::{
  github_http::{self, GithubHttp},
  github_user_token::GithubTokenError,
  remote::{GithubAction, GithubQuery},
};

#[async_trait]
pub trait DirectDeps: Send + Sync {
  /// The repository's GitHub `owner/name` from the host's git remote:
  /// `Ok(None)` when it has none, `Err` when the host could not say.
  async fn full_name(&self, repo: &str) -> Result<Option<String>, String>;
  async fn token(&self) -> Result<String, GithubTokenError>;
  /// Drops a token GitHub refused.
  async fn forget_token(&self);
  fn client(&self, token: String) -> GithubHttp;
}

pub const NO_GITHUB_REMOTE: &str = "invalid_arguments: this repository has no GitHub remote";

enum Call {
  Read(GithubQuery),
  Write(GithubAction),
}

impl Call {
  async fn run(&self, gh: &GithubHttp, full_name: &str) -> Result<Value, String> {
    match self {
      Self::Read(query) => github_http::github_read(gh, full_name, query.clone()).await,
      Self::Write(action) => github_http::github_write(gh, full_name, action.clone()).await,
    }
  }
}

async fn direct(deps: &dyn DirectDeps, repo: &str, call: Call) -> Option<Result<Value, String>> {
  let token = deps.token().await.ok()?;
  let full_name = match deps.full_name(repo).await {
    Ok(Some(full_name)) => full_name,
    Ok(None) => return Some(Err(NO_GITHUB_REMOTE.to_string())),
    Err(_) => return None,
  };
  let result = call.run(&deps.client(token), &full_name).await;
  match result {
    // The cached token was revoked or rotated: drop it and try once more.
    Err(error) if error.starts_with(github_http::UNAUTHORIZED) => {
      deps.forget_token().await;
      let token = deps.token().await.ok()?;
      Some(call.run(&deps.client(token), &full_name).await)
    }
    result => Some(result),
  }
}

/// A read answered here, or `None` to ask the host.
pub async fn read(
  deps: &dyn DirectDeps,
  repo: &str,
  query: GithubQuery,
) -> Option<Result<Value, String>> {
  direct(deps, repo, Call::Read(query)).await
}

/// A change made here, or `None` to ask the host.
pub async fn write(
  deps: &dyn DirectDeps,
  repo: &str,
  action: GithubAction,
) -> Option<Result<Value, String>> {
  direct(deps, repo, Call::Write(action)).await
}

#[cfg(test)]
mod tests {
  use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
  };

  use serde_json::json;
  use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;

  struct Fake {
    full_name: Result<Option<String>, String>,
    tokens: Mutex<Vec<Result<String, GithubTokenError>>>,
    forgotten: AtomicUsize,
    base: String,
  }

  impl Fake {
    fn new(base: &str, tokens: Vec<Result<String, GithubTokenError>>) -> Self {
      Self {
        full_name: Ok(Some("tznc/treq".into())),
        tokens: Mutex::new(tokens),
        forgotten: AtomicUsize::new(0),
        base: base.into(),
      }
    }
  }

  #[async_trait]
  impl DirectDeps for Fake {
    async fn full_name(&self, _repo: &str) -> Result<Option<String>, String> {
      self.full_name.clone()
    }
    async fn token(&self) -> Result<String, GithubTokenError> {
      let mut tokens = self.tokens.lock().unwrap();
      if tokens.len() > 1 {
        tokens.remove(0)
      } else {
        tokens[0].clone()
      }
    }
    async fn forget_token(&self) {
      self.forgotten.fetch_add(1, Ordering::SeqCst);
    }
    fn client(&self, token: String) -> GithubHttp {
      GithubHttp::with_base(token, self.base.clone())
    }
  }

  fn diff_mock(token: &str, status: u16) -> Mock {
    Mock::given(method("GET"))
      .and(path("/repos/tznc/treq/pulls/7"))
      .and(header("authorization", format!("Bearer {token}").as_str()))
      .respond_with(
        ResponseTemplate::new(status).set_body_string(if status == 200 {
          "diff --git a/x b/x\n"
        } else {
          "{\"message\":\"Bad credentials\"}"
        }),
      )
  }

  #[tokio::test]
  async fn reads_from_github_as_the_user() {
    let server = MockServer::start().await;
    diff_mock("ghu_1", 200).expect(1).mount(&server).await;
    let deps = Fake::new(&server.uri(), vec![Ok("ghu_1".into())]);
    let result = read(&deps, "/repos/app", GithubQuery::PrDiff { number: 7 }).await;
    assert_eq!(result, Some(Ok(json!("diff --git a/x b/x\n"))));
  }

  #[tokio::test]
  async fn retries_once_with_a_fresh_token_after_github_refuses_one() {
    let server = MockServer::start().await;
    diff_mock("ghu_old", 401).expect(1).mount(&server).await;
    diff_mock("ghu_new", 200).expect(1).mount(&server).await;
    let deps = Fake::new(
      &server.uri(),
      vec![Ok("ghu_old".into()), Ok("ghu_new".into())],
    );
    let result = read(&deps, "/repos/app", GithubQuery::PrDiff { number: 7 }).await;
    assert_eq!(result, Some(Ok(json!("diff --git a/x b/x\n"))));
    assert_eq!(deps.forgotten.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn gives_up_after_a_second_refusal() {
    let server = MockServer::start().await;
    diff_mock("ghu_1", 401).expect(2).mount(&server).await;
    let deps = Fake::new(&server.uri(), vec![Ok("ghu_1".into())]);
    let result = read(&deps, "/repos/app", GithubQuery::PrDiff { number: 7 }).await;
    assert!(
      matches!(&result, Some(Err(e)) if e.starts_with(github_http::UNAUTHORIZED)),
      "{result:?}"
    );
  }

  #[tokio::test]
  async fn falls_back_to_the_host_without_a_usable_token() {
    for error in [
      GithubTokenError::SignedOut,
      GithubTokenError::Reconnect("reconnect".into()),
      GithubTokenError::Unavailable("down".into()),
    ] {
      let deps = Fake::new("http://127.0.0.1:1", vec![Err(error)]);
      assert_eq!(
        read(&deps, "/repos/app", GithubQuery::PrDiff { number: 7 }).await,
        None
      );
      assert_eq!(
        write(&deps, "/repos/app", GithubAction::ReopenIssue { number: 1 }).await,
        None
      );
    }
  }

  #[tokio::test]
  async fn falls_back_when_the_host_cannot_name_the_repository() {
    let mut deps = Fake::new("http://127.0.0.1:1", vec![Ok("ghu_1".into())]);
    deps.full_name = Err("unknown command GitRemoteInfo".into());
    assert_eq!(
      read(&deps, "/repos/app", GithubQuery::PrDiff { number: 7 }).await,
      None
    );
  }

  #[tokio::test]
  async fn a_repository_without_a_github_remote_fails_as_on_the_host() {
    let mut deps = Fake::new("http://127.0.0.1:1", vec![Ok("ghu_1".into())]);
    deps.full_name = Ok(None);
    assert_eq!(
      read(&deps, "/repos/app", GithubQuery::PrDiff { number: 7 }).await,
      Some(Err(NO_GITHUB_REMOTE.into()))
    );
  }

  #[tokio::test]
  async fn writes_to_github_as_the_user() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
      .and(path("/repos/tznc/treq/issues/1"))
      .and(header("authorization", "Bearer ghu_1"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "number": 1 })))
      .expect(1)
      .mount(&server)
      .await;
    let deps = Fake::new(&server.uri(), vec![Ok("ghu_1".into())]);
    assert_eq!(
      write(&deps, "/repos/app", GithubAction::ReopenIssue { number: 1 }).await,
      Some(Ok(Value::Null))
    );
  }
}
