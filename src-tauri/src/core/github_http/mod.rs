//! GitHub over HTTPS with the signed-in user's token, for a client that has
//! no `gh` to run. Each read returns the same JSON as the host's
//! [`crate::core::remote::github_read`], which goes through `gh`, so a
//! screen cannot tell which one answered.

use crate::core::remote::GithubQuery;
use serde_json::Value;

/// A GitHub API client authenticated as one user.
pub struct GithubHttp {
  client: reqwest::Client,
  token: String,
  api_base: String,
}

impl GithubHttp {
  /// A client for `api.github.com`.
  pub fn new(token: String) -> Self {
    unimplemented!("{token}")
  }

  /// A client for the API at `api_base`, such as a test server.
  pub fn with_base(token: String, api_base: String) -> Self {
    unimplemented!("{token}{api_base}")
  }
}

/// [`GithubQuery`] answered over HTTPS for the repository `full_name`
/// (`owner/name`), as the host's `github_read` answers it.
pub async fn github_read(
  gh: &GithubHttp,
  full_name: &str,
  query: GithubQuery,
) -> Result<Value, String> {
  let _ = (&gh.client, &gh.token, &gh.api_base, full_name, query);
  unimplemented!()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::github::{GhIssue, GhListPage, GhPullRequest};
  use serde_json::json;
  use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
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
  async fn other_http_errors_carry_githubs_message() {
    let (server, gh) = start().await;
    Mock::given(path("/repos/o/r/pulls/9"))
      .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "message": "Not Found" })))
      .mount(&server)
      .await;
    let error = github_read(&gh, "o/r", GithubQuery::PrDiff { number: 9 })
      .await
      .unwrap_err();
    assert!(!error.starts_with("github_unauthorized"), "{error}");
    assert!(error.contains("Not Found"), "{error}");
  }

  #[tokio::test]
  async fn graphql_errors_are_surfaced() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "data": null,
        "errors": [{ "message": "Could not resolve to an Issue" }],
      })))
      .mount(&server)
      .await;
    let error = github_read(&gh, "o/r", GithubQuery::Issue { number: 1 })
      .await
      .unwrap_err();
    assert!(error.contains("Could not resolve to an Issue"), "{error}");
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
    let query = GithubQuery::ListPrs {
      state: "open".into(),
      page: 1,
    };
    let value = github_read(&gh, "o/r", query).await.unwrap();
    let expected = via_gh::<Vec<GhPullRequest>>(json!([gh_listed_pr(5)]));
    assert_eq!(
      value,
      json!({ "items": expected, "hasMore": false, "endCursor": null })
    );
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

  fn gql_comment(id: &str, reactions: Value) -> Value {
    json!({
      "id": id,
      "body": "Looks good",
      "createdAt": "2026-01-03T00:00:00Z",
      "author": { "login": "rev" },
      "reactionGroups": reactions,
    })
  }

  fn gh_comment(id: &str) -> Value {
    json!({
      "id": id,
      "author": { "login": "rev" },
      "authorAssociation": "MEMBER",
      "body": "Looks good",
      "createdAt": "2026-01-03T00:00:00Z",
      "includesCreatedEdit": false,
      "isMinimized": false,
      "minimizedReason": "",
      "reactionGroups": [{ "content": "HEART", "users": { "totalCount": 2 } }],
      "url": "https://github.com/o/r/issues/3#issuecomment-1",
      "viewerDidAuthor": false,
    })
  }

  /// GraphQL lists all eight reactions; the host shows only those with
  /// someone on them, with whether the viewer is one.
  fn gql_reactions() -> Value {
    json!([
      { "content": "THUMBS_UP", "viewerHasReacted": false, "reactors": { "totalCount": 0 } },
      { "content": "HEART", "viewerHasReacted": true, "reactors": { "totalCount": 2 } },
    ])
  }

  fn gql_issue(comment: &str, next: Option<&str>) -> ResponseTemplate {
    data(json!({ "repository": { "issue": {
      "number": 3, "title": "Crash", "state": "CLOSED",
      "url": "https://github.com/o/r/issues/3", "body": "It crashes",
      "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
      "author": { "login": "octo" },
      "labels": { "nodes": [{ "name": "bug", "color": "d73a4a" }] },
      "comments": {
        "nodes": [gql_comment(comment, gql_reactions())],
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
      },
    }}}))
  }

  #[tokio::test]
  async fn issue_reads_all_its_comments_with_reactions() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "owner": "o", "name": "r", "number": 3, "after": null,
      }})))
      .respond_with(gql_issue("IC_1", Some("k1")))
      .expect(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": { "after": "k1" } })))
      .respond_with(gql_issue("IC_2", None))
      .expect(1)
      .mount(&server)
      .await;
    let value = github_read(&gh, "o/r", GithubQuery::Issue { number: 3 })
      .await
      .unwrap();
    let mut expected = via_gh::<GhIssue>(json!({
      "author": { "id": "U_1", "is_bot": false, "login": "octo", "name": "Octo" },
      "body": "It crashes",
      "comments": [gh_comment("IC_1"), gh_comment("IC_2")],
      "createdAt": "2026-01-01T00:00:00Z",
      "labels": [{ "color": "d73a4a", "description": "", "id": "LA_1", "name": "bug" }],
      "number": 3,
      "state": "CLOSED",
      "title": "Crash",
      "updatedAt": "2026-01-02T00:00:00Z",
      "url": "https://github.com/o/r/issues/3",
    }));
    // What `fill_comment_reactions` adds.
    expected["comments"][0]["reactions"][0]["viewer_has_reacted"] = json!(true);
    expected["comments"][1]["reactions"][0]["viewer_has_reacted"] = json!(true);
    assert_eq!(value, expected);
  }

  #[tokio::test]
  async fn pr_reads_reviews_and_comments_like_gh_pr_view() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": {
        "owner": "o", "name": "r", "number": 5,
      }})))
      .respond_with(data(json!({ "repository": { "pullRequest": {
        "number": 5, "title": "Fix 5", "state": "OPEN",
        "url": "https://github.com/o/r/pull/5", "body": "",
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
        "author": { "login": "octo" },
        "labels": { "nodes": [] },
        "headRefName": "fix-5", "baseRefName": "main",
        "mergeStateStatus": "BLOCKED", "isDraft": true, "reviewDecision": null,
        "latestReviews": { "nodes": [{ "author": { "login": "rev" }, "state": "APPROVED" }] },
        "reviews": { "nodes": [
          { "author": { "login": "rev" }, "state": "COMMENTED", "submittedAt": "2026-01-03T00:00:00Z" },
          { "author": null, "state": "APPROVED", "submittedAt": "2026-01-04T00:00:00Z" },
        ]},
        "comments": {
          "nodes": [gql_comment("IC_1", gql_reactions())],
          "pageInfo": { "hasNextPage": false, "endCursor": "k1" },
        },
      }}})))
      .expect(1)
      .mount(&server)
      .await;
    let value = github_read(&gh, "o/r", GithubQuery::Pr { number: 5 })
      .await
      .unwrap();
    let review = |id: &str, login: &str, state: &str, at: &str| {
      json!({
        "id": id, "author": { "login": login }, "authorAssociation": "MEMBER",
        "body": "", "submittedAt": at, "includesCreatedEdit": false,
        "reactionGroups": [], "state": state, "commit": { "oid": "abc" },
      })
    };
    let mut expected = via_gh::<GhPullRequest>(json!({
      "author": { "id": "U_1", "is_bot": false, "login": "octo", "name": "Octo" },
      "baseRefName": "main",
      "body": "",
      "comments": [gh_comment("IC_1")],
      "createdAt": "2026-01-01T00:00:00Z",
      "headRefName": "fix-5",
      "isDraft": true,
      "labels": [],
      "latestReviews": [review("PRR_1", "rev", "APPROVED", "2026-01-03T00:00:00Z")],
      "mergeStateStatus": "BLOCKED",
      "number": 5,
      // gh prints a missing decision as an empty string.
      "reviewDecision": "",
      "reviews": [
        review("PRR_1", "rev", "COMMENTED", "2026-01-03T00:00:00Z"),
        // gh prints a deleted account as an empty login.
        review("PRR_2", "", "APPROVED", "2026-01-04T00:00:00Z"),
      ],
      "state": "OPEN",
      "title": "Fix 5",
      "updatedAt": "2026-01-02T00:00:00Z",
      "url": "https://github.com/o/r/pull/5",
    }));
    expected["comments"][0]["reactions"][0]["viewer_has_reacted"] = json!(true);
    assert_eq!(value, expected);
  }

  #[tokio::test]
  async fn pr_diff_reads_the_diff_media_type() {
    let (server, gh) = start().await;
    let diff = "diff --git a/x b/x\n+new\n";
    Mock::given(method("GET"))
      .and(path("/repos/o/r/pulls/9"))
      .and(header("accept", "application/vnd.github.diff"))
      .respond_with(ResponseTemplate::new(200).set_body_string(diff))
      .expect(1)
      .mount(&server)
      .await;
    let value = github_read(&gh, "o/r", GithubQuery::PrDiff { number: 9 })
      .await
      .unwrap();
    assert_eq!(value, json!(diff));
  }

  #[tokio::test]
  async fn pr_checks_rolls_up_like_gh_pr_checks() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_string_contains("statusCheckRollup"))
      .and(body_partial_json(json!({ "variables": {
        "owner": "o", "name": "r", "number": 4,
      }})))
      .respond_with(data(
        json!({ "repository": { "pullRequest": { "commits": { "nodes": [
          { "commit": { "statusCheckRollup": { "contexts": { "nodes": [
            {
              "__typename": "CheckRun", "name": "build", "status": "COMPLETED",
              "conclusion": "SUCCESS", "detailsUrl": "https://ci/build",
              "startedAt": "2026-01-01T00:00:00Z", "completedAt": "2026-01-01T00:01:30Z",
            },
            {
              "__typename": "CheckRun", "name": "test", "status": "IN_PROGRESS",
              "conclusion": null, "detailsUrl": "https://ci/test",
              "startedAt": "2026-01-01T00:00:00Z", "completedAt": null,
            },
            {
              "__typename": "StatusContext", "context": "deploy", "state": "FAILURE",
              "targetUrl": "https://ci/deploy",
            },
          ]}}}}
        ]}}}}),
      ))
      .expect(1)
      .mount(&server)
      .await;
    let value = github_read(&gh, "o/r", GithubQuery::PrChecks { number: 4 })
      .await
      .unwrap();
    assert_eq!(
      value,
      json!({
        "state": "failure", "total": 3, "passed": 1, "failed": 1, "pending": 1,
        "checks": [
          { "name": "build", "bucket": "pass", "link": "https://ci/build", "duration_secs": 90 },
          { "name": "test", "bucket": "pending", "link": "https://ci/test" },
          { "name": "deploy", "bucket": "fail", "link": "https://ci/deploy" },
        ],
      })
    );
  }

  #[tokio::test]
  async fn pr_checks_with_none_reported_is_null() {
    let (server, gh) = start().await;
    Mock::given(path("/graphql"))
      .respond_with(data(
        json!({ "repository": { "pullRequest": { "commits": { "nodes": [
          { "commit": { "statusCheckRollup": null } }
        ]}}}}),
      ))
      .mount(&server)
      .await;
    let value = github_read(&gh, "o/r", GithubQuery::PrChecks { number: 4 })
      .await
      .unwrap();
    assert_eq!(value, Value::Null);
  }

  fn gql_thread(id: &str, author: Value) -> Value {
    json!({
      "id": id, "isResolved": false, "isOutdated": true, "path": "src/a.rs",
      "line": 12, "diffSide": "RIGHT",
      "comments": { "nodes": [{
        "id": format!("{id}-c"), "body": "nit", "createdAt": "2026-01-01T00:00:00Z",
        "url": "https://github.com/o/r/pull/5#discussion_r1", "diffHunk": "@@ -1 +1 @@",
        "author": author, "reactionGroups": gql_reactions(),
      }]},
    })
  }

  #[tokio::test]
  async fn pr_review_threads_follows_the_cursor() {
    let (server, gh) = start().await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_string_contains("reviewThreads"))
      .and(body_partial_json(json!({ "variables": {
        "owner": "o", "repo": "r", "number": 5, "after": null,
      }})))
      .respond_with(data(
        json!({ "repository": { "pullRequest": { "reviewThreads": {
          "pageInfo": { "hasNextPage": true, "endCursor": "t1" },
          "nodes": [gql_thread("T1", json!({ "login": "rev", "avatarUrl": "https://a/rev" }))],
        }}}}),
      ))
      .expect(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_partial_json(json!({ "variables": { "after": "t1" } })))
      .respond_with(data(
        json!({ "repository": { "pullRequest": { "reviewThreads": {
          "pageInfo": { "hasNextPage": false, "endCursor": "t2" },
          "nodes": [gql_thread("T2", Value::Null)],
        }}}}),
      ))
      .expect(1)
      .mount(&server)
      .await;
    let value = github_read(&gh, "o/r", GithubQuery::PrReviewThreads { number: 5 })
      .await
      .unwrap();
    let thread = |id: &str, author: Value| {
      json!({
        "id": id, "is_resolved": false, "is_outdated": true, "path": "src/a.rs",
        "line": 12, "diff_side": "RIGHT",
        "comments": [{
          "id": format!("{id}-c"), "body": "nit", "author": author,
          "created_at": "2026-01-01T00:00:00Z", "diff_hunk": "@@ -1 +1 @@",
          "url": "https://github.com/o/r/pull/5#discussion_r1",
          "reactions": [{ "content": "HEART", "count": 2, "viewer_has_reacted": true }],
        }],
      })
    };
    assert_eq!(
      value,
      json!([
        thread(
          "T1",
          json!({ "login": "rev", "avatar_url": "https://a/rev" })
        ),
        thread("T2", json!({ "login": "ghost", "avatar_url": null })),
      ])
    );
  }

  #[tokio::test]
  async fn pr_review_threads_for_a_name_without_an_owner_is_empty() {
    let (server, gh) = start().await;
    let query = GithubQuery::PrReviewThreads { number: 5 };
    let value = github_read(&gh, "no-slash", query).await.unwrap();
    assert_eq!(value, json!([]));
    assert!(server.received_requests().await.unwrap().is_empty());
  }
}
