#![no_main]

#[path = "../../src/conflict_markers.rs"]
#[allow(dead_code)]
mod conflict_markers;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
  let Ok(text) = std::str::from_utf8(data) else {
    return;
  };
  let regions = conflict_markers::parse_conflict_markers(text, "f.txt");
  let file_lines: Vec<&str> = text.split('\n').collect();
  let mut previous_end = 0;
  for region in &regions {
    assert!(region.start_line > previous_end, "regions overlap");
    assert!(region.end_line >= region.start_line);
    for view in &region.lines {
      assert_eq!(view.raw, file_lines[view.file_line - 1]);
    }
    previous_end = region.end_line;
  }
});
