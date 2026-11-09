//! Phone patterns: grouped lists, choice rows, the bottom bar and the action
//! sheet. Each is what a desktop component becomes at [`Density::Touch`].
//!
//! [`Density::Touch`]: crate::theme::Density::Touch

use std::rc::Rc;

use gpui::{div, prelude::*, App, Div, Role, SharedString, Window};

use crate::{icon, theme, theme::Palette, zpx, Row, RowDensity};

/// A bordered group of rows: a phone's list section.
pub fn grouped(p: &Palette) -> Div {
  div()
    .flex()
    .flex_col()
    .rounded(zpx(theme::RADIUS_LG))
    .border_1()
    .border_color(p.border)
    .bg(p.background)
    .overflow_hidden()
    // With overflow hidden it could otherwise shrink below its rows.
    .flex_shrink_0()
}

/// One item of a [`grouped`] list split across a virtualized list's items:
/// the side borders of each, the top of the `first` and the bottom of the
/// `last` with their rounded corners, inset by the page gutter.
pub fn grouped_item(child: impl IntoElement, first: bool, last: bool, p: &Palette) -> Div {
  let r = zpx(theme::RADIUS_LG);
  div().px(zpx(theme::space::LG)).child(
    div()
      .flex()
      .flex_col()
      .border_l_1()
      .border_r_1()
      .border_color(p.border)
      .bg(p.background)
      .overflow_hidden()
      .when(first, |c| c.border_t_1().rounded_tl(r).rounded_tr(r))
      .when(last, |c| c.border_b_1().rounded_bl(r).rounded_br(r))
      .child(child),
  )
}

/// A touch row in a grouped list: title, optional detail, a divider above.
pub fn row(id: impl Into<SharedString>, title: impl Into<SharedString>, p: &Palette) -> Row {
  Row::new(id, p)
    .density(RowDensity::Touch)
    .divider(true)
    .title(title)
}

/// A row that opens a page: a chevron at the end.
pub fn nav_row(id: impl Into<SharedString>, title: impl Into<SharedString>, p: &Palette) -> Row {
  row(id, title, p).trailing(icon(theme::ICON_CHEVRON_RIGHT, 16., p.muted_foreground))
}

/// One option of a single choice, a phone's radio: no chevron (it goes
/// nowhere), a check and the selected fill on the chosen one.
pub fn choice(
  id: impl Into<SharedString>,
  title: impl Into<SharedString>,
  selected: bool,
  p: &Palette,
) -> Row {
  let check = div().w(zpx(16.)).when(selected, |c| {
    c.child(icon(theme::ICON_CHECK, 16., p.primary))
  });
  row(id, title, p)
    .role(Role::RadioButton)
    .trailing(check)
    .selected(selected)
}

/// The bar pinned under a page that holds its main action: a full-width
/// [`theme::CTA_HEIGHT`] button, an outline secondary beside it.
pub fn bottom_bar(p: &Palette) -> Div {
  div()
    .flex()
    .flex_none()
    .p(zpx(theme::space::MD))
    .gap(zpx(theme::space::SM))
    .border_t_1()
    .border_color(p.border)
    .bg(p.background)
}

/// What an [`ActionSheet`] entry does when tapped. The sheet closes first.
pub type Action = Rc<dyn Fn(&mut Window, &mut App)>;

/// One action of an [`ActionSheet`].
#[derive(Clone)]
pub struct SheetAction {
  pub id: SharedString,
  pub label: SharedString,
  pub danger: bool,
  pub action: Action,
}

impl SheetAction {
  pub fn new(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    action: impl Fn(&mut Window, &mut App) + 'static,
  ) -> Self {
    Self {
      id: id.into(),
      label: label.into(),
      danger: false,
      action: Rc::new(action),
    }
  }

  /// Red, for an action that destroys. Put it last.
  pub fn danger(mut self) -> Self {
    self.danger = true;
    self
  }
}

/// A phone's context menu: a list of actions in a bottom sheet, the
/// translation of [`crate::MenuItem`]s and a split button's menu. Shortcuts
/// are dropped; destructive actions go last.
#[derive(IntoElement)]
pub struct ActionSheet {
  palette: Palette,
  entries: Vec<SheetAction>,
  on_dismiss: Option<Action>,
}

/// How tall an action row is: shorter than a list's rows, as they are
/// actions, not records, but past the 44 target.
pub const ACTION_ROW_HEIGHT: f32 = 48.;

impl ActionSheet {
  pub fn new(entries: Vec<SheetAction>, palette: &Palette) -> Self {
    Self {
      palette: *palette,
      entries,
      on_dismiss: None,
    }
  }

  /// Closes the sheet before an entry's action runs.
  pub fn on_dismiss(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
    self.on_dismiss = Some(Rc::new(f));
    self
  }
}

impl RenderOnce for ActionSheet {
  fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let dismiss = self.on_dismiss;
    div()
      .flex()
      .flex_col()
      .children(self.entries.into_iter().map(move |entry| {
        let action = entry.action.clone();
        let dismiss = dismiss.clone();
        Row::new(entry.id.clone(), &p)
          .density(RowDensity::Touch)
          .role(Role::MenuItem)
          .label(entry.label.clone())
          .min_h(zpx(ACTION_ROW_HEIGHT))
          .when(entry.danger, |row| row.text_color(p.destructive))
          .child(entry.label.clone())
          .on_click(move |_, window, cx| {
            if let Some(dismiss) = &dismiss {
              dismiss(window, cx);
            }
            action(window, cx);
          })
      }))
  }
}

#[cfg(test)]
mod tests {
  use std::cell::RefCell;

  use gpui::{px, TestAppContext, VisualTestContext};

  use super::*;

  struct Host(Rc<RefCell<Vec<&'static str>>>);

  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      let log = self.0.clone();
      let dismissed = self.0.clone();
      div()
        .w(px(390.))
        .child(grouped(&p).child(choice("merge", "Merge commit", true, &p)))
        .child(bottom_bar(&p).child(div().debug_selector(|| "cta".into()).flex_1().h(px(52.))))
        .child(
          ActionSheet::new(
            vec![
              SheetAction::new("rename", "Rename…", move |_, _| {
                log.borrow_mut().push("rename")
              }),
              SheetAction::new("delete", "Delete…", |_, _| {}).danger(),
            ],
            &p,
          )
          .on_dismiss(move |_, _| dismissed.borrow_mut().push("dismiss")),
        )
    }
  }

  #[gpui::test]
  fn rows_and_actions_meet_the_touch_target(cx: &mut TestAppContext) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let window = cx.add_window({
      let log = log.clone();
      move |_, _| Host(log)
    });
    let mut cx = VisualTestContext::from_window(*window, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let choice = cx.debug_bounds("merge").unwrap();
    assert!(choice.size.height >= px(theme::TOUCH_TARGET));
    let rename = cx.debug_bounds("rename").unwrap();
    assert!(rename.size.height >= px(ACTION_ROW_HEIGHT));
    // The CTA spans the bar less its 12 padding on each side.
    let cta = cx.debug_bounds("cta").unwrap();
    assert_eq!(cta.size.width, px(390. - 24.));
    cx.simulate_click(rename.center(), gpui::Modifiers::none());
    assert_eq!(*log.borrow(), vec!["dismiss", "rename"]);
  }

  #[test]
  fn a_danger_action_is_flagged() {
    let a = SheetAction::new("d", "Delete", |_, _| {}).danger();
    assert!(a.danger);
  }
}
