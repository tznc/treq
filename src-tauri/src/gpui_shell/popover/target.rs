//! The target branch popover: `TargetBranchSelector`'s search over the
//! repository's branches (`w-[300px] p-0`, a cmdk list).

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, AnyElement, Context, KeyDownEvent, SharedString, Window,
};

use super::{Popover, SearchField};
use crate::gpui_shell::{
  overlay::cmdk,
  theme::{self, Palette},
  ui::{self, TextSize},
};

/// `w-[300px]`.
pub const WIDTH: f32 = 300.;

#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
  pub repo_path: String,
  /// The workspace's target branch, which gets the check mark.
  pub current: Option<String>,
  /// The branch names, read ahead while the pointer rested on the button:
  /// listed at once, and read again behind.
  pub branches: Option<Vec<String>>,
}

pub struct State {
  pub(super) spec: Spec,
  /// The branch names, `None` while they load.
  branches: Option<Vec<String>>,
  search: String,
  field: Option<SearchField>,
  /// The selected entry (cmdk's `value`), an index into `branches`.
  selected: Option<usize>,
  scroll: gpui::ScrollHandle,
  /// Where each entry's row sits among the list's children.
  rows: Vec<(usize, usize)>,
}

impl State {
  pub fn new(spec: Spec) -> Self {
    let mut this = Self {
      branches: spec.branches.clone(),
      spec,
      search: String::new(),
      field: None,
      selected: None,
      scroll: gpui::ScrollHandle::new(),
      rows: Vec::new(),
    };
    if this.branches.is_some() {
      this.select_first();
    }
    this
  }

  fn entries(&self) -> Vec<cmdk::Entry> {
    self
      .branches
      .iter()
      .flatten()
      .map(|name| cmdk::Entry {
        value: name.clone(),
        group: None,
        disabled: false,
      })
      .collect()
  }

  fn select_first(&mut self) {
    let entries = self.entries();
    self.selected = cmdk::first(&entries, &cmdk::visible(&entries, &self.search));
  }

  #[cfg(test)]
  pub fn selected(&self) -> Option<&str> {
    let at = self.selected?;
    self.branches.as_ref()?.get(at).map(String::as_str)
  }
}

impl Popover {
  /// `onLoadAvailableBranches`, as the popover opens.
  pub(super) fn load_target(&mut self, cx: &mut Context<Self>) {
    let super::Content::Target(state) = &self.content else {
      return;
    };
    let (host, repo) = (self.host.clone(), state.spec.repo_path.clone());
    self._load = Some(cx.spawn(async move |this, cx| {
      let read = cx
        .background_spawn(async move { host.branches(&repo) })
        .await;
      this
        .update(cx, |this, cx| {
          if let super::Content::Target(state) = &mut this.content {
            // A failed read lists nothing, as React's does.
            let names: Vec<String> = read
              .map_err(|err| log::warn!("target branches: {err}"))
              .unwrap_or_default()
              .into_iter()
              .map(|b| b.name)
              .collect();
            // The list read ahead, read again: the selection stays put.
            if state.branches.as_ref() != Some(&names) {
              state.branches = Some(names);
              state.select_first();
              cx.notify();
            }
          }
        })
        .ignore_closed();
    }));
  }

  fn set_search(&mut self, search: String, cx: &mut Context<Self>) {
    let super::Content::Target(state) = &mut self.content else {
      return;
    };
    if state.search != search {
      state.search = search;
      state.select_first();
      state.scroll.set_offset(gpui::point(px(0.), px(0.)));
      cx.notify();
    }
  }

  /// Moves the selection as cmdk does for a key, and scrolls it into view.
  pub(super) fn target_step(&mut self, step: cmdk::Move, cx: &mut Context<Self>) {
    let super::Content::Target(state) = &mut self.content else {
      return;
    };
    let entries = state.entries();
    let order = cmdk::visible(&entries, &state.search);
    let next = cmdk::moved(&entries, &order, state.selected, step);
    if next != state.selected {
      state.selected = next;
      if let Some(&(_, child)) = next.and_then(|n| state.rows.iter().find(|(e, _)| *e == n)) {
        state.scroll.scroll_to_item(child);
      }
      cx.notify();
    }
  }

  /// cmdk's root `onKeyDown`, after Escape.
  pub(super) fn target_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
    let key = event.keystroke.key.as_str();
    let m = crate::gpui_shell::keys::held(event.keystroke.modifiers);
    let step = match key {
      "down" if m.platform => Some(cmdk::Move::Last),
      "up" if m.platform => Some(cmdk::Move::First),
      "down" => Some(cmdk::Move::Step(1)),
      "up" => Some(cmdk::Move::Step(-1)),
      "n" | "j" if m.control => Some(cmdk::Move::Step(1)),
      "p" | "k" if m.control => Some(cmdk::Move::Step(-1)),
      "home" => Some(cmdk::Move::First),
      "end" => Some(cmdk::Move::Last),
      _ => None,
    };
    if let Some(step) = step {
      self.target_step(step, cx);
      cx.stop_propagation();
    } else if key == "enter" {
      let selected = match &self.content {
        super::Content::Target(state) => state.selected,
        _ => None,
      };
      if let Some(selected) = selected {
        self.pick_target(selected, cx);
      }
      cx.stop_propagation();
    }
  }

  /// `onSelect`: React retargets the workspace, and the popover closes.
  fn pick_target(&mut self, index: usize, cx: &mut Context<Self>) {
    let super::Content::Target(state) = &self.content else {
      return;
    };
    let Some(name) = state.branches.as_ref().and_then(|b| b.get(index)).cloned() else {
      return;
    };
    self.close(cx);
    self.field("workspace.target", name.into(), cx);
  }

  pub(super) fn render_target(
    &mut self,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let p = *p;
    if matches!(&self.content, super::Content::Target(s) if s.field.is_none()) {
      let field = SearchField::new("Search branches...", window, cx, |this, search, cx| {
        this.set_search(search, cx)
      });
      if let super::Content::Target(state) = &mut self.content {
        state.field = Some(field);
      }
    }
    let super::Content::Target(state) = &mut self.content else {
      return div().into_any_element();
    };
    let entries = state.entries();
    let order = cmdk::visible(&entries, &state.search);
    let mut children: Vec<AnyElement> = Vec::new();
    state.rows.clear();
    let message = |text: &'static str| {
      ui::text(div(), TextSize::Sm)
        .px_2()
        .py_4()
        .text_center()
        .text_color(p.muted_foreground)
        .child(text)
    };
    match &state.branches {
      None => children.push(
        ui::delayed(
          "target-loading",
          message("Loading branches...")
            .id("target-loading")
            .debug_selector(|| "target-loading".into())
            .flex()
            .flex_col()
            .items_center()
            .child(
              div()
                .mb_1()
                .child(ui::spinner("target-spin", 16., p.muted_foreground)),
            )
            .child("Loading branches..."),
        )
        .into_any_element(),
      ),
      Some(_) if order.is_empty() => children.push(
        message("No branches found")
          .id("target-empty")
          .debug_selector(|| "target-empty".into())
          .into_any_element(),
      ),
      Some(names) => {
        for &index in &order {
          let name = names[index].clone();
          let selected = state.selected == Some(index);
          let checked = state.spec.current.as_deref() == Some(name.as_str());
          let id = SharedString::from(format!("branch-option-{index}"));
          state.rows.push((index, children.len()));
          children.push(
            ui::Row::new(id, &p)
              .role(gpui::Role::ListBoxOption)
              .label(name.clone())
              .selected(selected)
              .px_3()
              .rounded_none()
              .font_family(theme::MONO_FONT)
              .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if let super::Content::Target(state) = &mut this.content {
                  if state.selected != Some(index) {
                    state.selected = Some(index);
                    cx.notify();
                  }
                }
              }))
              .on_click(cx.listener(move |this, _, _, cx| this.pick_target(index, cx)))
              .child(div().flex_1().min_w_0().truncate().child(name))
              .when(checked, |this| {
                this.trailing(ui::icon(theme::ICON_CHECK, 16., p.foreground))
              })
              .into_any_element(),
          );
        }
      }
    }
    let input = state.field.as_ref().map(SearchField::element);
    // `[cmdk-root]` is `bg-background`, over the popover's own fill.
    div()
      .id("target-branch-popover")
      .debug_selector(|| "target-branch-popover".into())
      .w(zpx(WIDTH))
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.background)
      .text_color(p.foreground)
      .shadow_md()
      .child(
        ui::text(div(), TextSize::Sm)
          .flex()
          .items_center()
          .h(zpx(40.))
          .px_3()
          .children(input.map(|input| input.flex_1())),
      )
      .child(
        div()
          .id("target-branch-list")
          .max_h(zpx(300.))
          .overflow_y_scroll()
          .track_scroll(&state.scroll)
          .children(children),
      )
      .into_any_element()
  }
}
