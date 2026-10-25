//! The Changes tab: every changed file's diff in one list, grouped by file,
//! so a review can comment across files without leaving the tab.

use std::{collections::HashMap, rc::Rc};

use gpui_kit::{
  component::{button::Button, h_flex, v_flex, ActiveTheme, Sizable},
  *,
};
use treq_lib::{
  core::{changes::WorkspaceFileHunksBatch, remote::TreqCommandRequest, review::LineRef},
  jj::{JjDiffHunk, JjFileChange, JjRevisionDiff},
};

use super::{change_detail, WorkspaceScreen};
use crate::{
  remote,
  ui::{
    load, review,
    screens::diff::{self, Picked},
    widgets, Load,
  },
};

/// The uncommitted files' hunks, read in one batch per working copy.
#[derive(Default)]
pub(super) struct FileHunks {
  /// The diff's working-copy token the hunks were read for.
  token: Option<Option<String>>,
  files: HashMap<String, Load<Vec<JjDiffHunk>>>,
}

/// Which file the picked lines are in: its path, and whether it is the
/// branch's committed version (a file can be in both lists).
pub(super) type PickedIn = (String, bool);

/// The picked lines and the file they are in.
pub(super) type PickedLines = (PickedIn, Picked);

/// One-letter status, as the list shows it.
fn status_letter(status: &str) -> &'static str {
  match status {
    "added" | "A" => "A",
    "deleted" | "D" => "D",
    "renamed" | "R" => "R",
    "copied" | "C" => "C",
    _ => "M",
  }
}

impl WorkspaceScreen {
  /// Reads the uncommitted files' hunks once the diff names a new working
  /// copy. Files the host deferred as large wait until opened.
  pub(super) fn load_hunks(&mut self, cx: &mut Context<Self>) {
    let Some(diff) = self.diff.ready() else {
      return;
    };
    let token = Some(diff.working_copy_token.clone());
    if self.hunks.token == token && diff.working_copy_token.is_some() {
      return;
    }
    let paths: Vec<String> = diff
      .uncommitted_files
      .iter()
      .filter(|f| !f.diff_deferred || self.opened.contains(&f.path))
      .map(|f| f.path.clone())
      .collect();
    self.hunks.token = token;
    for path in &paths {
      self
        .hunks
        .files
        .entry(path.clone())
        .or_insert(Load::Loading);
    }
    if paths.is_empty() {
      return;
    }
    let request = TreqCommandRequest::DiffFiles {
      repo: self.target.repo.clone(),
      workspace: Some(self.target.id.clone()),
      paths,
      token: diff.working_copy_token.clone(),
    };
    load::run(
      cx,
      remote::read::<WorkspaceFileHunksBatch>(request),
      |this, result, _| match result {
        Ok(batch) => {
          for file in batch.files {
            this.hunks.files.insert(
              file.path,
              match file.error {
                Some(error) => Load::Failed(error),
                None => Load::Ready(file.hunks),
              },
            );
          }
        }
        Err(error) => {
          for load in this.hunks.files.values_mut() {
            if load.ready().is_none() {
              *load = Load::Failed(error.clone());
            }
          }
        }
      },
    );
  }

  /// Opens a large file the host held back.
  fn open_deferred(&mut self, path: String, cx: &mut Context<Self>) {
    self.opened.insert(path);
    self.hunks.token = None;
    self.load_hunks(cx);
    cx.notify();
  }

  fn tap_line(
    &mut self,
    at: PickedIn,
    hunks: &[JjDiffHunk],
    line: LineRef,
    cx: &mut Context<Self>,
  ) {
    let current = self
      .picked
      .as_ref()
      .filter(|(place, _)| *place == at)
      .map(|(_, picked)| picked);
    self.picked = diff::tap(current, hunks, line).map(|p| (at, p));
    cx.notify();
  }

  /// A changed file: its header with the file comment button, its cards,
  /// and its diff with tappable lines.
  pub(super) fn file_block(
    &self,
    change: &JjFileChange,
    committed: bool,
    diff_data: &JjRevisionDiff,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let theme = cx.theme();
    let path = change.path.clone();
    let committed_hunks = committed.then(|| {
      diff_data
        .hunks_by_file
        .iter()
        .find(|f| f.path == change.path)
        .map(|f| f.hunks.clone())
        .unwrap_or_default()
    });
    let uncommitted = (!committed).then(|| self.hunks.files.get(&path)).flatten();
    let hunks: Option<&[JjDiffHunk]> = match (&committed_hunks, uncommitted) {
      (Some(h), _) => Some(h),
      (None, Some(Load::Ready(h))) => Some(h),
      _ => None,
    };
    let count = self.review.read(cx).count_for(&path);
    let mut detail = change_detail(change);
    if count > 0 {
      detail.push_str(&format!(
        " · {}",
        treq_lib::core::review::count_label(count, "comment")
      ));
    }
    let letter = status_letter(&change.status);
    let letter_color = match letter {
      "A" => theme.success,
      "D" => theme.danger,
      _ => theme.warning,
    };
    let header = h_flex()
      .px_3()
      .py_2()
      .gap_3()
      .items_center()
      .border_b_1()
      .border_color(theme.border)
      .bg(theme.secondary)
      .child(
        div()
          .font_family(theme.mono_font_family.clone())
          .text_color(letter_color)
          .child(letter),
      )
      .child(
        v_flex()
          .flex_1()
          .min_w_0()
          .child(div().truncate().child(path.clone()))
          .child(widgets::muted(detail, cx).truncate()),
      )
      .child(review::file_comment_button(&self.review, &path));
    let place: PickedIn = (path.clone(), committed);
    let picked: Vec<LineRef> = self
      .picked
      .as_ref()
      .filter(|(at, _)| *at == place)
      .map(|(_, picked)| picked.lines.clone())
      .unwrap_or_default();
    let section = if committed {
      "committed"
    } else {
      "uncommitted"
    };
    let body = match (hunks, uncommitted) {
      (Some([]), _) => {
        widgets::centered_message("No diff to show (binary or empty).", cx).into_any_element()
      }
      (Some(h), _) => {
        let this = cx.weak_entity();
        let (tap_place, tap_hunks) = (place.clone(), h.to_vec());
        div()
          .id(SharedString::from(format!("hunks-{section}-{path}")))
          .overflow_x_scroll()
          .child(review::diff_hunks(
            &path,
            h,
            &picked,
            &self.review,
            Rc::new(move |at, _, cx| {
              this
                .update(cx, |this, cx| {
                  this.tap_line(tap_place.clone(), &tap_hunks, at, cx)
                })
                .ok();
            }),
            cx,
          ))
          .into_any_element()
      }
      (None, Some(Load::Failed(error))) => widgets::error_text(error.clone(), cx)
        .p_3()
        .into_any_element(),
      (None, None) if change.diff_deferred => h_flex()
        .p_3()
        .gap_3()
        .items_center()
        .child(
          div()
            .flex_1()
            .child(widgets::muted("Large diff, not loaded.", cx)),
        )
        .child(
          Button::new(SharedString::from(format!("show-{path}")))
            .outline()
            .small()
            .label("Show diff")
            .on_click(cx.listener(move |this, _, _, cx| this.open_deferred(path.clone(), cx))),
        )
        .into_any_element(),
      _ => widgets::centered_message("Loading diff…", cx).into_any_element(),
    };
    widgets::card(cx)
      .child(header)
      .children(review::file_cards(&self.review, &change.path, hunks, cx))
      .child(body)
      .into_any_element()
  }

  /// What sits under the Changes tab: the picked lines' actions, or the
  /// review.
  pub(super) fn changes_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let Some(((path, committed), picked)) = &self.picked else {
      return review::bar(&self.review, &self.sessions, cx);
    };
    let hunks = self.picked_hunks(path, *committed);
    let pending =
      hunks.and_then(|h| treq_lib::core::review::pending_for_selection(path, &h, &picked.lines));
    let this = cx.weak_entity();
    Some(review::selection_bar(
      &self.review,
      pending,
      picked.lines.len() > 1,
      Rc::new(move |_, cx| {
        this
          .update(cx, |this, cx| {
            this.picked = None;
            cx.notify();
          })
          .ok();
      }),
      cx,
    ))
  }

  /// The hunks the picked lines index into.
  fn picked_hunks(&self, path: &str, committed: bool) -> Option<Vec<JjDiffHunk>> {
    if !committed {
      return self.hunks.files.get(path)?.ready().cloned();
    }
    self
      .diff
      .ready()?
      .hunks_by_file
      .iter()
      .find(|f| f.path == path)
      .map(|f| f.hunks.clone())
  }
}
