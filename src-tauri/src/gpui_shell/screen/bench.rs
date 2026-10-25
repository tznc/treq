//! The Changes tab's per-row and per-frame helpers, and what a comment card
//! runs on its text, for `benches/diff_view.rs`.
//! Not an API: the screens are private, so criterion reaches them here.

pub use super::{
  changes_review::{comments_under, is_outdated, line_at, line_has_comment, LineComment, Side},
  changes_threads::place_threads,
  code_data::{emojify, looks_like_markdown},
  commits_data::line_numbers,
  intraline::changed_ranges,
};

/// How many token spans `line` highlights to, through the line cache.
pub fn highlight(language: &'static str, line: &str) -> usize {
  super::highlight::spans(language, line).len()
}
