//! Cards: a surface for one object, with an optional header, a body and a
//! footer of actions (Bootstrap's card, MUI's outlined and elevated paper).

use gpui::{
  div, prelude::*, AnyElement, App, ClickEvent, Div, MouseButton, Role, SharedString, Window,
};

use crate::{
  text,
  theme::{self, Density, Elevation, Palette},
  zpx, OnClick, TextSize,
};

/// How a [`Card`] sets itself off from the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CardVariant {
  /// A hairline border on the card fill. The default.
  #[default]
  Outlined,
  /// No border, a raised shadow. Never inside another card.
  Elevated,
  /// A dashed border with no fill: an empty slot or a drop target.
  Ghost,
}

/// A card's title row: an optional leading icon, the title and a muted
/// description, and a trailing action such as an icon button.
pub struct CardHeader {
  title: SharedString,
  description: Option<SharedString>,
  leading: Option<AnyElement>,
  action: Option<AnyElement>,
}

impl CardHeader {
  pub fn new(title: impl Into<SharedString>) -> Self {
    Self {
      title: title.into(),
      description: None,
      leading: None,
      action: None,
    }
  }

  pub fn description(mut self, description: impl Into<SharedString>) -> Self {
    self.description = Some(description.into());
    self
  }

  pub fn leading(mut self, leading: impl IntoElement) -> Self {
    self.leading = Some(leading.into_any_element());
    self
  }

  pub fn action(mut self, action: impl IntoElement) -> Self {
    self.action = Some(action.into_any_element());
    self
  }
}

/// A card's actions. `end` holds the primary action, last; `start` holds
/// one that pulls the other way, such as Delete.
#[derive(Default)]
pub struct CardFooter {
  start: Vec<AnyElement>,
  end: Vec<AnyElement>,
}

impl CardFooter {
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

/// A bordered, rounded surface for one object. Its children are the body,
/// padded unless [`Card::flush`] (for a list or table edge to edge).
#[derive(IntoElement)]
pub struct Card {
  id: SharedString,
  palette: Palette,
  variant: CardVariant,
  header: Option<CardHeader>,
  body: Vec<AnyElement>,
  flush: bool,
  footer: Option<CardFooter>,
  selected: bool,
  label: Option<SharedString>,
  on_click: Option<OnClick>,
}

impl Card {
  pub fn new(id: impl Into<SharedString>, palette: &Palette) -> Self {
    Self {
      id: id.into(),
      palette: *palette,
      variant: CardVariant::default(),
      header: None,
      body: Vec::new(),
      flush: false,
      footer: None,
      selected: false,
      label: None,
      on_click: None,
    }
  }

  pub fn variant(mut self, variant: CardVariant) -> Self {
    self.variant = variant;
    self
  }

  pub fn header(mut self, header: CardHeader) -> Self {
    self.header = Some(header);
    self
  }

  /// Draws the body edge to edge, below a divider when there is a header.
  pub fn flush(mut self, flush: bool) -> Self {
    self.flush = flush;
    self
  }

  /// The footer, drawn only when it has actions.
  pub fn footer(mut self, footer: CardFooter) -> Self {
    self.footer = Some(footer).filter(|f| !f.start.is_empty() || !f.end.is_empty());
    self
  }

  /// A primary ring, for a card picked from a set (a plan, a template).
  pub fn selected(mut self, selected: bool) -> Self {
    self.selected = selected;
    self
  }

  /// Makes the whole card one button: a tab stop with a hover fill that
  /// Enter or Space clicks, named by its header's title.
  pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
    self.on_click = Some(std::rc::Rc::new(f));
    self
  }

  /// The accessible name when it isn't the header's title.
  pub fn label(mut self, label: impl Into<SharedString>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// Its fill, border and shadows, from the variant and selection.
  fn surface(&self) -> (Option<gpui::Hsla>, Option<gpui::Hsla>, Vec<gpui::BoxShadow>) {
    let p = &self.palette;
    let (fill, border, shadows) = match self.variant {
      CardVariant::Outlined => (Some(p.card), Some(p.border.opacity(0.5)), Vec::new()),
      CardVariant::Elevated => (Some(p.raised()), None, Elevation::Raised.shadows()),
      CardVariant::Ghost => (None, Some(p.border), Vec::new()),
    };
    if self.selected {
      (fill, Some(p.primary), shadows)
    } else {
      (fill, border, shadows)
    }
  }
}

impl ParentElement for Card {
  fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
    self.body.extend(elements);
  }
}

impl RenderOnce for Card {
  fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let p = self.palette;
    let pad = zpx(
      cx.try_global::<Density>()
        .copied()
        .unwrap_or_default()
        .card_padding(),
    );
    let (fill, border, shadows) = self.surface();
    let id = self.id.clone();
    let part = |name: &str| {
      let selector = format!("{id}-{name}");
      div().debug_selector(move || selector)
    };
    let name = self
      .label
      .clone()
      .or_else(|| self.header.as_ref().map(|h| h.title.clone()));
    let focus = self
      .on_click
      .as_ref()
      .map(|_| crate::tab_stop(SharedString::from(format!("{}-focus", self.id)), window, cx));
    let focused = focus.as_ref().is_some_and(|f| f.is_focused(window));
    let has_header = self.header.is_some();
    let has_body = !self.body.is_empty();
    let selector = self.id.to_string();

    let header = self.header.map(|h| {
      part("header")
        .flex()
        .items_start()
        .gap(zpx(theme::space::MD))
        .px(pad)
        .pt(pad)
        .when(!has_body || self.flush, |el| el.pb(pad))
        .children(h.leading)
        .child(
          div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
              text(div(), TextSize::Lg)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(h.title),
            )
            .when_some(h.description, |el, d| {
              el.child(
                text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child(d),
              )
            }),
        )
        .children(h.action)
    });
    let body: Option<Div> = has_body.then(|| {
      part("body")
        .flex()
        .flex_col()
        .when(!self.flush, |el| el.p(pad))
        .when(self.flush && has_header, |el| {
          el.border_t_1().border_color(p.border)
        })
        .children(self.body)
    });
    let footer = self.footer.map(|f| {
      let split = !f.start.is_empty();
      part("footer")
        .flex()
        .items_center()
        .gap(zpx(theme::space::SM))
        .px(pad)
        .py(zpx(theme::space::MD))
        .border_t_1()
        .border_color(p.border)
        .when(split, |el| {
          el.justify_between()
            .child(div().flex().gap(zpx(theme::space::SM)).children(f.start))
        })
        .when(!split, |el| el.justify_end())
        .child(div().flex().gap(zpx(theme::space::SM)).children(f.end))
    });

    div()
      .id(self.id)
      .debug_selector(move || selector)
      .flex()
      .flex_col()
      .min_w_0()
      .overflow_hidden()
      .rounded(zpx(theme::RADIUS_LG))
      .text_color(p.card_foreground)
      .when_some(fill, |el, fill| el.bg(fill))
      .when_some(border, |el, border| {
        el.border_1()
          .border_color(border)
          .when(self.variant == CardVariant::Ghost && !self.selected, |el| {
            el.border_dashed()
          })
      })
      .when(self.selected, |el| {
        el.shadow(vec![crate::focus_ring(&p, false)])
      })
      .when(!self.selected && !focused, |el| el.shadow(shadows))
      .when_some(focus, |el, focus| {
        el.track_focus(&focus)
          .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
      })
      .when(focused, |el| el.shadow(vec![crate::focus_ring(&p, false)]))
      .when_some(self.on_click, |el, f| {
        el.role(Role::Button)
          .when_some(name, |el, name| el.aria_label(name))
          .cursor_pointer()
          .hover(|s| s.bg(p.hover()))
          .on_click(move |event, window, cx| f(event, window, cx))
      })
      .children(header)
      .children(body)
      .children(footer)
  }
}

#[cfg(test)]
mod tests {
  use gpui::{px, TestAppContext, VisualTestContext};

  use super::*;

  #[test]
  fn variants_set_fill_border_and_shadow() {
    let p = theme::palette(false);
    let (fill, border, shadows) = Card::new("c", &p).surface();
    assert_eq!(fill, Some(p.card));
    assert_eq!(border, Some(p.border.opacity(0.5)));
    assert!(shadows.is_empty());

    let (_, border, shadows) = Card::new("c", &p).variant(CardVariant::Elevated).surface();
    assert_eq!(border, None);
    assert!(!shadows.is_empty());

    let (fill, border, _) = Card::new("c", &p).variant(CardVariant::Ghost).surface();
    assert_eq!(fill, None);
    assert_eq!(border, Some(p.border));

    let (_, border, _) = Card::new("c", &p).selected(true).surface();
    assert_eq!(border, Some(p.primary));
  }

  #[test]
  fn an_elevated_card_is_lighter_in_the_dark() {
    let dark = theme::palette(true);
    let (fill, _, _) = Card::new("c", &dark)
      .variant(CardVariant::Elevated)
      .surface();
    assert!(fill.unwrap().l > dark.card.l);
  }

  #[test]
  fn an_empty_footer_is_not_drawn() {
    let p = theme::palette(false);
    assert!(Card::new("c", &p)
      .footer(CardFooter::new())
      .footer
      .is_none());
    assert!(Card::new("c", &p)
      .footer(CardFooter::new().end(div()))
      .footer
      .is_some());
  }

  struct Host;

  impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
      let p = theme::palette(false);
      div()
        .w(px(400.))
        .flex()
        .flex_col()
        .gap_4()
        .child(
          Card::new("padded", &p)
            .header(CardHeader::new("Title"))
            .child(div().h(px(20.)).debug_selector(|| "padded-content".into()))
            .footer(CardFooter::new().end(div().w(px(40.)).h(px(28.)))),
        )
        .child(
          Card::new("flush", &p)
            .header(CardHeader::new("Title"))
            .flush(true)
            .child(div().h(px(20.)).debug_selector(|| "flush-content".into())),
        )
    }
  }

  #[gpui::test]
  fn the_body_is_padded_16_unless_flush(cx: &mut TestAppContext) {
    let window = cx.add_window(|_, _| Host);
    let mut cx = VisualTestContext::from_window(*window, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let card = cx.debug_bounds("padded").unwrap();
    let content = cx.debug_bounds("padded-content").unwrap();
    // One pixel of border, then the padding.
    assert_eq!(content.origin.x - card.origin.x, px(17.));
    let card = cx.debug_bounds("flush").unwrap();
    let content = cx.debug_bounds("flush-content").unwrap();
    assert_eq!(content.origin.x - card.origin.x, px(1.));
    let header = cx.debug_bounds("padded-header").unwrap();
    let footer = cx.debug_bounds("padded-footer").unwrap();
    assert_eq!(header.origin.x, footer.origin.x);
  }
}
