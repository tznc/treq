//! The workspace header's popovers (React's `PopoverContent` under the
//! Details button, `TargetBranchSelector` and `ChecksPopover`).
//!
//! The workspace view draws the popover over the whole window, on an
//! in-window backdrop, so a click anywhere outside the popover closes it.
//! The popover reads what it lists from the backend itself and says it is
//! done with a [`Closed`] event. What the window model does (retargeting
//! the workspace, running workflows, the prompt history dialog) goes back as
//! screen actions.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::input::{
  Input, InputEvent, InputState, MoveEnd, MoveHome, MoveToEnd, MoveToStart,
};
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, px, Bounds, Context, Entity, EventEmitter, FocusHandle, KeyDownEvent,
  MouseButton, MouseDownEvent, Pixels, Subscription, Task, Window,
};

use super::{
  overlay::cmdk,
  screen::{ActionSink, ScreenAction},
  theme::{self, Palette},
  ui,
};

mod checks;
pub mod data;
mod details;
mod target;
#[cfg(test)]
mod tests;

pub use checks::Input as ChecksInput;
pub use data::PopoverHost;
pub use details::Spec as DetailsSpec;
pub use target::Spec as TargetSpec;

/// The popover's `sideOffset`.
const SIDE_OFFSET: f32 = 4.;
/// How close to the window's edge a popover may come (base-ui's collision
/// padding).
const EDGE_PADDING: f32 = 5.;

/// Which edge of its button a popover lines up with (`align`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
  Start,
  End,
}

/// Which popover opens, and what it needs to know of its workspace.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
  Details(details::Spec),
  Target(target::Spec),
  Checks(checks::Input),
}

impl Kind {
  fn align(&self) -> Align {
    match self {
      Kind::Target(_) | Kind::Details(_) => Align::Start,
      Kind::Checks(_) => Align::End,
    }
  }
}

/// The popover asks to be closed.
pub struct Closed;

enum Content {
  Details(details::State),
  Target(target::State),
  Checks(checks::State),
}

/// The open popover: its button, what it lists and what it selected.
pub struct Popover {
  host: Arc<dyn PopoverHost>,
  /// The window model, for what the popover does not do itself.
  react: ActionSink,
  dark: bool,
  /// The button it opened from, in window coordinates.
  anchor: Bounds<Pixels>,
  align: Align,
  focus: FocusHandle,
  focused: bool,
  content: Content,
  _load: Option<Task<()>>,
}

impl EventEmitter<Closed> for Popover {}

impl Popover {
  pub fn new(
    kind: Kind,
    anchor: Bounds<Pixels>,
    dark: bool,
    host: Arc<dyn PopoverHost>,
    react: ActionSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let align = kind.align();
    let content = match kind {
      Kind::Details(spec) => Content::Details(details::State::new(spec)),
      Kind::Target(spec) => Content::Target(target::State::new(spec)),
      Kind::Checks(input) => Content::Checks(checks::State::new(input)),
    };
    let mut this = Self {
      host,
      react,
      dark,
      anchor,
      align,
      focus: cx.focus_handle(),
      focused: false,
      content,
      _load: None,
    };
    this.load(cx);
    this
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  /// The checks popover takes the workspace view's next state.
  pub fn set_checks(&mut self, input: ChecksInput, cx: &mut Context<Self>) {
    if let Content::Checks(state) = &mut self.content {
      state.set_input(input);
      cx.notify();
    }
  }

  fn send(&self, action: ScreenAction, cx: &mut gpui::App) {
    (self.react)(action, cx);
  }

  fn press(&self, id: &str, cx: &mut gpui::App) {
    self.send(ScreenAction::Press { id: id.into() }, cx);
  }

  fn field(&self, field: &str, value: serde_json::Value, cx: &mut gpui::App) {
    self.send(
      ScreenAction::Field {
        field: field.into(),
        value,
      },
      cx,
    );
  }

  pub fn close(&mut self, cx: &mut Context<Self>) {
    cx.emit(Closed);
  }

  /// Reads what the popover lists, off the main thread.
  fn load(&mut self, cx: &mut Context<Self>) {
    match &self.content {
      Content::Details(_) => self.load_details(cx),
      Content::Target(_) => self.load_target(cx),
      Content::Checks(_) => self.load_checks(cx),
    }
  }

  fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
    if crate::gpui_shell::ui::is_escape(&event.keystroke) {
      self.close(cx);
      cx.stop_propagation();
      return;
    }
    if matches!(self.content, Content::Target(_)) {
      self.target_key_down(event, cx);
    }
  }

  /// The popover's box: `rounded-md border bg-popover p-4 shadow-md`, `width`
  /// wide, `padding` inside.
  fn panel(
    &self,
    id: &'static str,
    width: f32,
    padding: f32,
    p: &Palette,
  ) -> gpui::Stateful<gpui::Div> {
    div()
      .id(id)
      .debug_selector(move || id.into())
      .w(zpx(width))
      .p(zpx(padding))
      .rounded(radius::MD)
      .border_1()
      .border_color(p.border)
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_md()
  }

  /// Where the popover's top left corner sits for a `width`-wide box in a
  /// window of `viewport`: below the button, lined up with it, and kept
  /// inside the window.
  fn origin(&self, width: f32, viewport: gpui::Size<Pixels>) -> gpui::Point<Pixels> {
    let anchor = self.anchor;
    let left = match self.align {
      Align::Start => f32::from(anchor.origin.x),
      Align::End => f32::from(anchor.right()) - width,
    };
    let most = (f32::from(viewport.width) - width - EDGE_PADDING).max(EDGE_PADDING);
    gpui::point(
      px(left.clamp(EDGE_PADDING, most)),
      anchor.bottom() + px(SIDE_OFFSET),
    )
  }
}

impl Render for Popover {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let viewport = window.viewport_size();
    if !self.focused && !matches!(self.content, Content::Target(_)) {
      self.focused = true;
      self.focus.focus(window, cx);
    }
    let (width, body) = match self.content {
      Content::Details(_) => (
        details::WIDTH,
        self.render_details(&p, cx).into_any_element(),
      ),
      Content::Target(_) => (
        target::WIDTH,
        self.render_target(&p, window, cx).into_any_element(),
      ),
      Content::Checks(_) => (checks::WIDTH, self.render_checks(&p, cx).into_any_element()),
    };
    let origin = self.origin(width, viewport);
    // A press anywhere outside the popover, its own button included,
    // only closes it.
    let close =
      |this: &mut Self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>| this.close(cx);
    div()
      .id("header-popover-backdrop")
      .debug_selector(|| "header-popover-backdrop".into())
      .track_focus(&self.focus)
      // The workspace view places it at the window's origin; it covers the
      // window.
      .w(viewport.width)
      .h(viewport.height)
      // What lies under the backdrop gets no press while the popover shows.
      .occlude()
      .font_family(theme::SANS_FONT)
      .text_color(p.foreground)
      .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key_down(event, cx)))
      // Home and End reach the search field as its own actions.
      .capture_action(cx.listener(|this, _: &MoveHome, _, cx| {
        this.target_step(cmdk::Move::First, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|this, _: &MoveEnd, _, cx| {
        this.target_step(cmdk::Move::Last, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|this, _: &MoveToStart, _, cx| {
        this.target_step(cmdk::Move::First, cx);
        cx.stop_propagation();
      }))
      .capture_action(cx.listener(|this, _: &MoveToEnd, _, cx| {
        this.target_step(cmdk::Move::Last, cx);
        cx.stop_propagation();
      }))
      .on_mouse_down(MouseButton::Left, cx.listener(close))
      .on_mouse_down(MouseButton::Right, cx.listener(close))
      .child(
        div()
          .id("header-popover")
          .role(gpui::Role::Dialog)
          .test_support()
          .absolute()
          .left(origin.x)
          .top(origin.y)
          .occlude()
          // A press inside is not an outside press.
          .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
          .child(body),
      )
  }
}

/// Keeps a [`Subscription`] and the search field it belongs to together.
struct SearchField {
  input: Entity<InputState>,
  _subscription: Subscription,
}

impl SearchField {
  fn new(
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<Popover>,
    on_change: impl Fn(&mut Popover, String, &mut Context<Popover>) + 'static,
  ) -> Self {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    input.update(cx, |input, cx| input.focus(window, cx));
    let subscription = cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        on_change(this, input.read(cx).value().to_string(), cx);
      }
    });
    Self {
      input,
      _subscription: subscription,
    }
  }

  fn element(&self) -> Input {
    ui::input(&self.input).appearance(false).bordered(false)
  }
}
