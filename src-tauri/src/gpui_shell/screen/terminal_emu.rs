//! Terminal emulation for the GPUI terminal pane: PTY output goes through
//! alacritty's parser into a grid, and keystrokes come back out as the bytes
//! xterm would send. Matches the xterm.js setup React used (5000 lines of
//! scrollback, xterm.js's default palette on a #1e1e1e background), with a
//! light scheme for the light theme.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::search::{Match, RegexSearch};
use alacritty_terminal::term::{Config, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb};
use std::sync::{Arc, Mutex};

pub const SCROLLBACK: usize = 5000;
/// The colours a frame is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scheme {
  pub background: u32,
  pub foreground: u32,
  pub selection: u32,
  pub ansi: [u32; 16],
}

/// xterm.js's defaults, as React drew every terminal.
pub const DARK: Scheme = Scheme {
  background: 0x1e1e1e,
  foreground: 0xffffff,
  selection: 0x4b4b4b,
  ansi: [
    0x2e3436, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf, 0x555753,
    0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
  ],
};

/// VS Code's light terminal, with green and yellow darkened to stay
/// readable on white.
pub const LIGHT: Scheme = Scheme {
  background: 0xffffff,
  foreground: 0x333333,
  selection: 0xadd6ff,
  ansi: [
    0x000000, 0xcd3131, 0x107c10, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555, 0x666666,
    0xcd3131, 0x098f09, 0x8f8f00, 0x0451a5, 0xbc05bc, 0x0598bc, 0xa5a5a5,
  ],
};

pub fn scheme(dark: bool) -> &'static Scheme {
  if dark {
    &DARK
  } else {
    &LIGHT
  }
}

/// What the terminal asks of its host while parsing output.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
  /// Bytes to write back to the PTY (device status replies and the like).
  Reply(String),
  Title(String),
  Copy(String),
  Bell,
}

#[derive(Clone)]
struct Listener(Arc<Mutex<Vec<Output>>>);

impl EventListener for Listener {
  fn send_event(&self, event: Event) {
    let output = match event {
      Event::PtyWrite(text) => Output::Reply(text),
      Event::Title(title) => Output::Title(title),
      Event::ClipboardStore(_, text) => Output::Copy(text),
      Event::Bell => Output::Bell,
      Event::TextAreaSizeRequest(_) | Event::ColorRequest(..) | Event::ClipboardLoad(..) => return,
      _ => return,
    };
    if let Ok(mut queue) = self.0.lock() {
      queue.push(output);
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
  pub cols: usize,
  pub rows: usize,
}

impl Dimensions for Size {
  fn total_lines(&self) -> usize {
    self.rows
  }
  fn screen_lines(&self) -> usize {
    self.rows
  }
  fn columns(&self) -> usize {
    self.cols
  }
}

/// A stretch of cells on one line that share a style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
  pub col: usize,
  pub cells: usize,
  pub text: String,
  pub fg: u32,
  pub bg: Option<u32>,
  pub bold: bool,
  pub italic: bool,
  pub underline: bool,
  pub strike: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
  /// Each viewport row's runs, shared with the next frame when the row
  /// didn't change.
  pub lines: Vec<Arc<Vec<Run>>>,
  /// Cursor position in the viewport, when shown and scrolled into view.
  pub cursor: Option<(usize, usize)>,
}

pub struct Emulator {
  term: Term<Listener>,
  parser: Processor,
  outputs: Arc<Mutex<Vec<Output>>>,
  size: Size,
  /// The find bar's current match.
  found: Option<Match>,
  /// The last frame built and the scheme it was built in. Redraws that
  /// change nothing here (another terminal's output, the cursor's blink)
  /// take it as it is.
  drawn: Option<(Scheme, Arc<Frame>)>,
  /// Whether the grid or the view of it changed since `drawn`; the
  /// terminal's damage says which rows.
  changed: bool,
  /// Whether every row needs rebuilding: the selection, which the damage
  /// doesn't track, changed.
  stale: bool,
}

impl Emulator {
  pub fn new(cols: usize, rows: usize) -> Self {
    let size = Size {
      cols: cols.max(2),
      rows: rows.max(1),
    };
    let outputs = Arc::new(Mutex::new(Vec::new()));
    let config = Config {
      scrolling_history: SCROLLBACK,
      ..Config::default()
    };
    Self {
      term: Term::new(config, &size, Listener(outputs.clone())),
      parser: Processor::new(),
      outputs,
      size,
      found: None,
      drawn: None,
      changed: false,
      stale: false,
    }
  }

  pub fn size(&self) -> Size {
    self.size
  }

  pub fn feed(&mut self, bytes: &[u8]) {
    self.changed = true;
    self.parser.advance(&mut self.term, bytes);
  }

  /// Takes what the terminal asked for since the last call.
  pub fn drain(&mut self) -> Vec<Output> {
    self
      .outputs
      .lock()
      .map(|mut queue| std::mem::take(&mut *queue))
      .unwrap_or_default()
  }

  /// Returns whether the size changed.
  pub fn resize(&mut self, cols: usize, rows: usize) -> bool {
    let size = Size {
      cols: cols.max(2),
      rows: rows.max(1),
    };
    if size == self.size {
      return false;
    }
    self.changed = true;
    if size.cols != self.size.cols && !self.alt_screen() {
      self.hold_cursor_line(size.cols);
    }
    self.size = size;
    self.term.resize(size);
    true
  }

  /// Keeps the line the cursor is on out of the reflow, as xterm.js does:
  /// the program there (a shell's prompt, a TUI's live region) redraws it
  /// for the new width, counting the rows it drew at the old one. Reflowed,
  /// it takes more or fewer rows than that, and the redraw leaves a copy
  /// behind or eats the line above. Cut to the new width instead.
  fn hold_cursor_line(&mut self, cols: usize) {
    let grid = self.term.grid_mut();
    let last = grid.last_column();
    let wraps = |grid: &alacritty_terminal::Grid<Cell>, line: Line| {
      grid[line][last].flags.contains(Flags::WRAPLINE)
    };
    let cursor = grid.cursor.point.line;
    let mut top = cursor;
    while top > grid.topmost_line() && wraps(grid, top - 1) {
      top -= 1;
    }
    let mut bottom = cursor;
    while bottom < grid.bottommost_line() && wraps(grid, bottom) {
      bottom += 1;
    }
    let mut line = top;
    while line <= bottom {
      let row = &mut grid[line];
      row[last].flags.remove(Flags::WRAPLINE);
      let mut from = cols;
      if from > 0 && from <= last.0 && row[Column(from - 1)].flags.contains(Flags::WIDE_CHAR) {
        from -= 1;
      }
      for col in from..=last.0 {
        row[Column(col)] = Cell::default();
      }
      line += 1;
    }
    grid.cursor.input_needs_wrap = false;
    grid.cursor.point.column = grid.cursor.point.column.min(Column(cols - 1));
  }

  pub fn scroll(&mut self, lines: i32) {
    self.scroll_display(Scroll::Delta(lines));
  }

  pub fn scroll_to_bottom(&mut self) {
    self.scroll_display(Scroll::Bottom);
  }

  /// Every keystroke scrolls to the bottom; only a move redraws.
  fn scroll_display(&mut self, scroll: Scroll) {
    let offset = self.display_offset();
    self.term.scroll_display(scroll);
    if self.display_offset() != offset {
      self.changed = true;
    }
  }

  pub fn display_offset(&self) -> usize {
    self.term.grid().display_offset()
  }

  pub fn history(&self) -> usize {
    self.term.grid().history_size()
  }

  pub fn app_cursor(&self) -> bool {
    self.term.mode().contains(TermMode::APP_CURSOR)
  }

  pub fn bracketed_paste(&self) -> bool {
    self.term.mode().contains(TermMode::BRACKETED_PASTE)
  }

  pub fn alt_screen(&self) -> bool {
    self.term.mode().contains(TermMode::ALT_SCREEN)
  }

  /// Whether the program asked for mouse events (DECSET 1000, 1002, 1003).
  pub fn mouse_reporting(&self) -> bool {
    self.term.mode().intersects(TermMode::MOUSE_MODE)
  }

  /// The report xterm sends for a mouse event, in the encoding the program
  /// asked for; `None` when the program doesn't want it.
  pub fn encode_mouse(&self, mouse: Mouse) -> Option<Vec<u8>> {
    let mode = *self.term.mode();
    if !mode.intersects(TermMode::MOUSE_MODE) {
      return None;
    }
    let held = mouse.button != Button::None;
    let wheel = matches!(mouse.button, Button::WheelUp | Button::WheelDown);
    let wanted = match mouse.action {
      MouseAction::Move if held => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
      MouseAction::Move => mode.contains(TermMode::MOUSE_MOTION),
      MouseAction::Release => !wheel,
      MouseAction::Press => held,
    };
    if !wanted {
      return None;
    }
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let mut code: u32 = match mouse.button {
      Button::Left => 0,
      Button::Middle => 1,
      Button::Right => 2,
      Button::None => 3,
      Button::WheelUp => 64,
      Button::WheelDown => 65,
    };
    if mouse.action == MouseAction::Release && !sgr {
      code = 3;
    }
    if mouse.action == MouseAction::Move {
      code += 32;
    }
    code += 4 * u32::from(mouse.shift) + 8 * u32::from(mouse.alt) + 16 * u32::from(mouse.ctrl);
    let col = mouse.col.min(self.size.cols - 1) as u32 + 1;
    let row = mouse.row.min(self.size.rows - 1) as u32 + 1;
    if sgr {
      let end = if mouse.action == MouseAction::Release {
        'm'
      } else {
        'M'
      };
      return Some(format!("\x1b[<{code};{col};{row}{end}").into_bytes());
    }
    // The X10 encoding: each value a character offset by 32. Plain bytes
    // stop at 127 here, since the PTY takes text.
    let limit = if mode.contains(TermMode::UTF8_MOUSE) {
      2047
    } else {
      127
    };
    let mut out = String::from("\x1b[M");
    for value in [code + 32, col + 32, row + 32] {
      if value > limit {
        return None;
      }
      out.push(char::from_u32(value)?);
    }
    Some(out.into_bytes())
  }

  /// Viewport point for a cell, accounting for scrollback.
  fn point(&self, line: usize, col: usize) -> Point {
    let line = line.min(self.size.rows - 1) as i32 - self.display_offset() as i32;
    Point::new(Line(line), Column(col.min(self.size.cols - 1)))
  }

  pub fn select_start(&mut self, line: usize, col: usize, right_half: bool, kind: SelectKind) {
    let side = if right_half { Side::Right } else { Side::Left };
    let ty = match kind {
      SelectKind::Simple => SelectionType::Simple,
      SelectKind::Word => SelectionType::Semantic,
      SelectKind::Line => SelectionType::Lines,
    };
    self.term.selection = Some(Selection::new(ty, self.point(line, col), side));
    self.redraw_all();
  }

  pub fn select_extend(&mut self, line: usize, col: usize, right_half: bool) {
    let point = self.point(line, col);
    let side = if right_half { Side::Right } else { Side::Left };
    if let Some(selection) = self.term.selection.as_mut() {
      selection.update(point, side);
      self.redraw_all();
    }
  }

  pub fn select_all(&mut self) {
    let top = Point::new(self.term.grid().topmost_line(), Column(0));
    let bottom = Point::new(
      self.term.grid().bottommost_line(),
      self.term.grid().last_column(),
    );
    let mut selection = Selection::new(SelectionType::Simple, top, Side::Left);
    selection.update(bottom, Side::Right);
    self.term.selection = Some(selection);
    self.redraw_all();
  }

  pub fn clear_selection(&mut self) {
    if self.term.selection.take().is_some() {
      self.redraw_all();
    }
  }

  pub fn selection_text(&self) -> Option<String> {
    self
      .term
      .selection_to_string()
      .filter(|text| !text.is_empty())
  }

  /// Selects the next (or previous) match of `query` after the last one,
  /// scrolling it into view, as xterm's search addon does.
  pub fn find(&mut self, query: &str, forward: bool) -> bool {
    if query.is_empty() {
      return false;
    }
    let pattern: String = query
      .chars()
      .flat_map(|c| {
        let special = "\\.+*?()|[]{}^$#&-~".contains(c);
        special.then_some('\\').into_iter().chain(Some(c))
      })
      .collect();
    let Ok(mut regex) = RegexSearch::new(&pattern) else {
      return false;
    };
    let grid = self.term.grid();
    let origin = match (&self.found, forward) {
      (Some(found), true) => found
        .end()
        .add(grid, alacritty_terminal::index::Boundary::Grid, 1),
      (Some(found), false) => found
        .start()
        .sub(grid, alacritty_terminal::index::Boundary::Grid, 1),
      (None, true) => Point::new(grid.topmost_line(), Column(0)),
      (None, false) => Point::new(grid.bottommost_line(), grid.last_column()),
    };
    let direction = if forward {
      Direction::Right
    } else {
      Direction::Left
    };
    let Some(found) = self
      .term
      .search_next(&mut regex, origin, direction, Side::Left, None)
    else {
      return false;
    };
    let mut selection = Selection::new(SelectionType::Simple, *found.start(), Side::Left);
    selection.update(*found.end(), Side::Right);
    self.term.selection = Some(selection);
    self.term.scroll_to_point(*found.start());
    self.found = Some(found);
    self.redraw_all();
    true
  }

  pub fn clear_find(&mut self) {
    if self.found.take().is_some() {
      self.term.selection = None;
      self.redraw_all();
    }
  }

  /// The visible screen as plain text, one line per row, trailing blanks trimmed.
  pub fn screen_text(&self) -> String {
    let mut lines = vec![String::new(); self.size.rows];
    let offset = self.display_offset() as i32;
    for cell in self.term.grid().display_iter() {
      let row = (cell.point.line.0 + offset) as usize;
      if row < lines.len() && !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
        lines[row].push(cell.c);
      }
    }
    let lines: Vec<&str> = lines.iter().map(|line| line.trim_end()).collect();
    lines.join("\n").trim_end().to_string()
  }

  fn redraw_all(&mut self) {
    self.changed = true;
    self.stale = true;
  }

  /// The viewport's runs, built once per change: only the rows the
  /// terminal damaged are built again (an echo, a spinner's tick), the
  /// rest taken from the last frame.
  pub fn frame(&mut self, scheme: &Scheme) -> Arc<Frame> {
    if let Some((drawn, frame)) = &self.drawn {
      if drawn == scheme && !self.changed {
        return frame.clone();
      }
    }
    let stale = std::mem::take(&mut self.stale);
    self.changed = false;
    let rows = self.size.rows;
    let damaged = match self.term.damage() {
      TermDamage::Full => None,
      TermDamage::Partial(lines) => {
        let mut damaged = vec![false; rows];
        for line in lines {
          if let Some(row) = damaged.get_mut(line.line) {
            *row = true;
          }
        }
        Some(damaged)
      }
    };
    self.term.reset_damage();
    let previous = self
      .drawn
      .take()
      .filter(|(drawn, frame)| !stale && drawn == scheme && frame.lines.len() == rows)
      .map(|(_, frame)| frame);
    let reuse = previous.as_deref().zip(damaged.as_deref());
    let frame = Arc::new(self.build_frame(scheme, reuse));
    self.drawn = Some((*scheme, frame.clone()));
    frame
  }

  /// Builds the frame, taking the rows `reuse` marks undamaged from its
  /// frame.
  fn build_frame(&self, scheme: &Scheme, reuse: Option<(&Frame, &[bool])>) -> Frame {
    let content = self.term.renderable_content();
    let offset = content.display_offset as i32;
    let lines = (0..self.size.rows)
      .map(|row| match reuse {
        Some((previous, damaged)) if !damaged[row] => previous.lines[row].clone(),
        _ => Arc::new(self.build_line(
          Line(row as i32 - offset),
          scheme,
          content.selection.as_ref(),
          content.colors,
        )),
      })
      .collect();
    let cursor = content.cursor.point;
    let row = cursor.line.0 + offset;
    let shown =
      content.mode.contains(TermMode::SHOW_CURSOR) && row >= 0 && (row as usize) < self.size.rows;
    Frame {
      lines,
      cursor: shown.then_some((row as usize, cursor.column.0)),
    }
  }

  fn build_line(
    &self,
    line: Line,
    scheme: &Scheme,
    selection: Option<&SelectionRange>,
    colors: &Colors,
  ) -> Vec<Run> {
    let cells = &self.term.grid()[line];
    let selected =
      |col: usize| selection.is_some_and(|range| range.contains(Point::new(line, Column(col))));
    // Trailing blanks draw nothing: stop after the last cell that does.
    let mut end = self.size.cols;
    while end > 0 {
      let cell = &cells[Column(end - 1)];
      let blank = cell.c.is_whitespace()
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell
          .flags
          .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
        && cell.zerowidth().is_none_or(<[char]>::is_empty)
        && !selected(end - 1);
      if !blank {
        break;
      }
      end -= 1;
    }
    let mut runs: Vec<Run> = Vec::new();
    for col in 0..end {
      let cell = &cells[Column(col)];
      if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
        continue;
      }
      let flags = cell.flags;
      let mut fg = resolve(cell.fg, colors, flags.contains(Flags::BOLD), scheme);
      let mut bg = match cell.bg {
        Color::Named(NamedColor::Background) => None,
        color => Some(resolve(color, colors, false, scheme)),
      };
      if flags.contains(Flags::INVERSE) {
        let back = bg.unwrap_or(scheme.background);
        bg = Some(fg);
        fg = back;
      }
      if flags.contains(Flags::DIM) {
        fg = dim(fg, scheme.background);
      }
      if flags.contains(Flags::HIDDEN) {
        fg = bg.unwrap_or(scheme.background);
      }
      if selected(col) {
        bg = Some(scheme.selection);
      }
      let width = if flags.contains(Flags::WIDE_CHAR) {
        2
      } else {
        1
      };
      let style = Run {
        col,
        cells: width,
        text: String::new(),
        fg,
        bg,
        bold: flags.contains(Flags::BOLD),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        strike: flags.contains(Flags::STRIKEOUT),
      };
      match runs.last_mut() {
        Some(last)
          if same_style(last, &style) && width == 1 && last.col + last.cells == style.col =>
        {
          last.cells += 1;
          last.text.push(cell.c);
        }
        _ => {
          let mut run = style;
          run.text.push(cell.c);
          for extra in cell.zerowidth().into_iter().flatten() {
            run.text.push(*extra);
          }
          runs.push(run);
        }
      }
    }
    let blank = |run: &Run| run.bg.is_none() && !run.underline && !run.strike;
    while runs
      .last()
      .is_some_and(|run| blank(run) && run.text.trim().is_empty())
    {
      runs.pop();
    }
    if let Some(last) = runs.last_mut().filter(|run| blank(run)) {
      let trimmed = last.text.trim_end().len();
      last.cells -= last.text.len() - trimmed;
      last.text.truncate(trimmed);
    }
    runs
  }
}

/// The mouse button of a [`Mouse`] event; `None` for motion with no button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Button {
  #[default]
  Left,
  Middle,
  Right,
  None,
  WheelUp,
  WheelDown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MouseAction {
  #[default]
  Press,
  Release,
  Move,
}

/// A mouse event over the grid, at a viewport cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mouse {
  pub button: Button,
  pub action: MouseAction,
  pub row: usize,
  pub col: usize,
  pub shift: bool,
  pub alt: bool,
  pub ctrl: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
  Simple,
  Word,
  Line,
}

fn same_style(a: &Run, b: &Run) -> bool {
  a.fg == b.fg
    && a.bg == b.bg
    && a.bold == b.bold
    && a.italic == b.italic
    && a.underline == b.underline
    && a.strike == b.strike
}

fn rgb(color: Rgb) -> u32 {
  (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

/// Two thirds of the way from the background to `color`.
fn dim(color: u32, background: u32) -> u32 {
  let channel =
    |shift: u32| ((((color >> shift) & 0xff) * 2 + ((background >> shift) & 0xff)) / 3) << shift;
  channel(16) | channel(8) | channel(0)
}

/// xterm's 256-colour table.
pub fn indexed(index: u8, scheme: &Scheme) -> u32 {
  match index {
    0..=15 => scheme.ansi[index as usize],
    16..=231 => {
      let levels = [0u32, 95, 135, 175, 215, 255];
      let n = u32::from(index - 16);
      (levels[(n / 36) as usize] << 16)
        | (levels[(n / 6 % 6) as usize] << 8)
        | levels[(n % 6) as usize]
    }
    _ => {
      let level = 8 + 10 * u32::from(index - 232);
      (level << 16) | (level << 8) | level
    }
  }
}

fn resolve(
  color: Color,
  colors: &alacritty_terminal::term::color::Colors,
  bold: bool,
  scheme: &Scheme,
) -> u32 {
  match color {
    Color::Spec(spec) => rgb(spec),
    Color::Indexed(index) => {
      // xterm.js draws bold text in the bright variant of the first 8 colours.
      let index = if bold && index < 8 { index + 8 } else { index };
      colors[index as usize]
        .map(rgb)
        .unwrap_or_else(|| indexed(index, scheme))
    }
    Color::Named(named) => {
      let index = named as usize;
      if let Some(spec) = colors[index] {
        return rgb(spec);
      }
      match named {
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
          scheme.foreground
        }
        NamedColor::DimForeground => dim(scheme.foreground, scheme.background),
        NamedColor::Background => scheme.background,
        _ if index < 8 && bold => scheme.ansi[index + 8],
        _ if index < 16 => scheme.ansi[index],
        _ => dim(
          scheme.ansi[(index - NamedColor::DimBlack as usize) % 8],
          scheme.background,
        ),
      }
    }
  }
}

/// A key press as GPUI reports it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Key<'a> {
  pub key: &'a str,
  pub key_char: Option<&'a str>,
  pub ctrl: bool,
  pub alt: bool,
  pub shift: bool,
  pub platform: bool,
}

/// The bytes xterm sends for a key press; `None` leaves the key to the app.
pub fn encode_key(key: Key<'_>, app_cursor: bool) -> Option<Vec<u8>> {
  if key.platform {
    return None;
  }
  let modifier = 1 + u8::from(key.shift) + 2 * u8::from(key.alt) + 4 * u8::from(key.ctrl);
  let cursor = |code: char| -> Vec<u8> {
    if modifier > 1 {
      format!("\x1b[1;{modifier}{code}").into_bytes()
    } else if app_cursor {
      format!("\x1bO{code}").into_bytes()
    } else {
      format!("\x1b[{code}").into_bytes()
    }
  };
  let tilde = |code: u8| -> Vec<u8> {
    if modifier > 1 {
      format!("\x1b[{code};{modifier}~").into_bytes()
    } else {
      format!("\x1b[{code}~").into_bytes()
    }
  };
  let meta = |bytes: Vec<u8>| {
    if key.alt {
      [b"\x1b".to_vec(), bytes].concat()
    } else {
      bytes
    }
  };
  let bytes = match key.key {
    "up" => cursor('A'),
    "down" => cursor('B'),
    "right" => cursor('C'),
    "left" => cursor('D'),
    "home" => cursor('H'),
    "end" => cursor('F'),
    "insert" => tilde(2),
    "delete" => tilde(3),
    "pageup" => tilde(5),
    "pagedown" => tilde(6),
    "f1" => b"\x1bOP".to_vec(),
    "f2" => b"\x1bOQ".to_vec(),
    "f3" => b"\x1bOR".to_vec(),
    "f4" => b"\x1bOS".to_vec(),
    "f5" => tilde(15),
    "f6" => tilde(17),
    "f7" => tilde(18),
    "f8" => tilde(19),
    "f9" => tilde(20),
    "f10" => tilde(21),
    "f11" => tilde(23),
    "f12" => tilde(24),
    "enter" => meta(b"\r".to_vec()),
    "escape" => b"\x1b".to_vec(),
    "tab" if key.shift => b"\x1b[Z".to_vec(),
    "tab" => b"\t".to_vec(),
    "backspace" if key.ctrl => meta(b"\x08".to_vec()),
    "backspace" => meta(b"\x7f".to_vec()),
    "space" if key.ctrl => vec![0],
    named if key.ctrl => {
      let mut chars = named.chars();
      let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
      };
      let code = match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '7' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
      };
      meta(vec![code])
    }
    "space" => meta(b" ".to_vec()),
    _ => meta(key.key_char?.as_bytes().to_vec()),
  };
  Some(bytes)
}

/// Text pasted into the terminal, wrapped when the program asked for it.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
  let text = text.replace("\r\n", "\r").replace('\n', "\r");
  if bracketed {
    let text = text.replace("\x1b[201~", "");
    format!("\x1b[200~{text}\x1b[201~").into_bytes()
  } else {
    text.into_bytes()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn key(name: &str) -> Key<'_> {
    Key {
      key: name,
      ..Key::default()
    }
  }

  #[test]
  fn draws_plain_output() {
    let mut emu = Emulator::new(20, 3);
    emu.feed(b"hello\r\nworld");
    assert_eq!(emu.screen_text(), "hello\nworld");
    let frame = emu.frame(&DARK);
    assert_eq!(frame.lines[0][0].text, "hello");
    assert_eq!(frame.lines[0][0].fg, DARK.foreground);
    assert_eq!(frame.cursor, Some((1, 5)));
  }

  #[test]
  fn colours_runs_with_sgr() {
    let mut emu = Emulator::new(20, 1);
    emu.feed(b"a\x1b[31mred\x1b[0m\x1b[1;32mok\x1b[38;5;196mx\x1b[48;2;1;2;3my");
    let frame = emu.frame(&DARK);
    let runs = &frame.lines[0];
    let texts: Vec<_> = runs
      .iter()
      .map(|run| (run.text.as_str(), run.fg, run.bg))
      .collect();
    assert_eq!(
      texts,
      vec![
        ("a", DARK.foreground, None),
        ("red", 0xcc0000, None),
        ("ok", 0x8ae234, None),
        ("x", 0xff0000, None),
        ("y", 0xff0000, Some(0x010203)),
      ]
    );
    assert!(runs[2].bold);
  }

  #[test]
  fn draws_in_the_light_scheme() {
    let mut emu = Emulator::new(10, 1);
    emu.feed(b"a\x1b[31mr\x1b[0m\x1b[2md\x1b[0m\x1b[7mi");
    let runs: Vec<_> = emu.frame(&LIGHT).lines[0]
      .iter()
      .map(|run| (run.fg, run.bg))
      .collect();
    assert_eq!(
      runs,
      vec![
        (LIGHT.foreground, None),
        (0xcd3131, None),
        // Dim fades towards the white background, not to black.
        (0x777777, None),
        (LIGHT.background, Some(LIGHT.foreground)),
      ]
    );
  }

  #[test]
  fn inverse_swaps_colours() {
    let mut emu = Emulator::new(10, 1);
    emu.feed(b"\x1b[7mx");
    let frame = emu.frame(&DARK);
    let run = &frame.lines[0][0];
    assert_eq!((run.fg, run.bg), (DARK.background, Some(DARK.foreground)));
  }

  #[test]
  fn keeps_scrollback_and_scrolls_it_into_view() {
    let mut emu = Emulator::new(10, 2);
    emu.feed(b"1\r\n2\r\n3\r\n4");
    assert_eq!(emu.screen_text(), "3\n4");
    assert_eq!(emu.history(), 2);
    emu.scroll(2);
    assert_eq!(emu.screen_text(), "1\n2");
    assert_eq!(emu.frame(&DARK).cursor, None);
    emu.scroll_to_bottom();
    assert_eq!(emu.screen_text(), "3\n4");
  }

  #[test]
  fn resizes_and_reflows() {
    let mut emu = Emulator::new(10, 3);
    emu.feed(b"abcdefghij\r\n");
    assert!(emu.resize(5, 3));
    assert!(!emu.resize(5, 3));
    // The wrapped line's head goes into scrollback, as in xterm.
    emu.scroll(emu.history() as i32);
    assert!(emu.screen_text().starts_with("abcde\nfghij"));
  }

  #[test]
  fn shrinking_leaves_the_cursor_line_to_the_program() {
    let mut emu = Emulator::new(20, 4);
    // Output above, then a prompt with its right prompt at the edge.
    emu.feed(b"abcdefghijklmnop\r\n$ ls\x1b[15G12:00\x1b[5G");
    assert!(emu.resize(10, 4));
    // The shell redraws its prompt in place, for the new width.
    emu.feed(b"\r\x1b[J$ ls 12:00");
    emu.scroll(emu.history() as i32);
    assert_eq!(emu.screen_text(), "abcdefghij\nklmnop\n$ ls 12:00");
  }

  #[test]
  fn growing_keeps_the_cursor_lines_rows() {
    let mut emu = Emulator::new(10, 4);
    emu.feed(b"out\r\n$ 345678901234");
    assert_eq!(emu.screen_text(), "out\n$ 34567890\n1234");
    assert!(emu.resize(20, 4));
    // The shell drew two rows; it goes back up one to redraw them.
    emu.feed(b"\x1b[1A\r\x1b[J$ 345678901234");
    assert_eq!(emu.screen_text(), "out\n$ 345678901234");
  }

  #[test]
  fn selects_and_copies_text() {
    let mut emu = Emulator::new(20, 2);
    emu.feed(b"hello world\r\nnext");
    emu.select_start(0, 6, false, SelectKind::Simple);
    emu.select_extend(0, 10, true);
    assert_eq!(emu.selection_text().as_deref(), Some("world"));
    assert_eq!(emu.frame(&DARK).lines[0][1].bg, Some(DARK.selection));
    emu.select_start(0, 1, false, SelectKind::Word);
    assert_eq!(emu.selection_text().as_deref(), Some("hello"));
    emu.select_all();
    assert_eq!(emu.selection_text().as_deref(), Some("hello world\nnext"));
    emu.clear_selection();
    assert_eq!(emu.selection_text(), None);
  }

  #[test]
  fn finds_matches_both_ways() {
    let mut emu = Emulator::new(20, 3);
    emu.feed(b"a.b one\r\ntwo a.b\r\nend");
    assert!(emu.find("a.b", true));
    assert_eq!(emu.selection_text().as_deref(), Some("a.b"));
    assert_eq!(emu.frame(&DARK).lines[0][0].bg, Some(DARK.selection));
    assert!(emu.find("a.b", true));
    assert_eq!(
      emu.frame(&DARK).lines[1].last().unwrap().bg,
      Some(DARK.selection)
    );
    assert!(emu.find("a.b", false));
    assert_eq!(emu.frame(&DARK).lines[0][0].bg, Some(DARK.selection));
    assert!(!emu.find("zzz", true));
    emu.clear_find();
    assert_eq!(emu.selection_text(), None);
  }

  #[test]
  fn answers_device_status_requests() {
    let mut emu = Emulator::new(10, 2);
    emu.feed(b"ab\x1b[6n\x1b]0;my title\x07");
    assert_eq!(
      emu.drain(),
      vec![
        Output::Reply("\x1b[1;3R".into()),
        Output::Title("my title".into())
      ]
    );
    assert!(emu.drain().is_empty());
  }

  #[test]
  fn tracks_modes() {
    let mut emu = Emulator::new(10, 2);
    assert!(!emu.app_cursor());
    emu.feed(b"\x1b[?1h\x1b[?2004h\x1b[?1049h");
    assert!(emu.app_cursor() && emu.bracketed_paste() && emu.alt_screen());
  }

  fn mouse(button: Button, action: MouseAction, row: usize, col: usize) -> Mouse {
    Mouse {
      button,
      action,
      row,
      col,
      ..Mouse::default()
    }
  }

  #[test]
  fn reports_nothing_until_the_program_asks() {
    let emu = Emulator::new(80, 24);
    assert!(!emu.mouse_reporting());
    assert_eq!(
      emu.encode_mouse(mouse(Button::Left, MouseAction::Press, 0, 0)),
      None
    );
  }

  #[test]
  fn reports_clicks_like_xterm() {
    let mut emu = Emulator::new(200, 24);
    emu.feed(b"\x1b[?1000h");
    assert!(emu.mouse_reporting());
    let enc = |emu: &Emulator, m: Mouse| emu.encode_mouse(m).map(|b| String::from_utf8(b).unwrap());
    assert_eq!(
      enc(&emu, mouse(Button::Left, MouseAction::Press, 2, 4)).as_deref(),
      Some("\x1b[M %#")
    );
    assert_eq!(
      enc(&emu, mouse(Button::Left, MouseAction::Release, 2, 4)).as_deref(),
      Some("\x1b[M#%#")
    );
    // Clicks only: no motion.
    assert_eq!(
      enc(&emu, mouse(Button::Left, MouseAction::Move, 2, 5)),
      None
    );
    assert_eq!(
      enc(&emu, mouse(Button::WheelUp, MouseAction::Press, 0, 0)).as_deref(),
      Some("\x1b[M`!!")
    );
    assert_eq!(
      enc(
        &emu,
        Mouse {
          ctrl: true,
          shift: true,
          ..mouse(Button::Right, MouseAction::Press, 0, 0)
        }
      )
      .as_deref(),
      Some("\x1b[M6!!")
    );
    // Too far right for the default encoding.
    assert_eq!(
      enc(&emu, mouse(Button::Left, MouseAction::Press, 0, 95)),
      None
    );
  }

  #[test]
  fn reports_sgr_and_motion_modes() {
    let mut emu = Emulator::new(300, 24);
    emu.feed(b"\x1b[?1002h\x1b[?1006h");
    let enc = |emu: &Emulator, m: Mouse| emu.encode_mouse(m).map(|b| String::from_utf8(b).unwrap());
    assert_eq!(
      enc(&emu, mouse(Button::Left, MouseAction::Press, 1, 200)).as_deref(),
      Some("\x1b[<0;201;2M")
    );
    assert_eq!(
      enc(&emu, mouse(Button::Left, MouseAction::Move, 1, 201)).as_deref(),
      Some("\x1b[<32;202;2M")
    );
    assert_eq!(
      enc(&emu, mouse(Button::Left, MouseAction::Release, 1, 201)).as_deref(),
      Some("\x1b[<0;202;2m")
    );
    // Drag mode reports motion only with a button held.
    assert_eq!(
      enc(&emu, mouse(Button::None, MouseAction::Move, 1, 1)),
      None
    );
    assert_eq!(
      enc(&emu, mouse(Button::WheelDown, MouseAction::Press, 0, 0)).as_deref(),
      Some("\x1b[<65;1;1M")
    );
    emu.feed(b"\x1b[?1003h");
    assert_eq!(
      enc(&emu, mouse(Button::None, MouseAction::Move, 1, 1)).as_deref(),
      Some("\x1b[<35;2;2M")
    );
  }

  #[test]
  fn reports_utf8_coordinates() {
    let mut emu = Emulator::new(300, 24);
    emu.feed(b"\x1b[?1000h\x1b[?1005h");
    let bytes = emu
      .encode_mouse(mouse(Button::Left, MouseAction::Press, 0, 199))
      .unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), "\x1b[M \u{e8}!");
  }

  #[test]
  fn maps_the_256_colour_table() {
    assert_eq!(indexed(1, &DARK), 0xcc0000);
    assert_eq!(indexed(16, &DARK), 0x000000);
    assert_eq!(indexed(21, &DARK), 0x0000ff);
    assert_eq!(indexed(231, &DARK), 0xffffff);
    assert_eq!(indexed(232, &DARK), 0x080808);
    assert_eq!(indexed(255, &DARK), 0xeeeeee);
  }

  #[test]
  fn encodes_keys_like_xterm() {
    let enc = |k: Key<'_>| encode_key(k, false).map(|b| String::from_utf8(b).unwrap());
    assert_eq!(
      enc(Key {
        key_char: Some("a"),
        ..key("a")
      })
      .as_deref(),
      Some("a")
    );
    assert_eq!(
      enc(Key {
        key_char: Some("A"),
        shift: true,
        ..key("a")
      })
      .as_deref(),
      Some("A")
    );
    assert_eq!(
      enc(Key {
        ctrl: true,
        ..key("c")
      })
      .as_deref(),
      Some("\x03")
    );
    assert_eq!(
      enc(Key {
        alt: true,
        key_char: Some("b"),
        ..key("b")
      })
      .as_deref(),
      Some("\x1bb")
    );
    assert_eq!(enc(key("enter")).as_deref(), Some("\r"));
    assert_eq!(enc(key("backspace")).as_deref(), Some("\x7f"));
    assert_eq!(
      enc(Key {
        shift: true,
        ..key("tab")
      })
      .as_deref(),
      Some("\x1b[Z")
    );
    assert_eq!(enc(key("up")).as_deref(), Some("\x1b[A"));
    assert_eq!(
      enc(Key {
        ctrl: true,
        ..key("left")
      })
      .as_deref(),
      Some("\x1b[1;5D")
    );
    assert_eq!(enc(key("pageup")).as_deref(), Some("\x1b[5~"));
    assert_eq!(
      enc(Key {
        shift: true,
        ..key("delete")
      })
      .as_deref(),
      Some("\x1b[3;2~")
    );
    assert_eq!(enc(key("f5")).as_deref(), Some("\x1b[15~"));
    assert_eq!(
      enc(Key {
        platform: true,
        ..key("c")
      }),
      None
    );
    assert_eq!(enc(key("shift")), None);
    assert_eq!(
      encode_key(key("up"), true)
        .map(|b| String::from_utf8(b).unwrap())
        .as_deref(),
      Some("\x1bOA")
    );
  }

  #[test]
  fn reuses_the_frame_until_the_grid_changes() {
    let mut emu = Emulator::new(10, 2);
    emu.feed(b"ab");
    let first = emu.frame(&DARK);
    assert!(Arc::ptr_eq(&first, &emu.frame(&DARK)));
    assert!(!Arc::ptr_eq(&first, &emu.frame(&LIGHT)));
    emu.select_start(0, 0, false, SelectKind::Simple);
    emu.select_extend(0, 1, true);
    assert_eq!(emu.frame(&LIGHT).lines[0][0].bg, Some(LIGHT.selection));
    emu.clear_selection();
    assert_eq!(emu.frame(&LIGHT).lines[0][0].bg, None);
    emu.feed(b"c");
    assert_eq!(emu.frame(&LIGHT).lines[0][0].text, "abc");
    // Typing scrolls to the bottom; already there, nothing changed.
    let drawn = emu.frame(&LIGHT);
    emu.scroll_to_bottom();
    assert!(Arc::ptr_eq(&drawn, &emu.frame(&LIGHT)));
    emu.feed(b"\r\n1\r\n2\r\n3");
    emu.scroll(1);
    assert_eq!(emu.frame(&LIGHT).lines[0][0].text, "1");
    emu.scroll_to_bottom();
    assert_eq!(emu.frame(&LIGHT).lines[0][0].text, "2");
  }

  #[test]
  fn builds_again_only_the_rows_that_changed() {
    let mut emu = Emulator::new(10, 3);
    emu.feed(b"one\r\ntwo\r\n$ ");
    let first = emu.frame(&DARK);
    emu.feed(b"l");
    let second = emu.frame(&DARK);
    assert!(Arc::ptr_eq(&first.lines[0], &second.lines[0]));
    assert_eq!(second.lines[2][0].text, "$ l");
    assert_eq!(second.cursor, Some((2, 3)));
    // The damage doesn't cover a selection: it redraws every row.
    emu.select_start(0, 0, false, SelectKind::Line);
    assert_eq!(emu.frame(&DARK).lines[0][0].bg, Some(DARK.selection));
    emu.clear_selection();
    assert_eq!(emu.frame(&DARK).lines[0][0].bg, None);
  }

  /// What a frame costs on a full screen of colored output: built after
  /// output scrolls the screen, after an echo, and reused when nothing
  /// changed.
  /// `cargo test --features gpui-shell --lib terminal_emu::tests::frame_cost -- --ignored --nocapture`
  #[test]
  #[ignore = "benchmark"]
  fn frame_cost() {
    let mut emu = Emulator::new(250, 70);
    for i in 0..1000 {
      emu.feed(
        format!(
          "\x1b[3{}m{i:>6}\x1b[0m \x1b[1;3{}m✓ passed\x1b[0m src/日本/mod_{i}.rs:7 \x1b[4{}m##\x1b[0m {}\r\n",
          i % 8,
          (i + 3) % 8,
          i % 8,
          "word ".repeat(10 + i % 30)
        )
        .as_bytes(),
      );
    }
    let n = 200;
    let start = std::time::Instant::now();
    for i in 0..n {
      emu.feed(format!("\x1b[3{}m{i}\x1b[0m {}\r\n", i % 8, "word ".repeat(30)).as_bytes());
      std::hint::black_box(emu.frame(&DARK));
    }
    let scrolled = start.elapsed() / n;
    let start = std::time::Instant::now();
    for _ in 0..n {
      emu.feed(b"x");
      std::hint::black_box(emu.frame(&DARK));
    }
    let echoed = start.elapsed() / n;
    let start = std::time::Instant::now();
    for _ in 0..n {
      std::hint::black_box(emu.frame(&DARK));
    }
    let reused = start.elapsed() / n;
    println!("frame 250x70: scrolled {scrolled:?}, echoed {echoed:?}, reused {reused:?}");
  }

  #[test]
  fn wraps_pastes_in_bracketed_mode() {
    assert_eq!(paste_bytes("a\nb", false), b"a\rb");
    assert_eq!(paste_bytes("x\x1b[201~", true), b"\x1b[200~x\x1b[201~");
  }
}
