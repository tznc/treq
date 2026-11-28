use crate::result_ext::ResultExt;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// jj-lib logs every file lock it takes at INFO, which would bury everything
/// else in the Logs tab's System source.
const DEFAULT_LOG_FILTER: &str = "info,jj_lib::lock=warn";

pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub struct TelemetryGuards {
  // Held only for its Drop impl, which flushes the non-blocking writer.
  _file_guard: WorkerGuard,
  /// Where the log files are written; the Logs tab's "System" source reads them.
  pub log_dir: PathBuf,
}

/// Installs a panic hook that forwards panic messages into the `tracing` pipeline
/// (so they land in the log file when a subscriber is installed) in addition to
/// running the default hook (which prints to stderr). Panic hooks run even when
/// the release profile uses `panic = "abort"`, unlike `catch_unwind`.
pub fn install_panic_hook() {
  let default_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |panic_info| {
    let location = panic_info
      .location()
      .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
      .unwrap_or_else(|| "unknown location".to_string());
    let message = panic_info
      .payload()
      .downcast_ref::<&str>()
      .map(|s| s.to_string())
      .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
      .unwrap_or_else(|| "<non-string panic payload>".to_string());
    tracing::error!(target: "panic", "panic at {}: {}", location, message);
    default_hook(panic_info);
  }));
}

/// The text a panic carried, read from the payload that a `JoinHandle::join`
/// or `catch_unwind` error holds. `panic!` with a literal gives a `&str`, with
/// format arguments a `String`.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
  payload
    .downcast_ref::<&str>()
    .copied()
    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    .unwrap_or("non-string panic payload")
}

pub fn init(log_dir: &Path) -> Result<TelemetryGuards, Box<dyn std::error::Error>> {
  std::fs::create_dir_all(log_dir)?;
  // Walks the whole log directory; nothing waits on it.
  let log_dir_owned = log_dir.to_path_buf();
  std::thread::spawn(move || cleanup_old_logs(&log_dir_owned, MAX_AGE));

  let appender = rolling::daily(log_dir, crate::core::system_logs::LOG_FILE_PREFIX);
  let (writer, file_guard) = tracing_appender::non_blocking(appender);

  let json_layer = JsonLogLayer::new(writer);

  // RUST_LOG unset is the usual case; an invalid one is reported once logging is up.
  let (filter, rust_log_error) = match EnvFilter::try_from_default_env() {
    Ok(filter) => (filter, None),
    Err(error) => (
      EnvFilter::new(DEFAULT_LOG_FILTER),
      std::env::var_os(EnvFilter::DEFAULT_ENV).map(|_| error),
    ),
  };
  let subscriber = Registry::default().with(filter).with(json_layer);
  tracing::subscriber::set_global_default(subscriber)?;
  if let Some(error) = rust_log_error {
    tracing::warn!(%error, "ignoring RUST_LOG; using {DEFAULT_LOG_FILTER}");
  }

  Ok(TelemetryGuards {
    _file_guard: file_guard,
    log_dir: log_dir.to_path_buf(),
  })
}

/// Forward a `log` record into tracing so records from `tauri-plugin-log`
/// (Rust `log::*` macros and JS `@tauri-apps/plugin-log`) end up in the log file.
pub fn forward_log_record(record: &log::Record<'_>) {
  let source = record.target();
  let msg = record.args();
  match record.level() {
    log::Level::Error => tracing::error!(source = source, "{}", msg),
    log::Level::Warn => tracing::warn!(source = source, "{}", msg),
    log::Level::Info => tracing::info!(source = source, "{}", msg),
    log::Level::Debug => tracing::debug!(source = source, "{}", msg),
    log::Level::Trace => tracing::trace!(source = source, "{}", msg),
  }
}

pub fn cleanup_old_logs(dir: &Path, max_age: Duration) {
  let Some(cutoff) = SystemTime::now().checked_sub(max_age) else {
    return;
  };
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let Ok(meta) = entry.metadata() else { continue };
    if !meta.is_file() {
      continue;
    }
    let Ok(modified) = meta.modified() else {
      continue;
    };
    if modified < cutoff {
      std::fs::remove_file(entry.path()).best_effort();
    }
  }
}

/// Minimal `tracing_subscriber::Layer` that writes one JSON object per line to
/// the log file. Replaces the previous OTel SDK pipeline (SdkLoggerProvider +
/// OpenTelemetryTracingBridge + a hand-rolled OTLP exporter) with a direct
/// tracing -> JSON path; nothing else parses this file back, so there is no
/// need to keep the OTLP field shape here (unlike `core/checks_logs.rs`,
/// which builds its own OTel-shaped records independently of this crate).
pub struct JsonLogLayer {
  writer: tracing_appender::non_blocking::NonBlocking,
}

impl JsonLogLayer {
  pub fn new(writer: tracing_appender::non_blocking::NonBlocking) -> Self {
    Self { writer }
  }
}

impl<S> Layer<S> for JsonLogLayer
where
  S: Subscriber + for<'a> LookupSpan<'a>,
{
  fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
    let mut visitor = MessageVisitor::default();
    event.record(&mut visitor);

    let time_unix_nano = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos().to_string())
      .unwrap_or_default();

    let metadata = event.metadata();
    let mut fields = Map::new();
    for (k, v) in visitor.fields {
      fields.insert(k, v);
    }

    let line = json!({
        "timeUnixNano": time_unix_nano,
        "severityText": metadata.level().as_str(),
        "target": metadata.target(),
        "message": visitor.message,
        "fields": fields,
    });

    use std::io::Write;
    let mut writer = self.writer.clone();
    writeln!(writer, "{}", line).best_effort();
  }
}

#[derive(Default)]
struct MessageVisitor {
  message: String,
  fields: Vec<(String, Value)>,
}

impl Visit for MessageVisitor {
  fn record_str(&mut self, field: &Field, value: &str) {
    if field.name() == "message" {
      self.message = value.to_string();
    } else {
      self
        .fields
        .push((field.name().to_string(), Value::String(value.to_string())));
    }
  }

  fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
    let rendered = format!("{:?}", value);
    if field.name() == "message" {
      self.message = rendered;
    } else {
      self
        .fields
        .push((field.name().to_string(), Value::String(rendered)));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::panic_message;
  use std::any::Any;

  #[test]
  fn reads_the_text_of_a_panic_payload() {
    let literal: Box<dyn Any + Send> = Box::new("boom");
    let formatted: Box<dyn Any + Send> = Box::new(format!("boom {}", 2));
    let other: Box<dyn Any + Send> = Box::new(7);
    assert_eq!(panic_message(literal.as_ref()), "boom");
    assert_eq!(panic_message(formatted.as_ref()), "boom 2");
    assert_eq!(panic_message(other.as_ref()), "non-string panic payload");
  }
}
