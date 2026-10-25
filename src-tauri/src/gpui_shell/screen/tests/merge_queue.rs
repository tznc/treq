//! The GitHub panel's Merge Queue tab (`MergeQueueTab`).

use gpui::TestAppContext;

use super::*;

fn entry(branch: &str, target: &str, position: i64, status: &str) -> serde_json::Value {
  serde_json::json!({
    "branch_name": branch, "pr_number": position, "status": status,
    "position": position, "target_branch": target
  })
}

/// The tab on `o/r`, with Rust's account and queue: `extra` may hold
/// `isPro` and `queueEnabled` (both default on) and `queueLoading` (the
/// queue never read); the rest goes to the route.
fn open(
  cx: &mut TestAppContext,
  host: FakeHost,
  entries: Vec<serde_json::Value>,
  extra: serde_json::Value,
) -> (Harness, Arc<FakeHost>) {
  let mut extra = extra.as_object().cloned().unwrap_or_default();
  let mut flag = |key: &str, default: bool| {
    extra
      .remove(key)
      .and_then(|v| v.as_bool())
      .unwrap_or(default)
  };
  let (pro, enabled, loading) = (
    flag("isPro", true),
    flag("queueEnabled", true),
    flag("queueLoading", false),
  );
  let mut route = serde_json::json!({
    "kind": "mergeQueue", "dark": false, "repoPath": "/repo", "fullName": "o/r",
    "remoteLoading": false, "showMergedHistory": false, "historyLimit": 5
  });
  for (k, v) in extra {
    route[k] = v;
  }
  let entries = serde_json::from_value(serde_json::Value::Array(entries)).expect("entries");
  *host.queue.lock().unwrap() = (!loading).then_some((enabled, entries));
  let snapshot = serde_json::from_value(route).expect("merge queue route");
  let (mut h, host) = workspace_harness(cx, host, snapshot);
  if pro {
    h.set_account(account_state(
      serde_json::json!({ "user": ada(), "subscription": super::pro() }),
    ));
  }
  (h, host)
}

fn active_queue(cx: &mut TestAppContext) -> (Harness, Arc<FakeHost>) {
  open(
    cx,
    queue_host(),
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
  let (mut h, _) = open(
    cx,
    queue_host(),
    vec![],
    serde_json::json!({"isPro": false}),
  );
  assert!(h.drawn("merge-queue-upsell"));
  assert!(h.drawn("github-tab-merge-queue"));
  assert!(!h.drawn("merge-queue-toolbar"));
}

#[gpui::test]
fn merge_queue_sends_its_opt_in_to_settings(cx: &mut TestAppContext) {
  let (mut h, _) = open(
    cx,
    queue_host(),
    vec![],
    serde_json::json!({"queueEnabled": false}),
  );
  assert!(h.drawn("merge-queue-disabled"));
  h.click("merge-queue-enable");
  h.cx.run_until_parked();
  assert_eq!(h.sent(), vec![press("mergeQueue.openSettings")]);
}

#[gpui::test]
fn merge_queue_draws_stacks_above_the_target_branch(cx: &mut TestAppContext) {
  let (mut h, host) = active_queue(cx);
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
fn merge_queue_removes_branches_through_rust(cx: &mut TestAppContext) {
  let (mut h, host) = active_queue(cx);
  h.click("merge-queue-remove-solo");
  h.click("merge-queue-remove-base");
  h.cx.run_until_parked();
  assert_eq!(host.writes(), ["dequeue o/r solo", "dequeue o/r base,mid"]);
  assert!(h.sent().is_empty());
  // Each removal rereads the queue.
  let reads = |host: &FakeHost| {
    host
      .calls()
      .iter()
      .filter(|c| *c == "read queue o/r")
      .count()
  };
  assert_eq!(reads(&host), 3);
}

#[gpui::test]
fn merge_queue_shows_an_empty_queue(cx: &mut TestAppContext) {
  let (mut h, _) = open(cx, queue_host(), vec![], serde_json::json!({}));
  assert!(h.drawn("merge-queue-empty"));
  assert!(h.drawn("merge-queue-target"));
}

#[gpui::test]
fn merge_queue_pages_merged_history(cx: &mut TestAppContext) {
  let merged: Vec<_> = (1..=7)
    .map(|i| entry(&format!("done{i}"), "main", i, "merged"))
    .collect();
  let (mut h, _) = open(
    cx,
    queue_host(),
    merged,
    serde_json::json!({"showMergedHistory": true}),
  );
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
  let (mut h, host) = open(
    cx,
    queue_host(),
    vec![entry("done", "main", 1, "merged")],
    serde_json::json!({}),
  );
  assert!(!h.drawn("merge-queue-history"));
  h.click("merge-queue-merged");
  h.click("merge-queue-refresh");
  h.click("github-tab-prs");
  h.cx.run_until_parked();
  assert_eq!(
    fields(&h, "mergeQueue.showMerged"),
    vec![serde_json::json!(true)]
  );
  // Refresh rereads the queue itself.
  let reads = host
    .calls()
    .iter()
    .filter(|c| *c == "read queue o/r")
    .count();
  assert_eq!(reads, 2);
  let sent = h.sent();
  assert!(!sent.contains(&press("mergeQueue.refresh")));
  assert!(sent.contains(&press("github.tab:prs")));
}

#[gpui::test]
fn merge_queue_copies_the_tip_sha(cx: &mut TestAppContext) {
  let (mut h, _) = active_queue(cx);
  h.click("merge-queue-copy-sha");
  h.cx.run_until_parked();
  let copied = h.cx.read_from_clipboard().and_then(|c| c.text());
  assert_eq!(copied.as_deref(), Some("abc123"));
}

#[gpui::test]
fn merge_queue_waits_for_the_queue(cx: &mut TestAppContext) {
  let (mut h, _) = open(
    cx,
    queue_host(),
    vec![],
    serde_json::json!({"queueLoading": true}),
  );
  assert!(!h.drawn("merge-queue-list"));
  assert!(!h.drawn("merge-queue-target"));
}

#[gpui::test]
fn merge_queue_reads_only_the_history_it_shows(cx: &mut TestAppContext) {
  let merged: Vec<_> = (1..=40)
    .map(|i| entry(&format!("done{i}"), "main", i, "merged"))
    .collect();
  let (mut h, host) = open(
    cx,
    queue_host(),
    merged.clone(),
    serde_json::json!({"showMergedHistory": true}),
  );
  h.cx
    .simulate_resize(gpui::size(gpui::px(1400.), gpui::px(2000.)));
  let reads = |host: &FakeHost| host.queue_history.lock().unwrap().clone();
  // The page shown plus the slack, not the 40 merged entries there are.
  assert_eq!(reads(&host), [25]);
  assert!(h.drawn("merge-queue-single-done40"));
  assert!(h.drawn("merge-queue-load-more"));

  let (_, host) = open(cx, queue_host(), merged, serde_json::json!({}));
  assert_eq!(reads(&host), [20]);
}
