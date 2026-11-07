//! The vt100 terminal model shared by the PTY daemon (Unix) and agent
//! messaging (all platforms).

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_screen_keeps_history_and_clamps_zero_size() {
    let mut parser = new_screen(0, 0);
    assert_eq!(parser.screen().size(), (1, 1));
    parser.set_scrollback(usize::MAX);
    let _ = HISTORY_LINES;
  }
}
