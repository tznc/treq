//! A changed file's diff hunks, and its working-copy content.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{button::Button, h_flex, ActiveTheme, Sizable},
  *,
};
use treq_lib::{
  core::remote::{FileRevision, TreqCommandRequest},
  jj::{JjDiffHunk, JjFileLines},
};

use super::{on_epoch, workspace::Target};
use crate::{
  remote,
  ui::{load, nav, widgets, Load},
};

/// A working-copy file's diff, fetched from the host. A deleted file has
/// no working-copy content to show.
pub fn open(target: Target, path: String, deleted: bool, cx: &mut App) {
  push(target, path, None, !deleted, cx)
}

/// A file's diff on the workspace's branch, from hunks already fetched.
pub fn open_hunks(target: Target, path: String, hunks: Vec<JjDiffHunk>, cx: &mut App) {
  push(target, path, Some(hunks), false, cx)
}

fn push(
  target: Target,
  path: String,
  hunks: Option<Vec<JjDiffHunk>>,
  has_content: bool,
  cx: &mut App,
) {
  let title = path.rsplit('/').next().unwrap_or(&path).to_string();
  let location = crate::ui::Location {
    repo: Some(target.repo.clone()),
    workspace: Some(crate::store::SnapshotWorkspace {
      id: target.id.clone(),
      name: target.name.clone(),
    }),
  };
  nav::push(title, location, cx, move |_, cx| {
    cx.new(|cx| DiffScreen::new(target, path, hunks, has_content, cx))
  });
}

/// How a diff line is coloured.
#[derive(Debug, PartialEq)]
pub enum LineKind {
  Added,
  Removed,
  Context,
}

pub fn line_kind(line: &str) -> LineKind {
  if line.starts_with('+') && !line.starts_with("+++") {
    LineKind::Added
  } else if line.starts_with('-') && !line.starts_with("---") {
    LineKind::Removed
  } else {
    LineKind::Context
  }
}

pub struct DiffScreen {
  target: Target,
  path: String,
  hunks: Load<Vec<JjDiffHunk>>,
  content: Load<JjFileLines>,
  show_content: bool,
  /// Set when the hunks came with the page, which then never refetches.
  fixed: bool,
  /// Whether the file has working-copy content to show.
  has_content: bool,
  rows: ListState,
  /// What `rows` shows, as the last render flattened it.
  items: Vec<DiffItem>,
  /// Per hunk, and for the file content: the width of the widest line, so
  /// that every line of one scrolls sideways together.
  hunk_widths: Vec<f32>,
  content_width: f32,
  hunk_scroll: Vec<ScrollHandle>,
  content_scroll: ScrollHandle,
}

/// One item of the diff list. Hunks and the file content are cards drawn
/// as a run of items, so only the lines near the screen are built.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DiffItem {
  Path,
  /// Loading, failed or empty.
  Body,
  Header {
    hunk: usize,
    last: bool,
  },
  Line {
    hunk: usize,
    line: usize,
    last: bool,
  },
  Toggle,
  ContentMessage,
  ContentLine {
    line: usize,
    first: bool,
    last: bool,
  },
  End,
}

impl DiffScreen {
  fn new(
    target: Target,
    path: String,
    hunks: Option<Vec<JjDiffHunk>>,
    has_content: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let fixed = hunks.is_some();
    let mut this = Self {
      target,
      path,
      hunks: hunks.map_or(Load::Idle, Load::Ready),
      content: Load::Idle,
      show_content: false,
      fixed,
      has_content,
      rows: widgets::list_state(),
      items: Vec::new(),
      hunk_widths: Vec::new(),
      content_width: 0.,
      hunk_scroll: Vec::new(),
      content_scroll: ScrollHandle::new(),
    };
    this.reload(cx);
    this
  }

  /// A screen showing `hunks`, without the host reads (measurement tests).
  #[cfg(test)]
  pub(crate) fn for_test(hunks: Vec<JjDiffHunk>) -> Self {
    Self {
      target: Target {
        repo: "/r".into(),
        id: "1".into(),
        name: "w".into(),
      },
      path: "src/a.rs".into(),
      hunks: Load::Ready(hunks),
      content: Load::Idle,
      show_content: false,
      fixed: true,
      has_content: false,
      rows: widgets::list_state(),
      items: Vec::new(),
      hunk_widths: Vec::new(),
      content_width: 0.,
      hunk_scroll: Vec::new(),
      content_scroll: ScrollHandle::new(),
    }
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    if self.fixed {
      return;
    }
    // Hunks on screen stay up while they're read again.
    if self.hunks.ready().is_none() {
      self.hunks = Load::Loading;
    }
    let request = TreqCommandRequest::DiffFile {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
      path: self.path.clone(),
    };
    load::run(
      cx,
      remote::read::<Vec<JjDiffHunk>>(request),
      |this, result, _| this.hunks = Load::from_result(result),
    );
    if self.show_content {
      self.load_content(cx);
    }
  }

  fn load_content(&mut self, cx: &mut Context<Self>) {
    if self.content.ready().is_none() {
      self.content = Load::Loading;
    }
    let request = TreqCommandRequest::ReadFile {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
      path: self.path.clone(),
      revision: FileRevision::WorkingCopy,
      start_line: None,
      end_line: None,
    };
    load::run(
      cx,
      remote::read::<JjFileLines>(request),
      |this, result, _| this.content = Load::from_result(result),
    );
  }
}

fn diff_line(line: &str, cx: &App) -> Div {
  let theme = cx.theme();
  let (bg, fg) = match line_kind(line) {
    LineKind::Added => (theme.success.opacity(0.15), theme.success),
    LineKind::Removed => (theme.danger.opacity(0.15), theme.danger),
    LineKind::Context => (gpui_kit::transparent_black(), theme.foreground),
  };
  widgets::mono(
    if line.is_empty() {
      " ".to_string()
    } else {
      line.to_string()
    },
    cx,
  )
  .px_2()
  .bg(bg)
  .text_color(fg)
}

/// Horizontal padding of a diff line (`px_2` on each side).
const LINE_PAD: f32 = 16.;

/// How wide `chars` monospace characters are, with a character to spare.
fn text_width(chars: usize, advance: f32) -> f32 {
  (chars + 1) as f32 * advance + LINE_PAD
}

/// The advance of one monospace character.
fn mono_advance(window: &Window, cx: &App) -> f32 {
  let theme = cx.theme();
  let text = window.text_system();
  let font = text.resolve_font(&font(theme.mono_font_family.clone()));
  text
    .advance(font, theme.mono_font_size, 'm')
    .map_or(f32::from(theme.mono_font_size) * 0.6, |size| {
      f32::from(size.width)
    })
}

impl DiffScreen {
  /// Lays the hunks and content out as list items.
  fn flatten(&mut self, advance: f32) {
    let mut items = vec![DiffItem::Path];
    self.hunk_widths.clear();
    match &self.hunks {
      Load::Ready(hunks) if !hunks.is_empty() => {
        for (h, hunk) in hunks.iter().enumerate() {
          let chars = hunk
            .lines
            .iter()
            .map(|l| l.chars().count())
            .chain([hunk.header.chars().count()])
            .max()
            .unwrap_or(0);
          self.hunk_widths.push(text_width(chars, advance));
          items.push(DiffItem::Header {
            hunk: h,
            last: hunk.lines.is_empty(),
          });
          items.extend((0..hunk.lines.len()).map(|line| DiffItem::Line {
            hunk: h,
            line,
            last: line + 1 == hunk.lines.len(),
          }));
        }
      }
      _ => items.push(DiffItem::Body),
    }
    if self.has_content {
      items.push(DiffItem::Toggle);
    }
    if self.show_content {
      match &self.content {
        Load::Ready(file) => {
          let chars = file
            .lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
          self.content_width = text_width(chars + 7, advance);
          let n = file.lines.len();
          items.extend((0..n).map(|line| DiffItem::ContentLine {
            line,
            first: line == 0,
            last: line + 1 == n,
          }));
        }
        _ => items.push(DiffItem::ContentMessage),
      }
    }
    items.push(DiffItem::End);
    let hunks = self.hunk_widths.len();
    self.hunk_scroll.resize_with(hunks, ScrollHandle::new);
    self.items = items;
  }

  /// A line that scrolls sideways with the others sharing `scroll`.
  fn scrolling(
    id: usize,
    scroll: &ScrollHandle,
    width: f32,
    line: impl IntoElement,
  ) -> Stateful<Div> {
    div()
      .id(("diff-line", id))
      .overflow_x_scroll()
      .track_scroll(scroll)
      .child(div().min_w(px(width)).child(line))
  }

  fn item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
    let Some(item) = self.items.get(ix).copied() else {
      return div().into_any_element();
    };
    match item {
      DiffItem::Path => widgets::muted(self.path.clone(), cx)
        .px_4()
        .pt_4()
        .into_any_element(),
      DiffItem::Body => {
        let body = match &self.hunks {
          Load::Failed(error) => widgets::error_text(error.clone(), cx),
          Load::Ready(_) => widgets::centered_message("No diff available for this file.", cx),
          _ => widgets::centered_message("Loading diff…", cx),
        };
        body.mx_4().mt_5().into_any_element()
      }
      DiffItem::Header { hunk, last } => {
        let Load::Ready(hunks) = &self.hunks else {
          return div().into_any_element();
        };
        let header = widgets::mono(hunks[hunk].header.clone(), cx)
          .px_2()
          .text_color(cx.theme().muted_foreground);
        let row = Self::scrolling(ix, &self.hunk_scroll[hunk], self.hunk_widths[hunk], header);
        widgets::card_item(
          div().pt_1().when(last, |d| d.pb_1()).child(row),
          true,
          last,
          cx,
        )
        .pt(px(if hunk == 0 { 20. } else { 12. }))
        .into_any_element()
      }
      DiffItem::Line { hunk, line, last } => {
        let Load::Ready(hunks) = &self.hunks else {
          return div().into_any_element();
        };
        let row = Self::scrolling(
          ix,
          &self.hunk_scroll[hunk],
          self.hunk_widths[hunk],
          diff_line(&hunks[hunk].lines[line], cx),
        );
        widgets::card_item(div().when(last, |d| d.pb_1()).child(row), false, last, cx)
          .into_any_element()
      }
      DiffItem::Toggle => div()
        .px_4()
        .pt_5()
        .child(
          h_flex().child(
            Button::new("toggle-content")
              .outline()
              .small()
              .label(if self.show_content {
                "Hide working-copy content"
              } else {
                "Show working-copy content"
              })
              .on_click(cx.listener(|this, _, _, cx| {
                this.show_content = !this.show_content;
                if this.show_content && this.content.ready().is_none() {
                  this.load_content(cx);
                }
                cx.notify();
              })),
          ),
        )
        .into_any_element(),
      DiffItem::ContentMessage => match &self.content {
        Load::Failed(error) => widgets::error_text(error.clone(), cx),
        _ => widgets::muted("Loading…", cx),
      }
      .px_4()
      .pt_5()
      .into_any_element(),
      DiffItem::ContentLine { line, first, last } => {
        let Load::Ready(file) = &self.content else {
          return div().into_any_element();
        };
        let text = widgets::mono(
          format!("{:>5}  {}", file.start_line + line, file.lines[line]),
          cx,
        )
        .px_2();
        let row = Self::scrolling(ix, &self.content_scroll, self.content_width, text);
        let inner = div()
          .when(first, |d| d.pt_1())
          .when(last, |d| d.pb_1())
          .child(row);
        widgets::card_item(inner, first, last, cx)
          .when(first, |d| d.pt_5())
          .into_any_element()
      }
      DiffItem::End => div().h_4().into_any_element(),
    }
  }
}

impl Render for DiffScreen {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let advance = mono_advance(window, cx);
    self.flatten(advance);
    widgets::resize(&self.rows, self.items.len());
    let screen = cx.weak_entity();
    div().id("diff").size_full().child(
      list(self.rows.clone(), move |ix, _, cx| {
        screen
          .update(cx, |this, cx| this.item(ix, cx))
          .unwrap_or_else(|_| div().into_any_element())
      })
      .size_full(),
    )
  }
}

#[cfg(test)]
mod tests {
  use super::{line_kind, LineKind};

  #[test]
  fn colours_added_and_removed_lines_but_not_file_headers() {
    assert_eq!(line_kind("+added"), LineKind::Added);
    assert_eq!(line_kind("-removed"), LineKind::Removed);
    assert_eq!(line_kind("+++ b/file"), LineKind::Context);
    assert_eq!(line_kind("--- a/file"), LineKind::Context);
    assert_eq!(line_kind(" context"), LineKind::Context);
  }
}
