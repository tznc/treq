//! The rules of reviewing a pull request on the phone: the PR's unified
//! diff as files and hunks, where review threads sit, the pending comments
//! and the review they become (desktop's `pr_review_data.rs`).

use treq_lib::{
  core::{
    remote::GithubAction,
    review::{line_at, pending_for_selection, LineRef, Side},
  },
  github::{GhDraftReviewComment, GhReviewThread},
  jj::JjDiffHunk,
};

/// One file of the PR's diff.
#[derive(Debug, Clone, PartialEq)]
pub struct PrFile {
  pub path: String,
  pub hunks: Vec<JjDiffHunk>,
  pub additions: usize,
  pub deletions: usize,
}

/// `gh pr diff` output as files, in diff order.
pub fn parse_diff(diff: &str) -> Vec<PrFile> {
  let mut files: Vec<PrFile> = Vec::new();
  let mut in_hunk = false;
  for line in diff.split('\n') {
    if let Some(rest) = line.strip_prefix("diff --git ") {
      // `a/<path> b/<path>`: the new path, until `+++` names it exactly.
      let path = rest.rsplit_once(" b/").map_or(rest, |(_, b)| b).to_string();
      files.push(PrFile {
        path,
        hunks: Vec::new(),
        additions: 0,
        deletions: 0,
      });
      in_hunk = false;
      continue;
    }
    let Some(file) = files.last_mut() else {
      continue;
    };
    if line.starts_with("@@") {
      file.hunks.push(JjDiffHunk {
        id: format!("{}#{}", file.path, file.hunks.len()),
        header: line.to_string(),
        lines: Vec::new(),
        patch: String::new(),
        conflict_style: Default::default(),
        conflict_regions: Vec::new(),
      });
      in_hunk = true;
    } else if in_hunk && line.starts_with([' ', '+', '-']) {
      if line.starts_with('+') {
        file.additions += 1;
      } else if line.starts_with('-') {
        file.deletions += 1;
      }
      if let Some(h) = file.hunks.last_mut() {
        h.lines.push(line.to_string());
      }
    } else if !in_hunk {
      if let Some(new) = line.strip_prefix("+++ b/") {
        file.path = new.to_string();
      } else if let Some(old) = line.strip_prefix("--- a/") {
        file.path = old.to_string();
      }
    }
  }
  files
}

/// GitHub's `diffSide` for a side.
pub fn side_name(side: Side) -> &'static str {
  match side {
    Side::Old => "LEFT",
    Side::New => "RIGHT",
  }
}

/// A comment written for the pending review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingComment {
  pub path: String,
  pub start_line: u32,
  pub end_line: u32,
  pub side: Side,
  /// The commented diff lines, with their `+`/`-`/` ` prefix.
  pub lines: Vec<String>,
  pub body: String,
}

/// The pending comment for `picked` lines of a file: `None` when they mix
/// removed lines with others, which one side cannot describe.
pub fn pending_for(path: &str, hunks: &[JjDiffHunk], picked: &[LineRef]) -> Option<PendingComment> {
  let p = pending_for_selection(path, hunks, picked)?;
  Some(PendingComment {
    path: p.file_path,
    start_line: p.start_line,
    end_line: p.end_line,
    side: p.side,
    lines: p.line_content,
    body: String::new(),
  })
}

/// Whether tapping `at` may extend a pick anchored at `anchor`: the same
/// hunk and the same side.
pub fn extends(hunks: &[JjDiffHunk], anchor: LineRef, at: LineRef) -> bool {
  anchor.0 == at.0
    && hunks
      .get(at.0)
      .is_some_and(|h| line_at(h, anchor.1).1 == line_at(h, at.1).1)
}

/// A ```suggestion block holding `lines` (diff lines) as they read now.
pub fn suggestion(lines: &[String]) -> String {
  let body: Vec<&str> = lines.iter().map(|l| l.get(1..).unwrap_or("")).collect();
  format!("```suggestion\n{}\n```", body.join("\n"))
}

pub fn to_draft(c: &PendingComment) -> GhDraftReviewComment {
  GhDraftReviewComment {
    path: c.path.clone(),
    line: u64::from(c.end_line),
    side: side_name(c.side).into(),
    start_line: (c.start_line < c.end_line).then_some(u64::from(c.start_line)),
    body: c.body.clone(),
  }
}

/// The review's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Event {
  #[default]
  Comment,
  Approve,
  RequestChanges,
}

impl Event {
  pub const ALL: [Event; 3] = [Event::Comment, Event::Approve, Event::RequestChanges];

  pub fn value(self) -> &'static str {
    match self {
      Event::Comment => "COMMENT",
      Event::Approve => "APPROVE",
      Event::RequestChanges => "REQUEST_CHANGES",
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      Event::Comment => "Comment",
      Event::Approve => "Approve",
      Event::RequestChanges => "Request changes",
    }
  }

  pub fn hint(self) -> &'static str {
    match self {
      Event::Comment => "Submit general feedback without explicit approval.",
      Event::Approve => "Submit feedback and approve merging these changes.",
      Event::RequestChanges => "Submit feedback that must be addressed before merging.",
    }
  }
}

/// Requesting changes needs a summary or a comment.
pub fn can_submit(event: Event, body: &str, pending: usize) -> bool {
  event != Event::RequestChanges || !body.trim().is_empty() || pending > 0
}

/// The host write submitting the review with every pending comment.
pub fn submit_action(
  number: u64,
  event: Event,
  body: &str,
  pending: &[PendingComment],
) -> GithubAction {
  GithubAction::SubmitPrReview {
    number,
    event: event.value().into(),
    body: body.into(),
    comments: pending.iter().map(to_draft).collect(),
  }
}

fn at_line(t: &GhReviewThread, number: u32, side: Side) -> bool {
  t.line == Some(i64::from(number)) && t.diff_side == side_name(side)
}

/// Threads anchored at a diff line: current, on `path`, at `number` on `side`.
pub fn threads_at<'a>(
  threads: &'a [GhReviewThread],
  path: &str,
  number: u32,
  side: Side,
) -> Vec<&'a GhReviewThread> {
  threads
    .iter()
    .filter(|t| !t.is_outdated && t.path == path && at_line(t, number, side))
    .collect()
}

/// Threads on `path` that sit at no line of `hunks`: outdated ones, or
/// lines outside the diff. Shown above the diff.
pub fn detached_threads<'a>(
  threads: &'a [GhReviewThread],
  path: &str,
  hunks: &[JjDiffHunk],
) -> Vec<&'a GhReviewThread> {
  let placed = |t: &GhReviewThread| {
    hunks.iter().any(|h| {
      (0..h.lines.len()).any(|l| {
        let (number, side) = line_at(h, l);
        at_line(t, number, side)
      })
    })
  };
  threads
    .iter()
    .filter(|t| t.path == path && (t.is_outdated || !placed(t)))
    .collect()
}

/// How many threads are on `path`.
pub fn thread_count(threads: &[GhReviewThread], path: &str) -> usize {
  threads.iter().filter(|t| t.path == path).count()
}

/// Whether a thread starts collapsed: resolved or outdated.
pub fn collapsed(thread: &GhReviewThread) -> bool {
  thread.is_resolved || thread.is_outdated
}

#[cfg(test)]
mod tests {
  use super::*;
  use treq_lib::github::{GhAuthor, GhReviewComment};

  const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
index 1..2 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@
 one
-two
+deux
+trois
 four
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-gone
";

  fn thread(path: &str, line: Option<i64>, side: &str, outdated: bool) -> GhReviewThread {
    GhReviewThread {
      id: format!("{path}:{line:?}"),
      is_resolved: false,
      is_outdated: outdated,
      path: path.into(),
      line,
      diff_side: side.into(),
      comments: vec![GhReviewComment {
        id: "c".into(),
        body: "hi".into(),
        author: GhAuthor {
          login: "a".into(),
          avatar_url: None,
        },
        created_at: String::new(),
        diff_hunk: String::new(),
        url: String::new(),
        reactions: Vec::new(),
      }],
    }
  }

  #[core::prelude::v1::test]
  fn parses_files_hunks_and_counts() {
    let files = parse_diff(DIFF);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "src/a.rs");
    assert_eq!((files[0].additions, files[0].deletions), (2, 1));
    assert_eq!(files[0].hunks.len(), 1);
    assert_eq!(
      files[0].hunks[0].lines,
      [" one", "-two", "+deux", "+trois", " four"]
    );
    assert_eq!(files[1].path, "old.txt");
    assert_eq!((files[1].additions, files[1].deletions), (0, 1));
    assert!(parse_diff("").is_empty());
  }

  #[core::prelude::v1::test]
  fn picks_stay_in_one_hunk_and_side() {
    let hunks = parse_diff(DIFF).remove(0).hunks;
    assert!(extends(&hunks, (0, 2), (0, 3)));
    assert!(extends(&hunks, (0, 0), (0, 4)));
    assert!(!extends(&hunks, (0, 1), (0, 2)));
    let c = pending_for("src/a.rs", &hunks, &[(0, 2), (0, 3)]).unwrap();
    assert_eq!((c.start_line, c.end_line, c.side), (2, 3, Side::New));
    assert_eq!(c.lines, ["+deux", "+trois"]);
    let old = pending_for("src/a.rs", &hunks, &[(0, 1)]).unwrap();
    assert_eq!((old.start_line, old.side), (2, Side::Old));
    assert!(pending_for("src/a.rs", &hunks, &[(0, 1), (0, 2)]).is_none());
  }

  #[core::prelude::v1::test]
  fn suggestion_strips_diff_prefixes() {
    assert_eq!(
      suggestion(&["+deux".into(), " four".into()]),
      "```suggestion\ndeux\nfour\n```"
    );
  }

  #[core::prelude::v1::test]
  fn pending_comments_become_one_review() {
    let one = PendingComment {
      path: "p".into(),
      start_line: 4,
      end_line: 4,
      side: Side::Old,
      lines: vec![],
      body: "x".into(),
    };
    let range = PendingComment {
      start_line: 2,
      end_line: 5,
      side: Side::New,
      ..one.clone()
    };
    assert_eq!(
      submit_action(7, Event::Approve, "ok", &[one, range]),
      GithubAction::SubmitPrReview {
        number: 7,
        event: "APPROVE".into(),
        body: "ok".into(),
        comments: vec![
          GhDraftReviewComment {
            path: "p".into(),
            line: 4,
            side: "LEFT".into(),
            start_line: None,
            body: "x".into()
          },
          GhDraftReviewComment {
            path: "p".into(),
            line: 5,
            side: "RIGHT".into(),
            start_line: Some(2),
            body: "x".into()
          },
        ]
      }
    );
  }

  #[core::prelude::v1::test]
  fn request_changes_needs_words() {
    assert!(can_submit(Event::Comment, "", 0));
    assert!(can_submit(Event::Approve, "", 0));
    assert!(!can_submit(Event::RequestChanges, "  ", 0));
    assert!(can_submit(Event::RequestChanges, "fix", 0));
    assert!(can_submit(Event::RequestChanges, "", 1));
  }

  #[core::prelude::v1::test]
  fn threads_sit_at_their_line_or_above_the_diff() {
    let hunks = parse_diff(DIFF).remove(0).hunks;
    let threads = vec![
      thread("src/a.rs", Some(3), "RIGHT", false),
      thread("src/a.rs", Some(2), "LEFT", false),
      thread("src/a.rs", Some(3), "RIGHT", true),
      thread("src/a.rs", Some(90), "RIGHT", false),
      thread("src/a.rs", None, "RIGHT", false),
      thread("other", Some(3), "RIGHT", false),
    ];
    assert_eq!(threads_at(&threads, "src/a.rs", 3, Side::New).len(), 1);
    assert_eq!(threads_at(&threads, "src/a.rs", 2, Side::Old).len(), 1);
    assert!(threads_at(&threads, "src/a.rs", 2, Side::New).is_empty());
    assert_eq!(detached_threads(&threads, "src/a.rs", &hunks).len(), 3);
    assert_eq!(thread_count(&threads, "src/a.rs"), 5);
    assert!(collapsed(&threads[2]));
    assert!(!collapsed(&threads[0]));
    let mut resolved = threads[0].clone();
    resolved.is_resolved = true;
    assert!(collapsed(&resolved));
    assert_eq!(side_name(Side::Old), "LEFT");
  }
}
