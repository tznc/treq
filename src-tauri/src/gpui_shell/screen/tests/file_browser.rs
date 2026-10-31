//! The Code tab's file browser, drawn by GPUI from the backend.

use std::{collections::HashMap, time::Duration};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext};

use super::*;
use crate::gpui_shell::screen::file_browser::FileBrowserView;

const BASE: &str = "/repo/.treq/workspaces/login";

fn entry(rel: &str, is_directory: bool) -> crate::core::WorkspaceEntry {
  crate::core::WorkspaceEntry {
    name: rel.rsplit('/').next().unwrap_or(rel).into(),
    path: format!("{BASE}/{rel}"),
    is_directory,
    modified_at: None,
    submodule_pin: None,
    submodule_synced: None,
    status: None,
  }
}

fn browser_host() -> FakeHost {
  let mut host = code_host();
  host.dirs = HashMap::from([
    (
      BASE.to_string(),
      vec![
        entry("src", true),
        entry(".git", true),
        entry("README.md", false),
        entry("a.rs", false),
        entry("logo.png", false),
        entry("gone.txt", false),
      ],
    ),
    (format!("{BASE}/src"), vec![entry("src/login.rs", false)]),
  ]);
  host.file_texts = HashMap::from([
    (format!("{BASE}/README.md"), "# Login\nSign in here".into()),
    (format!("{BASE}/a.rs"), "a\nb\nc".into()),
    (
      format!("{BASE}/src/login.rs"),
      "fn login() {\n  login_user();\n}".into(),
    ),
  ]);
  host.working = serde_json::from_value(serde_json::json!({
    "committed_files": [],
    "uncommitted_files": [
      {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 2, "diff_deferred": false}
    ],
    "hunks_by_file": [
      {"path": "a.rs", "hunks": [{"id": "h", "header": "@@ -1,2 +1,3 @@", "lines": [" a", "+b", "+c"], "patch": ""}]}
    ],
    "too_large_to_render": false
  }))
  .ok();
  host
}

fn browser_route(open: serde_json::Value) -> ScreenSnapshot {
  let mut route = route_json(Some(7), "main");
  route["code"] = true.into();
  route["frameHeight"] = 120.into();
  route["fileBrowser"] = open;
  serde_json::from_value(route).expect("file browser route")
}

fn harness(cx: &mut TestAppContext, open: serde_json::Value) -> (Harness, Arc<FakeHost>) {
  workspace_harness(cx, browser_host(), browser_route(open))
}

fn browser(h: &mut Harness) -> Entity<FileBrowserView> {
  h.cx.run_until_parked();
  h.screen.read_with(&h.cx, |screen, cx| {
    let workspace = screen.workspace.as_ref().expect("workspace").read(cx);
    workspace.file_browser().cloned().expect("file browser")
  })
}

fn selected(h: &mut Harness) -> Option<String> {
  let browser = browser(h);
  browser.read_with(&h.cx, |b, _| b.selected.clone())
}

#[gpui::test]
fn file_browser_replaces_the_code_tab_and_opens_the_readme(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, serde_json::json!({}));
  assert!(h.drawn("file-browser"));
  assert!(!h.drawn("code-readme"));
  assert_eq!(selected(&mut h), Some(format!("{BASE}/README.md")));
  assert!(h.drawn("file-content"));
  assert!(h.drawn("file-content-path"));
  assert!(h.drawn("code-line-1"));
  assert!(h.drawn("code-line-2"));
  assert!(!h.drawn("code-line-3"));
  assert!(h.drawn("file-modified"));
  // Markdown offers its preview; hidden directories are left out.
  assert!(h.drawn("markdown-preview-tab"));
  assert!(h.drawn("file-tree-src"));
  assert!(!h.drawn("file-tree-.git"));
  assert!(host
    .calls()
    .contains(&format!("read file {BASE}/README.md")));
}

#[gpui::test]
fn file_browser_expands_folders_and_opens_files(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, serde_json::json!({}));
  assert!(!h.drawn("file-tree-src/login.rs"));
  h.click("file-tree-src");
  assert!(h.drawn("file-tree-src/login.rs"));
  assert!(host.calls().contains(&format!("read dir {BASE}/src")));
  h.click("file-tree-src/login.rs");
  assert_eq!(selected(&mut h), Some(format!("{BASE}/src/login.rs")));
  assert!(h.drawn("code-line-3"));
  assert!(!h.drawn("markdown-preview-tab"));
  // Clicking the folder again folds it.
  h.click("file-tree-src");
  assert!(!h.drawn("file-tree-src/login.rs"));
}

#[gpui::test]
fn file_browser_opens_what_react_asks_for(cx: &mut TestAppContext) {
  let (mut h, _) = harness(
    cx,
    serde_json::json!({
      "selectedFile": format!("{BASE}/src/login.rs"),
      "expandedDir": format!("{BASE}/src"),
    }),
  );
  assert_eq!(selected(&mut h), Some(format!("{BASE}/src/login.rs")));
  assert!(h.drawn("file-tree-src/login.rs"));
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.file_browser.as_mut().unwrap().selected_file = Some(format!("{BASE}/a.rs"));
    }
  });
  assert_eq!(selected(&mut h), Some(format!("{BASE}/a.rs")));
}

#[gpui::test]
fn file_browser_marks_the_working_copy_changes(cx: &mut TestAppContext) {
  let (mut h, _) = harness(cx, serde_json::json!({}));
  h.click("file-tree-a.rs");
  let browser = browser(&mut h);
  let added = browser.read_with(&h.cx, |b, _| {
    let mut lines: Vec<u32> = b.marks.added.iter().copied().collect();
    lines.sort();
    lines
  });
  assert_eq!(added, [2, 3]);
}

#[gpui::test]
fn file_browser_shows_binary_and_unreadable_files(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, serde_json::json!({}));
  h.click("file-tree-logo.png");
  assert!(h.drawn("file-browser-binary"));
  assert!(!host.calls().contains(&format!("read file {BASE}/logo.png")));
  h.click("file-tree-gone.txt");
  assert!(h.drawn("file-browser-failed"));
  assert!(host
    .calls()
    .contains(&"notify toast Failed to read file".to_string()));
}

#[gpui::test]
fn file_browser_back_hands_the_tab_to_react(cx: &mut TestAppContext) {
  let (mut h, _) = harness(cx, serde_json::json!({}));
  h.click("file-browser-back");
  h.cx.run_until_parked();
  assert!(h.sent().contains(&press("fileBrowser.back")));
  // React closes it: the Code tab's overview comes back.
  h.update(|s| {
    if let View::Workspace(route) = &mut s.view {
      route.file_browser = None;
    }
  });
  assert!(!h.drawn("file-browser"));
}

fn comment_line_one(h: &mut Harness, text: &str) {
  h.click("add-comment-1");
  assert!(h.drawn("comment-input"));
  h.cx.simulate_input(text);
  h.click("comment-submit");
  h.cx.executor().advance_clock(Duration::from_millis(500));
  h.cx.run_until_parked();
}

#[gpui::test]
fn file_browser_keeps_comments_and_finishes_the_review(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, serde_json::json!({}));
  assert!(!h.drawn("file-browser-pending"));
  comment_line_one(&mut h, "Say who");
  assert!(h.drawn("file-browser-pending"));
  assert!(h.drawn("file-comment-card"));
  let saved: serde_json::Value = serde_json::from_str(
    &host
      .browser_review
      .lock()
      .unwrap()
      .clone()
      .unwrap_or_default(),
  )
  .expect("saved review");
  assert_eq!(saved[0]["file_path"], "README.md", "{saved}");
  assert_eq!(saved[0]["start_line"], 1, "{saved}");
  assert_eq!(saved[0]["text"], "Say who", "{saved}");

  h.click("finish-review");
  assert!(h.drawn("finish-review-popover"));
  h.click("review-plan");
  h.cx.run_until_parked();
  assert!(fields(&h, "fileBrowser.review").is_empty());
  let launches = host.launches.lock().unwrap().clone();
  assert_eq!(launches.len(), 1);
  assert_eq!(launches[0].0.session_name.as_deref(), Some("Code Review"));
  assert_eq!(launches[0].0.mode.as_deref(), Some("plan"));
  let markdown = launches[0].0.prompt.clone().unwrap_or_default();
  assert!(markdown.contains("README.md"), "{markdown}");
  assert_eq!(
    host
      .calls()
      .iter()
      .filter(|c| c.starts_with("record review"))
      .count(),
    1
  );
  assert!(markdown.contains("Say who"), "{markdown}");
  assert!(!h.drawn("file-browser-pending"));
  assert!(host.calls().contains(&"clear browser review 7".to_string()));
}

#[gpui::test]
fn file_browser_loads_the_kept_review(cx: &mut TestAppContext) {
  let host = browser_host();
  *host.browser_review.lock().unwrap() = Some(
    serde_json::json!([{
      "id": "c1",
      "file_path": "README.md",
      "hunk_id": "browser",
      "start_line": 2,
      "end_line": 2,
      "line_content": ["Sign in here"],
      "text": "Kept",
      "created_at": "2026-10-19T00:00:00Z"
    }])
    .to_string(),
  );
  let (mut h, _) = workspace_harness(cx, host, browser_route(serde_json::json!({})));
  assert!(h.drawn("file-browser-pending"));
  assert!(h.drawn("file-comment-card"));
}

#[gpui::test]
fn file_browser_discards_the_review_after_confirming(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, serde_json::json!({}));
  comment_line_one(&mut h, "Drop me");
  h.click("discard-review");
  assert!(h.drawn("discard-review-dialog"));
  h.click("keep-reviewing");
  assert!(!h.drawn("discard-review-dialog"));
  assert!(h.drawn("file-browser-pending"));
  h.click("discard-review");
  h.click("confirm-discard-review");
  assert!(!h.drawn("file-browser-pending"));
  assert!(host
    .calls()
    .contains(&"notify toast Review canceled".to_string()));
}

#[gpui::test]
fn file_browser_finds_text_in_the_open_file(cx: &mut TestAppContext) {
  let (mut h, _) = harness(cx, serde_json::json!({}));
  h.click("file-tree-src");
  h.click("file-tree-src/login.rs");
  h.click("code-line-1");
  h.cx.simulate_keystrokes("cmd-f");
  assert!(h.drawn("file-search"));
  h.cx.simulate_input("login");
  h.cx.executor().advance_clock(Duration::from_millis(150));
  let count = |h: &mut Harness| {
    let browser = browser(h);
    browser.read_with(&h.cx, |b, _| (b.search.current, b.search.matches.len()))
  };
  assert_eq!(count(&mut h), (0, 2));
  h.cx.simulate_keystrokes("enter");
  assert_eq!(count(&mut h), (1, 2));
  h.click("file-search-next");
  assert_eq!(count(&mut h), (0, 2));
  h.click("file-search-close");
  assert!(!h.drawn("file-search"));
}

#[gpui::test]
fn file_browser_tree_menu_copies_paths(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, serde_json::json!({}));
  h.cx.run_until_parked();
  let at = h
    .cx
    .debug_bounds("file-tree-README.md")
    .expect("row")
    .center();
  h.cx
    .simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
  h.cx
    .simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
  h.pick("Copy relative path");
  assert_eq!(
    h.cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
    Some("README.md")
  );
  assert!(host
    .calls()
    .contains(&"notify toast Relative path copied".to_string()));
}

#[gpui::test]
fn file_lines_show_their_numbers_in_a_gutter(cx: &mut TestAppContext) {
  let (mut h, _) = harness(cx, serde_json::json!({}));
  browser(&mut h);
  let gutter = h.cx.debug_bounds("code-gutter-1").expect("gutter");
  let line = h.cx.debug_bounds("code-line-1").expect("line");
  // Four `ch` of line number, as React's `min-w-[4ch]` gutter.
  assert!(gutter.size.width >= gpui::px(30.), "{gutter:?}");
  assert_eq!(gutter.left(), line.left());
}

#[gpui::test]
fn file_browser_discard_review_keeps_on_escape_and_discards_on_enter(cx: &mut TestAppContext) {
  let (mut h, host) = harness(cx, serde_json::json!({}));
  comment_line_one(&mut h, "Drop me");
  h.click("discard-review");
  h.cx.simulate_keystrokes("escape");
  assert!(!h.drawn("discard-review-dialog"));
  assert!(h.drawn("file-browser-pending"));
  h.click("discard-review");
  h.cx.simulate_keystrokes("enter");
  h.cx.run_until_parked();
  assert!(!h.drawn("file-browser-pending"));
  assert!(host
    .calls()
    .contains(&"notify toast Review canceled".to_string()));
}
