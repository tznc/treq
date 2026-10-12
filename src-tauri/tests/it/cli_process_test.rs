//! Runs the real `treq` binary as a CLI process: agents read its exit status
//! and run it in shells with no display server.
#![cfg(desktop)]

use std::process::{Command, Output};

fn treq(cwd: &std::path::Path, args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_treq"))
    .args(args)
    .current_dir(cwd)
    .env_remove("DISPLAY")
    .env_remove("WAYLAND_DISPLAY")
    .stdin(std::process::Stdio::null())
    .output()
    .expect("run treq")
}

#[test]
fn version_exits_zero_without_a_display() {
  let dir = tempfile::tempdir().unwrap();
  let out = treq(dir.path(), &["--version"]);
  assert_eq!(out.status.code(), Some(0), "{out:?}");
  assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_exits_zero() {
  let dir = tempfile::tempdir().unwrap();
  let out = treq(dir.path(), &["--help"]);
  assert_eq!(out.status.code(), Some(0), "{out:?}");
  assert!(String::from_utf8_lossy(&out.stdout).contains("Usage"));
}

#[test]
fn unknown_subcommand_exits_with_usage_error() {
  let dir = tempfile::tempdir().unwrap();
  let out = treq(dir.path(), &["bogus"]);
  assert_eq!(out.status.code(), Some(2), "{out:?}");
  assert!(String::from_utf8_lossy(&out.stderr).contains("bogus"));
}

#[test]
fn missing_required_argument_exits_with_usage_error() {
  let dir = tempfile::tempdir().unwrap();
  let out = treq(dir.path(), &["commit", "feat/x"]);
  assert_eq!(out.status.code(), Some(2), "{out:?}");
}

#[test]
fn failing_command_exits_non_zero_without_a_display() {
  let dir = tempfile::tempdir().unwrap();
  let out = treq(dir.path(), &["st"]);
  assert_eq!(out.status.code(), Some(1), "{out:?}");
  assert!(String::from_utf8_lossy(&out.stderr).contains("Not inside a"));
}
