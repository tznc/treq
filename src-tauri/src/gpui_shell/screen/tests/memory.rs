//! Long-session memory: what grows without a byte cap, measured with the
//! counting allocator in `allocs.rs`. Every bench is ignored; run one with
//! `cargo test --features gpui-shell --lib memory_ -- --ignored --nocapture
//! --test-threads=1`.

use std::time::Instant;

use super::{
  allocs::{heap, totals},
  *,
};
use crate::core::agent_chat;
use gpui::TestAppContext;

fn kib(bytes: isize) -> f64 {
  bytes as f64 / 1024.
}

/// A line of agent output, different on every line.
fn prose(i: usize) -> String {
  format!(
    "  I read src/module_{i}.rs and the function compute_{i} returns {} when the cache is cold.",
    i * 7919 % 1000
  )
}

/// A Claude-Code-like screen: a scrolled window of output, a spinner line
/// that changes on every redraw, and the input box.
fn agent_screen(tick: usize, rows: usize) -> String {
  let mut lines: Vec<String> = Vec::new();
  lines.push("╭─ Claude Code ──────────────────────────╮".into());
  let body = rows.saturating_sub(8);
  let first = (tick / 3).saturating_sub(body);
  for i in first..first + body {
    lines.push(prose(i));
  }
  lines.push(String::new());
  lines.push(format!(
    "✻ Thinking… ({}s · ↑ {:.1}k tokens · esc to interrupt)",
    tick, tick as f64 * 0.03
  ));
  lines.push(String::new());
  lines.push("╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌".into());
  lines.push(">".into());
  lines.push("╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌".into());
  lines.join("\n")
}

/// The chat recorder rewrites the whole chat file once a second while an
/// agent streams. Prints, per tick count, the cost of one record and the
/// size of what it keeps.
#[test]
#[ignore = "benchmark"]
fn memory_chat_recorder() {
  let dir = tempfile::tempdir().expect("tempdir");
  let repo = dir.path().to_str().expect("utf8").to_string();
  agent_chat::register_agent_chat(&repo, 1, "pty-1", "Claude", "claude", None, None)
    .expect("register");
  agent_chat::record_screen(&repo, 1, &agent_screen(0, 50)).expect("first");
  agent_chat::record_user_message(&repo, 1, &agent_screen(0, 50), "fix the bug in the cache")
    .expect("user");
  let path = agent_chat::agent_chats_dir(&repo).join("1.json");
  let mut written: u64 = 0;
  for tick in 1..=3600usize {
    let (a0, b0) = totals();
    let start = Instant::now();
    let chat = agent_chat::record_screen(&repo, 1, &agent_screen(tick, 50)).expect("record");
    let took = start.elapsed();
    let (a1, b1) = totals();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    written += size;
    if [1, 10, 60, 300, 600, 1200, 1800, 3600].contains(&tick) {
      println!(
        "tick {tick:>5}: messages={:>5} file={:>9} B  record={:>8.2} ms  allocs={:>6}  allocated={:>9.0} KiB  written so far={:>8.1} MiB",
        chat.messages.len(),
        size,
        took.as_secs_f64() * 1000.,
        a1 - a0,
        kib((b1 - b0) as isize),
        written as f64 / 1048576.
      );
    }
  }
}

/// Typing into a TUI that does not echo the key at the start of a chunk
/// leaves the key in `pending_echo` for good, and every later chunk scans it.
#[test]
#[ignore = "benchmark"]
fn memory_pending_echo() {
  let host: terminal_data::Host = Arc::new(terminal_host());
  let (live, _) = terminal_data::attach(&host, "memory-echo", 120, 40);
  // A repaint, as a TUI sends it: it never starts with the typed key.
  let chunk: String = format!("\x1b[2K\x1b[1A{}", "x".repeat(8000));
  let mut typed = 0usize;
  for target in [0usize, 1_000, 10_000, 100_000, 1_000_000] {
    while typed < target {
      live.typed("k");
      typed += 1;
    }
    let (a0, b0) = totals();
    let start = Instant::now();
    for _ in 0..20 {
      live.push_bytes(chunk.as_bytes());
    }
    let took = start.elapsed() / 20;
    let (a1, b1) = totals();
    println!(
      "typed={target:>8}: push_bytes(8 KB) = {:>8.1} us, {:>6} allocs, {:>9.1} KiB allocated per chunk",
      took.as_secs_f64() * 1e6,
      (a1 - a0) / 20,
      kib(((b1 - b0) / 20) as isize),
    );
  }
}

/// The in-memory file index kept for every workspace that was shown.
#[test]
#[ignore = "benchmark"]
fn memory_file_index() {
  for files in [10_000usize, 100_000] {
    let paths: Vec<String> = (0..files)
      .map(|i| format!("packages/pkg_{}/src/components/Widget{}/index_{i}.tsx", i % 200, i % 50))
      .collect();
    let avg = paths.iter().map(String::len).sum::<usize>() / files;
    let before = heap();
    let index = crate::file_indexer::FileIndex::new(paths);
    let held = heap() - before;
    println!(
      "{files:>7} files (avg path {avg} B): index holds {:>8.1} KiB = {:.0} B per file",
      kib(held),
      held as f64 / files as f64
    );
    drop(index);
  }
}

/// The syntax highlight cache holds up to 20,000 lines, whatever their size.
#[test]
#[ignore = "benchmark"]
fn memory_highlight_cache() {
  let rust = highlight::language("a.rs").expect("rust");
  let js = highlight::language("a.js").expect("js");
  let typical = |i: usize| format!("    let value_{i} = compute(\"item {i}\", {i}).map(|v| v + {i});");
  let minified = |i: usize| {
    let mut s = String::new();
    let mut n = 0;
    while s.len() < 19_900 {
      s.push_str(&format!("function f{i}_{n}(a,b){{return a+b*{n}+\"s{n}\"}};var v{n}={n};"));
      n += 1;
    }
    s
  };
  for (name, language, make, count) in [
    ("typical rust line", rust, &typical as &dyn Fn(usize) -> String, 5_000usize),
    ("minified js line (<20k)", js, &minified as &dyn Fn(usize) -> String, 200usize),
  ] {
    let before = heap();
    let mut chars = 0;
    for i in 0..count {
      let line = make(i);
      chars += line.len();
      highlight::spans(language, &line);
    }
    let held = heap() - before;
    println!(
      "{name:<24}: {count} lines, avg {} chars: cache holds {:>9.1} KiB = {:.0} B per line; x20,000 lines = {:.0} MiB",
      chars / count,
      kib(held),
      held as f64 / count as f64,
      held as f64 / count as f64 * 20_000. / 1048576.
    );
  }
}

/// A terminal's scrollback, by width.
#[test]
#[ignore = "benchmark"]
fn memory_terminal_scrollback() {
  for (cols, rows) in [(80usize, 24usize), (120, 40), (250, 70)] {
    let before = heap();
    let mut emulator = terminal_emu::Emulator::new(cols, rows);
    let empty = heap() - before;
    for i in 0..20_000 {
      emulator.feed(format!("\x1b[3{}m{i:>6}\x1b[0m output line {i}\r\n", i % 8).as_bytes());
    }
    let held = heap() - before;
    println!(
      "{cols}x{rows}: fresh {:>8.1} KiB, after 20,000 lines (history {}) {:>9.1} KiB",
      kib(empty),
      emulator.history(),
      kib(held)
    );
  }
}

/// One commit's diff, as the Commits tab keeps it.
#[test]
#[ignore = "benchmark"]
fn memory_commit_diff() {
  let diff = commits_large::large_host().diff.clone().expect("diff");
  let before = heap();
  let copy = diff.clone();
  let held = heap() - before;
  println!(
    "commit diff, 6 files x 500 lines of ~70 chars: {:>8.1} KiB (16 in the LRU = {:.1} MiB)",
    kib(held),
    kib(held) * 16. / 1024.
  );
  drop(copy);
}

/// Opens and closes a shell terminal over and over: what each leaves
/// behind (its scrollback, its view, its kept column) shows as live heap.
#[gpui::test]
#[ignore = "benchmark"]
fn memory_terminal_open_close(cx: &mut TestAppContext) {
  let (mut h, host, _) = terminal_perf::pane(cx, 0);
  h.cx.run_until_parked();
  let base = heap();
  for n in 1..=40 {
    h.cx.simulate_keystrokes("cmd-\\");
    h.cx.run_until_parked();
    let ptys = host.ptys.lock().unwrap().clone();
    let pty = ptys.last().expect("pty").clone();
    host.output(&pty, &terminal_perf::lines(0, 1500));
    h.cx.run_until_parked();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.simulate_keystrokes("cmd-w");
    h.cx.run_until_parked();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    if [1, 5, 10, 20, 40].contains(&n) {
      println!(
        "after {n:>3} shells opened + closed: live {:+9.1} KiB",
        kib(heap() - base)
      );
    }
  }
}

/// Opens and closes a dialog over and over.
#[gpui::test]
#[ignore = "benchmark"]
fn memory_modal_open_close(cx: &mut TestAppContext) {
  for (name, modal) in [("agent prompt", agent_modal()), ("prompt history", history_modal())] {
    let (mut h, _) = modal_harness(cx, history_host(), modal.clone());
    h.set_modal(None);
    let base = heap();
    for n in 1..=40 {
      h.set_modal(Some(modal.clone()));
      h.cx.update(|window, cx| window.draw(cx).clear(cx));
      h.set_modal(None);
      h.cx.update(|window, cx| window.draw(cx).clear(cx));
      if [1, 5, 10, 20, 40].contains(&n) {
        println!(
          "{name:<15} after {n:>3} open/close: live {:+9.1} KiB",
          kib(heap() - base)
        );
      }
    }
  }
}

/// The prompt history of a heavy user: thousands of prompts, kilobytes each.
#[gpui::test]
#[ignore = "benchmark"]
fn memory_prompt_history_large(cx: &mut TestAppContext) {
  let mut host = history_host();
  host.prompts = (0..5000)
    .map(|i| {
      prompt_entry(
        i + 1,
        Some(7),
        Some("login fixes"),
        &format!("Prompt {i}: {}", "please look at the failing test and fix it. ".repeat(50)),
        Some("claude"),
      )
    })
    .collect();
  let (mut h, _) = modal_harness(cx, host, serde_json::json!({ "kind": "none" }));
  h.set_modal(None);
  let base = heap();
  let (a0, b0) = totals();
  let start = Instant::now();
  h.set_modal(Some(history_modal()));
  h.cx.update(|window, cx| window.draw(cx).clear(cx));
  let opened = start.elapsed();
  let (a1, b1) = totals();
  println!(
    "5000 prompts x 2.2 KB: open + first frame {:.1} ms, {} allocs, {:.0} KiB allocated, live {:+.0} KiB",
    opened.as_secs_f64() * 1000.,
    a1 - a0,
    kib((b1 - b0) as isize),
    kib(heap() - base)
  );
  let mut samples: Vec<_> = (0..15)
    .map(|_| {
      h.cx.update(|window, cx| {
        window.refresh();
        let (a, b) = totals();
        let start = Instant::now();
        window.draw(cx).clear(cx);
        let (a2, b2) = totals();
        (start.elapsed(), a2 - a, b2 - b)
      })
    })
    .collect();
  samples.sort();
  let (took, allocs, bytes) = samples[7];
  println!(
    "one frame with the history open: {:.2} ms, {allocs} allocs, {:.0} KiB",
    took.as_secs_f64() * 1000.,
    kib(bytes as isize)
  );
}

/// The file index of a workspace is built once and never read again, so a
/// file an agent creates later is not found by "Jump to File" or an `@`
/// mention until the app restarts.
#[test]
#[ignore = "benchmark"]
fn memory_file_index_goes_stale() {
  let dir = tempfile::tempdir().expect("tempdir");
  let path = dir.path().to_str().expect("utf8").to_string();
  crate::jj::jj_git_init_colocated(&path).expect("init");
  std::fs::write(dir.path().join("a.txt"), "a").expect("write");
  let first = crate::commands::workspace::workspace_file_index(&path).expect("index");
  std::fs::write(dir.path().join("new_file_made_by_the_agent.txt"), "b").expect("write");
  let again = crate::commands::workspace::workspace_file_index(&path).expect("index");
  println!(
    "index after the first file: {} files; after a second file was created: {} files; same Arc: {}",
    first.len(),
    again.len(),
    std::sync::Arc::ptr_eq(&first, &again)
  );
  println!(
    "search for the new file finds: {:?}",
    again.search("new_file_made", 5)
  );
}

/// Typing into the Code tab's task composer, empty and holding a long
/// pasted prompt: allocations and time per keystroke (with its frame).
#[gpui::test]
#[ignore = "benchmark"]
fn memory_composer_typing(cx: &mut TestAppContext) {
  let (mut h, _) = workspace_harness(cx, code_host(), code_route());
  h.click("task-input");
  h.cx.run_until_parked();
  for (name, paste) in [("empty composer", 0usize), ("20 KB pasted prompt", 20_000), ("100 KB pasted prompt", 100_000)] {
    if paste > 0 {
      let text = "please check src/lib.rs and fix the failing test in module a\n".repeat(paste / 62);
      h.cx.update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)));
      h.cx.simulate_keystrokes("cmd-v");
      h.cx.run_until_parked();
      h.cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    let mut samples: Vec<(std::time::Duration, usize, usize)> = (0..20)
      .map(|_| {
        let (a0, b0) = totals();
        let start = Instant::now();
        h.cx.simulate_input("x");
        h.cx.run_until_parked();
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        let (a1, b1) = totals();
        (start.elapsed(), a1 - a0, b1 - b0)
      })
      .collect();
    samples.sort();
    let (took, allocs, bytes) = samples[10];
    println!(
      "{name:<22}: one keystroke + frame p50 {:.2} ms, {allocs} allocs, {:.0} KiB allocated",
      took.as_secs_f64() * 1000.,
      kib(bytes as isize)
    );
  }
}

/// `cat` of a big log into a maximized agent terminal: parse time per MB
/// on the reader side and the UI work it queues, with the scrollback full.
#[gpui::test]
#[ignore = "benchmark"]
fn memory_terminal_flood(cx: &mut TestAppContext) {
  let (mut h, host, ptys) = terminal_perf::pane(cx, 0);
  h.click("terminal-maximize");
  h.cx.run_until_parked();
  let pty = &ptys[0];
  let before = heap();
  let blob = terminal_perf::lines(0, 40_000);
  let chunks: Vec<&str> = blob
    .as_bytes()
    .chunks(8192)
    .map(|c| std::str::from_utf8(c).unwrap_or_default())
    .collect();
  let start = Instant::now();
  let mut ui = std::time::Duration::ZERO;
  for (n, chunk) in chunks.iter().enumerate() {
    host.output(pty, chunk);
    if n % 8 == 7 {
      let t = Instant::now();
      h.cx.run_until_parked();
      h.cx.update(|window, cx| window.draw(cx).clear(cx));
      ui += t.elapsed();
    }
  }
  let total = start.elapsed();
  println!(
    "flood of {:.1} MB in {} chunks: total {:.0} ms ({:.0} ms parse side, {:.0} ms UI side with a frame every 8 chunks); live {:+.1} MiB",
    blob.len() as f64 / 1048576.,
    chunks.len(),
    total.as_secs_f64() * 1000.,
    (total - ui).as_secs_f64() * 1000.,
    ui.as_secs_f64() * 1000.,
    (heap() - before) as f64 / 1048576.
  );
}
