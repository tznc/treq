//! Whether a waiting message is typed into its agent's terminal now, shown
//! as a badge instead, or kept until the agent is idle.

use super::idle::Screen;
use std::time::Duration;

/// How long the user must have left a terminal alone before treq types in it.
pub const TYPING_QUIET: Duration = Duration::from_secs(3);

/// How long after typing the wake line treq presses Enter: a TUI reads an
/// Enter straight after typed text as part of a paste.
pub const SUBMIT_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
  /// Type the wake line and press Enter.
  Wake,
  /// Leave the prompt alone; the terminal shows a badge and a banner.
  Badge,
  /// Keep the message queued and look again later.
  Defer,
}

/// `since_last_key` is how long ago the user last typed into this terminal,
/// `None` if they never did.
pub fn decide(screen: Screen, since_last_key: Option<Duration>) -> Action {
  match screen {
    Screen::Busy | Screen::Asking => Action::Defer,
    Screen::IdleTyped => Action::Badge,
    Screen::IdleEmpty if since_last_key.is_some_and(|since| since < TYPING_QUIET) => Action::Badge,
    Screen::IdleEmpty => Action::Wake,
  }
}

/// The line typed into an idle agent. It names who wrote and how to read
/// the messages, never their text.
pub fn wake_line(waiting: usize, from: &str, id: &str) -> String {
  if waiting == 1 {
    format!("[treq] 1 message from {from} ({id}). Read it: treq agent msg --inbox")
  } else {
    format!(
      "[treq] {waiting} messages, latest from {from} ({id}). Read them: treq agent msg --inbox"
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn wakes_an_agent_at_an_empty_prompt_the_user_left_alone() {
    assert_eq!(decide(Screen::IdleEmpty, None), Action::Wake);
    assert_eq!(decide(Screen::IdleEmpty, Some(Duration::from_secs(41))), Action::Wake);
  }

  #[test]
  fn badges_while_the_user_has_typed_in_the_last_three_seconds() {
    assert_eq!(decide(Screen::IdleEmpty, Some(Duration::from_millis(400))), Action::Badge);
    assert_eq!(decide(Screen::IdleEmpty, Some(Duration::from_millis(2_999))), Action::Badge);
    assert_eq!(decide(Screen::IdleEmpty, Some(TYPING_QUIET)), Action::Wake);
  }

  #[test]
  fn badges_while_the_prompt_holds_a_draft_however_old() {
    assert_eq!(decide(Screen::IdleTyped, None), Action::Badge);
    assert_eq!(decide(Screen::IdleTyped, Some(Duration::from_secs(600))), Action::Badge);
  }

  #[test]
  fn defers_while_the_agent_works_or_asks() {
    for since in [None, Some(Duration::from_secs(60))] {
      assert_eq!(decide(Screen::Busy, since), Action::Defer);
      assert_eq!(decide(Screen::Asking, since), Action::Defer);
    }
  }

  #[test]
  fn the_wake_line_names_the_sender_and_the_command_but_not_the_text() {
    assert_eq!(
      wake_line(1, "codex-1", "m-7f3a0c11"),
      "[treq] 1 message from codex-1 (m-7f3a0c11). Read it: treq agent msg --inbox"
    );
    assert_eq!(
      wake_line(3, "codex-1", "m-7f3a0c11"),
      "[treq] 3 messages, latest from codex-1 (m-7f3a0c11). Read them: treq agent msg --inbox"
    );
  }
}
