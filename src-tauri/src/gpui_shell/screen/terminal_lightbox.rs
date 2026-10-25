//! `SendAssetLightbox`: a terminal's `treq send` attachments at full size,
//! over the whole window, with a review to send its agent. Images take
//! highlighted regions (`ImageHighlightLayer`), text files line comments
//! (`TextAssetLineReview`).

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{
  cell::Cell,
  collections::{HashMap, HashSet},
  path::{Path, PathBuf},
  rc::Rc,
};

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use tauri_plugin_gpui::gpui::{
  self, div, img, prelude::*, Bounds, ClipboardItem, Context, Div, Entity, EventEmitter,
  FocusHandle, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels,
  SharedString, Stateful, Subscription, Window,
};

use super::{
  rows::Rows,
  terminal_data::Host,
  terminal_send::{
    fitted_size, has_unsent, highlighted_png, image_size, rect_from_drag, review_prompt,
    shell_quote, text_lines, Highlight, LineComment, Rect, Review, SendAsset,
  },
};
use crate::gpui_shell::{
  native::ToastTone,
  theme::{self, Palette},
  ui::{self, Button, TextSize, Variant},
};

/// `IMAGE_ZOOM_MIN`, `IMAGE_ZOOM_MAX`: one step, from 100% to 200%.
const ZOOM_MIN: f32 = 1.;
const ZOOM_MAX: f32 = 2.;

/// What the lightbox asks of its pane.
#[derive(Debug, Clone, PartialEq)]
pub enum LightboxEvent {
  Close,
  /// The review, to type into the agent.
  SendReview(String),
  Toast {
    title: String,
    description: String,
    tone: ToastTone,
  },
}

/// The line range being commented on, before its comment is added.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pending {
  start: usize,
  end: usize,
}

/// A text line's vertical padding: 23px rows, which the virtual list's
/// scroll math is tuned to.
const LINE_PADDING_Y: f32 = 1.5;

/// The lightbox is dark in both themes: its text and lines at `alpha`.
fn chrome(alpha: f32) -> Hsla {
  theme::palette(true).foreground.opacity(alpha)
}

/// Behind the lightbox's controls, which sit over the media.
fn scrim(alpha: f32) -> Hsla {
  theme::palette(true).scrim().alpha(alpha)
}

pub struct Lightbox {
  host: Host,
  assets: Vec<SendAsset>,
  index: usize,
  zoom: f32,
  /// Only an agent's terminal takes reviews (`onSendReview`).
  can_review: bool,
  general: Option<Entity<InputState>>,
  general_text: String,
  /// Whether the comment box was last disabled.
  general_disabled: Option<bool>,
  /// The review went: empty the comment box when next drawn.
  clear_general: bool,
  highlights: HashMap<String, Vec<Highlight>>,
  active_highlight: Option<u64>,
  highlight_inputs: HashMap<u64, Entity<TextareaState>>,
  lines: HashMap<String, Vec<LineComment>>,
  pending: Option<Pending>,
  composer: Option<Entity<TextareaState>>,
  composer_text: String,
  focus_composer: bool,
  /// The line a drag across lines started on.
  anchor: Option<usize>,
  /// Text attachments' contents, once read, line by line.
  texts: HashMap<String, Rc<Vec<String>>>,
  /// The shown text's rows, by the asset's index.
  text_rows: Rows<(usize, TextRow)>,
  reading: HashSet<String>,
  sizes: HashMap<String, Option<(u32, u32)>>,
  /// An image drag: where it started on the image, and the region so far.
  drag: Option<(f64, f64)>,
  draft: Option<Rect>,
  surface: Rc<Cell<Option<Bounds<Pixels>>>>,
  discard: bool,
  sending: bool,
  next_id: u64,
  focus: FocusHandle,
  focus_root: bool,
  _subscriptions: Vec<Subscription>,
}

/// A row of a text attachment: a line, by its number, the comment box
/// under the lines being commented on, or a comment, by its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextRow {
  Line(usize),
  Composer,
  Comment(u64),
}

/// A text this long draws its lines as a virtual list in a box of fixed
/// height; a shorter one sizes the box to its lines.
const LONG_TEXT: usize = 300;

impl EventEmitter<LightboxEvent> for Lightbox {}

impl Lightbox {
  pub fn new(
    host: Host,
    assets: Vec<SendAsset>,
    index: usize,
    can_review: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let mut this = Self {
      host,
      assets: Vec::new(),
      index,
      zoom: ZOOM_MIN,
      can_review,
      general: None,
      general_text: String::new(),
      general_disabled: None,
      clear_general: false,
      highlights: HashMap::new(),
      active_highlight: None,
      highlight_inputs: HashMap::new(),
      lines: HashMap::new(),
      pending: None,
      composer: None,
      composer_text: String::new(),
      focus_composer: false,
      anchor: None,
      texts: HashMap::new(),
      text_rows: Rows::default(),
      reading: HashSet::new(),
      sizes: HashMap::new(),
      drag: None,
      draft: None,
      surface: Rc::default(),
      discard: false,
      sending: false,
      next_id: 0,
      focus: cx.focus_handle(),
      focus_root: true,
      _subscriptions: Vec::new(),
    };
    this.set_assets(assets, cx);
    this.index = index.min(this.assets.len().saturating_sub(1));
    this
  }

  /// Follows the terminal's attachments as React's store changes,
  /// staying on the one shown while it is still there.
  pub fn set_assets(&mut self, assets: Vec<SendAsset>, cx: &mut Context<Self>) {
    if assets == self.assets {
      return;
    }
    let shown = self.current().map(|a| a.id.clone());
    self.assets = assets;
    let index = shown
      .and_then(|id| self.assets.iter().position(|a| a.id == id))
      .unwrap_or(self.index);
    let index = index.min(self.assets.len().saturating_sub(1));
    if index != self.index {
      self.show(index);
    }
    self.read_texts(cx);
    cx.notify();
  }

  fn current(&self) -> Option<&SendAsset> {
    self.assets.get(self.index)
  }

  /// Shows the asset at `index`, unzoomed (`currentIndex`'s effect).
  fn show(&mut self, index: usize) {
    self.index = index;
    self.zoom = ZOOM_MIN;
    self.active_highlight = None;
  }

  fn read_texts(&mut self, cx: &mut Context<Self>) {
    for asset in self.assets.iter().filter(|a| a.media_type == "text") {
      let path = asset.path.clone();
      if self.texts.contains_key(&path) || !self.reading.insert(path.clone()) {
        continue;
      }
      let host = self.host.clone();
      // The file is read and split into lines off the main thread.
      let read = cx.background_executor().spawn(async move {
        let text = host.read_send_asset(&path).unwrap_or_default();
        text_lines(&text)
          .into_iter()
          .map(str::to_string)
          .collect::<Vec<_>>()
      });
      let path = asset.path.clone();
      cx.spawn(async move |this, cx| {
        let lines = read.await;
        this
          .update(cx, |view, cx| {
            view.texts.insert(path, Rc::new(lines));
            cx.notify();
          })
          .ignore_closed();
      })
      .detach();
    }
  }

  fn current_highlights(&self) -> &[Highlight] {
    self
      .current()
      .and_then(|a| self.highlights.get(&a.id))
      .map_or(&[], Vec::as_slice)
  }

  fn current_lines(&self) -> &[LineComment] {
    self
      .current()
      .and_then(|a| self.lines.get(&a.id))
      .map_or(&[], Vec::as_slice)
  }

  /// `hasUnsent`: a comment not yet sent, or a line comment being written.
  pub fn has_unsent(&self) -> bool {
    has_unsent(
      &self.general_text,
      self.current_highlights(),
      self.current_lines(),
    ) || self.pending.is_some()
  }

  fn request_close(&mut self, cx: &mut Context<Self>) {
    if self.has_unsent() {
      self.discard = true;
      cx.notify();
    } else {
      cx.emit(LightboxEvent::Close);
    }
  }

  fn go(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
    let next = self.index as isize + delta;
    if next < 0 || next as usize >= self.assets.len() {
      return;
    }
    self.show(next as usize);
    window.focus(&self.focus, cx);
    cx.notify();
  }

  fn toast(&self, title: &str, description: String, tone: ToastTone, cx: &mut Context<Self>) {
    cx.emit(LightboxEvent::Toast {
      title: title.into(),
      description,
      tone,
    });
  }

  fn copy(&mut self, cx: &mut Context<Self>) {
    let Some(asset) = self.current() else {
      return;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(shell_quote(&asset.path)));
    self.toast("Asset path copied", String::new(), ToastTone::Success, cx);
  }

  fn reveal(&mut self) {
    if let Some(asset) = self.current() {
      self.host.reveal(&asset.path);
    }
  }

  fn toggle_zoom(&mut self, cx: &mut Context<Self>) {
    self.zoom = if self.zoom == ZOOM_MIN {
      ZOOM_MAX
    } else {
      ZOOM_MIN
    };
    cx.notify();
  }

  fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
    self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    cx.notify();
  }

  fn send_review(&mut self, cx: &mut Context<Self>) {
    if !self.can_review || self.sending || !self.has_unsent() {
      return;
    }
    let Some(asset) = self.current().cloned() else {
      return;
    };
    self.sending = true;
    let highlights = self.current_highlights().to_vec();
    let draw = asset.is_image() && !highlights.is_empty();
    let host = self.host.clone();
    let (path, repo, name) = (
      asset.path.clone(),
      asset.repo.clone(),
      format!("{}-review.png", asset.title),
    );
    let drawn = draw.then(|| highlights.clone());
    let annotate = cx.background_executor().spawn(async move {
      let highlights = drawn?;
      Some(
        highlighted_png(Path::new(&path), &highlights)
          .and_then(|png| host.write_send_review_image(&repo, &name, &png)),
      )
    });
    cx.spawn(async move |this, cx| {
      let annotated = annotate.await;
      this
        .update(cx, |view, cx| {
          let annotated = match annotated {
            Some(Ok(path)) => Some(path),
            Some(Err(error)) => {
              view.toast(
                "Could not attach highlighted image",
                error,
                ToastTone::Error,
                cx,
              );
              None
            }
            None => None,
          };
          let prompt = review_prompt(&Review {
            title: &asset.title,
            path: &asset.path,
            general: &view.general_text,
            highlights: &highlights,
            lines: view.lines.get(&asset.id).map_or(&[], Vec::as_slice),
            annotated: annotated.as_deref(),
          });
          cx.emit(LightboxEvent::SendReview(prompt));
          view.general_text.clear();
          view.highlights.insert(asset.id.clone(), Vec::new());
          view.lines.insert(asset.id.clone(), Vec::new());
          view.active_highlight = None;
          view.pending = None;
          view.sending = false;
          view.clear_general = true;
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }
}

/// Inputs, created once a window is at hand.
impl Lightbox {
  fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let general = match &self.general {
      Some(general) => general.clone(),
      None => {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a review comment…"));
        self
          ._subscriptions
          .push(
            cx.subscribe(&input, |view, input, event: &InputEvent, cx| match event {
              InputEvent::Change => {
                view.general_text = input.read(cx).value().to_string();
                cx.notify();
              }
              InputEvent::PressEnter { .. } => view.send_review(cx),
              _ => {}
            }),
          );
        self.general = Some(input.clone());
        input
      }
    };
    let disabled = !self.can_review || self.sending;
    let clear = std::mem::take(&mut self.clear_general);
    let toggle = self.general_disabled != Some(disabled);
    self.general_disabled = Some(disabled);
    general.update(cx, |input, cx| {
      if clear {
        input.set_value("", window, cx);
      }
      if toggle {
        input.set_disabled(disabled, cx);
      }
    });

    match (self.pending, &self.composer) {
      (None, Some(_)) => self.composer = None,
      (Some(_), None) => {
        let composer = cx.new(|cx| {
          TextareaState::new(window, cx)
            .rows(3)
            .placeholder("Comment on these lines")
        });
        self._subscriptions.push(cx.subscribe(
          &composer,
          |view, composer, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
              view.composer_text = composer.read(cx).value().to_string();
              cx.notify();
            }
          },
        ));
        self.composer = Some(composer);
      }
      _ => {}
    }
    if let Some(composer) = &self.composer {
      if std::mem::take(&mut self.focus_composer) {
        composer.update(cx, |input, cx| input.focus(window, cx));
      }
    }

    let live: HashSet<u64> = self.current_highlights().iter().map(|h| h.id).collect();
    self.highlight_inputs.retain(|id, _| live.contains(id));
    if let Some(id) = self.active_highlight.filter(|id| live.contains(id)) {
      if !self.highlight_inputs.contains_key(&id) {
        let comment = self
          .current_highlights()
          .iter()
          .find(|h| h.id == id)
          .map(|h| h.comment.clone())
          .unwrap_or_default();
        let input = cx.new(|cx| {
          TextareaState::new(window, cx)
            .rows(3)
            .placeholder("Comment on this area")
            .default_value(comment)
        });
        self._subscriptions.push(cx.subscribe(
          &input,
          move |view, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
              let text = input.read(cx).value().to_string();
              view.update_highlight(id, |h| h.comment = text);
              cx.notify();
            }
          },
        ));
        self.highlight_inputs.insert(id, input);
      }
    }
  }

  fn update_highlight(&mut self, id: u64, f: impl FnOnce(&mut Highlight)) {
    let Some(asset) = self.current().map(|a| a.id.clone()) else {
      return;
    };
    if let Some(h) = self
      .highlights
      .get_mut(&asset)
      .and_then(|list| list.iter_mut().find(|h| h.id == id))
    {
      f(h);
    }
  }

  fn remove_highlight(&mut self, id: u64, cx: &mut Context<Self>) {
    if let Some(asset) = self.current().map(|a| a.id.clone()) {
      if let Some(list) = self.highlights.get_mut(&asset) {
        list.retain(|h| h.id != id);
      }
    }
    self.active_highlight = None;
    cx.notify();
  }

  /// A window position on the image, and the image's size.
  fn surface_point(&self, position: gpui::Point<Pixels>) -> Option<((f64, f64), (f64, f64))> {
    let bounds = self.surface.get()?;
    let point = (
      f64::from(f32::from(position.x - bounds.origin.x)),
      f64::from(f32::from(position.y - bounds.origin.y)),
    );
    let size = (
      f64::from(f32::from(bounds.size.width)),
      f64::from(f32::from(bounds.size.height)),
    );
    Some((point, size))
  }

  fn start_highlight(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
    if let Some((point, _)) = self.surface_point(event.position) {
      self.drag = Some(point);
      self.draft = None;
    }
    cx.stop_propagation();
  }

  fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
    let Some(start) = self.drag else {
      return;
    };
    if let Some((point, size)) = self.surface_point(event.position) {
      self.draft = rect_from_drag(start, point, size);
      cx.notify();
    }
  }

  fn on_mouse_up(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
    self.anchor = None;
    let Some(start) = self.drag.take() else {
      return;
    };
    self.draft = None;
    let rect = self
      .surface_point(position)
      .and_then(|(point, size)| rect_from_drag(start, point, size));
    match rect {
      None => self.toggle_zoom(cx),
      Some(rect) => {
        let Some(asset) = self.current().map(|a| a.id.clone()) else {
          return;
        };
        self.next_id += 1;
        let id = self.next_id;
        self.highlights.entry(asset).or_default().push(Highlight {
          id,
          rect,
          comment: String::new(),
        });
        self.active_highlight = Some(id);
        cx.notify();
      }
    }
  }

  /// `selectLine`: a new range, or Shift+click to grow the one open.
  fn select_line(&mut self, line: usize, shift: bool) {
    match self.pending.as_mut() {
      Some(pending) if shift => {
        pending.start = pending.start.min(line);
        pending.end = pending.end.max(line);
      }
      _ => {
        self.pending = Some(Pending {
          start: line,
          end: line,
        });
        // A fresh composer, focused (`autoFocus`).
        self.composer = None;
        self.composer_text.clear();
        self.focus_composer = true;
      }
    }
  }

  fn line_down(&mut self, line: usize, event: &MouseDownEvent, cx: &mut Context<Self>) {
    let shift = event.modifiers.shift;
    let extending = shift && self.pending.is_some();
    self.select_line(line, shift);
    if !extending {
      self.anchor = Some(line);
    }
    cx.stop_propagation();
    cx.notify();
  }

  fn line_over(&mut self, line: usize, event: &MouseMoveEvent, cx: &mut Context<Self>) {
    let Some(anchor) = self.anchor else {
      return;
    };
    if event.pressed_button != Some(MouseButton::Left) {
      self.anchor = None;
      return;
    }
    let next = Pending {
      start: anchor.min(line),
      end: anchor.max(line),
    };
    if self.pending != Some(next) {
      self.pending = Some(next);
      cx.notify();
    }
  }

  fn commit_line_comment(&mut self, cx: &mut Context<Self>) {
    let (Some(pending), Some(asset)) = (self.pending, self.current().cloned()) else {
      return;
    };
    let text = self.composer_text.trim().to_string();
    if text.is_empty() {
      return;
    }
    let lines = self.texts.get(&asset.path).cloned().unwrap_or_default();
    let picked = lines
      .get(pending.start - 1..pending.end.min(lines.len()))
      .unwrap_or_default()
      .to_vec();
    self.next_id += 1;
    self.lines.entry(asset.id).or_default().push(LineComment {
      id: self.next_id,
      start: pending.start,
      end: pending.end,
      lines: picked,
      text,
    });
    self.pending = None;
    cx.notify();
  }

  fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
    match event.keystroke.key.as_str() {
      _ if ui::is_escape(&event.keystroke) => {
        cx.stop_propagation();
        if self.discard {
          self.discard = false;
          cx.notify();
        } else {
          self.request_close(cx);
        }
      }
      // The carousel's own keys, while nothing else in it has focus.
      "left" | "right" if self.focus.is_focused(window) && !self.discard => {
        cx.stop_propagation();
        let delta = if event.keystroke.key == "left" { -1 } else { 1 };
        self.go(delta, window, cx);
      }
      _ => {}
    }
  }
}

/// A click on the lightbox's content (`data-lightbox-content`), which
/// leaves it open.
fn content<E: InteractiveElement>(el: E) -> E {
  el.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// `Button variant="ghost" size="icon-xs" className="h-8 w-8 text-white
/// hover:bg-white/15"`.
fn tool(id: &'static str, icon: &'static str, disabled: bool) -> Stateful<Div> {
  div()
    .id(id)
    .debug_selector(move || id.into())
    .size_8()
    .flex()
    .items_center()
    .justify_center()
    .rounded(radius::MD)
    .when(disabled, |d| d.opacity(0.4))
    .when(!disabled, |d| {
      d.cursor_pointer().hover(|s| s.bg(chrome(0.15)))
    })
    .child(ui::icon(icon, 16., chrome(1.)))
}

impl Lightbox {
  fn controls(&self, cx: &mut Context<Self>) -> Div {
    let image = self.current().is_some_and(SendAsset::is_image);
    let mut pill = div()
      .flex()
      .flex_none()
      .items_center()
      .gap_1()
      .rounded_full()
      .border_1()
      .border_color(chrome(0.15))
      .bg(scrim(0.4))
      .p_1()
      .shadow_lg();
    if image {
      let (out, into) = (self.zoom <= ZOOM_MIN, self.zoom >= ZOOM_MAX);
      let zoom = self.zoom;
      pill = pill
        .child(
          tool("treq-send-zoom-out", theme::ICON_ZOOM_OUT, out).when(!out, |b| {
            b.on_click(cx.listener(move |view, _, _, cx| view.set_zoom(zoom - 1., cx)))
          }),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .debug_selector(|| "treq-send-zoom-level".into())
            .min_w_10()
            .px_1()
            .flex()
            .justify_center()
            .text_color(chrome(0.8))
            .child(format!("{}%", (self.zoom * 100.).round())),
        )
        .child(
          tool("treq-send-zoom-in", theme::ICON_ZOOM_IN, into).when(!into, |b| {
            b.on_click(cx.listener(move |view, _, _, cx| view.set_zoom(zoom + 1., cx)))
          }),
        )
        .child(div().mx_0p5().h_4().w_px().bg(chrome(0.2)));
    }
    let pill = content(
      pill
        .child(
          tool("treq-send-copy", theme::ICON_COPY, false)
            .on_click(cx.listener(|view, _, _, cx| view.copy(cx))),
        )
        .child(
          tool("treq-send-reveal", theme::ICON_FOLDER_OPEN, false)
            .on_click(cx.listener(|view, _, _, _| view.reveal())),
        )
        .child(
          tool("treq-send-close", theme::ICON_X, false)
            .on_click(cx.listener(|view, _, _, cx| view.request_close(cx))),
        ),
    );
    let input = self.general.clone().map(|general| {
      let disabled = !self.can_review || self.sending;
      content(
        ui::text(div(), TextSize::Sm)
          .id("treq-send-review-input")
          .debug_selector(|| "treq-send-review-input".into())
          .min_w(zpx(224.))
          .max_w(zpx(448.))
          .flex_1()
          .h_9()
          .px_3()
          .flex()
          .items_center()
          .rounded(radius::MD)
          .border_1()
          .border_color(chrome(0.15))
          .bg(scrim(0.4))
          .text_color(chrome(1.))
          .when(disabled, |d| d.opacity(0.4))
          .child(Input::new(&general).appearance(false).bordered(false)),
      )
    });
    div()
      .flex()
      .flex_wrap()
      .max_w_full()
      .items_center()
      .justify_center()
      .gap_2()
      .child(pill)
      .children(input)
  }

  fn image_slide(&self, asset: &SendAsset, window: &Window, cx: &mut Context<Self>) -> Div {
    let viewport = window.viewport_size();
    let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
    let shell = vw * 0.9 - 24.;
    let natural = self.sizes.get(&asset.path).copied().flatten();
    let (w, h) = match natural {
      Some(natural) => {
        let (w, h) = fitted_size(natural, (vw, vh), shell);
        (w * self.zoom, h * self.zoom)
      }
      // `fallbackImageHeight`, square until the size is known.
      None => (vh * 0.8 * self.zoom, vh * 0.8 * self.zoom),
    };
    let zoomed = self.zoom > ZOOM_MIN;
    let d = theme::palette(true);
    let surface = self.surface.clone();
    let active = self.active_highlight;
    let box_at = move |rect: Rect| {
      div()
        .absolute()
        .left(zpx(rect.x as f32 * w))
        .top(zpx(rect.y as f32 * h))
        .w(zpx(rect.width as f32 * w))
        .h(zpx(rect.height as f32 * h))
        .border_2()
    };
    let mut layer = div()
      .id("treq-send-highlight-layer")
      .debug_selector(|| "treq-send-highlight-layer".into())
      .relative()
      .flex_none()
      .w(zpx(w))
      .h(zpx(h))
      .child(
        img(PathBuf::from(&asset.path))
          .object_fit(ObjectFit::Contain)
          .size_full(),
      )
      .child(
        div()
          .id("treq-send-highlight-surface")
          .debug_selector(|| "treq-send-highlight-surface".into())
          .absolute()
          .inset_0()
          .cursor_crosshair()
          .child(
            gpui::canvas(move |b, _, _| surface.set(Some(b)), |_, _, _, _| {})
              .absolute()
              .size_full(),
          )
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, e: &MouseDownEvent, _, cx| view.start_highlight(e, cx)),
          ),
      )
      .children(
        self
          .draft
          .map(|rect| box_at(rect).border_color(ui::Tone::Danger.color(&d).opacity(0.7))),
      );
    for highlight in self.current_highlights() {
      let id = highlight.id;
      let panel = (active == Some(id)).then(|| {
        let input = self.highlight_inputs.get(&id).cloned();
        div()
          .id(SharedString::from(format!(
            "treq-send-highlight-comment-{id}"
          )))
          .debug_selector(move || format!("treq-send-highlight-comment-{id}"))
          .absolute()
          .left_full()
          .top_0()
          .ml_2()
          .w_56()
          .rounded(radius::MD)
          .border_1()
          .border_color(chrome(0.2))
          .bg(d.popover)
          .p_2()
          .shadow_xl()
          .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .child(
            ui::text(div(), TextSize::Sm)
              .min_h(zpx(72.))
              .px_3()
              .py_2()
              .rounded(radius::MD)
              .border_1()
              .border_color(chrome(0.15))
              .text_color(d.foreground)
              .children(input.map(|i| Textarea::new(&i).appearance(false).bordered(false))),
          )
          .child(
            div().mt_1().flex().justify_end().child(
              div()
                .id(SharedString::from(format!(
                  "treq-send-highlight-remove-{id}"
                )))
                .debug_selector(|| "treq-send-highlight-remove".into())
                .size_6()
                .flex()
                .items_center()
                .justify_center()
                .rounded(radius::MD)
                .cursor_pointer()
                .hover(|s| s.bg(chrome(0.1)))
                .on_click(cx.listener(move |view, _, _, cx| view.remove_highlight(id, cx)))
                .child(ui::icon(theme::ICON_X, 14., d.foreground)),
            ),
          )
      });
      layer = layer.child(
        box_at(highlight.rect)
          .id(SharedString::from(format!("treq-send-highlight-{id}")))
          .debug_selector(move || format!("treq-send-highlight-{id}"))
          .border_color(ui::Tone::Danger.color(&d))
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _, _, cx| {
              cx.stop_propagation();
              view.active_highlight = Some(id);
              cx.notify();
            }),
          )
          .children(panel),
      );
    }
    let layer = content(layer);
    let scroll = div()
      .id("treq-send-image-scroll")
      .debug_selector(|| "treq-send-image-scroll".into())
      .w_full()
      .max_w_full()
      .overflow_scroll()
      .flex()
      .when(zoomed, |d| {
        d.h(zpx(vh * 0.8))
          .items_start()
          .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      })
      .when(!zoomed, |d| d.max_h(zpx(vh * 0.8)).items_center())
      // An image wider than the box scrolls from its left edge.
      .when(w <= shell, |d| d.justify_center())
      .child(layer);
    div().w_full().flex().justify_center().child(scroll)
  }

  /// The shown text's rows: each line, then under the last line of a
  /// comment, the box being written and the comments made.
  fn sync_text_rows(&mut self, window: &Window) {
    let mut rows = Vec::new();
    if let Some(asset) = self.current().filter(|a| !a.is_image()) {
      let count = self.texts.get(&asset.path).map_or(1, |t| t.len());
      let comments = self.current_lines();
      for n in 1..=count {
        rows.push((self.index, TextRow::Line(n)));
        if self.composer.is_some() && self.pending.is_some_and(|p| p.end == n) {
          rows.push((self.index, TextRow::Composer));
        }
        rows.extend(
          comments
            .iter()
            .filter(|c| c.end == n)
            .map(|c| (self.index, TextRow::Comment(c.id))),
        );
      }
    }
    self.text_rows.sync(window, rows, |a, b| a == b);
  }

  fn text_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
    let Some(&(_, row)) = self.text_rows.get(ix) else {
      return div().into_any_element();
    };
    let palette = theme::palette(true);
    match row {
      TextRow::Line(n) => {
        let line = self
          .current()
          .and_then(|a| self.texts.get(&a.path))
          .map_or("Loading…", |t| t.get(n - 1).map_or("", String::as_str));
        let pending = self.pending.is_some_and(|p| n >= p.start && n <= p.end);
        let commented = self
          .current_lines()
          .iter()
          .any(|c| n >= c.start && n <= c.end);
        div()
          .id(SharedString::from(format!("treq-send-text-line-{n}")))
          .debug_selector(move || format!("treq-send-text-line-{n}"))
          .flex()
          .w_full()
          .items_start()
          .gap_3()
          .px_4()
          .py(zpx(LINE_PADDING_Y))
          .hover(|s| s.bg(chrome(0.05)))
          .when(pending, |d| d.bg(ui::Tone::Danger.tint(&palette)))
          .when(commented, |d| d.bg(ui::Tone::Warning.tint(&palette)))
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, e: &MouseDownEvent, _, cx| view.line_down(n, e, cx)),
          )
          .on_mouse_move(
            cx.listener(move |view, e: &MouseMoveEvent, _, cx| view.line_over(n, e, cx)),
          )
          .child(
            ui::text(div(), TextSize::Code)
              .w_8()
              .flex_none()
              .flex()
              .justify_end()
              .text_color(palette.muted_foreground)
              .child(n.to_string()),
          )
          .child(
            div()
              .min_w_0()
              .flex_1()
              .whitespace_normal()
              .child(if line.is_empty() {
                " ".to_string()
              } else {
                line.to_string()
              }),
          )
          .into_any_element()
      }
      TextRow::Composer => {
        let Some(composer) = &self.composer else {
          return div().into_any_element();
        };
        let blank = self.composer_text.trim().is_empty();
        div()
          .debug_selector(|| "treq-send-line-comment-composer".into())
          .border_t_1()
          .border_color(chrome(0.1))
          .bg(palette.background)
          .px_4()
          .py_3()
          .font_family(theme::SANS_FONT)
          .child(
            ui::text(div(), TextSize::Sm)
              .min_h(zpx(72.))
              .px_3()
              .py_2()
              .rounded(radius::MD)
              .border_1()
              .border_color(palette.input)
              .bg(palette.background)
              .text_color(palette.foreground)
              .child(Textarea::new(composer).appearance(false).bordered(false)),
          )
          .child(
            div()
              .mt_2()
              .flex()
              .justify_end()
              .gap_2()
              .child(
                Button::new("treq-send-line-comment-cancel", &palette)
                  .variant(Variant::Ghost)
                  .size(ui::Size::Xs)
                  .label("Cancel")
                  .on_click(cx.listener(|view, _, _, cx| {
                    view.pending = None;
                    cx.notify();
                  })),
              )
              .child(
                Button::new("treq-send-line-comment-save", &palette)
                  .size(ui::Size::Xs)
                  .disabled(blank)
                  .label("Add comment")
                  .on_click(cx.listener(|view, _, _, cx| view.commit_line_comment(cx))),
              ),
          )
          .into_any_element()
      }
      TextRow::Comment(id) => {
        let Some(comment) = self.current_lines().iter().find(|c| c.id == id) else {
          return div().into_any_element();
        };
        let range = if comment.start == comment.end {
          format!("LINES {}", comment.start)
        } else {
          format!("LINES {}–{}", comment.start, comment.end)
        };
        ui::text(div(), TextSize::Sm)
          .debug_selector(move || format!("treq-send-line-comment-{id}"))
          .border_t_1()
          .border_color(chrome(0.1))
          .bg(palette.background.opacity(0.8))
          .px_4()
          .py_2()
          .font_family(theme::SANS_FONT)
          .text_color(palette.foreground)
          .child(
            ui::text(div(), TextSize::Xs)
              .text_color(palette.muted_foreground)
              .child(range),
          )
          .child(div().whitespace_normal().child(comment.text.clone()))
          .into_any_element()
      }
    }
  }

  fn text_slide(&self, window: &Window, cx: &mut Context<Self>) -> Div {
    let d = theme::palette(true);
    let count = self.text_rows.len();
    let long = count > LONG_TEXT;
    let max_h = f32::from(window.viewport_size().height) * 0.8;
    let panel = div()
      .id("treq-send-text-preview")
      .debug_selector(|| "treq-send-text-preview".into())
      .w_full()
      .max_w(zpx(1152.))
      .max_h(zpx(max_h))
      .when(long, |this| this.h(zpx(max_h)))
      .flex()
      .flex_col()
      .overflow_hidden()
      .rounded(radius::LG)
      .bg(d.background)
      .shadow_2xl()
      .child(
        ui::text(div(), TextSize::Xs)
          .flex_none()
          .border_b_1()
          .border_color(chrome(0.1))
          .px_4()
          .py_1p5()
          .text_color(d.muted_foreground)
          .child("Click a line to comment. Drag or Shift+click to select multiple lines."),
      )
      .child(
        ui::text(div(), TextSize::Code)
          .id("treq-send-text-scroll")
          .min_h_0()
          .flex_shrink(1.)
          .font_family(theme::MONO_FONT)
          .text_color(d.foreground)
          .map(|this| {
            if long {
              this.flex_1().child(
                gpui::list(
                  self.text_rows.list.clone(),
                  cx.processor(|view, ix, _, cx| view.text_row(ix, cx)),
                )
                .size_full(),
              )
            } else {
              this
                .overflow_y_scroll()
                .children((0..count).map(|ix| self.text_row(ix, cx)))
            }
          }),
      );
    div().w_full().flex().justify_center().child(content(panel))
  }

  fn nav(&self, next: bool, cx: &mut Context<Self>) -> Stateful<Div> {
    let disabled = if next {
      self.index + 1 >= self.assets.len()
    } else {
      self.index == 0
    };
    let id = if next {
      "treq-send-next"
    } else {
      "treq-send-previous"
    };
    let icon = if next {
      theme::ICON_ARROW_RIGHT
    } else {
      theme::ICON_ARROW_LEFT
    };
    content(
      div()
        .id(id)
        .debug_selector(move || id.into())
        .absolute()
        .top(gpui::relative(0.5))
        .mt_neg_8()
        .when(next, |d| d.right_3())
        .when(!next, |d| d.left_3())
        .size_16()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(chrome(0.2))
        .bg(scrim(0.4))
        .when(disabled, |d| d.opacity(0.5))
        .when(!disabled, |d| {
          d.cursor_pointer().hover(|s| s.bg(scrim(0.6))).on_click(
            cx.listener(move |view, _, window, cx| view.go(if next { 1 } else { -1 }, window, cx)),
          )
        })
        .child(ui::icon(icon, 32., chrome(1.))),
    )
  }

  fn discard_dialog(&self, p: &Palette, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
    if !self.discard {
      return None;
    }
    let panel = div()
      .id("treq-send-unsaved-dialog")
      .debug_selector(|| "treq-send-unsaved-dialog".into())
      .w_full()
      .max_w(zpx(512.))
      .p_6()
      .rounded(radius::LG)
      .border_1()
      .border_color(p.border.opacity(0.5))
      .bg(p.popover)
      .text_color(p.popover_foreground)
      .shadow_2xl()
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .child(
        div()
          .flex()
          .flex_col()
          .gap_1p5()
          .child(
            ui::text(div(), TextSize::Lg)
              .font_weight(gpui::FontWeight::SEMIBOLD)
              .child("Discard unsent comments?"),
          )
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child(
                "You have review comments that have not been sent to the agent. Close anyway \
                 and lose them?",
              ),
          ),
      )
      .child(
        div()
          .mt_4()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("treq-send-unsaved-keep", p)
              .variant(Variant::Ghost)
              .label("Keep reviewing")
              .on_click(cx.listener(|view, _, _, cx| {
                view.discard = false;
                cx.notify();
              })),
          )
          .child(
            Button::new("treq-send-unsaved-discard", p)
              .label("Discard comments")
              .on_click(cx.listener(|_, _, _, cx| cx.emit(LightboxEvent::Close))),
          ),
      );
    Some(
      div()
        .id("treq-send-unsaved-backdrop")
        .absolute()
        .inset_0()
        .p_4()
        .flex()
        .items_center()
        .justify_center()
        .bg(scrim(0.4))
        .on_mouse_down(
          MouseButton::Left,
          cx.listener(|view, _, _, cx| {
            cx.stop_propagation();
            view.discard = false;
            cx.notify();
          }),
        )
        .child(panel),
    )
  }
}

impl Render for Lightbox {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.ensure_inputs(window, cx);
    if std::mem::take(&mut self.focus_root) {
      window.focus(&self.focus, cx);
    }
    if let Some(asset) = self.current().filter(|a| a.is_image()) {
      if !self.sizes.contains_key(&asset.path) {
        let path = asset.path.clone();
        let size = image_size(Path::new(&path));
        self.sizes.insert(path, size);
      }
    }
    self.sync_text_rows(window);
    let p = theme::palette(true);
    let viewport = window.viewport_size();
    let current = self.current().cloned();
    let header = div()
      .mb_3()
      .flex()
      .flex_col()
      .items_center()
      .gap_1p5()
      .children(current.as_ref().map(|asset| {
        ui::text(div(), TextSize::Sm)
          .id("treq-send-preview-title")
          .debug_selector(|| "treq-send-preview-title".into())
          .max_w_full()
          .truncate()
          .text_color(chrome(0.8))
          .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .child(asset.title.clone())
      }))
      .child(self.controls(cx));
    let slide = current.as_ref().map(|asset| {
      if asset.is_image() {
        self.image_slide(asset, window, cx)
      } else {
        self.text_slide(window, cx)
      }
    });
    let vw = f32::from(viewport.width);
    div()
      .id("treq-send-preview-lightbox")
      .debug_selector(|| "treq-send-preview-lightbox".into())
      .track_focus(&self.focus)
      .relative()
      .w(viewport.width)
      .h(viewport.height)
      .flex()
      .items_center()
      .justify_center()
      .font_family(theme::SANS_FONT)
      .text_color(chrome(1.))
      .occlude()
      .capture_key_down(cx.listener(Self::on_key))
      .on_mouse_move(cx.listener(Self::on_mouse_move))
      .on_mouse_up(
        MouseButton::Left,
        cx.listener(|view, e: &gpui::MouseUpEvent, _, cx| view.on_mouse_up(e.position, cx)),
      )
      .child(
        div()
          .id("treq-send-preview-backdrop")
          .absolute()
          .inset_0()
          .bg(scrim(0.55))
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, _, cx| view.request_close(cx)),
          ),
      )
      .child(
        div()
          .id("treq-send-preview-carousel-shell")
          .debug_selector(|| "treq-send-preview-carousel-shell".into())
          .relative()
          .w(zpx(vw * 0.9))
          .min_w(zpx(vw * 0.75))
          .px_3()
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _, _, cx| view.request_close(cx)),
          )
          .child(header)
          .children(slide),
      )
      .child(self.nav(false, cx))
      .child(self.nav(true, cx))
      .children(self.discard_dialog(&p, cx))
  }
}
