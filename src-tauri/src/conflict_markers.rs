use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStyle {
  JjDiff,
  JjSnapshot,
  JjGitDiff3,
  GitMerge,
  GitDiff3,
  #[default]
  Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictLineKind {
  Marker,
  Content,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictLineRole {
  Start,
  Base,
  Separator,
  End,
  Left,
  Right,
  Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ConflictLineView {
  pub raw: String,
  pub kind: ConflictLineKind,
  pub role: ConflictLineRole,
  pub file_line: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ConflictComparisonView {
  pub left_line_indexes: Vec<usize>,
  pub base_line_indexes: Vec<usize>,
  pub right_line_indexes: Vec<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ConflictRegionView {
  pub id: String,
  pub file_path: String,
  pub conflict_number: usize,
  pub total_conflicts: usize,
  pub start_line: usize,
  pub end_line: usize,
  pub marker_style: ConflictStyle,
  pub content: String,
  pub lines: Vec<ConflictLineView>,
  pub line_map: Vec<usize>,
  pub comparison: ConflictComparisonView,
}

pub type ConflictRegion = ConflictRegionView;

#[expect(
  clippy::string_slice,
  reason = "offsets come from ASCII find() and is_ascii_digit() scans of a marker line"
)]
fn extract_conflict_info(line: &str) -> Option<(usize, usize)> {
  let conflict_pos = line.find("Conflict ")?;
  let rest = &line[conflict_pos + 9..];

  let num_end = rest.find(|c: char| !c.is_ascii_digit())?;
  if num_end == 0 {
    return None;
  }
  let num: usize = rest[..num_end].parse().ok()?;

  let after_num = rest[num_end..].trim_start();
  if !after_num.starts_with("of ") {
    return None;
  }
  let total_rest = &after_num[3..];
  let total_end = total_rest
    .find(|c: char| !c.is_ascii_digit())
    .unwrap_or(total_rest.len());
  if total_end == 0 {
    return None;
  }
  let total: usize = total_rest[..total_end].parse().ok()?;

  Some((num, total))
}

fn classify_style(content: &str, start_marker: &str) -> ConflictStyle {
  let has_percent = content
    .lines()
    .any(|l| l.trim_start().starts_with("%%%%%%%"));
  let has_pipe = content
    .lines()
    .any(|l| l.trim_start().starts_with("|||||||"));
  let has_equal = content
    .lines()
    .any(|l| l.trim_start().starts_with("======="));
  let has_plus = content
    .lines()
    .any(|l| l.trim_start().starts_with("+++++++"));
  let has_minus = content
    .lines()
    .any(|l| l.trim_start().starts_with("-------"));
  let has_side = start_marker.contains("Side #");

  if has_percent {
    ConflictStyle::JjDiff
  } else if has_plus && has_minus {
    ConflictStyle::JjSnapshot
  } else if has_side && has_pipe && has_equal {
    ConflictStyle::JjGitDiff3
  } else if has_pipe && has_equal {
    ConflictStyle::GitDiff3
  } else if has_equal {
    ConflictStyle::GitMerge
  } else {
    ConflictStyle::Unknown
  }
}

fn marker_role(line: &str) -> Option<ConflictLineRole> {
  let trimmed = line.trim_start();
  if trimmed.starts_with("<<<<<<<") {
    Some(ConflictLineRole::Start)
  } else if trimmed.starts_with("|||||||") || trimmed.starts_with("-------") {
    Some(ConflictLineRole::Base)
  } else if trimmed.starts_with("=======")
    || trimmed.starts_with("%%%%%%%")
    || trimmed.starts_with("+++++++")
  {
    Some(ConflictLineRole::Separator)
  } else if trimmed.starts_with(">>>>>>>") {
    Some(ConflictLineRole::End)
  } else {
    None
  }
}

pub fn parse_conflict_markers(content: &str, file_path: &str) -> Vec<ConflictRegionView> {
  let lines: Vec<&str> = content.split('\n').collect();
  let mut regions = Vec::new();
  let mut i = 0;

  while i < lines.len() {
    let line = lines[i];
    if !line.trim_start().starts_with("<<<<<<<") {
      i += 1;
      continue;
    }

    let start_line = i + 1;
    let (conflict_number, total_conflicts) =
      extract_conflict_info(line).unwrap_or((regions.len() + 1, 0));

    // With no end marker the block runs to the end of the file.
    let mut end_line = lines.len();
    let mut block_lines: Vec<&str> = vec![line];
    let mut j = i + 1;
    while j < lines.len() {
      block_lines.push(lines[j]);
      if lines[j].trim_start().starts_with(">>>>>>>") {
        end_line = j + 1;
        i = j;
        break;
      }
      j += 1;
    }
    if j == lines.len() {
      i = j - 1;
    }

    let block_content = block_lines.join("\n");
    let style = classify_style(&block_content, line);

    let mut line_views = Vec::new();
    let mut left_line_indexes = Vec::new();
    let mut base_line_indexes = Vec::new();
    let mut right_line_indexes = Vec::new();
    let mut line_map = Vec::new();

    let mut section = ConflictLineRole::Left;
    for (idx, raw) in block_lines.iter().enumerate() {
      let file_line = start_line + idx;
      line_map.push(file_line);
      let role = if let Some(marker) = marker_role(raw) {
        match marker {
          ConflictLineRole::Start => section = ConflictLineRole::Left,
          ConflictLineRole::Base => section = ConflictLineRole::Base,
          ConflictLineRole::Separator => {
            section = ConflictLineRole::Right;
          }
          ConflictLineRole::End
          | ConflictLineRole::Left
          | ConflictLineRole::Right
          | ConflictLineRole::Unknown => {}
        }
        marker
      } else {
        section
      };
      let kind = if marker_role(raw).is_some() {
        ConflictLineKind::Marker
      } else {
        ConflictLineKind::Content
      };
      if kind == ConflictLineKind::Content {
        match section {
          ConflictLineRole::Left => left_line_indexes.push(idx),
          ConflictLineRole::Base => base_line_indexes.push(idx),
          ConflictLineRole::Right => right_line_indexes.push(idx),
          _ => {}
        }
      }
      line_views.push(ConflictLineView {
        raw: (*raw).to_string(),
        kind,
        role,
        file_line,
      });
    }

    regions.push(ConflictRegionView {
      id: format!("{}-conflict-{}", file_path, conflict_number),
      file_path: file_path.to_string(),
      conflict_number,
      total_conflicts,
      start_line,
      end_line,
      marker_style: style,
      content: block_content,
      lines: line_views,
      line_map,
      comparison: ConflictComparisonView {
        left_line_indexes,
        base_line_indexes,
        right_line_indexes,
      },
    });

    i += 1;
  }

  // Git-style markers carry no "Conflict N of M" suffix to parse a total from.
  let parsed_total = regions.len();
  for region in &mut regions {
    if region.total_conflicts == 0 {
      region.total_conflicts = parsed_total;
    }
  }

  regions
}

#[cfg(test)]
mod tests {
  use super::*;

  const GIT_DIFF3_CONFLICT: &str = "line before\n\
<<<<<<< Side #1 (Conflict 1 of 1)\nours\n\
||||||| Base\nbase\n\
=======\ntheirs\n\
>>>>>>> Side #2 (Conflict 1 of 1)\n\
line after\n";

  /// A start marker with no end marker used to report `end_line ==
  /// start_line` while holding every line to the end of the file, and a
  /// second start marker inside it became a second, overlapping region.
  #[test]
  fn unterminated_conflict_ends_at_the_last_line_without_overlap() {
    let regions = parse_conflict_markers("<<<<<<< a\nours\n<<<<<<< b\nmore", "f.txt");
    assert_eq!(regions.len(), 1);
    assert_eq!((regions[0].start_line, regions[0].end_line), (1, 4));
    assert_eq!(regions[0].lines.len(), 4);
  }

  /// Serializing as camelCase leaves the frontend's snake_case `start_line`/`end_line` undefined, which silently hides the inline conflict card.
  #[test]
  fn conflict_region_serializes_snake_case_field_names() {
    let regions = parse_conflict_markers(GIT_DIFF3_CONFLICT, "README.md");
    assert_eq!(regions.len(), 1);

    let value = serde_json::to_value(&regions[0]).expect("region serializes");
    let object = value.as_object().expect("region is a JSON object");

    for key in [
      "id",
      "file_path",
      "conflict_number",
      "total_conflicts",
      "start_line",
      "end_line",
      "marker_style",
      "content",
      "lines",
      "line_map",
      "comparison",
    ] {
      assert!(object.contains_key(key), "missing region key `{key}`");
    }

    let line = object["lines"][0]
      .as_object()
      .expect("line is a JSON object");
    for key in ["raw", "kind", "role", "file_line"] {
      assert!(line.contains_key(key), "missing line key `{key}`");
    }

    let comparison = object["comparison"]
      .as_object()
      .expect("comparison is a JSON object");
    for key in [
      "left_line_indexes",
      "base_line_indexes",
      "right_line_indexes",
    ] {
      assert!(
        comparison.contains_key(key),
        "missing comparison key `{key}`"
      );
    }
  }

  #[test]
  fn conflict_region_line_numbers_are_one_based_file_lines() {
    let regions = parse_conflict_markers(GIT_DIFF3_CONFLICT, "README.md");
    let region = &regions[0];

    // `<<<<<<<` is the 2nd line of the file, `>>>>>>>` the 8th.
    assert_eq!(region.start_line, 2);
    assert_eq!(region.end_line, 8);
    assert_eq!(region.conflict_number, 1);
    assert_eq!(region.total_conflicts, 1);
    assert_eq!(region.marker_style, ConflictStyle::JjGitDiff3);
  }

  #[test]
  fn total_conflicts_falls_back_to_the_number_of_parsed_regions() {
    let content = "<<<<<<< main\nours\n=======\ntheirs\n>>>>>>> feature\n\
                       middle\n\
                       <<<<<<< main\nours2\n=======\ntheirs2\n>>>>>>> feature\n";
    let regions = parse_conflict_markers(content, "README.md");

    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].conflict_number, 1);
    assert_eq!(regions[1].conflict_number, 2);
    assert!(regions.iter().all(|r| r.total_conflicts == 2));
  }

  #[test]
  fn conflict_region_assigns_side_roles_to_content_lines() {
    let regions = parse_conflict_markers(GIT_DIFF3_CONFLICT, "README.md");
    let region = &regions[0];

    let role_of = |raw: &str| {
      region
        .lines
        .iter()
        .find(|l| l.raw == raw)
        .unwrap_or_else(|| panic!("no line `{raw}`"))
        .role
    };
    assert_eq!(role_of("ours"), ConflictLineRole::Left);
    assert_eq!(role_of("base"), ConflictLineRole::Base);
    assert_eq!(role_of("theirs"), ConflictLineRole::Right);

    assert_eq!(region.comparison.left_line_indexes, vec![1]);
    assert_eq!(region.comparison.base_line_indexes, vec![3]);
    assert_eq!(region.comparison.right_line_indexes, vec![5]);
  }
}

#[cfg(test)]
mod proptests {
  use super::*;
  use crate::test_repo::proptest_config;
  use proptest::prelude::*;

  /// Lines that may be conflict markers, ordinary text, or text that looks like a marker.
  fn line() -> impl Strategy<Value = String> {
    prop_oneof![
      "[a-z ]{0,6}",
      Just("<<<<<<< Conflict 1 of 2".to_string()),
      Just("<<<<<<< Side #1".to_string()),
      Just("||||||| Base".to_string()),
      Just("%%%%%%% Changes".to_string()),
      Just("+++++++ Contents".to_string()),
      Just("------- Base".to_string()),
      Just("=======".to_string()),
      Just(">>>>>>> Conflict 1 of 2 ends".to_string()),
      "  ?(<{7}|>{7}|={7}) Conflict [0-9]{1,25} of [0-9]{0,25}",
    ]
  }

  proptest! {
    #![proptest_config(proptest_config(64))]

    /// Every region covers its own file lines: in order, not overlapping,
    /// and each line view is the file's line at that number.
    #[test]
    fn regions_are_ordered_disjoint_and_match_the_file(lines in prop::collection::vec(line(), 0..30)) {
      let content = lines.join("\n");
      let file_lines: Vec<&str> = content.split('\n').collect();
      let regions = parse_conflict_markers(&content, "f.txt");
      let mut previous_end = 0;
      for region in &regions {
        prop_assert!(region.start_line > previous_end, "regions overlap: {:?}", regions);
        prop_assert!(region.end_line >= region.start_line);
        prop_assert_eq!(region.lines.len(), region.end_line - region.start_line + 1);
        prop_assert_eq!(region.total_conflicts >= 1, true);
        for view in &region.lines {
          prop_assert_eq!(view.raw.as_str(), file_lines[view.file_line - 1]);
        }
        previous_end = region.end_line;
      }
    }

    /// Well-formed git conflicts between plain lines parse back exactly.
    #[test]
    fn parses_generated_git_conflicts_back(
      blocks in prop::collection::vec(
        (prop::collection::vec("[a-z ]{0,5}", 0..3),
         prop::collection::vec("[a-z ]{0,5}", 0..3),
         prop::collection::vec("[a-z ]{0,5}", 0..3)),
        0..4,
      )
    ) {
      let mut lines = Vec::new();
      for (before, ours, theirs) in &blocks {
        lines.extend(before.iter().cloned());
        lines.push("<<<<<<< ours".to_string());
        lines.extend(ours.iter().cloned());
        lines.push("=======".to_string());
        lines.extend(theirs.iter().cloned());
        lines.push(">>>>>>> theirs".to_string());
      }
      let regions = parse_conflict_markers(&lines.join("\n"), "f.txt");
      prop_assert_eq!(regions.len(), blocks.len());
      for (region, (_, ours, theirs)) in regions.iter().zip(&blocks) {
        prop_assert_eq!(region.marker_style, ConflictStyle::GitMerge);
        prop_assert_eq!(region.total_conflicts, blocks.len());
        let side = |indexes: &[usize]| indexes.iter().map(|&i| region.lines[i].raw.clone()).collect::<Vec<_>>();
        prop_assert_eq!(&side(&region.comparison.left_line_indexes), ours);
        prop_assert_eq!(&side(&region.comparison.right_line_indexes), theirs);
      }
    }
  }
}
