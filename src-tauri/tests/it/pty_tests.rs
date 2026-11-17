use crate::common::TestRepo;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use treq_lib::pty::{line_matches_auto_command, strip_ansi_codes, PtyManager, PtyOutput};

/// Helper: create a PtyManager and an output capture buffer.
fn setup() -> (PtyManager, Arc<Mutex<String>>) {
  let manager = PtyManager::new();
  let output = Arc::new(Mutex::new(String::new()));
  (manager, output)
}

/// Helper: build a callback that appends output to the shared buffer.
fn make_callback(output: &Arc<Mutex<String>>) -> PtyOutput {
  let output = Arc::clone(output);
  Box::new(move |data: &[u8]| {
    let mut buf = output.lock().unwrap();
    buf.push_str(&String::from_utf8_lossy(data));
  })
}

/// Helper: wait for output buffer to contain a substring (with timeout).
fn wait_for_output(output: &Arc<Mutex<String>>, needle: &str, timeout_ms: u64) -> bool {
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(timeout_ms);
  loop {
    {
      let buf = output.lock().unwrap();
      if buf.contains(needle) {
        return true;
      }
    }
    if start.elapsed() > timeout {
      return false;
    }
    thread::sleep(Duration::from_millis(50));
  }
}

/// Helper: wait until the buffer holds any output (the shell prompt), or the timeout passes.
fn wait_for_shell_ready(output: &Arc<Mutex<String>>, timeout_ms: u64) {
  wait_for_output_len_above(output, 0, timeout_ms);
}

/// Helper: wait until the buffer grows past `len` bytes (with timeout).
fn wait_for_output_len_above(output: &Arc<Mutex<String>>, len: usize, timeout_ms: u64) -> bool {
  let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
  while std::time::Instant::now() < deadline {
    if output.lock().unwrap().len() > len {
      return true;
    }
    thread::sleep(Duration::from_millis(20));
  }
  false
}

#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
  Command::new("kill")
    .args(["-0", &pid.to_string()])
    .status()
    .map(|status| status.success())
    .unwrap_or(false)
}

#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
  let output = Command::new("tasklist")
    .args(["/FI", &format!("PID eq {pid}")])
    .output();

  match output {
    Ok(output) => {
      let stdout = String::from_utf8_lossy(&output.stdout);
      stdout.contains(&pid.to_string()) && !stdout.contains("No tasks are running")
    }
    Err(_) => false,
  }
}

#[test]
fn test_pty_manager_new() {
  let manager = PtyManager::new();
  assert!(!manager.session_exists("nonexistent-session"));
  assert!(!manager.session_exists(""));
  assert!(!manager.session_exists("abc-123"));
}

#[test]
fn test_create_session_with_initial_command() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  let (manager, output) = setup();

  let result = manager.create_session(
    "test-initial-cmd".to_string(),
    None,
    Some(repo.repo_path),
    None,
    Vec::new(),
    Some("echo HELLO_FROM_INIT".to_string()),
    None,
    make_callback(&output),
  );

  assert!(result.is_ok());

  // Wait for the initial command output
  let found = wait_for_output(&output, "HELLO_FROM_INIT", 5000);
  assert!(
    found,
    "Expected 'HELLO_FROM_INIT' in output, got: {}",
    output.lock().unwrap()
  );

  let _ = manager.close_session("test-initial-cmd");
}

#[test]
fn test_write_to_nonexistent_session() {
  let manager = PtyManager::new();

  let result = manager.write_to_session("does-not-exist", "hello\n");
  assert!(result.is_err());
  assert_eq!(result.unwrap_err(), "Session not found");
}

#[test]
fn test_resize_nonexistent_session() {
  let manager = PtyManager::new();

  let result = manager.resize_session("does-not-exist", 24, 80);
  assert!(result.is_err());
  assert_eq!(result.unwrap_err(), "Session not found");
}

// Consolidates what used to be 7 separate tests (create, write, resize, utf8
// output, set_auto_command, close, close-terminates-process), each of which
// spawned its own shell. Windows' PowerShell cold-start (multiple seconds
// under CI contention, per pty.rs's slow-timeout comments) dominates this
// suite's wall time there, so reusing one spawned shell across these checks
// is the biggest lever for cutting it down. Kept in one test (rather than a
// shared-fixture-across-tests helper) so the checks stay ordered and the
// session isn't torn down or raced by parallel test execution.
#[test]
fn test_session_lifecycle_and_io() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  let (manager, output) = setup();
  let session_id = "test-lifecycle";

  let result = manager.create_session(
    session_id.to_string(),
    None,
    Some(repo.repo_path),
    None,
    Vec::new(),
    None,
    None,
    make_callback(&output),
  );
  assert!(
    result.is_ok(),
    "create_session should succeed: {:?}",
    result
  );
  assert!(manager.session_exists(session_id));

  let pid = manager
    .session_process_id(session_id)
    .expect("session should expose a process id");
  assert!(
    is_process_alive(pid),
    "Process should be alive after create. pid={pid}"
  );

  wait_for_shell_ready(&output, 5000);

  // --- write_to_session ---
  manager
    .write_to_session(session_id, "echo WRITE_TEST_OK\n")
    .expect("write_to_session should succeed");
  assert!(
    wait_for_output(&output, "WRITE_TEST_OK", 5000),
    "Expected 'WRITE_TEST_OK' in output, got: {}",
    output.lock().unwrap()
  );

  // --- resize_session ---
  assert!(
    manager.resize_session(session_id, 48, 120).is_ok(),
    "resize_session should succeed"
  );
  assert!(
    manager.resize_session(session_id, 24, 80).is_ok(),
    "resize_session to different dimensions should also succeed"
  );

  // --- utf8 output ---
  manager
    .write_to_session(session_id, "echo '你好世界'\n")
    .expect("write CJK");
  assert!(
    wait_for_output(&output, "你好世界", 5000),
    "CJK output expected, got: {}",
    output.lock().unwrap()
  );

  manager
    .write_to_session(session_id, "echo '🎉🚀✨'\n")
    .expect("write emoji");
  assert!(
    wait_for_output(&output, "🎉🚀✨", 5000),
    "Emoji output expected, got: {}",
    output.lock().unwrap()
  );

  manager
    .write_to_session(session_id, "echo '€£¥©®™'\n")
    .expect("write symbols");
  assert!(
    wait_for_output(&output, "€£¥©®™", 5000),
    "Symbol output expected, got: {}",
    output.lock().unwrap()
  );

  // --- set_auto_command (API surface only; filtering behavior is covered
  // by test_echo_suppression_filtering) ---
  assert!(
    manager
      .set_auto_command(session_id, "some long test command string here")
      .is_ok(),
    "set_auto_command should succeed on an existing session"
  );

  // --- close_session, idempotency, and process termination ---
  assert!(manager.close_session(session_id).is_ok());
  assert!(!manager.session_exists(session_id));
  assert!(
    manager.close_session(session_id).is_ok(),
    "closing again should still succeed (idempotent)"
  );

  let deadline = std::time::Instant::now() + Duration::from_secs(3);
  while std::time::Instant::now() < deadline && is_process_alive(pid) {
    thread::sleep(Duration::from_millis(50));
  }
  assert!(
    !is_process_alive(pid),
    "Process should be terminated after close. pid={pid}"
  );
}

#[test]
fn test_multiple_concurrent_sessions() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  let manager = PtyManager::new();

  let output_a = Arc::new(Mutex::new(String::new()));
  let output_b = Arc::new(Mutex::new(String::new()));
  let output_c = Arc::new(Mutex::new(String::new()));

  // Create 3 sessions
  manager
    .create_session(
      "multi-a".to_string(),
      None,
      Some(repo.repo_path.clone()),
      None,
      Vec::new(),
      None,
      None,
      make_callback(&output_a),
    )
    .expect("create session A");

  manager
    .create_session(
      "multi-b".to_string(),
      None,
      Some(repo.repo_path.clone()),
      None,
      Vec::new(),
      None,
      None,
      make_callback(&output_b),
    )
    .expect("create session B");

  manager
    .create_session(
      "multi-c".to_string(),
      None,
      Some(repo.repo_path),
      None,
      Vec::new(),
      None,
      None,
      make_callback(&output_c),
    )
    .expect("create session C");

  assert!(manager.session_exists("multi-a"));
  assert!(manager.session_exists("multi-b"));
  assert!(manager.session_exists("multi-c"));

  wait_for_shell_ready(&output_a, 5000);
  wait_for_shell_ready(&output_b, 5000);
  wait_for_shell_ready(&output_c, 5000);

  // Write unique commands to each session
  manager
    .write_to_session("multi-a", "echo SESSION_A_OUTPUT\n")
    .expect("write to A");
  manager
    .write_to_session("multi-b", "echo SESSION_B_OUTPUT\n")
    .expect("write to B");
  manager
    .write_to_session("multi-c", "echo SESSION_C_OUTPUT\n")
    .expect("write to C");

  // Verify each session got its output
  assert!(
    wait_for_output(&output_a, "SESSION_A_OUTPUT", 5000),
    "Session A output: {}",
    output_a.lock().unwrap()
  );
  assert!(
    wait_for_output(&output_b, "SESSION_B_OUTPUT", 5000),
    "Session B output: {}",
    output_b.lock().unwrap()
  );
  assert!(
    wait_for_output(&output_c, "SESSION_C_OUTPUT", 5000),
    "Session C output: {}",
    output_c.lock().unwrap()
  );

  // Cleanup
  let _ = manager.close_session("multi-a");
  let _ = manager.close_session("multi-b");
  let _ = manager.close_session("multi-c");
}

#[test]
fn test_session_isolation() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  let manager = PtyManager::new();

  let output_a = Arc::new(Mutex::new(String::new()));
  let output_b = Arc::new(Mutex::new(String::new()));

  manager
    .create_session(
      "iso-a".to_string(),
      None,
      Some(repo.repo_path.clone()),
      None,
      Vec::new(),
      None,
      None,
      make_callback(&output_a),
    )
    .expect("create session A");

  manager
    .create_session(
      "iso-b".to_string(),
      None,
      Some(repo.repo_path),
      None,
      Vec::new(),
      None,
      None,
      make_callback(&output_b),
    )
    .expect("create session B");

  wait_for_shell_ready(&output_a, 5000);
  wait_for_shell_ready(&output_b, 5000);

  // Write a unique marker to session A only
  manager
    .write_to_session("iso-a", "echo UNIQUE_MARKER_ISOLATION_A\n")
    .expect("write to A");

  // Wait for A's output
  assert!(
    wait_for_output(&output_a, "UNIQUE_MARKER_ISOLATION_A", 5000),
    "Session A should have the marker"
  );

  // Round-trip a command through session B so its reader has drained
  // everything sent before it
  manager
    .write_to_session("iso-b", "echo ISOLATION_B_DRAINED\n")
    .expect("write to B");
  assert!(
    wait_for_output(&output_b, "ISOLATION_B_DRAINED", 5000),
    "Session B output: {}",
    output_b.lock().unwrap()
  );

  // Session B should NOT contain the marker from session A
  let b_output = output_b.lock().unwrap();
  assert!(
    !b_output.contains("UNIQUE_MARKER_ISOLATION_A"),
    "Session B should not contain session A's output, but got: {}",
    b_output
  );

  let _ = manager.close_session("iso-a");
  let _ = manager.close_session("iso-b");
}

#[test]
fn test_strip_ansi_codes_plain() {
  assert_eq!(strip_ansi_codes("hello world"), "hello world");
}

#[test]
fn test_strip_ansi_codes_csi() {
  assert_eq!(strip_ansi_codes("\x1b[32mhello\x1b[0m"), "hello");
  assert_eq!(strip_ansi_codes("\x1b[1;31mred\x1b[0m"), "red");
}

#[test]
fn test_strip_ansi_codes_osc() {
  assert_eq!(strip_ansi_codes("\x1b]0;title\x07text"), "text");
}

#[test]
fn test_strip_ansi_codes_charset() {
  assert_eq!(strip_ansi_codes("\x1b(Bhello"), "hello");
}

#[test]
fn test_line_matches_auto_command_short_line() {
  // Lines shorter than 20 chars should never match
  assert!(!line_matches_auto_command(
    "short",
    "some long auto command that is definitely long enough"
  ));
}

#[test]
fn test_line_matches_auto_command_exact_substring() {
  let auto_cmd = "claude --permission-mode acceptEdits --append-system-prompt 'some long prompt'";
  let line = "claude --permission-mode acceptEdits --append-system-prompt 'some long prompt'";
  assert!(line_matches_auto_command(line, auto_cmd));
}

#[test]
fn test_line_matches_auto_command_partial_overlap() {
  let auto_cmd = "claude --permission-mode acceptEdits --append-system-prompt 'very long system prompt text here'";
  // A line that contains a 20+ char substring of the auto command
  let line = "$ claude --permission-mode acceptEdits --append-system-prompt 'very long system prompt text here'";
  assert!(line_matches_auto_command(line, auto_cmd));
}

#[test]
fn test_line_matches_auto_command_no_match() {
  let auto_cmd = "claude --permission-mode acceptEdits --append-system-prompt 'some prompt'";
  let line = "total 42\ndrwxr-xr-x  5 user  staff  160 Jan  1 00:00 .";
  assert!(!line_matches_auto_command(line, auto_cmd));
}

#[test]
fn test_line_matches_auto_command_normal_output() {
  let auto_cmd =
    "claude --permission-mode acceptEdits --append-system-prompt 'hello world this is a test'";
  // Normal CLI output should not match
  assert!(!line_matches_auto_command(
    "Hello! How can I help you today?",
    auto_cmd
  ));
  assert!(!line_matches_auto_command(
    "Processing your request...",
    auto_cmd
  ));
}

#[test]
fn test_set_auto_command_nonexistent_session() {
  let manager = PtyManager::new();
  let result = manager.set_auto_command("nonexistent", "some command");
  assert!(result.is_err());
  assert_eq!(result.unwrap_err(), "Session not found");
}

// Consolidates what used to be 3 separate tests (suppress_echo_filters_command,
// normal_output_not_filtered_without_filter, empty_lines_filtered_during_suppression)
// onto one spawned shell, run sequentially. Each stage resolves the filter it sets
// (the reader thread clears auto_command once the command's echo has passed)
// before the next stage runs, so state doesn't leak across stages; see
// test_suppress_echo_at_creation_filters_initial_prompt (kept separate, needs a
// session with no prior commands) for the filter-suppresses-the-shell-prompt case.
#[test]
fn test_echo_suppression_filtering() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  let (manager, output) = setup();
  let session_id = "test-filtering";

  manager
    .create_session(
      session_id.to_string(),
      None,
      Some(repo.repo_path),
      None,
      Vec::new(),
      None,
      None,
      make_callback(&output),
    )
    .expect("create_session should succeed");

  wait_for_shell_ready(&output, 5000);

  // --- filtered command echo, followed by visible output ---
  // Simulates the real workflow: set_auto_command + write the matching command.
  // `true` produces no output, so only the echo is filtered.
  let suppress_cmd = "true # suppress-test-unique-ident-1234567890";
  manager
    .set_auto_command(session_id, suppress_cmd)
    .expect("set_auto_command");
  let len_before = output.lock().unwrap().len();
  manager
    .write_to_session(session_id, &format!("{}\n", suppress_cmd))
    .expect("write filtered command");

  // Output after the hidden echo (the next prompt) means the filter has resolved
  wait_for_output_len_above(&output, len_before, 5000);

  manager
    .write_to_session(session_id, "echo VISIBLE_AFTER\n")
    .expect("write follow-up command");
  assert!(
    wait_for_output(&output, "VISIBLE_AFTER", 5000),
    "Expected output after filtered command echo, got: {}",
    output.lock().unwrap()
  );

  // --- output passes through normally once the filter has resolved ---
  manager
    .write_to_session(session_id, "echo NORMAL_OUTPUT_VISIBLE\n")
    .expect("write command");
  assert!(
    wait_for_output(&output, "NORMAL_OUTPUT_VISIBLE", 5000),
    "Normal output should not be filtered, got: {}",
    output.lock().unwrap()
  );

  // --- empty lines during suppression are filtered, real output after them appears ---
  let trigger_cmd = "echo TRIGGER_ECHO_MATCH_1234567890_ABCDE";
  manager
    .set_auto_command(session_id, trigger_cmd)
    .expect("set_auto_command");
  let len_before = output.lock().unwrap().len();
  manager
    .write_to_session(session_id, &format!("{}\n", trigger_cmd))
    .expect("write trigger command");

  wait_for_output_len_above(&output, len_before, 5000);

  manager
    .write_to_session(session_id, "echo AFTER_EMPTY_LINES\n")
    .expect("write commands");
  assert!(
    wait_for_output(&output, "AFTER_EMPTY_LINES", 5000),
    "Output after empty lines should appear, got: {}",
    output.lock().unwrap()
  );

  let _ = manager.close_session(session_id);
}

#[test]
fn test_suppress_echo_at_creation_filters_initial_prompt() {
  // Filter set at creation time suppresses shell prompt and all output
  // until the matching command echo is seen.
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  let (manager, output) = setup();

  // Use `true` so only the echo is filtered, not command output
  let filter_cmd = "true # creation-filter-unique-ident-1234567890";
  manager
    .create_session(
      "test-creation-filter".to_string(),
      None,
      Some(repo.repo_path),
      None,
      Vec::new(),
      None,
      Some(filter_cmd.to_string()),
      make_callback(&output),
    )
    .expect("create_session should succeed");

  // Wait for shell prompt to be emitted (and filtered since filter is active)
  thread::sleep(Duration::from_millis(500));

  // The initial prompt should have been suppressed.
  {
    let buf = output.lock().unwrap();
    let stripped = strip_ansi_codes(&buf);
    assert!(
      stripped.trim().is_empty(),
      "Initial prompt should be suppressed, but got: {}",
      buf.clone()
    );
  }

  // Now write the matching command (simulates ptyWriteSuppressEcho)
  manager
    .write_to_session("test-creation-filter", &format!("{}\n", filter_cmd))
    .expect("write filtered command");

  // Output after the hidden echo (the next prompt) means the filter has resolved
  wait_for_output_len_above(&output, 0, 5000);

  // Write a follow-up command to verify output works
  manager
    .write_to_session("test-creation-filter", "echo CREATION_VISIBLE\n")
    .expect("write follow-up command");

  let found = wait_for_output(&output, "CREATION_VISIBLE", 5000);
  assert!(
    found,
    "Output should appear after creation-time filter, got: {}",
    output.lock().unwrap()
  );

  let _ = manager.close_session("test-creation-filter");
}

/// Starts a session whose echo filter is set for `command`, types the command
/// the way ptyWriteSuppressEcho does, and returns the visible output lines.
#[cfg(unix)]
fn run_filtered_command(session_id: &str, command: &str, until: &str) -> Vec<String> {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  let (manager, output) = setup();
  manager
    .create_session(
      session_id.to_string(),
      None,
      Some(repo.repo_path),
      Some("/bin/sh".to_string()),
      Vec::new(),
      None,
      Some(command.to_string()),
      make_callback(&output),
    )
    .expect("create_session should succeed");
  thread::sleep(Duration::from_millis(300));
  manager
    .write_to_session(session_id, &format!("{command}\r"))
    .expect("write filtered command");
  assert!(
    wait_for_output(&output, until, 5000),
    "Expected {until:?} in output, got: {}",
    output.lock().unwrap()
  );
  let _ = manager.close_session(session_id);
  let text = strip_ansi_codes(&output.lock().unwrap());
  text
    .split('\n')
    .map(|l| l.trim_end_matches('\r').to_string())
    .collect()
}

#[cfg(unix)]
#[test]
fn test_echo_suppression_keeps_agent_lines_that_quote_the_prompt() {
  let command = "printf '%s\\n' '> Rework the ranking pipeline for search' '' 'status 1' 'END_OF_AGENT_OUTPUT' # Rework the ranking pipeline for search";
  let lines = run_filtered_command("quote-prompt", command, "END_OF_AGENT_OUTPUT");

  assert!(
    !lines.iter().any(|l| l.contains("printf")),
    "The command echo should be hidden, got: {lines:?}"
  );
  let first = lines
    .iter()
    .position(|l| l == "> Rework the ranking pipeline for search")
    .unwrap_or_else(|| panic!("Agent line quoting the prompt was dropped: {lines:?}"));
  assert_eq!(
    lines[first + 1..first + 3],
    ["".to_string(), "status 1".to_string()],
    "Blank agent output lines should be kept, got: {lines:?}"
  );
}

#[cfg(unix)]
#[test]
fn test_echo_suppression_hides_every_line_of_a_multiline_command() {
  let command = "printf 'OUT:%s\\n' 'first line of a multi-line prompt here\n\nfinal prompt line'";
  let lines = run_filtered_command("multiline-echo", command, "OUT:first line");

  assert!(
    !lines.iter().any(|l| l.contains("final prompt line'")),
    "The echo of the command's last line should be hidden, got: {lines:?}"
  );
  assert!(
    lines.iter().any(|l| l == "final prompt line"),
    "The command's own output should be shown, got: {lines:?}"
  );
}
