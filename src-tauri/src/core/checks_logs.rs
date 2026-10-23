//! Log collection and querying for workflow check runs.
//!
//! Each job's step output is appended as an OpenTelemetry log record into a
//! per-date SQLite database at `.treq/telemetry-{YYYY-MM-DD}.db`. Records
//! follow the OpenTelemetry log data model, so the `logs` view exposes the
//! standard field names (with `body`/`resource`/`instrumentationScope`/
//! `attributes` stored as JSON columns, readable with SQLite's `json_extract`).
//!
//! Reads ATTACH every dated database found under `.treq/` and union them into
//! one `logs` view, so queries can span dates without the caller needing to
//! know which files exist (see
//! <https://stackoverflow.com/questions/6824717/sqlite-how-do-you-join-tables-from-different-databases>).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Describes the entity that generated the log, per OTel resource conventions.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Resource {
  #[serde(rename = "service.name")]
  pub service_name: String,
  #[serde(rename = "service.version")]
  pub service_version: String,
}

impl Default for Resource {
  fn default() -> Self {
    Self {
      service_name: "treq".to_string(),
      service_version: env!("CARGO_PKG_VERSION").to_string(),
    }
  }
}

/// The scope that emitted the log.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct InstrumentationScope {
  pub name: String,
  pub version: String,
}

impl Default for InstrumentationScope {
  fn default() -> Self {
    Self {
      name: "treq.checks".to_string(),
      version: env!("CARGO_PKG_VERSION").to_string(),
    }
  }
}

/// Structured log body. OTel allows any value; a map keeps room to grow
/// without turning `body` into a bare string later.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Body {
  pub message: String,
}

/// Everything specific to treq lives here rather than as top-level fields, so
/// the record stays exactly the OTel log data model.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Attributes {
  pub run_id: i64,
  pub job_id: String,
  pub step_index: i64,
  pub step_name: String,
  /// OTel's convention for which standard stream a line came from.
  #[serde(rename = "log.iostream")]
  pub log_iostream: String,
}

/// One captured output line as an OpenTelemetry LogRecord.
///
/// The field set is exactly the OTel log data model — no treq-specific columns
/// at the top level. `traceId`/`spanId` are derived from the run and job so a
/// run reads as a trace and each job as a span within it.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
  /// RFC3339 UTC with nanosecond precision: fixed width, so it sorts
  /// correctly as text.
  pub timestamp: String,
  pub observed_timestamp: String,
  pub trace_id: String,
  pub span_id: String,
  /// W3C trace flags; bit 0 is "sampled".
  pub trace_flags: u8,
  pub severity_text: String,
  pub severity_number: u8,
  pub body: Body,
  pub resource: Resource,
  pub instrumentation_scope: InstrumentationScope,
  pub attributes: Attributes,
  /// Identifies the class of event these records represent.
  pub event_name: String,
}

/// Every record we emit is a line of step output.
const EVENT_NAME: &str = "check.step.output";

/// Marks records as sampled, the only trace-flag bit that applies here.
const TRACE_FLAGS_SAMPLED: u8 = 1;

/// How long a dated telemetry database is kept before being deleted.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

// ── Timestamps ───────────────────────────────────────────────────────────────

/// Current time as RFC3339 UTC with nanosecond precision.
pub fn now_timestamp() -> String {
  format_timestamp(
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(0),
  )
}

/// Render epoch nanoseconds as fixed-width RFC3339.
pub fn format_timestamp(unix_nano: u64) -> String {
  let secs = (unix_nano / 1_000_000_000) as i64;
  let nanos = (unix_nano % 1_000_000_000) as u32;
  chrono::DateTime::from_timestamp(secs, nanos)
    .unwrap_or_default()
    .format("%Y-%m-%dT%H:%M:%S%.9fZ")
    .to_string()
}

/// Current UTC date as `YYYY-MM-DD`, used to name the day's telemetry database.
fn current_date_utc() -> String {
  chrono::Utc::now().format("%Y-%m-%d").to_string()
}

// ── OTel severity ────────────────────────────────────────────────────────────

/// UI-facing level names mapped to their OTel severity text.
pub fn severity_text_for_level(level: &str) -> &'static str {
  match level {
    "error" => "ERROR",
    "warning" => "WARN",
    _ => "INFO",
  }
}

/// OTel severity numbers: INFO=9, WARN=13, ERROR=17.
pub fn severity_number_for_text(severity_text: &str) -> u8 {
  match severity_text {
    "ERROR" => 17,
    "WARN" => 13,
    _ => 9,
  }
}

/// A run becomes a trace; ids are derived so they are stable and reproducible
/// rather than random, which keeps re-reads of a log file consistent.
pub fn trace_id_for_run(run_id: i64) -> String {
  format!("{:032x}", run_id as u128)
}

/// Each job becomes a span within its run's trace.
pub fn span_id_for_job(run_id: i64, job_id: &str) -> String {
  let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
  for byte in job_id.as_bytes() {
    hash ^= *byte as u64;
    hash = hash.wrapping_mul(0x1000_0000_01b3);
  }
  hash ^= run_id as u64;
  format!("{:016x}", hash)
}

/// Build an OTel record for one captured line.
#[allow(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
pub fn make_log_line(
  timestamp: String,
  run_id: i64,
  job_id: &str,
  step_index: i64,
  step_name: &str,
  stream: &str,
  level: &str,
  message: &str,
) -> LogLine {
  let severity_text = severity_text_for_level(level);
  LogLine {
    observed_timestamp: timestamp.clone(),
    timestamp,
    trace_id: trace_id_for_run(run_id),
    span_id: span_id_for_job(run_id, job_id),
    trace_flags: TRACE_FLAGS_SAMPLED,
    severity_text: severity_text.to_string(),
    severity_number: severity_number_for_text(severity_text),
    body: Body {
      message: message.to_string(),
    },
    resource: Resource::default(),
    instrumentation_scope: InstrumentationScope::default(),
    attributes: Attributes {
      run_id,
      job_id: job_id.to_string(),
      step_index,
      step_name: step_name.to_string(),
      log_iostream: stream.to_string(),
    },
    event_name: EVENT_NAME.to_string(),
  }
}

/// Filters accepted by the logs browser.
///
/// `levels` is a set: an empty or absent list means "no level filter", matching
/// the multi-select showing nothing ticked.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct LogQuery {
  pub levels: Option<Vec<String>>,
  pub search: Option<String>,
  pub step_index: Option<i64>,
  /// Workflow filenames; keeps only records from the runs of any of them.
  /// An empty or absent list means "no workflow filter".
  pub workflows: Option<Vec<String>>,
  pub limit: Option<i64>,
  pub offset: Option<i64>,
}

/// Flattened projection of a stored record, for the UI only.
///
/// This is the shape the browser renders; it is not what the `logs` view
/// exposes, which stays exactly the OTel field set.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LogRecordView {
  pub timestamp: String,
  pub severity_number: u8,
  pub severity_text: String,
  pub body: String,
  pub trace_id: String,
  pub span_id: String,
  pub run_id: i64,
  pub job_id: String,
  pub step_index: i64,
  pub step_name: String,
  pub stream: String,
}

/// Result of an ad-hoc SQL query in the explorer.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SqlResult {
  pub columns: Vec<String>,
  /// Every cell rendered as text so any column type survives the boundary.
  pub rows: Vec<Vec<Option<String>>>,
  pub row_count: usize,
}

const DEFAULT_LIMIT: i64 = 2000;

// ── Paths ────────────────────────────────────────────────────────────────────

/// Directory holding the repo's dated telemetry databases.
fn telemetry_dir(repo_path: &str) -> PathBuf {
  Path::new(repo_path).join(".treq")
}

/// Path of the telemetry database for a given UTC date (`YYYY-MM-DD`).
fn telemetry_db_path(repo_path: &str, date: &str) -> PathBuf {
  telemetry_dir(repo_path).join(format!("telemetry-{}.db", date))
}

/// A marker recorded per job so `has_logs` can be answered without touching
/// SQLite. The value carries no filesystem meaning: records are looked up by
/// `run_id`/`job_id` across every dated database.
pub fn job_log_relative_path(run_id: i64, job_id: &str) -> String {
  format!("telemetry:{}:{}", run_id, job_id)
}

// ── Level inference ──────────────────────────────────────────────────────────

/// Classify a line by content.
///
/// Deliberately not keyed on the stream: plenty of tools (cargo, npm) write
/// ordinary progress to stderr, so treating stderr as an error would paint
/// most of a healthy run red.
pub fn infer_level(message: &str) -> &'static str {
  let lower = message.to_lowercase();

  let error_markers = [
    "error",
    "error:",
    "fatal",
    "panic",
    "failed",
    "failure",
    "exception",
    "traceback",
  ];
  let warn_markers = ["warning", "warn:", "deprecated"];

  if error_markers.iter().any(|m| lower.contains(m)) {
    return "error";
  }
  if warn_markers.iter().any(|m| lower.contains(m)) {
    return "warning";
  }
  "info"
}

/// Remove ANSI SGR/CSI escape sequences so stored text renders cleanly.
pub fn strip_ansi(input: &str) -> String {
  let mut out = String::with_capacity(input.len());
  let mut chars = input.chars().peekable();
  while let Some(c) = chars.next() {
    if c == '\u{1b}' {
      // Consume "[ ... <final byte>" of a CSI sequence.
      if chars.peek() == Some(&'[') {
        chars.next();
        for c2 in chars.by_ref() {
          if c2.is_ascii_alphabetic() {
            break;
          }
        }
      }
      continue;
    }
    out.push(c);
  }
  out
}

// ── Schema ───────────────────────────────────────────────────────────────────

const CREATE_LOGS_TABLE: &str = "CREATE TABLE IF NOT EXISTS logs (
  timestamp TEXT NOT NULL,
  observed_timestamp TEXT NOT NULL,
  trace_id TEXT NOT NULL,
  span_id TEXT NOT NULL,
  trace_flags INTEGER NOT NULL,
  severity_text TEXT NOT NULL,
  severity_number INTEGER NOT NULL,
  body TEXT NOT NULL,
  resource TEXT NOT NULL,
  instrumentation_scope TEXT NOT NULL,
  attributes TEXT NOT NULL,
  event_name TEXT NOT NULL,
  run_id INTEGER NOT NULL,
  job_id TEXT NOT NULL,
  step_index INTEGER NOT NULL,
  step_name TEXT NOT NULL,
  log_iostream TEXT NOT NULL
)";

fn ensure_schema(conn: &Connection) -> Result<(), String> {
  conn
    .execute_batch(CREATE_LOGS_TABLE)
    .map_err(|e| format!("Failed to create logs table: {}", e))?;
  conn
    .execute_batch(
      "CREATE INDEX IF NOT EXISTS idx_logs_run_job ON logs(run_id, job_id);
       CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);",
    )
    .map_err(|e| format!("Failed to create logs indexes: {}", e))?;
  Ok(())
}

/// Open a connection tuned for a single writer appending to its own file,
/// while tolerating concurrent readers/writers via WAL.
///
/// Switching a brand-new file to WAL mode takes an exclusive lock of its own,
/// and that lock isn't reliably covered by `PRAGMA busy_timeout`'s retry
/// logic: concurrent job writers can all open the same day's database for the
/// first time at once, and lose that race with an immediate "database is
/// locked" instead of a wait. So retry the whole open+configure step with a
/// short backoff rather than trusting busy_timeout alone.
fn open_writable(path: &Path) -> Result<Connection, String> {
  let mut last_err = String::new();
  for attempt in 0..50 {
    if attempt > 0 {
      std::thread::sleep(Duration::from_millis(20));
    }
    let conn = match Connection::open(path) {
      Ok(conn) => conn,
      Err(e) => {
        last_err = format!("Failed to open telemetry database: {}", e);
        continue;
      }
    };
    match conn.execute_batch(
      "PRAGMA busy_timeout=5000;
       PRAGMA journal_mode=WAL;
       PRAGMA synchronous=NORMAL;",
    ) {
      Ok(()) => return Ok(conn),
      Err(e) => last_err = format!("Failed to configure telemetry database: {}", e),
    }
  }
  Err(last_err)
}

/// Delete dated telemetry databases (and their WAL/SHM sidecars) older than
/// `max_age`, based on the date encoded in the filename.
fn cleanup_old_telemetry_dbs(dir: &Path, max_age: Duration) {
  let Some(cutoff) = chrono::Utc::now()
    .checked_sub_signed(chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::zero()))
  else {
    return;
  };
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let name = entry.file_name();
    let Some(name) = name.to_str() else { continue };
    let Some(date_part) = name
      .strip_prefix("telemetry-")
      .and_then(|s| s.strip_suffix(".db"))
    else {
      continue;
    };
    let Ok(date) = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d") else {
      continue;
    };
    if date < cutoff.date_naive() {
      let _ = std::fs::remove_file(dir.join(name));
      let _ = std::fs::remove_file(dir.join(format!("{}-wal", name)));
      let _ = std::fs::remove_file(dir.join(format!("{}-shm", name)));
    }
  }
}

// ── Writing ──────────────────────────────────────────────────────────────────

/// Append-only sink shared by a job's stdout and stderr reader threads.
#[derive(Clone)]
pub struct LogWriter {
  inner: Arc<Mutex<Connection>>,
}

impl LogWriter {
  pub fn create(repo_path: &str, _run_id: i64, _job_id: &str) -> Result<Self, String> {
    let dir = telemetry_dir(repo_path);
    std::fs::create_dir_all(&dir)
      .map_err(|e| format!("Failed to create .treq directory: {}", e))?;
    cleanup_old_telemetry_dbs(&dir, MAX_AGE);

    let db_path = telemetry_db_path(repo_path, &current_date_utc());
    let conn = open_writable(&db_path)?;
    ensure_schema(&conn)?;

    Ok(Self {
      inner: Arc::new(Mutex::new(conn)),
    })
  }

  pub fn write_line(&self, line: &LogLine) -> Result<(), String> {
    let body = serde_json::to_string(&line.body).map_err(|e| e.to_string())?;
    let resource = serde_json::to_string(&line.resource).map_err(|e| e.to_string())?;
    let scope = serde_json::to_string(&line.instrumentation_scope).map_err(|e| e.to_string())?;
    let attributes = serde_json::to_string(&line.attributes).map_err(|e| e.to_string())?;

    let conn = self
      .inner
      .lock()
      .map_err(|_| "Log writer mutex poisoned".to_string())?;
    conn
      .execute(
        "INSERT INTO logs (
          timestamp, observed_timestamp, trace_id, span_id, trace_flags,
          severity_text, severity_number, body, resource, instrumentation_scope,
          attributes, event_name, run_id, job_id, step_index, step_name, log_iostream
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
          line.timestamp,
          line.observed_timestamp,
          line.trace_id,
          line.span_id,
          line.trace_flags,
          line.severity_text,
          line.severity_number,
          body,
          resource,
          scope,
          attributes,
          line.event_name,
          line.attributes.run_id,
          line.attributes.job_id,
          line.attributes.step_index,
          line.attributes.step_name,
          line.attributes.log_iostream,
        ],
      )
      .map_err(|e| format!("Failed to write log line: {}", e))?;
    Ok(())
  }

  /// SQLite commits each `INSERT` as it happens, so there is nothing to flush;
  /// kept so callers don't need to change.
  pub fn flush(&self) -> Result<(), String> {
    Ok(())
  }
}

// ── Querying (SQLite, cross-date via ATTACH) ────────────────────────────────

/// Escape a value for embedding in a single-quoted SQL string literal.
fn sql_quote(value: &str) -> String {
  value.replace('\'', "''")
}

/// Every dated telemetry database found for a repo, oldest name first.
fn telemetry_db_files(repo_path: &str) -> Vec<PathBuf> {
  let dir = telemetry_dir(repo_path);
  let Ok(entries) = std::fs::read_dir(&dir) else {
    return vec![];
  };
  let mut files: Vec<PathBuf> = entries
    .flatten()
    .map(|e| e.path())
    .filter(|p| {
      p.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("telemetry-") && n.ends_with(".db"))
        .unwrap_or(false)
    })
    .collect();
  files.sort();
  files
}

/// True when at least one telemetry database exists, so callers can skip
/// opening SQLite entirely.
fn has_any_logs(repo_path: &str) -> bool {
  !telemetry_db_files(repo_path).is_empty()
}

/// Open an in-memory connection with every dated database ATTACHed and a
/// `logs` view exposing exactly the OpenTelemetry log fields (camelCased),
/// matching what the `logs.rs` docstring promises: no treq-specific columns
/// at the top level — those stay reachable through `attributes`.
///
/// A second view, `logs_raw`, is kept internal (not documented to explorer
/// users) with the flattened `run_id`/`job_id`/`step_index`/`step_name`/
/// `log_iostream` columns, so the UI's own queries don't have to pay for
/// `json_extract` on every row.
fn connect_with_logs_view(repo_path: &str) -> Result<Connection, String> {
  let conn =
    Connection::open_in_memory().map_err(|e| format!("Failed to open SQLite connection: {}", e))?;

  let files = telemetry_db_files(repo_path);
  let mut union_parts = Vec::with_capacity(files.len());
  for (idx, path) in files.iter().enumerate() {
    let alias = format!("t{}", idx);
    conn
      .execute_batch(&format!(
        "ATTACH DATABASE '{}' AS {}",
        sql_quote(&path.to_string_lossy()),
        alias
      ))
      .map_err(|e| format!("Failed to attach {}: {}", path.display(), e))?;
    union_parts.push(format!("SELECT * FROM {}.logs", alias));
  }

  let raw_sql = if union_parts.is_empty() {
    "CREATE TEMP VIEW logs_raw AS SELECT
        '' AS timestamp, '' AS observed_timestamp, '' AS trace_id, '' AS span_id,
        0 AS trace_flags, '' AS severity_text, 0 AS severity_number,
        '' AS body, '' AS resource, '' AS instrumentation_scope, '' AS attributes,
        '' AS event_name, 0 AS run_id, '' AS job_id, 0 AS step_index, '' AS step_name,
        '' AS log_iostream
      WHERE 0"
      .to_string()
  } else {
    format!(
      "CREATE TEMP VIEW logs_raw AS {}",
      union_parts.join(" UNION ALL ")
    )
  };
  conn
    .execute_batch(&raw_sql)
    .map_err(|e| format!("Failed to register logs_raw view: {}", e))?;

  conn
    .execute_batch(
      "CREATE TEMP VIEW logs AS SELECT
         timestamp,
         observed_timestamp AS observedTimestamp,
         trace_id AS traceId,
         span_id AS spanId,
         trace_flags AS traceFlags,
         severity_text AS severityText,
         severity_number AS severityNumber,
         body,
         resource,
         instrumentation_scope AS instrumentationScope,
         attributes,
         event_name AS eventName
       FROM logs_raw",
    )
    .map_err(|e| format!("Failed to register logs view: {}", e))?;

  Ok(conn)
}

/// Shared WHERE builder for the internal (non-explorer) readers, which query
/// `logs_raw`'s flattened columns.
///
/// `levels` carries UI names (info/warning/error); they are translated to OTel
/// severity text here so the stored records stay standard.
fn build_where_clause(query: &LogQuery, extra: &[String]) -> String {
  let mut conditions: Vec<String> = extra.to_vec();

  if let Some(levels) = query.levels.as_ref().filter(|l| !l.is_empty()) {
    let list = levels
      .iter()
      .map(|l| format!("'{}'", sql_quote(severity_text_for_level(l))))
      .collect::<Vec<_>>()
      .join(", ");
    conditions.push(format!("severity_text IN ({})", list));
  }
  if let Some(search) = query.search.as_ref().filter(|s| !s.is_empty()) {
    conditions.push(format!(
      "lower(json_extract(body, '$.message')) LIKE '%{}%'",
      sql_quote(&search.to_lowercase())
    ));
  }
  if let Some(step_index) = query.step_index {
    conditions.push(format!("step_index = {}", step_index));
  }

  if conditions.is_empty() {
    String::new()
  } else {
    format!("WHERE {}", conditions.join(" AND "))
  }
}

/// Condition limiting records to the runs of any workflow in
/// `query.workflows`, if the list is non-empty.
///
/// Log records carry only the run id, so each workflow is resolved to its run
/// ids through the local db. This keeps logs written before the filter
/// existed filterable too.
fn workflow_conditions(repo_path: &str, query: &LogQuery) -> Result<Vec<String>, String> {
  let filenames: Vec<&String> = query
    .workflows
    .iter()
    .flatten()
    .filter(|w| !w.is_empty())
    .collect();
  if filenames.is_empty() {
    return Ok(vec![]);
  }
  let mut ids = Vec::new();
  for filename in filenames {
    ids.extend(crate::local_db::list_workflow_run_ids(repo_path, filename)?);
  }
  if ids.is_empty() {
    return Ok(vec!["0".to_string()]);
  }
  let list = ids
    .iter()
    .map(|id| id.to_string())
    .collect::<Vec<_>>()
    .join(", ");
  Ok(vec![format!("run_id IN ({})", list)])
}

/// Projection feeding the UI's flat row struct, read straight off `logs_raw`.
const UI_PROJECTION: &str = "SELECT
      timestamp,
      severity_number,
      severity_text,
      json_extract(body, '$.message') AS body,
      trace_id,
      span_id,
      run_id,
      job_id,
      step_index,
      step_name,
      log_iostream";

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<LogRecordView> {
  Ok(LogRecordView {
    timestamp: row.get(0)?,
    severity_number: row.get::<_, i64>(1)? as u8,
    severity_text: row.get(2)?,
    body: row.get(3)?,
    trace_id: row.get(4)?,
    span_id: row.get(5)?,
    run_id: row.get(6)?,
    job_id: row.get(7)?,
    step_index: row.get(8)?,
    step_name: row.get(9)?,
    stream: row.get(10)?,
  })
}

/// Read a single job's logs, applying the browser's filters.
///
/// Returns an empty vec when nothing has been recorded — a job that produced
/// no output is a normal state, not an error.
pub fn query_logs(
  repo_path: &str,
  run_id: i64,
  job_id: &str,
  query: &LogQuery,
) -> Result<Vec<LogRecordView>, String> {
  if !has_any_logs(repo_path) {
    return Ok(vec![]);
  }
  let conn = connect_with_logs_view(repo_path)?;

  let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100_000);
  let offset = query.offset.unwrap_or(0).max(0);
  let extra = vec![
    format!("run_id = {}", run_id),
    format!("job_id = '{}'", sql_quote(job_id)),
  ];
  let sql = format!(
    "SELECT * FROM ({} FROM logs_raw {}) AS records
         ORDER BY timestamp, step_index LIMIT {} OFFSET {}",
    UI_PROJECTION,
    build_where_clause(query, &extra),
    limit,
    offset
  );

  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare log query: {}", e))?;
  let rows = stmt
    .query_map([], row_to_record)
    .map_err(|e| format!("Failed to query logs: {}", e))?;

  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read log rows: {}", e))
}

// ── Repo-wide data source ────────────────────────────────────────────────────

/// Browse log records across every run in the repo.
pub fn query_repo_logs(repo_path: &str, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
  if !has_any_logs(repo_path) {
    return Ok(vec![]);
  }
  let conn = connect_with_logs_view(repo_path)?;

  let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100_000);
  let offset = query.offset.unwrap_or(0).max(0);
  let sql = format!(
    "SELECT * FROM ({} FROM logs_raw {}) AS records
         ORDER BY run_id DESC, timestamp, step_index LIMIT {} OFFSET {}",
    UI_PROJECTION,
    build_where_clause(query, &workflow_conditions(repo_path, query)?),
    limit,
    offset
  );

  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare repo log query: {}", e))?;
  let rows = stmt
    .query_map([], row_to_record)
    .map_err(|e| format!("Failed to query repo logs: {}", e))?;

  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read repo log rows: {}", e))
}

// ── Timeseries ───────────────────────────────────────────────────────────────

/// One chart bucket: a time slot and how many records of each severity landed
/// in it.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LogBucket {
  pub bucket: String,
  pub severity_text: String,
  pub count: i64,
}

/// Count records per time bucket and severity, for the chart above the feed.
///
/// The bucket width is caller-supplied because the useful resolution depends on
/// how much history is on screen.
pub fn query_log_timeseries(
  repo_path: &str,
  query: &LogQuery,
  bucket_seconds: i64,
) -> Result<Vec<LogBucket>, String> {
  if !has_any_logs(repo_path) {
    return Ok(vec![]);
  }
  let conn = connect_with_logs_view(repo_path)?;
  let width = bucket_seconds.clamp(1, 86_400);

  // Integer division truncates toward the earlier bucket for our
  // always-non-negative epoch seconds, matching a floor().
  let sql = format!(
    "SELECT strftime('%Y-%m-%dT%H:%M:%SZ',
                (unixepoch(timestamp) / {w}) * {w}, 'unixepoch') AS bucket,
                severity_text AS severity_text,
                count(*) AS n
         FROM logs_raw {}
         GROUP BY bucket, severity_text
         ORDER BY bucket, severity_text",
    build_where_clause(query, &workflow_conditions(repo_path, query)?),
    w = width
  );

  let mut stmt = conn
    .prepare(&sql)
    .map_err(|e| format!("Failed to prepare timeseries query: {}", e))?;
  let rows = stmt
    .query_map([], |row| {
      Ok(LogBucket {
        bucket: row.get(0)?,
        severity_text: row.get(1)?,
        count: row.get(2)?,
      })
    })
    .map_err(|e| format!("Failed to query timeseries: {}", e))?;

  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| format!("Failed to read timeseries rows: {}", e))
}

// ── Logs explorer ────────────────────────────────────────────────────────────

/// Statement kinds the explorer will run. Everything else is rejected so a
/// stray ATTACH/PRAGMA can't reach outside the `logs` view.
const ALLOWED_SQL_PREFIXES: [&str; 5] =
  ["select", "with", "explain", "pragma table_info", "values"];

/// Reject anything that isn't a single read-only statement.
fn validate_sql(sql: &str) -> Result<(), String> {
  let trimmed = sql.trim().trim_end_matches(';').trim();
  if trimmed.is_empty() {
    return Err("Query is empty".to_string());
  }
  // One statement only: a second one could smuggle in a write.
  if trimmed.contains(';') {
    return Err("Only a single statement can be run at a time".to_string());
  }

  let lower = trimmed.to_lowercase();
  if !ALLOWED_SQL_PREFIXES.iter().any(|p| {
    lower
      .strip_prefix(p)
      .is_some_and(|rest| rest.starts_with(char::is_whitespace))
  }) {
    return Err("Only read-only queries are allowed (SELECT, WITH, VALUES, EXPLAIN)".to_string());
  }

  // These can still appear mid-statement, e.g. inside a CTE body.
  let blocked = [
    "attach ", "detach ", "copy ", "install ", "load ", "export ", "import ", "create ", "insert ",
    "update ", "delete ", "drop ", "alter ", "vacuum", "reindex",
  ];
  if let Some(word) = blocked.iter().find(|w| lower.contains(*w)) {
    return Err(format!(
      "Statement contains a disallowed keyword: {}",
      word.trim()
    ));
  }
  Ok(())
}

/// Run an ad-hoc read-only query against the `logs` view.
pub fn run_logs_sql(repo_path: &str, sql: &str, max_rows: i64) -> Result<SqlResult, String> {
  validate_sql(sql)?;

  // With no logs recorded yet the views are empty, so a query returns no rows.
  let conn = connect_with_logs_view(repo_path)?;
  let capped = format!(
    "SELECT * FROM ({}) AS explorer_query LIMIT {}",
    sql.trim().trim_end_matches(';'),
    max_rows.clamp(1, 10_000)
  );

  let mut stmt = conn
    .prepare(&capped)
    .map_err(|e| format!("Query error: {}", e))?;
  let columns: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();

  let mut rows = stmt.query([]).map_err(|e| format!("Query error: {}", e))?;
  let mut out_rows: Vec<Vec<Option<String>>> = Vec::new();

  while let Some(row) = rows.next().map_err(|e| format!("Query error: {}", e))? {
    let mut cells = Vec::with_capacity(columns.len());
    for idx in 0..columns.len() {
      let value = row
        .get_ref(idx)
        .map_err(|e| format!("Query error: {}", e))?;
      let cell = match value {
        rusqlite::types::ValueRef::Null => None,
        rusqlite::types::ValueRef::Integer(i) => Some(i.to_string()),
        rusqlite::types::ValueRef::Real(f) => Some(f.to_string()),
        rusqlite::types::ValueRef::Text(t) => Some(String::from_utf8_lossy(t).to_string()),
        rusqlite::types::ValueRef::Blob(_) => Some("<blob>".to_string()),
      };
      cells.push(cell);
    }
    out_rows.push(cells);
  }

  let row_count = out_rows.len();
  Ok(SqlResult {
    columns,
    rows: out_rows,
    row_count,
  })
}

/// Render a job's logs as a plain text file suitable for sharing.
pub fn export_logs(
  repo_path: &str,
  run_id: i64,
  job_id: &str,
  dest_path: &str,
) -> Result<String, String> {
  let lines = query_logs(
    repo_path,
    run_id,
    job_id,
    &LogQuery {
      limit: Some(100_000),
      ..Default::default()
    },
  )?;

  let mut out = String::new();
  for line in &lines {
    out.push_str(&format!(
      "{}  [{}] {}\n",
      line.timestamp, line.severity_text, line.body
    ));
  }

  if let Some(parent) = Path::new(dest_path).parent() {
    std::fs::create_dir_all(parent)
      .map_err(|e| format!("Failed to create export directory: {}", e))?;
  }
  std::fs::write(dest_path, out).map_err(|e| format!("Failed to write export file: {}", e))?;
  Ok(dest_path.to_string())
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  /// Fixed epoch base so bucketing assertions stay deterministic.
  const BASE_NANOS: u64 = 1_785_000_000_000_000_000;

  fn sample_line(step_index: i64, level: &str, message: &str) -> LogLine {
    sample_line_at(
      BASE_NANOS + (step_index as u64) * 1_000_000_000,
      step_index,
      level,
      message,
    )
  }

  fn sample_line_at(nanos: u64, step_index: i64, level: &str, message: &str) -> LogLine {
    make_log_line(
      format_timestamp(nanos),
      1,
      "job",
      step_index,
      &format!("step {}", step_index),
      "stdout",
      level,
      message,
    )
  }

  fn write_log(dir: &TempDir, run_id: i64, job_id: &str, lines: &[LogLine]) {
    let writer = LogWriter::create(&dir.path().to_string_lossy(), run_id, job_id).unwrap();
    for line in lines {
      writer.write_line(line).unwrap();
    }
    writer.flush().unwrap();
  }

  #[test]
  fn test_infer_level_classifies_errors_and_warnings() {
    assert_eq!(infer_level("error: build failed"), "error");
    assert_eq!(infer_level("warning: unused variable"), "warning");
    assert_eq!(infer_level("Compiling treq v0.1.3"), "info");
  }

  #[test]
  fn test_strip_ansi_removes_color_codes() {
    assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m text"), "red text");
    assert_eq!(strip_ansi("plain text"), "plain text");
  }

  #[test]
  fn test_query_logs_returns_empty_without_any_db() {
    let dir = TempDir::new().unwrap();
    let result = query_logs(
      &dir.path().to_string_lossy(),
      1,
      "job",
      &LogQuery::default(),
    )
    .unwrap();
    assert!(result.is_empty());
  }

  #[test]
  fn test_query_logs_reads_written_lines() {
    let dir = TempDir::new().unwrap();
    write_log(
      &dir,
      1,
      "job",
      &[
        sample_line(0, "info", "hello"),
        sample_line(1, "error", "boom"),
      ],
    );
    let result = query_logs(
      &dir.path().to_string_lossy(),
      1,
      "job",
      &LogQuery::default(),
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].body, "hello");
  }

  #[test]
  fn test_query_logs_filters_by_level() {
    let dir = TempDir::new().unwrap();
    write_log(
      &dir,
      1,
      "job",
      &[
        sample_line(0, "info", "hello"),
        sample_line(1, "error", "boom"),
      ],
    );
    let result = query_logs(
      &dir.path().to_string_lossy(),
      1,
      "job",
      &LogQuery {
        levels: Some(vec!["error".to_string()]),
        ..Default::default()
      },
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].body, "boom");
  }

  #[test]
  fn test_query_logs_filters_by_search_and_step() {
    let dir = TempDir::new().unwrap();
    write_log(
      &dir,
      1,
      "job",
      &[
        sample_line(0, "info", "compiling crate"),
        sample_line(1, "info", "linking binary"),
      ],
    );
    let by_search = query_logs(
      &dir.path().to_string_lossy(),
      1,
      "job",
      &LogQuery {
        search: Some("LINKING".to_string()),
        ..Default::default()
      },
    )
    .unwrap();
    assert_eq!(by_search.len(), 1);

    let by_step = query_logs(
      &dir.path().to_string_lossy(),
      1,
      "job",
      &LogQuery {
        step_index: Some(0),
        ..Default::default()
      },
    )
    .unwrap();
    assert_eq!(by_step.len(), 1);
  }

  #[test]
  fn test_query_logs_filters_by_multiple_levels() {
    let dir = TempDir::new().unwrap();
    write_log(
      &dir,
      1,
      "job",
      &[
        sample_line(0, "info", "hello"),
        sample_line(1, "warning", "careful"),
        sample_line(2, "error", "boom"),
      ],
    );
    let result = query_logs(
      &dir.path().to_string_lossy(),
      1,
      "job",
      &LogQuery {
        levels: Some(vec!["warning".to_string(), "error".to_string()]),
        ..Default::default()
      },
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.iter().all(|l| l.severity_text != "INFO"));
  }

  #[test]
  fn test_empty_levels_list_does_not_filter() {
    let dir = TempDir::new().unwrap();
    write_log(
      &dir,
      1,
      "job",
      &[
        sample_line(0, "info", "hello"),
        sample_line(1, "error", "boom"),
      ],
    );
    let result = query_logs(
      &dir.path().to_string_lossy(),
      1,
      "job",
      &LogQuery {
        levels: Some(vec![]),
        ..Default::default()
      },
    )
    .unwrap();
    assert_eq!(result.len(), 2);
  }

  #[test]
  fn test_query_logs_scopes_to_run_and_job() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    for (run_id, job_id, message) in [
      (1i64, "build", "run1 build"),
      (1i64, "test", "run1 test"),
      (2i64, "build", "run2 build"),
    ] {
      let writer = LogWriter::create(&repo, run_id, job_id).unwrap();
      writer
        .write_line(&make_log_line(
          format_timestamp(BASE_NANOS),
          run_id,
          job_id,
          0,
          "step 0",
          "stdout",
          "info",
          message,
        ))
        .unwrap();
      writer.flush().unwrap();
    }

    let result = query_logs(&repo, 1, "build", &LogQuery::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].body, "run1 build");
  }

  #[test]
  fn test_concurrent_writers_to_same_day_do_not_error() {
    // Regression test: multiple jobs in one run (or across runs on the same
    // day) all write into the same dated database concurrently. Each opens
    // its own connection and races to set journal_mode/create the schema on
    // first use -- without busy_timeout configured before that race, SQLite
    // returns "database is locked" instead of waiting its turn.
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();

    let handles: Vec<_> = (0..8)
      .map(|i| {
        let repo = repo.clone();
        std::thread::spawn(move || {
          let job_id = format!("job-{}", i);
          let writer = LogWriter::create(&repo, 1, &job_id).unwrap();
          for step in 0..20 {
            writer
              .write_line(&make_log_line(
                now_timestamp(),
                1,
                &job_id,
                step,
                "step",
                "stdout",
                "info",
                "line",
              ))
              .unwrap();
          }
          writer.flush().unwrap();
        })
      })
      .collect();

    for handle in handles {
      handle.join().unwrap();
    }

    let result = query_repo_logs(&repo, &LogQuery::default()).unwrap();
    assert_eq!(result.len(), 8 * 20);
  }

  #[test]
  fn test_query_repo_logs_spans_runs_with_ids() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    for run_id in [1i64, 2i64] {
      let writer = LogWriter::create(&repo, run_id, "build").unwrap();
      writer
        .write_line(&make_log_line(
          format_timestamp(BASE_NANOS),
          run_id,
          "build",
          0,
          "step 0",
          "stdout",
          "info",
          &format!("run {}", run_id),
        ))
        .unwrap();
      writer.flush().unwrap();
    }

    let result = query_repo_logs(&repo, &LogQuery::default()).unwrap();
    assert_eq!(result.len(), 2);
    // Newest run first.
    assert_eq!(result[0].run_id, 2);
    assert_eq!(result[0].job_id, "build");
  }

  /// Write one info line for a run created in the local db under `filename`.
  fn write_workflow_run_line(repo: &str, filename: &str, message: &str) -> i64 {
    let run_id = crate::local_db::create_workflow_run(repo, 1, filename).unwrap();
    let writer = LogWriter::create(repo, run_id, "build").unwrap();
    writer
      .write_line(&make_log_line(
        format_timestamp(BASE_NANOS),
        run_id,
        "build",
        0,
        "step 0",
        "stdout",
        "info",
        message,
      ))
      .unwrap();
    writer.flush().unwrap();
    run_id
  }

  #[test]
  fn test_query_repo_logs_filters_by_workflow() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    write_workflow_run_line(&repo, "ci.yaml", "from ci");
    write_workflow_run_line(&repo, "docs.yaml", "from docs");

    let query = LogQuery {
      workflows: Some(vec!["docs.yaml".to_string()]),
      ..Default::default()
    };
    let result = query_repo_logs(&repo, &query).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].body, "from docs");

    let all = query_repo_logs(&repo, &LogQuery::default()).unwrap();
    assert_eq!(all.len(), 2);
  }

  #[test]
  fn test_query_repo_logs_unions_selected_workflows() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    write_workflow_run_line(&repo, "ci.yaml", "from ci");
    write_workflow_run_line(&repo, "docs.yaml", "from docs");
    write_workflow_run_line(&repo, "lint.yaml", "from lint");

    let query = LogQuery {
      workflows: Some(vec!["ci.yaml".to_string(), "lint.yaml".to_string()]),
      ..Default::default()
    };
    let mut bodies: Vec<String> = query_repo_logs(&repo, &query)
      .unwrap()
      .into_iter()
      .map(|r| r.body)
      .collect();
    bodies.sort();
    assert_eq!(bodies, vec!["from ci", "from lint"]);
  }

  #[test]
  fn test_query_repo_logs_empty_workflow_list_keeps_all() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    write_workflow_run_line(&repo, "ci.yaml", "from ci");
    write_workflow_run_line(&repo, "docs.yaml", "from docs");
    write_workflow_run_line(&repo, "lint.yaml", "from lint");

    let query = LogQuery {
      workflows: Some(vec![]),
      ..Default::default()
    };
    assert_eq!(query_repo_logs(&repo, &query).unwrap().len(), 3);
  }

  #[test]
  fn test_query_repo_logs_empty_for_workflow_without_runs() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    write_workflow_run_line(&repo, "ci.yaml", "from ci");

    let query = LogQuery {
      workflows: Some(vec!["never.yaml".to_string()]),
      ..Default::default()
    };
    assert!(query_repo_logs(&repo, &query).unwrap().is_empty());
  }

  #[test]
  fn test_query_log_timeseries_filters_by_workflow() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    write_workflow_run_line(&repo, "ci.yaml", "from ci");
    write_workflow_run_line(&repo, "docs.yaml", "from docs");

    let query = LogQuery {
      workflows: Some(vec!["ci.yaml".to_string()]),
      ..Default::default()
    };
    let total: i64 = query_log_timeseries(&repo, &query, 1)
      .unwrap()
      .iter()
      .map(|b| b.count)
      .sum();
    assert_eq!(total, 1);
  }

  #[test]
  fn test_query_repo_logs_empty_without_runs() {
    let dir = TempDir::new().unwrap();
    let result = query_repo_logs(&dir.path().to_string_lossy(), &LogQuery::default()).unwrap();
    assert!(result.is_empty());
  }

  #[test]
  fn test_validate_sql_rejects_writes_and_multiple_statements() {
    assert!(validate_sql("SELECT * FROM logs").is_ok());
    assert!(validate_sql("WITH x AS (SELECT 1) SELECT * FROM x").is_ok());
    assert!(validate_sql("DROP TABLE logs").is_err());
    assert!(validate_sql("SELECT 1; DROP TABLE logs").is_err());
    assert!(validate_sql("ATTACH DATABASE '/tmp/x.db' AS x").is_err());
    assert!(validate_sql("").is_err());
  }

  #[test]
  fn test_run_logs_sql_returns_columns_and_rows() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let writer = LogWriter::create(&repo, 1, "build").unwrap();
    writer.write_line(&sample_line(0, "error", "boom")).unwrap();
    writer.write_line(&sample_line(1, "info", "fine")).unwrap();
    writer.flush().unwrap();

    let result = run_logs_sql(
      &repo,
      "SELECT severityText, count(*) AS n FROM logs GROUP BY severityText ORDER BY severityText",
      100,
    )
    .unwrap();
    assert_eq!(result.columns, vec!["severityText", "n"]);
    assert_eq!(result.row_count, 2);
  }

  #[test]
  fn test_run_logs_sql_returns_no_rows_before_any_logs() {
    let dir = TempDir::new().unwrap();
    let result = run_logs_sql(
      &dir.path().to_string_lossy(),
      "SELECT timestamp, severityText FROM logs",
      100,
    )
    .unwrap();
    assert_eq!(result.columns, vec!["timestamp", "severityText"]);
    assert_eq!(result.row_count, 0);
  }

  #[test]
  fn test_run_logs_sql_rejects_disallowed_statement() {
    let dir = TempDir::new().unwrap();
    let err = run_logs_sql(&dir.path().to_string_lossy(), "DELETE FROM logs", 100).unwrap_err();
    assert!(err.contains("read-only"));
  }

  #[test]
  fn test_records_are_written_in_otel_shape() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let writer = LogWriter::create(&repo, 7, "build").unwrap();
    writer
      .write_line(&make_log_line(
        format_timestamp(BASE_NANOS),
        7,
        "build",
        2,
        "Compile",
        "stderr",
        "error",
        "boom",
      ))
      .unwrap();
    writer.flush().unwrap();

    let result = run_logs_sql(
      &repo,
      "SELECT severityText, severityNumber, json_extract(body, '$.message') AS message, \
       eventName, traceFlags, json_extract(resource, '$.\"service.name\"') AS service_name, \
       json_extract(instrumentationScope, '$.name') AS scope_name, \
       json_extract(attributes, '$.run_id') AS run_id, \
       json_extract(attributes, '$.job_id') AS job_id, \
       json_extract(attributes, '$.step_index') AS step_index, \
       json_extract(attributes, '$.\"log.iostream\"') AS iostream, \
       length(traceId) AS trace_id_len, length(spanId) AS span_id_len, \
       timestamp, observedTimestamp \
       FROM logs",
      10,
    )
    .unwrap();

    assert_eq!(result.row_count, 1);
    let row = &result.rows[0];
    assert_eq!(row[0].as_deref(), Some("ERROR"));
    assert_eq!(row[1].as_deref(), Some("17"));
    assert_eq!(row[2].as_deref(), Some("boom"));
    assert_eq!(row[3].as_deref(), Some("check.step.output"));
    assert_eq!(row[4].as_deref(), Some("1"));
    assert_eq!(row[5].as_deref(), Some("treq"));
    assert_eq!(row[6].as_deref(), Some("treq.checks"));
    assert_eq!(row[7].as_deref(), Some("7"));
    assert_eq!(row[8].as_deref(), Some("build"));
    assert_eq!(row[9].as_deref(), Some("2"));
    assert_eq!(row[10].as_deref(), Some("stderr"));
    assert_eq!(row[11].as_deref(), Some("32"));
    assert_eq!(row[12].as_deref(), Some("16"));
    assert_eq!(row[13], row[14]);
    assert!(row[13].as_deref().unwrap().ends_with('Z'));
  }

  #[test]
  fn test_severity_mapping_covers_all_levels() {
    assert_eq!(severity_text_for_level("info"), "INFO");
    assert_eq!(severity_text_for_level("warning"), "WARN");
    assert_eq!(severity_text_for_level("error"), "ERROR");
    assert_eq!(severity_number_for_text("INFO"), 9);
    assert_eq!(severity_number_for_text("WARN"), 13);
    assert_eq!(severity_number_for_text("ERROR"), 17);
  }

  #[test]
  fn test_trace_id_is_per_run_and_span_id_per_job() {
    assert_eq!(trace_id_for_run(1), trace_id_for_run(1));
    assert_ne!(trace_id_for_run(1), trace_id_for_run(2));
    assert_ne!(span_id_for_job(1, "build"), span_id_for_job(1, "test"));
  }

  #[test]
  fn test_query_log_timeseries_buckets_by_severity() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let writer = LogWriter::create(&repo, 1, "build").unwrap();
    // Two errors in the first second, one info in the next.
    writer
      .write_line(&sample_line_at(BASE_NANOS, 0, "error", "a"))
      .unwrap();
    writer
      .write_line(&sample_line_at(BASE_NANOS + 1, 0, "error", "b"))
      .unwrap();
    writer
      .write_line(&sample_line_at(BASE_NANOS + 2_000_000_000, 1, "info", "c"))
      .unwrap();
    writer.flush().unwrap();

    let buckets = query_log_timeseries(&repo, &LogQuery::default(), 1).unwrap();
    let errors: i64 = buckets
      .iter()
      .filter(|b| b.severity_text == "ERROR")
      .map(|b| b.count)
      .sum();
    let infos: i64 = buckets
      .iter()
      .filter(|b| b.severity_text == "INFO")
      .map(|b| b.count)
      .sum();
    assert_eq!(errors, 2);
    assert_eq!(infos, 1);
    // The two errors share a bucket, the info lands in a later one.
    assert!(buckets.len() >= 2);
  }

  #[test]
  fn test_timeseries_truncates_fractional_seconds_not_rounds() {
    // Both lines fall in the same integer second, but one is past the
    // half-second mark. A rounding bucket (rather than a floor) would push
    // it into the next second and split what should be one bucket in two.
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let writer = LogWriter::create(&repo, 1, "build").unwrap();
    writer
      .write_line(&sample_line_at(BASE_NANOS + 100_000_000, 0, "info", "a"))
      .unwrap();
    writer
      .write_line(&sample_line_at(BASE_NANOS + 700_000_000, 0, "info", "b"))
      .unwrap();
    writer.flush().unwrap();

    let buckets = query_log_timeseries(&repo, &LogQuery::default(), 1).unwrap();
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0].count, 2);
  }

  #[test]
  fn test_timeseries_empty_without_runs() {
    let dir = TempDir::new().unwrap();
    let buckets =
      query_log_timeseries(&dir.path().to_string_lossy(), &LogQuery::default(), 1).unwrap();
    assert!(buckets.is_empty());
  }

  #[test]
  fn test_logs_view_exposes_otel_columns() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let writer = LogWriter::create(&repo, 1, "build").unwrap();
    writer.write_line(&sample_line(0, "info", "hello")).unwrap();
    writer.flush().unwrap();

    let result = run_logs_sql(
            &repo,
            "SELECT timestamp, severityText, json_extract(body, '$.message') AS message, json_extract(attributes, '$.run_id') AS run_id FROM logs",
            10,
        )
        .unwrap();
    assert_eq!(
      result.columns,
      vec!["timestamp", "severityText", "message", "run_id"]
    );
    assert_eq!(result.row_count, 1);
  }

  #[test]
  fn test_export_logs_writes_plain_text() {
    let dir = TempDir::new().unwrap();
    write_log(
      &dir,
      1,
      "job",
      &[
        sample_line(0, "info", "hello"),
        sample_line(1, "error", "boom"),
      ],
    );
    let dest = dir.path().join("out.log").to_string_lossy().to_string();
    export_logs(&dir.path().to_string_lossy(), 1, "job", &dest).unwrap();
    let contents = std::fs::read_to_string(&dest).unwrap();
    assert!(contents.contains("[ERROR] boom"));
    assert_eq!(contents.lines().count(), 2);
  }

  #[test]
  fn test_cross_date_query_unions_dated_databases() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().to_string();
    let telemetry_dir = dir.path().join(".treq");
    std::fs::create_dir_all(&telemetry_dir).unwrap();

    // Simulate "yesterday"'s database directly, since LogWriter always
    // writes to today's file.
    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1))
      .format("%Y-%m-%d")
      .to_string();
    let conn = open_writable(&telemetry_dir.join(format!("telemetry-{}.db", yesterday))).unwrap();
    ensure_schema(&conn).unwrap();
    let line = sample_line(0, "info", "from yesterday");
    conn
      .execute(
        "INSERT INTO logs (
          timestamp, observed_timestamp, trace_id, span_id, trace_flags,
          severity_text, severity_number, body, resource, instrumentation_scope,
          attributes, event_name, run_id, job_id, step_index, step_name, log_iostream
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
          line.timestamp,
          line.observed_timestamp,
          line.trace_id,
          line.span_id,
          line.trace_flags,
          line.severity_text,
          line.severity_number,
          serde_json::to_string(&line.body).unwrap(),
          serde_json::to_string(&line.resource).unwrap(),
          serde_json::to_string(&line.instrumentation_scope).unwrap(),
          serde_json::to_string(&line.attributes).unwrap(),
          line.event_name,
          line.attributes.run_id,
          line.attributes.job_id,
          line.attributes.step_index,
          line.attributes.step_name,
          line.attributes.log_iostream,
        ],
      )
      .unwrap();
    drop(conn);

    write_log(&dir, 1, "job", &[sample_line(1, "info", "from today")]);

    let result = query_repo_logs(&repo, &LogQuery::default()).unwrap();
    let bodies: Vec<&str> = result.iter().map(|r| r.body.as_str()).collect();
    assert!(bodies.contains(&"from yesterday"));
    assert!(bodies.contains(&"from today"));
  }
}
