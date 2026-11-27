//! The control that shows a value and opens a menu to change it.

use std::{rc::Rc, time::Duration};

use gpui::{
  div, prelude::*, Animation, AnimationExt as _, App, ClickEvent, ElementId, SharedString,
  Transformation, Window,
};

use crate::{
  icon, text, theme, theme::Palette, zpx, Button, OnClick, OnHover, SelectorExt as _, Size,
  TextSize, Variant,
};

/// The gap between a [`Picker::full_width`] and its menu: tighter than a
/// dropdown's 4px, so the two read as one control.
pub const MENU_GAP: f32 = 2.;

/// How long a [`Picker::full_width`]'s chevron takes to turn over.
pub const CHEVRON_TURN: Duration = Duration::from_millis(150);

/// Where a [`Picker`] sits, which sets its height.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PickerPlacement {
  /// A form field, 32 tall to line up with inputs.
  #[default]
  Form,
  /// A toolbar or composer row, 28 tall.
  Toolbar,
}

impl PickerPlacement {
  pub fn height(self) -> f32 {
    match self {
      PickerPlacement::Form => 32.,
      PickerPlacement::Toolbar => 28.,
    }
  }
}

/// A select: the current value with a trailing chevron, in an outline
/// button, so it reads as a value that can change. A command that opens a
/// menu is a [`Button`] with no chevron instead.
#[derive(IntoElement)]
pub struct Picker {
  id: SharedString,
  palette: Palette,
  value: Option<SharedString>,
  placeholder: SharedString,
  label: Option<SharedString>,
  icon: Option<&'static str>,
  badge: Option<SharedString>,
  placement: PickerPlacement,
  full_width: bool,
  open: bool,
  disabled: bool,
  on_click: Option<OnClick>,
  on_hover: Option<OnHover>,
}

impl Picker {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      value: None,
      placeholder: "Select…".into(),
      label: None,
      icon: None,
      badge: None,
      placement: PickerPlacement::default(),
      full_width: false,
      open: false,
      disabled: false,
      on_click: None,
      on_hover: None,
    }
  }

  /// The current value.
  pub fn value(mut self, value: impl Into<SharedString>) -> Self {
    self.value = Some(value.into());
    self
  }

  /// What shows while nothing is picked.
  pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
    self.placeholder = placeholder.into();
    self
  }

  /// What the value is, such as "Target branch": the accessible name.
  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// A leading badge with the value's initial, such as a project's, in
  /// place of an icon: filled while there is a value, a dashed outline while
  /// the placeholder shows. It is decoration; the accessible name stays the
  /// value.
  pub fn badge(mut self, initial: impl Into<SharedString>) -> Self {
    self.badge = Some(initial.into());
    self
  }

  /// Runs `f` as the pointer comes to rest on it or leaves.
  pub fn on_hover(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
    self.on_hover = Some(Rc::new(f));
    self
  }

  /// A leading icon inside the control, such as the agent's logo.
  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = Some(path);
    self
  }

  pub fn placement(mut self, placement: PickerPlacement) -> Self {
    self.placement = placement;
    self
  }

  /// A full-width dropdown, such as the sidebar's project switcher: it
  /// fills its container, and it and its menu read as one control.
  ///
  /// - The value takes the free space, so the chevron ends the control.
  /// - While the menu is open the chevron points up, turning over in
  ///   [`CHEVRON_TURN`] (ease-in-out) as the menu opens and closes, and the
  ///   control reads out as expanded. It holds still until the menu first
  ///   opens.
  /// - The menu opens [`MENU_GAP`] under it, left-aligned and at least as
  ///   wide, wider only when a row needs it, with the control's corners
  ///   ([`crate::radius::MD`]). A popup-window menu gets this from the app's
  ///   `popup::MenuAnchor::Button`; an in-view menu spans it with
  ///   `left_0().right_0()`.
  pub fn full_width(mut self) -> Self {
    self.full_width = true;
    self
  }

  /// Whether its menu is open, which draws it pressed.
  pub fn open(mut self, open: bool) -> Self {
    self.open = open;
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }

  /// Opens the menu.
  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(Rc::new(f));
    self
  }

  /// The accessible name: what the value is, then the value.
  pub fn accessible_name(&self) -> SharedString {
    let value = self.value.as_ref().unwrap_or(&self.placeholder);
    match &self.label {
      Some(label) => format!("{label}: {value}").into(),
      None => value.clone(),
    }
  }
}

impl RenderOnce for Picker {
  fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = self.palette;
    let id = self.id.clone();
    let chevron = icon(theme::ICON_CHEVRON_DOWN, 14., p.muted_foreground);
    let chevron = if self.full_width {
      let turns = menu_turns(&self.id, self.open, window, cx);
      let (from, to) = chevron_turn(self.open, turns);
      chevron
        .with_animation(
          // A new id each time the menu opens or closes starts the turn.
          ElementId::NamedInteger(format!("{}-chevron", self.id).into(), turns),
          Animation::new(CHEVRON_TURN).with_easing(gpui::ease_in_out),
          move |svg, delta| {
            svg.with_transformation(Transformation::rotate(gpui::percentage(
              from + (to - from) * delta,
            )))
          },
        )
        .into_any_element()
    } else {
      chevron.into_any_element()
    };
    let (full_width, open) = (self.full_width, self.open);
    let name = self.accessible_name();
    let empty = self.value.is_none();
    let value = self.value.unwrap_or(self.placeholder);
    let toolbar = self.placement == PickerPlacement::Toolbar;
    Button::new(self.id, &p)
      .variant(Variant::Outline)
      .size(Size::Xs)
      .disabled(self.disabled)
      .selected(self.open)
      .aria_label(name)
      .h(zpx(self.placement.height()))
      .px(zpx(if toolbar { 8. } else { 10. }))
      .gap_1p5()
      .justify_start()
      .font_weight(gpui::FontWeight::NORMAL)
      .when_some(self.icon, |b, path| b.icon(path, 16.))
      .when_some(self.badge, |b, initial| {
        b.child(badge(format!("{id}-badge"), initial, !empty, &p))
      })
      .child(
        text(div(), TextSize::Sm)
          .flex_1()
          .min_w_0()
          .truncate()
          .when(empty, |el| el.text_color(p.muted_foreground))
          .child(value),
      )
      .child(
        div()
          .selector(format!("{id}-chevron"))
          .flex()
          .flex_none()
          .child(chevron),
      )
      .when(full_width, |b| b.w_full().expanded(open))
      .when_some(self.on_click, |b, f| {
        b.on_click(move |e, w, cx| f(e, w, cx))
      })
      .when_some(self.on_hover, |b, f| {
        b.on_hover(move |hovered, w, cx| f(hovered, w, cx))
      })
  }
}

/// An 18px square with `initial`: filled when `filled`, else a dashed
/// outline.
fn badge(id: String, initial: SharedString, filled: bool, p: &Palette) -> impl IntoElement {
  div()
    .selector(id)
    .flex()
    .flex_none()
    .items_center()
    .justify_center()
    .size(zpx(18.))
    .rounded_md()
    .when(filled, |this| {
      this.bg(p.primary).text_color(p.primary_foreground)
    })
    .when(!filled, |this| {
      this
        .border_1()
        .border_dashed()
        .border_color(p.muted_foreground)
        .text_color(p.muted_foreground)
    })
    // Short label (['avatar initial']): 11px is fine.
    // ast-grep-ignore: gpui-text-xs-short-label-only
    .child(text(div(), TextSize::Xs).child(initial))
}

/// Where a full-width picker's chevron turns from and to, in turns, given
/// whether its menu is `open` and how many times that has changed.
fn chevron_turn(open: bool, turns: u64) -> (f32, f32) {
  let to = if open { 0.5 } else { 0. };
  let from = if turns == 0 { to } else { 0.5 - to };
  (from, to)
}

/// How many times picker `id`'s menu has opened or closed, kept across
/// frames.
fn menu_turns(id: &SharedString, open: bool, window: &mut Window, cx: &mut App) -> u64 {
  let key = ElementId::Name(format!("{id}-menu-turns").into());
  let state = window.use_keyed_state(key, cx, |_, _| (open, 0u64));
  let (was, turns) = *state.read(cx);
  if was == open {
    return turns;
  }
  state.update(cx, |state, _| *state = (open, turns + 1));
  turns + 1
}

#[cfg(test)]
mod tests {
  use gpui::{px, Context, Render, TestAppContext};
  use gpui_testing_library::{prelude::*, Screen};

  use super::*;

  /// A full-width picker in a 300px row, which sizes its children to their
  /// content unless they fill it.
  struct Column {
    open: bool,
  }

  impl Render for Column {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      div().flex().w(px(300.)).child(
        Picker::new("project", &p)
          .label("Project")
          .value("Event platform")
          .badge("E")
          .full_width()
          .open(self.open),
      )
    }
  }

  #[gpui::test]
  fn a_full_width_pickers_chevron_ends_it(cx: &mut TestAppContext) {
    cx.update(theme::init);
    let (_, cx) = cx.add_window_view(|_, _| Column { open: false });
    cx.run_until_parked();
    let picker = cx.debug_bounds("project").expect("the picker is drawn");
    let chevron = cx
      .debug_bounds("project-chevron")
      .expect("the chevron is drawn");
    assert!(
      picker.right() - chevron.right() <= px(12.),
      "{chevron:?} in {picker:?}"
    );
  }

  /// A picker that records the last hover it reported.
  struct Hovered {
    seen: std::rc::Rc<std::cell::Cell<Option<bool>>>,
  }

  impl Render for Hovered {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      let seen = self.seen.clone();
      div().w(px(300.)).child(
        Picker::new("project", &p)
          .value("Event platform")
          .on_hover(move |hovered, _, _| seen.set(Some(*hovered))),
      )
    }
  }

  #[gpui::test]
  fn a_picker_tells_when_the_pointer_rests_on_it(cx: &mut TestAppContext) {
    cx.update(theme::init);
    let seen = std::rc::Rc::new(std::cell::Cell::new(None));
    let (_, cx) = cx.add_window_view({
      let seen = seen.clone();
      move |_, _| Hovered { seen }
    });
    cx.run_until_parked();
    let window = cx.update(|window, _| window.window_handle());
    let screen = Screen::new(cx, window);
    screen.redraw();
    let picker = screen.get_by_role(by::role(gpui::Role::Button).name("Event platform"));
    screen.user().hover(&picker);
    assert_eq!(seen.get(), Some(true));
  }

  #[gpui::test]
  fn a_picker_badge_leads_its_value(cx: &mut TestAppContext) {
    cx.update(theme::init);
    let (_, cx) = cx.add_window_view(|_, _| Column { open: false });
    cx.run_until_parked();
    let picker = cx.debug_bounds("project").expect("the picker is drawn");
    let badge = cx
      .debug_bounds("project-badge")
      .expect("the badge is drawn");
    // An 18px square at the start, inside the picker.
    assert_eq!(badge.size, gpui::size(px(18.), px(18.)));
    assert!(badge.left() > picker.left() && badge.left() < picker.left() + px(12.));
    let window = cx.update(|window, _| window.window_handle());
    let screen = Screen::new(cx, window);
    screen.redraw();
    // The initial is decoration: the name stays what the value is.
    screen.get_by_role(by::role(gpui::Role::Button).name("Project: Event platform"));
  }

  #[gpui::test]
  fn a_full_width_picker_fills_its_container(cx: &mut TestAppContext) {
    cx.update(theme::init);
    let (_, cx) = cx.add_window_view(|_, _| Column { open: false });
    cx.run_until_parked();
    let bounds = cx.debug_bounds("project").expect("the picker is drawn");
    assert_eq!(bounds.size.width, px(300.));
  }

  #[gpui::test]
  fn a_full_width_picker_reports_whether_its_menu_is_open(cx: &mut TestAppContext) {
    cx.update(theme::init);
    for open in [false, true] {
      let (_, cx) = cx.add_window_view(move |_, _| Column { open });
      cx.run_until_parked();
      let window = cx.update(|window, _| window.window_handle());
      let screen = Screen::new(cx, window);
      screen.redraw();
      let picker = screen
        .get_by_role(by::role(gpui::Role::Button).name("Project: Event platform"))
        .snapshot();
      assert_eq!(picker.expanded(), Some(open));
    }
  }

  #[test]
  fn a_full_width_pickers_chevron_turns_over_with_its_menu() {
    assert_eq!(
      chevron_turn(false, 0),
      (0., 0.),
      "still until the menu first opens"
    );
    assert_eq!(chevron_turn(true, 1), (0., 0.5), "half a turn as it opens");
    assert_eq!(chevron_turn(false, 2), (0.5, 0.), "and back as it closes");
  }

  #[test]
  fn test_picker_builder() {
    let p = theme::palette(false);
    let picker = Picker::new("target", &p)
      .label("Target branch")
      .placeholder("Pick a branch")
      .placement(PickerPlacement::Toolbar)
      .icon(theme::ICON_CHEVRON_DOWN)
      .open(true)
      .disabled(true);
    assert_eq!(
      picker.accessible_name().as_ref(),
      "Target branch: Pick a branch"
    );
    let picker = picker.value("main");
    assert_eq!(picker.accessible_name().as_ref(), "Target branch: main");
    assert_eq!(PickerPlacement::Toolbar.height(), 28.);
  }
}
