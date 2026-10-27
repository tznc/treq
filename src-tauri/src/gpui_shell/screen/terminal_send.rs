//! `treq send` attachments over a terminal (`TerminalSendPreviews`), and
//! the review an agent gets of one (`sendAssetReview.ts`).
//!
//! The backend keeps the attachments (`core::agent_launch::send_assets`);
//! GPUI draws them, and drops one when it is dismissed. React keeps its own
//! copy (`treqSendStore`) for the browser panel.

use std::path::Path;

pub use crate::core::agent_launch::SendAsset;

impl SendAsset {
  pub fn is_image(&self) -> bool {
    self.media_type == "image"
  }
}

/// `assetsForPtySession`, less the browser previews the browser panel
/// shows: the attachments sent to `pty`, and those sent to no terminal
/// while it is the active one.
pub fn assets_for(assets: &[SendAsset], pty: &str, active: bool) -> Vec<SendAsset> {
  assets
    .iter()
    .filter(|a| a.media_type != "browser")
    .filter(|a| match &a.pty_session_id {
      Some(id) => id == pty,
      None => active,
    })
    .cloned()
    .collect()
}

/// `shellQuote`: one POSIX shell argument.
pub fn shell_quote(value: &str) -> String {
  format!("'{}'", value.replace('\'', "'\\''"))
}

/// A region of an image, in fractions of its size.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

fn round_norm(value: f64) -> f64 {
  (value * 10000.).round() / 10000.
}

/// `clampNormalizedRect`: the part of `rect` inside the image.
pub fn clamp_rect(rect: Rect) -> Rect {
  let unit = |v: f64| v.clamp(0., 1.);
  let (x, y) = (unit(rect.x), unit(rect.y));
  let (right, bottom) = (unit(rect.x + rect.width), unit(rect.y + rect.height));
  Rect {
    x: round_norm(x.min(right)),
    y: round_norm(y.min(bottom)),
    width: round_norm((right - x).abs()),
    height: round_norm((bottom - y).abs()),
  }
}

/// `rectFromDrag`: the region a drag from `start` to `end` drew over an
/// image `size` big, or `None` for a click.
pub fn rect_from_drag(start: (f64, f64), end: (f64, f64), size: (f64, f64)) -> Option<Rect> {
  let (w, h) = size;
  if w <= 0. || h <= 0. {
    return None;
  }
  let (x1, y1, x2, y2) = (start.0 / w, start.1 / h, end.0 / w, end.1 / h);
  let rect = clamp_rect(Rect {
    x: x1.min(x2),
    y: y1.min(y2),
    width: (x2 - x1).abs(),
    height: (y2 - y1).abs(),
  });
  (rect.width >= 0.01 || rect.height >= 0.01).then_some(rect)
}

/// `ImageHighlightComment`.
#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
  pub id: u64,
  pub rect: Rect,
  pub comment: String,
}

/// `AssetLineComment`: a comment on lines `start..=end` (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct LineComment {
  pub id: u64,
  pub start: usize,
  pub end: usize,
  pub lines: Vec<String>,
  pub text: String,
}

/// `TextAssetLineReview` splits a file this way; an empty one is one line.
pub fn text_lines(content: &str) -> Vec<&str> {
  content.split('\n').collect()
}

/// A Markdown text attachment draws rendered until its Source tab is picked.
pub fn renders_markdown(asset: &SendAsset) -> bool {
  asset.media_type == "text" && super::file_browser_data::is_markdown(&asset.path)
}

/// The top-level blocks of Markdown `lines`, as 1-based inclusive line
/// ranges: runs of non-blank lines, with a fenced code block kept whole
/// across its blank lines.
pub fn markdown_blocks(lines: &[&str]) -> Vec<(usize, usize)> {
  let mut blocks = Vec::new();
  let mut start = None;
  let mut fence: Option<&str> = None;
  for (ix, line) in lines.iter().enumerate() {
    let n = ix + 1;
    let trimmed = line.trim_start();
    if let Some(open) = fence {
      if trimmed.starts_with(open) {
        fence = None;
        blocks.extend(start.take().map(|s| (s, n)));
      }
      continue;
    }
    if trimmed.is_empty() {
      if let Some(s) = start.take() {
        blocks.push((s, n - 1));
      }
      continue;
    }
    if start.is_none() {
      start = Some(n);
    }
    fence = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f));
  }
  if let Some(s) = start {
    blocks.push((s, lines.len()));
  }
  blocks
}

/// The block a comment ending on `line` shows under: the one holding it,
/// or for a blank line between blocks, the one above.
pub fn block_at_or_before(blocks: &[(usize, usize)], line: usize) -> Option<usize> {
  blocks.iter().rposition(|&(start, _)| start <= line)
}

/// Points relative link and image targets in `text` at `dir`, the
/// attachment's folder, so they open and load from where the file lives.
pub fn resolve_relative_links(text: &str, dir: &str) -> String {
  #[expect(clippy::expect_used, reason = "constant pattern")]
  static LINK: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"\]\(([^)\s]+)").expect("regex"));
  LINK
    .replace_all(text, |c: &regex::Captures<'_>| {
      let target = &c[1];
      let external = target.starts_with(['#', '/', '<']) || target.contains(':');
      if external {
        c[0].to_string()
      } else {
        format!("]({}", Path::new(dir).join(target).display())
      }
    })
    .into_owned()
}

/// Whether a text attachment's panel draws dark. The lightbox is dark, but
/// rendered Markdown takes the app's theme, which gpui-kit's view follows.
pub fn text_panel_dark(rendered: bool, app_dark: bool) -> bool {
  !rendered || app_dark
}

/// "Suggest change": the picked lines as a ```` ```suggestion ```` block to
/// edit into what they should read, as code review starts one.
pub fn suggestion_for(lines: &[String]) -> String {
  format!("```suggestion\n{}\n```\n", lines.join("\n"))
}

/// `hasUnsentSendAssetReview`.
pub fn has_unsent(general: &str, highlights: &[Highlight], lines: &[LineComment]) -> bool {
  !general.trim().is_empty()
    || lines.iter().any(|c| !c.text.trim().is_empty())
    || highlights
      .iter()
      .any(|h| !h.comment.trim().is_empty() || h.rect.width > 0. || h.rect.height > 0.)
}

/// What [`review_prompt`] writes up.
pub struct Review<'a> {
  pub title: &'a str,
  pub path: &'a str,
  pub general: &'a str,
  pub highlights: &'a [Highlight],
  pub lines: &'a [LineComment],
  /// The image with its highlights drawn on, when it could be written.
  pub annotated: Option<&'a str>,
}

/// JavaScript's `Math.round(v * 100)` as a percentage.
fn pct(value: f64) -> String {
  format!("{}%", (value * 100. + 0.5).floor() as i64)
}

/// `formatSendAssetReviewPrompt`.
pub fn review_prompt(review: &Review<'_>) -> String {
  let mut out: Vec<String> = vec![format!("Review of {}", review.title), String::new()];
  match review.annotated {
    Some(path) => out.push(format!("Annotated image with highlighted regions: {path}")),
    None => out.push(format!("File: {}", review.path)),
  }
  out.push(String::new());
  let general = review.general.trim();
  if !general.is_empty() {
    out.push(general.to_string());
    out.push(String::new());
  }
  let highlights: Vec<&Highlight> = review
    .highlights
    .iter()
    .filter(|h| h.rect.width > 0. && h.rect.height > 0.)
    .collect();
  if !highlights.is_empty() {
    out.push("Highlights:".into());
    for (index, h) in highlights.iter().enumerate() {
      let r = h.rect;
      let comment = h.comment.trim();
      out.push(format!(
        "{}. Region {},{} {}×{}: {}",
        index + 1,
        pct(r.x),
        pct(r.y),
        pct(r.width),
        pct(r.height),
        if comment.is_empty() {
          "(no comment)"
        } else {
          comment
        }
      ));
    }
    out.push(String::new());
  }
  let comments: Vec<&LineComment> = review
    .lines
    .iter()
    .filter(|c| !c.text.trim().is_empty())
    .collect();
  if !comments.is_empty() {
    out.push("Inline comments:".into());
    out.push(String::new());
    for c in comments {
      let reference = if c.start == c.end {
        format!("{}:{}", review.title, c.start)
      } else {
        format!("{}:{}:{}", review.title, c.start, c.end)
      };
      let mut block = format!("{reference}\n");
      if !c.lines.is_empty() {
        block.push_str(&format!("```\n{}\n```\n", c.lines.join("\n")));
      }
      block.push_str(&format!("> {}\n", c.text.trim()));
      out.push(block);
      out.push(String::new());
    }
  }
  format!("{}\n", out.join("\n").trim())
}

/// `renderHighlightedImageBlob`: the image at `path` with each highlight
/// stroked in red, as a PNG.
pub fn highlighted_png(path: &Path, highlights: &[Highlight]) -> Result<Vec<u8>, String> {
  let mut image = image::open(path)
    .map_err(|e| format!("Failed to load image for highlight: {e}"))?
    .to_rgba8();
  let (w, h) = image.dimensions();
  let line = 2u32.max((f64::from(w.min(h)) * 0.006).round() as u32);
  let red = image::Rgba([0xef, 0x44, 0x44, 0xff]);
  for highlight in highlights {
    let r = clamp_rect(highlight.rect);
    // `strokeRect` centres its line on the edge.
    let left = r.x * f64::from(w);
    let top = r.y * f64::from(h);
    let right = left + r.width * f64::from(w);
    let bottom = top + r.height * f64::from(h);
    let half = f64::from(line) / 2.;
    let mut fill = |x0: f64, y0: f64, x1: f64, y1: f64| {
      let clamp = |v: f64, max: u32| (v.round().max(0.) as u32).min(max);
      for y in clamp(y0, h)..clamp(y1, h) {
        for x in clamp(x0, w)..clamp(x1, w) {
          image.put_pixel(x, y, red);
        }
      }
    };
    fill(left - half, top - half, right + half, top + half);
    fill(left - half, bottom - half, right + half, bottom + half);
    fill(left - half, top - half, left + half, bottom + half);
    fill(right - half, top - half, right + half, bottom + half);
  }
  let mut out = std::io::Cursor::new(Vec::new());
  image
    .write_to(&mut out, image::ImageFormat::Png)
    .map_err(|e| format!("Failed to encode highlighted image: {e}"))?;
  Ok(out.into_inner())
}

/// An image's pixel size, read from its header.
pub fn image_size(path: &Path) -> Option<(u32, u32)> {
  image::image_dimensions(path).ok()
}

/// `rememberFittedBaseSize`: the size an image of `natural` size shows at
/// unzoomed, fit within 80% of the window's height and the lightbox's width.
pub fn fitted_size(natural: (u32, u32), window: (f32, f32), parent_width: f32) -> (f32, f32) {
  let (nw, nh) = (natural.0 as f32, natural.1 as f32);
  if nw <= 0. || nh <= 0. {
    return (0., 0.);
  }
  let max_h = window.1 * 0.8;
  let max_w = (window.0 * 0.9).min(parent_width.max(window.0 * 0.75));
  let scale = (max_w / nw).min(max_h / nh);
  (nw * scale, nh * scale)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn asset(id: &str, pty: Option<&str>, media: &str) -> SendAsset {
    SendAsset {
      id: id.into(),
      repo: "/repo".into(),
      pty_session_id: pty.map(str::to_string),
      media_type: media.into(),
      path: format!("/tmp/{id}"),
      title: id.into(),
    }
  }

  #[test]
  fn picks_the_terminals_attachments() {
    let assets = [
      asset("a", Some("pty-1"), "image"),
      asset("b", Some("pty-2"), "text"),
      asset("c", None, "text"),
      asset("d", Some("pty-1"), "browser"),
    ];
    let ids = |active| {
      assets_for(&assets, "pty-1", active)
        .into_iter()
        .map(|a| a.id)
        .collect::<Vec<_>>()
    };
    assert_eq!(ids(false), ["a"]);
    assert_eq!(ids(true), ["a", "c"]);
  }

  #[test]
  fn renders_markdown_attachments_by_default() {
    let mut md = asset("a", None, "text");
    md.path = "/tmp/HANDOFF.MD".into();
    assert!(renders_markdown(&md));
    md.path = "/tmp/notes.markdown".into();
    assert!(renders_markdown(&md));
    md.path = "/tmp/notes.txt".into();
    assert!(!renders_markdown(&md));
    let mut img = asset("b", None, "image");
    img.path = "/tmp/shot.md".into();
    assert!(!renders_markdown(&img));
  }

  #[test]
  fn maps_markdown_blocks_to_source_lines() {
    let src =
      "# Title\n\nPara one\nstill one\n\n```rs\nfn a() {}\n\nfn b() {}\n```\n- a\n- b\n\n\n> quote";
    assert_eq!(
      markdown_blocks(&text_lines(src)),
      [(1, 1), (3, 4), (6, 10), (11, 12), (15, 15)]
    );
    assert!(markdown_blocks(&text_lines("")).is_empty());
    // An unclosed fence runs to the end.
    assert_eq!(markdown_blocks(&text_lines("~~~\na\n\nb")), [(1, 4)]);
  }

  #[test]
  fn resolves_relative_links_against_the_file() {
    let md = "![shot](img/a.png) [doc](../b.md) [web](https://x.io) [top](#h) [abs](/etc/c.md)";
    assert_eq!(
      resolve_relative_links(md, "/repo/docs"),
      "![shot](/repo/docs/img/a.png) [doc](/repo/docs/../b.md) [web](https://x.io) [top](#h) [abs](/etc/c.md)"
    );
  }

  #[test]
  fn markdown_cards_get_the_markdown_icon() {
    use super::super::terminal::send_asset_icon;
    use crate::gpui_shell::theme;
    let mut md = asset("a", None, "text");
    md.path = "/tmp/handoff.md".into();
    assert_eq!(send_asset_icon(&md), theme::ICON_FILE_MARKDOWN);
    assert_eq!(
      send_asset_icon(&asset("b", None, "text")),
      theme::ICON_FILE_TEXT
    );
  }

  #[test]
  fn rendered_markdown_follows_the_app_theme() {
    // Source stays dark like the rest of the lightbox.
    assert!(text_panel_dark(false, false));
    assert!(text_panel_dark(false, true));
    assert!(!text_panel_dark(true, false));
    assert!(text_panel_dark(true, true));
  }

  #[test]
  fn places_line_ranges_under_their_block() {
    let blocks = [(1, 1), (3, 4), (6, 10)];
    assert_eq!(block_at_or_before(&blocks, 1), Some(0));
    // A blank line between blocks goes with the block above it.
    assert_eq!(block_at_or_before(&blocks, 2), Some(0));
    assert_eq!(block_at_or_before(&blocks, 4), Some(1));
    assert_eq!(block_at_or_before(&blocks, 99), Some(2));
    assert_eq!(block_at_or_before(&blocks, 0), None);
    assert_eq!(block_at_or_before(&[], 3), None);
  }

  #[test]
  fn suggests_a_change_to_the_picked_lines() {
    let lines = ["- a".to_string(), "- b".to_string()];
    assert_eq!(suggestion_for(&lines), "```suggestion\n- a\n- b\n```\n");
  }

  #[test]
  fn quotes_paths_for_the_shell() {
    assert_eq!(shell_quote("/tmp/note.txt"), "'/tmp/note.txt'");
    assert_eq!(shell_quote("it's"), "'it'\\''s'");
  }

  #[test]
  fn reads_a_drag_as_a_region() {
    assert_eq!(
      rect_from_drag((10., 20.), (60., 70.), (100., 100.)),
      Some(Rect {
        x: 0.1,
        y: 0.2,
        width: 0.5,
        height: 0.5
      })
    );
    // Dragged up and left, and past the edge.
    assert_eq!(
      rect_from_drag((50., 50.), (-10., 120.), (100., 100.)),
      Some(Rect {
        x: 0.,
        y: 0.5,
        width: 0.5,
        height: 0.5
      })
    );
    assert_eq!(rect_from_drag((10., 10.), (10.5, 10.5), (100., 100.)), None);
    assert_eq!(rect_from_drag((0., 0.), (5., 5.), (0., 100.)), None);
  }

  fn highlight(rect: Rect, comment: &str) -> Highlight {
    Highlight {
      id: 1,
      rect,
      comment: comment.into(),
    }
  }

  fn line(start: usize, end: usize, lines: &[&str], text: &str) -> LineComment {
    LineComment {
      id: 1,
      start,
      end,
      lines: lines.iter().map(|l| l.to_string()).collect(),
      text: text.into(),
    }
  }

  #[test]
  fn knows_when_a_review_is_unsent() {
    assert!(!has_unsent("  ", &[], &[]));
    assert!(has_unsent("hi", &[], &[]));
    assert!(has_unsent("", &[], &[line(1, 1, &[], "x")]));
    assert!(!has_unsent("", &[], &[line(1, 1, &[], " ")]));
    assert!(has_unsent(
      "",
      &[highlight(
        Rect {
          width: 0.1,
          ..Rect::default()
        },
        ""
      )],
      &[]
    ));
  }

  #[test]
  fn writes_the_review_prompt_like_react() {
    let rect = Rect {
      x: 0.105,
      y: 0.2,
      width: 0.5,
      height: 0.25,
    };
    let prompt = review_prompt(&Review {
      title: "shot.png",
      path: "/tmp/shot.png",
      general: " looks off ",
      highlights: &[
        highlight(rect, "too big"),
        highlight(rect, ""),
        highlight(Rect::default(), "skipped"),
      ],
      lines: &[],
      annotated: Some("/repo/.treq/send/shot-review.png"),
    });
    assert_eq!(
      prompt,
      "Review of shot.png\n\nAnnotated image with highlighted regions: \
       /repo/.treq/send/shot-review.png\n\nlooks off\n\nHighlights:\n1. Region 11%,20% \
       50%×25%: too big\n2. Region 11%,20% 50%×25%: (no comment)\n"
    );
    let prompt = review_prompt(&Review {
      title: "a.rs",
      path: "/tmp/a.rs",
      general: "",
      highlights: &[],
      lines: &[
        line(2, 2, &["let x = 1;"], "rename"),
        line(3, 4, &["a", "b"], " why "),
        line(5, 5, &["c"], " "),
      ],
      annotated: None,
    });
    assert_eq!(
      prompt,
      "Review of a.rs\n\nFile: /tmp/a.rs\n\nInline comments:\n\na.rs:2\n```\nlet x = \
       1;\n```\n> rename\n\n\na.rs:3:4\n```\na\nb\n```\n> why\n"
    );
  }

  #[test]
  fn splits_text_into_lines() {
    assert_eq!(text_lines(""), [""]);
    assert_eq!(text_lines("a\nb\n"), ["a", "b", ""]);
  }

  #[test]
  fn fits_images_in_the_lightbox() {
    // Height-bound: 80% of 1000.
    assert_eq!(
      fitted_size((1000, 2000), (1600., 1000.), 1416.),
      (400., 800.)
    );
    // Width-bound, small images grow to fit.
    let (w, h) = fitted_size((100, 10), (1000., 1000.), 876.);
    assert_eq!(w, 876.);
    assert!((h - 87.6).abs() < 0.001);
    assert_eq!(fitted_size((0, 10), (1000., 1000.), 876.), (0., 0.));
  }

  #[test]
  fn strokes_highlights_onto_the_image() {
    let dir = tempfile::TempDir::new().expect("temp");
    let path = dir.path().join("shot.png");
    image::RgbaImage::from_pixel(100, 50, image::Rgba([0, 0, 0, 255]))
      .save(&path)
      .expect("save");
    assert_eq!(image_size(&path), Some((100, 50)));
    let rect = Rect {
      x: 0.2,
      y: 0.2,
      width: 0.5,
      height: 0.6,
    };
    let png = highlighted_png(&path, &[highlight(rect, "")]).expect("png");
    let out = image::load_from_memory(&png).expect("decode").to_rgba8();
    assert_eq!(out.dimensions(), (100, 50));
    let red = image::Rgba([0xef, 0x44, 0x44, 0xff]);
    // On the left edge (x = 20), not inside the region or outside it.
    assert_eq!(*out.get_pixel(20, 25), red);
    assert_eq!(*out.get_pixel(45, 25), image::Rgba([0, 0, 0, 255]));
    assert_eq!(*out.get_pixel(5, 5), image::Rgba([0, 0, 0, 255]));
    assert!(highlighted_png(&dir.path().join("missing.png"), &[]).is_err());
  }
}
