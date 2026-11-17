//! The vt100 terminal model shared by the PTY daemon (Unix) and agent
//! messaging (all platforms).

use std::num::NonZeroU16;

/// Lines of scrollback each terminal model keeps.
pub const HISTORY_LINES: usize = 250;

/// A terminal model sized `cols` x `rows` that keeps [`HISTORY_LINES`].
pub fn new_screen(cols: u16, rows: u16) -> vt100::Parser {
  vt100::Parser::new(nonzero(rows), nonzero(cols), HISTORY_LINES)
}

pub(crate) fn nonzero(value: u16) -> NonZeroU16 {
  NonZeroU16::new(value).unwrap_or(NonZeroU16::MIN)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_screen_keeps_history_and_clamps_zero_size() {
    let parser = new_screen(0, 0);
    assert_eq!(parser.screen().size(), (NonZeroU16::MIN, NonZeroU16::MIN));
    assert_eq!(HISTORY_LINES, 250);
  }
}
