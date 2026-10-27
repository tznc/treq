//! `github-panel/shared.tsx`: the chips, rows and states the GitHub view's
//! lists and details share.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, App, Div, FontWeight, Hsla, SharedString, Stateful, Window,
};

use super::super::github_data::{self as data, CiState};
use crate::{
  github::{GhLabel, GhPullRequest, PrCheckEntry},
  gpui_shell::{
    theme::{self, Palette},
    ui::{self, Button, Size, TextSize, Tone, Variant},
  },
};

/// A click handler for a part.
pub type OnClick = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;

/// Sets `id` as `el`'s element id and debug selector.
pub fn named(el: Div, id: impl Into<SharedString>) -> Stateful<Div> {
  let id: SharedString = id.into();
  let selector = id.to_string();
  el.id(id).debug_selector(move || selector)
}

/// `StateChip`: Draft, Open, Merged or Closed.
pub fn state_chip(state: &str, draft: bool, p: &Palette) -> impl IntoElement {
  let s = state.to_uppercase();
  let (label, tone) = if draft && s == "OPEN" {
    ("Draft", Tone::Neutral)
  } else if s == "OPEN" {
    ("Open", Tone::GitOpen)
  } else if s == "MERGED" {
    ("Merged", Tone::GitMerged)
  } else {
    ("Closed", Tone::GitClosed)
  };
  ui::Badge::new(label, p).tone(tone)
}

/// Where `pr`'s review stands, when it stands anywhere.
pub fn review_chip(pr: &crate::github::GhPullRequest, p: &Palette) -> Option<impl IntoElement> {
  use crate::github::ReviewState;
  let state = crate::core::pr_view::review_state(pr)?;
  let tone = match state {
    ReviewState::Approved => Tone::Success,
    ReviewState::ChangesRequested => Tone::Danger,
    ReviewState::ReviewRequired => Tone::Warning,
  };
  Some(
    div()
      .debug_selector(|| "github-review-state".into())
      .child(ui::Badge::new(state.label(), p).tone(tone)),
  )
}

/// A label's `#rrggbb` (or bare `rrggbb`) color.
fn hex(color: &str) -> Hsla {
  u32::from_str_radix(color.trim_start_matches('#'), 16)
    .map(theme::data_color)
    .unwrap_or_else(|_| theme::data_color(0))
}

/// `LabelChip`: the label's color at 20% behind, 40% around.
pub fn label_chip(label: &GhLabel) -> Div {
  let color = hex(&label.color);
  ui::text(div(), TextSize::Sm)
    .flex_none()
    .px_1p5()
    .py_0p5()
    .rounded(radius::SM)
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
    CiState::Success => (theme::ICON_CIRCLE_CHECK, Tone::Success.text(p)),
    CiState::Failure => (theme::ICON_CIRCLE_X, Tone::Danger.text(p)),
    CiState::Pending => (theme::ICON_SPINNER, Tone::Warning.text(p)),
    CiState::Inactive => (theme::ICON_MINUS, p.muted_foreground),
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
) -> ui::Row {
  let id: SharedString = id.into();
  let state = CiState::of_bucket(&check.bucket);
  let row = ui::Row::new(id.clone(), p)
    .label(check.name.clone())
    .w_full()
    .py_1p5()
    .rounded(radius::SM)
    .leading(ci_icon(format!("{id}-spin"), state, 16., p))
    .title(check.name.clone())
    .when_some(check.duration_secs, |row, secs| {
      row.trailing(
        ui::text(div(), TextSize::Sm)
          .flex_none()
          .text_color(p.muted_foreground)
          .child(data::check_duration(secs)),
      )
    });
  match on_open.filter(|_| !check.link.is_empty()) {
    Some(open) => row.on_click(open),
    None => row.interactive(false),
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
    .child({
      let error = error.to_string();
      div()
        .flex()
        .flex_wrap()
        .justify_center()
        .gap_2()
        .mt_1()
        .children(on_retry.map(|retry| {
          Button::new(SharedString::from(format!("{id}-retry")), p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .label("Try again")
            .on_click(retry)
        }))
        .child(
          Button::new(SharedString::from(format!("{id}-copy-error")), p)
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .icon(theme::ICON_COPY, 12.)
            .label("Copy error")
            .on_click(move |_, _, cx| {
              cx.write_to_clipboard(gpui::ClipboardItem::new_string(error.clone()))
            }),
        )
    })
}

/// `OpenInWebButton`.
pub fn open_in_web(id: &'static str, p: &Palette, on_click: OnClick) -> Button {
  Button::new(id, p)
    .variant(Variant::Outline)
    .size(Size::Sm)
    .gap_1p5()
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
    .rounded(radius::MD)
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

/// A reviewer's verdict in the timeline: their avatar (the initial until it
/// loads), their login and what they did.
pub fn review_event(
  review: &crate::github::GhReview,
  date: &str,
  image: Option<std::sync::Arc<gpui::Image>>,
  p: &Palette,
) -> Div {
  let login = review.author.login.clone();
  let approved = review.state == "APPROVED";
  let (icon, tone) = if approved {
    (theme::ICON_CIRCLE_CHECK, Tone::Success)
  } else {
    (theme::ICON_CIRCLE_X, Tone::Danger)
  };
  let avatar: gpui::AnyElement = match image {
    Some(image) => gpui::img(image)
      .size(zpx(20.))
      .flex_none()
      .rounded_full()
      .into_any_element(),
    None => ui::text(div(), TextSize::Xs)
      .size(zpx(20.))
      .flex_none()
      .rounded_full()
      .bg(p.muted)
      .flex()
      .items_center()
      .justify_center()
      .font_weight(FontWeight::MEDIUM)
      .text_color(p.muted_foreground)
      .child(
        review
          .author
          .login
          .chars()
          .next()
          .map(|c| c.to_uppercase().to_string())
          .unwrap_or_default(),
      )
      .into_any_element(),
  };
  ui::text(div(), TextSize::Sm)
    .debug_selector(move || format!("github-review-event-{login}"))
    .flex()
    .items_center()
    .gap_2()
    .px_3()
    .py_1()
    .text_color(p.muted_foreground)
    .child(ui::icon(icon, 14., tone.color(p)).flex_none())
    .child(avatar)
    .child(
      div()
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.foreground)
        .child(review.author.login.clone()),
    )
    .child(review.verdict().unwrap_or_default())
    .child("·")
    .child(date.to_string())
}

/// A dropdown menu's panel: `rounded-md border bg-popover p-1 shadow-md`.
pub fn menu_panel(id: &'static str, p: &Palette) -> Stateful<Div> {
  named(div(), id)
    .p_1()
    .rounded(radius::MD)
    .border_1()
    .border_color(p.border)
    .bg(p.popover)
    .text_color(p.popover_foreground)
    .shadow_md()
    .occlude()
}

/// A list row's title and number.
pub fn row_title(title: &str, number: u64, p: &Palette) -> Div {
  div()
    .flex()
    .items_center()
    .gap_2()
    .min_w_0()
    .child(
      ui::text(div(), TextSize::Lg)
        .font_weight(FontWeight::MEDIUM)
        .min_w_0()
        .truncate()
        .child(title.to_string()),
    )
    .child(
      ui::text(div(), TextSize::Sm)
        .flex_none()
        .text_color(p.muted_foreground)
        .child(format!("#{number}")),
    )
}

/// A list row's second line: `flex items-center gap-2 mt-1 text-base`.
pub fn row_meta(p: &Palette) -> Div {
  ui::text(div(), TextSize::Sm)
    .flex()
    .items_center()
    .gap_2()
    .mt_1()
    .min_w_0()
    .text_color(p.muted_foreground)
}

/// `PrListItem`'s text: the title and number, then the state, author,
/// branches (when `branches`) and creation date.
pub fn pr_list_item(pr: &GhPullRequest, branches: bool, p: &Palette) -> Div {
  div()
    .flex_1()
    .min_w_0()
    .child(row_title(&pr.title, pr.number, p))
    .child(
      row_meta(p)
        .child(state_chip(&pr.state, pr.is_draft, p))
        .child(
          div()
            .max_w(zpx(160.))
            .truncate()
            .child(pr.author.login.clone()),
        )
        .when(branches, |d| {
          d.child(
            div()
              .flex()
              .items_center()
              .gap_1()
              .min_w_0()
              .font_family(theme::MONO_FONT)
              .child(
                div()
                  .max_w(zpx(256.))
                  .truncate()
                  .child(pr.head_ref_name.clone()),
              )
              .child(div().flex_none().child("→"))
              .child(
                div()
                  .max_w(zpx(256.))
                  .truncate()
                  .child(pr.base_ref_name.clone()),
              ),
          )
        })
        .child(
          div()
            .flex_none()
            .child(crate::gpui_shell::time::date(&pr.created_at)),
        ),
    )
}

/// A comment thread's fold: a full-width outline button under the same
/// gap as the cards, which shows the comments it hides.
pub fn fold_row(
  id: &'static str,
  hidden: usize,
  p: &Palette,
  on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
  Button::new(id, p)
    .variant(Variant::Outline)
    .size(Size::Sm)
    .w_full()
    .gap_1()
    .icon(theme::ICON_CHEVRON_DOWN, 14.)
    .label(format!("Show {hidden} earlier comments"))
    .on_click(on_click)
    .into_any_element()
}
