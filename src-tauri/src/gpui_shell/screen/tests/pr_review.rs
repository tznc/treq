//! The pull request review screen (`PrReviewPage`).

use gpui::TestAppContext;

use super::*;
use crate::gpui_shell::screen::pr_review_data::{Event, NewDraft, PrReviewHost};

impl PrReviewHost for FakeHost {
  fn remote_full_name(&self, _: &str) -> Option<String> {
    self.record("read remote".into());
    self.pr_remote.clone()
  }

  fn view_pr(&self, full_name: &str, number: u64) -> Result<crate::github::GhPullRequest, String> {
    self.record(format!("read pr {full_name} {number}"));
    serde_json::from_value(serde_json::json!({
      "number": number, "title": "Fix login", "state": "OPEN",
      "url": "https://github.com/o/r/pull/5", "body": null,
      "author": {"login": "octo"}, "labels": [],
      "headRefName": "fix/login", "baseRefName": "main", "mergeStateStatus": null,
      "createdAt": "", "updatedAt": "", "comments": null, "isDraft": false
    }))
    .map_err(|e| e.to_string())
  }

  fn pr_diff(&self, full_name: &str, number: u64) -> Result<String, String> {
    self.record(format!("read pr diff {full_name} {number}"));
    self
      .pr_diff
      .clone()
      .unwrap_or_else(|| Err("no diff".into()))
  }

  fn current_user(&self) -> Result<String, String> {
    self.record("read viewer".into());
    self.pr_viewer.clone().ok_or_else(|| "signed out".into())
  }

  fn pr_threads(
    &self,
    full_name: &str,
    number: u64,
  ) -> Result<Vec<crate::github::GhReviewThread>, String> {
    self.record(format!("read pr threads {full_name} {number}"));
    Ok(self.threads.clone())
  }

  fn reply_thread(&self, thread_id: &str, body: &str) -> Result<(), String> {
    self.record(format!("reply {thread_id} {body}"));
    Ok(())
  }

  fn submit_review(
    &self,
    full_name: &str,
    number: u64,
    event: &str,
    body: &str,
    comments: &[crate::github::GhDraftReviewComment],
  ) -> Result<(), String> {
    self.record(format!(
      "submit review {full_name} {number} {event} body={body:?} comments={}",
      comments.len()
    ));
    self.pr_submit_error.clone().map_or(Ok(()), Err)
  }

  fn drafts(
    &self,
    _: &str,
    number: u64,
  ) -> Result<Vec<crate::local_db::AgentReviewComment>, String> {
    self.record(format!("read drafts {number}"));
    Ok(self.agent.lock().unwrap().clone())
  }

  fn create_draft(&self, _: &str, number: u64, draft: &NewDraft) -> Result<(), String> {
    self.record(format!(
      "create draft {number} {} {}-{} {:?} {} hash={}",
      draft.file_path,
      draft.start_line,
      draft.end_line,
      draft.side,
      draft.text,
      draft.line_hash.is_some()
    ));
    let mut row = draft_row("new", 0, None);
    row.file_path = draft.file_path.clone();
    row.start_line = draft.start_line;
    row.end_line = draft.end_line;
    row.comment_text = draft.text.clone();
    row.line_hash = draft.line_hash.clone();
    row.quoted_text = draft.quoted_text.clone();
    self.agent.lock().unwrap().push(row);
    Ok(())
  }

  fn update_draft(
    &self,
    _: &str,
    id: &str,
    text: Option<&str>,
    line_hash: Option<&str>,
    quoted_text: Option<&str>,
  ) -> Result<(), String> {
    self.record(format!(
      "update draft {id} {text:?} {}",
      line_hash.is_some()
    ));
    for row in self.agent.lock().unwrap().iter_mut().filter(|r| r.id == id) {
      if let Some(text) = text {
        row.comment_text = text.into();
      }
      if let Some(hash) = line_hash {
        row.line_hash = Some(hash.into());
      }
      if let Some(quoted) = quoted_text {
        row.quoted_text = Some(quoted.into());
      }
    }
    Ok(())
  }
}

const DIFF: &str = "diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,3 @@
 a
+b
+c
diff --git a/b.rs b/b.rs
new file mode 100644
--- /dev/null
+++ b/b.rs
@@ -0,0 +1 @@
+d
";

fn draft_row(id: &str, line: i64, hash: Option<&str>) -> crate::local_db::AgentReviewComment {
  serde_json::from_value(serde_json::json!({
    "id": id, "repo_path": "/repo", "target_type": "github_pr", "target_id": "5",
    "file_path": "a.rs", "hunk_id": null, "start_line": line, "end_line": line, "side": "new",
    "comment_text": "Rename this", "suggested_replacement": null, "status": "open",
    "source": "local-agent", "created_at": "", "resolved_at": null, "line_hash": hash
  }))
  .expect("draft row")
}

fn pr_host() -> FakeHost {
  FakeHost {
    pr_remote: Some("o/r".into()),
    pr_diff: Some(Ok(DIFF.into())),
    pr_viewer: Some("me".into()),
    ..FakeHost::default()
  }
}

fn pr_route() -> ScreenSnapshot {
  serde_json::from_value(serde_json::json!({
    "kind": "prReview", "dark": false, "repoPath": "/repo", "prNumber": 5
  }))
  .expect("pr review route")
}

/// The postable drafts and the outdated count, as the header reads them.
fn counts(h: &mut Harness) -> (usize, usize) {
  h.cx.run_until_parked();
  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.pr_review.clone())
    .expect("pr review view");
  h.cx.update(|_, cx| {
    let changes = view.read(cx).changes.clone().expect("diff");
    let pr = changes.read(cx);
    let pr = pr.pr_mode().expect("pr mode");
    (pr.postable().len(), pr.outdated())
  })
}

fn event(h: &mut Harness) -> Event {
  let view = h
    .screen
    .read_with(&h.cx, |s, _| s.pr_review.clone())
    .expect("pr review view");
  view.read_with(&h.cx, |v, _| v.event)
}

#[gpui::test]
fn pr_review_draws_the_pr_and_its_diff(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, pr_host(), pr_route());
  for selector in [
    "pr-review-page",
    "pr-review-title",
    "pr-review-subtitle",
    "pr-file-a.rs",
    "pr-file-b.rs",
    "file-card-a.rs",
    "file-card-b.rs",
    "pr-review-ai-review",
    "pr-review-finish",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  // Read-only: no file comments, no file menu, no commit sidebar.
  assert!(!h.drawn("add-file-comment-button"));
  assert!(!h.drawn("file-row-a.rs"));
  let calls = host.calls();
  for call in [
    "read pr o/r 5",
    "read pr diff o/r 5",
    "read pr threads o/r 5",
    "read drafts 5",
  ] {
    assert!(calls.contains(&call.to_string()), "{call}: {calls:?}");
  }
}

#[gpui::test]
fn pr_review_stays_empty_without_a_github_remote(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_remote: None,
    ..pr_host()
  };
  let (mut h, host) = workspace_harness(cx, host, pr_route());
  assert!(h.drawn("pr-review-page"));
  assert!(!h.drawn("pr-review-title"));
  assert!(!host.calls().iter().any(|c| c.starts_with("read pr diff")));
}

#[gpui::test]
fn pr_review_shows_why_the_diff_failed(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_diff: Some(Err("HTTP 404".into())),
    ..pr_host()
  };
  let (mut h, _) = workspace_harness(cx, host, pr_route());
  assert!(h.drawn("pr-diff-error"));
  assert!(!h.drawn("file-card-a.rs"));
}

#[gpui::test]
fn pr_review_keeps_a_line_comment_as_a_draft(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, pr_host(), pr_route());
  comment_line(&mut h, "Nit");
  assert!(
    host
      .writes()
      .contains(&"create draft 5 a.rs 2-2 New Nit hash=true".to_string()),
    "{:?}",
    host.writes()
  );
  assert!(h.drawn("inline-comment-card"));
  assert_eq!(counts(&mut h), (1, 0));
  // No pending review is kept for a PR.
  assert!(!host.writes().iter().any(|w| w.starts_with("save review")));
}

#[gpui::test]
fn pr_review_edits_and_deletes_a_draft(cx: &mut TestAppContext) {
  let host = pr_host();
  *host.agent.lock().unwrap() = vec![draft_row("d1", 2, None)];
  let (mut h, host) = workspace_harness(cx, host, pr_route());
  // The agent's draft had no hash: it gets one on first sight.
  assert!(host
    .writes()
    .contains(&"update draft d1 None true".to_string()));
  h.click("inline-comment-card");
  h.click("comment-edit");
  h.cx.simulate_input("!");
  h.click("comment-save");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"update draft d1 Some(\"Rename this!\") false".to_string()));
  h.click("delete-comment");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"delete agent comment d1".to_string()));
  assert!(!h.drawn("inline-comment-card"));
}

#[gpui::test]
fn pr_review_lists_outdated_drafts_apart(cx: &mut TestAppContext) {
  let host = pr_host();
  *host.agent.lock().unwrap() = vec![draft_row("d1", 2, Some("deadbeef"))];
  let (mut h, _) = workspace_harness(cx, host, pr_route());
  assert!(h.drawn("outdated-comments"));
  assert!(!h.drawn("inline-comment-card"));
  assert_eq!(counts(&mut h), (0, 1));
}

#[gpui::test]
fn pr_review_submits_the_drafts_to_github(cx: &mut TestAppContext) {
  let host = pr_host();
  *host.agent.lock().unwrap() = vec![draft_row("d1", 2, None)];
  let (mut h, host) = workspace_harness(cx, host, pr_route());
  h.click("pr-review-finish");
  assert!(h.drawn("pr-review-popover"));
  h.click("pr-review-event-APPROVE");
  h.click("pr-review-submit");
  h.cx.run_until_parked();
  let writes = host.writes();
  assert!(
    writes.contains(&"submit review o/r 5 APPROVE body=\"\" comments=1".to_string()),
    "{writes:?}"
  );
  assert!(writes.contains(&"resolve agent comment d1".to_string()));
  assert!(
    writes.contains(&"notify toast Review submitted".to_string()),
    "{writes:?}"
  );
  assert!(!h.drawn("pr-review-popover"));
  assert_eq!(event(&mut h), Event::Comment);
  assert_eq!(counts(&mut h), (0, 0));
  assert!(h.sent().contains(&press("prReview.submitted")));
}

#[gpui::test]
fn pr_review_stays_open_after_a_comment(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, pr_host(), pr_route());
  h.click("pr-review-finish");
  h.cx.run_until_parked();
  h.cx.simulate_input("looks fine");
  h.click("pr-review-submit");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .iter()
    .any(|w| w.starts_with("submit review o/r 5 COMMENT")));
  assert!(!h.sent().contains(&press("prReview.submitted")));
}

#[gpui::test]
fn pr_review_keeps_the_review_when_submitting_fails(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_submit_error: Some("HTTP 502".into()),
    ..pr_host()
  };
  let (mut h, host) = workspace_harness(cx, host, pr_route());
  h.click("pr-review-finish");
  h.cx.run_until_parked();
  h.cx.simulate_input("ship it");
  h.click("pr-review-event-APPROVE");
  h.click("pr-review-submit");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"notify toast Failed to submit review".to_string()));
  assert!(!h.sent().contains(&press("prReview.submitted")));
  assert!(h.drawn("pr-review-popover"));
  assert_eq!(event(&mut h), Event::Approve);
  h.click("pr-review-submit");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"submit review o/r 5 APPROVE body=\"ship it\" comments=0".to_string()));
}

#[gpui::test]
fn pr_review_needs_a_reason_to_request_changes(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, pr_host(), pr_route());
  h.click("pr-review-finish");
  h.click("pr-review-event-REQUEST_CHANGES");
  h.click("pr-review-submit");
  h.cx.run_until_parked();
  assert!(!host.writes().iter().any(|w| w.starts_with("submit review")));
}

#[gpui::test]
fn pr_review_author_can_only_comment(cx: &mut TestAppContext) {
  let host = FakeHost {
    pr_viewer: Some("octo".into()),
    ..pr_host()
  };
  let (mut h, _) = workspace_harness(cx, host, pr_route());
  h.click("pr-review-finish");
  h.click("pr-review-event-APPROVE");
  h.cx.run_until_parked();
  assert_eq!(event(&mut h), Event::Comment);
}

#[gpui::test]
fn pr_review_starts_an_ai_review_in_the_home_repository(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, pr_host(), pr_route());
  h.click("pr-review-ai-review");
  h.cx.run_until_parked();
  assert!(fields(&h, "prReview.startAgentReview").is_empty());
  let launches = host.launches.lock().unwrap().clone();
  let [(request, true)] = &launches[..] else {
    panic!("one selected launch: {launches:?}");
  };
  assert_eq!(request.workspace_id, None);
  assert_eq!(request.session_name.as_deref(), Some("AI Review"));
  let prompt = request.prompt.clone().unwrap_or_default();
  assert!(
    prompt.contains("--target-type github_pr --target-id 5"),
    "{prompt}"
  );
  assert!(prompt.contains("`gh pr diff 5 --repo o/r`"), "{prompt}");
}

#[gpui::test]
fn pr_review_back_leaves_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, pr_host(), pr_route());
  h.click("pr-review-back");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("prReview.close")]);
}

#[gpui::test]
fn pr_review_replies_to_a_thread(cx: &mut TestAppContext) {
  let mut host = pr_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, host) = workspace_harness(cx, host, pr_route());
  h.click("github-thread-reply");
  assert!(h.drawn("github-thread-reply-box"));
  h.cx.simulate_input("Done");
  h.click("github-thread-reply-send");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"reply t1 Done".to_string()));
  assert!(!h.drawn("github-thread-reply-box"));
}

#[gpui::test]
fn pr_review_marks_files_viewed_without_the_backend(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, pr_host(), pr_route());
  h.click("viewed-a.rs");
  h.cx.run_until_parked();
  assert!(!host.writes().iter().any(|w| w.starts_with("mark viewed")));
  assert!(!h.drawn("gutter-a.rs-0-1"));
}

#[gpui::test]
fn pr_review_file_list_scrolls_to_the_file(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, pr_host(), pr_route());
  h.click("pr-file-b.rs");
  h.cx.run_until_parked();
  assert!(h.drawn("file-card-b.rs"));
}
