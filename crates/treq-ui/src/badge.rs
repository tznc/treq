//! Badges, counts and status dots. Color means state: a count is always
//! neutral, and a problem shows once, as a dot.

use gpui::{div, prelude::*, App, Hsla, SharedString, Window};
use gpui_kit::base::TestSupportExt as _;

use crate::{text, theme::Palette, zpx, TextSize};

/// What a [`Badge`] or [`Dot`] says about its object.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tone {
  #[default]
  Neutral,
  Primary,
  Success,
  Warning,
  Danger,
  /// An open pull request, or added lines.
  GitOpen,
  GitMerged,
  /// A closed pull request, or removed lines.
  GitClosed,
}

impl Tone {
  pub fn color(self, p: &Palette) -> Hsla {
    match self {
      Tone::Neutral => p.muted_foreground,
      Tone::Primary => p.primary,
      Tone::Success => p.success,
      Tone::Warning => p.warning,
      Tone::Danger => p.destructive,
      Tone::GitOpen => p.pr_open,
      Tone::GitMerged => p.pr_merged,
      Tone::GitClosed => p.pr_closed,
    }
  }

  /// The tone as text on the window background: the 400 shade in dark
  /// mode and the 700 shade in light, where the solid color is too faint.
  pub fn text(self, p: &Palette) -> Hsla {
    use crate::theme::tw;
    let (dark, light) = match self {
      Tone::Neutral => return p.muted_foreground,
      Tone::Primary => return p.primary,
      Tone::GitOpen | Tone::GitMerged | Tone::GitClosed => return self.color(p),
      Tone::Success => (tw::GREEN_400, tw::GREEN_700),
      Tone::Warning => (tw::YELLOW_400, tw::YELLOW_700),
      Tone::Danger => (tw::RED_400, tw::RED_700),
    };
    tw::color(if p.dark { dark } else { light })
  }

  /// The fill behind a badge, an alert or a highlighted line.
  pub fn tint(self, p: &Palette) -> Hsla {
    self.color(p).opacity(if p.dark { 0.15 } else { 0.1 })
  }

  /// The border around a tinted area.
  pub fn border(self, p: &Palette) -> Hsla {
    self.color(p).opacity(0.4)
  }
}

/// A short label in a pill, such as `Draft` or `PRO`: 20 tall, caption
/// text, the tone's color over its tint.
#[derive(IntoElement)]
pub struct Badge {
  label: SharedString,
  tone: Tone,
  palette: Palette,
}

impl Badge {
  pub fn new(label: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      label: label.into(),
      tone: Tone::Neutral,
      palette: *palette,
    }
  }

  pub fn tone(mut self, tone: Tone) -> Self {
    self.tone = tone;
    self
  }
}

impl RenderOnce for Badge {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    let (fg, bg) = match self.tone {
      Tone::Neutral => (p.foreground, p.muted),
      tone => {
        let color = tone.color(p);
        (color, color.opacity(if p.dark { 0.2 } else { 0.12 }))
      }
    };
    pill().px(zpx(6.)).bg(bg).text_color(fg).child(self.label)
  }
}

/// A number of things, such as changed files on a tab: a neutral pill
/// with tabular digits. It never takes a state's color.
#[derive(IntoElement)]
pub struct Count {
  count: usize,
  palette: Palette,
}

impl Count {
  pub fn new(count: usize, palette: &Palette) -> Self {
    Self {
      count,
      palette: *palette,
    }
  }

  /// The count as shown, capped so the pill keeps its width.
  pub fn label(&self) -> SharedString {
    if self.count > 999 {
      "999+".into()
    } else {
      self.count.to_string().into()
    }
  }
}

impl RenderOnce for Count {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = &self.palette;
    pill()
      .min_w(zpx(20.))
      .px(zpx(6.))
      .justify_center()
      .bg(p.muted)
      .text_color(p.muted_foreground)
      .font_family(crate::theme::MONO_FONT)
      .child(self.label())
  }
}

/// A 6px status dot, one per object, after its name. `label` names the
/// state ("Conflict", "Uncommitted changes"): hovering the dot shows it as
/// a tooltip, and screen readers read it, since a color alone says nothing.
#[derive(IntoElement)]
pub struct Dot {
  tone: Tone,
  label: SharedString,
  palette: Palette,
}

impl Dot {
  pub fn new(tone: Tone, label: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      tone,
      label: label.into(),
      palette: *palette,
    }
  }
}

impl RenderOnce for Dot {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let label = self.label.clone();
    div()
      .id(gpui::ElementId::Name(format!("dot-{}", self.label).into()))
      .role(gpui::Role::Image)
      .aria_label(self.label)
      .flex_none()
      .size(zpx(6.))
      .rounded_full()
      .bg(self.tone.color(&self.palette))
      .tooltip(move |window, cx| {
        gpui_kit::component::tooltip::Tooltip::new(label.clone()).build(window, cx)
      })
      .test_support()
  }
}

fn pill() -> gpui::Div {
  text(div(), TextSize::Xs)
    .flex()
    .flex_none()
    .items_center()
    .h(zpx(20.))
    .rounded_full()
    .font_weight(gpui::FontWeight::MEDIUM)
    .whitespace_nowrap()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme;

  #[test]
  fn test_badge_builder() {
    let p = theme::palette(true);
    let badge = Badge::new("Draft", &p).tone(Tone::GitMerged);
    assert_eq!(badge.tone, Tone::GitMerged);
    assert_eq!(Tone::GitMerged.color(&p), p.pr_merged);
    assert_eq!(Tone::default().color(&p), p.muted_foreground);
    assert_eq!(Tone::Danger.text(&p), theme::tw::color(theme::tw::RED_400));
    assert_eq!(
      Tone::Danger.text(&theme::palette(false)),
      theme::tw::color(theme::tw::RED_700)
    );
  }

  #[test]
  fn test_dot_builder() {
    let p = theme::palette(false);
    let dot = Dot::new(Tone::Danger, "Conflict", &p);
    assert_eq!(dot.tone, Tone::Danger);
    assert_eq!(dot.label.as_ref(), "Conflict");
  }

  #[test]
  fn counts_cap_at_three_digits() {
    let p = theme::palette(false);
    assert_eq!(Count::new(7, &p).label().as_ref(), "7");
    assert_eq!(Count::new(999, &p).label().as_ref(), "999");
    assert_eq!(Count::new(1000, &p).label().as_ref(), "999+");
  }
}
