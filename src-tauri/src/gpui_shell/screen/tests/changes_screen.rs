use gpui::{Modifiers, MouseButton, TestAppContext};

use super::*;

#[gpui::test]
fn changes_tab_lists_files_and_their_diffs(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  for selector in [
    "changes-diff-viewer",
    "file-row-a.rs",
    "file-row-b.rs",
    "file-card-a.rs",
    "file-card-b.rs",
    "commit-button",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  assert!(!h.drawn("workspace-code"));
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
  assert!(h.drawn("file-card-Cargo.lock"));
  assert!(h.drawn("load-generated-Cargo.lock"));
  assert!(h.drawn("load-generated-gen/api.ts"));
  assert!(!h.drawn("load-generated-a.rs"));
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
    !h.drawn("load-generated-Cargo.lock"),
    "the diff shows instead"
  );
  assert!(h.drawn("load-generated-gen/api.ts"));
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
  assert!(h.drawn("file-row-a.rs"));
  assert!(h.drawn("file-card-a.rs"));
  for hidden in [
    "file-row-Cargo.lock",
    "file-card-Cargo.lock",
    "file-row-gen/api.ts",
  ] {
    assert!(!h.drawn(hidden), "{hidden}");
  }
  filter(&mut h, "Generated files");
  assert!(h.drawn("file-row-Cargo.lock"));
  assert!(h.drawn("file-card-gen/api.ts"));
}

#[gpui::test]
fn changes_tab_keeps_a_selected_generated_file_listed(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_generated(), changes_route());
  h.click("file-row-Cargo.lock");
  h.click("stage-Cargo.lock");
  filter(&mut h, "Generated files");
  assert!(h.drawn("unstage-Cargo.lock"));
  assert!(!h.drawn("file-row-gen/api.ts"));
}

#[gpui::test]
fn changes_tab_offers_the_filter_only_with_something_to_filter(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  assert!(h.drawn("file-row-a.rs"));
  assert!(!h.drawn("changes-filter"));
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
fn changes_tab_filters_the_committed_group(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(true));
  assert!(h.drawn("committed-section-toggle"));
  assert!(h.drawn("file-card-c.rs"));
  filter(&mut h, "Committed");
  assert_eq!(
    fields(&h, "changes.showCommitted"),
    vec![serde_json::json!(false)]
  );
}

#[gpui::test]
fn changes_tab_hides_the_committed_group_when_filtered_out(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host_with_committed(), committed_route(false));
  assert!(h.drawn("file-row-a.rs"));
  assert!(h.drawn("changes-filter"));
  assert!(!h.drawn("committed-section-toggle"));
  assert!(!h.drawn("file-card-c.rs"));
}

#[gpui::test]
fn changes_tab_asks_for_a_commit_message(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("commit-message");
  h.cx.simulate_keystrokes("secondary-enter");
  h.cx.run_until_parked();
  assert!(h.drawn("commit-message-error"));
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

#[gpui::test]
fn changes_tab_commits_only_staged_files(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("file-row-a.rs");
  h.click("stage-a.rs");
  h.cx.run_until_parked();
  assert!(h.drawn("unstage-a.rs"));
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
  assert!(!h.drawn("expand-after-c.rs-0"));
  assert!(!h.drawn("expand-all-c.rs-0"));
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
  assert!(!h.drawn("expand-all-above-c.rs-0"));
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
  assert!(h.drawn("expand-after-c.rs-0"));
  assert!(!h.drawn("expand-all-c.rs-0"));
}

#[gpui::test]
fn changes_tab_discards_everything_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("discard-all-changes");
  assert!(h.drawn("discard-all-dialog"));
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
  assert!(!h.drawn("discard-all-dialog"));
}

/// Found by the fuzzer: the confirmation listens for Escape only while
/// focus is inside it, so opening it moves the focus there.
#[gpui::test]
fn escape_closes_the_discard_all_confirmation(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("discard-all-changes");
  assert!(h.drawn("discard-all-dialog"));
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("discard-all-dialog"));
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
  assert!(!h.drawn("stale-banner"));
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
  assert!(h.drawn("stale-banner"));
}

#[gpui::test]
fn an_edit_on_disk_during_a_review_waits_behind_the_stale_banner(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  edit_a_rs(&mut h, &host);
  assert!(h.drawn("stale-banner"), "a changed diff, same file list");
  assert_eq!(
    shown_lines(&mut h, "a.rs"),
    [" a", "+b", "+c"],
    "the reviewed diff stays"
  );
  h.click("reload-stale");
  assert!(!h.drawn("stale-banner"));
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

#[gpui::test]
fn enter_edits_the_comment_on_the_cursor_line(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("enter");
  assert!(
    h.drawn("comment-edit"),
    "Enter opens the line's comment to edit"
  );
}

#[gpui::test]
fn comment_cards_have_no_hover_pencil(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  assert!(h.drawn("inline-comment-card"));
  assert!(!h.drawn("comment-card-pencil"));
}

#[gpui::test]
fn left_comments_the_selected_lines(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  h.click("gutter-a.rs-0-1");
  h.cx.simulate_keystrokes("shift-down left");
  assert!(h.drawn("comment-input"));
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
  assert_eq!(
    h.screen.read_with(&h.cx, |screen, cx| {
      let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
      let changes = workspace.changes().expect("changes").read(cx);
      changes.review.draft_text.clone()
    }),
    "```suggestion\nb\nc\n```\n"
  );
  h.cx.simulate_keystrokes("secondary-enter");
  assert!(h.drawn("agent-review-suggestion"), "drawn as a diff");
  // The write changes a.rs on disk; that read is not stale.
  let mut applied = host.working.clone().expect("working");
  applied.hunks_by_file[0].hunks[0].lines = vec![" a".into(), "+B".into()];
  *host.edited.lock().unwrap() = Some(applied);
  h.click("apply-suggestion");
  h.cx.run_until_parked();
  assert!(!h.drawn("stale-banner"), "its own write shows at once");
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
    !h.drawn("inline-comment-card"),
    "the applied comment is done"
  );
}

#[gpui::test]
fn changes_tab_comments_a_line_and_keeps_the_review(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  assert!(!h.drawn("review-action-bar"));
  comment_line(&mut h, "Nit");
  assert!(h.drawn("inline-comment-card"));
  assert!(h.drawn("review-action-bar"));
  settle_save(&mut h);
  assert!(host.writes().contains(&"save review 7 None".to_string()));
  let saved = host.review.lock().unwrap().clone().unwrap_or_default();
  assert!(saved.contains("\"Nit\""), "{saved}");
  assert!(saved.contains("\"file_path\":\"a.rs\""), "{saved}");
  h.click("delete-comment");
  h.cx.run_until_parked();
  assert!(!h.drawn("inline-comment-card"));
  assert!(!h.drawn("review-action-bar"));
  assert!(host.writes().contains(&"clear review 7".to_string()));
}

#[gpui::test]
fn changes_tab_edits_a_comment(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("inline-comment-card");
  assert!(h.drawn("comment-save"));
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
  h.click("add-file-comment-button");
  assert!(h.drawn("comment-input"));
  h.click("comment-input");
  h.cx.simulate_input("Split this");
  h.click("comment-submit");
  assert!(h.drawn("file-comment-card"));
  assert!(h.drawn("review-action-bar"));
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
  assert!(h.drawn("inline-comment-card"));
  assert!(h.drawn("review-action-bar"));
}

#[gpui::test]
fn changes_tab_sends_the_review_to_an_agent(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("finish-review");
  assert!(h.drawn("finish-review-popover"));
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
  assert!(!h.drawn("review-action-bar"));
  assert!(!h.drawn("finish-review-popover"));
  assert!(host.writes().contains(&"clear review 7".to_string()));
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
fn changes_tab_discards_the_review_once_confirmed(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("discard-review");
  assert!(h.drawn("discard-review-dialog"));
  h.click("confirm-discard-review");
  assert!(!h.drawn("discard-review-dialog"));
  assert!(!h.drawn("review-action-bar"));
  assert!(!h.drawn("inline-comment-card"));
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
  assert!(h.drawn("diff-search"));
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
  assert!(!h.drawn("diff-search"));
}

#[gpui::test]
fn changes_tab_shows_review_threads_on_their_line(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false), gh_thread("t2", 40, true)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(
    host
      .calls()
      .contains(&"read threads feature/login".to_string()),
    "{:?}",
    host.calls()
  );
  // Line 2 is in the diff; line 40 is not, so it sits above the file.
  assert!(h.drawn("github-comment-card"));
  assert!(h.drawn("github-outdated-threads"));
  // The resolved one starts folded.
  assert!(h.drawn("github-thread-collapsed-preview"));
}

#[gpui::test]
fn changes_tab_quotes_a_review_thread(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  h.click("github-comment-quote");
  assert!(h.drawn("comment-input"));
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
fn changes_tab_reacts_to_a_review_comment(cx: &mut TestAppContext) {
  let mut host = changes_host();
  host.threads = vec![gh_thread("t1", 2, false)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(!h.drawn("reaction-t1-c-EYES"));
  h.click("reaction-add-t1-c");
  h.click("reaction-pick-t1-c-EYES");
  h.cx.run_until_parked();
  assert!(h.drawn("reaction-t1-c-EYES"));
  assert!(host.writes().contains(&"react t1-c EYES true".to_string()));
  h.click("reaction-t1-c-EYES");
  h.cx.run_until_parked();
  assert!(!h.drawn("reaction-t1-c-EYES"));
  assert!(host.writes().contains(&"react t1-c EYES false".to_string()));
}

#[gpui::test]
fn changes_tab_resolves_an_agent_comment(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(Some("b and c"))];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(h.drawn("agent-review-comment-card"));
  assert!(h.drawn("agent-review-suggestion"));
  h.click("agent-resolve-ag");
  h.cx.run_until_parked();
  assert!(host
    .writes()
    .contains(&"resolve agent comment ag".to_string()));
  assert!(!h.drawn("agent-review-comment-card"));
}

#[gpui::test]
fn changes_tab_counts_open_agent_comments(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, _) = workspace_harness(cx, host, changes_route());
  assert!(h.drawn("agent-review-comment-count"));
  h.click("agent-resolve-ag");
  h.cx.run_until_parked();
  assert!(!h.drawn("agent-review-comment-count"));
}

#[gpui::test]
fn an_agent_comment_poll_that_reads_the_same_comments_changes_nothing(cx: &mut TestAppContext) {
  let host = changes_host();
  *host.agent.lock().unwrap() = vec![agent_comment(None)];
  let (mut h, host) = workspace_harness(cx, host, changes_route());
  assert!(h.drawn("agent-review-comment-card"));
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
  h.drawn("agent-review-comment-card");
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
  assert!(h.drawn("terminal-column-agent"));
  h.click("agent-send-ag");
  h.cx.run_until_parked();
  assert!(fields(&h, "changes.agentComment").is_empty());
  let prompt = "Please address this review comment.\n\na.rs:2\n> Use one line\n\nSuggested change:\n```\none()\n```\n";
  let sent = format!("write {} {:?}", agent_pty(), format!("{prompt}\n"));
  assert!(pty_writes(&host).contains(&sent), "{:?}", pty_writes(&host));
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
  assert!(h.drawn("terminal-column-agent"));
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
  assert!(!h.drawn("agent-apply-ag"));
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
  assert!(h.drawn("discard-all-dialog"));
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(host.writes().iter().any(|w| w.starts_with("discard")));
  assert!(!h.drawn("discard-all-dialog"));
}

#[gpui::test]
fn the_discard_review_confirmation_keeps_on_escape_and_discards_on_enter(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, changes_host(), changes_route());
  comment_line(&mut h, "Nit");
  h.click("discard-review");
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("discard-review-dialog"));
  assert!(h.drawn("review-action-bar"));
  h.click("discard-review");
  h.cx.simulate_keystrokes("enter");
  assert!(!h.drawn("discard-review-dialog"));
  assert!(!h.drawn("review-action-bar"));
}
