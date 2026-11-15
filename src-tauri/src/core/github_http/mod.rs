//! GitHub over HTTPS with the signed-in user's token, for a client that has
//! no `gh` to run. Each read returns the same JSON as the host's
//! [`crate::core::remote::github_read`], which goes through `gh`, so a
//! screen cannot tell which one answered.

use crate::core::remote::{GithubContext, GithubQuery};
use crate::github::{self, GhIssue, GhListPage, GhPullRequest, GH_LIST_PAGE_SIZE};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.github.com";
const JSON_MEDIA_TYPE: &str = "application/vnd.github+json";

/// How an error from a rejected token starts, so the caller can drop the
/// token it cached and try once more with a fresh one.
pub const UNAUTHORIZED: &str = "github_unauthorized";

/// A GitHub API client authenticated as one user.
pub struct GithubHttp {
  client: reqwest::Client,
  token: String,
  api_base: String,
}

impl GithubHttp {
  /// A client for `api.github.com`.
  pub fn new(token: String) -> Self {
    Self::with_base(token, API_BASE.to_string())
  }

  /// A client for the API at `api_base`, such as a test server.
  pub fn with_base(token: String, api_base: String) -> Self {
    Self {
      client: reqwest::Client::new(),
      token,
      api_base: api_base.trim_end_matches('/').to_string(),
    }
  }

  fn request(&self, method: Method, path: &str, accept: &str) -> reqwest::RequestBuilder {
    self
      .client
      .request(method, format!("{}{path}", self.api_base))
      .bearer_auth(&self.token)
      .header(reqwest::header::ACCEPT, accept)
      .header("X-GitHub-Api-Version", "2022-11-28")
      .header(reqwest::header::USER_AGENT, "treq")
  }

  /// The response's body, or GitHub's message when it refused; a 401 reads
  /// as [`UNAUTHORIZED`].
  async fn send(&self, request: reqwest::RequestBuilder) -> Result<String, String> {
    let response = request
      .send()
      .await
      .map_err(|error| format!("GitHub request failed: {error}"))?;
    let status = response.status();
    let text = response
      .text()
      .await
      .map_err(|error| format!("GitHub response failed: {error}"))?;
    if status.is_success() {
      return Ok(text);
    }
    let message = serde_json::from_str::<Value>(&text)
      .ok()
      .and_then(|body| body.get("message")?.as_str().map(str::to_string))
      .unwrap_or(text);
    if status == reqwest::StatusCode::UNAUTHORIZED {
      return Err(format!("{UNAUTHORIZED}: {message}"));
    }
    Err(format!("GitHub returned {status}: {message}"))
  }

  /// A REST call with an optional JSON body; an empty response is `null`.
  pub async fn rest(
    &self,
    method: Method,
    path: &str,
    body: Option<&Value>,
  ) -> Result<Value, String> {
    let mut request = self.request(method, path, JSON_MEDIA_TYPE);
    if let Some(body) = body {
      request = request.json(body);
    }
    let text = self.send(request).await?;
    if text.trim().is_empty() {
      return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|error| format!("Failed to parse GitHub response: {error}"))
  }

  /// A REST `GET` read as text in the media type `accept`.
  pub async fn rest_text(&self, path: &str, accept: &str) -> Result<String, String> {
    self.send(self.request(Method::GET, path, accept)).await
  }

  /// A GraphQL call's whole response; its `errors`, if any, as the error.
  pub async fn graphql(&self, query: &str, variables: Value) -> Result<Value, String> {
    let body = json!({ "query": query, "variables": variables });
    let response = self.rest(Method::POST, "/graphql", Some(&body)).await?;
    match response.get("errors").and_then(Value::as_array) {
      Some(errors) if !errors.is_empty() => Err(
        errors
          .iter()
          .filter_map(|error| error.get("message").and_then(Value::as_str))
          .collect::<Vec<_>>()
          .join("; "),
      ),
      _ => Ok(response),
    }
  }
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, String> {
  serde_json::from_value(value).map_err(|error| format!("Failed to parse GitHub response: {error}"))
}

fn to_json<T: serde::Serialize>(value: T) -> Result<Value, String> {
  serde_json::to_value(value).map_err(|error| error.to_string())
}

/// `full_name`'s owner and name.
fn owner_and_name(full_name: &str) -> Result<(&str, &str), String> {
  full_name
    .split_once('/')
    .filter(|(owner, name)| !owner.is_empty() && !name.is_empty() && !name.contains('/'))
    .ok_or_else(|| format!("Expected an OWNER/REPO name, got: {full_name}"))
}

/// [`GithubQuery`] answered over HTTPS for the repository `full_name`
/// (`owner/name`), as the host's `github_read` answers it.
pub async fn github_read(
  gh: &GithubHttp,
  full_name: &str,
  query: GithubQuery,
) -> Result<Value, String> {
  match query {
    GithubQuery::Context => {
      let user = gh.rest(Method::GET, "/user", None).await?;
      let login = user["login"]
        .as_str()
        .ok_or("GitHub sent no login for the signed-in user")?;
      to_json(GithubContext {
        full_name: full_name.to_string(),
        login: login.to_string(),
      })
    }
    GithubQuery::ListIssues { state, after } => {
      to_json(list_issues(gh, full_name, &state, after.as_deref()).await?)
    }
    GithubQuery::ListPrs { state, page } => to_json(list_prs(gh, full_name, &state, page).await?),
    GithubQuery::SearchOpenPrs { mine, query, limit } => {
      to_json(search_open_prs(gh, full_name, mine, &query, limit).await?)
    }
    // Answered by the host's `gh` until it is read over HTTPS here.
    other => Err(format!(
      "unsupported_query: {other:?} is not read over HTTPS yet"
    )),
  }
}

/// `gh_list_issues_impl`: one page of [`github::issues_page_query`].
async fn list_issues(
  gh: &GithubHttp,
  full_name: &str,
  state: &str,
  after: Option<&str>,
) -> Result<GhListPage<GhIssue>, String> {
  let (owner, name) = owner_and_name(full_name)?;
  let query = github::issues_page_query(state)?;
  let variables = json!({
    "owner": owner,
    "name": name,
    "first": GH_LIST_PAGE_SIZE.clamp(1, 100),
    "after": after,
  });
  let response = gh.graphql(&query, variables).await?;
  Ok(parse::<github::IssuesPageResponse>(response)?.into_page())
}

/// The fields `gh pr list --json ...` reads, as GraphQL.
const LISTED_PR_FIELDS: &str = "number title state url author { login } \
  labels(first: 100) { nodes { name color } } headRefName baseRefName \
  mergeStateStatus createdAt updatedAt isDraft";

/// Search terms in one string, with single spaces.
fn search_terms(terms: &[&str]) -> String {
  terms
    .iter()
    .flat_map(|term| term.split_whitespace())
    .collect::<Vec<_>>()
    .join(" ")
}

/// Pull requests matching the search `q`, in its order, up to `want`.
async fn search_prs(gh: &GithubHttp, q: &str, want: u32) -> Result<Vec<GhPullRequest>, String> {
  let query = format!(
    "query($q: String!, $first: Int!, $after: String) {{ \
       search(query: $q, type: ISSUE, first: $first, after: $after) {{ \
         nodes {{ ... on PullRequest {{ {LISTED_PR_FIELDS} }} }} \
         pageInfo {{ hasNextPage endCursor }} }} }}"
  );
  let want = want as usize;
  let mut prs = Vec::new();
  let mut after: Option<String> = None;
  while prs.len() < want {
    let first = (want - prs.len()).min(100);
    let response = gh
      .graphql(&query, json!({ "q": q, "first": first, "after": after }))
      .await?;
    let search = &response["data"]["search"];
    for node in search["nodes"].as_array().into_iter().flatten() {
      // An issue among the results comes back as an empty node.
      if node.get("number").is_some() {
        prs.push(parse(gh_shape(node.clone()))?);
      }
    }
    let page = &search["pageInfo"];
    match page["endCursor"].as_str() {
      Some(cursor) if page["hasNextPage"].as_bool() == Some(true) => {
        after = Some(cursor.to_string());
      }
      _ => break,
    }
  }
  prs.truncate(want);
  Ok(prs)
}

/// `gh_list_prs_impl`: `gh pr list --state` searches GitHub the same way.
async fn list_prs(
  gh: &GithubHttp,
  full_name: &str,
  state: &str,
  page: u32,
) -> Result<GhListPage<GhPullRequest>, String> {
  let filter = match state {
    // `--state open` includes drafts, which have their own filter.
    "open" => "is:open draft:false",
    "draft" => "is:open draft:true",
    "closed" => "is:closed",
    "merged" => "is:merged",
    "all" => "",
    other => return Err(format!("Unknown pull request state: {other}")),
  };
  let repo = format!("repo:{full_name}");
  let q = search_terms(&[&repo, "is:pr", filter, "sort:created-desc"]);
  let limit = GH_LIST_PAGE_SIZE.max(1);
  let page = page.max(1);
  let fetched = search_prs(gh, &q, limit * page).await?;
  let mut list = github::take_list_page(fetched, limit, page);
  if page == 1 {
    // As on the host: the search API's count, closed meaning unmerged.
    let count_filter = match state {
      "closed" => "is:closed is:unmerged",
      _ => filter,
    };
    list.total_count = count_prs(gh, &search_terms(&[&repo, "is:pr", count_filter])).await;
  }
  Ok(list)
}

/// How many pull requests the search `q` matches; `None` when GitHub
/// cannot say, which the host also reads as no total.
async fn count_prs(gh: &GithubHttp, q: &str) -> Option<u64> {
  let query = "query($q: String!) { search(query: $q, type: ISSUE, first: 0) { issueCount } }";
  let response = gh.graphql(query, json!({ "q": q })).await.ok()?;
  response["data"]["search"]["issueCount"].as_u64()
}

/// `gh_search_open_prs_impl`.
async fn search_open_prs(
  gh: &GithubHttp,
  full_name: &str,
  mine: bool,
  query: &str,
  limit: u32,
) -> Result<Vec<GhPullRequest>, String> {
  let repo = format!("repo:{full_name}");
  let author = if mine { "author:@me" } else { "" };
  let q = search_terms(&[
    &repo,
    "is:pr",
    "is:open",
    query,
    "sort:created-desc",
    author,
  ]);
  search_prs(gh, &q, limit.max(1)).await
}

/// A GraphQL node in the shape `gh ... --json` prints it: connections as
/// plain lists, a deleted account as an empty login, and a missing review
/// decision or submission time as an empty string.
fn gh_shape(mut node: Value) -> Value {
  fn fix_author(node: &mut Value) {
    if let Some(author) = node.get_mut("author").filter(|author| author.is_null()) {
      *author = json!({ "login": "" });
    }
  }
  let Some(object) = node.as_object_mut() else {
    return node;
  };
  for key in ["labels", "comments", "latestReviews", "reviews"] {
    if let Some(field) = object.get_mut(key) {
      if let Some(nodes) = field.get_mut("nodes").map(Value::take) {
        *field = nodes;
      }
      for item in field.as_array_mut().into_iter().flatten() {
        fix_author(item);
        if let Some(at) = item.get_mut("submittedAt").filter(|at| at.is_null()) {
          *at = json!("");
        }
      }
    }
  }
  if let Some(decision) = object.get_mut("reviewDecision").filter(|d| d.is_null()) {
    *decision = json!("");
  }
  fix_author(&mut node);
  node
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;
  use wiremock::matchers::{header, method, path};
  use wiremock::{Mock, MockServer, ResponseTemplate};

  async fn start() -> (MockServer, GithubHttp) {
    let server = MockServer::start().await;
    let gh = GithubHttp::with_base("tok".into(), server.uri());
    (server, gh)
  }

  fn data(value: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "data": value }))
  }

  /// What the host returns for `gh_json`, the output of `gh ... --json`:
  /// parsed into the host's type `T` and serialized again.
  fn via_gh<T: serde::de::DeserializeOwned + serde::Serialize>(gh_json: Value) -> Value {
    serde_json::to_value(serde_json::from_value::<T>(gh_json).unwrap()).unwrap()
  }

  /// A `gh pr list --json ...` entry.
  fn gh_listed_pr(number: u64) -> Value {
    json!({
      "author": { "id": "U_1", "is_bot": false, "login": "octo", "name": "Octo" },
      "baseRefName": "main",
      "createdAt": "2026-01-01T00:00:00Z",
      "headRefName": format!("fix-{number}"),
      "isDraft": false,
      "labels": [{ "color": "d73a4a", "description": "", "id": "LA_1", "name": "bug" }],
      "mergeStateStatus": "CLEAN",
      "number": number,
      "state": "OPEN",
      "title": format!("Fix {number}"),
      "updatedAt": "2026-01-02T00:00:00Z",
      "url": format!("https://github.com/o/r/pull/{number}"),
    })
  }

  /// The same pull request as a GraphQL `search` node.
  fn gql_listed_pr(number: u64) -> Value {
    json!({
      "number": number,
      "title": format!("Fix {number}"),
      "state": "OPEN",
      "url": format!("https://github.com/o/r/pull/{number}"),
      "author": { "login": "octo" },
      "labels": { "nodes": [{ "name": "bug", "color": "d73a4a" }] },
      "headRefName": format!("fix-{number}"),
      "baseRefName": "main",
      "mergeStateStatus": "CLEAN",
      "createdAt": "2026-01-01T00:00:00Z",
      "updatedAt": "2026-01-02T00:00:00Z",
      "isDraft": false,
    })
  }

  fn search_page(nodes: Vec<Value>, next: Option<&str>) -> ResponseTemplate {
    data(json!({ "search": {
      "nodes": nodes,
      "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
    }}))
  }

  #[tokio::test]
  async fn context_reads_the_login_with_the_users_token() {
    let (server, gh) = start().await;
    Mock::given(method("GET"))
      .and(path("/user"))
      .and(header("authorization", "Bearer tok"))
      .and(header("accept", "application/vnd.github+json"))
      .and(header("x-github-api-version", "2022-11-28"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "login": "octo", "id": 1 })))
      .expect(1)
      .mount(&server)
      .await;
    let value = github_read(&gh, "o/r", GithubQuery::Context).await.unwrap();
    assert_eq!(value, json!({ "full_name": "o/r", "login": "octo" }));
  }

  #[tokio::test]
  async fn a_rejected_token_is_reported_as_unauthorized() {
    let (server, gh) = start().await;
    Mock::given(path("/user"))
      .respond_with(
        ResponseTemplate::new(401).set_body_json(json!({ "message": "Bad credentials" })),
      )
      .mount(&server)
      .await;
    let error = github_read(&gh, "o/r", GithubQuery::Context)
      .await
      .unwrap_err();
    assert!(error.starts_with("github_unauthorized:"), "{error}");
  }

  #[tokio::test]
  async fn list_issues_reads_one_cursor_page() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_string_contains("states: [OPEN]"))
      .and(body_partial_json(json!({
        "variables": { "owner": "o", "name": "r", "first": 30, "after": "c1" }
      })))
      .respond_with(data(json!({ "repository": { "issues": {
        "nodes": [
          {
            "number": 2, "title": "Crash", "state": "OPEN",
            "url": "https://github.com/o/r/issues/2",
            "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
            "author": { "login": "octo" },
            "labels": { "nodes": [{ "name": "bug", "color": "d73a4a" }] },
          },
          {
            "number": 1, "title": "Old", "state": "OPEN",
            "url": "https://github.com/o/r/issues/1",
            "createdAt": "2025-01-01T00:00:00Z", "updatedAt": "2025-01-02T00:00:00Z",
            "author": null,
            "labels": { "nodes": [] },
          },
        ],
        "pageInfo": { "hasNextPage": true, "endCursor": "c2" },
        "totalCount": 41,
      }}})))
      .expect(1)
      .mount(&server)
      .await;
    let value = github_read(
      &gh,
      "o/r",
      GithubQuery::ListIssues {
        state: "open".into(),
        after: Some("c1".into()),
      },
    )
    .await
    .unwrap();
    assert_eq!(
      value,
      json!({
        "items": [
          {
            "number": 2, "title": "Crash", "state": "OPEN",
            "url": "https://github.com/o/r/issues/2", "body": null,
            "author": { "login": "octo", "avatar_url": null },
            "labels": [{ "name": "bug", "color": "d73a4a" }],
            "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-02T00:00:00Z",
            "comments": null,
          },
          {
            "number": 1, "title": "Old", "state": "OPEN",
            "url": "https://github.com/o/r/issues/1", "body": null,
            "author": { "login": "ghost", "avatar_url": null },
            "labels": [],
            "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-02T00:00:00Z",
            "comments": null,
          },
        ],
        "hasMore": true,
        "endCursor": "c2",
        "totalCount": 41,
      })
    );
  }

  #[tokio::test]
  async fn list_issues_rejects_an_unknown_state_without_a_request() {
    let (server, gh) = start().await;
    let query = GithubQuery::ListIssues {
      state: "weird".into(),
      after: None,
    };
    let error = github_read(&gh, "o/r", query).await.unwrap_err();
    assert_eq!(error, "Unknown issue state: weird");
    assert!(server.received_requests().await.unwrap().is_empty());
  }

  #[tokio::test]
  async fn list_prs_searches_open_non_draft_prs_like_gh() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "q": "repo:o/r is:pr is:open draft:false sort:created-desc",
        "first": 30,
      }})))
      .respond_with(search_page(vec![gql_listed_pr(5)], None))
      .expect(1)
      .mount(&server)
      .await;
    // `gh pr list` gives no total, so the host counts the filter with the
    // search API on the first page.
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_string_contains("issueCount"))
      .and(body_partial_json(json!({ "variables": {
        "q": "repo:o/r is:pr is:open draft:false",
      }})))
      .respond_with(data(json!({ "search": { "issueCount": 7 } })))
      .expect(1)
      .mount(&server)
      .await;
    let query = GithubQuery::ListPrs {
      state: "open".into(),
      page: 1,
    };
    let value = github_read(&gh, "o/r", query).await.unwrap();
    let expected = via_gh::<Vec<GhPullRequest>>(json!([gh_listed_pr(5)]));
    assert_eq!(
      value,
      json!({ "items": expected, "hasMore": false, "endCursor": null, "totalCount": 7 })
    );
  }

  #[tokio::test]
  async fn list_prs_counts_closed_prs_without_the_merged_ones_like_gh() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "q": "repo:o/r is:pr is:closed sort:created-desc",
      }})))
      .respond_with(search_page(vec![], None))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_string_contains("issueCount"))
      .and(body_partial_json(json!({ "variables": {
        "q": "repo:o/r is:pr is:closed is:unmerged",
      }})))
      .respond_with(data(json!({ "search": { "issueCount": 3 } })))
      .expect(1)
      .mount(&server)
      .await;
    let query = GithubQuery::ListPrs {
      state: "closed".into(),
      page: 1,
    };
    let value = github_read(&gh, "o/r", query).await.unwrap();
    assert_eq!(value["totalCount"], 3);
  }

  #[tokio::test]
  async fn list_prs_later_page_fetches_up_to_it_and_slices() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "q": "repo:o/r is:pr is:open draft:true sort:created-desc",
        "first": 60,
        "after": null,
      }})))
      .respond_with(search_page(
        (1..=60).map(gql_listed_pr).collect(),
        Some("c60"),
      ))
      .expect(1)
      .mount(&server)
      .await;
    let query = GithubQuery::ListPrs {
      state: "draft".into(),
      page: 2,
    };
    let value = github_read(&gh, "o/r", query).await.unwrap();
    let fetched: Vec<GhPullRequest> =
      serde_json::from_value(json!((1..=60).map(gh_listed_pr).collect::<Vec<_>>())).unwrap();
    let expected = crate::github::take_list_page(fetched, 30, 2);
    assert_eq!(value, serde_json::to_value(expected).unwrap());
    assert_eq!(value["items"][0]["number"], 31);
    assert_eq!(value["hasMore"], true);
  }

  #[tokio::test]
  async fn list_prs_pages_through_search_results_past_one_hundred() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "q": "repo:o/r is:pr is:merged sort:created-desc",
        "first": 100,
        "after": null,
      }})))
      .respond_with(search_page(
        (1..=100).map(gql_listed_pr).collect(),
        Some("c100"),
      ))
      .expect(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "first": 20,
        "after": "c100",
      }})))
      .respond_with(search_page((101..=105).map(gql_listed_pr).collect(), None))
      .expect(1)
      .mount(&server)
      .await;
    let query = GithubQuery::ListPrs {
      state: "merged".into(),
      page: 4,
    };
    let value = github_read(&gh, "o/r", query).await.unwrap();
    let page: GhListPage<GhPullRequest> = serde_json::from_value(value).unwrap();
    let numbers: Vec<u64> = page.items.iter().map(|pr| pr.number).collect();
    assert_eq!(numbers, (91..=105).collect::<Vec<_>>());
    assert!(!page.has_more);
  }

  #[tokio::test]
  async fn list_prs_rejects_an_unknown_state_without_a_request() {
    let (server, gh) = start().await;
    let query = GithubQuery::ListPrs {
      state: "weird".into(),
      page: 1,
    };
    let error = github_read(&gh, "o/r", query).await.unwrap_err();
    assert_eq!(error, "Unknown pull request state: weird");
    assert!(server.received_requests().await.unwrap().is_empty());
  }

  #[tokio::test]
  async fn search_open_prs_narrows_to_the_viewer() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "q": "repo:o/r is:pr is:open fix sort:created-desc author:@me",
        "first": 5,
      }})))
      .respond_with(search_page(vec![gql_listed_pr(7)], None))
      .expect(1)
      .mount(&server)
      .await;
    let query = GithubQuery::SearchOpenPrs {
      mine: true,
      query: " fix ".into(),
      limit: 5,
    };
    let value = github_read(&gh, "o/r", query).await.unwrap();
    assert_eq!(
      value,
      via_gh::<Vec<GhPullRequest>>(json!([gh_listed_pr(7)]))
    );
  }
}
