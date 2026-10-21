//! `github-panel/shared.tsx`: the chips, rows and states the GitHub view's
//! lists and details share.

use crate::gpui_shell::ui::zpx;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, rgb, App, Div, FontWeight, Hsla, SharedString, Stateful, Window,
};

use super::super::github_data::{self as data, CiState};
use crate::{
  github::{GhLabel, PrCheckEntry},
  gpui_shell::{
    theme::{self, tw, Palette},
    ui::{self, Button, Size, TextSize, Variant},
  },
};

/// Tailwind purple-500, behind the Merged chip.
const PURPLE_500: u32 = 0xa855f7;
/// Tailwind yellow-700, a running check.
const YELLOW_700: u32 = 0xa16207;

/// A click handler for a part.
pub type OnClick = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;

/// Sets `id` as `el`'s element id and debug selector.
pub fn named(el: Div, id: impl Into<SharedString>) -> Stateful<Div> {
  let id: SharedString = id.into();
  let selector = id.to_string();
  el.id(id).debug_selector(move || selector)
}

/// `StateChip`: Draft, Open, Merged or Closed.
pub fn state_chip(state: &str, draft: bool, p: &Palette) -> Div {
  let s = state.to_uppercase();
  let (icon, label, bg, fg) = if draft && s == "OPEN" {
    (theme::ICON_PR_DRAFT, "Draft", p.muted, p.muted_foreground)
  } else if s == "OPEN" {
    (
      theme::ICON_CIRCLE_DOT,
      "Open",
      tw::color(tw::GREEN_500).opacity(0.2),
      p.pr_open,
    )
  } else if s == "MERGED" {
    (
      theme::ICON_MERGE,
      "Merged",
      tw::color(PURPLE_500).opacity(0.2),
      p.pr_merged,
    )
  } else {
    (
      theme::ICON_CIRCLE_ALERT,
      "Closed",
      tw::color(tw::RED_500).opacity(0.2),
      p.pr_closed,
    )
  };
  ui::text(div(), TextSize::Sm)
    .flex()
    .flex_none()
    .items_center()
    .gap_1()
    .px_2()
    .py(zpx(2.))
    .rounded_full()
    .bg(bg)
    .text_color(fg)
    .child(ui::icon(icon, 12., fg))
    .child(label)
}

/// A label's `#rrggbb` (or bare `rrggbb`) color.
fn hex(color: &str) -> Hsla {
  u32::from_str_radix(color.trim_start_matches('#'), 16)
    .map(|v| rgb(v).into())
    .unwrap_or_else(|_| gpui::black())
}

/// `LabelChip`: the label's color at 20% behind, 40% around.
pub fn label_chip(label: &GhLabel) -> Div {
  let color = hex(&label.color);
  ui::text(div(), TextSize::Sm)
    .flex_none()
    .px(zpx(6.))
    .py(zpx(2.))
    .rounded(zpx(4.))
    .font_weight(FontWeight::MEDIUM)
    .bg(color.opacity(0.2))
    .text_color(color)
    .border_1()
    .border_color(color.opacity(0.4))
    .child(label.name.clone())
}

/// The labels under a title: `flex gap-1 flex-wrap mt-2`.
pub fn labels(labels: &[GhLabel]) -> Option<Div> {
  (!labels.is_empty()).then(|| {
    div()
      .flex()
      .flex_wrap()
      .gap_1()
      .mt_2()
      .children(labels.iter().map(label_chip))
  })
}

/// `ciStatusStyle(state).textClassName`, and its icon.
pub fn ci_style(state: CiState, p: &Palette) -> (&'static str, Hsla) {
  match state {
    CiState::Success => (
      theme::ICON_CIRCLE_CHECK,
      tw::color(if p.dark { tw::GREEN_400 } else { tw::GREEN_700 }),
    ),
    CiState::Failure => (
      theme::ICON_CIRCLE_X,
      tw::color(if p.dark { tw::RED_400 } else { tw::RED_700 }),
    ),
    CiState::Pending => (
      theme::ICON_SPINNER,
      tw::color(if p.dark { tw::YELLOW_400 } else { YELLOW_700 }),
    ),
  }
}

/// A status icon that spins while pending.
pub fn ci_icon(
  id: impl Into<SharedString>,
  state: CiState,
  size: f32,
  p: &Palette,
) -> gpui::AnyElement {
  let (icon, color) = ci_style(state, p);
  let icon = ui::icon(icon, size, color);
  if state == CiState::Pending {
    ui::spin(gpui::ElementId::Name(id.into()), icon).into_any_element()
  } else {
    icon.into_any_element()
  }
}

/// `CheckEntryRow`, opening its run on a click when it has a link.
pub fn check_row(
  id: impl Into<SharedString>,
  check: &PrCheckEntry,
  p: &Palette,
  on_open: Option<OnClick>,
) -> Stateful<Div> {
  let id: SharedString = id.into();
  let state = CiState::of_bucket(&check.bucket);
  let row = ui::text(div(), TextSize::Sm)
    .flex()
    .items_center()
    .gap_2()
    .py(zpx(6.))
    .px_2()
    .child(ci_icon(format!("{id}-spin"), state, 16., p))
    .child(
      div()
        .flex_1()
        .min_w_0()
        .truncate()
        .child(check.name.clone()),
    )
    .children(check.duration_secs.map(|secs| {
      ui::text(div(), TextSize::Sm)
        .flex_none()
        .text_color(p.muted_foreground)
        .child(data::check_duration(secs))
    }));
  let muted = p.muted;
  match on_open.filter(|_| !check.link.is_empty()) {
    Some(open) => named(div(), id)
      .w_full()
      .rounded(zpx(4.))
      .cursor_pointer()
      .hover(move |s| s.bg(muted.opacity(0.5)))
      .on_click(open)
      .child(row),
    None => named(div(), id).child(row),
  }
}

/// `text-base font-semibold uppercase tracking-widest text-muted-foreground`.
pub fn section_heading(
  label: impl Into<SharedString>,
  icon: Option<&'static str>,
  p: &Palette,
) -> Div {
  let label: SharedString = label.into();
  ui::text(div(), TextSize::Sm)
    .flex()
    .items_center()
    .gap_1()
    .font_weight(FontWeight::SEMIBOLD)
    .text_color(p.muted_foreground)
    .children(icon.map(|icon| ui::icon(icon, 12., p.muted_foreground)))
    // `tracking-widest` is 0.1em: GPUI has no letter spacing, so the
    // capitals stand in.
    .child(label.to_uppercase())
}

/// `EmptyState`.
pub fn empty_state(icon: &'static str, message: &str, p: &Palette) -> Div {
  div()
    .flex()
    .flex_col()
    .items_center()
    .justify_center()
    .py_12()
    .px_6()
    .gap_2()
    .text_center()
    .child(ui::icon(icon, 24., p.muted_foreground))
    .child(
      ui::text(div(), TextSize::Sm)
        .text_color(p.muted_foreground)
        .child(message.to_string()),
    )
}

/// `ErrorState`: the gh message, so auth or network trouble shows.
pub fn error_state(
  id: &str,
  title: &str,
  error: &str,
  p: &Palette,
  on_retry: Option<OnClick>,
) -> Stateful<Div> {
  named(div(), format!("{id}-error"))
    .flex()
    .flex_col()
    .items_center()
    .justify_center()
    .py_12()
    .px_6()
    .gap_2()
    .text_center()
    .child(ui::icon(theme::ICON_CIRCLE_ALERT, 24., p.destructive))
    .child(
      ui::text(div(), TextSize::Sm)
        .font_weight(FontWeight::MEDIUM)
        .child(title.to_string()),
    )
    .child(
      ui::text(div(), TextSize::Sm)
        .max_w_full()
        .text_color(p.muted_foreground)
        .child(error.to_string()),
    )
    .children(on_retry.map(|retry| {
      Button::new(SharedString::from(format!("{id}-retry")), p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .mt_1()
        .label("Try again")
        .on_click(retry)
    }))
}

/// A centered `Loader2` (`w-5 h-5`).
pub fn loading(id: &'static str, p: &Palette) -> Div {
  div()
    .flex()
    .items_center()
    .justify_center()
    .py_12()
    .child(ui::spinner(id, 20., p.muted_foreground))
}

/// `LoadMoreButton`.
pub fn load_more(id: &'static str, loading: bool, p: &Palette, on_click: OnClick) -> Div {
  div().p_3().child(
    Button::new(id, p)
      .variant(Variant::Outline)
      .w_full()
      .disabled(loading)
      .when(loading, |b| {
        b.child(ui::spinner(
          SharedString::from(format!("{id}-spin")),
          16.,
          p.foreground,
        ))
      })
      .label("Load more")
      .on_click(on_click),
  )
}

/// `OpenInWebButton`.
pub fn open_in_web(id: &'static str, p: &Palette, on_click: OnClick) -> Button {
  Button::new(id, p)
    .variant(Variant::Outline)
    .size(Size::Sm)
    .gap(zpx(6.))
    .icon(theme::ICON_EXTERNAL_LINK, 16.)
    .label("Open in Web")
    .on_click(on_click)
}

/// A small button's spinner (`w-3 h-3 mr-1 animate-spin`) while `busy`.
pub fn busy_spinner(button: Button, id: &str, busy: bool, color: Hsla) -> Button {
  button.when(busy, |b| {
    b.child(ui::spinner(
      SharedString::from(format!("{id}-spin")),
      12.,
      color,
    ))
  })
}

/// One comment: `bg-muted/30 rounded-md p-3`, its author and date over
/// its body.
pub fn comment(author: &str, date: &str, body: gpui::AnyElement, p: &Palette) -> Div {
  ui::text(div(), TextSize::Sm)
    .bg(p.muted.opacity(0.3))
    .rounded(zpx(6.))
    .p_3()
    .child(
      div()
        .flex()
        .items_center()
        .gap_1()
        .mb_1()
        .text_color(p.muted_foreground)
        .child(
          div()
            .font_weight(FontWeight::MEDIUM)
            .child(author.to_string()),
        )
        .child("·")
        .child(date.to_string()),
    )
    .child(body)
}

/// A dropdown menu's panel: `rounded-md border bg-popover p-1 shadow-md`.
pub fn menu_panel(id: &'static str, p: &Palette) -> Stateful<Div> {
  named(div(), id)
    .p_1()
    .rounded(zpx(8.))
    .border_1()
    .border_color(p.border)
    .bg(p.popover)
    .text_color(p.popover_foreground)
    .shadow_md()
    .occlude()
}

/// A plain `DropdownMenuItem`.
pub fn menu_item(
  id: impl Into<SharedString>,
  label: &str,
  p: &Palette,
  on_click: OnClick,
) -> Stateful<Div> {
  let accent = p.menu_accent;
  ui::text(named(div(), id), TextSize::Sm)
    .px_2()
    .py(zpx(6.))
    .rounded(zpx(4.))
    .cursor_pointer()
    .hover(move |s| s.bg(accent))
    .on_click(on_click)
    .child(label.to_string())
}
