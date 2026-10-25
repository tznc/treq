//! The Logs tab's filters, chart, multi-select and Logs Explorer
//! (`workspace/logs-tab.test.tsx`).

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn logs_tab_filters_by_level(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(None));
  h.click("log-level-filter");
  assert!(h.drawn("logs-filter-list"));
  h.click("log-level-option-error");
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .contains(&"read check logs Some([\"error\"]) None Some([])".to_string()),
    "{:?}",
    host.calls()
  );
  h.click("log-level-option-all");
  h.cx.run_until_parked();
  let reads: Vec<String> = host
    .calls()
    .into_iter()
    .filter(|c| c.starts_with("read check logs"))
    .collect();
  assert_eq!(
    reads.last().map(String::as_str),
    Some("read check logs None None Some([])"),
    "{reads:?}"
  );
}

#[gpui::test]
fn escape_closes_the_level_menu(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, logs_host(), logs_route(None));
  h.click("log-level-filter");
  assert!(h.drawn("logs-filter-list"));
  // What the shell window does with an Escape, whatever has focus.
  assert!(h.cx.update(|_, cx| crate::gpui_shell::ui::escape_menus(cx)));
  assert!(!h.drawn("logs-filter-list"));
  assert!(!h.cx.update(|_, cx| crate::gpui_shell::ui::escape_menus(cx)));
}

#[gpui::test]
fn logs_tab_draws_the_chart_once_there_are_buckets(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, logs_host(), logs_route(None));
  assert!(!h.drawn("logs-timeseries-chart"), "nothing to chart");
  let mut host = logs_host();
  host.buckets = vec![crate::core::checks_logs::LogBucket {
    bucket: "2026-10-19T10:00:01Z".into(),
    severity_text: "ERROR".into(),
    count: 1,
  }];
  let (mut h, _) = workspace_harness(cx, host, logs_route(None));
  assert!(h.drawn("repo-log-line"));
  assert!(h.drawn("logs-timeseries-chart"));
}

#[gpui::test]
fn logs_tab_toggles_lines_in_multi_select(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, logs_host(), logs_route(None));
  h.click("multi-select-toggle");
  h.click("repo-log-line");
  assert!(h.drawn("selection-count"));
  h.click("repo-log-line");
  assert!(!h.drawn("selection-count"));
}

#[gpui::test]
fn logs_explorer_runs_a_group_by_template(cx: &mut TestAppContext) {
  let mut host = logs_host();
  host.sql = Some(Ok(crate::core::checks_logs::SqlResult {
    columns: vec!["job_id".into(), "errors".into()],
    rows: vec![vec![Some("setup".into()), Some("1".into())]],
    row_count: 1,
  }));
  let (mut h, host) = workspace_harness(cx, host, logs_route(None));
  h.click("logs-explorer-view");
  h.click("template-menu");
  assert!(h.drawn("sql-template-Recent lines"));
  h.click("sql-template-Errors by job");
  h.click("run-query");
  h.cx.run_until_parked();
  assert!(
    host.calls().contains(
      &"run sql SELECT json_extract(attributes, '$.job_id') AS job_id, count(*) AS errors"
        .to_string()
    ),
    "{:?}",
    host.calls()
  );
  assert!(h.drawn("sql-results"));
  assert!(!h.drawn("sql-error"));
}

#[gpui::test]
fn logs_explorer_shows_why_a_query_was_rejected(cx: &mut TestAppContext) {
  let mut host = logs_host();
  host.sql = Some(Err(
    "Only read-only queries are allowed (SELECT, WITH, VALUES, EXPLAIN)".into(),
  ));
  let (mut h, _) = workspace_harness(cx, host, logs_route(None));
  h.click("logs-explorer-view");
  h.click("run-query");
  h.cx.run_until_parked();
  assert!(h.drawn("sql-error"));
  assert!(!h.drawn("sql-results"));
}

#[gpui::test]
fn system_logs_search_and_offer_no_explorer(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, logs_host(), logs_route(Some("system")));
  assert!(h.drawn("system-logs-output"));
  assert!(!h.drawn("logs-explorer-view"));
  h.click("system-logs-search");
  h.cx.simulate_input("error-line");
  h.cx
    .executor()
    .advance_clock(std::time::Duration::from_secs(1));
  h.cx.run_until_parked();
  assert!(
    host
      .calls()
      .contains(&"read system logs Some(\"error-line\")".to_string()),
    "{:?}",
    host.calls()
  );
}

// Round 7 measurements: `cargo test --lib logs_perf -- --ignored --nocapture --test-threads=1`.

fn perf_host(records: usize, buckets: usize, long: bool) -> FakeHost {
  let mut host = logs_host();
  host.logs = (0..records)
    .map(|i| {
      let body = if long {
        format!("line {i} {}", "x".repeat(4000))
      } else {
        format!("line {i} compiling crate number {i} with a moderately long message body xxxxxxxxxxxx")
      };
      log_record(&body, if i % 50 == 0 { "ERROR" } else { "INFO" })
    })
    .collect();
  host.buckets = (0..buckets)
    .flat_map(|i| {
      let t = 1_785_000_000 + (i as i64) * 7;
      ["INFO", "ERROR"].map(|s| crate::core::checks_logs::LogBucket {
        bucket: chrono::DateTime::from_timestamp(t, 0)
          .unwrap()
          .format("%Y-%m-%dT%H:%M:%SZ")
          .to_string(),
        severity_text: s.into(),
        count: 1 + (i as i64 % 40),
      })
    })
    .collect();
  host
}

fn logs_perf_case(cx: &mut TestAppContext, label: &str, records: usize, buckets: usize, long: bool) {
  use super::changes_large::{frame, report, timed};
  let (mut h, _) = workspace_harness(cx, perf_host(records, buckets, long), logs_route(None));
  assert!(h.drawn("repo-log-line"));
  println!("-- {label}: records={records} buckets={buckets}x2 long={long}");
  let mut full: Vec<_> = (0..8).map(|_| frame(&mut h)).collect();
  report("full frame", &mut full);
  let mut click: Vec<_> = (0..6)
    .map(|_| timed(&mut h, |h| h.click("repo-log-line")))
    .collect();
  report("click a line (notify)", &mut click);
}

#[gpui::test]
#[ignore = "benchmark"]
fn logs_perf_baseline(cx: &mut TestAppContext) {
  logs_perf_case(cx, "default cap", 2000, 0, false);
}

#[gpui::test]
#[ignore = "benchmark"]
fn logs_perf_long_lines(cx: &mut TestAppContext) {
  logs_perf_case(cx, "long lines", 2000, 0, true);
}

#[gpui::test]
#[ignore = "benchmark"]
fn logs_perf_chart_300(cx: &mut TestAppContext) {
  logs_perf_case(cx, "chart 300 slots", 2000, 150, false);
}

#[gpui::test]
#[ignore = "benchmark"]
fn logs_perf_chart_20k(cx: &mut TestAppContext) {
  logs_perf_case(cx, "chart sparse 20k", 2000, 20_000, false);
}
