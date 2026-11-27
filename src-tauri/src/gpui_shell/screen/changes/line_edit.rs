//! E edits the selected diff lines in place. An editor stands in for the
//! picked lines the file still has (context and added lines, never removed
//! ones), and saving writes them straight into the checkout's file, refused
//! when the file no longer reads as the diff showed.

use super::review::LineEdit;
use super::*;
use crate::core::review::editable_lines;
use crate::gpui_shell::native::Toast;
use crate::gpui_shell::screen::commits_data::{selection, LineRef};
use gpui::AnyElement;
use gpui_kit::component::input::Textarea;

impl ChangesView {
  /// E: opens the editor on the selected lines, or on the lines the
  /// selected text spans; `false` when nothing there can be edited.
  pub(super) fn start_line_edit(&mut self, cx: &mut Context<Self>) -> bool {
    if self.read_only() {
      return false;
    }
    let picked = match (&self.review.selection, &self.review.text) {
      (Some(s), _) => Some((s.path.clone(), s.lines.clone(), s.head.0)),
      (None, Some(t)) => self.file_hunks(&t.path).map(|d| {
        (
          t.path.clone(),
          selection(&d.hunks, t.anchor.0, t.head.0),
          t.head.0 .0,
        )
      }),
      (None, None) => None,
    };
    let Some((path, rows, head)) = picked else {
      return false;
    };
    let Some(lines) = self
      .file_hunks(&path)
      .and_then(|d| editable_lines(&d.hunks, &rows, head))
    else {
      return false;
    };
    self.review.line_edit_text = lines.original.join("\n");
    self.review.line_edit = Some(LineEdit { path, lines });
    self.review.line_edit_input = None;
    cx.notify();
    true
  }

  /// Cmd+Enter or Save: writes the lines, unless they are unchanged.
  fn save_line_edit(&mut self, cx: &mut Context<Self>) {
    let Some(edit) = self.review.line_edit.clone() else {
      return;
    };
    let text = self.review.line_edit_text.clone();
    if text == edit.lines.original.join("\n") {
      self.close_line_edit(cx);
      return;
    }
    let workspace = self.parent.working_directory();
    self.run(
      move |host, _| {
        host.apply_suggestion(
          &workspace,
          &edit.path,
          edit.lines.lines,
          &edit.lines.original,
          &text,
        )
      },
      |view, result, cx| match result {
        Ok(()) => {
          view.close_line_edit(cx);
          // This view's own write: shown at once, not behind the stale banner.
          view.review.apply_next = true;
          view.load(cx);
        }
        Err(err) => super::tell(
          view.host.as_ref(),
          Toast::error("Could not save the lines")
            .description(err)
            .into(),
        ),
      },
      cx,
    );
  }

  /// Escape or Cancel; the diff takes the keyboard back.
  fn close_line_edit(&mut self, cx: &mut Context<Self>) {
    self.review.line_edit = None;
    self.review.line_edit_input = None;
    self.review.line_edit_text.clear();
    self.review.refocus = true;
    cx.notify();
  }

  /// What a diff row the editor covers draws: the editor on its first row
  /// and nothing on the others. `None` for a row it does not cover.
  pub(super) fn line_edit_row(
    &self,
    path: &str,
    at: LineRef,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let edit = self.review.line_edit.as_ref().filter(|e| e.path == path)?;
    if edit.lines.rows.first() == Some(&at) {
      Some(self.line_editor(edit, p, cx).into_any_element())
    } else if edit.lines.rows.contains(&at) {
      Some(div().into_any_element())
    } else {
      None
    }
  }

  fn line_editor(&self, edit: &LineEdit, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let (start, end) = edit.lines.lines;
    let border = p.border.opacity(0.4);
    div()
      .selector("line-edit")
      .role(gpui::Role::Group)
      .aria_label(format!("Editing {} lines {start} to {end}", edit.path))
      .flex()
      .bg(p.muted)
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .capture_key_down(cx.listener(|view, e: &KeyDownEvent, _, cx| {
        let k = &e.keystroke;
        let held = crate::gpui_shell::keys::held(k.modifiers);
        if ui::is_escape(k) {
          cx.stop_propagation();
          view.close_line_edit(cx);
        } else if k.key == "enter" && (held.platform || held.control) {
          cx.stop_propagation();
          view.save_line_edit(cx);
        }
      }))
      // Lines up with the code: the gutter, comment and marker columns.
      .child(div().w_24().flex_none().border_r_1().border_color(border))
      .child(div().w_6().flex_none())
      .child(div().w_5().flex_none())
      .child(
        div()
          .flex_1()
          .min_w_0()
          .flex()
          .flex_col()
          .gap_1()
          .py_1()
          .pr_2()
          .children(self.review.line_edit_input.as_ref().map(|i| {
            div()
              .rounded(radius::SM)
              .border_1()
              .border_color(p.primary)
              .bg(p.background)
              .font_family(theme::MONO_FONT)
              .child(ui::text(
                Textarea::new(&i.state).aria_label("Edit lines"),
                TextSize::Code,
              ))
          }))
          .child(
            div()
              .flex()
              .justify_end()
              .gap_2()
              .font_family(theme::SANS_FONT)
              .child(
                Button::new("line-edit-cancel", p)
                  .variant(Variant::Ghost)
                  .size(Size::Sm)
                  .label("Cancel")
                  .on_click(cx.listener(|view, _, _, cx| view.close_line_edit(cx))),
              )
              .child(
                Button::new("line-edit-save", p)
                  .size(Size::Sm)
                  .label("Save")
                  .on_click(cx.listener(|view, _, _, cx| view.save_line_edit(cx))),
              ),
          ),
      )
  }
}
