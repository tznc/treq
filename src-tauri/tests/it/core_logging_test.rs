use crate::common::TestRepo;
use serde_json::Value;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use treq_lib::telemetry::{cleanup_old_logs, JsonLogLayer};

/// True for a JSON record produced by one of this test's own
/// `forward_log_record` calls, as opposed to any stray line from elsewhere.
fn is_forwarded_record(v: &Value) -> bool {
  v["message"]
    .as_str()
    .is_some_and(|body| body.starts_with("message at "))
}

/// Drive the real logging pipeline to produce a file on disk, then rewind its
/// mtime to simulate age — much closer to what we ship than `fs::write` of
/// arbitrary bytes.
fn emit_log_line_to(dir: &std::path::Path) -> std::path::PathBuf {
  let appender = tracing_appender::rolling::daily(dir, "treq");
  let (writer, worker_guard) = tracing_appender::non_blocking(appender);

  let layer = JsonLogLayer::new(writer);
  let subscriber = Registry::default().with(layer);
  let dispatch_guard = tracing::subscriber::set_default(subscriber);

  tracing::info!(source = "seed", "seed line");

  drop(dispatch_guard);
  drop(worker_guard);

  std::fs::read_dir(dir)
    .expect("readdir")
    .flatten()
    .find(|e| e.file_name().to_string_lossy().starts_with("treq."))
    .expect("rolling appender wrote a file")
    .path()
}

/// `cleanup_old_logs` is called on every app start; it must delete files
/// older than the threshold and leave fresher files untouched.
#[test]
fn cleanup_old_logs_deletes_only_files_older_than_threshold() {
  let dir = tempfile::tempdir().expect("tempdir");

  let old = emit_log_line_to(dir.path());
  // Rewind mtime so the file looks ancient compared to the cutoff below.
  TestRepo::set_file_modified_back(&old, Duration::from_secs(60 * 60)).expect("mtime old");

  // Move the just-written file aside so a second emission produces a distinct path, then restore it.
  let stash = dir.path().join("old.stash");
  TestRepo::rename_path(&old, &stash).expect("stash old");

  let fresh = emit_log_line_to(dir.path());
  let old_final = dir.path().join("old.log");
  TestRepo::rename_path(&stash, &old_final).expect("restore old");
  TestRepo::set_file_modified_back(&old_final, Duration::from_secs(60 * 60)).expect("mtime old");

  cleanup_old_logs(dir.path(), Duration::from_secs(60));

  assert!(
    !old_final.exists(),
    "file older than threshold should be deleted"
  );
  assert!(fresh.exists(), "file newer than threshold should be kept");
}

/// `cleanup_old_logs` must skip subdirectories and non-existent paths
/// without panicking.
#[test]
fn cleanup_old_logs_is_tolerant_of_subdirs_and_missing_dir() {
  let dir = tempfile::tempdir().expect("tempdir");

  let subdir = dir.path().join("subdir");
  TestRepo::ensure_dir(&subdir).expect("mkdir");

  let old = emit_log_line_to(dir.path());
  TestRepo::set_file_modified_back(&old, Duration::from_secs(60 * 60)).expect("mtime old");

  cleanup_old_logs(dir.path(), Duration::from_secs(60));

  assert!(!old.exists(), "regular file should be deleted");
  assert!(subdir.exists(), "subdir should be left alone");

  // Missing directory — must not panic.
  cleanup_old_logs(
    std::path::Path::new("/definitely/does/not/exist/treq-test"),
    Duration::from_secs(1),
  );
}

/// End-to-end: tracing events flow through the `tracing_subscriber` JSON
/// layer and get written as one JSON object per line, preserving severity,
/// message, and any extra event fields (here, `source`, mirroring how
/// `forward_log_record` attaches the original `log` target).
///
/// Uses a thread-local subscriber (`tracing::subscriber::set_default`) rather
/// than the process-global one `telemetry::init` installs, so this test can
/// coexist with the other tests in this binary.
#[test]
fn forward_log_record_emits_json_per_level() {
  let dir = tempfile::tempdir().expect("tempdir");

  let appender = tracing_appender::rolling::daily(dir.path(), "treq");
  let (writer, worker_guard) = tracing_appender::non_blocking(appender);

  let layer = JsonLogLayer::new(writer);
  let subscriber = Registry::default().with(layer);
  let dispatch_guard = tracing::subscriber::set_default(subscriber);

  tracing::error!(source = "test_source", "message at ERROR");
  tracing::warn!(source = "test_source", "message at WARN");
  tracing::info!(source = "test_source", "message at INFO");
  tracing::debug!(source = "test_source", "message at DEBUG");
  tracing::trace!(source = "test_source", "message at TRACE");

  drop(dispatch_guard);
  drop(worker_guard); // flush non-blocking writer worker

  let log_file = std::fs::read_dir(dir.path())
    .expect("readdir")
    .flatten()
    .find(|e| e.file_name().to_string_lossy().starts_with("treq."))
    .expect("rolling appender wrote a file");

  // Flushing on drop only waits a bounded amount of time for the
  // non-blocking writer's background thread; under heavy CI load that
  // window can be missed even though the record was queued. Poll instead of
  // assuming the flush already landed by the time we read.
  let is_own_record =
    |line: &&str| serde_json::from_str::<Value>(line).is_ok_and(|v| is_forwarded_record(&v));

  let mut contents = String::new();
  let mut lines: Vec<&str> = Vec::new();
  for _ in 0..50 {
    contents = std::fs::read_to_string(log_file.path()).expect("read log file");
    lines = contents
      .lines()
      .filter(|l| !l.is_empty())
      .filter(is_own_record)
      .collect();
    if lines.len() >= 5 {
      break;
    }
    std::thread::sleep(Duration::from_millis(20));
  }
  assert_eq!(
    lines.len(),
    5,
    "expected one JSON line per forwarded record, got: {contents}"
  );

  let expected_severities = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];
  for (line, expected_severity) in lines.iter().zip(expected_severities) {
    let v: Value =
      serde_json::from_str(line).unwrap_or_else(|e| panic!("invalid JSON line {line:?}: {e}"));

    assert_eq!(
      v["severityText"], expected_severity,
      "severityText should match log level"
    );

    let message = v["message"].as_str().expect("message string");
    assert!(
      message.starts_with("message at "),
      "message should contain forwarded message, got {message:?}"
    );

    let source = v["fields"]["source"].as_str();
    assert_eq!(
      source,
      Some("test_source"),
      "original log target should be preserved as fields.source"
    );

    assert!(
      v["timeUnixNano"].is_string(),
      "timeUnixNano should be a string"
    );
  }
}
