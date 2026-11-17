//! What the PR view shows of a pull request: where its review stands, its
//! timeline, and its checks folded to what needs attention.

use crate::github::{
  GhAuthor, GhIssueComment, GhPullRequest, GhReview, PrCheckEntry, ReviewState, TimelineItem,
};

/// The comments and the reviews' verdicts, oldest first. GitHub's
/// timestamps are ISO 8601 in UTC, so they sort as text.
pub fn timeline<'a>(
  comments: &'a [GhIssueComment],
  reviews: &'a [GhReview],
) -> Vec<TimelineItem<'a>> {
  let mut items: Vec<(&str, TimelineItem)> = comments
    .iter()
    .map(|c| (c.created_at.as_str(), TimelineItem::Comment(c)))
    .chain(
      reviews
        .iter()
        .filter(|r| r.verdict().is_some())
        .map(|r| (r.submitted_at.as_str(), TimelineItem::Review(r))),
    )
    .collect();
  items.sort_by(|a, b| a.0.cmp(b.0));
  items.into_iter().map(|(_, item)| item).collect()
}

/// `author`'s avatar: the one GitHub reported, else their profile image.
pub fn avatar_url(author: &GhAuthor) -> String {
  author
    .avatar_url
    .clone()
    .filter(|url| !url.is_empty())
    .unwrap_or_else(|| format!("https://github.com/{}.png?size=64", author.login))
}

/// GitHub's review decision when branch protection gives one, else what the
/// reviewers' latest reviews add up to: a change request outweighs an
/// approval, and dismissed reviews count for nothing. A draft is not asking
/// for review yet.
pub fn review_state(pr: &GhPullRequest) -> Option<ReviewState> {
  let state = match pr.review_decision.as_deref() {
    Some("APPROVED") => Some(ReviewState::Approved),
    Some("CHANGES_REQUESTED") => Some(ReviewState::ChangesRequested),
    Some("REVIEW_REQUIRED") => Some(ReviewState::ReviewRequired),
    _ => {
      let has = |s: &str| pr.latest_reviews.iter().any(|r| r.state == s);
      if has("CHANGES_REQUESTED") {
        Some(ReviewState::ChangesRequested)
      } else if has("APPROVED") {
        Some(ReviewState::Approved)
      } else {
        None
      }
    }
  };
  state.filter(|s| !(pr.is_draft && *s == ReviewState::ReviewRequired))
}

/// GitHub's "WIP" status marks a draft-in-progress, not a check that runs.
pub fn is_inactive_check(check: &PrCheckEntry) -> bool {
  check.name.eq_ignore_ascii_case("WIP")
}

/// The checks the PR view counts: all but the inactive ones.
pub fn active_checks(checks: &[PrCheckEntry]) -> impl Iterator<Item = &PrCheckEntry> {
  checks.iter().filter(|c| !is_inactive_check(c))
}

fn is_failing(check: &PrCheckEntry) -> bool {
  matches!(check.bucket.as_str(), "fail" | "cancel")
}

fn is_running(check: &PrCheckEntry) -> bool {
  !matches!(
    check.bucket.as_str(),
    "pass" | "skipping" | "fail" | "cancel"
  )
}

/// The collapsed checks' one line: "16/17 passed · 1 failed · 1 running".
pub fn checks_summary(checks: &[PrCheckEntry]) -> String {
  let (mut total, mut passed, mut failed, mut running) = (0, 0, 0, 0);
  for check in active_checks(checks) {
    total += 1;
    if is_failing(check) {
      failed += 1;
    } else if is_running(check) {
      running += 1;
    } else {
      passed += 1;
    }
  }
  let mut line = format!("{passed}/{total} passed");
  if failed > 0 {
    line.push_str(&format!(" · {failed} failed"));
  }
  if running > 0 {
    line.push_str(&format!(" · {running} running"));
  }
  line
}

/// The checks the collapsed section still lists: at most three, failing
/// ones first, then running ones.
pub fn collapsed_checks(checks: &[PrCheckEntry]) -> Vec<&PrCheckEntry> {
  let active: Vec<_> = active_checks(checks).collect();
  let failing = active.iter().copied().filter(|c| is_failing(c));
  let running = active.iter().copied().filter(|c| is_running(c));
  failing.chain(running).take(3).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn reviewed(draft: bool, decision: Option<&str>, reviews: &[&str]) -> GhPullRequest {
    serde_json::from_value(serde_json::json!({
      "number": 1, "title": "t", "state": "OPEN", "url": "", "body": null,
      "author": {"login": "a"}, "labels": [], "headRefName": "h", "baseRefName": "main",
      "mergeStateStatus": null, "createdAt": "", "updatedAt": "", "comments": null,
      "isDraft": draft, "reviewDecision": decision,
      "latestReviews": reviews.iter().map(|s| serde_json::json!({
        "author": {"login": "r"}, "state": s
      })).collect::<Vec<_>>(),
    }))
    .unwrap()
  }

  #[test]
  fn review_state_follows_githubs_decision() {
    assert_eq!(
      review_state(&reviewed(false, Some("APPROVED"), &[])),
      Some(ReviewState::Approved)
    );
    assert_eq!(
      review_state(&reviewed(false, Some("CHANGES_REQUESTED"), &["APPROVED"])),
      Some(ReviewState::ChangesRequested)
    );
    assert_eq!(
      review_state(&reviewed(false, Some("REVIEW_REQUIRED"), &[])),
      Some(ReviewState::ReviewRequired)
    );
  }

  #[test]
  fn review_state_reads_latest_reviews_without_branch_protection() {
    assert_eq!(review_state(&reviewed(false, None, &[])), None);
    assert_eq!(
      review_state(&reviewed(false, Some(""), &["APPROVED"])),
      Some(ReviewState::Approved)
    );
    assert_eq!(
      review_state(&reviewed(false, None, &["APPROVED", "CHANGES_REQUESTED"])),
      Some(ReviewState::ChangesRequested),
      "a change request outweighs an approval"
    );
    assert_eq!(
      review_state(&reviewed(false, None, &["DISMISSED", "COMMENTED"])),
      None,
      "a dismissed approval no longer counts"
    );
  }

  #[test]
  fn review_state_leaves_drafts_unrequested() {
    assert_eq!(
      review_state(&reviewed(true, Some("REVIEW_REQUIRED"), &[])),
      None
    );
    assert_eq!(
      review_state(&reviewed(true, Some("APPROVED"), &[])),
      Some(ReviewState::Approved)
    );
  }

  fn check(name: &str, bucket: &str) -> PrCheckEntry {
    PrCheckEntry {
      name: name.into(),
      bucket: bucket.into(),
      link: String::new(),
      duration_secs: None,
    }
  }

  #[test]
  fn checks_summary_leaves_out_the_wip_status() {
    let checks = [
      check("build", "pass"),
      check("test", "pending"),
      check("WIP", "pending"),
      check("lint", "fail"),
    ];
    assert_eq!(checks_summary(&checks), "1/3 passed · 1 failed · 1 running");
    assert_eq!(checks_summary(&checks[..1]), "1/1 passed");
  }

  #[test]
  fn collapsed_checks_show_up_to_three_failing_then_running() {
    let checks = [
      check("a", "pending"),
      check("b", "pass"),
      check("WIP", "pending"),
      check("c", "fail"),
      check("d", "pending"),
      check("e", "pending"),
    ];
    let names: Vec<_> = collapsed_checks(&checks)
      .iter()
      .map(|c| c.name.as_str())
      .collect();
    assert_eq!(names, ["c", "a", "d"]);
    assert!(collapsed_checks(&[check("b", "pass")]).is_empty());
  }

  fn comment_at(id: &str, at: &str) -> GhIssueComment {
    GhIssueComment {
      id: id.into(),
      body: String::new(),
      author: GhAuthor {
        login: "c".into(),
        avatar_url: None,
      },
      created_at: at.into(),
      reactions: vec![],
    }
  }

  fn review_at(login: &str, state: &str, at: &str) -> GhReview {
    GhReview {
      author: GhAuthor {
        login: login.into(),
        avatar_url: None,
      },
      state: state.into(),
      submitted_at: at.into(),
    }
  }

  #[test]
  fn timeline_puts_verdicts_among_comments_by_time() {
    let comments = [
      comment_at("c1", "2026-08-01T10:00:00Z"),
      comment_at("c2", "2026-08-03T10:00:00Z"),
    ];
    let reviews = [
      review_at("alice", "APPROVED", "2026-08-02T10:00:00Z"),
      review_at("bob", "COMMENTED", "2026-08-02T11:00:00Z"),
      review_at("carol", "CHANGES_REQUESTED", "2026-08-04T10:00:00Z"),
      review_at("dave", "DISMISSED", "2026-08-04T11:00:00Z"),
    ];
    let order: Vec<String> = timeline(&comments, &reviews)
      .iter()
      .map(|item| match item {
        TimelineItem::Comment(c) => c.id.clone(),
        TimelineItem::Review(r) => format!("{} {}", r.author.login, r.state),
      })
      .collect();
    assert_eq!(
      order,
      ["c1", "alice APPROVED", "c2", "carol CHANGES_REQUESTED"]
    );
  }

  #[test]
  fn review_event_names_the_verdict() {
    assert_eq!(
      review_at("a", "APPROVED", "").verdict(),
      Some("approved these changes")
    );
    assert_eq!(
      review_at("a", "CHANGES_REQUESTED", "").verdict(),
      Some("requested changes")
    );
    assert_eq!(review_at("a", "COMMENTED", "").verdict(), None);
  }

  #[test]
  fn avatar_url_falls_back_to_the_github_profile_image() {
    let mut author = GhAuthor {
      login: "alice".into(),
      avatar_url: None,
    };
    assert_eq!(avatar_url(&author), "https://github.com/alice.png?size=64");
    author.avatar_url = Some("https://avatars.example/a.png".into());
    assert_eq!(avatar_url(&author), "https://avatars.example/a.png");
  }
}
