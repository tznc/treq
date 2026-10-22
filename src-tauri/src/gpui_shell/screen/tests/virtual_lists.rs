//! The other long lists as virtual lists: a large merge preview, stash
//! diff, prompt history and text attachment draw only the rows on screen,
//! and a scroll brings the rest in.

use gpui::{Modifiers, TestAppContext};

use super::*;

const FILES: usize = 6;
const LINES: usize = 500;

/// `FILES` files of `LINES` changed lines each.
fn large_diff() -> crate::jj::JjRevisionDiff {
  let files: Vec<_> = (0..FILES)
    .map(|f| {
      serde_json::json!({
        "path": format!("src/f{f}.rs"), "status": "M", "previous_path": null,
        "changed_line_count": LINES, "diff_deferred": false,
      })
    })
    .collect();
  let hunks: Vec<_> = (0..FILES)
    .map(|f| {
      let lines: Vec<String> = (0..LINES).map(|i| format!("+let v{i} = {f};")).collect();
      serde_json::json!({
        "path": format!("src/f{f}.rs"),
        "hunks": [{"id": "h", "header": format!("@@ -0,0 +1,{LINES} @@"), "lines": lines, "patch": ""}],
      })
    })
    .collect();
  serde_json::from_value(serde_json::json!({
    "committed_files": files,
    "hunks_by_file": hunks,
    "too_large_to_render": false,
  }))
  .expect("diff")
}

/// Scrolls the wheel over `selector` until `target` is drawn, or panics.
fn scroll_until(h: &mut Harness, selector: &'static str, target: &'static str) {
  let at = h.cx.debug_bounds(selector).expect(selector).center();
  for _ in 0..200 {
    if h.drawn(target) {
      return;
    }
    h.cx.simulate_event(gpui::ScrollWheelEvent {
      position: at,
      delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-800.))),
      modifiers: Modifiers::none(),
      touch_phase: gpui::TouchPhase::Moved,
    });
  }
  panic!("{target} never scrolled into view");
}

#[gpui::test]
fn a_large_merge_preview_draws_only_the_rows_on_screen(cx: &mut TestAppContext) {
  let mut merge = merge_preview(2);
  merge.diff = Some(large_diff());
  let (mut h, _) = merge_harness(
    cx,
    FakeHost {
      merge,
      ..FakeHost::default()
    },
  );
  assert!(h.drawn("merge-file-src/f0.rs"));
  assert!(!h.drawn("merge-file-src/f5.rs"), "far below the window");
  // The message card spans the page, as it did before the list.
  let page = h.cx.debug_bounds("merge-scroll").expect("page");
  let message = h.cx.debug_bounds("merge-message").expect("message");
  assert!(
    message.size.width > page.size.width * 0.5,
    "{message:?} in {page:?}"
  );
  scroll_until(&mut h, "merge-scroll", "merge-file-src/f5.rs");
  assert!(!h.drawn("merge-file-src/f0.rs"));
  // A folded file takes its lines with it.
  h.click("merge-file-src/f5.rs");
  assert!(!h.drawn("merge-hunk-5-0"));
}

#[gpui::test]
fn a_large_stash_diff_draws_only_the_rows_on_screen(cx: &mut TestAppContext) {
  let host = FakeHost {
    stash_diff: Some(large_diff()),
    ..stash_host()
  };
  let (mut h, _) = modal_harness(cx, host, stash_modal());
  assert!(h.drawn("stash-diff-file-0"));
  assert!(!h.drawn("stash-diff-file-5"));
  scroll_until(&mut h, "stash-diff-pane", "stash-diff-file-5");
  // Another stash starts from its top.
  h.click("stash-list-item-1");
  assert!(h.drawn("stash-diff-file-0"));
}

#[gpui::test]
fn a_long_prompt_history_draws_only_the_rows_on_screen(cx: &mut TestAppContext) {
  let prompts = (0..2000)
    .map(|i| {
      let mut entry = prompt_entry(2000 - i, Some(7), Some("login"), &"x".repeat(5000), None);
      entry.created_at = "2026-10-19T10:05:09Z".into();
      entry
    })
    .collect();
  let host = FakeHost {
    prompts,
    ..FakeHost::default()
  };
  let (mut h, _) = modal_harness(cx, host, history_modal());
  assert!(h.drawn("prompt-history-item-0"));
  assert!(!h.drawn("prompt-history-item-1999"));
  scroll_until(&mut h, "prompt-history-list", "prompt-history-item-1999");
  h.click("prompt-history-item-1999");
  let selected = h.screen.read_with(&h.cx, |s, cx| {
    s.prompt_history.as_ref().map(|v| v.read(cx).selected)
  });
  assert_eq!(selected, Some(Some(1)));
}

#[gpui::test]
fn a_long_text_attachment_draws_only_the_lines_on_screen(cx: &mut TestAppContext) {
  let (mut h, host) = workspace_harness(cx, terminal_host(), terminal_route(serde_json::json!({})));
  *host.send.lock().unwrap() = serde_json::from_value(serde_json::json!([
    { "id": "t1", "repo": "/repo", "ptySessionId": agent_pty(), "mediaType": "text",
      "path": "/tmp/long.txt", "title": "long.txt" },
  ]))
  .expect("assets");
  set_terminal(&mut h, serde_json::json!({ "activeSessionId": 3 }));
  h.click("terminal-send-preview-t1");
  assert!(h.drawn("treq-send-text-line-1"));
  assert!(!h.drawn("treq-send-text-line-2000"));
  scroll_until(&mut h, "treq-send-text-preview", "treq-send-text-line-2000");
  // A comment on a line far down sits under it.
  h.click("treq-send-text-line-2000");
  assert!(h.drawn("treq-send-line-comment-composer"));
  let line = h.cx.debug_bounds("treq-send-text-line-2000").expect("line");
  let composer = h
    .cx
    .debug_bounds("treq-send-line-comment-composer")
    .expect("composer");
  assert!(composer.top() >= line.bottom() - gpui::px(1.));
}
