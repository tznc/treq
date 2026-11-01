//! The Changes tab on a conflicted workspace (`review/conflict.test.tsx`):
//! the Conflicts section, the inline conflict cards and what a conflicted
//! file does not offer.

use gpui::TestAppContext;

use super::*;

/// A rebase conflict as jj writes it: Side #1 is the target, Side #2 the
/// workspace.
const SIDES: &str = "<<<<<<< Side #1 (Conflict 1 of 1)\nmain side\n||||||| Base\n# Test Repository\n=======\nworkspace side\n>>>>>>> Side #2 (Conflict 1 of 1)";
/// The target deleted the file the workspace changed.
const DELETED: &str = "<<<<<<< Side #1 (Conflict 1 of 1)\n||||||| Base\n# Test Repository\n=======\nworkspace side\n>>>>>>> Side #2 (Conflict 1 of 1)";

/// `README.md` conflicted with `content` and a line after it, next to
/// `a.rs` with a plain change. `regions: false` keeps the hunks but drops
/// their conflict regions; `hunks: false` drops the hunks.
fn conflict_host(content: &str, regions: bool, hunks: bool) -> FakeHost {
  let mut host = changes_host();
  let regions = if regions {
    crate::conflict_markers::parse_conflict_markers(content, "README.md")
  } else {
    Vec::new()
  };
  let mut lines: Vec<String> = content.lines().map(|l| format!("+{l}")).collect();
  lines.push(" after".into());
  let header = format!("@@ -1,1 +1,{} @@", lines.len());
  let readme = if hunks {
    serde_json::json!([{"id": "r", "header": header, "lines": lines, "patch": "", "conflict_regions": regions}])
  } else {
    serde_json::json!([])
  };
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [
      {"path": "README.md", "status": "M", "previous_path": null, "changed_line_count": 8, "diff_deferred": false},
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false}
    ],
    "conflicted_files": ["README.md"],
    "hunks_by_file": [
      {"path": "README.md", "hunks": readme},
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1,2 +1,3 @@", "lines": [" a", "+b", "+c"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

fn bounds(h: &mut Harness, selector: String) -> Option<gpui::Bounds<gpui::Pixels>> {
  h.cx.run_until_parked();
  h.cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}

const CARD: &str = "conflict-card-README.md-conflict-1";

#[gpui::test]
fn changes_tab_lists_no_conflicts_for_a_clean_workspace(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("file-row-a.rs"));
  assert!(!h.drawn("conflicts-section-toggle"));
}

#[gpui::test]
fn changes_tab_draws_a_conflict_card_with_base_and_both_sides(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, true), changes_route());
  assert!(h.drawn("conflicts-section-toggle"));
  assert!(h.drawn("conflict-row-README.md"));
  assert!(h.drawn(CARD));
  let role = |i: usize, role: &str| format!("conflict-line-README.md-conflict-1-{i}-{role}");
  for (i, side) in [(1, "Left"), (3, "Base"), (5, "Right")] {
    assert!(
      bounds(&mut h, role(i, side)).is_some(),
      "line {i} is {side}"
    );
  }
  let tops: Vec<_> = ["Side #1", "Base", "Side #2"]
    .into_iter()
    .map(|label| {
      bounds(&mut h, format!("conflict-label-{label}"))
        .unwrap_or_else(|| panic!("{label}"))
        .top()
    })
    .collect();
  assert!(tops[0] < tops[1] && tops[1] < tops[2], "{tops:?}");
  assert!(!h.drawn("conflict-deleted-side"));
}

#[gpui::test]
fn changes_tab_offers_no_line_comments_on_a_conflicted_file(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, true), changes_route());
  // The line after the region is drawn, without a comment button.
  assert!(h.drawn("gutter-README.md-0-7"));
  assert!(!h.drawn("add-comment-README.md-0-7"));
  assert!(h.drawn("add-comment-a.rs-0-1"), "other files keep theirs");
}

#[gpui::test]
fn changes_tab_invents_no_card_without_conflict_regions(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, false, true), changes_route());
  assert!(h.drawn("conflict-row-README.md"));
  assert!(h.drawn("file-card-README.md"));
  assert!(!h.drawn(CARD));
  assert!(h.drawn("gutter-README.md-0-0"), "the lines show as a diff");
}

#[gpui::test]
fn changes_tab_says_a_conflicted_file_without_hunks_has_no_diff(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, false), changes_route());
  assert!(h.drawn("conflict-row-README.md"));
  assert!(h.drawn("empty-diff-placeholder"));
  assert!(!h.drawn(CARD));
}

/// A conflict only in a commit stays on screen with committed changes
/// hidden.
#[gpui::test]
fn changes_tab_keeps_a_committed_conflict_with_committed_changes_hidden(cx: &mut TestAppContext) {
  let mut host = conflict_host(SIDES, true, true);
  if let Some(working) = &mut host.working {
    working.uncommitted_files.clear();
  }
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(h.drawn("conflict-row-README.md"));
  assert!(h.drawn(CARD));
  assert!(!h.drawn("file-card-a.rs"), "committed changes stay hidden");
}

#[gpui::test]
fn changes_tab_draws_the_deleted_side_of_a_delete_modify_conflict(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(DELETED, true, true), changes_route());
  assert!(h.drawn(CARD));
  assert!(h.drawn("conflict-deleted-side"));
}

/// The Conflicts section's dot and each conflicted file's dot say what
/// they mean.
#[gpui::test]
fn conflict_dots_are_named(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, true), changes_route());
  assert!(h.drawn("conflict-row-README.md"));
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  let dots = screen.get_all_by_role(by::role(gpui::Role::Image).name("Conflict"));
  assert!(dots.len() >= 2, "{}", dots.len());
}
