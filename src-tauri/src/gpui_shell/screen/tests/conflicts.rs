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

/// The conflict card the user sees for README.md's only conflict.
const CARD: &str = "Conflict 1 of 1";

/// Whether the Conflicts section lists `path`, marked as a conflict.
fn lists_conflict(h: &mut Harness, path: &str) -> bool {
  let screen = h.rendered();
  screen
    .query_all_by_role(gpui_testing_library::by::role(gpui::Role::Button).name(path))
    .iter()
    .any(|row| {
      gpui_testing_library::within(row)
        .query_by_role(gpui_testing_library::by::role(gpui::Role::Image).name("Conflict"))
        .is_some()
    })
}

/// Whether the user sees the notice that `side` deleted the file.
fn shows_deleted(h: &mut Harness, side: &str) -> bool {
  h.shows(gpui::Role::Label, &format!("{side} deleted this file"))
}

/// Whether line `name` of a diff offers a comment button.
fn offers_comment(h: &mut Harness, name: &str) -> bool {
  h.rendered()
    .query_by_role(
      gpui_testing_library::by::role(gpui::Role::Button)
        .name(name)
        .hidden(true),
    )
    .is_some()
}

#[gpui::test]
fn changes_tab_lists_no_conflicts_for_a_clean_workspace(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.shows(gpui::Role::Button, "a.rs"));
  assert!(!h.shows(gpui::Role::Button, "Collapse Conflicts"));
}

#[gpui::test]
fn changes_tab_draws_a_conflict_card_with_base_and_both_sides(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, true), changes_route());
  assert!(h.shows(gpui::Role::Button, "Collapse Conflicts"));
  assert!(lists_conflict(&mut h, "README.md"));
  assert!(h.shows(gpui::Role::Group, CARD));
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
  assert!(!shows_deleted(&mut h, "Side #1"));
  assert!(!shows_deleted(&mut h, "Side #2"));
}

#[gpui::test]
fn changes_tab_offers_no_line_comments_on_a_conflicted_file(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, true), changes_route());
  // The line after the region is drawn, without a comment button.
  assert!(h.shows(gpui::Role::Row, " after"));
  assert!(!offers_comment(&mut h, "Add comment on README.md line 8"));
  assert!(
    offers_comment(&mut h, "Add comment on a.rs line 2"),
    "other files keep theirs"
  );
}

#[gpui::test]
fn changes_tab_invents_no_card_without_conflict_regions(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, false, true), changes_route());
  assert!(lists_conflict(&mut h, "README.md"));
  assert!(h.shows(gpui::Role::Heading, "README.md"));
  assert!(!h.shows(gpui::Role::Group, CARD));
  assert!(
    h.shows(gpui::Role::Row, "+<<<<<<< Side #1 (Conflict 1 of 1)"),
    "the lines show as a diff"
  );
}

#[gpui::test]
fn changes_tab_says_a_conflicted_file_without_hunks_has_no_diff(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, false), changes_route());
  assert!(lists_conflict(&mut h, "README.md"));
  assert!(h.shows(gpui::Role::Status, "No diff for this conflicted file"));
  assert!(!h.shows(gpui::Role::Group, CARD));
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
  assert!(lists_conflict(&mut h, "README.md"));
  assert!(h.shows(gpui::Role::Group, CARD));
  assert!(
    !h.shows(gpui::Role::Heading, "a.rs"),
    "committed changes stay hidden"
  );
}

#[gpui::test]
fn changes_tab_draws_the_deleted_side_of_a_delete_modify_conflict(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, conflict_host(DELETED, true, true), changes_route());
  assert!(h.shows(gpui::Role::Group, CARD));
  assert!(shows_deleted(&mut h, "Side #1"));
}

/// The Conflicts section's dot and each conflicted file's dot say what
/// they mean.
#[gpui::test]
fn conflict_dots_are_named(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(cx, conflict_host(SIDES, true, true), changes_route());
  assert!(lists_conflict(&mut h, "README.md"));
  let window = h.cx.update(|window, _| window.window_handle());
  let screen = gpui_testing_library::Screen::new(&h.cx, window);
  screen.redraw();
  let dots = screen.get_all_by_role(by::role(gpui::Role::Image).name("Conflict"));
  assert!(dots.len() >= 2, "{}", dots.len());
}
