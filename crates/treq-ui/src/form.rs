//! Forms: labelled fields laid out vertically, horizontally (label column
//! beside the control, Bootstrap's horizontal form), inline, or mixed with
//! multi-column rows and titled sections.

use gpui::{div, prelude::*, AnyElement, App, Div, FontWeight, SharedString, Window};

use crate::{
  text,
  theme::{self, space, Palette},
  zoomed, zpx, TextSize,
};

/// Where a [`Field`] puts its label.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum FieldLayout {
  /// Label above the control. The default.
  #[default]
  Vertical,
  /// Label in a column `label_width` wide, left of the control. Stacks
  /// vertically in a window narrower than [`theme::STACK_BREAKPOINT`].
  Horizontal { label_width: f32 },
  /// Label and hint on the left, the control at the end of the row: a
  /// setting in a list.
  Trailing,
}

impl FieldLayout {
  /// Horizontal with the default label column.
  pub fn horizontal() -> Self {
    FieldLayout::Horizontal {
      label_width: theme::LABEL_WIDTH,
    }
  }

  /// What it draws as in a window `window_width` wide (both in pixels).
  pub fn at_width(self, window_width: f32, breakpoint: f32) -> Self {
    match self {
      FieldLayout::Horizontal { .. } if window_width < breakpoint => FieldLayout::Vertical,
      other => other,
    }
  }
}

/// A label, its control, and a hint or an error under the control. The
/// error replaces the hint. The label, hint and error are all body text, so
/// they line up with the text in the control.
#[derive(IntoElement)]
pub struct Field {
  id: SharedString,
  palette: Palette,
  label: Option<SharedString>,
  required: bool,
  optional: bool,
  hint: Option<SharedString>,
  error: Option<SharedString>,
  layout: Option<FieldLayout>,
  control: Vec<AnyElement>,
}

impl Field {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      label: None,
      required: false,
      optional: false,
      hint: None,
      error: None,
      layout: None,
      control: Vec::new(),
    }
  }

  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// Marks the label with a red asterisk.
  pub fn required(mut self) -> Self {
    self.required = true;
    self
  }

  /// Adds a muted "(optional)", for the few optional fields of a form that
  /// is mostly required.
  pub fn optional(mut self) -> Self {
    self.optional = true;
    self
  }

  pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
    self.hint = Some(hint.into());
    self
  }

  /// What is wrong and how to fix it. Replaces the hint while set.
  pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
    self.error = error.map(Into::into);
    self
  }

  /// Its own layout, over the one its [`Form`] gives it.
  pub fn layout(mut self, layout: FieldLayout) -> Self {
    self.layout = Some(layout);
    self
  }

  fn inherit(mut self, layout: FieldLayout) -> Self {
    self.layout.get_or_insert(layout);
    self
  }

  /// The text under the control: the error, else the hint.
  fn message(&self) -> Option<(SharedString, bool)> {
    match (&self.error, &self.hint) {
      (Some(e), _) => Some((e.clone(), true)),
      (None, Some(h)) => Some((h.clone(), false)),
      _ => None,
    }
  }
}

impl ParentElement for Field {
  fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
    self.control.extend(elements);
  }
}

fn part(id: &SharedString, name: &str) -> Div {
  let selector = format!("{id}-{name}");
  div().debug_selector(move || selector)
}

impl RenderOnce for Field {
  fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let width = f32::from(window.viewport_size().width);
    let breakpoint = f32::from(zoomed(theme::STACK_BREAKPOINT, window));
    let layout = self.layout.unwrap_or_default().at_width(width, breakpoint);
    let message = self.message().map(|(m, error)| {
      part(&self.id, "message")
        .id(SharedString::from(format!("{}-message", self.id)))
        .map(|el| text(el, TextSize::Sm))
        .text_color(if error {
          p.destructive
        } else {
          p.muted_foreground
        })
        .when(error, |el| el.role(gpui::Role::Alert))
        .child(m)
    });
    let label = self.label.map(|l| {
      part(&self.id, "label")
        .map(|el| text(el, TextSize::Sm))
        .font_weight(FontWeight::MEDIUM)
        .flex()
        .gap_1()
        .child(l)
        .when(self.required, |el| {
          el.child(div().text_color(p.destructive).child("*"))
        })
        .when(self.optional, |el| {
          el.child(
            div()
              .font_weight(FontWeight::NORMAL)
              .text_color(p.muted_foreground)
              .child("(optional)"),
          )
        })
    });
    let gap = zpx(theme::LABEL_GAP);
    let control = part(&self.id, "control")
      .flex()
      .flex_col()
      .gap(gap)
      .min_w_0()
      .children(self.control);
    let selector = self.id.to_string();
    let root = div().debug_selector(move || selector).min_w_0();
    match layout {
      FieldLayout::Vertical => root
        .flex()
        .flex_col()
        .gap(gap)
        .children(label)
        .child(control)
        .children(message),
      FieldLayout::Horizontal { label_width } => root
        .flex()
        .items_start()
        .gap(zpx(space::LG))
        // Top-aligned to the control's first line of text: (28 - 20) / 2.
        .child(
          div()
            .flex_none()
            .w(zpx(label_width))
            .pt(zpx((theme::CONTROL_MD - 20.) / 2.))
            .children(label),
        )
        .child(
          div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(gap)
            .child(control)
            .children(message),
        ),
      FieldLayout::Trailing => root
        .flex()
        .items_center()
        .justify_between()
        .gap(zpx(space::LG))
        .child(
          div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .children(label)
            .children(message),
        )
        .child(control.flex_none()),
    }
  }
}

/// A row of fields side by side, each `span` columns wide (Bootstrap's
/// `.row` of `.col-*`). Its fields are vertical whatever the form's layout.
#[derive(Default)]
pub struct FormRow {
  cols: Vec<(u16, Field)>,
}

impl FormRow {
  pub fn new() -> Self {
    Self::default()
  }

  /// One equal column.
  pub fn col(self, field: Field) -> Self {
    self.span(1, field)
  }

  /// A column `span` times as wide as a [`FormRow::col`].
  pub fn span(mut self, span: u16, field: Field) -> Self {
    self.cols.push((span.max(1), field));
    self
  }
}

/// A titled group of fields: a fieldset with a legend.
pub struct FormSection {
  title: SharedString,
  description: Option<SharedString>,
  items: Vec<Item>,
}

impl FormSection {
  pub fn new(title: impl Into<SharedString>) -> Self {
    Self {
      title: title.into(),
      description: None,
      items: Vec::new(),
    }
  }

  pub fn description(mut self, description: impl Into<SharedString>) -> Self {
    self.description = Some(description.into());
    self
  }

  pub fn field(mut self, field: Field) -> Self {
    self.items.push(Item::Field(field));
    self
  }

  pub fn row(mut self, row: FormRow) -> Self {
    self.items.push(Item::Row(row));
    self
  }

  /// Anything else in the flow, such as an inline group or a checkbox.
  pub fn child(mut self, child: impl IntoElement) -> Self {
    self.items.push(Item::Other(child.into_any_element()));
    self
  }
}

/// A form's buttons. `end` holds the submit, last; `start` one that pulls
/// the other way.
#[derive(Default)]
pub struct FormActions {
  start: Vec<AnyElement>,
  end: Vec<AnyElement>,
}

impl FormActions {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn start(mut self, action: impl IntoElement) -> Self {
    self.start.push(action.into_any_element());
    self
  }

  pub fn end(mut self, action: impl IntoElement) -> Self {
    self.end.push(action.into_any_element());
    self
  }
}

/// How a [`Form`] lays out its fields.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum FormLayout {
  #[default]
  Vertical,
  Horizontal {
    label_width: f32,
  },
  /// Fields side by side, wrapping, with their controls' bottoms aligned: a
  /// filter bar.
  Inline,
}

impl FormLayout {
  pub fn horizontal() -> Self {
    FormLayout::Horizontal {
      label_width: theme::LABEL_WIDTH,
    }
  }

  fn field_layout(self) -> FieldLayout {
    match self {
      FormLayout::Horizontal { label_width } => FieldLayout::Horizontal { label_width },
      _ => FieldLayout::Vertical,
    }
  }
}

#[allow(clippy::large_enum_variant)] // Built per frame, never stored.
enum Item {
  Field(Field),
  Row(FormRow),
  Section(FormSection),
  Other(AnyElement),
}

/// Fields, rows and sections, 16 apart, then the actions. It only arranges:
/// each [`Field`] draws itself and works on its own.
#[derive(IntoElement)]
pub struct Form {
  id: SharedString,
  palette: Palette,
  layout: FormLayout,
  items: Vec<Item>,
  actions: Option<FormActions>,
}

impl Form {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      layout: FormLayout::default(),
      items: Vec::new(),
      actions: None,
    }
  }

  pub fn layout(mut self, layout: FormLayout) -> Self {
    self.layout = layout;
    self
  }

  pub fn field(mut self, field: Field) -> Self {
    self.items.push(Item::Field(field));
    self
  }

  pub fn row(mut self, row: FormRow) -> Self {
    self.items.push(Item::Row(row));
    self
  }

  pub fn section(mut self, section: FormSection) -> Self {
    self.items.push(Item::Section(section));
    self
  }

  /// Anything else in the flow, such as a checkbox.
  pub fn child(mut self, child: impl IntoElement) -> Self {
    self.items.push(Item::Other(child.into_any_element()));
    self
  }

  pub fn actions(mut self, actions: FormActions) -> Self {
    self.actions = Some(actions);
    self
  }
}

fn render_row(row: FormRow) -> Div {
  div()
    .flex()
    .gap(zpx(space::LG))
    .children(row.cols.into_iter().map(|(span, field)| {
      div()
        .flex_grow(span as f32)
        .flex_shrink(1.)
        .flex_basis(gpui::relative(0.))
        .min_w_0()
        .child(field.inherit(FieldLayout::Vertical))
    }))
}

fn render_items(items: Vec<Item>, layout: FieldLayout, p: &Palette) -> Vec<AnyElement> {
  items
    .into_iter()
    .map(|item| match item {
      Item::Field(f) => f.inherit(layout).into_any_element(),
      Item::Row(r) => render_row(r).into_any_element(),
      Item::Other(e) => e,
      Item::Section(s) => div()
        .flex()
        .flex_col()
        .gap(zpx(space::MD))
        .child(
          div()
            .flex()
            .flex_col()
            .child(
              text(div().id(s.title.clone()), TextSize::Sm)
                .role(gpui::Role::Heading)
                .font_weight(FontWeight::SEMIBOLD)
                .child(s.title),
            )
            .when_some(s.description, |el, d| {
              el.child(
                text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child(d),
              )
            }),
        )
        .children(render_items(s.items, layout, p))
        .into_any_element(),
    })
    .collect()
}

impl RenderOnce for Form {
  fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
    let p = self.palette;
    let field_layout = self.layout.field_layout();
    let selector = self.id.to_string();
    let inline = self.layout == FormLayout::Inline;
    // Actions line up with the controls, past the label column, unless the
    // labels have stacked.
    let indent = match field_layout.at_width(
      f32::from(window.viewport_size().width),
      f32::from(zoomed(theme::STACK_BREAKPOINT, window)),
    ) {
      FieldLayout::Horizontal { label_width } => Some(label_width + space::LG),
      _ => None,
    };
    let actions = self.actions.map(|a| {
      let split = !a.start.is_empty();
      part(&self.id, "actions")
        .flex()
        .items_center()
        .gap(zpx(space::SM))
        .when_some(indent, |el, i| el.pl(zpx(i)))
        .when(split, |el| {
          el.child(div().flex().gap(zpx(space::SM)).children(a.start))
        })
        .map(|el| match (split, indent) {
          (true, _) => el.justify_between(),
          (false, Some(_)) => el.justify_start(),
          (false, None) => el.justify_end(),
        })
        .child(div().flex().gap(zpx(space::SM)).children(a.end))
    });
    let items = render_items(self.items, field_layout, &p);
    div()
      .debug_selector(move || selector)
      .flex()
      .when(inline, |el| el.flex_wrap().items_end())
      .when(!inline, |el| el.flex_col())
      .gap(zpx(theme::FIELD_GAP))
      .children(items)
      .children(actions)
  }
}

#[cfg(test)]
mod tests {
  use gpui::{px, size, TestAppContext, VisualTestContext};

  use super::*;

  #[test]
  fn horizontal_stacks_below_the_breakpoint() {
    let h = FieldLayout::horizontal();
    assert_eq!(h.at_width(800., 480.), h);
    assert_eq!(h.at_width(400., 480.), FieldLayout::Vertical);
    assert_eq!(
      FieldLayout::Trailing.at_width(300., 480.),
      FieldLayout::Trailing
    );
  }

  #[test]
  fn the_error_replaces_the_hint() {
    let p = theme::palette(false);
    let f = Field::new("f", &p).hint("Becomes the branch name.");
    assert_eq!(
      f.message(),
      Some(("Becomes the branch name.".into(), false))
    );
    let f = f.error(Some("Use a full URL."));
    assert_eq!(f.message(), Some(("Use a full URL.".into(), true)));
    let f = Field::new("f", &p).error(None::<&str>);
    assert_eq!(f.message(), None);
  }

  #[test]
  fn a_field_keeps_its_own_layout_over_the_forms() {
    let p = theme::palette(false);
    let f = Field::new("f", &p).layout(FieldLayout::Trailing);
    assert_eq!(
      f.inherit(FieldLayout::Vertical).layout,
      Some(FieldLayout::Trailing)
    );
    let f = Field::new("f", &p).inherit(FieldLayout::horizontal());
    assert_eq!(f.layout, Some(FieldLayout::horizontal()));
  }

  fn control(id: &'static str) -> Div {
    div().h(px(28.)).w_full().debug_selector(move || id.into())
  }

  struct Host(FormLayout);

  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      div().w(px(600.)).child(
        Form::new("form", &p)
          .layout(self.0)
          .field(
            Field::new("name", &p)
              .label("Name")
              .hint("Becomes the branch name.")
              .child(control("name-input")),
          )
          .row(
            FormRow::new()
              .span(
                2,
                Field::new("path", &p)
                  .label("Path")
                  .child(control("path-input")),
              )
              .col(
                Field::new("depth", &p)
                  .label("Depth")
                  .child(control("depth-input")),
              ),
          )
          .actions(
            FormActions::new().end(
              div()
                .w(px(60.))
                .h(px(28.))
                .debug_selector(|| "submit".into()),
            ),
          ),
      )
    }
  }

  fn mount(cx: &mut TestAppContext, layout: FormLayout, width: f32) -> VisualTestContext {
    let window = cx.add_window(move |_, _| Host(layout));
    let mut cx = VisualTestContext::from_window(*window, cx);
    cx.simulate_resize(size(px(width), px(600.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx
  }

  #[gpui::test]
  fn vertical_puts_the_label_above_and_the_hint_below(cx: &mut TestAppContext) {
    let mut cx = mount(cx, FormLayout::Vertical, 800.);
    let label = cx.debug_bounds("name-label").unwrap();
    let input = cx.debug_bounds("name-input").unwrap();
    let hint = cx.debug_bounds("name-message").unwrap();
    assert_eq!(label.origin.x, input.origin.x);
    assert_eq!(input.origin.y - label.bottom(), px(6.));
    assert_eq!(hint.origin.x, input.origin.x);
    assert_eq!(hint.origin.y - input.bottom(), px(6.));
    // Hint, label and control text share the body size.
    assert_eq!(hint.size.height, label.size.height);
  }

  #[gpui::test]
  fn horizontal_puts_the_label_in_a_140_column(cx: &mut TestAppContext) {
    let mut cx = mount(cx, FormLayout::horizontal(), 800.);
    let form = cx.debug_bounds("form").unwrap();
    let label = cx.debug_bounds("name-label").unwrap();
    let input = cx.debug_bounds("name-input").unwrap();
    let hint = cx.debug_bounds("name-message").unwrap();
    assert_eq!(input.origin.x - form.origin.x, px(156.));
    // The label's text sits on the control's text line.
    assert_eq!(label.origin.y - input.origin.y, px(4.));
    assert_eq!(hint.origin.x, input.origin.x);
    // The actions line up with the controls.
    let submit = cx.debug_bounds("submit").unwrap();
    assert_eq!(submit.origin.x, input.origin.x);
  }

  #[gpui::test]
  fn horizontal_stacks_in_a_narrow_window(cx: &mut TestAppContext) {
    let mut cx = mount(cx, FormLayout::horizontal(), 400.);
    let label = cx.debug_bounds("name-label").unwrap();
    let input = cx.debug_bounds("name-input").unwrap();
    assert_eq!(label.origin.x, input.origin.x);
    assert!(input.origin.y > label.bottom());
  }

  #[gpui::test]
  fn a_row_splits_its_width_by_span(cx: &mut TestAppContext) {
    let mut cx = mount(cx, FormLayout::horizontal(), 800.);
    let path = cx.debug_bounds("path-input").unwrap();
    let depth = cx.debug_bounds("depth-input").unwrap();
    // 600 less one 16 gap, split 2:1; rows stay vertical in any layout.
    assert!((path.size.width - depth.size.width * 2.).abs() <= px(1.));
    assert!((path.size.width + depth.size.width - px(584.)).abs() <= px(1.));
    assert_eq!(path.origin.y, depth.origin.y);
  }
}
