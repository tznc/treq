//! Round 6 measurement: what shaping costs on the real (cosmic-text) text
//! system, and which glyphs send it through the system font fallback.
//! Run under strace to count font files opened between the `MARK` lines:
//! `strace -f -e trace=openat,write -o out cargo-test-binary fonts_perf --ignored --nocapture`

use std::{borrow::Cow, time::Instant};

use gpui::{font, px, FontRun, PlatformTextSystem};
use gpui_wgpu::CosmicTextSystem;

const INTER_REGULAR: &[u8] = include_bytes!("../../../mobile-gpui/assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../../mobile-gpui/assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../mobile-gpui/assets/fonts/Inter-SemiBold.ttf");
const JB_REGULAR: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
const JB_BOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");

fn mark(name: &str, started: &mut Instant) {
  let now = Instant::now();
  println!("MARK {name}: +{:?}", now - *started);
  // A write the strace log can find.
  let _ = std::fs::metadata(format!("/nonexistent/MARK_{name}"));
  *started = Instant::now();
}

fn shape(system: &CosmicTextSystem, family: &'static str, text: &str) {
  let id = system.font_id(&font(family)).expect("font");
  let runs = [FontRun {
    len: text.len(),
    font_id: id,
  }];
  let line = system.layout_line(text, px(13.), &runs);
  std::hint::black_box(line);
}

#[test]
#[ignore = "measurement"]
fn fonts_perf() {
  let mut t = Instant::now();
  let system = CosmicTextSystem::new("DejaVu Sans");
  mark("system_new", &mut t);
  system
    .add_fonts(vec![
      Cow::Borrowed(INTER_REGULAR),
      Cow::Borrowed(INTER_MEDIUM),
      Cow::Borrowed(INTER_SEMIBOLD),
      Cow::Borrowed(JB_REGULAR),
      Cow::Borrowed(JB_BOLD),
    ])
    .unwrap();
  mark("add_fonts", &mut t);
  shape(&system, "Inter", "Settings Close Save changes");
  mark("inter_ascii_first", &mut t);
  shape(&system, "Inter", "Command palette — Go to Home › … · ↑↓ ↵");
  mark("inter_chrome_glyphs", &mut t);
  shape(&system, "JetBrains Mono", "fn main() { println!(\"hi\"); }");
  mark("mono_ascii_first", &mut t);
  for (name, text) in [
    ("mono_arrow", "a → b"),
    ("mono_cmd", "⌘K"),
    ("mono_prompt", "❯ ~/src"),
    ("mono_box", "┌──┐│ │└──┘"),
    ("inter_cjk", "你好 日本"),
    ("inter_emoji", "🎉 🚀"),
    ("mono_powerline", "\u{e0b0}\u{e0a0}"),
    ("mono_private", "\u{f015}"),
    ("mono_replacement", "\u{fffd}"),
  ] {
    shape(&system, if name.starts_with("inter") { "Inter" } else { "JetBrains Mono" }, text);
    mark(name, &mut t);
  }
  // Repeated shaping of the same string: cosmic-text has no cache, GPUI's
  // LineLayoutCache does.
  for _ in 0..3 {
    shape(&system, "Inter", "Settings Close Save changes");
  }
  mark("inter_ascii_repeat_x3", &mut t);
}
