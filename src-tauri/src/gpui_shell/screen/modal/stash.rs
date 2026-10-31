//! `StashModal`: the repository's stashed changes, each one's diff, and
//! applying, copying and deleting them.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, deferred, div, prelude::*, ClipboardItem, Context, Entity, FontWeight, SharedString,
  Subscription, Window,
};

use super::{message, Env};
use crate::{
  gpui_shell::{
    native::ToastTone,
    screen::{code::menu_panel, rows::Rows},
    theme::{self, Palette},
    time,
    ui::{self, Button, Size, TextSize, Variant},
  },
  jj::JjRevisionDiff,
  local_db::StashEntry,
};

/// Target branch value that applies onto the home repo working copy.
pub const HOME_APPLY_TARGET: &str = ".";

/// One place a stash applies to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Target {
  pub branch: String,
  pub label: String,
}

/// What React sends: the repository and the workspaces to apply to, most
/// recently active first.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub repo_path: String,
  pub workspaces: Vec<Target>,
}

/// A row of a stash's diff, by file and hunk index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
  File(usize),
  HunkHeader(usize, usize),
  Line(usize, usize, usize),
  /// The space under a hunk.
  HunkEnd(usize, usize),
}

struct LoadedDiff {
  id: i64,
  /// `None` when it failed to load, which React shows as nothing.
  diff: Option<JjRevisionDiff>,
}

pub struct StashView {
  env: Env,
  route: Route,
  dark: bool,
  /// `None` until the first read returns.
  pub(in crate::gpui_shell::screen) entries: Option<Vec<StashEntry>>,
  pub(in crate::gpui_shell::screen) selected: Option<i64>,
  diff: Option<LoadedDiff>,
  /// The diff's rows.
  rows: Rows<Row>,
  /// `patch-<id>`, `delete-<id>` or `apply-<id>` while one runs.
  pub(in crate::gpui_shell::screen) pending: Option<String>,
  pub(in crate::gpui_shell::screen) more_open: bool,
  /// The apply picker and its search box.
  pub(in crate::gpui_shell::screen) apply: Option<Entity<InputState>>,
  apply_query: String,
  apply_index: usize,
  _apply_subscription: Option<Subscription>,
  focus: gpui::FocusHandle,
  focused: bool,
}

/// `getLinePrefix`.
fn line_prefix(line: &str) -> &'static str {
  if line.starts_with('+') {
    "+"
  } else if line.starts_with('-') {
    "-"
  } else {
    " "
  }
}

/// A diff line without its `+`, `-` or space marker.
fn line_text(line: &str) -> &str {
  line.strip_prefix(['+', '-', ' ']).unwrap_or(line)
}

/// `files_changed.length` as the list writes it.
fn files_label(count: usize) -> String {
  format!("{count} {}", if count == 1 { "file" } else { "files" })
}

impl StashView {
  pub fn new(env: Env, route: Route, dark: bool, cx: &mut Context<Self>) -> Self {
    let mut this = Self {
      env,
      route,
      dark,
      entries: None,
      selected: None,
      diff: None,
      rows: Rows::default(),
      pending: None,
      more_open: false,
      apply: None,
      apply_query: String::new(),
      apply_index: 0,
      _apply_subscription: None,
      focus: cx.focus_handle(),
      focused: false,
    };
    this.load(cx);
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route == route && self.dark == dark {
      return;
    }
    let moved = self.route.repo_path != route.repo_path;
    self.route = route;
    self.dark = dark;
    if moved {
      self.entries = None;
      self.selected = None;
      self.diff = None;
      self.load(cx);
    }
    cx.notify();
  }

  /// Reads the stashes, and selects the most recent unless one is picked.
  fn load(&mut self, cx: &mut Context<Self>) {
    let (host, repo) = (self.env.host.clone(), self.route.repo_path.clone());
    let key = repo.clone();
    cx.spawn(async move |this, cx| {
      let entries = cx
        .background_spawn(async move { host.stashes(&repo) })
        .await
        .unwrap_or_else(|err| {
          log::warn!("Failed to list stashes: {err}");
          Vec::new()
        });
      this
        .update(cx, |view, cx| {
          if view.route.repo_path != key {
            return;
          }
          let selected_exists = entries.iter().any(|e| Some(e.id) == view.selected);
          if view.selected.is_none() || !selected_exists {
            view.selected = entries.first().map(|e| e.id);
          }
          view.entries = Some(entries);
          view.load_diff(cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Reads the selected stash's diff, unless it is the one already read.
  fn load_diff(&mut self, cx: &mut Context<Self>) {
    let Some(id) = self.selected else {
      self.diff = None;
      return;
    };
    if self.diff.as_ref().is_some_and(|d| d.id == id) {
      return;
    }
    self.diff = None;
    let (host, repo) = (self.env.host.clone(), self.route.repo_path.clone());
    cx.spawn(async move |this, cx| {
      let diff = cx
        .background_spawn(async move { host.stash_diff(&repo, id) })
        .await
        .map_err(|err| log::warn!("Failed to load the stash diff: {err}"))
        .ok();
      this
        .update(cx, |view, cx| {
          if view.selected == Some(id) {
            view.rows = Rows::default();
            view.diff = Some(LoadedDiff { id, diff });
            cx.notify();
          }
        })
        .ignore_closed();
    })
    .detach();
  }

  pub(in crate::gpui_shell::screen) fn select(&mut self, id: i64, cx: &mut Context<Self>) {
    self.selected = Some(id);
    self.close_menus(cx);
    self.load_diff(cx);
    cx.notify();
  }

  fn close(&self, cx: &mut Context<Self>) {
    self.env.press("stash.close", cx);
  }

  fn close_menus(&mut self, cx: &mut Context<Self>) {
    self.more_open = false;
    self.close_apply(cx);
  }

  /// `handleApplyOpenChange(false)`.
  fn close_apply(&mut self, cx: &mut Context<Self>) {
    if self.apply.take().is_some() {
      self.apply_query.clear();
      self.apply_index = 0;
      self._apply_subscription = None;
      cx.notify();
    }
  }

  fn selected_entry(&self) -> Option<StashEntry> {
    self
      .entries
      .as_ref()?
      .iter()
      .find(|e| Some(e.id) == self.selected)
      .cloned()
  }

  /// Home first, then the workspaces: `applyTargets`.
  pub(in crate::gpui_shell::screen) fn targets(&self) -> Vec<Target> {
    std::iter::once(Target {
      branch: HOME_APPLY_TARGET.into(),
      label: "Home repo".into(),
    })
    .chain(self.route.workspaces.iter().cloned())
    .collect()
  }

  /// `filteredApplyTargets`.
  fn filtered_targets(&self) -> Vec<Target> {
    let query = self.apply_query.trim().to_lowercase();
    self
      .targets()
      .into_iter()
      .filter(|t| {
        query.is_empty()
          || t.label.to_lowercase().contains(&query)
          || t.branch.to_lowercase().contains(&query)
      })
      .collect()
  }

  fn open_apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.more_open = false;
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search workspaces…"));
    input.update(cx, |input, cx| input.focus(window, cx));
    self._apply_subscription = Some(cx.subscribe_in(
      &input,
      window,
      |view, input, event: &InputEvent, window, cx| match event {
        InputEvent::Change => {
          view.apply_query = input.read(cx).value().to_string();
          view.apply_index = 0;
          cx.notify();
        }
        InputEvent::PressEnter { .. } => view.apply_selected(window, cx),
        _ => {}
      },
    ));
    self.apply = Some(input);
    cx.notify();
  }

  /// cmdk's Enter: the highlighted row.
  fn apply_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let filtered = self.filtered_targets();
    let Some(entry) = self.selected_entry() else {
      return;
    };
    match filtered.get(self.apply_index) {
      Some(target) => self.apply_to(&entry, target.branch.clone(), cx),
      None => self.apply_to_new(&entry, window, cx),
    }
  }

  fn step_apply(&mut self, delta: isize, cx: &mut Context<Self>) {
    // The filtered rows, then "New workspace…".
    let len = self.filtered_targets().len() as isize + 1;
    self.apply_index = (self.apply_index as isize + delta).rem_euclid(len) as usize;
    cx.notify();
  }

  fn toggle_more(&mut self, cx: &mut Context<Self>) {
    self.close_apply(cx);
    self.more_open = !self.more_open;
    cx.notify();
  }

  /// `handleCopyPatch`.
  fn copy_patch(&mut self, entry: &StashEntry, cx: &mut Context<Self>) {
    self.more_open = false;
    self.pending = Some(format!("patch-{}", entry.id));
    cx.notify();
    let (host, repo, id) = (
      self.env.host.clone(),
      self.route.repo_path.clone(),
      entry.id,
    );
    cx.spawn(async move |this, cx| {
      let patch = cx
        .background_spawn(async move { host.stash_patch(&repo, id) })
        .await;
      this
        .update(cx, |view, cx| {
          view.pending = None;
          match patch {
            Ok(patch) => {
              cx.write_to_clipboard(ClipboardItem::new_string(patch));
              view.env.toast(ToastTone::Success, "Patch copied", "");
            }
            Err(err) => view
              .env
              .toast(ToastTone::Error, "Failed to copy patch", err),
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// `handleDelete`.
  fn delete(&mut self, entry: &StashEntry, cx: &mut Context<Self>) {
    self.more_open = false;
    self.pending = Some(format!("delete-{}", entry.id));
    cx.notify();
    let (host, repo) = (self.env.host.clone(), self.route.repo_path.clone());
    let entry = entry.clone();
    cx.spawn(async move |this, cx| {
      let id = entry.id;
      let result = cx
        .background_spawn({
          let repo = repo.clone();
          async move { host.delete_stash(&repo, id) }
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.pending = None;
          match result {
            Ok(()) => {
              if view.selected == Some(id) {
                view.selected = None;
                view.diff = None;
              }
              view.env.workspace_changed(&repo);
              view.load(cx);
              view.env.toast(ToastTone::Success, "Stash deleted", "");
            }
            Err(err) => view
              .env
              .toast(ToastTone::Error, "Failed to delete stash", err),
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// `handleApply`.
  fn apply_to(&mut self, entry: &StashEntry, target: String, cx: &mut Context<Self>) {
    self.pending = Some(format!("apply-{}", entry.id));
    self.close_apply(cx);
    cx.notify();
    let (host, repo, id) = (
      self.env.host.clone(),
      self.route.repo_path.clone(),
      entry.id,
    );
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn({
          let repo = repo.clone();
          async move { host.apply_stash(&repo, id, &target) }
        })
        .await;
      this
        .update(cx, |view, cx| {
          view.pending = None;
          match result {
            Ok(()) => {
              view.env.workspace_changed(&repo);
              view.env.toast(ToastTone::Success, "Stash applied", "");
            }
            Err(err) => view
              .env
              .toast(ToastTone::Error, "Failed to apply stash", err),
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// `handleApplyToNew`: React opens the create-workspace dialog.
  fn apply_to_new(&mut self, entry: &StashEntry, _: &mut Window, cx: &mut Context<Self>) {
    self.close_apply(cx);
    self.env.field(
      "stash.applyToNew",
      serde_json::to_value(entry).unwrap_or_default(),
      cx,
    );
  }
}

impl StashView {
  fn list(&self, entries: &[StashEntry], p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
    let muted = p.muted_foreground;
    div()
      .debug_selector(|| "stash-list".into())
      .w(zpx(288.))
      .flex_none()
      .h_full()
      .child(
        div()
          .id("stash-list-scroll")
          .size_full()
          .overflow_y_scroll()
          .flex()
          .flex_col()
          .gap_1()
          .pr_2()
          .border_r_1()
          .border_color(p.border)
          .children(entries.iter().enumerate().map(|(i, entry)| {
            let id = entry.id;
            let selected = self.selected == Some(id);
            let selector = format!("stash-list-item-{i}");
            ui::text(
              ui::Row::new(SharedString::from(selector), p)
                .density(ui::RowDensity::TwoLine)
                .label(entry.workspace_label.clone())
                .selected(selected),
              TextSize::Xs,
            )
            .w_full()
            .flex_none()
            .py_2()
            .rounded(radius::MD)
            .items_start()
            .child(
              div()
                .font_family(theme::MONO_FONT)
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(entry.workspace_label.clone()),
            )
            .child(
              div()
                .mt_0p5()
                .flex()
                .items_center()
                .gap_2()
                .text_color(muted)
                .child(
                  div()
                    .font_family(theme::MONO_FONT)
                    .child(entry.short_commit_id.clone()),
                )
                .child(time::locale(&entry.created_at)),
            )
            .child(
              div()
                .mt_1()
                .flex()
                .items_center()
                .gap_2()
                .child(
                  div()
                    .text_color(ui::Tone::GitOpen.text(p))
                    .child(format!("+{}", entry.additions)),
                )
                .child(
                  div()
                    .text_color(ui::Tone::GitClosed.text(p))
                    .child(format!("-{}", entry.deletions)),
                )
                .child(
                  div()
                    .text_color(muted)
                    .child(files_label(entry.files_changed.len())),
                ),
            )
            .on_click(cx.listener(move |view, _, _, cx| view.select(id, cx)))
          })),
      )
  }

  fn detail(&mut self, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
    let pane = div()
      .debug_selector(|| "stash-detail".into())
      .flex_1()
      .min_w_0()
      .h_full()
      .flex()
      .flex_col();
    let Some(entry) = self.selected_entry() else {
      return pane.child(message(
        "stash-select",
        "Select a stash to view its changes.",
        p,
      ));
    };
    let busy = self.pending.is_some();
    let applying = self
      .pending
      .as_deref()
      .is_some_and(|a| a.starts_with("apply-"));
    let other = self
      .pending
      .as_deref()
      .is_some_and(|a| !a.starts_with("apply-"));
    let green = ui::Tone::GitOpen.text(p);
    let red = ui::Tone::GitClosed.text(p);

    let info = ui::text(div(), TextSize::Xs)
      .min_w_0()
      .flex()
      .flex_col()
      .gap_1()
      .child(
        ui::text(div(), TextSize::Sm)
          .font_family(theme::MONO_FONT)
          .font_weight(FontWeight::MEDIUM)
          .truncate()
          .child(entry.workspace_label.clone()),
      )
      .child(
        div()
          .flex()
          .flex_wrap()
          .gap_x_3()
          .gap_y_0p5()
          .text_color(p.muted_foreground)
          .child(
            div()
              .font_family(theme::MONO_FONT)
              .child(entry.short_commit_id.clone()),
          )
          .child(time::locale(&entry.created_at))
          .child(
            div()
              .flex()
              .gap_1()
              .child(
                div()
                  .text_color(green)
                  .child(format!("+{}", entry.additions)),
              )
              .child(div().text_color(red).child(format!("-{}", entry.deletions))),
          )
          .child(format!("{} files", entry.files_changed.len())),
      );

    let apply = Button::new("stash-apply-button", p)
      .aria_label("Apply…")
      .size(Size::Sm)
      .h(zpx(28.))
      .px_2()
      .disabled(busy)
      .child(if applying {
        ui::spinner("stash-applying", 14., p.primary_foreground).into_any_element()
      } else {
        ui::icon(theme::ICON_UPLOAD, 14., p.primary_foreground).into_any_element()
      })
      .child(div().ml_1().child("Apply…"))
      .child(
        ui::icon(theme::ICON_CHEVRONS_UP_DOWN, 12., p.primary_foreground)
          .ml_1()
          .opacity(0.7),
      )
      .on_click(cx.listener(|view, _, window, cx| {
        if view.apply.is_some() {
          view.close_apply(cx);
        } else {
          view.open_apply(window, cx);
        }
      }));
    let more = Button::new("stash-more-menu", p)
      .aria_label("More stash actions")
      .variant(Variant::Outline)
      .size(Size::Sm)
      .h(zpx(28.))
      .px_2()
      .disabled(busy)
      .child(if other {
        ui::spinner("stash-busy", 14., p.foreground).into_any_element()
      } else {
        ui::icon(theme::ICON_MORE, 14., p.foreground).into_any_element()
      })
      .on_click(cx.listener(|view, _, _, cx| view.toggle_more(cx)));

    let controls = div()
      .flex_none()
      .flex()
      .items_center()
      .gap_1p5()
      .child(
        div()
          .relative()
          .child(apply)
          .when(self.apply.is_some(), |this| {
            this.child(self.apply_popover(&entry, p, cx))
          }),
      )
      .child(div().relative().child(more).when(self.more_open, |this| {
        this.child(self.more_menu(&entry, p, cx))
      }));

    let header = div()
      .flex_none()
      .flex()
      .items_start()
      .justify_between()
      .gap_2()
      .pb_2()
      .border_b_1()
      .border_color(p.border)
      .bg(p.background)
      .child(info)
      .child(controls);

    let body = match &self.diff {
      Some(LoadedDiff { id, diff }) if *id == entry.id => match diff {
        Some(diff) => self.diff_pane(diff, p, cx),
        None => div().into_any_element(),
      },
      _ => ui::delayed(
        "stash-diff-loading",
        message("stash-diff-loading", "Loading diff…", p),
      )
      .into_any_element(),
    };
    pane.gap_3().child(header).child(
      // The diff's list scrolls itself.
      div().flex_1().min_h_0().child(body),
    )
  }

  fn more_menu(&self, entry: &StashEntry, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let (patch, delete) = (entry.clone(), entry.clone());
    let panel = menu_panel("stash-more-popup", p)
      .top(zpx(32.))
      .right_0()
      .min_w(zpx(128.))
      .child(
        ui::MenuItem::new("stash-copy-patch", "Copy as git patch", p)
          .icon(theme::ICON_COPY)
          .on_click(cx.listener(move |view, _, _, cx| view.copy_patch(&patch, cx))),
      )
      .child(div().h_px().my_1().mx_neg_1().bg(p.border))
      .child(
        ui::MenuItem::new("stash-delete", "Delete stash", p)
          .icon(theme::ICON_TRASH)
          .danger()
          .on_click(cx.listener(move |view, _, _, cx| view.delete(&delete, cx))),
      )
      .on_mouse_down_out(cx.listener(|view, _, _, cx| {
        view.more_open = false;
        cx.notify();
      }));
    deferred(panel).with_priority(3)
  }

  fn apply_popover(
    &self,
    entry: &StashEntry,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let Some(input) = self.apply.clone() else {
      return div().into_any_element();
    };
    let filtered = self.filtered_targets();
    let selected = self.apply_index;
    let row = |id: SharedString, label: SharedString, active: bool| {
      ui::text(
        ui::Row::new(id, p)
          .role(gpui::Role::ListBoxOption)
          .label(label)
          .selected(active),
        TextSize::Xs,
      )
      .rounded(radius::MD)
    };
    let list = if filtered.is_empty() {
      ui::text(div(), TextSize::Xs)
        .debug_selector(|| "stash-apply-empty".into())
        .p_3()
        .text_center()
        .text_color(p.muted_foreground)
        .child("No workspaces found")
        .into_any_element()
    } else {
      div()
        .child(
          ui::text(div(), TextSize::Xs)
            .px_2()
            .py_1p5()
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.muted_foreground)
            .child("Apply to"),
        )
        .children(filtered.iter().enumerate().map(|(i, target)| {
          let (entry, branch) = (entry.clone(), target.branch.clone());
          row(
            SharedString::from(format!("stash-apply-option-{}", target.branch)),
            SharedString::from(target.label.clone()),
            i == selected,
          )
          .child(div().truncate().child(target.label.clone()))
          .on_click(cx.listener(move |view, _, _, cx| view.apply_to(&entry, branch.clone(), cx)))
        }))
        .into_any_element()
    };
    let new_entry = entry.clone();
    let new_active = selected == filtered.len();
    let panel = div()
      .id("stash-apply-popover")
      .debug_selector(|| "stash-apply-popover".into())
      .absolute()
      .top(zpx(32.))
      .right_0()
      .w(zpx(288.))
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_md()
      .occlude()
      .capture_action(cx.listener(|view, _: &MoveDown, _, cx| {
        view.step_apply(1, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|view, _: &MoveUp, _, cx| {
        view.step_apply(-1, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|view, _: &Escape, _, cx| {
        view.close_apply(cx);
        cx.stop_propagation();
      }))
      .on_mouse_down_out(cx.listener(|view, _, _, cx| view.close_apply(cx)))
      .child(
        ui::text(div(), TextSize::Xs)
          .flex()
          .items_center()
          .h(zpx(36.))
          .px_3()
          .border_b_1()
          .border_color(p.border)
          .child(
            div()
              .debug_selector(|| "stash-apply-workspace-select".into())
              .w_full()
              .child(ui::input(&input).appearance(false).bordered(false).flex_1()),
          ),
      )
      .child(
        div()
          .id("stash-apply-list")
          .max_h(zpx(224.))
          .overflow_y_scroll()
          .p_1()
          .child(list)
          .child(
            div()
              .mt_1()
              .pt_1()
              .border_t_1()
              .border_color(p.border)
              .child(
                row(
                  SharedString::from("stash-apply-new-workspace"),
                  SharedString::from("New workspace…"),
                  new_active,
                )
                .child(ui::icon(theme::ICON_PLUS, 14., p.popover_foreground))
                .child("New workspace…")
                .on_click(
                  cx.listener(move |view, _, window, cx| view.apply_to_new(&new_entry, window, cx)),
                ),
              ),
          ),
      );
    deferred(panel).with_priority(3).into_any_element()
  }
}

impl StashView {
  /// The rows of the selected stash's diff.
  fn sync_rows(&mut self, window: &Window) {
    let mut rows = Vec::new();
    if let Some(diff) = self
      .diff
      .as_ref()
      .and_then(|d| d.diff.as_ref())
      .filter(|d| !d.too_large_to_render)
    {
      for (f, file) in diff.hunks_by_file.iter().enumerate() {
        rows.push(Row::File(f));
        for (h, hunk) in file.hunks.iter().enumerate() {
          rows.push(Row::HunkHeader(f, h));
          rows.extend((0..hunk.lines.len()).map(|l| Row::Line(f, h, l)));
          rows.push(Row::HunkEnd(f, h));
        }
      }
    }
    self.rows.sync(window, rows, |a, b| a == b);
  }

  /// `StashDiffPane`.
  fn diff_pane(
    &self,
    diff: &JjRevisionDiff,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> gpui::AnyElement {
    if diff.too_large_to_render {
      return message(
        "stash-diff-too-large",
        diff
          .render_block_reason
          .clone()
          .unwrap_or_else(|| "Diff is too large to render.".into()),
        p,
      )
      .into_any_element();
    }
    if diff.hunks_by_file.is_empty() {
      return message("stash-diff-empty", "No file changes in this stash.", p).into_any_element();
    }
    div()
      .debug_selector(|| "stash-diff-pane".into())
      .size_full()
      .child(
        gpui::list(
          self.rows.list.clone(),
          cx.processor(|view, ix, _, _| view.row(ix)),
        )
        .size_full(),
      )
      .into_any_element()
  }

  fn row(&self, ix: usize) -> gpui::AnyElement {
    let p = theme::palette(self.dark);
    let (Some(&row), Some(diff)) = (
      self.rows.get(ix),
      self.diff.as_ref().and_then(|d| d.diff.as_ref()),
    ) else {
      return div().into_any_element();
    };
    let files = &diff.hunks_by_file;
    let hunk = |f: usize, h: usize| files.get(f).and_then(|file| file.hunks.get(h));
    let text = ui::text(div(), TextSize::Xs).font_family(theme::MONO_FONT);
    match row {
      Row::File(f) => text
        .debug_selector(move || format!("stash-diff-file-{f}"))
        .when(f > 0, |this| this.pt_4())
        .child(
          div()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(p.border)
            .bg(p.background.opacity(0.95))
            .font_weight(FontWeight::MEDIUM)
            .child(files.get(f).map(|f| f.path.clone()).unwrap_or_default()),
        )
        .into_any_element(),
      Row::HunkHeader(f, h) => text
        .px_2()
        .py_0p5()
        .text_color(p.muted_foreground)
        .bg(p.muted.opacity(0.4))
        .child(hunk(f, h).map(|h| h.header.clone()).unwrap_or_default())
        .into_any_element(),
      Row::Line(f, h, l) => {
        let line = hunk(f, h)
          .and_then(|h| h.lines.get(l))
          .map_or("", String::as_str);
        text
          .w_full()
          .px_2()
          .flex()
          .whitespace_nowrap()
          .overflow_hidden()
          .when(line.starts_with('+'), |this| {
            this.bg(ui::Tone::GitOpen.tint(&p))
          })
          .when(line.starts_with('-'), |this| {
            this.bg(ui::Tone::GitClosed.tint(&p))
          })
          .child(
            div()
              .mr_1()
              .text_color(p.muted_foreground)
              .child(line_prefix(line)),
          )
          .child(line_text(line).to_string())
          .into_any_element()
      }
      Row::HunkEnd(..) => div().h_2().into_any_element(),
    }
  }
}

impl Render for StashView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    if !self.focused {
      self.focused = true;
      window.focus(&self.focus, cx);
    }
    self.sync_rows(window);
    let viewport = window.viewport_size();
    // With no stashes there is only a sentence to show: size the dialog to
    // it (`sm:max-w-lg`) instead of the list-and-detail layout's 75vw × 80vh.
    let empty = self
      .entries
      .as_ref()
      .is_some_and(|entries| entries.is_empty());
    let header = ui::text(div(), TextSize::Lg)
      .font_weight(FontWeight::SEMIBOLD)
      .flex()
      .items_center()
      .gap_2()
      .child(ui::icon(theme::ICON_ARCHIVE, 16., p.popover_foreground))
      .child("Stashed changes");
    let entries = self.entries.clone();
    let body = match &entries {
      None => ui::delayed(
        "stash-loading",
        message("stash-loading", "Loading stashes…", &p),
      )
      .into_any_element(),
      Some(entries) if entries.is_empty() => message(
        "stash-empty",
        "No stashed changes yet. Stash working-copy changes from the Changes panel to save them here.",
        &p,
      )
      .into_any_element(),
      Some(entries) => div()
        .flex_1()
        .min_h_0()
        .flex()
        .gap_4()
        .overflow_hidden()
        .child(self.list(entries, &p, cx))
        .child(self.detail(&p, cx))
        .into_any_element(),
    };
    // Escape closes an open menu first.
    let close = ui::handler(cx, |view, _, cx| {
      if view.apply.is_some() || view.more_open {
        view.close_menus(cx);
      } else {
        view.close(cx);
      }
    });
    ui::Dialog::new("stash-modal", &p)
      .overlay(viewport)
      .map(|d| {
        if empty {
          d.width(512.)
        } else {
          d.relative_size(0.75, Some(0.8))
        }
      })
      .title("Stashed changes")
      .header(header)
      .focus(&self.focus)
      .on_close(close)
      .child(body)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn strips_the_diff_marker() {
    assert_eq!(line_text("+added"), "added");
    assert_eq!(line_text("-removed"), "removed");
    assert_eq!(line_text(" context"), "context");
    assert_eq!(line_text("\\ No newline"), "\\ No newline");
    assert_eq!(line_prefix("+x"), "+");
    assert_eq!(line_prefix("-x"), "-");
    assert_eq!(line_prefix("x"), " ");
  }

  #[test]
  fn counts_files_as_the_list_does() {
    assert_eq!(files_label(1), "1 file");
    assert_eq!(files_label(3), "3 files");
  }
}
