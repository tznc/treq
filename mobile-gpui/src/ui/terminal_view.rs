//! A terminal grid bound to one remote PTY session.
//!
//! Output from treq's `RemotePtyManager` feeds a `vt100` parser; each screen
//! row renders as styled text. The view registers itself as GPUI's text
//! input handler, so focusing it brings up the soft keyboard and typed text
//! (including IME commits) is written to the PTY. Named keys and Ctrl/Alt
//! combinations are encoded in `on_key_down`.

use std::ops::Range;

use futures::{channel::mpsc, StreamExt};
use gpui_kit::{component::ActiveTheme, *};

use crate::{backend, terminal};

const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 17.;
const SCROLLBACK: usize = 5000;

/// What the PTY channel reports to the view.
pub enum PtyEvent {
  Output(Vec<u8>),
  Exited(Option<u32>),
}

pub struct TerminalView {
  session_id: String,
  parser: vt100::Parser,
  focus: FocusHandle,
  size: (u16, u16),
  cell: Option<Size<Pixels>>,
  /// Set once the remote channel ended.
  pub exit: Option<Option<u32>>,
  on_exit: Option<Box<dyn FnOnce(Option<u32>, &mut Window, &mut App)>>,
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
      parser: vt100::Parser::new(rows, cols, SCROLLBACK),
      focus,
      size: (cols, rows),
      cell: None,
      exit: None,
      on_exit: Some(Box::new(on_exit)),
      _reader: reader,
    }
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
    if (cols, rows) == self.size {
      return;
    }
    self.size = (cols, rows);
    self.parser.screen_mut().set_size(rows, cols);
    let id = self.session_id.clone();
    backend::spawn_detached(async move {
      let _ = backend::get().ptys.resize(&id, cols, rows).await;
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
    let current = self.parser.screen().scrollback() as f32;
    let target = (current + lines).max(0.).round() as usize;
    self.parser.screen_mut().set_scrollback(target);
    cx.notify();
  }
}

impl Drop for TerminalView {
  fn drop(&mut self) {
    // Detach: the remote session keeps running and can be reattached.
    let id = self.session_id.clone();
    if backend::try_get().is_some() {
      backend::spawn_detached(async move {
        let _ = backend::get().ptys.close(&id).await;
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
            let entity = entity.clone();
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
