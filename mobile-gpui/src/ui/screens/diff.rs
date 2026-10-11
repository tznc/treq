//! A changed file's diff hunks, and its working-copy content.

use gpui_kit::{
  component::{button::Button, v_flex, ActiveTheme, Sizable},
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

pub fn open(target: Target, path: String, cx: &mut App) {
  let title = path.rsplit('/').next().unwrap_or(&path).to_string();
  let location = crate::ui::Location {
    repo: Some(target.repo.clone()),
    workspace: Some(crate::store::SnapshotWorkspace {
      id: target.id.clone(),
      name: target.name.clone(),
    }),
  };
  nav::push(title, location, cx, move |_, cx| {
    cx.new(|cx| DiffScreen::new(target, path, cx))
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
  scroll: ScrollHandle,
}

impl DiffScreen {
  fn new(target: Target, path: String, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let mut this = Self {
      target,
      path,
      hunks: Load::Idle,
      content: Load::Idle,
      show_content: false,
      scroll: ScrollHandle::new(),
    };
    this.reload(cx);
    this
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    self.hunks = Load::Loading;
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
    self.content = Load::Loading;
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

impl Render for DiffScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let hunks = match &self.hunks {
      Load::Idle | Load::Loading => v_flex().child(widgets::centered_message("Loading diff…", cx)),
      Load::Failed(error) => v_flex().child(widgets::error_text(error.clone(), cx)),
      Load::Ready(hunks) if hunks.is_empty() => v_flex().child(widgets::centered_message(
        "No diff available for this file.",
        cx,
      )),
      Load::Ready(hunks) => v_flex()
        .gap_3()
        .children(hunks.iter().enumerate().map(|(i, hunk)| {
          widgets::card(cx).child(
            div().id(("hunk", i)).overflow_x_scroll().child(
              v_flex()
                .py_1()
                .child(
                  widgets::mono(hunk.header.clone(), cx)
                    .px_2()
                    .text_color(cx.theme().muted_foreground),
                )
                .children(hunk.lines.iter().map(|line| diff_line(line, cx))),
            ),
          )
        })),
    };
    let content = self.show_content.then(|| match &self.content {
      Load::Idle | Load::Loading => widgets::muted("Loading…", cx).into_any_element(),
      Load::Failed(error) => widgets::error_text(error.clone(), cx).into_any_element(),
      Load::Ready(file) => widgets::card(cx)
        .child(
          div()
            .id("content")
            .overflow_x_scroll()
            .child(
              v_flex()
                .py_1()
                .children(file.lines.iter().enumerate().map(|(i, line)| {
                  widgets::mono(format!("{:>5}  {line}", file.start_line + i), cx).px_2()
                })),
            ),
        )
        .into_any_element(),
    });
    div()
      .id("diff")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(widgets::muted(self.path.clone(), cx))
          .child(hunks)
          .child(
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
          )
          .children(content),
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
