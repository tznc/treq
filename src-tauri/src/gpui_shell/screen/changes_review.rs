//! The Changes tab's review model, shared with mobile in `core::review`.

pub use crate::core::review::*;

use super::changes_data::Conflict;

/// `formatReviewMarkdown`, with a note's lines read from `conflicts`.
pub fn review_markdown(
  comments: &[LineComment],
  summary: &str,
  notes: &[ConflictNote],
  conflicts: &[Conflict],
  conflicted_files: &[String],
) -> String {
  crate::core::review::review_markdown(
    comments,
    summary,
    notes,
    |note| {
      conflicts
        .iter()
        .find(|c| {
          c.region.conflict_number == note.conflict_number && c.id.starts_with(&note.file_path)
        })
        .map(|c| (c.region.start_line, c.region.end_line))
    },
    conflicted_files,
  )
}
