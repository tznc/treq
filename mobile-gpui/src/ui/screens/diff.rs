//! Diff lines: their colours, and picking them for a comment.

use gpui_kit::{component::ActiveTheme, *};
use treq_lib::{
  core::review::{selection, LineRef},
  jj::JjDiffHunk,
};

use crate::ui::widgets;

/// How a diff line is coloured.
#[derive(Debug, PartialEq)]
pub enum LineKind {
  Added,
  Removed,
  Context,
}

pub fn line_kind(line: &str) -> LineKind {
  if line.starts_with('+') && !line.starts_with("+++") {
    LineKind::Added
  } else if line.starts_with('-') && !line.starts_with("---") {
    LineKind::Removed
  } else {
    LineKind::Context
  }
}

/// Diff lines picked for a comment: tap one, then tap another to take the
/// lines between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
  pub anchor: LineRef,
  pub lines: Vec<LineRef>,
}

/// The lines picked after tapping `at`: a first tap picks a line, a second
/// extends to `at` (or, on the same line, drops it), and a tap after a
/// range starts over.
pub fn tap(picked: Option<&Picked>, hunks: &[JjDiffHunk], at: LineRef) -> Option<Picked> {
  match picked {
    Some(p) if p.lines.len() == 1 && p.anchor == at => None,
    Some(p) if p.lines.len() == 1 => Some(Picked {
      anchor: p.anchor,
      lines: selection(hunks, p.anchor, at),
    }),
    _ => Some(Picked {
      anchor: at,
      lines: vec![at],
    }),
  }
}

pub fn diff_line(line: &str, cx: &App) -> Div {
  let theme = cx.theme();
  let (bg, fg) = match line_kind(line) {
    LineKind::Added => (theme.success.opacity(0.15), theme.success),
    LineKind::Removed => (theme.danger.opacity(0.15), theme.danger),
    LineKind::Context => (gpui_kit::transparent_black(), theme.foreground),
  };
  widgets::mono(
    if line.is_empty() {
      " ".to_string()
    } else {
      line.to_string()
    },
    cx,
  )
  .px_2()
  .bg(bg)
  .text_color(fg)
}

#[cfg(test)]
mod tests {
  use super::{line_kind, tap, LineKind, Picked};
  use treq_lib::jj::JjDiffHunk;

  fn hunk(lines: &[&str]) -> JjDiffHunk {
    serde_json::from_value(serde_json::json!({
      "id": "h", "header": "@@ -1,3 +1,3 @@", "lines": lines, "patch": ""
    }))
    .unwrap()
  }

  #[test]
  fn taps_pick_a_line_then_a_range_then_start_over() {
    let hunks = [hunk(&[" a", "+b", "+c", " d"])];
    let one = tap(None, &hunks, (0, 1)).unwrap();
    assert_eq!(one.lines, [(0, 1)]);
    let range = tap(Some(&one), &hunks, (0, 3)).unwrap();
    assert_eq!(range.lines, [(0, 1), (0, 2), (0, 3)]);
    let backwards = tap(Some(&one), &hunks, (0, 0)).unwrap();
    assert_eq!(backwards.lines, [(0, 0), (0, 1)]);
    assert_eq!(tap(Some(&one), &hunks, (0, 1)), None);
    assert_eq!(
      tap(Some(&range), &hunks, (0, 2)),
      Some(Picked {
        anchor: (0, 2),
        lines: vec![(0, 2)]
      })
    );
  }

  #[test]
  fn colours_added_and_removed_lines_but_not_file_headers() {
    assert_eq!(line_kind("+added"), LineKind::Added);
    assert_eq!(line_kind("-removed"), LineKind::Removed);
    assert_eq!(line_kind("+++ b/file"), LineKind::Context);
    assert_eq!(line_kind("--- a/file"), LineKind::Context);
    assert_eq!(line_kind(" context"), LineKind::Context);
  }
}
