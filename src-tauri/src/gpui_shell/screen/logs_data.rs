//! What the Logs tab (`LogsTab`) reads, and the pure parts of drawing it:
//! the prompts it sends an agent, the line selection, the chart's
//! timeline.

use std::collections::BTreeSet;

use super::changes_data::ChangesHost;
use crate::{
  core::{
    agent_chat::{AgentChat, AgentChatSummary, ChatRole},
    checks::WorkflowInfo,
    checks_logs::{LogBucket, LogQuery, LogRecordView, SqlResult},
  },
  gpui_shell::native::Backend,
};

/// `run_logs_sql`'s default row cap.
const SQL_MAX_ROWS: i64 = 500;

pub trait LogsHost: ChangesHost {
  /// `get_repo_logs`: the repository's check logs.
  fn repo_logs(&self, repo_path: &str, query: &LogQuery) -> Result<Vec<LogRecordView>, String>;
  /// `get_log_timeseries` in one-second buckets.
  fn log_timeseries(&self, repo_path: &str, query: &LogQuery) -> Result<Vec<LogBucket>, String>;
  /// `get_app_setup_logs`.
  fn app_setup_logs(&self, query: &LogQuery) -> Result<Vec<LogRecordView>, String>;
  /// `get_system_logs`: the app's own log files.
  fn system_logs(&self, query: &LogQuery) -> Result<Vec<LogRecordView>, String>;
  /// The repository's code review history (`core::review_history`).
  fn review_history(&self, repo_path: &str, query: &LogQuery) -> Result<Vec<LogRecordView>, String>;
  /// `open_system_log_dir`.
  fn open_system_log_dir(&self) -> Result<(), String>;
  fn workflows(&self, repo_path: &str) -> Result<Vec<WorkflowInfo>, String>;
  fn agent_chats(&self, repo_path: &str) -> Result<Vec<AgentChatSummary>, String>;
  fn agent_chat(&self, repo_path: &str, session_id: i64) -> Result<Option<AgentChat>, String>;
  /// `run_logs_sql`: read-only SQL over the `logs` view.
  fn run_sql(&self, repo_path: &str, sql: &str) -> Result<SqlResult, String>;
}

pub type Host = std::sync::Arc<dyn LogsHost>;

impl Backend {
  fn log_dir(&self) -> std::path::PathBuf {
    use tauri::Manager;
    self
      .app()
      .state::<crate::AppState>()
      .telemetry
      .log_dir
      .clone()
  }
}

impl LogsHost for Backend {
  fn repo_logs(&self, repo_path: &str, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
    self.local_only(repo_path, "Checks and logs")?;
    crate::core::checks_logs::query_repo_logs(repo_path, query)
  }

  fn log_timeseries(&self, repo_path: &str, query: &LogQuery) -> Result<Vec<LogBucket>, String> {
    self.local_only(repo_path, "Checks and logs")?;
    crate::core::checks_logs::query_log_timeseries(repo_path, query, 1)
  }

  fn review_history(&self, repo_path: &str, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
    self.local_only(repo_path, "Review history")?;
    crate::core::review_history::log_records(repo_path, query)
  }

  fn app_setup_logs(&self, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
    let db = crate::core::app_setup_script::app_db_path()?;
    crate::core::app_setup_script::query_logs(&db, query)
  }

  fn system_logs(&self, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
    crate::core::system_logs::query_system_logs(&self.log_dir(), query)
  }

  fn open_system_log_dir(&self) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    self
      .app()
      .opener()
      .open_path(self.log_dir().to_string_lossy(), None::<&str>)
      .map_err(|e| e.to_string())
  }

  fn workflows(&self, repo_path: &str) -> Result<Vec<WorkflowInfo>, String> {
    self.local_only(repo_path, "Checks and logs")?;
    crate::core::checks::list_workflows_sync(repo_path)
  }

  fn agent_chats(&self, repo_path: &str) -> Result<Vec<AgentChatSummary>, String> {
    self.local_only(repo_path, "Agent chat logs")?;
    crate::core::agent_chat::list_agent_chats(repo_path)
  }

  fn agent_chat(&self, repo_path: &str, session_id: i64) -> Result<Option<AgentChat>, String> {
    self.local_only(repo_path, "Agent chat logs")?;
    crate::core::agent_chat::get_agent_chat(repo_path, session_id)
  }

  fn run_sql(&self, repo_path: &str, sql: &str) -> Result<SqlResult, String> {
    self.local_only(repo_path, "Checks and logs")?;
    crate::core::checks_logs::run_logs_sql(repo_path, sql, SQL_MAX_ROWS)
  }
}

/// The "open in Logs" source for a workspace setup script: the check logs,
/// filtered to the setup script's runs.
pub const SETUP_SCRIPT_SOURCE: &str = "setup-script";

/// `LogSource`: where the tab reads its lines from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
  #[default]
  Checks,
  Agent,
  AppSetup,
  System,
  Reviews,
}

impl Source {
  pub const ALL: [Source; 5] = [
    Source::Checks,
    Source::Agent,
    Source::Reviews,
    Source::AppSetup,
    Source::System,
  ];

  /// The source an "open in Logs" link names; anything unknown, such as
  /// [`SETUP_SCRIPT_SOURCE`], opens the check logs.
  pub fn parse(value: &str) -> Self {
    match value {
      "agent-chats" => Source::Agent,
      "app-setup" => Source::AppSetup,
      "system" => Source::System,
      "reviews" => Source::Reviews,
      _ => Source::Checks,
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      Source::Checks => "Checks",
      Source::Agent => "Agent",
      Source::AppSetup => "App setup",
      Source::System => "System",
      Source::Reviews => "Reviews",
    }
  }
}

/// `LOG_LEVELS`, as the level filter offers them.
pub const LEVELS: [&str; 3] = ["info", "warning", "error"];

/// `MultiSelectFilter`'s trigger label: `all` with nothing ticked, the one
/// ticked option's label, else how many.
pub fn filter_label(
  selected: &[String],
  options: &[(String, String)],
  all: &str,
  noun: &str,
) -> String {
  match selected {
    [] => all.to_string(),
    [one] => options
      .iter()
      .find(|(value, _)| value == one)
      .map_or_else(|| one.clone(), |(_, label)| label.clone()),
    many => format!("{} {noun}", many.len()),
  }
}

/// Ticks or unticks `value`, keeping the order things were ticked in.
pub fn toggle(selected: &mut Vec<String>, value: &str) {
  match selected.iter().position(|v| v == value) {
    Some(i) => {
      selected.remove(i);
    }
    None => selected.push(value.to_string()),
  }
}

/// `MAX_PROMPT_LINES`: so a huge selection can't blow past the agent's
/// context.
const MAX_PROMPT_LINES: usize = 500;

fn fence(body: &str) -> String {
  format!("```\n{body}\n```")
}

fn truncation_note(total: usize, shown: usize) -> String {
  if total > shown {
    format!("\n\n_({} further rows omitted.)_", total - shown)
  } else {
    String::new()
  }
}

/// `buildLogLinesPrompt`: log records picked out of a feed.
pub fn log_lines_prompt(records: &[LogRecordView], context: &str) -> String {
  let shown = &records[..records.len().min(MAX_PROMPT_LINES)];
  let body = shown
    .iter()
    .map(|r| {
      format!(
        "{}  [{}]  {}/{}  {}",
        r.timestamp, r.severity_text, r.job_id, r.step_name, r.body
      )
    })
    .collect::<Vec<_>>()
    .join("\n");
  [
    format!(
      "Here are {} log line{} from {context}.",
      records.len(),
      if records.len() == 1 { "" } else { "s" }
    ),
    String::new(),
    fence(&body),
    truncation_note(records.len(), shown.len()),
    String::new(),
    "Please help me understand what went wrong and how to fix it.".into(),
  ]
  .join("\n")
}

/// `buildSqlResultPrompt`: a whole result set from the explorer.
pub fn sql_result_prompt(result: &SqlResult, sql: &str) -> String {
  let shown = &result.rows[..result.rows.len().min(MAX_PROMPT_LINES)];
  let header = result.columns.join("\t");
  let body = shown
    .iter()
    .map(|row| {
      row
        .iter()
        .map(|cell| cell.as_deref().unwrap_or("NULL"))
        .collect::<Vec<_>>()
        .join("\t")
    })
    .collect::<Vec<_>>()
    .join("\n");
  [
    "Here is the result of a query against my check logs.".to_string(),
    String::new(),
    "Query:".into(),
    format!("```sql\n{}\n```", sql.trim()),
    String::new(),
    format!(
      "Result ({} row{}):",
      result.row_count,
      if result.row_count == 1 { "" } else { "s" }
    ),
    fence(&format!("{header}\n{body}")),
    truncation_note(result.rows.len(), shown.len()),
    String::new(),
    "Please help me interpret these results.".into(),
  ]
  .join("\n")
}

/// `agentChatToLogRecords`: a stored agent conversation as feed rows.
pub fn agent_chat_records(chat: &AgentChat) -> Vec<LogRecordView> {
  chat
    .messages
    .iter()
    .filter(|m| !m.message.trim().is_empty())
    .map(|m| {
      let role = match m.role {
        ChatRole::User => "user",
        ChatRole::Agent => "agent",
      };
      LogRecordView {
        timestamp: m.time.clone(),
        severity_number: 9,
        severity_text: "INFO".into(),
        body: m.message.clone(),
        trace_id: chat.session_id.to_string(),
        span_id: m.id.to_string(),
        run_id: chat.session_id,
        job_id: role.into(),
        step_index: i64::try_from(m.id).unwrap_or_default(),
        step_name: chat.name.clone(),
        stream: role.into(),
      }
    })
    .collect()
}

/// `formatTimestamp`: the time of day in UTC to the millisecond, or the
/// text as is when it is not a date.
pub fn format_timestamp(timestamp: &str) -> String {
  match chrono::DateTime::parse_from_rfc3339(timestamp) {
    Ok(date) => date
      .with_timezone(&chrono::Utc)
      .format("%H:%M:%S%.3f")
      .to_string(),
    Err(_) => timestamp.to_string(),
  }
}

/// Indexes of the records whose body contains `query`, ignoring case.
pub fn find_matches(records: &[LogRecordView], query: &str) -> Vec<usize> {
  let query = query.trim().to_lowercase();
  if query.is_empty() {
    return Vec::new();
  }
  records
    .iter()
    .enumerate()
    .filter(|(_, r)| r.body.to_lowercase().contains(&query))
    .map(|(i, _)| i)
    .collect()
}

/// `useLineSelection`: a dragged run of lines, or with multi-select on,
/// lines toggled one by one.
#[derive(Debug, Default, Clone)]
pub struct LineSelection {
  pub selected: BTreeSet<usize>,
  pub multi: bool,
  pub dragging: bool,
  anchor: Option<usize>,
}

impl LineSelection {
  pub fn mouse_down(&mut self, index: usize) {
    if self.multi {
      if !self.selected.remove(&index) {
        self.selected.insert(index);
      }
      return;
    }
    self.anchor = Some(index);
    self.dragging = true;
    self.selected = BTreeSet::from([index]);
  }

  pub fn mouse_enter(&mut self, index: usize) {
    let Some(anchor) = self.anchor.filter(|_| self.dragging) else {
      return;
    };
    let (start, end) = (anchor.min(index), anchor.max(index));
    self.selected = (start..=end).collect();
  }

  pub fn mouse_up(&mut self) {
    self.dragging = false;
  }

  pub fn clear(&mut self) {
    self.selected.clear();
    self.anchor = None;
  }

  pub fn toggle_multi(&mut self) {
    self.multi = !self.multi;
    self.clear();
  }

  pub fn select_all(&mut self, count: usize) {
    self.selected = (0..count).collect();
  }
}

/// `TEMPLATES`' first query, which the editor starts with.
pub const DEFAULT_QUERY: &str =
  "SELECT timestamp, severityText, json_extract(attributes, '$.job_id') AS job_id,
       json_extract(body, '$.message') AS message
FROM logs
ORDER BY timestamp DESC
LIMIT 50";

/// The explorer's templates: label, description and query.
pub const TEMPLATES: [(&str, &str, &str); 5] = [
  (
    "Recent lines",
    "Newest records across every run",
    DEFAULT_QUERY,
  ),
  (
    "Errors by job",
    "Which jobs produce the most ERROR records",
    "SELECT json_extract(attributes, '$.job_id') AS job_id, count(*) AS errors
FROM logs
WHERE severityText = 'ERROR'
GROUP BY job_id
ORDER BY errors DESC",
  ),
  (
    "Severity by run",
    "Record counts per run, split by severity",
    "SELECT json_extract(attributes, '$.run_id') AS run_id, severityText, count(*) AS records
FROM logs
GROUP BY run_id, severityText
ORDER BY run_id DESC, severityText",
  ),
  (
    "Slowest steps",
    "Wall-clock span of each step, longest first",
    "SELECT json_extract(attributes, '$.job_id') AS job_id,
       json_extract(attributes, '$.step_name') AS step_name,
       (unixepoch(max(timestamp)) - unixepoch(min(timestamp))) * 1000 AS duration_ms
FROM logs
GROUP BY job_id, step_name
ORDER BY duration_ms DESC",
  ),
  (
    "Trace overview",
    "One row per trace and span, with severity counts",
    "SELECT traceId, spanId, json_extract(attributes, '$.job_id') AS job_id,
       count(*) AS records,
       count(*) FILTER (WHERE severityText = 'ERROR') AS errors
FROM logs
GROUP BY traceId, spanId, job_id
ORDER BY errors DESC, records DESC",
  ),
];

/// The chart's severities, lowest first so ERROR stacks on top.
pub const SERIES: [(&str, &str); 3] = [("INFO", "Info"), ("WARN", "Warn"), ("ERROR", "Error")];

/// The most columns the chart draws. A longer range merges seconds into
/// columns, so the bars stay wider than a pixel and a repaint stays cheap.
const MAX_COLUMNS: i64 = 200;

/// `LogsTimeseriesChart`'s bars: each slot's label and its count per
/// [`SERIES`], quiet seconds filled in so bursts don't look adjacent.
#[derive(Debug, Default, PartialEq)]
pub struct Chart {
  pub labels: Vec<String>,
  pub counts: Vec<[i64; 3]>,
}

/// The chart of `buckets`: one column per second up to [`MAX_COLUMNS`]; a
/// longer range has its seconds merged into that many columns, each
/// labelled by its first second.
pub fn chart(buckets: &[LogBucket]) -> Chart {
  let parse = |b: &LogBucket| {
    chrono::DateTime::parse_from_rfc3339(&b.bucket)
      .ok()
      .map(|d| d.timestamp_millis())
  };
  let points: Vec<(i64, usize, i64)> = buckets
    .iter()
    .filter_map(|b| {
      let series = SERIES.iter().position(|(s, _)| *s == b.severity_text)?;
      Some((parse(b)?, series, b.count))
    })
    .collect();
  let (Some(first), Some(last)) = (
    points.iter().map(|p| p.0).min(),
    points.iter().map(|p| p.0).max(),
  ) else {
    return Chart::default();
  };
  let seconds = (last - first) / 1000 + 1;
  // Seconds per column, whole seconds.
  let width = (seconds + MAX_COLUMNS - 1) / MAX_COLUMNS;
  let columns = ((seconds + width - 1) / width) as usize;
  let mut counts = vec![[0; 3]; columns];
  for (at, series, count) in points {
    counts[((at - first) / 1000 / width) as usize][series] += count;
  }
  let labels = (0..columns as i64)
    .map(|i| {
      chrono::DateTime::from_timestamp_millis(first + i * width * 1000)
        .map(|d| d.format("%H:%M:%S").to_string())
        .unwrap_or_default()
    })
    .collect();
  Chart { labels, counts }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::agent_chat::{AgentType, ChatMessage};

  fn record(body: &str) -> LogRecordView {
    LogRecordView {
      timestamp: "2026-10-19T10:00:01.250Z".into(),
      severity_number: 9,
      severity_text: "INFO".into(),
      body: body.into(),
      trace_id: String::new(),
      span_id: String::new(),
      run_id: 3,
      job_id: "build".into(),
      step_index: 0,
      step_name: "Test".into(),
      stream: "stdout".into(),
    }
  }

  fn bucket(at: &str, severity: &str, count: i64) -> LogBucket {
    LogBucket {
      bucket: at.into(),
      severity_text: severity.into(),
      count,
    }
  }

  #[test]
  fn prompts_for_the_picked_lines() {
    let prompt = log_lines_prompt(&[record("boom")], "my check logs");
    assert_eq!(
      prompt,
      "Here are 1 log line from my check logs.\n\n```\n2026-10-19T10:00:01.250Z  [INFO]  build/Test  boom\n```\n\n\nPlease help me understand what went wrong and how to fix it."
    );
  }

  #[test]
  fn prompts_for_a_result_set() {
    let result = SqlResult {
      columns: vec!["a".into(), "b".into()],
      rows: vec![vec![Some("1".into()), None]],
      row_count: 1,
    };
    let prompt = sql_result_prompt(&result, " SELECT 1 \n");
    assert!(prompt.contains("```sql\nSELECT 1\n```"), "{prompt}");
    assert!(
      prompt.contains("Result (1 row):\n```\na\tb\n1\tNULL\n```"),
      "{prompt}"
    );
  }

  #[test]
  fn agent_chats_skip_blank_messages() {
    let chat = AgentChat {
      session_id: 7,
      pty_session_id: String::new(),
      name: "Fix".into(),
      agent: AgentType::Claude,
      workspace_id: None,
      created_at: String::new(),
      screen_before_last_user_message: String::new(),
      messages: vec![
        ChatMessage {
          id: 1,
          role: ChatRole::User,
          message: "Hi".into(),
          time: "t".into(),
        },
        ChatMessage {
          id: 2,
          role: ChatRole::Agent,
          message: "  ".into(),
          time: "t".into(),
        },
      ],
    };
    let records = agent_chat_records(&chat);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].job_id, "user");
    assert_eq!(records[0].run_id, 7);
  }

  #[test]
  fn formats_the_time_of_day() {
    assert_eq!(format_timestamp("2026-10-19T10:00:01.250Z"), "10:00:01.250");
    assert_eq!(format_timestamp("later"), "later");
  }

  #[test]
  fn labels_the_filter_by_what_is_ticked() {
    let options = vec![("ci.yml".to_string(), "CI".to_string())];
    assert_eq!(
      filter_label(&[], &options, "All checks", "checks"),
      "All checks"
    );
    assert_eq!(
      filter_label(&["ci.yml".into()], &options, "All checks", "checks"),
      "CI"
    );
    assert_eq!(
      filter_label(&["a".into(), "b".into()], &options, "All checks", "checks"),
      "2 checks"
    );
  }

  #[test]
  fn drags_a_run_of_lines_and_toggles_in_multi_select() {
    let mut s = LineSelection::default();
    s.mouse_down(4);
    s.mouse_enter(2);
    assert_eq!(s.selected, BTreeSet::from([2, 3, 4]));
    s.mouse_up();
    s.mouse_enter(9);
    assert_eq!(s.selected, BTreeSet::from([2, 3, 4]));
    s.toggle_multi();
    s.mouse_down(1);
    s.mouse_down(5);
    s.mouse_down(1);
    assert_eq!(s.selected, BTreeSet::from([5]));
  }

  #[test]
  fn finds_lines_ignoring_case() {
    let records = [record("Error here"), record("ok"), record("an error")];
    assert_eq!(find_matches(&records, "ERROR"), vec![0, 2]);
    assert!(find_matches(&records, "  ").is_empty());
  }

  #[test]
  fn charts_quiet_seconds_as_empty_slots() {
    let chart = chart(&[
      bucket("2026-10-19T10:00:03Z", "ERROR", 2),
      bucket("2026-10-19T10:00:01Z", "INFO", 5),
      bucket("2026-10-19T10:00:01Z", "WARN", 1),
    ]);
    assert_eq!(chart.labels, vec!["10:00:01", "10:00:02", "10:00:03"]);
    assert_eq!(chart.counts, vec![[5, 1, 0], [0, 0, 0], [0, 0, 2]]);
  }

  #[test]
  fn charts_a_long_range_in_merged_columns() {
    // 20 000 populated seconds, 7 s apart: about 39 hours.
    let buckets: Vec<_> = (0..20_000)
      .map(|i| {
        let at = chrono::DateTime::from_timestamp(1_785_000_000 + i * 7, 0).unwrap();
        bucket(&at.format("%Y-%m-%dT%H:%M:%SZ").to_string(), "INFO", 2)
      })
      .collect();
    let chart = chart(&buckets);
    assert!(chart.labels.len() <= 200 && chart.labels.len() > 100);
    assert_eq!(chart.labels.len(), chart.counts.len());
    // Merging loses no records.
    assert_eq!(chart.counts.iter().map(|c| c[0]).sum::<i64>(), 40_000);
  }
}
