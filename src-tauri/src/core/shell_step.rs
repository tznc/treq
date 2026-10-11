//! Runs one `sh -c` script with a timeout, streaming each output line to a
//! callback. Shared by workflow check steps, the workspace setup script and
//! the application setup script.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::checks_logs::strip_ansi;

/// How long to wait, after the script exits, for its output readers to drain.
/// A background job the script started can hold the pipes open forever, so
/// the readers are never joined outright.
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Receives `(stream, line)` for each output line, with ANSI escapes removed.
/// `stream` is `"stdout"` or `"stderr"`.
pub type LineSink = Arc<dyn Fn(&'static str, &str) + Send + Sync>;

pub struct ShellStep<'a> {
  pub script: &'a str,
  pub cwd: &'a Path,
  pub env: Option<&'a HashMap<String, String>>,
  pub timeout: Duration,
}

pub enum StepOutcome {
  Exited(ExitStatus),
  /// The script outlived its timeout; it and everything it started were killed.
  TimedOut,
}

impl StepOutcome {
  pub fn success(&self) -> bool {
    matches!(self, StepOutcome::Exited(status) if status.success())
  }
}

/// Runs `step` and returns how it ended. `Err` means the script could not be
/// started or waited on.
pub fn run(step: &ShellStep, sink: LineSink) -> Result<StepOutcome, String> {
  let mut command = Command::new("sh");
  command
    .args(["-c", step.script])
    .current_dir(step.cwd)
    .env("PATH", crate::binary_paths::get_extended_path())
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  if let Some(env) = step.env {
    command.envs(env);
  }
  // Its own process group, so a timeout also kills what the script started.
  #[cfg(unix)]
  std::os::unix::process::CommandExt::process_group(&mut command, 0);
  let mut child = command.spawn().map_err(|e| e.to_string())?;

  // One thread per pipe; a single reader would deadlock on large output.
  let (done_tx, done_rx) = mpsc::channel();
  let mut readers = 0;
  if let Some(stdout) = child.stdout.take() {
    forward_lines("stdout", stdout, sink.clone(), done_tx.clone());
    readers += 1;
  }
  if let Some(stderr) = child.stderr.take() {
    forward_lines("stderr", stderr, sink, done_tx);
    readers += 1;
  }

  let outcome = wait(&mut child, step.timeout)?;
  let deadline = Instant::now() + OUTPUT_DRAIN_GRACE;
  for _ in 0..readers {
    let left = deadline.saturating_duration_since(Instant::now());
    if done_rx.recv_timeout(left).is_err() {
      break;
    }
  }
  Ok(outcome)
}

fn wait(child: &mut Child, timeout: Duration) -> Result<StepOutcome, String> {
  let deadline = Instant::now() + timeout;
  loop {
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
      return Ok(StepOutcome::Exited(status));
    }
    if Instant::now() >= deadline {
      kill_process_group(child);
      let _ = child.wait();
      return Ok(StepOutcome::TimedOut);
    }
    std::thread::sleep(Duration::from_millis(100));
  }
}

/// Reads raw lines so a non-UTF-8 byte is replaced rather than ending the
/// read: closing the pipe early would kill the script with SIGPIPE.
fn forward_lines(
  stream: &'static str,
  pipe: impl Read + Send + 'static,
  sink: LineSink,
  done: mpsc::Sender<()>,
) {
  std::thread::spawn(move || {
    let mut reader = BufReader::new(pipe);
    let mut buf = Vec::new();
    while matches!(reader.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
      let line = String::from_utf8_lossy(&buf);
      sink(stream, &strip_ansi(line.trim_end_matches(['\n', '\r'])));
      buf.clear();
    }
    let _ = done.send(());
  });
}

#[cfg(unix)]
fn kill_process_group(child: &mut Child) {
  // `process_group(0)` made the script's pid its group id.
  unsafe {
    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
  }
}

#[cfg(not(unix))]
fn kill_process_group(child: &mut Child) {
  let _ = child.kill();
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Mutex;

  fn run_collecting(script: &str, timeout: Duration) -> (StepOutcome, Vec<(String, String)>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink: LineSink = {
      let lines = lines.clone();
      Arc::new(move |stream, line| {
        lines
          .lock()
          .unwrap()
          .push((stream.to_string(), line.to_string()))
      })
    };
    let step = ShellStep {
      script,
      cwd: Path::new("."),
      env: None,
      timeout,
    };
    let outcome = run(&step, sink).unwrap();
    let lines = lines.lock().unwrap().clone();
    (outcome, lines)
  }

  #[test]
  fn forwards_every_line_before_returning() {
    let (outcome, lines) = run_collecting(
      "for i in $(seq 1 500); do echo out$i; echo err$i 1>&2; done",
      Duration::from_secs(30),
    );
    assert!(outcome.success());
    assert_eq!(lines.len(), 1000);
    assert!(lines.contains(&("stdout".into(), "out500".into())));
    assert!(lines.contains(&("stderr".into(), "err500".into())));
  }

  #[test]
  fn keeps_reading_past_invalid_utf8() {
    let (outcome, lines) = run_collecting(
      "printf 'bad \\377 byte\\n'; for i in $(seq 1 2000); do echo after$i; done",
      Duration::from_secs(30),
    );
    assert!(outcome.success());
    assert_eq!(lines[0].1, "bad \u{fffd} byte");
    assert!(lines.contains(&("stdout".into(), "after2000".into())));
  }

  #[test]
  fn kills_a_script_that_outlives_the_timeout() {
    let started = Instant::now();
    let (outcome, _) = run_collecting("sleep 30", Duration::from_millis(200));
    assert!(matches!(outcome, StepOutcome::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(10));
  }

  #[test]
  fn does_not_wait_for_background_jobs() {
    // The job inherits the script's stdout and holds it open after `sh` exits.
    let started = Instant::now();
    let (outcome, _) = run_collecting("sleep 30 &", Duration::from_secs(60));
    assert!(outcome.success());
    assert!(started.elapsed() < Duration::from_secs(10));
  }

  #[test]
  fn reports_a_failing_exit() {
    let (outcome, _) = run_collecting("exit 3", Duration::from_secs(30));
    assert!(!outcome.success());
  }
}
