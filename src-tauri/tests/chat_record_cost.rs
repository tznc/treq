//! Cost of the agent chat recorder over a long streaming session.
//!
//! `cargo test --test chat_record_cost -- --ignored --nocapture`
//! (`CHAT_TICKS=3600` for an hour). Linux only: bytes written come from
//! `/proc/self/io`.
#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::undocumented_unsafe_blocks,
  reason = "measurement test"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(layout.size(), Ordering::Relaxed);
    System.alloc(layout)
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    System.dealloc(ptr, layout)
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(new, Ordering::Relaxed);
    System.realloc(ptr, layout, new)
  }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn written_bytes() -> u64 {
  std::fs::read_to_string("/proc/self/io")
    .unwrap()
    .lines()
    .find_map(|line| line.strip_prefix("wchar: "))
    .unwrap()
    .trim()
    .parse()
    .unwrap()
}

/// A 50-row Claude-like screen at tick `t`: the transcript scrolls one line a
/// tick, a spinner line changes every tick, and the input box sits below.
fn screen(t: usize) -> String {
  let mut rows: Vec<String> = (0..44)
    .map(|row| {
      let n = t + row;
      format!(
        "  ● Edited src/module_{}/file_{}.rs: line {n} of the agent's streamed output",
        n / 7,
        n % 13
      )
    })
    .collect();
  rows.push(format!("✻ Thinking… ({}s · esc to interrupt)", t));
  rows.push(String::new());
  rows.push("───────────────────────────────────────────────────────────────".into());
  rows.push("> ".into());
  rows.push("───────────────────────────────────────────────────────────────".into());
  rows.join("\n")
}

#[test]
#[ignore = "measurement"]
fn record_cost_over_a_streaming_session() {
  let ticks: usize = std::env::var("CHAT_TICKS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(600);
  let dir = tempfile::TempDir::new().unwrap();
  let repo = dir.path().to_str().unwrap();
  treq_lib::core::register_agent_chat(repo, 1, "pty", "Claude", "claude", None, None).unwrap();
  let marks = [60, 600, 1800, 3600];
  let (mut total_time, mut window_allocs, mut window_bytes, mut window_io) = (0.0, 0, 0, 0);
  let start_io = written_bytes();
  for t in 0..ticks {
    let text = screen(t);
    let w0 = written_bytes();
    let (a0, b0) = (
      ALLOCS.load(Ordering::Relaxed),
      BYTES.load(Ordering::Relaxed),
    );
    let started = Instant::now();
    let _ = treq_lib::core::record_screen(repo, 1, &text);
    total_time += started.elapsed().as_secs_f64();
    window_allocs += ALLOCS.load(Ordering::Relaxed) - a0;
    window_bytes += BYTES.load(Ordering::Relaxed) - b0;
    window_io += written_bytes() - w0;
    if marks.contains(&(t + 1)) || t + 1 == ticks {
      let n = if marks.contains(&(t + 1)) {
        t + 1
      } else {
        ticks
      };
      let file = std::fs::metadata(dir.path().join(".treq/agent-chats/1.json"))
        .unwrap()
        .len();
      println!(
        "tick {n}: file {} KiB, mean per record {:.2} ms, {} allocs, {} KiB allocated, {} B written; written so far {} MiB",
        file / 1024,
        total_time * 1000.0 / n as f64,
        window_allocs / n,
        window_bytes / n / 1024,
        window_io / n as u64,
        (written_bytes() - start_io) / (1 << 20)
      );
    }
  }
}
