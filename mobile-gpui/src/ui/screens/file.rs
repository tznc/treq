//! One file of the workspace, read-only with line numbers. Tapping lines
//! selects them, and "Ask agent" starts an agent with the file (and the
//! selected lines) named in its first message.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::{
  core::remote::{FileRevision, TreqCommandRequest},
  jj::JjFileLines,
};

use super::{code, diff, terminal::Sessions, workspace::Target};
use crate::{
  errors, remote,
  store::SnapshotWorkspace,
  ui::{load, nav, widgets, Location},
};

/// Lines read per request; a full page means there may be more.
const PAGE: usize = 300;

/// Extensions shown as "binary" instead of read as text.
const BINARY: &[&str] = &[
  "png", "jpg", "jpeg", "gif", "webp", "ico", "bmp", "pdf", "zip", "gz", "tgz", "xz", "bz2", "7z",
  "tar", "jar", "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4", "mov", "wav", "ogg", "webm",
  "wasm", "so", "dylib", "dll", "exe", "bin", "class", "o", "a", "db", "sqlite",
];

pub fn is_binary(path: &str) -> bool {
  let name = code::split_path(path).0;
  name
    .rsplit_once('.')
    .is_some_and(|(_, ext)| BINARY.contains(&ext.to_ascii_lowercase().as_str()))
}

pub fn open(
  target: Target,
  path: String,
  status: Option<String>,
  sessions: WeakEntity<Sessions>,
  cx: &mut App,
) {
  let title = code::split_path(&path).0.to_string();
  let location = Location {
    repo: Some(target.repo.clone()),
    workspace: Some(SnapshotWorkspace {
      id: target.id.clone(),
      name: target.name.clone(),
    }),
  };
  nav::push(title, location, cx, move |_, cx| {
    cx.new(|cx| FileScreen::new(target, path, status, sessions, cx))
  });
}

/// The lines a tap leaves selected, given the selection before it.
/// One line selected and another tapped selects the range between them;
/// the same line tapped again clears it; a tap after a range starts over.
pub fn tap_line(selection: Option<(usize, usize)>, line: usize) -> Option<(usize, usize)> {
  match selection {
    Some((a, b)) if a == b && a == line => None,
    Some((a, b)) if a == b => Some((a.min(line), a.max(line))),
    _ => Some((line, line)),
  }
}

/// The first message an agent asked about `path` starts with.
pub fn agent_prompt(path: &str, selection: Option<(usize, usize)>) -> String {
  match selection {
    Some((a, b)) if a == b => format!("In {path} line {a}: "),
    Some((a, b)) => format!("In {path} lines {a}-{b}: "),
    None => format!("In {path}: "),
  }
}

pub struct FileScreen {
  target: Target,
  path: String,
  status: Option<String>,
  sessions: WeakEntity<Sessions>,
  lines: Vec<String>,
  /// The first line not read yet, when the last page came back full.
  more_from: Option<usize>,
  loading: bool,
  error: Option<String>,
  selection: Option<(usize, usize)>,
  scroll: ScrollHandle,
}

impl FileScreen {
  fn new(
    target: Target,
    path: String,
    status: Option<String>,
    sessions: WeakEntity<Sessions>,
    cx: &mut Context<Self>,
  ) -> Self {
    let binary = is_binary(&path);
    let mut this = Self {
      target,
      path,
      status,
      sessions,
      lines: Vec::new(),
      more_from: None,
      loading: false,
      error: None,
      selection: None,
      scroll: ScrollHandle::new(),
    };
    if !binary {
      this.read_from(1, cx);
    }
    this
  }

  fn read_from(&mut self, start: usize, cx: &mut Context<Self>) {
    if self.loading {
      return;
    }
    self.loading = true;
    self.error = None;
    let request = TreqCommandRequest::ReadFile {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
      path: self.path.clone(),
      revision: FileRevision::WorkingCopy,
      start_line: Some(start),
      end_line: Some(start + PAGE - 1),
    };
    load::run(
      cx,
      remote::read::<JjFileLines>(request),
      move |this, result, _| {
        this.loading = false;
        match result {
          Ok(page) => {
            let full = page.lines.len() >= PAGE;
            this.lines.extend(page.lines);
            this.more_from = full.then_some(start + PAGE);
          }
          Err(error) => this.error = Some(errors::describe(&error).message),
        }
      },
    );
  }

  fn ask_agent(&mut self, cx: &mut Context<Self>) {
    let prompt = agent_prompt(&self.path, self.selection);
    self
      .sessions
      .update(cx, |sessions, cx| sessions.new_agent_with(prompt, cx))
      .ok();
  }

  fn line_row(&self, index: usize, line: &str, cx: &mut Context<Self>) -> Stateful<Div> {
    let number = index + 1;
    let selected = self
      .selection
      .is_some_and(|(a, b)| (a..=b).contains(&number));
    let theme = cx.theme();
    h_flex()
      .id(("line", index))
      .px_2()
      .when(selected, |row| row.bg(theme.primary.opacity(0.15)))
      .child(
        widgets::mono(format!("{number:>5}  "), cx).text_color(if selected {
          theme.primary
        } else {
          theme.muted_foreground
        }),
      )
      .child(widgets::mono(
        if line.is_empty() {
          " ".to_string()
        } else {
          line.to_string()
        },
        cx,
      ))
      .on_click(cx.listener(move |this, _, _, cx| {
        this.selection = tap_line(this.selection, number);
        cx.notify();
      }))
  }

  fn body(&self, cx: &mut Context<Self>) -> AnyElement {
    if is_binary(&self.path) {
      return widgets::card(cx)
        .child(widgets::centered_message("Binary file, not shown.", cx))
        .into_any_element();
    }
    if self.lines.is_empty() {
      return widgets::card(cx)
        .p_4()
        .child(match (&self.error, self.loading) {
          (Some(error), _) => widgets::error_text(error.clone(), cx),
          (None, true) => widgets::centered_message("Loading…", cx),
          (None, false) => widgets::centered_message("This file is empty.", cx),
        })
        .into_any_element();
    }
    let rows: Vec<_> = self
      .lines
      .iter()
      .enumerate()
      .map(|(i, line)| self.line_row(i, line, cx))
      .collect();
    v_flex()
      .gap_3()
      .child(
        widgets::card(cx).child(
          div()
            .id("content")
            .overflow_x_scroll()
            .child(v_flex().py_1().min_w_full().children(rows)),
        ),
      )
      .when_some(self.error.clone(), |list, error| {
        list.child(widgets::error_text(error, cx))
      })
      .when_some(self.more_from, |list, from| {
        list.child(
          h_flex().child(
            Button::new("more-lines")
              .outline()
              .small()
              .label(if self.loading {
                "Loading…"
              } else {
                "Show more lines"
              })
              .loading(self.loading)
              .on_click(cx.listener(move |this, _, _, cx| this.read_from(from, cx))),
          ),
        )
      })
      .into_any_element()
  }
}

impl Render for FileScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let changed = self.status.as_deref().is_some_and(|s| s == "workingCopy");
    let ask_label = match self.selection {
      Some((a, b)) if a == b => format!("Ask agent about line {a}"),
      Some((a, b)) => format!("Ask agent about lines {a}–{b}"),
      None => "Ask agent about this file".into(),
    };
    let border = cx.theme().border;
    v_flex()
      .size_full()
      .child(
        div()
          .id("file")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .track_scroll(&self.scroll)
          .child(
            widgets::page()
              .gap_3()
              .child(
                v_flex()
                  .gap_1()
                  .child(widgets::muted(self.path.clone(), cx))
                  .when_some(
                    self.status.as_deref().and_then(code::status_label),
                    |head, label| head.child(widgets::muted(label, cx)),
                  )
                  .when(self.selection.is_none() && !self.lines.is_empty(), |head| {
                    head.child(widgets::muted(
                      "Tap a line to select it, then another to select the lines between.",
                      cx,
                    ))
                  }),
              )
              .child(self.body(cx)),
          ),
      )
      .child(
        h_flex()
          .p_3()
          .gap_2()
          .border_t_1()
          .border_color(border)
          .when(changed, |bar| {
            let (target, path) = (self.target.clone(), self.path.clone());
            bar.child(
              Button::new("view-changes")
                .outline()
                .large()
                .label("Changes")
                .on_click(move |_, _, cx| diff::open(target.clone(), path.clone(), cx)),
            )
          })
          .child(
            Button::new("ask-agent")
              .primary()
              .large()
              .flex_1()
              .icon(IconName::Bot)
              .label(ask_label)
              .on_click(cx.listener(|this, _, _, cx| this.ask_agent(cx))),
          ),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[core::prelude::v1::test]
  fn taps_select_a_line_then_a_range_then_start_over() {
    let one = tap_line(None, 7);
    assert_eq!(one, Some((7, 7)));
    assert_eq!(tap_line(one, 7), None);
    let range = tap_line(one, 3);
    assert_eq!(range, Some((3, 7)));
    assert_eq!(tap_line(range, 10), Some((10, 10)));
  }

  #[core::prelude::v1::test]
  fn the_prompt_names_the_file_and_lines() {
    assert_eq!(agent_prompt("src/a.rs", None), "In src/a.rs: ");
    assert_eq!(
      agent_prompt("src/a.rs", Some((4, 4))),
      "In src/a.rs line 4: "
    );
    assert_eq!(
      agent_prompt("src/a.rs", Some((4, 9))),
      "In src/a.rs lines 4-9: "
    );
  }

  #[core::prelude::v1::test]
  fn binary_files_go_by_extension() {
    assert!(is_binary("assets/logo.PNG"));
    assert!(!is_binary("src/main.rs"));
    assert!(!is_binary("Makefile"));
  }
}
