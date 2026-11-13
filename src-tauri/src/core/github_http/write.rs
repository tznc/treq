//! The changes [`crate::core::remote::GithubAction`] names, made over HTTPS
//! with the same checks and results as the host's `github_write`.

use super::GithubHttp;
use crate::core::remote::{require_text, GithubAction};
use crate::github;
use reqwest::Method;
use serde_json::{json, Value};

const DELETE_ISSUE_MUTATION: &str =
  "mutation($id: ID!) { deleteIssue(input: {issueId: $id}) { clientMutationId } }";
const CONVERT_TO_DRAFT_MUTATION: &str = "mutation($id: ID!) { convertPullRequestToDraft(input: {pullRequestId: $id}) { clientMutationId } }";
const MARK_READY_MUTATION: &str = "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { clientMutationId } }";

/// [`GithubAction`] carried out over HTTPS on the repository `full_name`
/// (`owner/name`), as the host's `github_write` carries it out.
pub async fn github_write(
  gh: &GithubHttp,
  full_name: &str,
  action: GithubAction,
) -> Result<Value, String> {
  match &action {
    GithubAction::CreateIssue { title, .. } | GithubAction::EditIssue { title, .. } => {
      require_text(title, "the title")?;
    }
    GithubAction::CommentOnIssue { body, .. }
    | GithubAction::CommentOnPr { body, .. }
    | GithubAction::ReplyToReviewThread { body, .. } => require_text(body, "the comment")?,
    _ => {}
  }
  let repo = format!("/repos/{full_name}");
  match action {
    GithubAction::CreateIssue { title, body } => {
      let body = json!({ "title": title, "body": body });
      let created = gh
        .rest(Method::POST, &format!("{repo}/issues"), Some(&body))
        .await?;
      let number = created["number"]
        .as_u64()
        .ok_or_else(|| format!("Could not parse issue number from output: {created}"))?;
      Ok(json!({ "number": number }))
    }
    GithubAction::EditIssue {
      number,
      title,
      body,
    } => {
      let body = json!({ "title": title, "body": body });
      patch(gh, &format!("{repo}/issues/{number}"), &body).await
    }
    // A pull request's conversation comments are its issue's comments.
    GithubAction::CommentOnIssue { number, body } | GithubAction::CommentOnPr { number, body } => {
      let path = format!("{repo}/issues/{number}/comments");
      done(
        gh.rest(Method::POST, &path, Some(&json!({ "body": body })))
          .await,
      )
    }
    GithubAction::CloseIssue { number, reason } => {
      if !github::ISSUE_CLOSE_REASONS.contains(&reason.as_str()) {
        return Err(format!("Unknown issue close reason: {reason}"));
      }
      // `gh issue close --reason "not planned"` sends `not_planned`.
      let body = json!({ "state": "closed", "state_reason": reason.replace(' ', "_") });
      patch(gh, &format!("{repo}/issues/{number}"), &body).await
    }
    GithubAction::ReopenIssue { number } => {
      patch(
        gh,
        &format!("{repo}/issues/{number}"),
        &json!({ "state": "open" }),
      )
      .await
    }
    GithubAction::DeleteIssue { number } => {
      let id = node_id(gh, &format!("{repo}/issues/{number}")).await?;
      done(gh.graphql(DELETE_ISSUE_MUTATION, json!({ "id": id })).await)
    }
    GithubAction::ClosePr { number } => {
      patch(
        gh,
        &format!("{repo}/pulls/{number}"),
        &json!({ "state": "closed" }),
      )
      .await
    }
    GithubAction::ReopenPr { number } => {
      patch(
        gh,
        &format!("{repo}/pulls/{number}"),
        &json!({ "state": "open" }),
      )
      .await
    }
    GithubAction::SetPrDraft { number, draft } => {
      let id = node_id(gh, &format!("{repo}/pulls/{number}")).await?;
      let mutation = if draft {
        CONVERT_TO_DRAFT_MUTATION
      } else {
        MARK_READY_MUTATION
      };
      done(gh.graphql(mutation, json!({ "id": id })).await)
    }
    GithubAction::ReplyToReviewThread { thread_id, body } => {
      let variables = json!({ "threadId": thread_id, "body": body });
      done(gh.graphql(github::REPLY_THREAD_MUTATION, variables).await)
    }
    GithubAction::SubmitPrReview {
      number,
      event,
      body,
      comments,
    } => {
      let body = github::review_request_body(&event, &body, &comments)?;
      let path = format!("{repo}/pulls/{number}/reviews");
      done(gh.rest(Method::POST, &path, Some(&body)).await)
    }
    GithubAction::SetReaction {
      comment_id,
      content,
      on,
    } => {
      let mutation = if on {
        github::ADD_REACTION_MUTATION
      } else {
        github::REMOVE_REACTION_MUTATION
      };
      let variables = json!({ "id": comment_id, "content": content });
      done(gh.graphql(mutation, variables).await)
    }
  }
}

/// `null`, as the host returns for a change that succeeded.
fn done(result: Result<Value, String>) -> Result<Value, String> {
  result.map(|_| Value::Null)
}

async fn patch(gh: &GithubHttp, path: &str, body: &Value) -> Result<Value, String> {
  done(gh.rest(Method::PATCH, path, Some(body)).await)
}

/// The GraphQL node id of the issue or pull request at the REST `path`.
async fn node_id(gh: &GithubHttp, path: &str) -> Result<String, String> {
  let item = gh.rest(Method::GET, path, None).await?;
  item["node_id"]
    .as_str()
    .map(str::to_string)
    .ok_or_else(|| format!("GitHub sent no node id for {path}"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::github::GhDraftReviewComment;
  use serde_json::json;
  use wiremock::matchers::{body_json, body_partial_json, body_string_contains, method, path};
  use wiremock::{Mock, MockServer, ResponseTemplate};

  async fn start() -> (MockServer, GithubHttp) {
    let server = MockServer::start().await;
    let gh = GithubHttp::with_base("tok".into(), server.uri());
    (server, gh)
  }

  fn ok(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
  }

  async fn expect_rest(server: &MockServer, verb: &str, at: &str, body: Value, reply: Value) {
    Mock::given(method(verb))
      .and(path(at))
      .and(body_json(body))
      .respond_with(ok(reply))
      .expect(1)
      .mount(server)
      .await;
  }

  async fn expect_graphql(server: &MockServer, mutation: &str, variables: Value) {
    Mock::given(method("POST"))
      .and(path("/graphql"))
      .and(body_string_contains(mutation))
      .and(body_partial_json(json!({ "variables": variables })))
      .respond_with(ok(
        json!({ "data": { (mutation): { "clientMutationId": null } } }),
      ))
      .expect(1)
      .mount(server)
      .await;
  }

  async fn assert_no_requests(server: &MockServer) {
    assert!(server.received_requests().await.unwrap().is_empty());
  }

  #[tokio::test]
  async fn create_issue_returns_its_number() {
    let (server, gh) = start().await;
    let created = json!({ "number": 12, "html_url": "https://github.com/o/r/issues/12" });
    let body = json!({ "title": "Crash", "body": "It crashes" });
    expect_rest(&server, "POST", "/repos/o/r/issues", body, created).await;
    let action = GithubAction::CreateIssue {
      title: "Crash".into(),
      body: "It crashes".into(),
    };
    let value = github_write(&gh, "o/r", action).await.unwrap();
    assert_eq!(value, json!({ "number": 12 }));
  }

  #[tokio::test]
  async fn blank_text_fails_before_any_request() {
    let (server, gh) = start().await;
    let cases = [
      (
        GithubAction::CreateIssue {
          title: " ".into(),
          body: "b".into(),
        },
        "invalid_arguments: the title is empty",
      ),
      (
        GithubAction::EditIssue {
          number: 1,
          title: "".into(),
          body: "b".into(),
        },
        "invalid_arguments: the title is empty",
      ),
      (
        GithubAction::CommentOnIssue {
          number: 1,
          body: "\n".into(),
        },
        "invalid_arguments: the comment is empty",
      ),
      (
        GithubAction::CommentOnPr {
          number: 1,
          body: "".into(),
        },
        "invalid_arguments: the comment is empty",
      ),
      (
        GithubAction::ReplyToReviewThread {
          thread_id: "T".into(),
          body: " ".into(),
        },
        "invalid_arguments: the comment is empty",
      ),
    ];
    for (action, expected) in cases {
      assert_eq!(
        github_write(&gh, "o/r", action).await.unwrap_err(),
        expected
      );
    }
    assert_no_requests(&server).await;
  }

  #[tokio::test]
  async fn edit_issue_replaces_title_and_body() {
    let (server, gh) = start().await;
    let body = json!({ "title": "New", "body": "Text" });
    expect_rest(
      &server,
      "PATCH",
      "/repos/o/r/issues/3",
      body,
      json!({ "number": 3 }),
    )
    .await;
    let action = GithubAction::EditIssue {
      number: 3,
      title: "New".into(),
      body: "Text".into(),
    };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn comments_on_issues_and_prs_go_to_the_issue_comments() {
    let (server, gh) = start().await;
    let reply = json!({ "id": 1 });
    let issue = json!({ "body": "On the issue" });
    expect_rest(
      &server,
      "POST",
      "/repos/o/r/issues/3/comments",
      issue,
      reply.clone(),
    )
    .await;
    let pr = json!({ "body": "On the PR" });
    expect_rest(&server, "POST", "/repos/o/r/issues/5/comments", pr, reply).await;
    let on_issue = GithubAction::CommentOnIssue {
      number: 3,
      body: "On the issue".into(),
    };
    let on_pr = GithubAction::CommentOnPr {
      number: 5,
      body: "On the PR".into(),
    };
    assert_eq!(
      github_write(&gh, "o/r", on_issue).await.unwrap(),
      Value::Null
    );
    assert_eq!(github_write(&gh, "o/r", on_pr).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn close_issue_sends_the_reason() {
    let (server, gh) = start().await;
    let body = json!({ "state": "closed", "state_reason": "not_planned" });
    expect_rest(&server, "PATCH", "/repos/o/r/issues/3", body, json!({})).await;
    let action = GithubAction::CloseIssue {
      number: 3,
      reason: "not planned".into(),
    };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn close_issue_rejects_an_unknown_reason_without_a_request() {
    let (server, gh) = start().await;
    let action = GithubAction::CloseIssue {
      number: 3,
      reason: "duplicate".into(),
    };
    let error = github_write(&gh, "o/r", action).await.unwrap_err();
    assert_eq!(error, "Unknown issue close reason: duplicate");
    assert_no_requests(&server).await;
  }

  #[tokio::test]
  async fn reopen_issue_opens_it() {
    let (server, gh) = start().await;
    let body = json!({ "state": "open" });
    expect_rest(&server, "PATCH", "/repos/o/r/issues/3", body, json!({})).await;
    let action = GithubAction::ReopenIssue { number: 3 };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn delete_issue_deletes_it_by_node_id() {
    let (server, gh) = start().await;
    Mock::given(method("GET"))
      .and(path("/repos/o/r/issues/3"))
      .respond_with(ok(json!({ "number": 3, "node_id": "I_3" })))
      .expect(1)
      .mount(&server)
      .await;
    expect_graphql(&server, "deleteIssue", json!({ "id": "I_3" })).await;
    let action = GithubAction::DeleteIssue { number: 3 };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn close_and_reopen_pr_set_its_state() {
    let (server, gh) = start().await;
    let closed = json!({ "state": "closed" });
    expect_rest(&server, "PATCH", "/repos/o/r/pulls/5", closed, json!({})).await;
    let action = GithubAction::ClosePr { number: 5 };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
    server.verify().await;
    server.reset().await;
    let open = json!({ "state": "open" });
    expect_rest(&server, "PATCH", "/repos/o/r/pulls/5", open, json!({})).await;
    let action = GithubAction::ReopenPr { number: 5 };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  async fn mount_pr_node(server: &MockServer) {
    Mock::given(method("GET"))
      .and(path("/repos/o/r/pulls/5"))
      .respond_with(ok(json!({ "number": 5, "node_id": "PR_5" })))
      .mount(server)
      .await;
  }

  #[tokio::test]
  async fn set_pr_draft_converts_to_draft() {
    let (server, gh) = start().await;
    mount_pr_node(&server).await;
    expect_graphql(
      &server,
      "convertPullRequestToDraft",
      json!({ "id": "PR_5" }),
    )
    .await;
    let action = GithubAction::SetPrDraft {
      number: 5,
      draft: true,
    };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn set_pr_draft_off_marks_it_ready() {
    let (server, gh) = start().await;
    mount_pr_node(&server).await;
    expect_graphql(
      &server,
      "markPullRequestReadyForReview",
      json!({ "id": "PR_5" }),
    )
    .await;
    let action = GithubAction::SetPrDraft {
      number: 5,
      draft: false,
    };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn reply_to_review_thread_posts_the_reply() {
    let (server, gh) = start().await;
    expect_graphql(
      &server,
      "addPullRequestReviewThreadReply",
      json!({ "threadId": "T_1", "body": "Done" }),
    )
    .await;
    let action = GithubAction::ReplyToReviewThread {
      thread_id: "T_1".into(),
      body: "Done".into(),
    };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn submit_pr_review_sends_its_inline_comments() {
    let (server, gh) = start().await;
    let body = json!({
      "event": "REQUEST_CHANGES",
      "body": "See inline",
      "comments": [
        { "path": "a.rs", "line": 4, "side": "RIGHT", "body": "one" },
        {
          "path": "b.rs", "line": 9, "side": "LEFT", "body": "range",
          "start_line": 7, "start_side": "LEFT",
        },
      ],
    });
    expect_rest(
      &server,
      "POST",
      "/repos/o/r/pulls/5/reviews",
      body,
      json!({ "id": 1 }),
    )
    .await;
    let comment = |path: &str, line, side: &str, start_line, body: &str| GhDraftReviewComment {
      path: path.into(),
      line,
      side: side.into(),
      start_line,
      body: body.into(),
    };
    let action = GithubAction::SubmitPrReview {
      number: 5,
      event: "REQUEST_CHANGES".into(),
      body: "See inline".into(),
      comments: vec![
        // A start at the line itself is a one-line comment.
        comment("a.rs", 4, "RIGHT", Some(4), "one"),
        comment("b.rs", 9, "LEFT", Some(7), "range"),
      ],
    };
    assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
  }

  #[tokio::test]
  async fn submit_pr_review_rejects_an_unknown_event_without_a_request() {
    let (server, gh) = start().await;
    let action = GithubAction::SubmitPrReview {
      number: 5,
      event: "MERGE".into(),
      body: String::new(),
      comments: Vec::new(),
    };
    let error = github_write(&gh, "o/r", action).await.unwrap_err();
    assert_eq!(error, "Invalid review event: MERGE");
    assert_no_requests(&server).await;
  }

  #[tokio::test]
  async fn set_reaction_adds_or_removes_it() {
    let (server, gh) = start().await;
    let variables = json!({ "id": "IC_1", "content": "HEART" });
    expect_graphql(&server, "addReaction", variables.clone()).await;
    expect_graphql(&server, "removeReaction", variables).await;
    for on in [true, false] {
      let action = GithubAction::SetReaction {
        comment_id: "IC_1".into(),
        content: "HEART".into(),
        on,
      };
      assert_eq!(github_write(&gh, "o/r", action).await.unwrap(), Value::Null);
    }
  }

  #[tokio::test]
  async fn a_failed_mutation_reports_githubs_error() {
    let (server, gh) = start().await;
    Mock::given(path("/graphql"))
      .respond_with(ok(json!({
        "data": null,
        "errors": [{ "message": "Could not resolve to a node" }],
      })))
      .mount(&server)
      .await;
    let action = GithubAction::SetReaction {
      comment_id: "gone".into(),
      content: "HEART".into(),
      on: true,
    };
    let error = github_write(&gh, "o/r", action).await.unwrap_err();
    assert!(error.contains("Could not resolve to a node"), "{error}");
  }
}
