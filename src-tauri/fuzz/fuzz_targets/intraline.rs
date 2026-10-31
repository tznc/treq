#![no_main]

#[path = "../../src/gpui_shell/screen/intraline.rs"]
#[allow(dead_code)]
mod intraline;

use libfuzzer_sys::fuzz_target;

// Input: hunk lines separated by newlines. Every mark must be an in-order,
// non-empty byte range on char boundaries of the line's text.
fuzz_target!(|data: &[u8]| {
  let Ok(text) = std::str::from_utf8(data) else {
    return;
  };
  let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
  for (index, marks) in intraline::changed_ranges(&lines) {
    let body = lines[index].get(1..).unwrap_or("");
    let mut previous_end = 0;
    for mark in &marks {
      assert!(mark.start < mark.end && mark.start >= previous_end);
      assert!(body.get(mark.clone()).is_some(), "mark off a char boundary");
      previous_end = mark.end;
    }
  }
});
