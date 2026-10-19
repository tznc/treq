//! The GitHub panel's Merge Queue tab (`MergeQueueTab`).

use gpui::TestAppContext;

use super::*;

fn entry(branch: &str, target: &str, position: i64, status: &str) -> serde_json::Value {
  serde_json::json!({
    "branch_name": branch, "pr_number": position, "status": status,
    "position": position, "target_branch": target
  })
}

fn queue(entries: Vec<serde_json::Value>, extra: serde_json::Value) -> ScreenSnapshot {
  let mut route = serde_json::json!({
    "kind": "mergeQueue", "dark": false, "repoPath": "/repo", "fullName": "o/r",
    "remoteLoading": false, "isPro": true, "queueEnabled": true, "queueLoading": false,
    "entries": entries, "dequeuing": false, "showMergedHistory": false, "historyLimit": 5
  });
  for (k, v) in extra.as_object().cloned().unwrap_or_default() {
    route[k] = v;
  }
  serde_json::from_value(route).expect("merge queue route")
}

fn active_queue() -> ScreenSnapshot {
  queue(
    vec![
      entry("solo", "main", 1, "queued"),
      entry("base", "main", 2, "testing"),
      entry("mid", "base", 3, "queued"),
    ],
    serde_json::json!({}),
  )
}

fn queue_host() -> FakeHost {
  FakeHost {
    history: serde_json::from_value(serde_json::json!({
      "commits": [commit("abc123", 7, 3)],
      "target_branch": "main",
      "workspace_branch": "solo"
    }))
    .ok(),
    workspaces: vec![
      serde_json::from_value(super::workspace_dialogs::workspace_json(3, "solo", None))
        .expect("workspace"),
    ],
    ..FakeHost::default()
  }
}

#[gpui::test]
fn merge_queue_sells_pro_to_free_accounts(cx: &mut TestAppContext) {
  let snapshot = queue(vec![], serde_json::json!({"isPro": false}));
  let (mut h, _) = workspace_harness(cx, queue_host(), snapshot);
  assert!(h.drawn("merge-queue-upsell"));
  assert!(h.drawn("github-tab-merge-queue"));
  assert!(!h.drawn("merge-queue-toolbar"));
}

#[gpui::test]
fn merge_queue_sends_its_opt_in_to_settings(cx: &mut TestAppContext) {
  let snapshot = queue(vec![], serde_json::json!({"queueEnabled": false}));
  let (mut h, _) = workspace_harness(cx, queue_host(), snapshot);
  assert!(h.drawn("merge-queue-disabled"));
  h.click("merge-queue-enable");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("mergeQueue.openSettings")]);
}

#[gpui::test]
fn merge_queue_draws_stacks_above_the_target_branch(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, queue_host(), active_queue());
  for selector in [
    "github-repo-name",
    "merge-queue-toolbar",
    "merge-queue-list",
    "merge-queue-rail",
    "merge-queue-single-solo",
    "merge-queue-stack-base",
    "merge-queue-entry-3",
    "merge-queue-target",
    "merge-queue-target-branch",
    "merge-queue-tip-sha",
  ] {
    assert!(h.drawn(selector), "{selector}");
  }
  // The next to merge sits just above the target branch.
  let solo = h.cx.debug_bounds("merge-queue-single-solo").expect("solo");
  let stack = h.cx.debug_bounds("merge-queue-stack-base").expect("stack");
  let target = h.cx.debug_bounds("merge-queue-target").expect("target");
  assert!(stack.top() < solo.top() && solo.top() < target.top());
  // The workspace's own lines changed fill the entry without any.
  assert!(host
    .calls()
    .contains(&"read commits Some(3) false None".to_string()));
  assert!(h.drawn("diff-stats-inline"));
}

#[gpui::test]
fn merge_queue_removes_branches_through_react(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, queue_host(), active_queue());
  h.click("merge-queue-remove-solo");
  h.click("merge-queue-remove-base");
  h.cx.run_until_parked();
  assert_eq!(
    fields(&h, "mergeQueue.dequeue"),
    vec![
      serde_json::json!(["solo"]),
      serde_json::json!(["base", "mid"])
    ]
  );
}

#[gpui::test]
fn merge_queue_shows_an_empty_queue(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, queue_host(), queue(vec![], serde_json::json!({})));
  assert!(h.drawn("merge-queue-empty"));
  assert!(h.drawn("merge-queue-target"));
}

#[gpui::test]
fn merge_queue_pages_merged_history(cx: &mut TestAppContext) {
  let merged: Vec<_> = (1..=7)
    .map(|i| entry(&format!("done{i}"), "main", i, "merged"))
    .collect();
  let snapshot = queue(merged, serde_json::json!({"showMergedHistory": true}));
  let (mut h, _) = workspace_harness(cx, queue_host(), snapshot);
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(2000.)));
  assert!(h.drawn("merge-queue-history"));
  assert!(h.drawn("merge-queue-single-done7"));
  assert!(!h.drawn("merge-queue-single-done2"));
  // Merged history has no Remove.
  assert!(!h.drawn("merge-queue-remove-done7"));
  h.click("merge-queue-load-more");
  h.cx.run_until_parked();
  assert_eq!(
    fields(&h, "mergeQueue.historyLimit"),
    vec![serde_json::json!(10)]
  );
}

#[gpui::test]
fn merge_queue_hides_history_until_asked(cx: &mut TestAppContext) {
  let snapshot = queue(
    vec![entry("done", "main", 1, "merged")],
    serde_json::json!({}),
  );
  let (mut h, _) = workspace_harness(cx, queue_host(), snapshot);
  assert!(!h.drawn("merge-queue-history"));
  h.click("merge-queue-merged");
  h.click("merge-queue-refresh");
  h.click("github-tab-prs");
  h.cx.run_until_parked();
  assert_eq!(
    fields(&h, "mergeQueue.showMerged"),
    vec![serde_json::json!(true)]
  );
  let sent = h.sent();
  assert!(sent.contains(&press("mergeQueue.refresh")));
  assert!(sent.contains(&press("github.tab:prs")));
}

#[gpui::test]
fn merge_queue_copies_the_tip_sha(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, queue_host(), active_queue());
  h.click("merge-queue-copy-sha");
  h.cx.run_until_parked();
  let copied = h.cx.read_from_clipboard().and_then(|c| c.text());
  assert_eq!(copied.as_deref(), Some("abc123"));
}

#[gpui::test]
fn merge_queue_waits_for_the_queue(cx: &mut TestAppContext) {
  let snapshot = queue(vec![], serde_json::json!({"queueLoading": true}));
  let (mut h, _) = workspace_harness(cx, queue_host(), snapshot);
  assert!(!h.drawn("merge-queue-list"));
  assert!(!h.drawn("merge-queue-target"));
}
