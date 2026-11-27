//! The Changes tab: every changed file's diff in one list, grouped by file,
//! so a review can comment across files without leaving the tab.

use crate::mobile::ui::widgets::TouchButton as _;
use std::{collections::HashMap, rc::Rc};

use crate::{
  core::{changes::WorkspaceFileHunksBatch, remote::TreqCommandRequest, review::LineRef},
  jj::{JjDiffHunk, JjFileChange, JjRevisionDiff},
};
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    v_flex, ActiveTheme, Disableable as _, Sizable,
  },
  div, AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
  StatefulInteractiveElement, Styled,
};

use super::{change_detail, WorkspaceScreen};
use crate::mobile::{
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
      |this, result, cx| match result {
        Ok(batch) => {
          let shown = this
            .diff
            .ready()
            .map(|diff| shown_files(diff, &batch))
            .unwrap_or_default();
          this.review.update(cx, |review, cx| review.show(shown, cx));
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

  /// Reads again the diffs that failed.
  fn retry_hunks(&mut self, cx: &mut Context<Self>) {
    self
      .hunks
      .files
      .retain(|_, load| !matches!(load, Load::Failed(_)));
    self.hunks.token = None;
    self.load_hunks(cx);
    cx.notify();
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
    let palette = treq_ui::theme::current(cx);
    let path = change.path.clone();
    // Borrowed from the diff, not cloned for every file on every render.
    let committed_hunks: Option<&[JjDiffHunk]> = committed.then(|| {
      diff_data
        .hunks_by_file
        .iter()
        .find(|f| f.path == change.path)
        .map_or(&[][..], |f| f.hunks.as_slice())
    });
    let uncommitted = (!committed).then(|| self.hunks.files.get(&path)).flatten();
    let hunks: Option<&[JjDiffHunk]> = match (&committed_hunks, uncommitted) {
      (Some(h), _) => Some(h),
      (None, Some(Load::Ready(h))) => Some(h.as_slice()),
      _ => None,
    };
    let count = self.review.read(cx).count_for(&path);
    let mut detail = change_detail(change);
    if count > 0 {
      detail.push_str(&format!(
        " · {}",
        crate::core::review::count_label(count, "comment")
      ));
    }
    let letter = status_letter(&change.status);
    let letter_color = match letter {
      "A" => palette.diff_added(),
      "D" => palette.diff_removed(),
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
        widgets::empty("diff-empty", "No diff to show (binary or empty).", cx).into_any_element()
      }
      (Some(h), _) => {
        let this = cx.weak_entity();
        let tap_place = place;
        div()
          .id(SharedString::from(format!("hunks-{section}-{path}")))
          .overflow_x_scroll()
          .child(review::diff_hunks(
            &path,
            h,
            &picked,
            &self.review,
            Rc::new(move |at, _, cx| {
              // Hunks are read on tap, not copied into every file's handler each render.
              this
                .update(cx, |this, cx| {
                  if let Some(hunks) = this.picked_hunks(&tap_place.0, tap_place.1) {
                    this.tap_line(tap_place.clone(), &hunks, at, cx);
                  }
                })
                .ok();
            }),
            cx,
          ))
          .into_any_element()
      }
      (None, Some(Load::Failed(error))) => widgets::failed(
        SharedString::from(format!("diff-failed-{section}-{path}")),
        error,
        cx.listener(|this, _, _, cx| this.retry_hunks(cx)),
        cx,
      )
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
            .on_click(cx.listener(move |this, _, _, cx| this.open_deferred(path.clone(), cx)))
            .touch(),
        )
        .into_any_element(),
      _ => widgets::skeleton(SharedString::from(format!("diff-{section}-{path}")), 3, cx),
    };
    widgets::card(cx)
      .child(header)
      .children(review::file_cards(&self.review, &change.path, hunks, cx))
      .child(body)
      .into_any_element()
  }

  /// What sits under the Changes tab: the picked lines' actions, the
  /// review, or the commit bar while there is something to commit.
  pub(super) fn changes_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let Some(((path, committed), picked)) = &self.picked else {
      return review::bar(&self.review, &self.sessions, cx).or_else(|| self.commit_bar(cx));
    };
    let hunks = self.picked_hunks(path, *committed);
    let pending =
      hunks.and_then(|h| crate::core::review::pending_for_selection(path, &h, &picked.lines));
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

  /// The commit message and Commit, under the uncommitted changes.
  fn commit_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let diff = self.diff.ready()?;
    if diff.uncommitted_files.is_empty() {
      return None;
    }
    let committing = self.busy == Some("commit");
    Some(
      widgets::bottom_bar(cx)
        .items_center()
        .child(div().flex_1().child(Input::new(&self.commit_message)))
        .child(
          Button::new("commit")
            .primary()
            .large()
            .label("Commit")
            .loading(committing)
            .disabled(self.busy.is_some())
            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx)))
            .touch(),
        )
        .into_any_element(),
    )
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

/// The files of `batch` with the content id `diff` lists for them, which
/// review comments follow; none when the batch was read at another diff.
pub(super) fn shown_files(
  diff: &JjRevisionDiff,
  batch: &WorkspaceFileHunksBatch,
) -> Vec<(String, Vec<JjDiffHunk>, String)> {
  if diff.working_copy_token.as_deref() != Some(batch.snapshot_token.as_str()) {
    return Vec::new();
  }
  batch
    .files
    .iter()
    .filter(|f| f.error.is_none())
    .filter_map(|f| {
      let listed = diff.uncommitted_files.iter().find(|c| c.path == f.path)?;
      Some((f.path.clone(), f.hunks.clone(), listed.content_id.clone()?))
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shown_files_carry_the_listed_content_ids_of_the_same_diff() {
    let diff: JjRevisionDiff = serde_json::from_value(serde_json::json!({
      "committed_files": [],
      "uncommitted_files": [
        {"path": "a.rs", "status": "M", "previous_path": null, "changed_line_count": 1,
         "diff_deferred": false, "content_id": "fbase>fv1:git"},
        {"path": "b.rs", "status": "M", "previous_path": null, "changed_line_count": 1,
         "diff_deferred": false}
      ],
      "hunks_by_file": [],
      "too_large_to_render": false,
      "working_copy_token": "t1"
    }))
    .unwrap();
    let batch = |token: &str| -> WorkspaceFileHunksBatch {
      serde_json::from_value(serde_json::json!({
        "snapshotToken": token,
        "files": [{"path": "a.rs", "hunks": []}, {"path": "b.rs", "hunks": []}]
      }))
      .unwrap()
    };
    let shown = shown_files(&diff, &batch("t1"));
    assert_eq!(shown.len(), 1);
    assert_eq!(
      (shown[0].0.as_str(), shown[0].2.as_str()),
      ("a.rs", "fbase>fv1:git")
    );
    assert!(shown_files(&diff, &batch("t2")).is_empty());
  }
}
