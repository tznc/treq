#![no_main]

#[path = "../../src/unified_patch.rs"]
#[allow(dead_code)]
mod unified_patch;

use libfuzzer_sys::fuzz_target;

// Input: the patch, then an optional original file after a NUL byte.
fuzz_target!(|data: &[u8]| {
  let Ok(text) = std::str::from_utf8(data) else {
    return;
  };
  let (patch, original) = match text.split_once('\0') {
    Some((patch, original)) => (patch, Some(original)),
    None => (text, None),
  };
  if let Ok(patches) = unified_patch::parse(patch) {
    for file_patch in &patches {
      let _ = unified_patch::apply(original, file_patch);
    }
  }
});
