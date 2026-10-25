//! Reads the app's own log files (the JSON lines `telemetry::JsonLogLayer`
//! writes) so the Logs tab can show them under its "System" source.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::checks_logs::{
  format_timestamp, severity_number_for_text, severity_text_for_level, LogQuery, LogRecordView,
};

/// Prefix of the daily-rolled log files (`treq.YYYY-MM-DD`).
pub const LOG_FILE_PREFIX: &str = "treq";

const DEFAULT_LIMIT: usize = 2000;

/// The newest `query.limit` matching records across every log file, oldest
/// first. Lines that are not valid JSON log records are skipped.
pub fn query_system_logs(log_dir: &Path, query: &LogQuery) -> Result<Vec<LogRecordView>, String> {
  let limit = query
    .limit
    .and_then(|l| usize::try_from(l).ok())
    .unwrap_or(DEFAULT_LIMIT);
  let levels: Vec<&str> = query
    .levels
    .iter()
    .flatten()
    .map(|l| severity_text_for_level(l))
    .collect();
  let search = query
    .search
    .as_deref()
    .filter(|s| !s.is_empty())
    .map(str::to_lowercase);

  let mut records = Vec::new();
  // Newest file first, newest line first, so we can stop at `limit`.
  'files: for path in log_files(log_dir).iter().rev() {
    let Ok(content) = std::fs::read_to_string(path) else {
      continue;
    };
    for line in content.lines().rev() {
      let Some(record) = parse_line(line) else {
        continue;
      };
      if !levels.is_empty() && !levels.contains(&record.severity_text.as_str()) {
        continue;
      }
      if let Some(search) = &search {
        if !record.body.to_lowercase().contains(search) {
          continue;
        }
      }
      records.push(record);
      if records.len() >= limit {
        break 'files;
      }
    }
  }
  records.reverse();
  Ok(records)
}

/// Log files in `log_dir`, oldest first (the date suffix sorts by name).
fn log_files(log_dir: &Path) -> Vec<PathBuf> {
  let prefix = format!("{}.", LOG_FILE_PREFIX);
  let Ok(entries) = std::fs::read_dir(log_dir) else {
    return Vec::new();
  };
  let mut files: Vec<PathBuf> = entries
    .flatten()
    .map(|e| e.path())
    .filter(|p| p.is_file())
    .filter(|p| {
      p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(&prefix))
    })
    .collect();
  files.sort();
  files
}

/// One JSON log line as a Logs tab record. The level column only knows
/// INFO/WARN/ERROR, so DEBUG and TRACE show as INFO. `job_id` carries where
/// the record came from: the `log` target forwarded from `tauri-plugin-log`
/// (`fields.source`), else the tracing target.
fn parse_line(line: &str) -> Option<LogRecordView> {
  let value: Value = serde_json::from_str(line).ok()?;
  let nanos = value.get("timeUnixNano")?.as_str()?.parse::<u64>().ok()?;
  let severity_text = match value.get("severityText").and_then(Value::as_str) {
    Some("ERROR") => "ERROR",
    Some("WARN") => "WARN",
    _ => "INFO",
  };
  let source = value
    .pointer("/fields/source")
    .and_then(Value::as_str)
    .or_else(|| value.get("target").and_then(Value::as_str))
    .unwrap_or_default();
  Some(LogRecordView {
    timestamp: format_timestamp(nanos),
    severity_number: severity_number_for_text(severity_text),
    severity_text: severity_text.to_string(),
    body: value
      .get("message")
      .and_then(Value::as_str)
      .unwrap_or_default()
      .to_string(),
    trace_id: String::new(),
    span_id: String::new(),
    run_id: 0,
    job_id: source.to_string(),
    step_index: 0,
    step_name: String::new(),
    stream: String::new(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn line(nanos: u64, level: &str, message: &str) -> String {
    serde_json::json!({
      "timeUnixNano": nanos.to_string(),
      "severityText": level,
      "target": "treq_lib::thing",
      "message": message,
      "fields": {},
    })
    .to_string()
  }

  fn write_logs(dir: &Path) {
    std::fs::write(
      dir.join("treq.2026-10-13"),
      format!(
        "{}\nnot json\n{}\n",
        line(1, "INFO", "older info"),
        line(2, "ERROR", "older error")
      ),
    )
    .unwrap();
    std::fs::write(
      dir.join("treq.2026-10-14"),
      format!(
        "{}\n{}\n",
        line(3, "WARN", "newer warn"),
        line(4, "DEBUG", "newer debug")
      ),
    )
    .unwrap();
    std::fs::write(dir.join("other.log"), line(5, "ERROR", "ignored")).unwrap();
  }

  fn bodies(records: &[LogRecordView]) -> Vec<&str> {
    records.iter().map(|r| r.body.as_str()).collect()
  }

  #[test]
  fn reads_all_files_oldest_first_and_skips_bad_lines() {
    let dir = tempfile::tempdir().unwrap();
    write_logs(dir.path());
    let records = query_system_logs(dir.path(), &LogQuery::default()).unwrap();
    assert_eq!(
      bodies(&records),
      vec!["older info", "older error", "newer warn", "newer debug"]
    );
    assert_eq!(records[3].severity_text, "INFO");
    assert_eq!(records[0].job_id, "treq_lib::thing");
  }

  #[test]
  fn limit_keeps_the_newest_records() {
    let dir = tempfile::tempdir().unwrap();
    write_logs(dir.path());
    let query = LogQuery {
      limit: Some(3),
      ..Default::default()
    };
    let records = query_system_logs(dir.path(), &query).unwrap();
    assert_eq!(
      bodies(&records),
      vec!["older error", "newer warn", "newer debug"]
    );
  }

  #[test]
  fn filters_by_level_and_search() {
    let dir = tempfile::tempdir().unwrap();
    write_logs(dir.path());
    let query = LogQuery {
      levels: Some(vec!["warning".to_string(), "error".to_string()]),
      search: Some("NEWER".to_string()),
      ..Default::default()
    };
    let records = query_system_logs(dir.path(), &query).unwrap();
    assert_eq!(bodies(&records), vec!["newer warn"]);
  }

  #[test]
  fn source_field_wins_over_target() {
    let record = parse_line(
      r#"{"timeUnixNano":"1","severityText":"INFO","target":"treq_lib::telemetry","message":"m","fields":{"source":"webview"}}"#,
    )
    .unwrap();
    assert_eq!(record.job_id, "webview");
  }

  #[test]
  fn missing_dir_is_empty() {
    let records =
      query_system_logs(Path::new("/nonexistent/treq-logs"), &LogQuery::default()).unwrap();
    assert!(records.is_empty());
  }

  /// Round 7 measurement: `cargo test --lib logs_perf_system -- --ignored --nocapture`.
  #[test]
  #[ignore = "perf measurement"]
  fn logs_perf_system() {
    use std::io::Write;
    use std::time::Instant;
    let dir = tempfile::tempdir().unwrap();
    for day in 0..7u64 {
      let mut f = std::io::BufWriter::new(
        std::fs::File::create(dir.path().join(format!("treq.2026-10-{:02}", 10 + day))).unwrap(),
      );
      for i in 0..150_000u64 {
        let l = if i % 5000 == 0 { "ERROR" } else if i % 50 == 0 { "WARN" } else { "INFO" };
        writeln!(f, "{}", line(day * 1_000_000 + i, l, &format!("request {i} handled by worker thread pool with a typical message length ok"))).unwrap();
      }
    }
    let bytes: u64 = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.metadata().unwrap().len()).sum();
    println!("7 files, {} MB", bytes / 1_000_000);
    let run = |label: &str, q: LogQuery| {
      let t = Instant::now();
      let r = query_system_logs(dir.path(), &q).unwrap();
      println!("{label}: {:?} rows={}", t.elapsed(), r.len());
    };
    run("default", LogQuery::default());
    run("errors only", LogQuery { levels: Some(vec!["error".into()]), ..Default::default() });
    run("search rare (miss)", LogQuery { search: Some("zzzznotfound".into()), ..Default::default() });
  }
}
