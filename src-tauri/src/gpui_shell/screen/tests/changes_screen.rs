use gpui::{Modifiers, MouseButton, TestAppContext};

use super::*;

#[gpui::test]
fn changes_tab_lists_files_and_their_diffs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  for (role, name) in [
    (gpui::Role::Group, "Changes"),
    (gpui::Role::Button, "a.rs"),
    (gpui::Role::Button, "b.rs"),
    (gpui::Role::Group, "a.rs"),
    (gpui::Role::Group, "b.rs"),
    (gpui::Role::Button, "Commit"),
  ] {
    assert!(h.shows(role, name), "{name}");
  }
  assert!(!h.shows(gpui::Role::Group, "Code"));
  assert!(host.calls().contains(&"read workspace diff 7".to_string()));
  assert!(
    host.calls().iter().any(|c| c.starts_with("read hunks")),
    "{:?}",
    host.calls()
  );
}

fn changes_host_with_generated() -> FakeHost {
  let mut host = changes_host();
  host.gitattributes = "gen/** linguist-generated\n".into();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false},
      {"path": "Cargo.lock", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false},
      {"path": "gen/api.ts", "status": "A", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1,2 +1,3 @@", "lines": [" a", "+b", "+c"], "patch": ""}]},
      {"path": "Cargo.lock", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-x", "+y"], "patch": ""}]},
      {"path": "gen/api.ts", "hunks": [{"id": "h", "header": "@@ -0,0 +1,1 @@", "lines": ["+z"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

#[gpui::test]
fn changes_tab_waits_to_read_generated_diffs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  assert!(h.shows(gpui::Role::Group, "Cargo.lock"));
  assert!(offers_load_diff(&mut h, "Cargo.lock"));
  assert!(offers_load_diff(&mut h, "gen/api.ts"));
  assert!(!offers_load_diff(&mut h, "a.rs"));
  let reads: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read hunks"))
    .collect();
  assert_eq!(reads, vec!["read hunks [\"a.rs\"]".to_string()]);
}

#[gpui::test]
fn changes_tab_loads_a_generated_diff_on_request(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  h.click("load-generated-Cargo.lock");
  h.cx.run_until_parked();
  assert!(host
    .calls()
    .contains(&"read hunks [\"Cargo.lock\"]".to_string()));
  assert!(
    !offers_load_diff(&mut h, "Cargo.lock"),
    "the diff shows instead"
  );
  assert!(offers_load_diff(&mut h, "gen/api.ts"));
}

/// Whether the banner says files changed under the review.
fn shows_stale_banner(h: &mut Harness) -> bool {
  h.rendered()
    .query_by_role(gpui_testing_library::by::role(gpui::Role::Status).name(
      gpui_testing_library::TextMatch::regex(" changed since you started reviewing$"),
    ))
    .is_some()
}

/// Whether a group of comments named like `group` holds a comment.
fn has_comment(h: &mut Harness, group: &str) -> bool {
  use gpui_testing_library::by;
  let screen = h.rendered();
  screen
    .query_all_by_role(
      by::role(gpui::Role::Group).name(gpui_testing_library::TextMatch::regex(group)),
    )
    .iter()
    .any(|g| {
      gpui_testing_library::within(g)
        .query_by_role(by::role(gpui::Role::Button).name("Edit comment"))
        .is_some()
    })
}

/// Whether a button named `name` is drawn, shown or waiting for hover.
fn shows_hidden(h: &mut Harness, name: &str) -> bool {
  h.rendered()
    .query_by_role(
      gpui_testing_library::by::role(gpui::Role::Button)
        .name(name)
        .hidden(true),
    )
    .is_some()
}

/// Whether `path`'s card offers "Load diff" for its deferred diff.
fn offers_load_diff(h: &mut Harness, path: &str) -> bool {
  use gpui_testing_library::by;
  let screen = h.rendered();
  screen
    .query_all_by_role(by::role(gpui::Role::Group).name(path))
    .iter()
    .any(|card| {
      gpui_testing_library::within(card)
        .query_by_role(by::role(gpui::Role::Button).name("Load diff"))
        .is_some()
    })
}

/// Clicks the filter button and `item` in its menu.
fn filter(h: &mut Harness, item: &str) {
  h.click("changes-filter");
  h.pick(item);
}

#[gpui::test]
fn changes_tab_hides_generated_files_from_the_list_and_diff(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  filter(&mut h, "Generated files");
  assert!(h.shows(gpui::Role::Button, "a.rs"));
  assert!(h.shows(gpui::Role::Group, "a.rs"));
  for (role, hidden) in [
    (gpui::Role::Button, "Cargo.lock"),
    (gpui::Role::Group, "Cargo.lock"),
    (gpui::Role::Button, "gen/api.ts"),
  ] {
    assert!(!h.shows(role, hidden), "{hidden}");
  }
  filter(&mut h, "Generated files");
  assert!(h.shows(gpui::Role::Button, "Cargo.lock"));
  assert!(h.shows(gpui::Role::Group, "gen/api.ts"));
}

#[gpui::test]
fn changes_tab_keeps_a_selected_generated_file_listed(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  h.click("file-row-Cargo.lock");
  h.click("stage-Cargo.lock");
  filter(&mut h, "Generated files");
  assert!(shows_hidden(&mut h, "Unstage Cargo.lock"));
  assert!(!h.shows(gpui::Role::Button, "gen/api.ts"));
}

#[gpui::test]
fn changes_tab_offers_the_filter_only_with_something_to_filter(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.shows(gpui::Role::Button, "a.rs"));
  assert!(!h.shows(gpui::Role::Button, "Filter changes"));
}

fn committed_route(show: bool) -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "changes".into();
  route["showCommitted"] = show.into();
  serde_json::from_value(route).expect("changes route")
}

fn changes_host_with_committed() -> FakeHost {
  let mut host = changes_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [
      {"path": "c.rs", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-a", "+b"], "patch": ""}]},
      {"path": "c.rs", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-c", "+d"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

#[gpui::test]
fn changes_tab_hides_the_committed_group_from_its_checkbox(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(true));
  assert!(h.shows(gpui::Role::Button, "Collapse Committed"));
  assert!(h.shows(gpui::Role::Group, "c.rs"));
  assert!(
    !h.shows(gpui::Role::Button, "Filter changes"),
    "no filter button for committed"
  );
  h.click("committed-hide");
  assert_eq!(
    fields(&h, "changes.showCommitted"),
    vec![serde_json::json!(false)]
  );
}

#[gpui::test]
fn changes_tab_hides_the_committed_group_when_filtered_out(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(false));
  assert!(h.shows(gpui::Role::Button, "a.rs"));
  assert!(!h.shows(gpui::Role::Button, "Filter changes"));
  assert!(
    h.shows(gpui::Role::Button, "Expand Committed"),
    "the header stays to unhide"
  );
  assert!(!h.shows(gpui::Role::Button, "c.rs"));
  assert!(!h.shows(gpui::Role::Group, "c.rs"));
}

fn changes_host_committed_only() -> FakeHost {
  let mut host = changes_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [
      {"path": "c.rs", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "uncommitted_files": [],
    "hunks_by_file": [
      {"path": "c.rs", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-c", "+d"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

/// With nothing uncommitted there is nothing to commit: no commit box, one
/// muted line where the Changes group would be.
#[gpui::test]
fn changes_tab_offers_no_commit_box_with_nothing_to_commit(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_committed_only(), committed_route(true));
  assert!(h.shows(gpui::Role::Button, "Collapse Committed"));
  assert!(!h.shows(gpui::Role::Button, "Commit"));
  assert!(!h.shows(gpui::Role::MultilineTextInput, "Message"));
  assert!(h.shows(gpui::Role::Status, "Nothing to commit"));
}

/// The commit box sits in the Changes group: under its header, above its
/// files.
#[gpui::test]
fn changes_tab_draws_the_commit_box_inside_the_changes_group(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(true));
  assert!(!h.shows(gpui::Role::Status, "Nothing to commit"));
  let mut top = |s: &'static str| {
    h.cx.run_until_parked();
    h.cx
      .debug_bounds(s)
      .unwrap_or_else(|| panic!("{s} not drawn"))
      .origin
      .y
  };
  let header = top("changes-section-toggle");
  let message = top("commit-message");
  let file = top("file-row-a.rs");
  let committed = top("committed-section-toggle");
  assert!(header < message, "header above the commit box");
  assert!(message < file, "commit box above the group's files");
  assert!(file < committed, "the Committed group follows");
}

#[gpui::test]
fn changes_tab_asks_for_a_commit_message(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("commit-message");
  h.cx.simulate_keystrokes("secondary-enter");
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Alert, "Enter a commit message."));
  assert!(!host.writes().iter().any(|w| w.starts_with("commit")));
}

#[gpui::test]
fn changes_tab_commits_the_working_copy(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("commit-message");
  h.cx.simulate_input("Fix");
  h.click("commit-button");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"commit Some(7) \"Fix\"".to_string()),
    "{:?}",
    host.writes()
  );
}

/// A long name and path truncate before the row's action buttons.
#[gpui::test]
fn changes_tab_keeps_file_actions_inside_a_long_row(cx: &mut TestAppContext) {
  let path = "src/some/deeply/nested/directory/tree/a_really_long_file_name_for_testing.rs";
  let mut host = changes_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [
      {"path": path, "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": path, "hunks": [{"id": "h", "header": "@@ -1,1 +1,1 @@", "lines": ["+a"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
  h.click(leak(format!("file-row-{path}")));
  h.cx.run_until_parked();
  let row = h
    .cx
    .debug_bounds(leak(format!("file-row-{path}")))
    .expect("row");
  for action in ["discard", "stage"] {
    let b = h
      .cx
      .debug_bounds(leak(format!("{action}-{path}")))
      .unwrap_or_else(|| panic!("{action} not drawn"));
    assert!(
      b.right() <= row.right(),
      "{action} {b:?} past the row {row:?}"
    );
  }
}

/// An unselected row draws its discard and stage buttons too, shown on hover.
#[gpui::test]
fn changes_tab_offers_file_actions_on_unselected_rows(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  h.click("file-row-a.rs");
  h.cx.run_until_parked();
  for name in ["Discard changes to b.rs", "Stage b.rs"] {
    assert!(shows_hidden(&mut h, name), "{name}");
  }
}

#[gpui::test]
fn changes_tab_commits_only_staged_files(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("file-row-a.rs");
  h.click("stage-a.rs");
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Button, "Unstage a.rs"));
  h.click("commit-message");
  h.cx.simulate_input("Part");
  h.click("commit-button");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .contains(&"split \"Part\" [\"a.rs\"]".to_string()),
    "{:?}",
    host.writes()
  );
}

#[gpui::test]
fn changes_tab_marks_files_viewed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("viewed-a.rs");
  h.cx.run_until_parked();
  assert!(
    host
      .writes()
      .iter()
      .any(|w| w.starts_with("mark viewed a.rs ")),
    "{:?}",
    host.writes()
  );
  h.click("viewed-a.rs");
  h.cx.run_until_parked();
  assert!(host.writes().contains(&"unmark viewed a.rs".to_string()));
}

/// Viewed marks are kept, so reopening the tab (or the app) shows them
/// again, except on a file whose diff changed since.
#[gpui::test]
fn reopening_the_tab_restores_viewed_files(cx: &mut TestAppContext) {
  let mut host = changes_host();
  let hunks = host.working.as_ref().expect("working").hunks_by_file[0]
    .hunks
    .clone();
  host.viewed = vec![
    (
      "a.rs".into(),
      crate::gpui_shell::screen::changes_data::hunks_hash(&hunks),
    ),
    ("b.rs".into(), "stale".into()),
  ];
  if let Some(diff) = host.working.as_mut() {
    let mut b = diff.uncommitted_files[0].clone();
    b.path = "b.rs".into();
    diff.uncommitted_files.push(b);
    let mut b = diff.hunks_by_file[0].clone();
    b.path = "b.rs".into();
    diff.hunks_by_file.push(b);
  }
  let (h, host) = workspace_harness(cx, host, changes_route());
  h.cx.run_until_parked();
  let viewed = h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    let mut viewed: Vec<String> = changes.viewed.keys().cloned().collect();
    viewed.sort();
    viewed
  });
  assert_eq!(viewed, vec!["a.rs".to_string()], "{:?}", host.writes());
  assert!(
    host.writes().contains(&"unmark viewed b.rs".to_string()),
    "{:?}",
    host.writes()
  );
}

#[gpui::test]
fn a_viewed_committed_file_stays_viewed_across_reloads(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.working = host.working.map(|mut diff| {
    let committed: crate::jj::JjRevisionDiff = serde_json::from_value(serde_json::json!({
      "committed_files": [
        {"path": "c.rs", "status": "M", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
      ],
      "hunks_by_file": [
        {"path": "c.rs", "hunks": [{"id": "h", "header": "@@ -1 +1 @@", "lines": ["-x", "+y"], "patch": ""}]}
      ],
      "too_large_to_render": false
    }))
    .expect("committed diff");
    diff.committed_files = committed.committed_files;
    diff.hunks_by_file.extend(committed.hunks_by_file);
    diff
  });
  let mut route = changes_route();
  if let View::Workspace(r) = &mut route.view {
    r.show_committed = Some(true);
  }
  let (mut h, host) = workspace_harness(cx, host, route);
  h.click("viewed-c.rs");
  h.cx.run_until_parked();
  edit_a_rs(&mut h, &host);
  let viewed = h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    changes.viewed.contains_key("c.rs")
  });
  assert!(viewed, "{:?}", host.writes());
  assert!(!host.writes().contains(&"unmark viewed c.rs".to_string()));
}

/// The committed card's "Show more lines" reads the working copy, which
/// a file with no uncommitted edits shares with the branch head.
#[gpui::test]
fn show_more_lines_expands_a_committed_file(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.working = host.working.map(|mut diff| {
    let committed: crate::jj::JjRevisionDiff = serde_json::from_value(serde_json::json!({
      "committed_files": [
        {"path": "c.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false}
      ],
      "hunks_by_file": [
        {"path": "c.rs", "hunks": [
          {"id": "h0", "header": "@@ -1,4 +1,4 @@", "lines": ["-a", "+a", " b", " c", " d"], "patch": ""},
          {"id": "h1", "header": "@@ -30,4 +30,4 @@", "lines": [" a", " b", " c", "-d", "+d"], "patch": ""}
        ]}
      ],
      "too_large_to_render": false
    }))
    .expect("committed diff");
    diff.committed_files = committed.committed_files;
    diff.hunks_by_file.extend(committed.hunks_by_file);
    diff
  });
  let mut route = changes_route();
  if let View::Workspace(r) = &mut route.view {
    r.show_committed = Some(true);
  }
  let (mut h, host) = workspace_harness(cx, host, route);
  h.click("expand-after-c.rs-0");
  h.cx.run_until_parked();
  assert!(
    host.calls().contains(&"read lines c.rs 5..24".to_string()),
    "{:?}",
    host.calls()
  );
  let read = h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    changes
      .context
      .get(&("c.rs".to_string(), 0, super::changes::Dir::After))
      .cloned()
      .unwrap_or_default()
  });
  assert_eq!(read.first().map(String::as_str), Some("line 5"));
  assert_eq!(read.len(), 20);
}

fn committed_gap_host(second: &str) -> FakeHost {
  let mut host = changes_host();
  let second = second.to_string();
  host.working = host.working.map(|mut diff| {
    let committed: crate::jj::JjRevisionDiff = serde_json::from_value(serde_json::json!({
      "committed_files": [
        {"path": "c.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false}
      ],
      "hunks_by_file": [
        {"path": "c.rs", "hunks": [
          {"id": "h0", "header": "@@ -1,4 +1,4 @@", "lines": ["-a", "+a", " b", " c", " d"], "patch": ""},
          {"id": "h1", "header": second, "lines": [" a", " b", " c", "-d", "+d"], "patch": ""}
        ]}
      ],
      "too_large_to_render": false
    }))
    .expect("committed diff");
    diff.committed_files = committed.committed_files;
    diff.hunks_by_file.extend(committed.hunks_by_file);
    diff
  });
  host
}

fn show_committed_route() -> ScreenSnapshot {
  let mut route = changes_route();
  if let View::Workspace(r) = &mut route.view {
    r.show_committed = Some(true);
  }
  route
}

/// A gap of at most [`EXPAND_ALL_MAX`] lines offers "Expand all", which
/// reads the whole gap in one click and closes it.
#[gpui::test]
fn expand_all_fills_a_small_gap_between_hunks(cx: &mut TestAppContext) {
  let host = committed_gap_host("@@ -30,4 +30,4 @@");
  let (mut h, host) = workspace_harness(cx, host, show_committed_route());
  h.click("expand-all-c.rs-0");
  h.cx.run_until_parked();
  assert!(
    host.calls().contains(&"read lines c.rs 5..29".to_string()),
    "{:?}",
    host.calls()
  );
  assert!(!h.shows(gpui::Role::Button, "Show more lines of c.rs from line 5"));
  assert!(!h.shows(
    gpui::Role::Button,
    "Expand all 25 lines of c.rs from line 5"
  ));
}

/// Above the first hunk the gap runs from line 1.
#[gpui::test]
fn expand_all_reads_from_the_start_of_the_file(cx: &mut TestAppContext) {
  let mut host = committed_gap_host("@@ -60,4 +60,4 @@");
  host.working = host.working.map(|mut diff| {
    for f in &mut diff.hunks_by_file {
      if f.path == "c.rs" {
        f.hunks.remove(0);
        f.hunks[0].header = "@@ -30,4 +30,4 @@".into();
      }
    }
    diff
  });
  let (mut h, host) = workspace_harness(cx, host, show_committed_route());
  h.click("expand-all-above-c.rs-0");
  h.cx.run_until_parked();
  assert!(
    host.calls().contains(&"read lines c.rs 1..29".to_string()),
    "{:?}",
    host.calls()
  );
  assert!(!h.shows(
    gpui::Role::Button,
    "Expand all 29 lines of c.rs from line 1"
  ));
}

/// Expanded lines number in the same columns as the hunk's own lines.
#[gpui::test]
fn expanded_line_numbers_align_with_the_hunk(cx: &mut TestAppContext) {
  let host = committed_gap_host("@@ -100,4 +100,4 @@");
  let (mut h, _) = workspace_harness(cx, host, show_committed_route());
  h.click("expand-after-c.rs-0");
  h.cx.run_until_parked();
  let hunk = h.cx.debug_bounds("line-new-c.rs-0-1").expect("hunk line");
  let context = h
    .cx
    .debug_bounds("context-new-c.rs-0-after-0")
    .expect("expanded line");
  assert_eq!(hunk.right(), context.right());
}

#[gpui::test]
fn a_large_gap_offers_only_twenty_line_steps(cx: &mut TestAppContext) {
  let host = committed_gap_host("@@ -100,4 +100,4 @@");
  let (mut h, _) = workspace_harness(cx, host, show_committed_route());
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Button, "Show more lines of c.rs from line 5"));
  assert!(h
    .rendered()
    .query_by_role(gpui_testing_library::by::role(gpui::Role::Button).name(
      gpui_testing_library::TextMatch::regex("^Expand all .* of c.rs from line 5$")
    ))
    .is_none());
}

#[gpui::test]
fn changes_tab_discards_everything_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("discard-all-changes");
  assert!(h.shows(gpui::Role::Dialog, "Discard all changes?"));
  h.click("discard-all-dialog-confirm");
  h.cx.run_until_parked();
  let writes = host.writes();
  // Snapshotted first, so the toast can undo it.
  assert_eq!(
    writes[..2],
    [
      "snapshot /repo/.treq/workspaces/login".to_string(),
      "discard /repo/.treq/workspaces/login".to_string()
    ],
    "{writes:?}"
  );
  assert!(!h.shows(gpui::Role::Dialog, "Discard all changes?"));
}

/// Found by the fuzzer: the confirmation listens for Escape only while
/// focus is inside it, so opening it moves the focus there.
#[gpui::test]
fn escape_closes_the_discard_all_confirmation(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("discard-all-changes");
  assert!(h.shows(gpui::Role::Dialog, "Discard all changes?"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.shows(gpui::Role::Dialog, "Discard all changes?"));
  assert!(!host.writes().iter().any(|w| w.starts_with("discard")));
}

#[gpui::test]
fn changes_tab_reads_again_only_when_files_change(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  let reads = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("read workspace diff"))
      .count()
  };
  h.drawn("changes-diff-viewer");
  assert_eq!(reads(&host), 1);
  // React bumps this after each read the tab reports.
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.revision += 1;
    }
  });
  h.drawn("changes-diff-viewer");
  assert_eq!(reads(&host), 1);
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  h.drawn("changes-diff-viewer");
  assert_eq!(reads(&host), 2);
}

/// An agent edits `a.rs` on disk: its `+c` becomes `+e`, and the watcher
/// bumps `changesRevision`.
fn edit_a_rs(h: &mut Harness, host: &FakeHost) {
  let mut edited = host.working.clone().expect("working");
  edited.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+b".into(), "+e".into()];
  *host.edited.lock().unwrap() = Some(edited);
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  h.cx.run_until_parked();
}

/// The diff lines the Changes tab shows for `path`.
fn shown_lines(h: &mut Harness, path: &str) -> Vec<String> {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    changes.hunks[path]
      .hunks
      .iter()
      .flat_map(|h| h.lines.clone())
      .collect()
  })
}

#[gpui::test]
fn changes_tab_follows_edits_on_disk_outside_a_review(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.drawn("changes-diff-viewer");
  edit_a_rs(&mut h, &host);
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
  assert!(!shows_stale_banner(&mut h));
}

#[gpui::test]
fn a_refresh_that_reads_the_same_diff_keeps_its_rows(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.drawn("changes-diff-viewer");
  // The hunks `a.rs` shows and the rows' layout revision.
  let state = |h: &mut Harness| {
    h.drawn("changes-diff-viewer");
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let changes = workspace.changes().expect("changes").read(cx);
      (
        Arc::as_ptr(&changes.hunks["a.rs"].hunks),
        changes.layout_revision(),
      )
    })
  };
  let before = state(&mut h);
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  assert_eq!(state(&mut h), before);
  edit_a_rs(&mut h, &host);
  let after = state(&mut h);
  assert_ne!(after.0, before.0, "the edited file is read again");
  assert_ne!(after.1, before.1, "and laid out again");
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
}

/// `changes_host` whose reads name the working-copy diff they saw.
fn tokened_changes_host() -> FakeHost {
  let mut host = changes_host();
  if let Some(working) = host.working.as_mut() {
    working.working_copy_token = Some("t1".into());
  }
  host
}

#[gpui::test]
fn a_refresh_of_an_unchanged_working_copy_reads_no_hunks(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, tokened_changes_host(), changes_route());
  h.drawn("changes-diff-viewer");
  let (diffs, hunks) = (
    count_calls(&host, "read workspace diff"),
    count_calls(&host, "read hunks"),
  );
  bump_changes_revision(&mut h);
  assert_eq!(count_calls(&host, "read workspace diff"), diffs + 1);
  assert_eq!(
    count_calls(&host, "read hunks"),
    hunks,
    "same diff, nothing to read"
  );

  let mut edited = host.working.clone().expect("working");
  edited.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+b".into(), "+e".into()];
  edited.working_copy_token = Some("t2".into());
  *host.edited.lock().unwrap() = Some(edited);
  bump_changes_revision(&mut h);
  assert!(count_calls(&host, "read hunks") > hunks, "an edit is read");
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
}

#[gpui::test]
fn hunks_read_from_a_newer_diff_are_read_again(cx: &mut TestAppContext) {
  let host = tokened_changes_host();
  *host.hunks_token.lock().unwrap() = Some("t2".into());
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  h.drawn("changes-diff-viewer");
  let hunks = count_calls(&host, "read hunks");
  bump_changes_revision(&mut h);
  assert!(
    count_calls(&host, "read hunks") > hunks,
    "the diff names t1 but the hunks were read at t2"
  );
}

/// `tokened_changes_host` whose diff names each file's content.
fn content_id_changes_host() -> FakeHost {
  let mut host = tokened_changes_host();
  if let Some(working) = host.working.as_mut() {
    for file in &mut working.uncommitted_files {
      file.content_id = Some(format!("{}@1", file.path));
    }
  }
  host
}

#[gpui::test]
fn a_one_file_edit_reads_only_that_files_hunks(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, content_id_changes_host(), changes_route());
  h.drawn("changes-diff-viewer");
  let b_rs = |h: &mut Harness| {
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let changes = workspace.changes().expect("changes").read(cx);
      Arc::as_ptr(&changes.hunks["b.rs"].hunks)
    })
  };
  let before = b_rs(&mut h);
  let hunks = count_calls(&host, "read hunks");

  let mut edited = host.working.clone().expect("working");
  edited.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+b".into(), "+e".into()];
  edited.uncommitted_files[0].content_id = Some("a.rs@2".into());
  edited.working_copy_token = Some("t2".into());
  *host.edited.lock().unwrap() = Some(edited);
  bump_changes_revision(&mut h);

  let reads: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read hunks"))
    .skip(hunks)
    .collect();
  assert_eq!(reads, ["read hunks [\"a.rs\"]"], "b.rs kept its id");
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
  assert_eq!(b_rs(&mut h), before, "b.rs keeps its hunks");

  // Both read at t2 now: the same diff again reads nothing.
  let hunks = count_calls(&host, "read hunks");
  bump_changes_revision(&mut h);
  assert_eq!(count_calls(&host, "read hunks"), hunks);
}

#[gpui::test]
fn hunks_are_read_from_the_listed_diff(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, tokened_changes_host(), changes_route());
  h.drawn("changes-diff-viewer");
  let reads = count_calls(&host, "read hunks");
  assert!(reads > 0);
  assert_eq!(
    count_calls(&host, "hunks at t1"),
    reads,
    "every batch names t1"
  );
}

#[gpui::test]
fn hunks_read_at_another_diff_keep_no_content_id(cx: &mut TestAppContext) {
  let host = content_id_changes_host();
  *host.hunks_token.lock().unwrap() = Some("t2".into());
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  h.drawn("changes-diff-viewer");
  let hunks = count_calls(&host, "read hunks");
  // Another token, same ids: the ids can't vouch for hunks read elsewhere.
  let mut edited = host.working.clone().expect("working");
  edited.working_copy_token = Some("t3".into());
  *host.edited.lock().unwrap() = Some(edited);
  bump_changes_revision(&mut h);
  let reads: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read hunks"))
    .skip(hunks)
    .collect();
  assert_eq!(reads.concat().matches(".rs").count(), 2, "{reads:?}");
}

#[gpui::test]
fn a_review_recheck_reads_only_files_whose_content_changed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, content_id_changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  let hunks = count_calls(&host, "read hunks");
  let mut edited = host.working.clone().expect("working");
  edited.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+b".into(), "+e".into()];
  edited.uncommitted_files[0].content_id = Some("a.rs@2".into());
  edited.working_copy_token = Some("t2".into());
  *host.edited.lock().unwrap() = Some(edited);
  bump_changes_revision(&mut h);

  let reads: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read hunks"))
    .skip(hunks)
    .collect();
  assert_eq!(reads, ["read hunks [\"a.rs\"]"]);
  assert!(shows_stale_banner(&mut h));
}

#[gpui::test]
fn an_edit_on_disk_during_a_review_waits_behind_the_stale_banner(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  edit_a_rs(&mut h, &host);
  assert!(shows_stale_banner(&mut h), "a changed diff, same file list");
  assert_eq!(
    shown_lines(&mut h, "a.rs"),
    [" a", "+b", "+c"],
    "the reviewed diff stays"
  );
  h.click("reload-stale");
  assert!(!shows_stale_banner(&mut h));
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
}

/// Deleting the last comment ends the review: edits on disk show again.
#[gpui::test]
fn deleting_the_last_comment_ends_the_review(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  click_named(&mut h, gpui::Role::Button, "Delete comment");
  h.cx.run_until_parked();
  edit_a_rs(&mut h, &host);
  assert!(!shows_stale_banner(&mut h));
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+b", "+e"]);
}

#[gpui::test]
fn arrow_keys_move_and_extend_the_line_selection(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  // a.rs: " a", "+b", "+c"; b.rs: "+d".
  h.click("gutter-a.rs-0-0");
  h.cx.simulate_keystrokes("down");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 1)])));
  h.cx.simulate_keystrokes("shift-down");
  assert_eq!(
    selected_lines(&mut h),
    Some(("a.rs".into(), vec![(0, 1), (0, 2)]))
  );
  h.cx.simulate_keystrokes("shift-down");
  assert_eq!(
    selected_lines(&mut h),
    Some(("a.rs".into(), vec![(0, 1), (0, 2)])),
    "a selection stays in its file"
  );
  h.cx.simulate_keystrokes("down");
  assert_eq!(selected_lines(&mut h), Some(("b.rs".into(), vec![(0, 0)])));
  h.cx.simulate_keystrokes("up");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 2)])));
  h.cx.simulate_keystrokes("shift-up");
  assert_eq!(
    selected_lines(&mut h),
    Some(("a.rs".into(), vec![(0, 1), (0, 2)]))
  );
  h.cx.simulate_keystrokes("escape");
  assert_eq!(selected_lines(&mut h), None);
}

/// End and Home take the line cursor to the last and first drawn lines.
#[gpui::test]
fn end_and_home_move_the_line_cursor_to_the_ends(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  // a.rs: " a", "+b", "+c"; b.rs: "+d".
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("end");
  assert_eq!(selected_lines(&mut h), Some(("b.rs".into(), vec![(0, 0)])));
  h.cx.simulate_keystrokes("up");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 2)])));
  h.cx.simulate_keystrokes("home");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 0)])));
  h.cx.simulate_keystrokes("down");
  assert_eq!(selected_lines(&mut h), Some(("a.rs".into(), vec![(0, 1)])));
}

#[gpui::test]
fn enter_edits_the_comment_on_the_cursor_line(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("enter");
  assert!(
    h.shows(gpui::Role::MultilineTextInput, "Comment"),
    "Enter opens the line's comment to edit"
  );
}

#[gpui::test]
fn comment_cards_have_no_hover_pencil(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  assert!(has_comment(&mut h, "^Comments on a.rs line \\d+$"));
  let edits = h
    .rendered()
    .query_all_by_role(
      gpui_testing_library::by::role(gpui::Role::Button)
        .name("Edit comment")
        .hidden(true),
    )
    .len();
  assert_eq!(edits, 1, "the card itself edits; no pencil inside it");
}

#[gpui::test]
fn left_comments_the_selected_lines(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("shift-down left");
  assert!(h.shows(gpui::Role::MultilineTextInput, "Add a comment…"));
  h.cx.simulate_input("Both");
  h.cx.simulate_keystrokes("secondary-enter");
  settle_save(&mut h);
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(
    saved.contains("\"start_line\":2") && saved.contains("\"end_line\":3"),
    "{saved}"
  );
  // The diff has the keyboard again.
  h.cx.simulate_keystrokes("down");
  assert!(selected_lines(&mut h).is_some());
}

#[gpui::test]
fn a_suggested_change_is_drawn_and_applied_to_the_file(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("shift-down left");
  h.click("suggest-change");
  // A code editor with the lines and a preview, not fences in the comment.
  assert!(h.shows(gpui::Role::Group, "Suggested change"));
  assert!(h.shows(gpui::Role::Group, "Suggestion preview"));
  let (comment, suggestion) = h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    let changes = workspace.changes().expect("changes").read(cx);
    (
      changes.review.draft_text.clone(),
      changes.review.suggestion_text.clone(),
    )
  });
  assert_eq!(comment, "");
  assert_eq!(suggestion.as_deref(), Some("b\nc"));
  h.cx.simulate_keystrokes("secondary-enter");
  settle_save(&mut h);
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(saved.contains(r"```suggestion\nb\nc\n```"), "{saved}");
  assert!(h.shows(gpui::Role::Group, "Suggestion"), "drawn as a diff");
  // The write changes a.rs on disk; that read is not stale.
  let mut applied = host.working.clone().expect("working");
  applied.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+B".into()];
  *host.edited.lock().unwrap() = Some(applied);
  h.click("apply-suggestion");
  h.cx.run_until_parked();
  assert!(!shows_stale_banner(&mut h), "its own write shows at once");
  assert_eq!(shown_lines(&mut h, "a.rs"), [" a", "+B"]);
  assert!(
    host
      .calls()
      .iter()
      .any(|c| c.contains("apply suggestion") && c.contains("a.rs 2..3 [\"b\", \"c\"]")),
    "{:?}",
    host.calls()
  );
  assert!(
    !has_comment(&mut h, "^Comments on a.rs line \\d+$"),
    "the applied comment is done"
  );
}

#[gpui::test]
fn changes_tab_comments_a_line_and_keeps_the_review(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  assert!(!h.shows(gpui::Role::Toolbar, "Review"));
  comment_line(&mut h, "Nit");
  assert!(has_comment(&mut h, "^Comments on a.rs line \\d+$"));
  assert!(h.shows(gpui::Role::Toolbar, "Review"));
  settle_save(&mut h);
  assert!(host.writes().contains(&"save review 7 None".to_string()));
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(saved.contains("\"Nit\""), "{saved}");
  assert!(saved.contains("\"file_path\":\"a.rs\""), "{saved}");
  click_named(&mut h, gpui::Role::Button, "Delete comment");
  h.cx.run_until_parked();
  assert!(!has_comment(&mut h, "^Comments on a.rs line \\d+$"));
  assert!(!h.shows(gpui::Role::Toolbar, "Review"));
  assert!(host.writes().contains(&"clear review 7".to_string()));
}

#[gpui::test]
fn changes_tab_edits_a_comment(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("inline-comment-card");
  assert!(h.shows(gpui::Role::Button, "Save"));
  h.click("comment-edit");
  h.cx.simulate_input("!");
  h.click("comment-save");
  settle_save(&mut h);
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(saved.contains("\"Nit!\""), "{saved}");
}

#[gpui::test]
fn changes_tab_comments_a_file(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  click_named(&mut h, gpui::Role::Button, "Add file comment on b.rs");
  assert!(h.shows(gpui::Role::MultilineTextInput, "Add a comment…"));
  h.click("comment-input");
  h.cx.simulate_input("Split this");
  h.click("comment-submit");
  assert!(has_comment(&mut h, "^Comments on b.rs$"));
  assert!(h.shows(gpui::Role::Toolbar, "Review"));
}

#[gpui::test]
fn changes_tab_reads_the_kept_review(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.review.lock().unwrap() = Some(
    serde_json::json!([{
      "id": "c1", "file_path": "a.rs", "hunk_id": "h", "start_line": 1, "end_line": 1,
      "line_content": ["+b"], "text": "Kept", "created_at": "", "side": "new"
    }])
    .to_string(),
  );
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(host.calls().contains(&"read review 7".to_string()));
  assert!(has_comment(&mut h, "^Comments on a.rs line \\d+$"));
  assert!(h.shows(gpui::Role::Toolbar, "Review"));
}

#[gpui::test]
fn changes_tab_sends_the_review_to_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("finish-review");
  assert!(h.shows(gpui::Role::Dialog, "Finish your review"));
  h.click("review-plan");
  h.cx.run_until_parked();
  assert!(fields(&h, "changes.review").is_empty());
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1);
  let (request, select) = &launches[0];
  assert_eq!(request.session_name.as_deref(), Some("Code review"));
  assert_eq!(request.mode.as_deref(), Some("plan"));
  assert_eq!(request.workspace_id, Some(7));
  let recorded: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("record review"))
    .collect();
  assert_eq!(recorded, ["record review Some(7) changes plan"]);
  assert!(!select);
  let markdown = request.prompt.clone().unwrap_or_default();
  assert!(
    markdown.contains("Nit") && markdown.contains("a.rs"),
    "{markdown}"
  );
  assert!(!h.shows(gpui::Role::Toolbar, "Review"));
  assert!(!h.shows(gpui::Role::Dialog, "Finish your review"));
  assert!(host.writes().contains(&"clear review 7".to_string()));
  assert!(host
    .writes()
    .contains(&"keep sent comments 7 a.rs:Nit".to_string()));
}

#[gpui::test]
fn changes_tab_starts_an_ai_review_in_a_new_terminal(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("start-agent-review");
  h.cx.run_until_parked();
  assert!(fields(&h, "changes.startReview").is_empty());
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1, "{:?}", host.calls());
  let (request, select) = &launches[0];
  assert_eq!(request.session_name.as_deref(), Some("AI Review"));
  assert_eq!(request.mode.as_deref(), Some("acceptEdits"));
  assert_eq!(request.workspace_id, Some(7));
  assert!(!select);
  let prompt = request.prompt.clone().unwrap_or_default();
  assert!(prompt.contains("--target-type workspace_diff"), "{prompt}");
  assert!(prompt.contains("--target-id 7"), "{prompt}");
}

#[gpui::test]
fn starting_an_ai_review_says_so_in_a_toast(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("start-agent-review");
  h.cx.run_until_parked();
  let calls = host.calls();
  assert!(
    calls
      .iter()
      .any(|c| c.starts_with("notify toast Review started on ")),
    "{calls:?}"
  );
}

#[gpui::test]
fn changes_tab_discards_the_review_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("discard-review");
  assert!(h.shows(gpui::Role::Dialog, "Discard 1 pending comment?"));
  h.click("confirm-discard-review");
  assert!(!h.shows(gpui::Role::Dialog, "Discard 1 pending comment?"));
  assert!(!h.shows(gpui::Role::Toolbar, "Review"));
  assert!(!has_comment(&mut h, "^Comments on a.rs line \\d+$"));
}

#[gpui::test]
fn changes_tab_comments_a_dragged_selection(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.cx.run_until_parked();
  let from = h
    .cx
    .debug_bounds("gutter-a.rs-0-1")
    .expect("gutter")
    .center();
  let to = h
    .cx
    .debug_bounds("gutter-a.rs-0-2")
    .expect("gutter")
    .center();
  h.cx
    .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
  h.cx
    .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
  h.cx
    .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
  h.click("add-comment-a.rs-0-2");
  h.click("comment-input");
  h.cx.simulate_input("Both");
  h.click("comment-submit");
  settle_save(&mut h);
  let saved: serde_json::Value =
    serde_json::from_str(&host.review.lock().unwrap().clone().unwrap_or_default())
      .expect("saved review");
  assert_eq!(saved[0]["start_line"], 2, "{saved}");
  assert_eq!(saved[0]["end_line"], 3, "{saved}");
  assert_eq!(saved[0]["line_content"], serde_json::json!(["+b", "+c"]));
}

#[gpui::test]
fn changes_tab_finds_text_in_the_diff(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  h.click("file-card-a.rs");
  h.cx.simulate_keystrokes("cmd-f");
  assert!(h.shows(gpui::Role::Search, "Find"));
  // a.rs: " a", "+b", "+c"; b.rs: "+d".
  // A short diff is searched on every key: no pause is waited for.
  h.cx.simulate_input("b");
  h.cx.run_until_parked();
  let count = |h: &mut Harness| {
    h.cx.run_until_parked();
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let search = &workspace.changes().expect("changes").read(cx).search;
      (search.current, search.total)
    })
  };
  assert_eq!(count(&mut h), (0, 1));
  h.cx.simulate_keystrokes("enter");
  assert_eq!(count(&mut h), (0, 1));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.shows(gpui::Role::Search, "Find"));
}

#[gpui::test]
fn escape_closes_the_find_bar_after_focus_leaves_it(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  h.click("file-card-a.rs");
  h.cx.simulate_keystrokes("cmd-f");
  assert!(h.shows(gpui::Role::Search, "Find"));
  // Clicking the diff takes focus out of the bar.
  h.click("file-card-a.rs");
  h.cx.simulate_keystrokes("escape");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Search, "Find"));
}

#[gpui::test]
fn changes_tab_shows_review_threads_on_their_line(cx: &mut TestAppContext) {
  let mut host = changes_host();
  let mut folded = gh_thread("t2", 40, true);
  folded.comments[0].body = "Folded away".into();
  host.threads = vec![gh_thread("t1", 2, false), folded];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(
    host
      .calls()
      .contains(&"read threads feature/login".to_string()),
    "{:?}",
    host.calls()
  );
  // Line 2 is in the diff; line 40 is not, so it sits above the file.
  assert!(h.shows(gpui::Role::Group, "Comment by octo"));
  assert!(h.shows(gpui::Role::Group, "Outdated threads on a.rs"));
  // The resolved one starts folded.
  assert!(h.shows(gpui::Role::Paragraph, "Folded away"));
}

#[gpui::test]
fn a_review_thread_draws_its_replies_as_one_card(cx: &mut TestAppContext) {
  let mut host = changes_host();
  let mut thread = gh_thread("t1", 2, false);
  let mut reply = thread.comments[0].clone();
  reply.id = "t1-r".into();
  reply.body = "Non blocking.".into();
  thread.comments.push(reply);
  host.threads = vec![thread];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  let first = h.cx.debug_bounds("github-comment-t1-c").expect("first");
  let second = h.cx.debug_bounds("github-comment-t1-r").expect("reply");
  // Replies stack flush inside one card, split by a rule, not a gap.
  assert_eq!(first.bottom(), second.top());
}

#[gpui::test]
fn review_thread_quote_button_sits_in_the_card_bottom_row(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, _host) = workspace_harness(cx, host, changes_route());
  h.cx.run_until_parked();
  let card = by_id_prefix(&mut h, "github-comment-card-").bounds();
  let quote = h.cx.debug_bounds("github-comment-quote").expect("quote");
  let body = h.cx.debug_bounds("github-comment-body").expect("body");
  assert!(
    card.contains(&quote.origin) && card.contains(&quote.bottom_right()),
    "{quote:?} outside {card:?}"
  );
  assert!(quote.top() >= body.bottom(), "{quote:?} not below {body:?}");
  // A "Quote" button inside the comment, reading "Quote".
  use gpui_testing_library::prelude::*;
  let screen = a11y_screen(&mut h);
  let comment = screen.get_by_role(by::role(gpui::Role::Group).name("Comment by octo"));
  let button = within(&comment).get_by_role(by::role(gpui::Role::Button).name("Quote"));
  expect(&button).to_be_visible();
  expect(&button).to_have_text_content("Quote");
  // The library reads a button's text from its accessible name, so an
  // icon-only button with aria_label "Quote" would pass the above; its
  // width shows the label is drawn.
  assert!(quote.size.width > gpui::px(40.), "{quote:?} draws no label");
}

#[gpui::test]
fn changes_tab_quotes_a_review_thread(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  h.click("github-comment-quote");
  assert!(h.shows(gpui::Role::MultilineTextInput, "Add a comment…"));
  h.click("comment-input");
  h.cx.simulate_input("Agreed");
  h.click("comment-submit");
  settle_save(&mut h);
  let saved: serde_json::Value =
    serde_json::from_str(&host.review.lock().unwrap().clone().unwrap_or_default())
      .expect("saved review");
  assert_eq!(saved[0]["source"], "github", "{saved}");
  assert_eq!(saved[0]["github_author"], "octo");
  assert_eq!(
    saved[0]["line_content"],
    serde_json::json!(["Why two lines?"])
  );
  assert_eq!(saved[0]["start_line"], 2);
}

#[gpui::test]
fn changes_tab_draws_a_github_suggestion_as_a_diff(cx: &mut TestAppContext) {
  let mut host = changes_host();
  let mut thread = gh_thread("t1", 2, false);
  thread.comments[0].body = "Drop it.\n\n```suggestion\n```".into();
  thread.comments[0].diff_hunk = "@@ -1,2 +1,2 @@\n a\n+b".into();
  host.threads = vec![thread];
  let (mut h, _host) = workspace_harness(cx, host, changes_route());
  assert!(h.shows(gpui::Role::Heading, "Suggested change"));
}

#[gpui::test]
fn changes_tab_reacts_to_a_review_comment(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(!h.shows(gpui::Role::Button, "Eyes (1, you reacted)"));
  h.click("reaction-add-t1-c");
  h.click("reaction-pick-t1-c-EYES");
  h.cx.run_until_parked();
  assert!(h.shows(gpui::Role::Button, "Eyes (1, you reacted)"));
  assert!(host.writes().contains(&"react t1-c EYES true".to_string()));
  h.click("reaction-t1-c-EYES");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Button, "Eyes (1, you reacted)"));
  assert!(host.writes().contains(&"react t1-c EYES false".to_string()));
}

#[gpui::test]
fn changes_tab_resolves_an_agent_comment(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(Some("b and c"))];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(h.shows(gpui::Role::Group, "Agent comment on a.rs line 2"));
  assert!(h.shows(gpui::Role::Group, "Suggestion"));
  h.click("agent-resolve-ag");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"resolve agent comment ag".to_string()));
  assert!(!h.shows(gpui::Role::Group, "Agent comment on a.rs line 2"));
}

#[gpui::test]
fn changes_tab_marks_a_sent_review_comment_and_folds_it_once_resolved(cx: &mut TestAppContext) {
  use gpui::Role;
  let host = changes_host();
  let mut sent = agent_comment(None);
  sent.source = "sent-review".into();
  *host.agent.lock().unwrap() = vec![sent];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(h.shows(Role::Group, "Sent comment on a.rs line 2"));
  assert!(!h.shows(Role::Group, "Agent comment on a.rs line 2"));
  assert!(!h.shows(Role::Button, "Delete"));
  assert!(!h.shows(Role::Button, "Send to agent"));
  click_named(&mut h, Role::Button, "Resolve");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"resolve agent comment ag".to_string()));
  // Resolved, it stays on its line, folded to one row.
  assert!(h.shows(Role::Button, "Show resolved comment"));
  assert!(!h.shows(Role::Group, "Sent comment on a.rs line 2"));
  click_named(&mut h, Role::Button, "Show resolved comment");
  assert!(h.shows(Role::Group, "Sent comment on a.rs line 2"));
  assert!(!h.shows(Role::Button, "Resolve"));
  assert!(h.shows(Role::Button, "Hide resolved comment"));
}

#[gpui::test]
fn changes_tab_marks_an_agent_comment_as_local(cx: &mut TestAppContext) {
  use gpui::Role;
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(h.shows(Role::Group, "Agent comment on a.rs line 2"));
  assert!(h.shows(Role::Button, "Delete"));
}

#[gpui::test]
fn changes_tab_counts_open_agent_comments(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(h.shows(gpui::Role::Label, "1 local review comment"));
  h.click("agent-resolve-ag");
  h.cx.run_until_parked();
  assert!(!h.shows(gpui::Role::Label, "1 local review comment"));
}

#[gpui::test]
fn an_agent_comment_poll_that_reads_the_same_comments_changes_nothing(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(h.shows(gpui::Role::Group, "Agent comment on a.rs line 2"));
  let revision = |h: &mut Harness| {
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      workspace
        .changes()
        .expect("changes")
        .read(cx)
        .threads
        .revision
    })
  };
  let (before, reads) = (revision(&mut h), count_calls(&host, "read agent comments"));
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_secs(10));
  h.cx.run_until_parked();
  assert!(count_calls(&host, "read agent comments") > reads, "polled");
  assert_eq!(revision(&mut h), before, "nothing new, nothing redone");

  host.agent.lock().unwrap()[0].comment_text = "Changed".into();
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_secs(10));
  h.cx.run_until_parked();
  assert_ne!(revision(&mut h), before);
}

#[gpui::test]
fn changes_tab_applies_an_agent_suggestion_and_reads_again(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(Some("b and c"))];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  let reads = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("read workspace diff"))
      .count()
  };
  by_id_prefix(&mut h, "agent-review-comment-card-");
  assert_eq!(reads(&host), 1);
  h.click("agent-apply-ag");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"apply agent suggestion ag".to_string()));
  assert_eq!(reads(&host), 2);
}

/// The Changes tab with GPUI's terminal pane below, its agent session 3
/// open on `feature/login`.
fn changes_with_terminal(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  let mut host = changes_host();
  host.sessions = terminal_host().sessions;
  host.workspaces = host.loaded.workspace.clone().into_iter().collect();
  *host.agent.lock().unwrap() = vec![agent_comment(Some("one()"))];
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "changes".into();
  route["terminal"] = serde_json::json!({ "activeSessionId": 3 });
  let route = serde_json::from_value(route).expect("changes route");
  workspace_harness(cx, host, route)
}

#[gpui::test]
fn changes_tab_sends_an_agent_comment_to_the_idle_agent(cx: &mut TestAppContext) {
  let (mut h, host) = changes_with_terminal(cx);
  assert!(h.shows(gpui::Role::Group, "Claude 1 terminal"));
  h.click("agent-send-ag");
  h.cx.run_until_parked();
  assert!(fields(&h, "changes.agentComment").is_empty());
  let prompt = "Please address this review comment.\n\na.rs:2\n> Use one line\n\nSuggested change:\n```\none()\n```\n";
  // The text, then Enter as its own later write, so the TUI submits it.
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_millis(500));
  h.cx.run_until_parked();
  let writes = pty_writes(&host);
  let text = format!("write {} {:?}", agent_pty(), prompt);
  let enter = format!("write {} {:?}", agent_pty(), "\r");
  let at = writes
    .iter()
    .position(|w| *w == text)
    .expect("text written");
  assert_eq!(writes.get(at + 1), Some(&enter), "{writes:?}");
  assert!(host.launches.lock().unwrap().is_empty());
  assert!(host
    .calls()
    .contains(&"notify toast Comment sent to agent".to_string()));
}

#[gpui::test]
fn changes_tab_starts_an_agent_for_a_comment_without_an_idle_one(cx: &mut TestAppContext) {
  // The open agent runs in another workspace, so it is not this one's.
  let mut sessions = terminal_host().sessions;
  sessions[0].workspace_id = Some(99);
  let mut changes = changes_host();
  changes.sessions = sessions;
  changes.workspaces = changes.loaded.workspace.clone().into_iter().collect();
  *changes.agent.lock().unwrap() = vec![agent_comment(None)];
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["tab"] = "changes".into();
  route["terminal"] = serde_json::json!({ "activeSessionId": 3 });
  let (mut h, host) = workspace_harness(cx, changes, serde_json::from_value(route).expect("route"));
  assert!(h.shows(gpui::Role::Group, "Claude 1 terminal"));
  h.click("agent-send-ag");
  h.cx.run_until_parked();
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1, "{:?}", host.calls());
  let request = &launches[0].0;
  assert_eq!(request.session_name.as_deref(), Some("Review comment"));
  assert_eq!(request.mode.as_deref(), Some("acceptEdits"));
  assert_eq!(
    request.prompt.as_deref(),
    Some("Please address this review comment.\n\na.rs:2\n> Use one line\n")
  );
  assert!(host
    .calls()
    .contains(&"notify toast Comment sent to agent".to_string()));
}

#[gpui::test]
fn changes_tab_sends_an_agent_comment_to_react(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(!h.shows(gpui::Role::Button, "Apply"));
  h.click("agent-send-ag");
  h.cx.run_until_parked();
  let sent = fields(&h, "changes.agentComment");
  assert_eq!(sent.len(), 1);
  assert_eq!(sent[0]["id"], "ag");
  assert_eq!(sent[0]["comment_text"], "Use one line");
}

#[gpui::test]
fn enter_confirms_the_discard_all_confirmation(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("discard-all-changes");
  assert!(h.shows(gpui::Role::Dialog, "Discard all changes?"));
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(host.writes().iter().any(|w| w.starts_with("discard")));
  assert!(!h.shows(gpui::Role::Dialog, "Discard all changes?"));
}

#[gpui::test]
fn the_discard_review_confirmation_keeps_on_escape_and_discards_on_enter(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("discard-review");
  h.cx.simulate_keystrokes("escape");
  assert!(!h.shows(gpui::Role::Dialog, "Discard 1 pending comment?"));
  assert!(h.shows(gpui::Role::Toolbar, "Review"));
  h.click("discard-review");
  h.cx.simulate_keystrokes("enter");
  assert!(!h.shows(gpui::Role::Dialog, "Discard 1 pending comment?"));
  assert!(!h.shows(gpui::Role::Toolbar, "Review"));
}

/// The review toolbar's discard says what it discards, and its
/// confirmation counts the comments it drops.
#[gpui::test]
fn discarding_pending_comments_names_what_it_discards(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  let screen = a11y_screen(&mut h);
  expect(screen.get_by_role(by::role(gpui::Role::Button).name("Discard pending comments…")))
    .to_be_visible();
  h.click("discard-review");
  let screen = a11y_screen(&mut h);
  expect(screen.get_by_role(by::role(gpui::Role::Heading).name("Discard 1 pending comment?")))
    .to_be_visible();
  expect(screen.get_by_role(by::role(gpui::Role::Button).name("Cancel"))).to_be_visible();
  expect(screen.get_by_role(by::role(gpui::Role::Button).name("Discard comments"))).to_be_visible();
}

fn changes_host_with_named_statuses() -> FakeHost {
  let mut host = changes_host();
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [
      {"path": "c.rs", "status": "R", "previous_path": "old.rs", "changed_line_count": 1, "diff_deferred": false}
    ],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false},
      {"path": "b.rs", "status": "A", "previous_path": null, "changed_line_count": 1, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1,2 +1,3 @@", "lines": [" a", "+b", "+c"], "patch": ""}]},
      {"path": "b.rs", "hunks": [{"id": "h", "header": "@@ -0,0 +1,1 @@", "lines": ["+d"], "patch": ""}]},
      {"path": "c.rs", "hunks": [{"id": "h", "header": "@@ -1,1 +1,1 @@", "lines": ["-x", "+y"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

/// The file list's status letters name their status, and a committed file's
/// card no longer repeats "Committed": the list's group label says it.
#[gpui::test]
fn file_statuses_are_named_and_committed_cards_carry_no_pill(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(
    cx,
    changes_host_with_named_statuses(),
    committed_route(true),
  );
  assert!(
    h.shows(gpui::Role::Group, "c.rs"),
    "the committed file's card"
  );
  let screen = a11y_screen(&mut h);
  for name in ["Modified", "Added", "Renamed"] {
    expect(screen.get_by_role(by::role(gpui::Role::Label).name(name))).to_be_visible();
  }
  let card = screen.get_by_role(by::role(gpui::Role::Group).name("c.rs"));
  assert!(within(&card)
    .query_by_role(by::role(gpui::Role::Label).name("Committed"))
    .is_none());
}

/// A file with no diff lines (here a binary) is marked viewed by its
/// content, so it is viewed no longer once that content changes.
#[gpui::test]
fn a_viewed_file_without_diff_lines_unmarks_when_it_changes(cx: &mut TestAppContext) {
  let diff = |id: &str| -> crate::jj::JjRevisionDiff {
    serde_json::from_value(serde_json::json!({
      "committed_files": [],
      "uncommitted_files": [
        {"path": "img.png", "status": "M", "previous_path": null, "changed_line_count": 0,
         "diff_deferred": false, "content_id": id}
      ],
      "hunks_by_file": [{"path": "img.png", "hunks": []}],
      "too_large_to_render": false
    }))
    .expect("diff")
  };
  let mut host = changes_host();
  host.working = Some(diff("v1"));
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  h.click("viewed-img.png");
  h.cx.run_until_parked();
  *host.edited.lock().unwrap() = Some(diff("v2"));
  h.update(|s| {
    if let View::Workspace(r) = &mut s.view {
      r.changes_revision += 1;
    }
  });
  h.cx.run_until_parked();
  assert!(
    host.writes().contains(&"unmark viewed img.png".to_string()),
    "{:?}",
    host.writes()
  );
}

/// The Changes header's buttons are named for what they do, so a user (and
/// the QA specs) find discard all by its name rather than by place.
#[gpui::test]
fn the_changes_header_buttons_are_named(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  let screen = a11y_screen(&mut h);
  for name in ["Discard all changes…", "Stash all changes"] {
    expect(screen.get_by_role(by::role(gpui::Role::Button).name(name))).to_be_visible();
  }
  let user = screen.user();
  user.click(&screen.get_by_role(by::role(gpui::Role::Button).name("Discard all changes…")));
  assert!(h.shows(gpui::Role::Dialog, "Discard all changes?"));
}

/// A review thread's fold toggle is a button named for what a click does.
#[gpui::test]
fn a_review_thread_folds_from_a_named_button(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, true)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  let screen = a11y_screen(&mut h);
  let show = screen.get_by_role(by::role(gpui::Role::Button).name("Show review thread"));
  screen.user().click(&show);
  let screen = a11y_screen(&mut h);
  expect(screen.get_by_role(by::role(gpui::Role::Button).name("Hide review thread")))
    .to_be_visible();
  assert!(!h.shows(gpui::Role::Paragraph, "Why two lines?"));
}

/// The Changes section's header is the shared section heading: a heading
/// named by its title, with a toggle that names what it collapses.
#[gpui::test]
fn the_changes_section_header_is_a_heading(cx: &mut TestAppContext) {
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  let screen = a11y_screen(&mut h);
  expect(screen.get_by_role(by::role(gpui::Role::Heading).name("Changes"))).to_be_visible();
  expect(screen.get_by_role(by::role(gpui::Role::Button).name("Collapse Changes"))).to_be_visible();
}

#[gpui::test]
fn a_committed_file_takes_a_file_comment(cx: &mut TestAppContext) {
  use gpui::Role;
  use gpui_testing_library::prelude::*;
  let (mut h, _) = workspace_harness(cx, changes_host_committed_only(), committed_route(true));
  let screen = h.rendered();
  let user = screen.user();
  user.click(&screen.get_by_role(by::role(Role::Button).name("Add file comment on c.rs")));
  user.keyboard("Split this");
  user.click(&screen.get_by_role(by::role(Role::Button).name("Add comment")));
  h.cx.run_until_parked();
  expect(
    h.rendered()
      .get_by_role(by::role(Role::Button).name("Edit comment")),
  )
  .to_be_visible();
}

fn drag_mouse(h: &mut Harness, from: gpui::Point<gpui::Pixels>, to: gpui::Point<gpui::Pixels>) {
  h.cx.simulate_event(gpui::MouseDownEvent {
    position: from,
    button: MouseButton::Left,
    modifiers: Modifiers::none(),
    click_count: 1,
    first_mouse: false,
  });
  for step in 1..=4 {
    let position = from + (to - from) * (step as f32 / 4.);
    h.cx
      .simulate_mouse_move(position, Some(MouseButton::Left), Modifiers::none());
  }
  h.cx.simulate_event(gpui::MouseUpEvent {
    position: to,
    button: MouseButton::Left,
    modifiers: Modifiers::none(),
    click_count: 1,
  });
  h.cx.run_until_parked();
}

fn file_list_width(h: &mut Harness) -> f32 {
  h.cx.run_until_parked();
  f32::from(
    h.cx
      .debug_bounds("changes-file-list")
      .expect("file list drawn")
      .size
      .width,
  )
}

#[gpui::test]
fn dragging_the_file_list_edge_resizes_it_within_bounds(cx: &mut TestAppContext) {
  let (mut h, _host) = workspace_harness(cx, changes_host(), changes_route());
  assert_eq!(file_list_width(&mut h), 240.);
  let list = h.cx.debug_bounds("changes-file-list").expect("drawn");
  let handle = h
    .cx
    .debug_bounds("changes-file-list-resize")
    .expect("drawn")
    .center();
  drag_mouse(
    &mut h,
    handle,
    gpui::point(list.left() + gpui::px(320.), handle.y),
  );
  assert_eq!(file_list_width(&mut h), 320.);

  let handle = h
    .cx
    .debug_bounds("changes-file-list-resize")
    .expect("drawn")
    .center();
  drag_mouse(
    &mut h,
    handle,
    gpui::point(list.left() + gpui::px(5.), handle.y),
  );
  assert_eq!(file_list_width(&mut h), 160.);

  let handle = h
    .cx
    .debug_bounds("changes-file-list-resize")
    .expect("drawn")
    .center();
  h.cx.simulate_click(handle, Modifiers::none());
  h.cx.simulate_event(gpui::MouseDownEvent {
    position: handle,
    button: MouseButton::Left,
    modifiers: Modifiers::none(),
    click_count: 2,
    first_mouse: false,
  });
  h.cx.simulate_event(gpui::MouseUpEvent {
    position: handle,
    button: MouseButton::Left,
    modifiers: Modifiers::none(),
    click_count: 2,
  });
  assert_eq!(file_list_width(&mut h), 240.);
}

#[gpui::test]
fn the_file_list_width_is_saved_and_restored(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  let list = h.cx.debug_bounds("changes-file-list").expect("drawn");
  let handle = h
    .cx
    .debug_bounds("changes-file-list-resize")
    .expect("drawn")
    .center();
  drag_mouse(
    &mut h,
    handle,
    gpui::point(list.left() + gpui::px(300.), handle.y),
  );
  h.cx.run_until_parked();
  assert_eq!(
    host.setting("changes_file_list_width").as_deref(),
    Some("300")
  );

  let host = changes_host();
  host
    .settings
    .lock()
    .unwrap()
    .insert("changes_file_list_width".into(), Some("410".into()));
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert_eq!(file_list_width(&mut h), 410.);
}

/// Resting on a diff line's + button keeps its "Add comment" tooltip open:
/// small pointer moves over the button must not hide and re-show it.
#[gpui::test]
fn the_add_comment_tooltip_stays_open_while_hovered(cx: &mut TestAppContext) {
  use crate::gpui_shell::popup::PopupRequest;
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  h.cx.run_until_parked();
  let at = h
    .cx
    .debug_bounds("add-comment-a.rs-0-1")
    .expect("+ button")
    .center();
  for i in 0..20 {
    let jitter = gpui::point(gpui::px((i % 2) as f32), gpui::px(0.));
    h.cx
      .simulate_mouse_move(at + jitter, None, Modifiers::none());
    h.cx
      .executor()
      .advance_clock(std::time::Duration::from_millis(100));
    h.cx.run_until_parked();
  }
  let log: Vec<&str> = h
    .popups
    .borrow()
    .iter()
    .filter_map(|p| match p {
      PopupRequest::ShowTooltip { .. } => Some("show"),
      PopupRequest::HideTooltip => Some("hide"),
      _ => None,
    })
    .collect();
  assert_eq!(log, vec!["show"], "tooltip popup requests: {log:?}");
}
