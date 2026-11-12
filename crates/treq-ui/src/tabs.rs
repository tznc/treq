//! One tab strip for views and panels.

use std::rc::Rc;

use gpui::{div, prelude::*, App, FocusHandle, KeyDownEvent, SharedString, Window};
use gpui_kit::base::TestSupportExt as _;

use crate::{badge::Tone, text, theme, theme::Palette, zpx, Badge, Count, Dot, TextSize};

/// How a [`Tabs`] strip looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TabsStyle {
  /// A track with the selected segment raised: switches between views of
  /// one thing, such as Code / Commits / Changes / Logs.
  #[default]
  Segmented,
  /// Labels with a line under the selected one: switches panels, such as
  /// GitHub / Linear.
  Underline,
  /// A phone's bottom tab bar: equal tabs with the icon over the label,
  /// for the top-level areas of a screen. Five tabs at most.
  Bar,
}

/// One tab: a label, then an optional badge, count and status dot.
pub struct Tab {
  id: SharedString,
  label: SharedString,
  badge: Option<(SharedString, Tone)>,
  count: Option<usize>,
  dot: Option<(Tone, SharedString)>,
  icon: Option<&'static str>,
  disabled: bool,
}

impl Tab {
  pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
    Self {
      id: id.into(),
      label: label.into(),
      badge: None,
      count: None,
      dot: None,
      icon: None,
      disabled: false,
    }
  }

  /// A leading icon in the tab's text colour.
  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = Some(path);
    self
  }

  /// A label for what the tab is, such as a `PRO` plan chip.
  pub fn badge(mut self, label: impl Into<SharedString>, tone: Tone) -> Self {
    self.badge = Some((label.into(), tone));
    self
  }

  /// How many things the tab holds, as a neutral count.
  pub fn count(mut self, count: usize) -> Self {
    self.count = Some(count);
    self
  }

  /// A status dot, such as danger for a tab that holds a conflict, with
  /// the label its tooltip shows.
  pub fn dot(mut self, tone: Tone, label: impl Into<SharedString>) -> Self {
    self.dot = Some((tone, label.into()));
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }
}

type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A tab list. The application stores which tab is selected and changes
/// it from `on_select`. With a focus handle, the arrow keys, Home and End
/// move between tabs.
#[derive(IntoElement)]
pub struct Tabs {
  id: SharedString,
  palette: Palette,
  style: TabsStyle,
  tabs: Vec<Tab>,
  selected: usize,
  focus: Option<FocusHandle>,
  on_select: Option<OnSelect>,
}

impl Tabs {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      style: TabsStyle::default(),
      tabs: Vec::new(),
      selected: 0,
      focus: None,
      on_select: None,
    }
  }

  /// A segmented strip: a muted track, the active segment filled with
  /// [`Palette::selected`] and no shadow, hovers with
  /// [`Palette::segment_hover`].
  pub fn segmented(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self::new(id, palette).style(TabsStyle::Segmented)
  }

  /// A segment's fill at rest.
  pub fn segment_fill(&self, active: bool) -> Option<gpui::Hsla> {
    active.then(|| self.palette.selected())
  }

  /// A segment's fill under the pointer.
  pub fn segment_hover(&self) -> gpui::Hsla {
    self.palette.segment_hover()
  }

  pub fn style(mut self, style: TabsStyle) -> Self {
    self.style = style;
    self
  }

  pub fn tab(mut self, tab: Tab) -> Self {
    self.tabs.push(tab);
    self
  }

  pub fn tabs(mut self, tabs: impl IntoIterator<Item = Tab>) -> Self {
    self.tabs.extend(tabs);
    self
  }

  pub fn selected(mut self, selected: usize) -> Self {
    self.selected = selected;
    self
  }

  /// Takes the arrow keys while `focus` is focused.
  pub fn focus(mut self, focus: &FocusHandle) -> Self {
    self.focus = Some(focus.clone());
    self
  }

  pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
    self.on_select = Some(Rc::new(f));
    self
  }
}

/// The tab a key moves to from `current`, skipping disabled tabs and
/// wrapping at the ends, or `None` for any other key.
pub fn tab_for_key(key: &str, current: usize, disabled: &[bool]) -> Option<usize> {
  let len = disabled.len();
  let enabled = |i: &usize| !disabled[*i];
  match key {
    "home" => (0..len).find(enabled),
    "end" => (0..len).rev().find(enabled),
    "right" | "down" => (1..=len).map(|step| (current + step) % len).find(enabled),
    "left" | "up" => (1..=len)
      .map(|step| (current + len - step % len) % len)
      .find(enabled),
    _ => None,
  }
}

impl RenderOnce for Tabs {
  fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = self.palette;
    // The list is one tab stop and the arrows move between its tabs; a
    // caller that moves focus itself passes its own handle.
    let focus = self.focus.clone().or_else(|| {
      self.on_select.as_ref().map(|_| {
        let key = SharedString::from(format!("{}-focus", self.id));
        crate::tab_stop(key, window, cx)
      })
    });
    let focused = focus.as_ref().is_some_and(|f| f.is_focused(window));
    let own_focus = self.focus.is_none();
    // A phone is tapped, so every tab is at least the 44pt touch minimum.
    let touch = cx.try_global::<theme::Density>().copied() == Some(theme::Density::Touch);
    let segmented = self.style == TabsStyle::Segmented;
    let bar = self.style == TabsStyle::Bar;
    let selected = self.selected;
    let disabled: Vec<bool> = self.tabs.iter().map(|tab| tab.disabled).collect();
    let on_key = self.on_select.clone();
    let selector = self.id.to_string();
    let selected_fill = self.segment_fill(true);
    let segment_hover = self.segment_hover();
    let strip = div()
      .id(self.id)
      .debug_selector(move || selector)
      .role(gpui::Role::TabList)
      .test_support()
      .flex()
      .items_center()
      .when_some(focus, |el, focus| {
        el.track_focus(&focus)
          // A click on its own stop leaves focus where it was.
          .when(own_focus, |el| {
            el.on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
              window.prevent_default()
            })
          })
          .when_some(on_key, move |el, on_select| {
            el.on_key_down(move |event: &KeyDownEvent, window, cx| {
              let key = event.keystroke.key.as_str();
              if let Some(next) = tab_for_key(key, selected, &disabled) {
                cx.stop_propagation();
                on_select(next, window, cx);
              }
            })
          })
      })
      .map(|el| {
        if bar {
          el.w_full()
            .border_t_1()
            .border_color(p.border)
            .bg(p.background)
        } else if segmented {
          el.gap(zpx(2.))
            .p(zpx(2.))
            .rounded(zpx(theme::RADIUS_MD))
            .bg(p.muted)
        } else {
          el.gap_4().border_b_1().border_color(p.border)
        }
      })
      .children(self.tabs.into_iter().enumerate().map(|(ix, tab)| {
        let active = ix == selected;
        let on_select = self.on_select.clone().filter(|_| !tab.disabled);
        let selector = tab.id.to_string();
        let id = tab.id.to_string();
        let fg = if tab.disabled {
          p.muted_foreground.opacity(0.6)
        } else if active {
          p.foreground
        } else {
          p.muted_foreground
        };
        let fg = if bar && active && !tab.disabled {
          p.primary
        } else {
          fg
        };
        let on_fill = active && selected_fill.is_some();
        text(div(), if bar { TextSize::Xs } else { TextSize::Sm })
          .id(tab.id)
          .debug_selector(move || selector)
          .role(gpui::Role::Tab)
          .aria_label(tab.label.clone())
          .aria_selected(active)
          .test_support()
          .flex()
          .items_center()
          .gap_1p5()
          .font_weight(gpui::FontWeight::MEDIUM)
          .text_color(fg)
          .map(|el| {
            if bar {
              el.flex_1()
                .flex_col()
                .justify_center()
                .gap(zpx(2.))
                .h(zpx(56.))
                .relative()
            } else if segmented {
              el.h(zpx(if touch { 44. } else { 24. }))
                .when(touch, |el| el.flex_1().justify_center())
                .px_2()
                .rounded(zpx(theme::RADIUS_MD - 2.))
                .when_some(selected_fill.filter(|_| active), |el, fill| el.bg(fill))
            } else {
              el.h(zpx(if touch { 44. } else { 32. }))
                .border_b_2()
                .mb(zpx(-1.))
                .border_color(if active {
                  p.primary
                } else {
                  gpui::transparent_black()
                })
            }
          })
          .when(focused && active, |el| {
            el.rounded(zpx(theme::RADIUS_MD - 2.))
              .shadow(vec![crate::focus_ring(&p, false)])
          })
          .when(!tab.disabled && !active, |el| {
            el.cursor_pointer().hover(move |s| {
              let s = s.text_color(p.foreground);
              if segmented {
                s.bg(segment_hover)
              } else {
                s
              }
            })
          })
          .when_some(tab.icon, |el, path| {
            el.child(crate::icon(path, if bar { 22. } else { 16. }, fg))
          })
          .child(tab.label)
          .when_some(tab.badge, |el, (label, tone)| {
            el.child(Badge::new(label, &p).tone(tone))
          })
          .when_some(tab.count, |el, count| {
            let id = format!("{id}-count");
            el.child(
              div()
                .debug_selector(move || id)
                .when(bar, |el| {
                  el.absolute().top(zpx(4.)).left(gpui::relative(0.55))
                })
                .child(if on_fill {
                  Count::new(count, &p).on_selected(fg)
                } else {
                  Count::new(count, &p)
                }),
            )
          })
          .when_some(tab.dot, |el, (tone, label)| {
            let id = format!("{id}-dot");
            el.child(
              div()
                .debug_selector(move || id)
                .when(bar, |el| {
                  el.absolute().top(zpx(6.)).left(gpui::relative(0.6))
                })
                .child(Dot::new(tone, label, &p)),
            )
          })
          .when_some(on_select, move |el, f| {
            el.on_click(move |_, window, cx| f(ix, window, cx))
          })
      }));
    // A segmented track is as wide as its tabs: the row around it takes
    // the stretch a column parent gives its children.
    if segmented {
      div().flex().child(strip).into_any_element()
    } else {
      strip.into_any_element()
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_tabs_builder() {
    let p = theme::palette(false);
    let tabs = Tabs::new("views", &p)
      .style(TabsStyle::Underline)
      .tab(Tab::new("code", "Code"))
      .tabs([
        Tab::new("commits", "Commits").count(2),
        Tab::new("changes", "Changes")
          .count(3)
          .dot(Tone::Danger, "Conflict"),
        Tab::new("logs", "Logs").disabled(true),
        Tab::new("team", "Team").badge("PRO", Tone::Neutral),
      ])
      .selected(1);
    assert_eq!(tabs.tabs.len(), 5);
    assert_eq!(tabs.tabs[4].badge, Some(("PRO".into(), Tone::Neutral)));
    assert_eq!(tabs.tabs[2].dot, Some((Tone::Danger, "Conflict".into())));
    assert_eq!(tabs.selected, 1);
  }

  #[test]
  fn segmented_tabs_fill_the_active_segment_with_the_selected_token() {
    for dark in [false, true] {
      let p = theme::palette(dark);
      let tabs = Tabs::segmented("views", &p)
        .tab(Tab::new("code", "Code").icon(theme::ICON_CODE))
        .tab(Tab::new("commits", "Commits"));
      assert_eq!(tabs.style, TabsStyle::Segmented);
      assert_eq!(tabs.tabs[0].icon, Some(theme::ICON_CODE));
      assert_eq!(tabs.segment_fill(true), Some(p.selected()));
      assert_eq!(tabs.segment_fill(false), None);
      assert_eq!(tabs.segment_hover(), p.segment_hover());
    }
  }

  #[test]
  fn arrow_keys_skip_disabled_tabs_and_wrap() {
    let disabled = [false, true, false, false];
    assert_eq!(tab_for_key("right", 0, &disabled), Some(2));
    assert_eq!(tab_for_key("right", 3, &disabled), Some(0));
    assert_eq!(tab_for_key("left", 2, &disabled), Some(0));
    assert_eq!(tab_for_key("left", 0, &disabled), Some(3));
    assert_eq!(tab_for_key("home", 3, &[true, false]), Some(1));
    assert_eq!(tab_for_key("end", 0, &disabled), Some(3));
    assert_eq!(tab_for_key("enter", 0, &disabled), None);
  }

  mod gpui_tests {
    use gpui::{
      div, Context, IntoElement, ParentElement as _, Render, TestAppContext, VisualTestContext,
      Window,
    };

    use crate::{theme, theme::Density, Tab, Tabs, TabsStyle};

    struct Host;

    impl Render for Host {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        div()
          .child(
            Tabs::segmented("seg", &p)
              .tab(Tab::new("seg-a", "A"))
              .tab(Tab::new("seg-b", "B")),
          )
          .child(
            Tabs::new("line", &p)
              .style(TabsStyle::Underline)
              .tab(Tab::new("line-a", "A")),
          )
      }
    }

    fn heights(cx: &mut TestAppContext, density: Option<Density>) -> (f32, f32) {
      if let Some(density) = density {
        cx.update(|cx| cx.set_global(density));
      }
      let window = cx.add_window(|_, _| Host);
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      let seg = cx.debug_bounds("seg-a").expect("segment").size.height;
      let line = cx
        .debug_bounds("line-a")
        .expect("underline tab")
        .size
        .height;
      (f32::from(seg), f32::from(line))
    }

    struct Bar;

    impl Render for Bar {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        gpui::Styled::w(div(), gpui::px(400.)).child(
          Tabs::new("bar", &p)
            .style(TabsStyle::Bar)
            .tab(Tab::new("bar-a", "Terminals").icon(theme::ICON_TERMINAL))
            .tab(Tab::new("bar-b", "Changes").icon(theme::ICON_FILE_DIFF))
            .tab(Tab::new("bar-c", "Commits").icon(theme::ICON_COMMIT))
            .tab(Tab::new("bar-d", "Code").icon(theme::ICON_CODE)),
        )
      }
    }

    #[gpui::test]
    fn a_tab_bar_splits_the_width_in_56_tall_tabs(cx: &mut TestAppContext) {
      let window = cx.add_window(|_, _| Bar);
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      let a = cx.debug_bounds("bar-a").unwrap();
      let d = cx.debug_bounds("bar-d").unwrap();
      assert_eq!(a.size.width, d.size.width);
      assert_eq!(a.size.width, gpui::px(100.));
      assert_eq!(a.size.height, gpui::px(56.));
    }

    struct Column;

    impl Render for Column {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        gpui::Styled::flex_col(gpui::Styled::flex(gpui::Styled::w(div(), gpui::px(800.)))).child(
          Tabs::segmented("seg", &p)
            .tab(Tab::new("seg-a", "Hourly"))
            .tab(Tab::new("seg-b", "Monthly")),
        )
      }
    }

    #[gpui::test]
    fn segmented_tabs_fit_their_tabs_in_a_wide_column(cx: &mut TestAppContext) {
      let window = cx.add_window(|_, _| Column);
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      let track = f32::from(cx.debug_bounds("seg").expect("track").size.width);
      let a = cx.debug_bounds("seg-a").expect("segment a");
      let b = cx.debug_bounds("seg-b").expect("segment b");
      let tabs = f32::from(b.origin.x + b.size.width - a.origin.x);
      assert!(track < 400., "track is {track}px wide in an 800px column");
      assert!(
        track - tabs < 8.,
        "track {track}px hugs its tabs ({tabs}px)"
      );
    }

    #[gpui::test]
    fn desktop_tabs_keep_their_height(cx: &mut TestAppContext) {
      assert_eq!(heights(cx, None), (24., 32.));
    }

    #[gpui::test]
    fn touch_tabs_are_at_least_44pt_tall(cx: &mut TestAppContext) {
      let (seg, line) = heights(cx, Some(Density::Touch));
      assert!(seg >= 44., "segment is {seg}pt");
      assert!(line >= 44., "underline tab is {line}pt");
    }
  }
}
