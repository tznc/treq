//! Terminal emulation helpers independent of the UI: key encoding, the
//! xterm palette, and turning a `vt100` screen row into text plus styled
//! runs.

use std::ops::Range;

/// Bytes a terminal key sends. `key` uses GPUI key names (`enter`, `up`,
/// lowercase characters); `application_cursor` is DECCKM.
pub fn key_bytes(key: &str, control: bool, alt: bool, application_cursor: bool) -> Option<Vec<u8>> {
  let cursor = |c: char| {
    if application_cursor {
      format!("\x1bO{c}").into_bytes()
    } else {
      format!("\x1b[{c}").into_bytes()
    }
  };
  let bytes = match key {
    "enter" => b"\r".to_vec(),
    "backspace" => b"\x7f".to_vec(),
    "tab" => b"\t".to_vec(),
    "escape" => b"\x1b".to_vec(),
    "up" => cursor('A'),
    "down" => cursor('B'),
    "right" => cursor('C'),
    "left" => cursor('D'),
    "home" => cursor('H'),
    "end" => cursor('F'),
    "delete" => b"\x1b[3~".to_vec(),
    "pageup" => b"\x1b[5~".to_vec(),
    "pagedown" => b"\x1b[6~".to_vec(),
    key if control => {
      let mut chars = key.chars();
      let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
      };
      match c {
        'a'..='z' => vec![c as u8 - b'a' + 1],
        '@' | ' ' | '2' => vec![0],
        '[' => vec![0x1b],
        '\\' => vec![0x1c],
        ']' => vec![0x1d],
        _ => return None,
      }
    }
    key if alt => {
      let mut bytes = vec![0x1b];
      bytes.extend_from_slice(key.as_bytes());
      bytes
    }
    // Printable keys arrive as text through the input handler.
    _ => return None,
  };
  Some(bytes)
}

/// The 256-color xterm palette as `0xRRGGBB`.
pub fn palette(index: u8) -> u32 {
  const BASE: [u32; 16] = [
    0x1e1e1e, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
    0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
  ];
  match index {
    0..=15 => BASE[index as usize],
    16..=231 => {
      let i = index - 16;
      let level = |v: u8| if v == 0 { 0 } else { 55 + v as u32 * 40 };
      (level(i / 36) << 16) | (level(i / 6 % 6) << 8) | level(i % 6)
    }
    232..=255 => {
      let v = 8 + (index - 232) as u32 * 10;
      (v << 16) | (v << 8) | v
    }
  }
}

pub const DEFAULT_FG: u32 = 0xd4d4d4;
pub const DEFAULT_BG: u32 = 0x1e1e1e;

fn rgb(color: vt100::Color, default: u32) -> u32 {
  match color {
    vt100::Color::Default => default,
    vt100::Color::Idx(i) => palette(i),
    vt100::Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
  }
}

/// How a run of cells is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellStyle {
  pub fg: u32,
  /// `None` for the terminal background.
  pub bg: Option<u32>,
  pub bold: bool,
  pub underline: bool,
}

/// One screen row: its text (wide-character continuation cells dropped)
/// and the styled byte ranges of that text.
pub struct Row {
  pub text: String,
  pub runs: Vec<(Range<usize>, CellStyle)>,
}

/// Builds row `row` of `screen`. `cursor` is the cursor column on this row,
/// drawn inverted.
pub fn row(screen: &vt100::Screen, row: u16, cols: u16, cursor: Option<u16>) -> Row {
  let mut text = String::new();
  let mut runs: Vec<(Range<usize>, CellStyle)> = Vec::new();
  for col in 0..cols {
    let Some(cell) = screen.cell(row, col) else {
      break;
    };
    if cell.is_wide_continuation() {
      continue;
    }
    let mut fg = rgb(cell.fgcolor(), DEFAULT_FG);
    let mut bg = match cell.bgcolor() {
      vt100::Color::Default => None,
      color => Some(rgb(color, DEFAULT_BG)),
    };
    if cell.inverse() != (cursor == Some(col)) {
      let back = bg.unwrap_or(DEFAULT_BG);
      bg = Some(fg);
      fg = back;
    }
    let style = CellStyle {
      fg,
      bg,
      bold: cell.bold(),
      underline: cell.underline(),
    };
    let start = text.len();
    match cell.contents() {
      "" => text.push(' '),
      contents => text.push_str(contents),
    }
    match runs.last_mut() {
      Some((range, last)) if *last == style => range.end = text.len(),
      _ => runs.push((start..text.len(), style)),
    }
  }
  Row { text, runs }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn encodes_named_and_control_keys() {
    assert_eq!(
      key_bytes("enter", false, false, false),
      Some(b"\r".to_vec())
    );
    assert_eq!(
      key_bytes("up", false, false, false),
      Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
      key_bytes("up", false, false, true),
      Some(b"\x1bOA".to_vec())
    );
    assert_eq!(key_bytes("c", true, false, false), Some(vec![3]));
    assert_eq!(key_bytes("d", true, false, false), Some(vec![4]));
    assert_eq!(key_bytes("x", false, true, false), Some(b"\x1bx".to_vec()));
    assert_eq!(key_bytes("x", false, false, false), None);
  }

  #[test]
  fn xterm_palette() {
    assert_eq!(palette(1), 0xcd3131);
    assert_eq!(palette(16), 0x000000);
    assert_eq!(palette(231), 0xffffff);
    assert_eq!(palette(232), 0x080808);
  }

  #[test]
  fn rows_merge_styles_and_invert_the_cursor() {
    let mut parser = vt100::Parser::new(2, 10, 0);
    parser.process(b"ab\x1b[31mcd\x1b[0m");
    let row = row(parser.screen(), 0, 10, Some(4));
    assert_eq!(row.text, "abcd      ");
    assert_eq!(row.runs[0].0, 0..2);
    assert_eq!(row.runs[1].0, 2..4);
    assert_eq!(row.runs[1].1.fg, palette(1));
    // The cursor cell (column 4) is inverted.
    let cursor = row.runs.iter().find(|(r, _)| r.start == 4).unwrap();
    assert_eq!(cursor.1.bg, Some(DEFAULT_FG));
  }
}
