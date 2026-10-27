//! Runs `git` and `jj` in unit tests to build fixture repositories. Shipped
//! code must not shell out; this module only compiles under `cfg(test)`.

use std::path::Path;
use std::process::Command;

/// Runs `program args` in `cwd` and returns its trimmed stdout, panicking
/// with stderr when it fails.
pub(crate) fn run(cwd: &Path, program: &str, args: &[&str]) -> String {
  let output = Command::new(program)
    .current_dir(cwd)
    .env("JJ_USER", "Test User")
    .env("JJ_EMAIL", "test@example.com")
    .args(args)
    .output()
    .unwrap_or_else(|e| panic!("{program} {args:?}: {e}"));
  assert!(
    output.status.success(),
    "{program} {args:?} failed: {}{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(crate) fn git(cwd: &Path, args: &[&str]) -> String {
  run(cwd, "git", args)
}

pub(crate) fn jj(cwd: &Path, args: &[&str]) -> String {
  run(cwd, "jj", args)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn git_returns_trimmed_stdout() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    assert_eq!(
      git(dir.path(), &["rev-parse", "--is-inside-work-tree"]),
      "true"
    );
  }

  #[test]
  #[should_panic(expected = "git [\"no-such-command\"] failed")]
  fn a_failing_command_panics_with_its_args() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["no-such-command"]);
  }
}
