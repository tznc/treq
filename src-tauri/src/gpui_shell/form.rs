//! Form controls for GPUI screens whose values React owns.
//!
//! A control keeps its own GPUI state (text being edited, an open
//! dropdown) and reports each change to React as a
//! [`ScreenAction::Field`]. React stores it as a draft, as its own control
//! would, and the next snapshot carries the value back. A focused text
//! field keeps what is typed rather than taking the snapshot's value, which
//! can lag a keystroke behind.

use crate::gpui_shell::ui::zpx;
use std::collections::HashMap;

use gpui_kit::base::{ObservedElement, TestSupportExt as _};
use gpui_kit::component::{
  input::{InputEvent, InputState, Textarea, TextareaState},
  searchable_list::SearchableListItem,
  select::{Select, SelectEvent, SelectState},
  slider::{Slider, SliderEvent, SliderState},
  IndexPath, Sizable,
};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  accesskit::Toggled, div, prelude::*, App, Context, Div, Entity, Focusable as _, Role,
  SharedString, Stateful, Subscription, Window,
};

use super::{
  screen::{ActionSink, ScreenAction},
  theme::{self, Palette},
  ui::{self, TextSize},
};

/// One `<option>` of a `<select>`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SelectOption {
  pub value: String,
  pub label: String,
}

impl SearchableListItem for SelectOption {
  type Value = String;

  fn title(&self) -> SharedString {
    self.label.clone().into()
  }

  fn value(&self) -> &String {
    &self.value
  }
}

type Options = Vec<SelectOption>;

/// A select control's state, as [`Controls::select`] returns it.
pub type SelectStateOf = SelectState<Options>;

/// The GPUI state of a screen's controls, by field name.
pub struct Controls {
  sink: ActionSink,
  inputs: HashMap<String, Entity<InputState>>,
  textareas: HashMap<String, Entity<TextareaState>>,
  selects: HashMap<String, (Options, Entity<SelectState<Options>>)>,
  sliders: HashMap<String, Entity<SliderState>>,
  _subscriptions: Vec<Subscription>,
}

impl Controls {
  pub fn new(sink: ActionSink) -> Self {
    Self {
      sink,
      inputs: HashMap::new(),
      textareas: HashMap::new(),
      selects: HashMap::new(),
      sliders: HashMap::new(),
      _subscriptions: Vec::new(),
    }
  }

  fn send(sink: &ActionSink, field: &str, value: serde_json::Value, cx: &mut App) {
    sink(
      ScreenAction::Field {
        field: field.to_string(),
        value,
      },
      cx,
    );
  }

  /// `ui/input.tsx`: a single-line text field.
  pub fn input<V: 'static>(
    &mut self,
    field: &str,
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<V>,
  ) -> Entity<InputState> {
    self.text_input(field, value, placeholder, false, window, cx)
  }

  /// `<Input type="password">`: the text drawn as dots.
  pub fn password<V: 'static>(
    &mut self,
    field: &str,
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<V>,
  ) -> Entity<InputState> {
    self.text_input(field, value, placeholder, true, window, cx)
  }

  fn text_input<V: 'static>(
    &mut self,
    field: &str,
    value: &str,
    placeholder: &str,
    masked: bool,
    window: &mut Window,
    cx: &mut Context<V>,
  ) -> Entity<InputState> {
    if let Some(state) = self.inputs.get(field).cloned() {
      if !state.read(cx).focus_handle(cx).is_focused(window) && state.read(cx).value() != value {
        state.update(cx, |s, cx| s.set_value(value.to_string(), window, cx));
      }
      return state;
    }
    let state = cx.new(|cx| {
      InputState::new(window, cx)
        .placeholder(placeholder.to_string())
        .default_value(value.to_string())
        .masked(masked)
    });
    let (sink, name) = (self.sink.clone(), field.to_string());
    self._subscriptions.push(
      cx.subscribe(&state, move |_, state, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          let value = state.read(cx).value().to_string();
          Self::send(&sink, &name, value.into(), cx);
        }
      }),
    );
    self.inputs.insert(field.to_string(), state.clone());
    state
  }

  /// `ui/textarea.tsx` with `rows` lines.
  pub fn textarea<V: 'static>(
    &mut self,
    field: &str,
    value: &str,
    placeholder: &str,
    rows: usize,
    window: &mut Window,
    cx: &mut Context<V>,
  ) -> Entity<TextareaState> {
    if let Some(state) = self.textareas.get(field).cloned() {
      if !state.read(cx).focus_handle(cx).is_focused(window) && state.read(cx).value() != value {
        state.update(cx, |s, cx| s.set_value(value.to_string(), window, cx));
      }
      return state;
    }
    let state = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(rows)
        .placeholder(placeholder.to_string())
        .default_value(value.to_string())
    });
    let (sink, name) = (self.sink.clone(), field.to_string());
    self._subscriptions.push(
      cx.subscribe(&state, move |_, state, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          let value = state.read(cx).value().to_string();
          Self::send(&sink, &name, value.into(), cx);
        }
      }),
    );
    self.textareas.insert(field.to_string(), state.clone());
    state
  }

  /// A `<select>` of `options` showing `value`.
  pub fn select<V: 'static>(
    &mut self,
    field: &str,
    options: &[SelectOption],
    value: &str,
    window: &mut Window,
    cx: &mut Context<V>,
  ) -> Entity<SelectState<Options>> {
    let index = options
      .iter()
      .position(|o| o.value == value)
      .map(IndexPath::new);
    if let Some((known, state)) = self.selects.get_mut(field) {
      let state = state.clone();
      if known.as_slice() != options {
        *known = options.to_vec();
        state.update(cx, |s, cx| s.set_items(options.to_vec(), window, cx));
      }
      if state.read(cx).selected_value().map(String::as_str) != Some(value) {
        state.update(cx, |s, cx| match index {
          Some(_) => s.set_selected_value(&value.to_string(), window, cx),
          None => s.set_selected_index(None, window, cx),
        });
      }
      return state;
    }
    let state = cx.new(|cx| SelectState::new(options.to_vec(), index, window, cx));
    let (sink, name) = (self.sink.clone(), field.to_string());
    self._subscriptions.push(cx.subscribe(
      &state,
      move |_, _, event: &SelectEvent<Options>, cx| {
        let SelectEvent::Confirm(Some(value)) = event else {
          return;
        };
        Self::send(&sink, &name, value.clone().into(), cx);
      },
    ));
    self
      .selects
      .insert(field.to_string(), (options.to_vec(), state.clone()));
    state
  }

  /// `ui/slider.tsx`. Each move is a `Field`; the release is a `Commit`.
  #[allow(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  pub fn slider<V: 'static>(
    &mut self,
    field: &str,
    min: f32,
    max: f32,
    step: f32,
    value: f32,
    window: &mut Window,
    cx: &mut Context<V>,
  ) -> Entity<SliderState> {
    if let Some(state) = self.sliders.get(field).cloned() {
      if state.read(cx).value().start() != value {
        state.update(cx, |s, cx| s.set_value(value, window, cx));
      }
      return state;
    }
    let state = cx.new(|_| {
      SliderState::new()
        .min(min)
        .max(max)
        .step(step)
        .default_value(value)
    });
    let (sink, name) = (self.sink.clone(), field.to_string());
    self._subscriptions.push(cx.subscribe(
      &state,
      move |_, _, event: &SliderEvent, cx| match event {
        SliderEvent::Change(value) => Self::send(&sink, &name, value.start().into(), cx),
        SliderEvent::Release(value) => sink(
          ScreenAction::Commit {
            field: name.clone(),
            value: value.start().into(),
          },
          cx,
        ),
      },
    ));
    self.sliders.insert(field.to_string(), state.clone());
    state
  }
}

/// `ui/input.tsx` and a `<select>`: `h-10`.
const FIELD_HEIGHT: f32 = 40.;

/// A text field drawn like `ui/input.tsx`.
pub fn input_field(
  id: impl Into<SharedString>,
  state: &Entity<InputState>,
  mono: bool,
  disabled: bool,
) -> impl IntoElement {
  let id: SharedString = id.into();
  let selector = id.to_string();
  div()
    .id(id)
    .debug_selector(move || selector)
    .w_full()
    .child(
      // `Input::h` sets a multi-line height; a single line takes its own.
      Styled::h(ui::input(state).disabled(disabled), zpx(FIELD_HEIGHT))
        .when(mono, |this| this.font_family(theme::MONO_FONT)),
    )
}

/// A multi-line field drawn like `ui/textarea.tsx`.
pub fn textarea_field(
  state: &Entity<TextareaState>,
  mono: bool,
  disabled: bool,
) -> impl IntoElement {
  ui::text(Textarea::new(state), TextSize::Sm)
    .disabled(disabled)
    .when(mono, |this| this.font_family(theme::MONO_FONT))
}

/// A `<select>`: `mt-2 w-full px-3 py-2 border rounded-md`.
pub fn select_field(
  id: impl Into<SharedString>,
  name: impl Into<SharedString>,
  state: &Entity<SelectState<Options>>,
  disabled: bool,
) -> impl IntoElement {
  let id: SharedString = id.into();
  let selector = id.to_string();
  div()
    .id(id.clone())
    .debug_selector(move || selector)
    .w_full()
    .child(
      Select::new(state)
        .id(id)
        .accessibility_label(name)
        .disabled(disabled)
        .h(zpx(FIELD_HEIGHT)),
    )
}

const SMALL_FIELD_HEIGHT: f32 = 32.;

/// `select_field` at a small button's height, for a control inside a row.
pub fn small_select_field(
  id: impl Into<SharedString>,
  name: impl Into<SharedString>,
  state: &Entity<SelectState<Options>>,
  disabled: bool,
) -> impl IntoElement {
  let id: SharedString = id.into();
  let selector = id.to_string();
  div()
    .id(id.clone())
    .debug_selector(move || selector)
    .w_full()
    .child(
      Select::new(state)
        .id(id)
        .accessibility_label(name)
        .disabled(disabled)
        .small()
        .h(zpx(SMALL_FIELD_HEIGHT)),
    )
}

/// A slider: `h-2` track in `--secondary`, `--primary` fill.
pub fn slider_field(state: &Entity<SliderState>) -> impl IntoElement {
  Slider::new(state)
}

/// A switch, observed for UI tests. Its name is React's `<Switch aria-label>`
/// or its `<Label htmlFor>`.
pub type Switch = ObservedElement<Stateful<Div>>;

/// `ui/switch.tsx`: a 36×20 pill with a 16px thumb.
pub fn switch(
  id: impl Into<SharedString>,
  name: impl Into<SharedString>,
  checked: bool,
  disabled: bool,
  palette: &Palette,
  on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Switch {
  switch_sized(
    id,
    name,
    checked,
    disabled,
    palette,
    (36., 20., 16.),
    move |value: &bool, window: &mut Window, cx: &mut App| on_change(*value, window, cx),
  )
}

/// A switch `(width, height, thumb)` pixels, as a `className` resizes it.
pub fn switch_sized(
  id: impl Into<SharedString>,
  name: impl Into<SharedString>,
  checked: bool,
  disabled: bool,
  palette: &Palette,
  (width, height, thumb): (f32, f32, f32),
  on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> Switch {
  let id: SharedString = id.into();
  let selector = id.to_string();
  div()
    .id(id)
    .debug_selector(move || selector)
    .role(Role::Switch)
    .aria_label(name)
    .aria_toggled(if checked {
      Toggled::True
    } else {
      Toggled::False
    })
    .flex()
    .flex_none()
    .items_center()
    .w(zpx(width))
    .h(zpx(height))
    .rounded_full()
    .border_2()
    .border_color(gpui_kit::transparent_black())
    .shadow_sm()
    .bg(if checked {
      palette.primary
    } else {
      palette.input
    })
    .when(disabled, |this| this.opacity(0.5))
    .child(
      div()
        .size(zpx(thumb))
        .rounded_full()
        .bg(palette.background)
        .shadow_lg()
        .when(checked, |this| this.ml(zpx(width - 4. - thumb))),
    )
    .when(!disabled, |this| {
      this
        .cursor_pointer()
        .on_click(move |_, window, cx| on_change(&!checked, window, cx))
    })
    .test_support()
}
