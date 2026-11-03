//! List rows: the sidebar, file trees, dialog lists, commits and PRs.
//!
//! Every selectable row in the app is a [`Row`]: one height per density,
//! one hover fill ([`Palette::hover`]) and one selected fill
//! ([`Palette::selected`]), with no shadow and the text left its own colour.

use gpui::{
  div, prelude::*, px, AnyElement, App, Div, Hsla, Interactivity, Role, SharedString, Stateful,
  StyleRefinement, Window,
};

use gpui_kit::base::TestSupportExt as _;

use crate::{icon, spinner, text, theme, theme::Palette, zpx, TextSize};

/// How tall a [`Row`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RowDensity {
  /// [`theme::ROW_HEIGHT_COMPACT`] (28): the sidebar, file trees and menus.
  #[default]
  Compact,
  /// [`theme::ROW_HEIGHT`] (32): lists in screens, dialogs and settings.
  Default,
  /// At least 48, with the meta on a second line: commits, PRs, issues.
  TwoLine,
  /// At least [`TOUCH_ROW_HEIGHT`] (56), for a finger: body-size title, the
  /// meta on a second line, touch padding, and a press fill instead of a
  /// hover fill, which would stay on after a tap.
  Touch,
}

/// The height of a [`RowDensity::Touch`] row.
pub const TOUCH_ROW_HEIGHT: f32 = 56.;

impl RowDensity {
  pub fn min_height(self) -> f32 {
    match self {
      RowDensity::Compact => theme::ROW_HEIGHT_COMPACT,
      RowDensity::Default => theme::ROW_HEIGHT,
      RowDensity::TwoLine => 48.,
      RowDensity::Touch => TOUCH_ROW_HEIGHT,
    }
  }
}

/// A selectable list row, in slots: `leading` (an icon, chevron or
/// checkbox), a `title` and `meta`, free `child`ren after them, `trailing`
/// status (a [`crate::Dot`] or [`crate::Count`]) and a fixed-width lane for
/// actions, so titles don't shift when the actions change. Hover and
/// selected use the palette's one fill each.
///
/// Styles set on the row (padding, a font, a width) apply over its
/// defaults; the fills are the row's own.
#[derive(IntoElement)]
pub struct Row {
  base: Stateful<Div>,
  palette: Palette,
  density: RowDensity,
  role: Option<Role>,
  clickable: bool,
  label: Option<SharedString>,
  icon: Option<&'static str>,
  leading: Vec<AnyElement>,
  title: Option<SharedString>,
  meta: Option<SharedString>,
  body: Vec<AnyElement>,
  overlays: Vec<AnyElement>,
  trailing: Vec<AnyElement>,
  actions: Option<(AnyElement, f32)>,
  selected: bool,
  disabled: bool,
  interactive: bool,
  centered: bool,
  divider: bool,
}

impl Row {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    let id: SharedString = id.into();
    let selector = id.to_string();
    Self {
      base: div().id(id).debug_selector(move || selector),
      palette: *palette,
      density: RowDensity::default(),
      role: None,
      clickable: false,
      label: None,
      icon: None,
      leading: Vec::new(),
      title: None,
      meta: None,
      body: Vec::new(),
      overlays: Vec::new(),
      trailing: Vec::new(),
      actions: None,
      selected: false,
      disabled: false,
      interactive: true,
      centered: false,
      divider: false,
    }
  }

  /// The "Load more" row at the end of a paged list: a button, disabled
  /// with a spinner while the next page loads.
  pub fn load_more(id: impl Into<SharedString>, loading: bool, palette: &Palette) -> Self {
    let id: SharedString = id.into();
    let spin = SharedString::from(format!("{id}-spin"));
    let mut row = Self::new(id, palette)
      .density(RowDensity::Default)
      .role(Role::Button)
      .title("Load more")
      .disabled(loading);
    row.centered = true;
    if loading {
      row = row.leading(spinner(spin, 14., palette.muted_foreground));
    }
    row
  }

  pub fn density(mut self, density: RowDensity) -> Self {
    self.density = density;
    self
  }

  /// The accessibility role. Unless set, a row that takes a click (see
  /// [`Row::on_click`]) is a [`Role::Button`] and any other a
  /// [`Role::ListItem`]: a palette's rows are options, a menu's are menu
  /// items.
  pub fn role(mut self, role: Role) -> Self {
    self.role = Some(role);
    self
  }

  /// Runs `listener` on a click, and makes the row a button unless its role
  /// is set.
  pub fn on_click(
    mut self,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
  ) -> Self {
    self.clickable = true;
    self.base = self.base.on_click(listener);
    self
  }

  /// The role the row draws with: [`Row::role`], else by whether it takes
  /// a click.
  pub fn resolved_role(&self) -> Role {
    self.role.unwrap_or(if self.clickable {
      Role::Button
    } else {
      Role::ListItem
    })
  }

  /// The accessible name, when it isn't the title (or there is none).
  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    self.label = Some(label.into());
    self
  }

  pub fn icon(mut self, path: &'static str) -> Self {
    self.icon = Some(path);
    self
  }

  /// Before the title: a chevron, a status icon, an avatar, a checkbox.
  pub fn leading(mut self, element: impl IntoElement) -> Self {
    self.leading.push(element.into_any_element());
    self
  }

  pub fn title(mut self, title: impl Into<SharedString>) -> Self {
    self.title = Some(title.into());
    self
  }

  /// Secondary text: after the title, or under it in a two-line row.
  pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
    self.meta = Some(meta.into());
    self
  }

  /// An absolutely placed child of the row itself, laid out against the
  /// whole row: a tree guide, a tooltip anchor.
  pub fn overlay(mut self, element: impl IntoElement) -> Self {
    self.overlays.push(element.into_any_element());
    self
  }

  /// A status dot or count after the title.
  pub fn trailing(mut self, element: impl IntoElement) -> Self {
    self.trailing.push(element.into_any_element());
    self
  }

  /// The row's actions, in a lane `width` wide that is kept whether or not
  /// they show.
  pub fn actions(mut self, actions: impl IntoElement, width: f32) -> Self {
    self.actions = Some((actions.into_any_element(), width));
    self
  }

  pub fn selected(mut self, selected: bool) -> Self {
    self.selected = selected;
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }

  /// A hairline under the row, as between the rows of a card. It sits on
  /// the next row's top edge (or the card's border under the last row) and
  /// stops short of rounded corners.
  pub fn divider(mut self, divider: bool) -> Self {
    self.divider = divider;
    self
  }

  /// False for a row that takes no click, such as a line of results: no
  /// hover and no pointer cursor.
  pub fn interactive(mut self, interactive: bool) -> Self {
    self.interactive = interactive;
    self
  }

  /// The fill under the pointer: [`Palette::hover`], on a row that takes a
  /// click and isn't already selected.
  pub fn hover_fill(&self) -> Option<Hsla> {
    let touch = self.density == RowDensity::Touch;
    (self.interactive && !self.disabled && !self.selected && !touch).then(|| self.palette.hover())
  }

  /// The accessible name: [`Row::label`], else the title.
  pub fn accessible_name(&self) -> Option<SharedString> {
    self.label.clone().or_else(|| self.title.clone())
  }

  /// The row's fill at rest: [`Palette::selected`] when selected.
  pub fn fill(&self) -> Option<Hsla> {
    self.selected.then(|| self.palette.selected())
  }

  /// The title's colour, the same selected or not.
  pub fn title_color(&self) -> Hsla {
    if self.disabled {
      self.palette.muted_foreground.opacity(0.6)
    } else if self.centered {
      self.palette.muted_foreground
    } else {
      self.palette.foreground
    }
  }
}

impl ParentElement for Row {
  fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
    self.body.extend(elements);
  }
}

impl InteractiveElement for Row {
  fn interactivity(&mut self) -> &mut Interactivity {
    self.base.interactivity()
  }
}

impl StatefulInteractiveElement for Row {}

impl Styled for Row {
  fn style(&mut self) -> &mut gpui::StyleRefinement {
    self.base.style()
  }
}

impl RenderOnce for Row {
  fn render(mut self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let touch = self.density == RowDensity::Touch;
    let two_line = self.density == RowDensity::TwoLine || touch;
    let (body_px, _) = theme::Density::Touch.font_sizes();
    let hover = self.hover_fill();
    let fg = self.title_color();
    let fill = self.fill();
    let name = self.accessible_name();
    let interactive = self.interactive && !self.disabled;
    // A checkbox row says it is toggled, and load more is a plain button.
    let role = self.resolved_role();
    let selectable = !matches!(role, Role::CheckBox | Role::MenuItemCheckBox) && !self.centered;
    let centered = self.centered;
    let title = self.title.map(|title| {
      text(div(), TextSize::Sm)
        .when(touch, |el| {
          el.text_size(px(body_px)).line_height(px(body_px + 5.))
        })
        .min_w_0()
        .truncate()
        .text_color(fg)
        .child(title)
    });
    let meta = self.meta.map(|meta| {
      text(div(), if two_line { TextSize::Xs } else { TextSize::Sm })
        .when(touch, |el| {
          el.text_size(px(body_px - 1.)).line_height(px(body_px + 4.))
        })
        .min_w_0()
        .truncate()
        .text_color(p.muted_foreground)
        .child(meta)
    });
    // The row's defaults go first, so styles set on it win.
    let user: StyleRefinement = std::mem::take(self.base.style());
    let mut base = text(self.base, TextSize::Sm)
      .when(touch, |el| {
        el.text_size(px(body_px)).line_height(px(body_px + 5.))
      })
      .role(role)
      .when_some(name, |el, name| el.aria_label(name))
      .when(selectable, |el| el.aria_selected(self.selected))
      .test_support()
      .flex()
      .items_center()
      .gap_2()
      .px_2()
      .when(touch, |el| el.gap_3().px_4().py_2().relative())
      // A finger's target is in points: a touch row doesn't follow the rem.
      .map(|el| match touch {
        true => el.min_h(px(TOUCH_ROW_HEIGHT)),
        false => el.min_h(zpx(self.density.min_height())),
      })
      // An explicit min height drops flex's content minimum: without this a
      // row in a short column would shrink to it and spill its content.
      .flex_shrink_0()
      .when(two_line && !touch, |el| el.py_1p5())
      .when(!touch, |el| el.rounded(zpx(theme::RADIUS_SM)))
      .text_color(p.foreground)
      .when(centered, |el| el.justify_center())
      .when(interactive, |el| el.cursor_pointer());
    base.style().refine(&user);
    base
      .when_some(fill, |el, fill| el.bg(fill))
      .when_some(hover, |el, hover| el.hover(move |s| s.bg(hover)))
      .when(touch && interactive, |el| {
        let pressed = p.hover();
        el.active(move |s| s.bg(pressed))
          .child(crate::press::press_overlay())
      })
      .when(self.divider, |el| {
        el.child(
          div()
            .absolute()
            .bottom(px(-1.))
            .left(px(theme::RADIUS_LG))
            .right(px(theme::RADIUS_LG))
            .h(px(1.))
            .bg(p.border),
        )
      })
      .when_some(self.icon, |el, path| {
        el.child(icon(path, 16., p.muted_foreground))
      })
      .children(self.leading)
      .child(
        div()
          .when(!centered, |el| el.flex_1())
          .min_w_0()
          .flex()
          .when(two_line, |el| el.flex_col())
          .when(!two_line, |el| el.items_center().gap_2())
          .children(title)
          .children(meta)
          .children(self.body),
      )
      .children(self.trailing)
      .children(self.overlays)
      .when_some(self.actions, |el, (actions, width)| {
        el.child(
          div()
            .flex_none()
            .w(zpx(width))
            .flex()
            .justify_end()
            .child(actions),
        )
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_row_builder() {
    let p = theme::palette(false);
    let row = Row::new("commit-1", &p)
      .density(RowDensity::TwoLine)
      .icon(theme::ICON_CIRCLE_DOT)
      .title("feat: keyvalues cache")
      .meta("just now")
      .trailing(div())
      .actions(div(), 56.)
      .selected(true)
      .disabled(false);
    assert_eq!(row.density.min_height(), 48.);
    assert_eq!(row.trailing.len(), 1);
    assert!(row.actions.is_some_and(|(_, width)| width == 56.));
  }

  #[test]
  fn densities_use_the_row_height_tokens() {
    assert_eq!(RowDensity::Compact.min_height(), theme::ROW_HEIGHT_COMPACT);
    assert_eq!(RowDensity::Default.min_height(), theme::ROW_HEIGHT);
    assert_eq!(theme::ROW_HEIGHT_COMPACT, 28.);
    assert_eq!(theme::ROW_HEIGHT, 32.);
  }

  /// A phone row is at least as tall as the 56pt touch row.
  #[test]
  fn touch_rows_are_at_least_56_tall() {
    assert!(RowDensity::Touch.min_height() >= 56.);
  }

  /// A phone row darkens while pressed instead of keeping a hover fill,
  /// which would stay on after a tap.
  #[test]
  fn touch_rows_have_no_hover_fill() {
    let p = theme::palette(false);
    let row = Row::new("a", &p).density(RowDensity::Touch).divider(true);
    assert_eq!(row.hover_fill(), None);
    assert!(row.divider);
  }

  #[test]
  fn slots_collect_leading_body_and_trailing() {
    let p = theme::palette(false);
    let row = Row::new("file", &p)
      .leading(div())
      .leading(div())
      .title("src/main.rs")
      .child(div())
      .trailing(div())
      .overlay(div())
      .label("main.rs, modified");
    assert_eq!(row.overlays.len(), 1);
    assert_eq!(row.leading.len(), 2);
    assert_eq!(row.body.len(), 1);
    assert_eq!(row.trailing.len(), 1);
    assert_eq!(row.accessible_name().as_deref(), Some("main.rs, modified"));
  }

  #[test]
  fn the_title_is_the_accessible_name_by_default() {
    let p = theme::palette(true);
    let row = Row::new("ws", &p).title("feat/cache");
    assert_eq!(row.accessible_name().as_deref(), Some("feat/cache"));
  }

  #[test]
  fn selected_fills_with_the_selected_token_and_keeps_the_text_colour() {
    for dark in [false, true] {
      let p = theme::palette(dark);
      let on = Row::new("a", &p).selected(true);
      let off = Row::new("b", &p);
      assert_eq!(on.fill(), Some(p.selected()));
      assert_eq!(off.fill(), None);
      assert_eq!(on.title_color(), p.foreground);
      assert_eq!(on.title_color(), off.title_color());
    }
  }

  #[test]
  fn load_more_is_a_button_named_load_more() {
    let p = theme::palette(false);
    let row = Row::load_more("prs-more", false, &p);
    assert_eq!(row.resolved_role(), gpui::Role::Button);
    assert_eq!(row.accessible_name().as_deref(), Some("Load more"));
    assert!(!row.disabled);
    assert!(Row::load_more("prs-more", true, &p).disabled);
  }

  #[test]
  fn a_row_that_takes_a_click_is_a_button() {
    let p = theme::palette(false);
    let row = Row::new("file", &p)
      .title("README.md")
      .on_click(|_, _, _| {});
    assert_eq!(row.resolved_role(), gpui::Role::Button);
  }

  #[test]
  fn a_row_without_a_click_is_a_list_item() {
    let p = theme::palette(false);
    assert_eq!(Row::new("line", &p).resolved_role(), gpui::Role::ListItem);
  }

  #[test]
  fn an_explicit_role_wins_over_the_click_default() {
    let p = theme::palette(false);
    let option = Row::new("cmd", &p)
      .on_click(|_, _, _| {})
      .role(gpui::Role::ListBoxOption);
    assert_eq!(option.resolved_role(), gpui::Role::ListBoxOption);
    let item = Row::new("item", &p)
      .role(gpui::Role::MenuItem)
      .on_click(|_, _, _| {});
    assert_eq!(item.resolved_role(), gpui::Role::MenuItem);
  }

  #[test]
  fn hover_uses_the_hover_token_only_on_rows_that_take_a_click() {
    let p = theme::palette(false);
    assert_eq!(Row::new("a", &p).hover_fill(), Some(p.hover()));
    assert_eq!(Row::new("b", &p).selected(true).hover_fill(), None);
    assert_eq!(Row::new("c", &p).disabled(true).hover_fill(), None);
    assert_eq!(Row::new("d", &p).interactive(false).hover_fill(), None);
  }

  mod render {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
      div, px, Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
      VisualTestContext, Window,
    };

    use crate::{theme, Row, RowDensity};

    struct Host {
      clicks: Rc<Cell<usize>>,
    }

    impl Render for Host {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        let clicks = self.clicks.clone();
        div()
          .w(px(300.))
          .flex()
          .flex_col()
          .child(Row::new("compact", &p).title("compact"))
          .child(
            Row::new("default", &p)
              .density(RowDensity::Default)
              .title("default"),
          )
          .child(
            Row::new("tall", &p).py_1p5().child(
              div()
                .flex_1()
                .min_w_0()
                .child(
                  crate::text(div(), crate::TextSize::Sm)
                    .truncate()
                    .child("Title"),
                )
                .child(
                  crate::text(div(), crate::TextSize::Sm)
                    .truncate()
                    .child("More"),
                ),
            ),
          )
          .child(
            Row::new("touch", &p)
              .density(RowDensity::Touch)
              .title("touch")
              .meta("detail"),
          )
          .child(
            Row::load_more("more", false, &p).on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
          )
      }
    }

    fn mount(cx: &mut TestAppContext) -> (VisualTestContext, Rc<Cell<usize>>) {
      let clicks = Rc::new(Cell::new(0));
      let window = cx.add_window({
        let clicks = clicks.clone();
        move |_, _| Host { clicks }
      });
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      (cx, clicks)
    }

    #[gpui::test]
    fn rows_are_as_tall_as_their_density(cx: &mut TestAppContext) {
      let (mut cx, _) = mount(cx);
      let compact = cx.debug_bounds("compact").expect("compact row");
      let default = cx.debug_bounds("default").expect("default row");
      assert_eq!(compact.size.height, px(28.));
      assert_eq!(default.size.height, px(32.));
    }

    #[gpui::test]
    fn a_touch_row_is_56_tall_with_its_detail_below(cx: &mut TestAppContext) {
      let (mut cx, _) = mount(cx);
      let touch = cx.debug_bounds("touch").expect("touch row");
      assert!(touch.size.height >= px(56.), "{:?}", touch.size.height);
      // A finger's target is in points, not rems: a phone's 15px rem
      // doesn't shrink it.
      cx.update(|window, cx| {
        window.set_rem_size(px(15.));
        window.refresh();
        window.draw(cx).clear(cx);
      });
      let touch = cx.debug_bounds("touch").expect("touch row");
      assert!(touch.size.height >= px(56.), "{:?}", touch.size.height);
    }

    #[gpui::test]
    fn a_row_grows_to_fit_two_lines_of_content(cx: &mut TestAppContext) {
      let (mut cx, _) = mount(cx);
      let tall = cx.debug_bounds("tall").expect("tall row");
      assert_eq!(tall.size.height, px(52.));
    }

    /// In a scrolling column shorter than its rows, a row keeps its content's
    /// height rather than shrinking to its minimum.
    #[gpui::test]
    fn a_row_does_not_shrink_in_a_short_column(cx: &mut TestAppContext) {
      let window = cx.add_window(|_, _| Short);
      let mut cx = VisualTestContext::from_window(*window, cx);
      cx.update(|window, cx| window.draw(cx).clear(cx));
      let row = cx.debug_bounds("short-tall").expect("row");
      assert_eq!(row.size.height, px(52.));
    }

    struct Short;

    impl Render for Short {
      fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(false);
        div()
          .w(px(300.))
          .max_h(px(60.))
          .flex()
          .flex_col()
          .children((0..3).map(|i| {
            Row::new(
              if i == 0 {
                "short-tall".to_string()
              } else {
                format!("short-{i}")
              },
              &p,
            )
            .py_1p5()
            .child(div().child(div().h(px(20.))).child(div().h(px(20.))))
          }))
      }
    }

    #[gpui::test]
    fn clicking_load_more_runs_its_handler(cx: &mut TestAppContext) {
      let (mut cx, clicks) = mount(cx);
      let more = cx.debug_bounds("more").expect("load more row");
      cx.simulate_click(more.center(), gpui::Modifiers::none());
      assert_eq!(clicks.get(), 1);
    }
  }
}
