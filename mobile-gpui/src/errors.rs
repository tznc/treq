//! Readable errors. The transport and the remote CLI report failures as
//! `code: detail` strings (often with the code twice); screens show the
//! sentence [`describe`] makes of them, plus the one action that fixes it.

/// What the user can do about a [`Problem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
  /// Try again: the host was unreachable or the connection dropped.
  Retry,
  /// The host presented a different key; compare and trust the new one.
  ReviewHostKey,
  /// The device's access to a Treq Cloud machine ended.
  Reauthenticate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
  pub message: String,
  pub fix: Option<Fix>,
}

impl Problem {
  fn new(message: impl Into<String>, fix: Option<Fix>) -> Self {
    Self {
      message: message.into(),
      fix,
    }
  }
}

/// Codes the CLI and transport put before the detail.
const CODES: &[&str] = &[
  "transport_error",
  "dependency_error",
  "invalid_arguments",
  "invalid_remote_json",
  "treq_command_failed",
  "command_failed",
  "not_found",
  "filesystem_error",
  "jj_error",
  "git_error",
  "git_command_failed",
];

/// The sentence and action for a raw error string.
pub fn describe(raw: &str) -> Problem {
  let lower = raw.to_lowercase();
  let has = |needle: &str| lower.contains(needle);

  if has("credential_cut_off") || has("is cut off") {
    return Problem::new(
      "This device's access to the machine ended. Reauthenticate to continue.",
      Some(Fix::Reauthenticate),
    );
  }
  if has("host key mismatch") || has("unknown server key") || has("server key changed") {
    return Problem::new(
      "The host presented a different key from the one you trusted. Review the new key before connecting.",
      Some(Fix::ReviewHostKey),
    );
  }
  if has("authentication failed") || has("permission denied (publickey") {
    return Problem::new(
      "The host refused this device's key. Add the device's public key to ~/.ssh/authorized_keys on the host.",
      Some(Fix::Retry),
    );
  }
  if has("key material unavailable") {
    return Problem::new("This device's SSH key couldn't be read.", None);
  }
  if has("tmux nor screen") {
    return Problem::new(
      "Terminals need tmux or screen on the host. Install one there, then try again.",
      Some(Fix::Retry),
    );
  }
  if let Some(agent) = unknown_agent(raw) {
    return Problem::new(format!("{agent} isn't installed on this host."), None);
  }
  if has("treq: command not found") || has("treq: not found") || (has("exit=some(127)")) {
    return Problem::new(
      "treq isn't installed on this host, or isn't on the PATH for SSH sessions.",
      None,
    );
  }
  // A treq from before `file list`: clap wants `--path`, or names the action.
  if has("unknown file action 'list'")
    || (has("required arguments were not provided") && has("--path"))
  {
    return Problem::new(
      "Browsing files needs a newer treq on this host. Update treq there, then try again.",
      None,
    );
  }
  // A treq from before `git pr`: clap refuses `--title`, or names the action.
  if has("unknown git action 'pr") || (has("--title") && has("wasn't expected")) {
    return Problem::new(
      "Pull requests need a newer treq on this host. Update treq there, then try again.",
      None,
    );
  }
  if has("gh cli not found") {
    return Problem::new(
      "Pull requests use the GitHub CLI on this host, and it isn't installed. Install gh there and run gh auth login.",
      None,
    );
  }
  if has("gh auth login") || has("not logged into any github hosts") {
    return Problem::new(
      "The GitHub CLI on this host isn't signed in. Run gh auth login there, then try again.",
      None,
    );
  }
  if has("no git remote named") {
    return Problem::new(
      "This repository has no Git remote to push to. Add one on the host (git remote add origin …), then push again.",
      None,
    );
  }
  if has("connection refused") {
    return Problem::new(
      "The host refused the connection. Check the address and that SSH is running there.",
      Some(Fix::Retry),
    );
  }
  if has("ssh connection failed")
    || has("ssh channel error")
    || has("exceeded its deadline")
    || has("timed out")
    || has("failed to lookup")
    || has("no route to host")
    || has("network is unreachable")
  {
    return Problem::new(
      "Couldn't reach the host. Check that it's online and reachable from this phone.",
      Some(Fix::Retry),
    );
  }
  Problem::new(strip_codes(raw), None)
}

/// "Unknown agent 'claude'" → "Claude".
fn unknown_agent(raw: &str) -> Option<String> {
  let rest = &raw[raw.find("Unknown agent '")? + "Unknown agent '".len()..];
  let name = &rest[..rest.find('\'')?];
  Some(match name {
    "claude" => "Claude Code".into(),
    "codex" => "Codex".into(),
    "cursor-agent" => "Cursor Agent".into(),
    other => other.to_string(),
  })
}

/// Drops the leading `code: ` prefixes and capitalizes what's left.
fn strip_codes(raw: &str) -> String {
  let mut text = raw.trim();
  'strip: loop {
    for code in CODES {
      if let Some(rest) = text
        .strip_prefix(code)
        .and_then(|rest| rest.strip_prefix(':'))
      {
        text = rest.trim_start();
        continue 'strip;
      }
    }
    break;
  }
  let mut chars = text.chars();
  match chars.next() {
    Some(first) => first.to_uppercase().chain(chars).collect(),
    None => raw.to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn doubled_codes_are_dropped() {
    let problem = describe("dependency_error: dependency_error: jj is missing");
    assert_eq!(problem.message, "Jj is missing");
    assert_eq!(problem.fix, None);
  }

  #[test]
  fn unreachable_hosts_offer_a_retry() {
    for raw in [
      "transport_error: ssh channel error: Channel send error",
      "transport_error: ssh connection failed: timed out",
      "transport_error: ssh operation exceeded its deadline",
    ] {
      let problem = describe(raw);
      assert!(problem.message.starts_with("Couldn't reach"), "{raw}");
      assert_eq!(problem.fix, Some(Fix::Retry));
    }
  }

  #[test]
  fn a_changed_host_key_asks_for_review() {
    let problem = describe("transport_error: ssh connection failed: Unknown server key");
    assert_eq!(problem.fix, Some(Fix::ReviewHostKey));
    assert_eq!(
      describe("host key mismatch for endpoint user-managed-1").fix,
      Some(Fix::ReviewHostKey)
    );
  }

  #[test]
  fn a_cutoff_asks_to_reauthenticate() {
    let problem = describe("credential_cut_off: endpoint e1 (certificate expired)");
    assert_eq!(problem.fix, Some(Fix::Reauthenticate));
  }

  #[test]
  fn missing_tools_say_what_to_install() {
    assert!(
      describe("dependency_error: dependency_error: Neither tmux nor screen is installed")
        .message
        .contains("tmux or screen")
    );
    assert_eq!(
      describe("invalid_arguments: Unknown agent 'claude'").message,
      "Claude Code isn't installed on this host."
    );
  }

  #[test]
  fn a_push_without_a_remote_says_to_add_one() {
    let problem = describe(
      "git_command_failed: Push failed: IO error: Failed to push bookmark: No git remote named 'origin'",
    );
    assert!(problem
      .message
      .starts_with("This repository has no Git remote"));
  }

  #[test]
  fn github_cli_problems_say_what_to_do_on_the_host() {
    assert!(describe("treq_command_failed: gh CLI not found")
      .message
      .contains("Install gh"));
    assert!(describe(
      "gh pr view failed: To get started with GitHub CLI, please run:  gh auth login"
    )
    .message
    .contains("isn't signed in"));
    assert!(describe("unknown git action 'pr-create'")
      .message
      .starts_with("Pull requests need a newer treq"));
  }

  #[test]
  fn unknown_errors_keep_their_detail() {
    assert_eq!(
      describe("treq_command_failed: workspace 7 not found").message,
      "Workspace 7 not found"
    );
    assert_eq!(describe("").message, "");
  }
}
