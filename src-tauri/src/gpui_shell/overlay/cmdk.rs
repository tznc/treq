//! What the `cmdk` package does for React's command dialogs: its fuzzy
//! `commandScore` filter, the order it shows matches in, and how the arrow
//! keys move the selection.

use std::collections::HashMap;

const SCORE_CONTINUE_MATCH: f64 = 1.;
const SCORE_SPACE_WORD_JUMP: f64 = 0.9;
const SCORE_NON_SPACE_WORD_JUMP: f64 = 0.8;
const SCORE_CHARACTER_JUMP: f64 = 0.17;
const SCORE_TRANSPOSITION: f64 = 0.1;
const PENALTY_SKIPPED: f64 = 0.999;
const PENALTY_CASE_MISMATCH: f64 = 0.9999;
const PENALTY_NOT_COMPLETE: f64 = 0.99;

/// `[\\\/_+.#"@\[\(\{&]`
fn is_gap(ch: char) -> bool {
  matches!(
    ch,
    '\\' | '/' | '_' | '+' | '.' | '#' | '"' | '@' | '[' | '(' | '{' | '&'
  )
}

/// `[\s-]`
fn is_space(ch: char) -> bool {
  ch.is_whitespace() || ch == '-'
}

/// `toLowerCase()` with `[\s-]` turned into spaces, one char per char.
fn normalize(text: &[char]) -> Vec<char> {
  text
    .iter()
    .map(|&ch| {
      if is_space(ch) {
        ' '
      } else {
        ch.to_lowercase().next().unwrap_or(ch)
      }
    })
    .collect()
}

struct Scorer<'a> {
  string: &'a [char],
  abbr: &'a [char],
  lower_string: &'a [char],
  lower_abbr: &'a [char],
  memo: HashMap<(usize, usize), f64>,
}

impl Scorer<'_> {
  fn find(&self, ch: char, from: usize) -> Option<usize> {
    (from..self.lower_string.len()).find(|&i| self.lower_string[i] == ch)
  }

  fn score(&mut self, string_index: usize, abbr_index: usize) -> f64 {
    if abbr_index >= self.abbr.len() {
      return if string_index == self.string.len() {
        SCORE_CONTINUE_MATCH
      } else {
        PENALTY_NOT_COMPLETE
      };
    }
    if let Some(&memo) = self.memo.get(&(string_index, abbr_index)) {
      return memo;
    }
    let abbr_char = self.lower_abbr[abbr_index];
    let next_abbr = self.lower_abbr.get(abbr_index + 1).copied();
    let mut high = 0.;
    let mut index = self.find(abbr_char, string_index);
    while let Some(i) = index {
      let mut score = self.score(i + 1, abbr_index + 1);
      if score > high {
        if i == string_index {
          score *= SCORE_CONTINUE_MATCH;
        } else if i > 0 && is_gap(self.string[i - 1]) {
          score *= SCORE_NON_SPACE_WORD_JUMP;
          let skipped = self.skipped(string_index, i, is_gap);
          if skipped > 0 && string_index > 0 {
            score *= PENALTY_SKIPPED.powi(skipped);
          }
        } else if i > 0 && is_space(self.string[i - 1]) {
          score *= SCORE_SPACE_WORD_JUMP;
          let skipped = self.skipped(string_index, i, is_space);
          if skipped > 0 && string_index > 0 {
            score *= PENALTY_SKIPPED.powi(skipped);
          }
        } else {
          score *= SCORE_CHARACTER_JUMP;
          if string_index > 0 {
            score *= PENALTY_SKIPPED.powi((i - string_index) as i32);
          }
        }
        if self.string[i] != self.abbr[abbr_index] {
          score *= PENALTY_CASE_MISMATCH;
        }
      }
      let before = i.checked_sub(1).map(|j| self.lower_string[j]);
      if (score < SCORE_TRANSPOSITION && before == next_abbr)
        || (next_abbr == Some(abbr_char) && before != Some(abbr_char))
      {
        let transposed = self.score(i + 1, abbr_index + 2);
        if transposed * SCORE_TRANSPOSITION > score {
          score = transposed * SCORE_TRANSPOSITION;
        }
      }
      if score > high {
        high = score;
      }
      index = self.find(abbr_char, i + 1);
    }
    self.memo.insert((string_index, abbr_index), high);
    high
  }

  /// How many `matches` chars `string.slice(from, to - 1)` holds.
  fn skipped(&self, from: usize, to: usize, matches: fn(char) -> bool) -> i32 {
    let end = to.saturating_sub(1).max(from);
    self.string[from..end]
      .iter()
      .filter(|&&ch| matches(ch))
      .count() as i32
  }
}

/// cmdk's `commandScore(value, search)`: 0 for no match, up to 1.
pub fn command_score(value: &str, search: &str) -> f64 {
  let string: Vec<char> = value.chars().collect();
  let abbr: Vec<char> = search.chars().collect();
  let lower_string = normalize(&string);
  let lower_abbr = normalize(&abbr);
  Scorer {
    string: &string,
    abbr: &abbr,
    lower_string: &lower_string,
    lower_abbr: &lower_abbr,
    memo: HashMap::new(),
  }
  .score(0, 0)
}

/// One `Command.Item`: what it filters on, its `Command.Group` and whether
/// it is `disabled`.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
  pub value: String,
  pub group: Option<usize>,
  pub disabled: bool,
}

/// The items cmdk shows for `search`, as indexes into `entries`, in the
/// order it shows them: everything as given while the search is empty, else
/// the matches best first, in groups ordered by their best match.
pub fn visible(entries: &[Entry], search: &str) -> Vec<usize> {
  if search.is_empty() {
    return (0..entries.len()).collect();
  }
  let scores: Vec<f64> = entries
    .iter()
    .map(|e| command_score(e.value.trim(), search))
    .collect();
  let mut matched: Vec<usize> = (0..entries.len()).filter(|&i| scores[i] > 0.).collect();
  matched.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
  // Groups in first-seen order, then by their best item (a stable sort).
  let mut groups: Vec<Option<usize>> = Vec::new();
  for e in entries {
    if !groups.contains(&e.group) {
      groups.push(e.group);
    }
  }
  let best = |group: Option<usize>| {
    matched
      .iter()
      .filter(|&&i| entries[i].group == group)
      .map(|&i| scores[i])
      .fold(0., f64::max)
  };
  let ungrouped: Vec<usize> = matched
    .iter()
    .copied()
    .filter(|&i| entries[i].group.is_none())
    .collect();
  let mut grouped: Vec<Option<usize>> = groups.into_iter().filter(Option::is_some).collect();
  grouped.sort_by(|&a, &b| best(b).total_cmp(&best(a)));
  let mut order = ungrouped;
  for group in grouped {
    order.extend(
      matched
        .iter()
        .copied()
        .filter(|&i| entries[i].group == group),
    );
  }
  order
}

/// How a key moves the selection (cmdk's root `onKeyDown`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
  /// Arrow keys, and Ctrl+N/J/P/K.
  Step(isize),
  /// Home, or Cmd+Up.
  First,
  /// End, or Cmd+Down.
  Last,
  /// Alt+arrow: the first item of the next or previous group.
  Group(isize),
}

/// The entry selected after `step` from `selected`, among `order` (as
/// [`visible`] returns it) without the disabled ones. Steps stop at the ends;
/// cmdk's `loop` is off.
pub fn moved(
  entries: &[Entry],
  order: &[usize],
  selected: Option<usize>,
  step: Move,
) -> Option<usize> {
  let enabled: Vec<usize> = order
    .iter()
    .copied()
    .filter(|&i| !entries[i].disabled)
    .collect();
  let at = selected.and_then(|s| enabled.iter().position(|&i| i == s));
  let by = |delta: isize| -> Option<usize> {
    let from = at.map_or(-1, |a| a as isize);
    let to = from + delta;
    (to >= 0)
      .then(|| enabled.get(to as usize).copied())
      .flatten()
      .or(selected)
  };
  match step {
    Move::Step(delta) => by(delta),
    Move::First => enabled.first().copied().or(selected),
    Move::Last => enabled.last().copied().or(selected),
    Move::Group(delta) => {
      let current = selected.map(|s| entries[s].group);
      let Some(Some(group)) = current else {
        return by(delta);
      };
      // Groups in the order shown.
      let mut shown: Vec<usize> = Vec::new();
      for &i in order {
        if let Some(g) = entries[i].group {
          if !shown.contains(&g) {
            shown.push(g);
          }
        }
      }
      let Some(pos) = shown.iter().position(|&g| g == group) else {
        return by(delta);
      };
      let mut next = pos as isize + delta;
      while next >= 0 && (next as usize) < shown.len() {
        let g = shown[next as usize];
        if let Some(&first) = enabled.iter().find(|&&i| entries[i].group == Some(g)) {
          return Some(first);
        }
        next += delta;
      }
      by(delta)
    }
  }
}

/// The first enabled entry shown, which cmdk selects when the search changes.
pub fn first(entries: &[Entry], order: &[usize]) -> Option<usize> {
  order.iter().copied().find(|&i| !entries[i].disabled)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(value: &str) -> Entry {
    Entry {
      value: value.into(),
      group: None,
      disabled: false,
    }
  }

  fn grouped(value: &str, group: usize) -> Entry {
    Entry {
      group: Some(group),
      ..entry(value)
    }
  }

  #[test]
  fn scores_like_cmdk() {
    // node_modules/cmdk/dist/command-score.js, run under node.
    let cases = [
      ("Go to Settings", "go", 0.989901),
      ("Go to Settings", "set", 0.8909109000000001),
      ("Go to Settings", "gts", 0.801739628019),
      ("feat/alpha", "ALPHA", 0.7996000799920006),
      ("new-shell", "new shell", 0.9999),
      ("Search Files", "sf", 0.8908218089100001),
      ("Search Files", "sl", 0.1669416071566729),
      ("Start a new agent terminal", "agent", 0.891),
      ("Open Repository in New Window", "orw", 0.8000569368075398),
      ("Run check: CI", "ci", 0.899820009),
    ];
    for (value, search, expected) in cases {
      let score = command_score(value, search);
      assert!(
        (score - expected).abs() < 1e-12,
        "{value:?} {search:?}: {score}"
      );
    }
    assert_eq!(command_score("Go to Settings", "zz"), 0.);
    assert_eq!(command_score("Settings", "Settings"), 1.);
  }

  #[test]
  fn shows_everything_in_order_without_a_search() {
    let entries = [entry("b"), entry("a")];
    assert_eq!(visible(&entries, ""), vec![0, 1]);
  }

  #[test]
  fn sorts_matches_best_first() {
    let entries = [
      entry("Go to Home"),
      entry("Go to Settings"),
      entry("Search Files"),
    ];
    assert_eq!(visible(&entries, "se"), vec![2, 1]);
  }

  #[test]
  fn orders_groups_by_their_best_match() {
    let entries = [
      grouped("main", 0),
      grouped("feat/search", 1),
      grouped("origin/search@origin", 2),
    ];
    assert_eq!(visible(&entries, "search"), vec![1, 2]);
    assert_eq!(visible(&entries, "ma"), vec![0]);
  }

  #[test]
  fn steps_without_wrapping_and_skips_disabled() {
    let mut entries = vec![entry("a"), entry("b"), entry("c")];
    let order = [0, 1, 2];
    assert_eq!(moved(&entries, &order, Some(0), Move::Step(1)), Some(1));
    assert_eq!(moved(&entries, &order, Some(2), Move::Step(1)), Some(2));
    assert_eq!(moved(&entries, &order, Some(0), Move::Step(-1)), Some(0));
    assert_eq!(moved(&entries, &order, None, Move::Step(1)), Some(0));
    assert_eq!(moved(&entries, &order, Some(1), Move::Last), Some(2));
    assert_eq!(moved(&entries, &order, Some(1), Move::First), Some(0));
    entries[1].disabled = true;
    assert_eq!(moved(&entries, &order, Some(0), Move::Step(1)), Some(2));
  }

  #[test]
  fn jumps_between_groups() {
    let entries = [grouped("a", 0), grouped("b", 1), grouped("c", 1)];
    let order = [0, 1, 2];
    assert_eq!(moved(&entries, &order, Some(0), Move::Group(1)), Some(1));
    assert_eq!(moved(&entries, &order, Some(2), Move::Group(-1)), Some(0));
    assert_eq!(moved(&entries, &order, Some(1), Move::Group(1)), Some(2));
  }
}
