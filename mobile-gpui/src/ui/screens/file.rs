//! One file of the workspace, read-only with line numbers. Tapping lines
//! selects them for a comment; comments show under their last line and
//! batch into the Code tab's review, sent to an agent from the bar at the
//! bottom, as in desktop's file browser.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Sizable,
  },
  *,
};
use std::rc::Rc;
use treq_lib::{
  core::remote::{FileRevision, TreqCommandRequest},
  jj::JjFileLines,
};

use super::{code, terminal::Sessions, workspace::Target};
use crate::{
  errors, remote,
  store::SnapshotWorkspace,
  ui::{
    load, nav,
    review::{self, Handler, Review, FILE_LINES},
    widgets, Location,
  },
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

/// What the file viewer opens with.
pub struct Opened {
  pub target: Target,
  pub path: String,
  pub status: Option<String>,
  pub sessions: Entity<Sessions>,
  pub review: Entity<Review>,
  pub show_changes: Handler,
}

pub fn open(opened: Opened, cx: &mut App) {
  let title = code::split_path(&opened.path).0.to_string();
  let location = Location {
    repo: Some(opened.target.repo.clone()),
    workspace: Some(SnapshotWorkspace {
      id: opened.target.id.clone(),
      name: opened.target.name.clone(),
    }),
  };
  nav::push(title, location, cx, move |_, cx| {
    cx.new(|cx| FileScreen::new(opened, cx))
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

pub struct FileScreen {
  opened: Opened,
  lines: Vec<String>,
  /// The first line not read yet, when the last page came back full.
  more_from: Option<usize>,
  loading: bool,
  error: Option<String>,
  selection: Option<(usize, usize)>,
  scroll: ScrollHandle,
}

impl FileScreen {
  fn new(opened: Opened, cx: &mut Context<Self>) -> Self {
    cx.observe(&opened.review, |_, _, cx| cx.notify()).detach();
    let binary = is_binary(&opened.path);
    let mut this = Self {
      opened,
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

  fn path(&self) -> &str {
    &self.opened.path
  }

  fn read_from(&mut self, start: usize, cx: &mut Context<Self>) {
    if self.loading {
      return;
    }
    self.loading = true;
    self.error = None;
    let request = TreqCommandRequest::ReadFile {
      repo: self.opened.target.repo.clone(),
      workspace: Some(self.opened.target.id.clone()),
      path: self.opened.path.clone(),
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

  /// The comment box for the selected lines, with their text.
  fn pending(&self) -> Option<treq_lib::core::review::Pending> {
    let (a, b) = self.selection?;
    let lines = self.lines.get(a - 1..b.min(self.lines.len()))?.to_vec();
    Some(review::lines_pending(
      self.path(),
      a as u32,
      b as u32,
      lines,
    ))
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
      .when(selected, |row| row.bg(review::picked_bg(cx)))
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
    if is_binary(self.path()) {
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
    let review = self.opened.review.clone();
    // Comments on this file's lines, by the line they end on.
    let comments: Vec<_> = review
      .read(cx)
      .comments()
      .iter()
      .filter(|c| c.file_path == self.opened.path && c.hunk_id == FILE_LINES && !c.is_file())
      .cloned()
      .collect();
    let mut rows: Vec<AnyElement> = Vec::new();
    for (i, line) in self.lines.iter().enumerate() {
      rows.push(self.line_row(i, line, cx).into_any_element());
      for c in comments.iter().filter(|c| c.end_line as usize == i + 1) {
        rows.push(review::comment_card(&review, c, false, cx));
      }
    }
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

  fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let changed = self.opened.status.as_deref() == Some("workingCopy");
    let show_changes = self.opened.show_changes.clone();
    v_flex()
      .gap_1()
      .child(
        h_flex()
          .gap_2()
          .items_center()
          .child(
            widgets::muted(self.opened.path.clone(), cx)
              .flex_1()
              .min_w_0(),
          )
          .when(changed, |row| {
            row.child(
              Button::new("view-changes")
                .ghost()
                .small()
                .label("Changes")
                .on_click(move |_, window, cx| show_changes(window, cx)),
            )
          })
          .child(review::file_comment_button(
            &self.opened.review,
            self.path(),
          )),
      )
      .when_some(
        self.opened.status.as_deref().and_then(code::status_label),
        |head, label| head.child(widgets::muted(label, cx)),
      )
      // Kept while lines are picked, so the rows don't jump under the tap.
      .when(!self.lines.is_empty(), |head| {
        head.child(widgets::muted(
          "Tap a line to comment on it, or two lines for the range between.",
          cx,
        ))
      })
      .children(review::file_cards(
        &self.opened.review,
        self.path(),
        None,
        cx,
      ))
  }
}

impl Render for FileScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let footer = match self.selection {
      Some(_) => {
        let this = cx.weak_entity();
        let clear: Handler = Rc::new(move |_, cx| {
          this
            .update(cx, |screen, cx| {
              screen.selection = None;
              cx.notify();
            })
            .ok();
        });
        Some(review::selection_bar(
          &self.opened.review,
          self.pending(),
          false,
          clear,
          cx,
        ))
      }
      None => review::bar(&self.opened.review, &self.opened.sessions, cx),
    };
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
              .child(self.header(cx))
              .child(self.body(cx)),
          ),
      )
      .children(footer)
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
  fn binary_files_go_by_extension() {
    assert!(is_binary("assets/logo.PNG"));
    assert!(!is_binary("src/main.rs"));
    assert!(!is_binary("Makefile"));
  }
}
