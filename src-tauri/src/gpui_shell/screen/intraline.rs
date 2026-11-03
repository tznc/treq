//! The characters a replaced line changed, as GitHub marks them: each
//! removed line is paired with the added line in the same place of its
//! change block, and the characters that differ between the two are
//! picked out inside the red and green rows.

// Lists of ranges here are mark lists, often of one mark.
#![expect(
  clippy::single_range_in_vec_init,
  reason = "ranges here are mark lists, often of one mark"
)]

use std::{collections::HashMap, ops::Range};

/// Past this share of changed characters the pair reads better as plain
/// red and green rows than with nearly everything marked.
const CHANGE_LIMIT: f64 = 0.6;
/// Middles longer than this (after the common prefix and suffix) are
/// marked whole rather than diffed: the table is quadratic.
const MAX_MIDDLE: usize = 400;
/// Unchanged runs this short between two changes join them, so a renamed
/// word shows as one mark and not a scatter of letters.
const MIN_KEPT: usize = 3;

/// Ranges of one line, in order.
type Marks = Vec<Range<usize>>;

/// Byte ranges to mark in each line's text (the line without its `+`/`-`),
/// by the line's index in the hunk.
pub fn changed_ranges(lines: &[String]) -> HashMap<usize, Marks> {
  let mut out = HashMap::new();
  let mut table = Vec::new();
  let mut i = 0;
  while i < lines.len() {
    let removed_start = i;
    while i < lines.len() && lines[i].starts_with('-') {
      i += 1;
    }
    let added_start = i;
    while i < lines.len() && lines[i].starts_with('+') {
      i += 1;
    }
    let (removed, added) = (added_start - removed_start, i - added_start);
    if removed == 0 || added == 0 {
      i = i.max(removed_start + 1);
      continue;
    }
    for k in 0..removed.min(added) {
      let (r, a) = (removed_start + k, added_start + k);
      if let Some((old, new)) = pair(text(&lines[r]), text(&lines[a]), &mut table) {
        if !old.is_empty() {
          out.insert(r, old);
        }
        if !new.is_empty() {
          out.insert(a, new);
        }
      }
    }
  }
  out
}

/// The changed byte ranges of one removed and one added line, `None` when
/// they share too little to be worth marking.
pub fn line_marks(old: &str, new: &str) -> Option<(Marks, Marks)> {
  pair(old, new, &mut Vec::new())
}

fn text(line: &str) -> &str {
  line.get(1..).unwrap_or("")
}

/// The changed ranges of a removed and an added line, `None` when they
/// share too little to be worth marking.
fn pair(old: &str, new: &str, table: &mut Vec<u16>) -> Option<(Marks, Marks)> {
  if old == new {
    return None;
  }
  let a: Vec<(usize, char)> = old.char_indices().collect();
  let b: Vec<(usize, char)> = new.char_indices().collect();
  let prefix = a.iter().zip(&b).take_while(|(x, y)| x.1 == y.1).count();
  let suffix = a[prefix..]
    .iter()
    .rev()
    .zip(b[prefix..].iter().rev())
    .take_while(|(x, y)| x.1 == y.1)
    .count();
  let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
  let (mut old_marks, mut new_marks) = if am.len() > MAX_MIDDLE || bm.len() > MAX_MIDDLE {
    (vec![0..am.len()], vec![0..bm.len()])
  } else {
    lcs_marks(am, bm, table)
  };
  join_close(&mut old_marks);
  join_close(&mut new_marks);
  let changed = |marks: &[Range<usize>]| marks.iter().map(|r| r.len()).sum::<usize>();
  let longest = a.len().max(b.len()).max(1);
  if changed(&old_marks).max(changed(&new_marks)) as f64 / longest as f64 > CHANGE_LIMIT {
    return None;
  }
  Some((
    to_bytes(old, &a, prefix, &old_marks),
    to_bytes(new, &b, prefix, &new_marks),
  ))
}

/// Char-index runs of each side that the longest common subsequence
/// leaves out. `table` is scratch space, reused across pairs.
fn lcs_marks(a: &[(usize, char)], b: &[(usize, char)], table: &mut Vec<u16>) -> (Marks, Marks) {
  let (n, m) = (a.len(), b.len());
  let w = m + 1;
  table.clear();
  table.resize((n + 1) * w, 0);
  for i in (0..n).rev() {
    let (head, below) = table[i * w..].split_at_mut(w);
    for j in (0..m).rev() {
      head[j] = if a[i].1 == b[j].1 {
        below[j + 1] + 1
      } else {
        below[j].max(head[j + 1])
      };
    }
  }
  let lcs = |i: usize, j: usize| table[i * w + j];
  let (mut old, mut new) = (Vec::new(), Vec::new());
  let push = |marks: &mut Marks, at: usize| match marks.last_mut() {
    Some(r) if r.end == at => r.end += 1,
    _ => marks.push(at..at + 1),
  };
  let (mut i, mut j) = (0, 0);
  while i < n || j < m {
    if i < n && j < m && a[i].1 == b[j].1 {
      i += 1;
      j += 1;
    } else if j < m && (i == n || lcs(i, j + 1) >= lcs(i + 1, j)) {
      push(&mut new, j);
      j += 1;
    } else {
      push(&mut old, i);
      i += 1;
    }
  }
  (old, new)
}

fn join_close(marks: &mut Marks) {
  let mut joined: Marks = Vec::with_capacity(marks.len());
  for r in marks.drain(..) {
    match joined.last_mut() {
      Some(last) if r.start - last.end < MIN_KEPT => last.end = r.end,
      _ => joined.push(r),
    }
  }
  *marks = joined;
}

/// Char-index ranges of the middle (after `prefix` chars) as byte ranges.
fn to_bytes(
  text: &str,
  chars: &[(usize, char)],
  prefix: usize,
  marks: &[Range<usize>],
) -> Vec<Range<usize>> {
  let byte = |c: usize| chars.get(c).map_or(text.len(), |(b, _)| *b);
  marks
    .iter()
    .map(|r| byte(prefix + r.start)..byte(prefix + r.end))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| l.to_string()).collect()
  }

  #[test]
  fn marks_the_characters_a_replaced_line_changed() {
    let marks = changed_ranges(&lines(&[
      " fn a() {",
      "-  let x = 1;",
      "+  let x = 42;",
      " }",
    ]));
    assert_eq!(marks[&1], [10..11]);
    assert_eq!(marks[&2], [10..12]);
    assert_eq!(marks.len(), 2);
  }

  #[test]
  fn pairs_lines_in_order_within_a_block() {
    let marks = changed_ranges(&lines(&[
      "-let alpha = 1;",
      "-let beta = 2;",
      "+let alpha = 3;",
      "+let beta = 4;",
      "+let gamma = 5;",
    ]));
    assert_eq!(marks[&0], [12..13]);
    assert_eq!(marks[&2], [12..13]);
    assert_eq!(marks[&1], [11..12]);
    assert_eq!(marks[&3], [11..12]);
    assert!(
      !marks.contains_key(&4),
      "the extra added line has no partner"
    );
  }

  #[test]
  fn leaves_rewritten_lines_and_lone_changes_plain() {
    let marks = changed_ranges(&lines(&[
      "-completely different",
      "+nothing alike here",
      "+added",
    ]));
    assert!(marks.is_empty());
    assert!(changed_ranges(&lines(&["+only added", " context"])).is_empty());
  }

  #[test]
  fn joins_changes_split_by_a_letter_or_two() {
    let mut marks = vec![0..2, 3..5, 9..10];
    join_close(&mut marks);
    assert_eq!(marks, [0..5, 9..10]);
    let marks = changed_ranges(&lines(&["-call(foo, 1)", "+call(bar, 1)"]));
    assert_eq!(marks[&0], [5..8]);
    assert_eq!(marks[&1], [5..8]);
  }

  #[test]
  fn ranges_land_on_character_boundaries() {
    let marks = changed_ranges(&lines(&["-say(\"héllo wörld\")", "+say(\"héllo world\")"]));
    let old = "say(\"héllo wörld\")";
    for r in &marks[&0] {
      assert!(old.is_char_boundary(r.start) && old.is_char_boundary(r.end));
      assert_eq!(&old[r.clone()], "ö");
    }
  }

  mod props {
    use super::*;
    use crate::test_repo::proptest_config;
    use proptest::prelude::*;

    /// A hunk line: `+`, `-` or ` ` and text from a small alphabet with
    /// multi-byte characters, so pairs share runs and marks cross UTF-8.
    fn hunk_line() -> impl Strategy<Value = String> {
      ("[-+ ]", "[ab é\u{1F600}\r\t]{0,12}").prop_map(|(sign, text)| format!("{sign}{text}"))
    }

    proptest! {
      #![proptest_config(proptest_config(256))]

      /// Marks sit only on changed lines and are ordered, non-empty byte
      /// ranges on the text's char boundaries.
      #[test]
      fn marks_are_ordered_ranges_on_char_boundaries(
        lines in prop::collection::vec(hunk_line(), 0..12),
      ) {
        for (index, marks) in changed_ranges(&lines) {
          prop_assert!(lines[index].starts_with(['+', '-']));
          let body = text(&lines[index]);
          let mut previous_end = 0;
          for mark in &marks {
            prop_assert!(mark.start < mark.end && mark.start >= previous_end, "{marks:?}");
            prop_assert!(body.get(mark.clone()).is_some(), "{mark:?} in {body:?}");
            previous_end = mark.end;
          }
        }
      }

      /// A line paired with itself has nothing to mark.
      #[test]
      fn identical_lines_have_no_marks(line in "[ab é\u{1F600}]{0,16}") {
        prop_assert!(line_marks(&line, &line).is_none());
      }
    }
  }
}
