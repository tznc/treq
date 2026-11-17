//! A terminal grid bound to one remote PTY session.
//!
//! Output from treq's `RemotePtyManager` feeds a `vt100` parser; each screen
//! row renders as styled text. The view registers itself as GPUI's text
//! input handler, so focusing it brings up the soft keyboard and typed text
//! (including IME commits) is written to the PTY. Named keys and Ctrl/Alt
//! combinations are encoded in `on_key_down`.

use crate::result_ext::ResultExt as _;
use std::{cell::Cell, ops::Range};

use futures::{channel::mpsc, StreamExt};
use gpui_kit::{
  canvas, component::ActiveTheme, div, px, size, App, Bounds, Context, ElementInputHandler,
  EntityInputHandler, FocusHandle, Focusable, Font, FontWeight, HighlightStyle, Hsla,
  InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Point, Render,
  ScrollDelta, ScrollWheelEvent, Size, Styled, StyledText, Task, UTF16Selection, UnderlineStyle,
  Window,
};

use crate::mobile::{backend, terminal};

const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 17.;
const SCROLLBACK: usize = 5000;

/// vt100 sizes are non-zero; a zero-sized view gets one row or column.
fn nonzero(value: u16) -> std::num::NonZeroU16 {
  std::num::NonZeroU16::new(value).unwrap_or(std::num::NonZeroU16::MIN)
}

thread_local! {
  /// The grid size the last terminal measured, in columns and rows.
  static LAST_SIZE: Cell<Option<(u16, u16)>> = const { Cell::new(None) };
}

/// The size the last terminal measured, so the next session starts at it
/// instead of resizing (and the shell reprinting its prompt) once shown.
pub fn last_size() -> Option<(u16, u16)> {
  LAST_SIZE.with(Cell::get)
}

/// What the PTY channel reports to the view.
pub enum PtyEvent {
  Output(Vec<u8>),
  Exited(Option<u32>),
}

/// Called once with the exit status when the remote channel ends.
type ExitHandler = Box<dyn FnOnce(Option<u32>, &mut Window, &mut App)>;

pub struct TerminalView {
  session_id: String,
  parser: vt100::Parser,
  focus: FocusHandle,
  size: (u16, u16),
  cell: Option<Size<Pixels>>,
  /// The part of a line the finger has moved but the view has not scrolled.
  scroll_remainder: f32,
  /// Set once the remote channel ended.
  pub exit: Option<Option<u32>>,
  on_exit: Option<ExitHandler>,
  _reader: Task<()>,
}

impl TerminalView {
  /// `events` receives the channel's output; `session_id` addresses it in
  /// the PTY manager for writes and resizes.
  pub fn new(
    session_id: String,
    (cols, rows): (u16, u16),
    mut events: mpsc::UnboundedReceiver<PtyEvent>,
    on_exit: impl FnOnce(Option<u32>, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let reader = cx.spawn_in(window, async move |this, cx| {
      while let Some(event) = events.next().await {
        // Coalesce whatever else already arrived into one update.
        let mut batch = vec![event];
        while let Ok(event) = events.try_recv() {
          batch.push(event);
        }
        let alive = this.update_in(cx, |view, window, cx| {
          for event in batch {
            match event {
              PtyEvent::Output(bytes) => view.parser.process(&bytes),
              PtyEvent::Exited(status) => {
                view.exit = Some(status);
                if let Some(on_exit) = view.on_exit.take() {
                  let window_handle = window.window_handle();
                  cx.defer(move |cx| {
                    window_handle
                      .update(cx, |_, window, cx| on_exit(status, window, cx))
                      .ok();
                  });
                }
              }
            }
          }
          cx.notify();
        });
        if alive.is_err() {
          break;
        }
      }
    });
    let focus = cx.focus_handle();
    window.focus(&focus, cx);
    Self {
      session_id,
      parser: vt100::Parser::new(nonzero(rows), nonzero(cols), SCROLLBACK),
      focus,
      size: (cols, rows),
      cell: None,
      scroll_remainder: 0.,
      exit: None,
      on_exit: Some(Box::new(on_exit)),
      _reader: reader,
    }
  }

  /// How many lines the view is scrolled back (measurement tests).
  #[cfg(test)]
  pub(crate) fn scrollback_offset(&self) -> usize {
    self.parser.screen().scrollback()
  }

  /// Whether the program asked for application cursor keys (DECCKM),
  /// which changes what the arrow keys send.
  pub fn application_cursor(&self) -> bool {
    self.parser.screen().application_cursor()
  }

  pub fn focus(&self, window: &mut Window, cx: &mut App) {
    window.focus(&self.focus, cx);
  }

  /// Writes `bytes` to the remote PTY and scrolls back to the live screen.
  pub fn write(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
    if self.exit.is_some() {
      return;
    }
    if self.parser.screen().scrollback() > 0 {
      self.parser.screen_mut().set_scrollback(0);
      self.scroll_remainder = 0.;
      cx.notify();
    }
    let id = self.session_id.clone();
    backend::spawn_detached(async move {
      if let Err(error) = backend::get().ptys.write(&id, &bytes).await {
        log::warn!("terminal write failed: {error}");
      }
    });
  }

  fn resize(&mut self, bounds: Size<Pixels>, cell: Size<Pixels>, cx: &mut Context<Self>) {
    let cols = (f32::from(bounds.width) / f32::from(cell.width))
      .floor()
      .max(10.) as u16;
    let rows = (f32::from(bounds.height) / LINE_HEIGHT).floor().max(4.) as u16;
    self.cell = Some(cell);
    LAST_SIZE.with(|last| last.set(Some((cols, rows))));
    if (cols, rows) == self.size {
      return;
    }
    self.size = (cols, rows);
    self
      .parser
      .screen_mut()
      .set_size(nonzero(rows), nonzero(cols));
    let id = self.session_id.clone();
    backend::spawn_detached(async move {
      backend::get()
        .ptys
        .resize(&id, cols, rows)
        .await
        .best_effort();
    });
    cx.notify();
  }

  fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
    let keystroke = &event.keystroke;
    let application_cursor = self.parser.screen().application_cursor();
    let modifiers = keystroke.modifiers;
    if let Some(bytes) = terminal::key_bytes(
      &keystroke.key,
      modifiers.control,
      modifiers.alt,
      application_cursor,
    ) {
      self.write(bytes, cx);
      cx.stop_propagation();
    }
  }

  fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
    let lines = match event.delta {
      ScrollDelta::Lines(delta) => delta.y,
      ScrollDelta::Pixels(delta) => f32::from(delta.y) / LINE_HEIGHT,
    };
    let current = self.parser.screen().scrollback();
    let target = scrolled_to(current, lines, &mut self.scroll_remainder);
    self.parser.screen_mut().set_scrollback(target);
    cx.notify();
  }
}

/// The scrollback after moving `lines` (positive scrolls back) from
/// `current`. Touch drags arrive as many small pixel deltas, so the
/// fraction of a line left over is carried in `remainder` to the next event.
fn scrolled_to(current: usize, lines: f32, remainder: &mut f32) -> usize {
  let moved = *remainder + lines;
  let whole = moved.trunc();
  *remainder = moved - whole;
  let target = current as f32 + whole;
  if moved < 0. && target <= 0. {
    // At the live screen: nothing to carry back the other way.
    *remainder = 0.;
    return 0;
  }
  target as usize
}

impl Drop for TerminalView {
  fn drop(&mut self) {
    // Detach: the remote session keeps running and can be reattached.
    let id = self.session_id.clone();
    if backend::try_get().is_some() {
      backend::spawn_detached(async move {
        backend::get().ptys.close(&id).await.best_effort();
      });
    }
  }
}

fn color(rgb: u32) -> Hsla {
  gpui_kit::rgb(rgb).into()
}

impl Render for TerminalView {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let (cols, rows) = self.size;
    let screen = self.parser.screen();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let show_cursor = !screen.hide_cursor() && screen.scrollback() == 0 && self.exit.is_none();
    let mono = cx.theme().mono_font_family.clone();
    let lines = (0..rows).map(|row| {
      let cursor = (show_cursor && row == cursor_row).then_some(cursor_col);
      let terminal::Row { text, runs } = terminal::row(screen, row, cols, cursor);
      let highlights = runs.into_iter().map(|(range, style)| {
        (
          range,
          HighlightStyle {
            color: Some(color(style.fg)),
            background_color: style.bg.map(color),
            font_weight: style.bold.then_some(FontWeight::BOLD),
            underline: style.underline.then(|| UnderlineStyle {
              thickness: px(1.),
              color: None,
              wavy: false,
            }),
            ..Default::default()
          },
        )
      });
      div()
        .h(px(LINE_HEIGHT))
        .whitespace_nowrap()
        .child(StyledText::new(text).with_highlights(highlights))
    });

    let entity = cx.entity();
    let focus = self.focus.clone();
    let font = Font {
      family: mono.clone(),
      ..Font::default()
    };
    // Measures the grid and registers the view as the text input handler.
    let measure = canvas(
      {
        let entity = entity.clone();
        move |bounds, window, cx| {
          let font_id = window.text_system().resolve_font(&font);
          let width = window
            .text_system()
            .advance(font_id, px(FONT_SIZE), 'm')
            .map(|size| size.width)
            .unwrap_or(px(FONT_SIZE * 0.6));
          let cell = size(width, px(LINE_HEIGHT));
          let current = entity.read(cx).cell;
          let needs_resize = current != Some(cell) || {
            let (c, r) = entity.read(cx).size;
            let cols = (f32::from(bounds.size.width) / f32::from(width))
              .floor()
              .max(10.) as u16;
            let rows = (f32::from(bounds.size.height) / LINE_HEIGHT)
              .floor()
              .max(4.) as u16;
            (cols, rows) != (c, r)
          };
          if needs_resize {
            let entity = entity;
            let bounds_size = bounds.size;
            cx.defer(move |cx| entity.update(cx, |view, cx| view.resize(bounds_size, cell, cx)));
          }
        }
      },
      move |bounds, _, window, cx| {
        window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
      },
    )
    .absolute()
    .size_full();

    div()
      .id("terminal")
      .key_context("Terminal")
      .track_focus(&self.focus)
      .on_key_down(cx.listener(Self::on_key))
      .on_scroll_wheel(cx.listener(Self::on_scroll))
      .on_mouse_down(
        MouseButton::Left,
        cx.listener(|view, _, window, cx| {
          view.focus(window, cx);
          window.request_virtual_keyboard();
        }),
      )
      .relative()
      .size_full()
      .overflow_hidden()
      .bg(color(terminal::DEFAULT_BG))
      .text_color(color(terminal::DEFAULT_FG))
      .font_family(mono)
      .text_size(px(FONT_SIZE))
      .line_height(px(LINE_HEIGHT))
      .px_1()
      .child(measure)
      .children(lines)
  }
}

impl Focusable for TerminalView {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus.clone()
  }
}

// The terminal has no editable text of its own: everything typed is sent to
// the PTY, and the IME sees an empty document.
impl EntityInputHandler for TerminalView {
  fn text_for_range(
    &mut self,
    _: Range<usize>,
    _: &mut Option<Range<usize>>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<String> {
    Some(String::new())
  }

  fn selected_text_range(
    &mut self,
    _: bool,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<UTF16Selection> {
    Some(UTF16Selection {
      range: 0..0,
      reversed: false,
    })
  }

  fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
    None
  }

  fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

  fn replace_text_in_range(
    &mut self,
    _: Option<Range<usize>>,
    text: &str,
    _: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if !text.is_empty() {
      self.write(text.replace('\n', "\r").into_bytes(), cx);
    }
  }

  fn replace_and_mark_text_in_range(
    &mut self,
    _: Option<Range<usize>>,
    _: &str,
    _: Option<Range<usize>>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) {
    // Composition is not shown; the committed text arrives through
    // `replace_text_in_range`.
  }

  fn bounds_for_range(
    &mut self,
    _: Range<usize>,
    element_bounds: Bounds<Pixels>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<Bounds<Pixels>> {
    Some(element_bounds)
  }

  fn character_index_for_point(
    &mut self,
    _: Point<Pixels>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<usize> {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn drag(px_per_event: f32, events: usize) -> usize {
    let (mut scrollback, mut remainder) = (0, 0.);
    for _ in 0..events {
      scrollback = scrolled_to(scrollback, px_per_event / LINE_HEIGHT, &mut remainder);
    }
    scrollback
  }

  #[core::prelude::v1::test]
  fn slow_drags_scroll_and_fast_ones_do_not_overshoot() {
    // 20 events: 340 px is 20 lines at 17 px per line.
    assert_eq!(drag(3., 20), 3);
    assert_eq!(drag(8., 20), 9);
    assert_eq!(drag(9., 20), 10);
    assert_eq!(drag(17., 20), 20);
  }

  #[core::prelude::v1::test]
  fn dragging_back_to_the_live_screen_stops_there() {
    let mut remainder = 0.;
    assert_eq!(scrolled_to(2, -0.4, &mut remainder), 2);
    assert_eq!(scrolled_to(2, -3., &mut remainder), 0);
    assert_eq!(remainder, 0.);
    assert_eq!(scrolled_to(0, 0.5, &mut remainder), 0);
  }
}
