//! Reads an agent's terminal screen to tell whether treq may type into it.
//!
//! Each harness draws a prompt line (`>`, `›`, `❯` or `→`, often inside a
//! box) when it waits for input, and a hint like "esc to interrupt" while it
//! works. Text in the prompt is the user's draft unless it is drawn faint
//! (dim, italic or grey), which is how the harnesses draw their placeholder.
//! Anything treq cannot read counts as busy, so a doubt never ends with treq
//! typing over someone.

/// The harnesses treq launches (`core::agent_launch::AGENTS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
  Claude,
  Codex,
  Cursor,
  Copilot,
}

impl Harness {
  pub const ALL: [Harness; 4] = [
    Harness::Claude,
    Harness::Codex,
    Harness::Cursor,
    Harness::Copilot,
  ];

  pub fn id(self) -> &'static str {
    match self {
      Harness::Claude => "claude",
      Harness::Codex => "codex",
      Harness::Cursor => "cursor",
      Harness::Copilot => "copilot",
    }
  }

  pub fn parse(id: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|harness| harness.id() == id)
  }
}

/// What an agent's screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
  /// Working, starting up, or a screen treq cannot read.
  Busy,
  /// Waiting on an approval or a choice: never typed into.
  Asking,
  /// Waiting at an empty prompt.
  IdleEmpty,
  /// Waiting at a prompt that holds the user's draft.
  IdleTyped,
}

/// Hints the harnesses show while they work.
const BUSY_HINTS: [&str; 4] = [
  "esc to interrupt",
  "esc to cancel",
  "ctrl+c to stop",
  "ctrl+c to interrupt",
];

/// What approval and choice dialogs say.
const ASKING_HINTS: [&str; 6] = [
  "do you want to",
  "allow command",
  "(y/n)",
  "[y/n]",
  "yes (y)",
  "1. yes",
];

/// The characters a prompt line starts with.
const PROMPT_GLYPHS: [char; 4] = ['>', '›', '❯', '→'];

/// Box sides around a prompt.
const BORDERS: [char; 4] = ['│', '┃', '║', '|'];

/// How many of the screen's last non-blank lines are read: the prompt and
/// the hints sit at the bottom.
const WINDOW: usize = 14;

/// One screen row: as drawn, and with faint cells blanked out.
struct Line {
  text: String,
  solid: String,
}

/// The harnesses share their prompt shapes and hints, so one reading
/// serves all four; `harness` is kept for the ones that will differ.
pub fn classify(harness: Harness, screen: &vt100::Screen) -> Screen {
  let _ = harness;
  let lines = rows(screen);
  let end = lines
    .iter()
    .rposition(|line| !line.text.trim().is_empty())
    .map_or(0, |last| last + 1);
  let window: Vec<&Line> = lines[..end]
    .iter()
    .rev()
    .filter(|line| !line.text.trim().is_empty())
    .take(WINDOW)
    .collect::<Vec<_>>()
    .into_iter()
    .rev()
    .collect();
  let lower = window
    .iter()
    .map(|line| line.text.to_lowercase())
    .collect::<Vec<_>>()
    .join("\n");
  if BUSY_HINTS.iter().any(|hint| lower.contains(hint)) {
    return Screen::Busy;
  }
  if ASKING_HINTS.iter().any(|hint| lower.contains(hint)) {
    return Screen::Asking;
  }
  let visible = &lines[..end];
  let Some(prompt) = visible
    .iter()
    .rposition(|line| prompt_text(&line.text).is_some())
  else {
    return Screen::Busy;
  };
  if end - prompt > WINDOW {
    return Screen::Busy;
  }
  if prompt_text(&visible[prompt].solid).is_some_and(|draft| !draft.is_empty()) {
    return Screen::IdleTyped;
  }
  for line in &visible[prompt + 1..] {
    let Some(inside) = inside_border(&line.solid) else {
      break;
    };
    if !inside.is_empty() {
      return Screen::IdleTyped;
    }
  }
  Screen::IdleEmpty
}

fn rows(screen: &vt100::Screen) -> Vec<Line> {
  let (rows, cols) = screen.size();
  let (rows, cols) = (u16::from(rows), u16::from(cols));
  (0..rows)
    .map(|row| {
      let mut line = Line {
        text: String::new(),
        solid: String::new(),
      };
      for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
          continue;
        };
        if cell.is_wide_continuation() {
          continue;
        }
        let contents = if cell.has_contents() {
          cell.contents()
        } else {
          " "
        };
        line.text.push_str(contents);
        if is_faint(cell) {
          line.solid.extend(contents.chars().map(|_| ' '));
        } else {
          line.solid.push_str(contents);
        }
      }
      line
    })
    .collect()
}

/// How placeholders are drawn: dim, italic, or a grey foreground.
fn is_faint(cell: &vt100::Cell) -> bool {
  if cell.dim() || cell.italic() {
    return true;
  }
  match cell.fgcolor() {
    vt100::Color::Idx(idx) => idx == 8 || (232..=250).contains(&idx),
    vt100::Color::Rgb(r, g, b) => {
      let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
      hi - lo < 16 && hi <= 180
    }
    vt100::Color::Default => false,
  }
}

/// A row's text after its leading box side and prompt glyph, when the row is
/// a prompt line.
fn prompt_text(row: &str) -> Option<String> {
  let trimmed = row.trim_start().trim_start_matches(BORDERS).trim_start();
  let mut chars = trimmed.chars();
  let glyph = chars.next()?;
  if !PROMPT_GLYPHS.contains(&glyph) {
    return None;
  }
  let rest = chars.as_str();
  if !(rest.is_empty() || rest.starts_with(' ')) {
    return None;
  }
  Some(strip_right_border(rest))
}

/// A row's text inside a box's sides, when the row is one of its middle rows.
fn inside_border(row: &str) -> Option<String> {
  let trimmed = row.trim_start();
  let rest = trimmed.strip_prefix(BORDERS)?;
  Some(strip_right_border(rest))
}

fn strip_right_border(text: &str) -> String {
  text.trim_end().trim_end_matches(BORDERS).trim().to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  const DIM: &str = "\x1b[2m";
  const GREY: &str = "\x1b[90m";
  const RESET: &str = "\x1b[0m";

  fn screen(text: &str) -> vt100::Parser {
    let mut parser = crate::core::pty_daemon::new_screen(100, 24);
    parser.process(text.replace('\n', "\r\n").as_bytes());
    parser
  }

  fn check(harness: Harness, text: &str) -> Screen {
    classify(harness, screen(text).screen())
  }

  fn claude(prompt: &str, footer: &str) -> String {
    format!(
      "● Rebased onto main. 3 conflicts resolved.\n\n\
       ╭──────────────────────────────────────────╮\n\
       │ > {prompt}\n\
       ╰──────────────────────────────────────────╯\n  {footer}\n"
    )
  }

  #[test]
  fn claude_at_an_empty_prompt_is_idle() {
    assert_eq!(
      check(Harness::Claude, &claude("", "? for shortcuts")),
      Screen::IdleEmpty
    );
  }

  #[test]
  fn claude_placeholder_text_is_not_a_draft() {
    let prompt = format!("{DIM}Try \"refactor <filepath>\"{RESET}");
    assert_eq!(
      check(Harness::Claude, &claude(&prompt, "? for shortcuts")),
      Screen::IdleEmpty
    );
  }

  #[test]
  fn claude_with_a_draft_is_typed() {
    assert_eq!(
      check(
        Harness::Claude,
        &claude("now also squash the fixups", "? for shortcuts")
      ),
      Screen::IdleTyped
    );
  }

  #[test]
  fn claude_working_is_busy() {
    let text = format!(
      "✻ Rebasing… (12s · esc to interrupt)\n{}",
      claude("", "? for shortcuts")
    );
    assert_eq!(check(Harness::Claude, &text), Screen::Busy);
  }

  #[test]
  fn claude_permission_dialog_is_asking() {
    let text = "● Bash(git push --force)\n\n\
                ╭──────────────────────────────────────────╮\n\
                │ Do you want to proceed?                   \n\
                │ ❯ 1. Yes                                  \n\
                │   2. No, and tell Claude what to do        \n\
                ╰──────────────────────────────────────────╯\n";
    assert_eq!(check(Harness::Claude, text), Screen::Asking);
  }

  #[test]
  fn claude_draft_on_a_later_line_of_the_box_is_typed() {
    let text = "╭──────────────────────────────────────────╮\n\
                │ >                                         \n\
                │   second line of my draft                 \n\
                ╰──────────────────────────────────────────╯\n";
    assert_eq!(check(Harness::Claude, text), Screen::IdleTyped);
  }

  #[test]
  fn codex_at_an_empty_prompt_is_idle() {
    let text = format!(
      "• Updated 3 files.\n\n› {GREY}Ask Codex to do anything{RESET}\n\n  ⏎ send   ⌃J newline   ⌃C quit\n"
    );
    assert_eq!(check(Harness::Codex, &text), Screen::IdleEmpty);
  }

  #[test]
  fn codex_with_a_draft_is_typed() {
    let text = "• Updated 3 files.\n\n› run the benches again\n\n  ⏎ send   ⌃J newline   ⌃C quit\n";
    assert_eq!(check(Harness::Codex, text), Screen::IdleTyped);
  }

  #[test]
  fn codex_working_is_busy() {
    let text = "• Working (8s • Esc to interrupt)\n\n› \n\n  ⏎ send   ⌃J newline   ⌃C quit\n";
    assert_eq!(check(Harness::Codex, text), Screen::Busy);
  }

  #[test]
  fn codex_approval_is_asking() {
    let text = "Allow command?\n  $ rm -rf target\n› Yes (y)   No (n)\n";
    assert_eq!(check(Harness::Codex, text), Screen::Asking);
  }

  #[test]
  fn cursor_at_an_empty_prompt_is_idle() {
    let text = format!(
      " ┌────────────────────────────────────────┐\n │ → {GREY}Plan, search, build anything{RESET}\n └────────────────────────────────────────┘\n   Auto · 12% context\n"
    );
    assert_eq!(check(Harness::Cursor, &text), Screen::IdleEmpty);
  }

  #[test]
  fn cursor_with_a_draft_is_typed() {
    let text = " ┌────────────────────────────────────────┐\n │ → add a cache test\n └────────────────────────────────────────┘\n";
    assert_eq!(check(Harness::Cursor, text), Screen::IdleTyped);
  }

  #[test]
  fn cursor_generating_is_busy() {
    let text = "  ⬢ Generating…  ctrl+c to stop\n ┌──────┐\n │ → \n └──────┘\n";
    assert_eq!(check(Harness::Cursor, text), Screen::Busy);
  }

  #[test]
  fn copilot_at_an_empty_prompt_is_idle() {
    let text = format!(
      "● Bumped 14 dependencies.\n\n╭────────────────────────────────╮\n│ > {DIM}Enter @ to mention files{RESET}\n╰────────────────────────────────╯\n  Ctrl+c Exit · Ctrl+r Expand\n"
    );
    assert_eq!(check(Harness::Copilot, &text), Screen::IdleEmpty);
  }

  #[test]
  fn copilot_with_a_draft_is_typed() {
    let text =
      "╭────────────────────────────────╮\n│ > pin serde too\n╰────────────────────────────────╯\n";
    assert_eq!(check(Harness::Copilot, text), Screen::IdleTyped);
  }

  #[test]
  fn copilot_thinking_is_busy() {
    let text = "∙ Thinking (Esc to cancel)\n╭──────╮\n│ > \n╰──────╯\n";
    assert_eq!(check(Harness::Copilot, text), Screen::Busy);
  }

  #[test]
  fn copilot_confirmation_is_asking() {
    let text = "Do you want to run this command?\n❯ 1. Yes\n  2. No, and tell Copilot what to do differently (Esc)\n";
    assert_eq!(check(Harness::Copilot, text), Screen::Asking);
  }

  #[test]
  fn a_screen_with_no_prompt_is_busy_for_every_harness() {
    for harness in Harness::ALL {
      assert_eq!(check(harness, "Loading…\n"), Screen::Busy, "{harness:?}");
      assert_eq!(check(harness, ""), Screen::Busy, "{harness:?}");
    }
  }

  #[test]
  fn a_shell_prompt_after_the_agent_exits_is_busy() {
    for harness in Harness::ALL {
      assert_eq!(
        check(harness, "user@host:~/app$ \n"),
        Screen::Busy,
        "{harness:?}"
      );
    }
  }

  #[test]
  fn harness_ids_round_trip() {
    for harness in Harness::ALL {
      assert_eq!(Harness::parse(harness.id()), Some(harness));
    }
    assert_eq!(Harness::parse("gemini"), None);
  }
}
