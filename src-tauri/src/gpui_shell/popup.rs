//! Tooltips and menus that pop out of the GPUI sidebar, and the app's
//! toasts, drawn like the React app's `TooltipContent`, `ContextMenuContent`
//! and `ToastItem`.
//!
//! Each popup is a small borderless, unfocusable Tauri window with GPUI
//! attached, so it can reach past the edge of the shell window. One
//! window per [`Slot`] (per shell window, for toasts) is built on first use,
//! then moved and resized.

use crate::gpui_shell::ui::zpx;
use crate::gpui_shell::ui::{self, radius, TextSize};
use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  rc::Rc,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Wry};
use tauri_plugin_gpui::{
  gpui::{
    self, div, prelude::*, px, svg, App, Bounds, ClipboardItem, Context, Hsla, Pixels, Point,
    SharedString, WeakEntity, Window,
  },
  GpuiOptions, GpuiWindowExt,
};

use super::{
  defer_main,
  native::{NativeEvent, ToastTone},
  sidebar::PrState,
  theme::{self, Palette},
};

/// A dropdown's gap below its button: `sideOffset={4}`.
const DROPDOWN_OFFSET: f32 = 4.;
/// Room around a popup's box for its shadow.
const SHADOW_MARGIN: f32 = 8.;
/// Gap between a tooltip and its element: base-ui's `sideOffset` in
/// `ui/tooltip.tsx`.
const TOOLTIP_OFFSET: f32 = 6.;
/// How close a tooltip may come to the window's edge, as base-ui's
/// collision padding.
const EDGE_PADDING: f32 = 5.;
/// `TextSize::Sm`: 13px on a 20px line.
const TEXT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 20.;
/// A text tooltip's widest box: `max-w-xs`. Longer text wraps.
const TOOLTIP_MAX_WIDTH: f32 = 320.;
/// A branch tooltip's title wraps inside `max-w-xs`, less `px-3` and the border.
const BRANCH_NAME_MAX_WIDTH: f32 = TOOLTIP_MAX_WIDTH - 24. - 2.;
/// A [`TooltipContent::List`] row: `text-xs`.
const LIST_ITEM_SIZE: f32 = 11.;
const LIST_ITEM_LINE: f32 = 16.;
/// A menu row: `px-2 py-1.5 text-sm`.
const ITEM_HEIGHT: f32 = 32.;
const ITEM_PADDING_X: f32 = 8.;
/// A menu icon: `w-4 h-4 mr-2`.
const ITEM_ICON: f32 = 16.;
const ITEM_ICON_GAP: f32 = 8.;
/// `my-1 h-px`.
const SEPARATOR_HEIGHT: f32 = 9.;
/// A radio item's `pl-8`, with its dot in a `left-2 h-4 w-4` box.
const RADIO_INSET: f32 = 32.;
/// `ml-2` before a submenu row's detail.
const DETAIL_GAP: f32 = 8.;
/// The menu box: `p-1 border min-w-[8rem]`.
const MENU_PADDING: f32 = 4.;
const MENU_MIN_WIDTH: f32 = 128.;
/// The toast stack's corner: `fixed bottom-4 left-4`, and its `gap-2`.
const TOAST_INSET: f32 = 16.;
const TOAST_GAP: f32 = 8.;
/// A success toast with nothing but a title is one short line with no close
/// button; it dismisses itself.
pub(crate) fn is_compact(toast: &Toast) -> bool {
  toast.kind == ToastKind::Success && toast.description.is_none() && toast.action.is_none()
}

/// A toast's width: `min-w-[320px] max-w-sm`.
const TOAST_MIN_WIDTH: f32 = 320.;
const TOAST_MAX_WIDTH: f32 = 384.;
/// Everything in a toast's row but its text: `p-4` and a border, the
/// `h-4 w-4` icon and the `p-1` close button with `gap-3` between them.
const TOAST_CHROME: f32 = 2. + 32. + 16. + 12. + 12. + 24.;
/// The same without the close button and its `gap-3`.
const COMPACT_CHROME: f32 = 2. + 32. + 16. + 12.;

/// What a tooltip shows.
#[derive(Debug, Clone, PartialEq)]
pub enum TooltipContent {
  Text(SharedString),
  /// A branch in mono with its icon, and its conflict and PR state below,
  /// like the React sidebar's row tooltips. A titled workspace's `name`
  /// sits above the branch.
  Branch {
    name: Option<SharedString>,
    title: SharedString,
    conflicted: bool,
    pr: Option<PrState>,
  },
  /// A heading over a list of paths: `font-medium mb-1`, then `text-xs
  /// font-mono` rows `space-y-0.5`, as the header's conflict warning.
  List {
    title: SharedString,
    items: Vec<SharedString>,
  },
  /// Text and its shortcut in a `Kbd` chip: `flex items-center gap-1.5`,
  /// as the terminal pane's control tooltips.
  Shortcut {
    text: SharedString,
    keys: SharedString,
  },
}

/// `ui/kbd.tsx`'s `Kbd`: `h-5 min-w-5 px-1 rounded-sm text-xs font-medium`.
const KBD_HEIGHT: f32 = 20.;
const KBD_PADDING_X: f32 = 4.;
/// `gap-1.5` between a [`TooltipContent::Shortcut`]'s text and its chip.
const KBD_GAP: f32 = 6.;

/// A [`TooltipContent::Shortcut`]'s chip width for `keys`.
fn kbd_width(keys_width: f32) -> f32 {
  (keys_width + 2. * KBD_PADDING_X).max(KBD_HEIGHT)
}

/// Which side of its element a tooltip sits on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
  Right,
  Bottom,
  Top,
}

/// A tooltip change from a sidebar. `anchor` is in the window's coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupRequest {
  ShowTooltip {
    anchor: Bounds<Pixels>,
    side: Side,
    content: TooltipContent,
    dark: bool,
  },
  HideTooltip,
  /// A menu of `entries` with its corner at `at`, for a right-click.
  ShowMenu {
    entries: Vec<MenuEntry>,
    at: Point<Pixels>,
    dark: bool,
  },
  /// A menu of `entries` below the button at `anchor`.
  ShowDropdown {
    entries: Vec<MenuEntry>,
    anchor: Bounds<Pixels>,
    dark: bool,
  },
  /// A native `<select>`'s list of `entries` below the control at `anchor`,
  /// lined up with its left edge.
  ShowSelect {
    entries: Vec<MenuEntry>,
    anchor: Bounds<Pixels>,
    dark: bool,
  },
  /// A header popover, drawn in the modal window over the whole shell
  /// window (see `popover`).
  ShowPopover {
    popover: gpui::Entity<super::popover::Popover>,
    dark: bool,
  },
  HidePopover,
}

/// Item ids at and above this run a GPUI screen's own handler instead of
/// going to React.
const NATIVE_ID_BASE: u32 = 1 << 30;

type Pick = Rc<dyn Fn(&mut App)>;

thread_local! {
  /// The handlers of the native menu last built.
  static NATIVE_PICKS: RefCell<HashMap<u32, Pick>> = RefCell::new(HashMap::new());
}

/// A menu entry whose pick runs a handler on the main thread.
pub enum NativeMenuItem {
  Item {
    text: String,
    icon: Option<MenuIcon>,
    destructive: bool,
    /// A `DropdownMenuRadioItem`, picked or not.
    radio: Option<bool>,
    /// A `DropdownMenuCheckboxItem`, checked or not: picking it toggles
    /// the check and keeps the menu open.
    check: Option<bool>,
    on_select: Pick,
  },
  Separator,
  Submenu {
    text: String,
    icon: Option<MenuIcon>,
    /// Muted text after `text`, such as the value a filter is set to.
    detail: Option<String>,
    items: Vec<NativeMenuItem>,
  },
  /// `DropdownMenuLabel`: a heading row nothing picks.
  Label(String),
}

impl NativeMenuItem {
  pub fn item(text: impl Into<String>, on_select: impl Fn(&mut App) + 'static) -> Self {
    NativeMenuItem::Item {
      text: text.into(),
      icon: None,
      destructive: false,
      radio: None,
      check: None,
      on_select: Rc::new(on_select),
    }
  }

  /// A `DropdownMenuSub`: `items` open beside a row reading `text`, then
  /// `detail` muted.
  pub fn submenu(
    text: impl Into<String>,
    detail: Option<String>,
    items: Vec<NativeMenuItem>,
  ) -> Self {
    NativeMenuItem::Submenu {
      text: text.into(),
      icon: None,
      detail,
      items,
    }
  }

  /// Draws the item as a radio item, with its dot when `checked`.
  pub fn radio(mut self, checked: bool) -> Self {
    if let NativeMenuItem::Item { radio, .. } = &mut self {
      *radio = Some(checked);
    }
    self
  }

  /// Draws the item as a checkbox item, with its check when `checked`.
  pub fn check(mut self, checked: bool) -> Self {
    if let NativeMenuItem::Item { check, .. } = &mut self {
      *check = Some(checked);
    }
    self
  }

  pub fn icon(mut self, value: MenuIcon) -> Self {
    match &mut self {
      NativeMenuItem::Item { icon, .. } | NativeMenuItem::Submenu { icon, .. } => {
        *icon = Some(value)
      }
      NativeMenuItem::Separator | NativeMenuItem::Label(_) => {}
    }
    self
  }

  pub fn destructive(mut self) -> Self {
    if let NativeMenuItem::Item { destructive, .. } = &mut self {
      *destructive = true;
    }
    self
  }
}

/// `items` as menu entries, their handlers kept for the pick. Building a
/// menu drops the previous one's handlers.
pub fn native_entries(items: Vec<NativeMenuItem>) -> Vec<MenuEntry> {
  NATIVE_PICKS.with(|picks| picks.borrow_mut().clear());
  let mut next = NATIVE_ID_BASE;
  fn build(items: Vec<NativeMenuItem>, next: &mut u32) -> Vec<MenuEntry> {
    items
      .into_iter()
      .map(|item| match item {
        NativeMenuItem::Item {
          text,
          icon,
          destructive,
          radio,
          check,
          on_select,
        } => {
          let id = *next;
          *next += 1;
          NATIVE_PICKS.with(|picks| picks.borrow_mut().insert(id, on_select));
          MenuEntry::Item {
            id,
            text,
            icon,
            destructive,
            radio,
            check,
          }
        }
        NativeMenuItem::Separator => MenuEntry::Separator,
        NativeMenuItem::Label(text) => MenuEntry::Label { text },
        NativeMenuItem::Submenu {
          text,
          icon,
          detail,
          items,
        } => MenuEntry::Submenu {
          text,
          icon,
          detail,
          items: build(items, next),
        },
      })
      .collect()
  }
  build(items, &mut next)
}

/// The menu a right-click on a GitHub link opens at `at`: "Copy link",
/// which copies `url` and toasts through `notify`.
pub fn copy_link_menu(
  url: String,
  at: Point<Pixels>,
  dark: bool,
  notify: impl Fn(NativeEvent) + 'static,
) -> PopupRequest {
  let item = NativeMenuItem::item("Copy link", move |cx| {
    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
    notify(NativeEvent::Toast {
      title: "Link copied".into(),
      description: String::new(),
      tone: ToastTone::Success,
      action: None,
    });
  })
  .icon(MenuIcon::Link);
  PopupRequest::ShowMenu {
    entries: native_entries(vec![item]),
    at,
    dark,
  }
}

/// Flips the check of checkbox item `id`, in a submenu too.
fn toggle_check(entries: &mut [MenuEntry], id: u32) {
  for entry in entries {
    match entry {
      MenuEntry::Item {
        id: item,
        check: Some(checked),
        ..
      } if *item == id => *checked = !*checked,
      MenuEntry::Submenu { items, .. } => toggle_check(items, id),
      _ => {}
    }
  }
}

/// Picks native menu item `id`, as a click on it would.
#[cfg(test)]
pub fn pick_native(id: u32, cx: &mut App) {
  if let Some(pick) = native_pick(id) {
    pick(cx);
  }
}

fn native_pick(id: u32) -> Option<Pick> {
  (id >= NATIVE_ID_BASE)
    .then(|| NATIVE_PICKS.with(|picks| picks.borrow().get(&id).cloned()))
    .flatten()
}

/// Where a sidebar's popup requests go: popup windows in the app, a list in
/// tests.
pub type PopupSink = Rc<dyn Fn(PopupRequest, &mut App)>;

/// An icon beside a menu item, from the React menus' lucide icons.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuIcon {
  Archive,
  ArrowRightLeft,
  Copy,
  FileDiff,
  Globe,
  MessageSquare,
  FolderOpen,
  FolderPlus,
  FolderSearch,
  GitBranch,
  Link,
  Pencil,
  Settings,
  #[serde(rename = "trash-2")]
  Trash2,
}

impl MenuIcon {
  fn path(self) -> &'static str {
    match self {
      MenuIcon::Archive => theme::ICON_ARCHIVE,
      MenuIcon::Copy => theme::ICON_COPY,
      MenuIcon::FolderOpen => theme::ICON_FOLDER_OPEN,
      MenuIcon::FolderPlus => theme::ICON_FOLDER_PLUS,
      MenuIcon::FolderSearch => theme::ICON_FOLDER_SEARCH,
      MenuIcon::ArrowRightLeft => theme::ICON_ARROW_RIGHT_LEFT,
      MenuIcon::FileDiff => theme::ICON_FILE_DIFF,
      MenuIcon::Globe => theme::ICON_GLOBE,
      MenuIcon::MessageSquare => theme::ICON_MESSAGE,
      MenuIcon::GitBranch => theme::ICON_BRANCH,
      MenuIcon::Link => theme::ICON_LINK,
      MenuIcon::Pencil => theme::ICON_PENCIL,
      MenuIcon::Settings => theme::ICON_SETTINGS,
      MenuIcon::Trash2 => theme::ICON_TRASH,
    }
  }
}

/// One menu entry, as React sends it. Item ids come back in
/// [`MENU_SELECT_EVENT`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MenuEntry {
  Item {
    id: u32,
    text: String,
    #[serde(default)]
    icon: Option<MenuIcon>,
    #[serde(default)]
    destructive: bool,
    /// A radio item (`pl-8`, its dot drawn when `true`).
    #[serde(default)]
    radio: Option<bool>,
    /// A checkbox item (`pl-8`, its check drawn when `true`).
    #[serde(default)]
    check: Option<bool>,
  },
  Separator,
  Submenu {
    text: String,
    #[serde(default)]
    icon: Option<MenuIcon>,
    /// Muted `text-xs` after the text (`ml-2`).
    #[serde(default)]
    detail: Option<String>,
    items: Vec<MenuEntry>,
  },
  /// `DropdownMenuLabel`: `px-2 py-1.5 text-sm font-semibold`.
  Label {
    text: String,
  },
}

/// A toast's kind, which picks its icon.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToastKind {
  Success,
  Error,
  Info,
  Warning,
}

/// One toast, as React's toast store holds it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Toast {
  pub id: String,
  pub title: String,
  #[serde(default)]
  pub description: Option<String>,
  #[serde(rename = "type")]
  pub kind: ToastKind,
  /// The action button's label.
  #[serde(default)]
  pub action: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
  Tooltip,
  Menu,
  Submenu,
  /// A shell window's toasts; each shell window has its own.
  Toasts,
}

impl Slot {
  fn label(self, owner: &str) -> String {
    match self {
      Slot::Tooltip => "gpui-popup-tooltip".into(),
      Slot::Menu => "gpui-popup-menu".into(),
      Slot::Submenu => "gpui-popup-submenu".into(),
      Slot::Toasts => format!("gpui-popup-toasts-{owner}"),
    }
  }
}

#[derive(Clone)]
enum Content {
  Empty,
  Tooltip(TooltipContent),
  Menu {
    entries: Vec<MenuEntry>,
    /// The submenu trigger whose submenu is open.
    open_submenu: Option<usize>,
  },
  Toasts(Vec<Toast>),
}

/// The root view of a popup window.
struct PopupView {
  app: AppHandle,
  slot: Slot,
  content: Content,
  palette: Palette,
  /// The window the popup belongs to.
  owner: String,
  /// The popup's box on screen, for placing a submenu beside an item.
  frame: Bounds<Pixels>,
}

/// The width of `text` in `family` at `text-sm`, without kerning.
fn text_width(cx: &App, text: &str, family: &'static str) -> f32 {
  text_width_at(cx, text, family, TEXT_SIZE)
}

/// The width of `text` in `family` at `size` px, without kerning.
fn text_width_at(cx: &App, text: &str, family: &'static str, size: f32) -> f32 {
  let system = cx.text_system();
  let font = system.resolve_font(&gpui::font(family));
  let size = px(size);
  text
    .chars()
    .map(|ch| {
      system
        .advance(font, size, ch)
        .map(|advance| f32::from(advance.width))
        .unwrap_or(0.)
    })
    .sum()
}

/// The widest line and the line count of `text` at `text-sm`, wrapped at
/// spaces to fit `max` px, as GPUI wraps it.
fn wrapped_text(cx: &App, text: &str, max: f32) -> (f32, usize) {
  wrap_lines(text, max, |s| text_width(cx, s, theme::SANS_FONT))
}

fn wrap_lines(text: &str, max: f32, width: impl Fn(&str) -> f32) -> (f32, usize) {
  let space = width(" ");
  let (mut widest, mut line, mut lines) = (0f32, 0f32, 1);
  for word in text.split_whitespace() {
    let word = width(word);
    if line > 0. && line + space + word > max {
      widest = widest.max(line);
      line = word;
      lines += 1;
    } else {
      line += if line > 0. { space + word } else { word };
    }
  }
  (widest.max(line), lines)
}

/// The tooltip box's size: `px-3 py-1.5` and a border around its lines.
fn tooltip_size(cx: &App, content: &TooltipContent) -> (f32, f32) {
  // The height of wrapped lines past each row's first.
  let mut flush = 0.;
  let (width, lines) = match content {
    TooltipContent::Text(text) => {
      // Wrapped lines sit flush, without the `gap-1` between rows below.
      let (width, lines) = wrapped_text(cx, text, TOOLTIP_MAX_WIDTH - 24. - 2.);
      return (
        (width + 24. + 2.).ceil(),
        LINE_HEIGHT * lines as f32 + 12. + 2.,
      );
    }
    TooltipContent::Shortcut { text, keys } => {
      // The chip's `font-medium` is a little wider than the regular face.
      let keys = text_width_at(cx, keys, theme::SANS_FONT, LIST_ITEM_SIZE) * 1.03;
      (
        text_width(cx, text, theme::SANS_FONT) + KBD_GAP + kbd_width(keys),
        1,
      )
    }
    TooltipContent::Branch {
      name,
      title,
      conflicted,
      pr,
    } => {
      // `gap-1.5` after a `w-3` icon.
      let mut width = 12. + 6. + text_width(cx, title, theme::MONO_FONT);
      let mut lines = 1;
      if let Some(name) = name {
        // A long title wraps; its lines sit flush, without the `gap-1`.
        let (name_width, name_lines) = wrapped_text(cx, name, BRANCH_NAME_MAX_WIDTH);
        width = width.max(name_width);
        lines += 1;
        flush = LINE_HEIGHT * (name_lines - 1) as f32;
      }
      if *conflicted {
        width = width.max(text_width(cx, "Conflicts detected", theme::SANS_FONT));
        lines += 1;
      }
      if let Some(pr) = pr {
        width = width.max(text_width(cx, pr.label(), theme::SANS_FONT));
        lines += 1;
      }
      (width, lines)
    }
    TooltipContent::List { title, items } => {
      // The title is `font-medium`, a little wider than the regular face.
      let width = items
        .iter()
        .map(|item| text_width_at(cx, item, theme::MONO_FONT, LIST_ITEM_SIZE))
        .fold(text_width(cx, title, theme::SANS_FONT) * 1.03, f32::max);
      let items = items.len() as f32;
      let height = LINE_HEIGHT + 4. + items * LIST_ITEM_LINE + (items - 1.).max(0.) * 2.;
      return ((width + 24. + 2.).ceil(), height + 12. + 2.);
    }
  };
  let lines = lines as f32;
  (
    (width + 24. + 2.).ceil(),
    LINE_HEIGHT * lines + 4. * (lines - 1.) + flush + 12. + 2.,
  )
}

fn entry_width(cx: &App, entry: &MenuEntry) -> f32 {
  let (text, icon, submenu) = match entry {
    MenuEntry::Separator => return 0.,
    MenuEntry::Label { text } => {
      return ITEM_PADDING_X * 2. + weighted_text_width(cx, text, gpui::FontWeight::SEMIBOLD)
    }
    MenuEntry::Item { text, icon, .. } => (text, icon, false),
    MenuEntry::Submenu { text, icon, .. } => (text, icon, true),
  };
  let mut width = ITEM_PADDING_X * 2. + text_width(cx, text, theme::SANS_FONT);
  if icon.is_some() {
    width += ITEM_ICON + ITEM_ICON_GAP;
  }
  if let MenuEntry::Item { radio, check, .. } = entry {
    if radio.is_some() || check.is_some() {
      width += RADIO_INSET - ITEM_PADDING_X;
    }
  }
  if let MenuEntry::Submenu {
    detail: Some(detail),
    ..
  } = entry
  {
    width += DETAIL_GAP + text_width_at(cx, detail, theme::SANS_FONT, LIST_ITEM_SIZE);
  }
  if submenu {
    width += text_width(cx, "›", theme::SANS_FONT);
  }
  width
}

fn entry_height(entry: &MenuEntry) -> f32 {
  match entry {
    MenuEntry::Separator => SEPARATOR_HEIGHT,
    _ => ITEM_HEIGHT,
  }
}

/// The menu box's size: its widest row, at least `min-w-[8rem]`.
fn menu_size(cx: &App, entries: &[MenuEntry]) -> (f32, f32) {
  let inner = entries
    .iter()
    .map(|entry| entry_width(cx, entry))
    .fold(0., f32::max);
  let width = (inner + MENU_PADDING * 2. + 2.).max(MENU_MIN_WIDTH).ceil();
  let height = entries.iter().map(entry_height).sum::<f32>() + MENU_PADDING * 2. + 2.;
  (width, height)
}

/// The top of entry `index` inside the menu box.
fn entry_top(entries: &[MenuEntry], index: usize) -> f32 {
  1. + MENU_PADDING + entries[..index].iter().map(entry_height).sum::<f32>()
}

/// The number of lines `text` takes in Inter at `text-sm` within `width`.
fn wrapped_lines(cx: &App, text: &str, width: f32) -> usize {
  let mut wrapper = cx
    .text_system()
    .line_wrapper(gpui::font(theme::SANS_FONT), px(TEXT_SIZE));
  text
    .split('\n')
    .map(|line| {
      let fragments = [gpui::LineFragment::text(line)];
      1 + wrapper
        .wrap_line(&fragments, px(width), gpui::IndentAdjustment::NoIndent)
        .count()
    })
    .sum()
}

/// The width of `text` in Inter at `text-sm` and `weight`.
fn weighted_text_width(cx: &App, text: &str, weight: gpui::FontWeight) -> f32 {
  let system = cx.text_system();
  let font = system.resolve_font(&gpui::Font {
    weight,
    ..gpui::font(theme::SANS_FONT)
  });
  text
    .chars()
    .map(|ch| {
      system
        .advance(font, px(TEXT_SIZE), ch)
        .map(|advance| f32::from(advance.width))
        .unwrap_or(0.)
    })
    .sum()
}

/// A toast's box width: its text on one line, between `min-w-[320px]` and
/// `max-w-sm`. The stack is as wide as its widest toast, and every toast
/// fills it (`w-full`).
fn toast_width(cx: &App, toast: &Toast) -> f32 {
  let mut text = weighted_text_width(cx, &toast.title, gpui::FontWeight::SEMIBOLD);
  if let Some(description) = &toast.description {
    let widest = description
      .split('\n')
      .map(|line| text_width(cx, line, theme::SANS_FONT))
      .fold(0., f32::max);
    text = text.max(widest);
  }
  if let Some(action) = &toast.action {
    text = text.max(weighted_text_width(cx, action, gpui::FontWeight::MEDIUM));
  }
  if is_compact(toast) {
    return (text + COMPACT_CHROME).min(TOAST_MAX_WIDTH).ceil();
  }
  (text + TOAST_CHROME)
    .clamp(TOAST_MIN_WIDTH, TOAST_MAX_WIDTH)
    .ceil()
}

/// A toast's box height at `width`: its text column (a `leading-none`
/// title, the description and action below it with `space-y-1`), or the
/// close button where taller, in `p-4` and a border.
fn toast_height(cx: &App, toast: &Toast, width: f32) -> f32 {
  let mut text = TEXT_SIZE;
  if let Some(description) = &toast.description {
    let lines = wrapped_lines(cx, description, width - TOAST_CHROME);
    text += 4. + LINE_HEIGHT * lines as f32;
  }
  if toast.action.is_some() {
    // `mt-1` on the button.
    text += 4. + 4. + LINE_HEIGHT;
  }
  // The close button is the tallest part of a toast that has one.
  let floor = if is_compact(toast) { 0. } else { 24. };
  text.max(floor) + 32. + 2.
}

/// The toast stack's size: toasts one above another with `gap-2`.
fn toasts_size(cx: &App, toasts: &[Toast]) -> (f32, f32) {
  let width = toasts
    .iter()
    .map(|toast| toast_width(cx, toast))
    .fold(0., f32::max);
  let height = toasts
    .iter()
    .map(|toast| toast_height(cx, toast, width))
    .sum::<f32>()
    + TOAST_GAP * toasts.len().saturating_sub(1) as f32;
  (width, height)
}

/// The toast stack's corner, `bottom-4 left-4` in `client` (the shell
/// window's client area on screen).
fn place_toasts(size: (f32, f32), client: Bounds<Pixels>) -> Point<Pixels> {
  gpui::point(
    client.left() + px(TOAST_INSET),
    client.bottom() - px(TOAST_INSET + size.1),
  )
}

/// A tooltip's box beside `anchor` (screen coordinates), shifted to stay
/// inside `viewport`, as base-ui does inside the webview.
pub fn place_tooltip(
  anchor: Bounds<Pixels>,
  side: Side,
  size: (f32, f32),
  viewport: Bounds<Pixels>,
) -> Point<Pixels> {
  let (width, height) = size;
  let anchor_left = f32::from(anchor.left());
  let anchor_top = f32::from(anchor.top());
  let (x, y) = match side {
    Side::Right => (
      f32::from(anchor.right()) + TOOLTIP_OFFSET,
      anchor_top + (f32::from(anchor.size.height) - height) / 2.,
    ),
    Side::Bottom => (
      anchor_left + (f32::from(anchor.size.width) - width) / 2.,
      f32::from(anchor.bottom()) + TOOLTIP_OFFSET,
    ),
    Side::Top => (
      anchor_left + (f32::from(anchor.size.width) - width) / 2.,
      anchor_top - TOOLTIP_OFFSET - height,
    ),
  };
  let min_x = f32::from(viewport.left()) + EDGE_PADDING;
  let max_x = f32::from(viewport.right()) - EDGE_PADDING - width;
  let min_y = f32::from(viewport.top()) + EDGE_PADDING;
  let max_y = f32::from(viewport.bottom()) - EDGE_PADDING - height;
  gpui::point(px(x.min(max_x).max(min_x)), px(y.min(max_y).max(min_y)))
}

/// A menu's box with its corner at `at`, flipped to the other side of the
/// point where it would leave `screen`.
pub fn place_menu(at: Point<Pixels>, size: (f32, f32), screen: Bounds<Pixels>) -> Point<Pixels> {
  let (width, height) = size;
  let (mut x, mut y) = (f32::from(at.x), f32::from(at.y));
  if x + width > f32::from(screen.right()) {
    x -= width;
  }
  if y + height > f32::from(screen.bottom()) {
    y -= height;
  }
  gpui::point(
    px(x.max(f32::from(screen.left()))),
    px(y.max(f32::from(screen.top()))),
  )
}

/// A submenu's box beside its trigger row (screen coordinates), on the
/// left instead where the right would leave `screen`.
fn place_submenu(
  trigger: Bounds<Pixels>,
  size: (f32, f32),
  screen: Bounds<Pixels>,
) -> Point<Pixels> {
  let (width, height) = size;
  // base-ui's submenu: its box's top-left at the trigger row's top-right.
  let mut x = f32::from(trigger.right());
  if x + width > f32::from(screen.right()) {
    x = f32::from(trigger.left()) - width;
  }
  let mut y = f32::from(trigger.top());
  if y + height > f32::from(screen.bottom()) {
    y = f32::from(screen.bottom()) - height;
  }
  gpui::point(px(x), px(y))
}

impl PopupView {
  fn render_tooltip(&self, content: &TooltipContent, cx: &App) -> gpui::AnyElement {
    let palette = &self.palette;
    match content {
      // Text that `tooltip_size` fits on one line stays on one line, so a
      // shaped width a hair over the measured one doesn't wrap it.
      TooltipContent::Text(text) => div()
        .when(
          wrapped_text(cx, text, TOOLTIP_MAX_WIDTH - 24. - 2.).1 == 1,
          |this| this.whitespace_nowrap(),
        )
        .child(text.clone())
        .into_any_element(),
      TooltipContent::Shortcut { text, keys } => div()
        .flex()
        .items_center()
        .gap(zpx(KBD_GAP))
        .child(text.clone())
        .child(
          ui::text(div(), TextSize::Xs)
            .debug_selector(|| "tooltip-kbd".into())
            .h(zpx(KBD_HEIGHT))
            .min_w(zpx(KBD_HEIGHT))
            .px(zpx(KBD_PADDING_X))
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::MD)
            .bg(palette.muted)
            .text_color(palette.muted_foreground)
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(keys.clone()),
        )
        .into_any_element(),
      TooltipContent::Branch {
        name,
        title,
        conflicted,
        pr,
      } => div()
        .flex()
        .flex_col()
        .gap_1()
        .when_some(name.as_ref(), |this, name| {
          let one_line = wrapped_text(cx, name, BRANCH_NAME_MAX_WIDTH).1 == 1;
          this.child(
            div()
              .max_w(zpx(BRANCH_NAME_MAX_WIDTH))
              .when(one_line, |this| this.whitespace_nowrap())
              .child(name.clone()),
          )
        })
        .child(
          div()
            .flex()
            .items_center()
            .gap_1p5()
            .font_family(theme::MONO_FONT)
            .child(
              svg()
                .path(theme::ICON_BRANCH)
                .size(zpx(12.))
                .flex_none()
                .text_color(palette.popover_foreground),
            )
            .child(title.clone()),
        )
        .when(*conflicted, |this| {
          this.child(
            div()
              .text_color(palette.destructive)
              .child("Conflicts detected"),
          )
        })
        .when_some(*pr, |this, pr| {
          this.child(
            div()
              .text_color(super::sidebar::pr_color(pr, palette))
              .child(pr.label()),
          )
        })
        .into_any_element(),
      TooltipContent::List { title, items } => div()
        .flex()
        .flex_col()
        .child(
          div()
            .mb_1()
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(title.clone()),
        )
        .child(
          ui::text(div(), TextSize::Xs)
            .flex()
            .flex_col()
            .gap_0p5()
            .font_family(theme::MONO_FONT)
            .children(items.iter().cloned()),
        )
        .into_any_element(),
    }
  }

  fn render_menu(
    &self,
    entries: &[MenuEntry],
    open_submenu: Option<usize>,
    cx: &mut Context<Self>,
  ) -> gpui::AnyElement {
    let palette = self.palette;
    let mut list = div().flex().flex_col();
    for (index, entry) in entries.iter().enumerate() {
      let row = match entry {
        MenuEntry::Separator => div()
          // `-mx-1` cancels the box's padding.
          .mx(zpx(-MENU_PADDING))
          .my_1()
          .h_px()
          .bg(palette.muted)
          .into_any_element(),
        MenuEntry::Label { text } => div()
          .flex()
          .flex_none()
          .items_center()
          .h(zpx(ITEM_HEIGHT))
          .px(zpx(ITEM_PADDING_X))
          .font_weight(gpui::FontWeight::SEMIBOLD)
          .whitespace_nowrap()
          .child(SharedString::from(text.clone()))
          .into_any_element(),
        MenuEntry::Item {
          id,
          text,
          icon,
          destructive,
          radio,
          check,
        } => {
          let id = *id;
          let checkable = check.is_some();
          let color = if *destructive {
            palette.destructive
          } else {
            palette.popover_foreground
          };
          menu_row(("item", index), text, *icon, color, &palette, false)
            .debug_selector(move || format!("menu-item-{id}"))
            .when_some(*radio, |row, checked| {
              row.relative().pl(zpx(RADIO_INSET)).child(
                div()
                  .absolute()
                  .left(zpx(ITEM_PADDING_X))
                  .size_4()
                  .flex()
                  .items_center()
                  .justify_center()
                  .when(checked, |this| {
                    this.child(div().size_2().rounded_full().bg(color))
                  }),
              )
            })
            .when_some(*check, |row, checked| {
              row
                .relative()
                .pl(zpx(RADIO_INSET))
                .role(gpui::Role::MenuItemCheckBox)
                .aria_toggled(if checked {
                  gpui::accesskit::Toggled::True
                } else {
                  gpui::accesskit::Toggled::False
                })
                .child(
                  div()
                    .absolute()
                    .left(zpx(ITEM_PADDING_X))
                    .size_4()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |this| {
                      this.child(super::ui::icon(theme::ICON_CHECK, 16., color))
                    }),
                )
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
              if *hovered {
                this.set_open_submenu(None, cx);
              }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
              if checkable {
                if let Content::Menu { entries, .. } = &mut this.content {
                  toggle_check(entries, id);
                }
                cx.notify();
              } else {
                this.close();
              }
              if let Some(pick) = native_pick(id) {
                pick(cx);
              }
            }))
            .into_any_element()
        }
        MenuEntry::Submenu {
          text,
          icon,
          detail,
          items,
        } => {
          let items = items.clone();
          menu_row(
            ("submenu", index),
            text,
            *icon,
            palette.popover_foreground,
            &palette,
            open_submenu == Some(index),
          )
          .debug_selector(move || format!("menu-submenu-{index}"))
          .when_some(detail.clone(), |row, detail| {
            row.child(
              ui::text(div(), TextSize::Xs)
                .ml(zpx(DETAIL_GAP))
                .text_color(palette.muted_foreground)
                .whitespace_nowrap()
                .child(detail),
            )
          })
          .child(div().ml_auto().child("›"))
          .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered && this.content_open_submenu() != Some(index) {
              this.set_open_submenu(Some(index), cx);
              this.open_submenu_window(index, items.clone(), super::ui::zoom_factor(cx));
            }
          }))
          .into_any_element()
        }
      };
      list = list.child(row);
    }
    list.into_any_element()
  }

  fn content_open_submenu(&self) -> Option<usize> {
    match &self.content {
      Content::Menu { open_submenu, .. } => *open_submenu,
      _ => None,
    }
  }

  fn set_open_submenu(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
    if let Content::Menu { open_submenu, .. } = &mut self.content {
      if *open_submenu != index {
        let closing = open_submenu.is_some();
        *open_submenu = index;
        cx.notify();
        if closing && index.is_none() && self.slot == Slot::Menu {
          defer_main(&self.app, |_| hide(Slot::Submenu));
        }
      }
    }
  }

  /// Opens the submenu of row `index` beside it.
  fn open_submenu_window(&self, index: usize, items: Vec<MenuEntry>, zoom: f32) {
    let Content::Menu { entries, .. } = &self.content else {
      return;
    };
    // The trigger row, inside the box's border and padding.
    let inset = px((1. + MENU_PADDING) * zoom);
    let top = self.frame.origin.y + px(entry_top(entries, index) * zoom);
    let trigger = Bounds::new(
      gpui::point(self.frame.origin.x + inset, top),
      gpui::size(self.frame.size.width - inset * 2., px(ITEM_HEIGHT * zoom)),
    );
    let (palette, owner) = (self.palette, self.owner.clone());
    defer_main(&self.app, move |app| {
      show_menu_at(
        app,
        Slot::Submenu,
        &owner,
        items,
        palette,
        move |size, screen| place_submenu(trigger, size, screen),
      );
    });
  }

  /// Closes the menus.
  fn close(&self) {
    defer_main(&self.app, |_| {
      hide(Slot::Submenu);
      hide(Slot::Menu);
    });
  }
}

impl PopupView {
  /// The toast stack: `ToastItem`s in `flex flex-col gap-2`.
  fn render_toasts(&self, toasts: &[Toast], cx: &mut Context<Self>) -> gpui::AnyElement {
    let palette = self.palette;
    let composited = composited();
    let mut stack = div().flex().flex_col().gap(zpx(TOAST_GAP));
    for toast in toasts {
      let (icon, icon_color) = match toast.kind {
        ToastKind::Error => (theme::ICON_CIRCLE_ALERT, palette.destructive),
        ToastKind::Info => (theme::ICON_INFO, palette.primary),
        ToastKind::Success => (theme::ICON_CIRCLE_CHECK, palette.toast_success),
        ToastKind::Warning => (theme::ICON_TRIANGLE_ALERT, palette.toast_warning),
      };
      let id = toast.id.clone();
      let close_id = toast.id.clone();
      let compact = is_compact(toast);
      // `flex items-start gap-3 rounded-lg border bg-popover p-4 shadow-lg`
      let item = div()
        .id(SharedString::from(format!("toast-{}", toast.id)))
        .flex()
        .items_start()
        .gap_3()
        .p_4()
        .when(composited, |this| this.rounded_lg().shadow_lg())
        .border_1()
        .border_color(palette.border)
        .bg(palette.popover)
        .text_color(palette.popover_foreground)
        .child(
          svg()
            .path(icon)
            .size(zpx(16.))
            .when(!compact, |this| this.mt_0p5())
            .flex_none()
            .text_color(icon_color),
        )
        .child(
          div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
              ui::text(div(), TextSize::Sm)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(SharedString::from(toast.title.clone())),
            )
            .when_some(toast.description.clone(), |this, description| {
              this.child(
                div()
                  .text_color(palette.muted_foreground)
                  .child(SharedString::from(description)),
              )
            })
            .when_some(toast.action.clone(), |this, action| {
              // `mt-1 font-medium text-primary underline`
              this.child(
                div()
                  .id("action")
                  .mt_1()
                  .font_weight(gpui::FontWeight::MEDIUM)
                  .text_color(palette.primary)
                  .underline()
                  .hover(|style| style.text_decoration_none())
                  .cursor_pointer()
                  .child(SharedString::from(action))
                  .on_click(cx.listener(move |this, _, _, _| this.click_toast(&id, true))),
              )
            }),
        )
        .when(!compact, |this| {
          this.child(
            // `rounded-md p-1 text-muted-foreground/70 hover:bg-accent
            // hover:text-foreground`
            div()
              .id("close")
              .group("close")
              .flex_none()
              .p_1()
              .rounded_md()
              .hover(|style| style.bg(palette.menu_accent))
              .cursor_pointer()
              .child(
                svg()
                  .path(theme::ICON_X)
                  .size(zpx(16.))
                  .text_color(palette.muted_foreground.opacity(0.7))
                  .group_hover("close", |style| style.text_color(palette.foreground)),
              )
              .on_click(cx.listener(move |this, _, _, _| this.click_toast(&close_id, false))),
          )
        });
      stack = stack.child(item);
    }
    stack.into_any_element()
  }

  /// Runs a toast click: its action or close button.
  fn click_toast(&self, id: &str, action: bool) {
    let (owner, id) = (self.owner.clone(), id.to_string());
    defer_main(&self.app, move |app| {
      super::toasts::click(app, &owner, &id, action)
    });
  }
}

/// A menu row: `rounded-sm px-2 py-1.5 text-sm`, `bg-accent` on hover.
fn menu_row(
  id: impl Into<gpui::ElementId>,
  text: &str,
  icon: Option<MenuIcon>,
  color: Hsla,
  palette: &Palette,
  highlighted: bool,
) -> gpui::Stateful<gpui::Div> {
  let hover_bg = palette.menu_accent;
  div()
    .id(id)
    .flex()
    .flex_none()
    .items_center()
    .h(zpx(ITEM_HEIGHT))
    .px(zpx(ITEM_PADDING_X))
    .rounded_sm()
    .text_color(color)
    .when(highlighted, |this| this.bg(hover_bg))
    .cursor_pointer()
    .hover(move |style| style.bg(hover_bg))
    .when_some(icon, |this, icon| {
      this.child(
        svg()
          .path(icon.path())
          .size(zpx(ITEM_ICON))
          .flex_none()
          .mr(zpx(ITEM_ICON_GAP))
          .text_color(color),
      )
    })
    .child(
      div()
        .whitespace_nowrap()
        .child(SharedString::from(text.to_string())),
    )
}

impl Render for PopupView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    #[cfg(feature = "e2e")]
    super::e2e::begin_frame(window);
    super::ui::apply_zoom(window, cx);
    let palette = self.palette;
    let body = match self.content.clone() {
      Content::Empty => None,
      Content::Toasts(toasts) => {
        return ui::text(div(), TextSize::Sm)
          .size_full()
          .p(zpx(shadow_margin()))
          .font_family(theme::SANS_FONT)
          .child(self.render_toasts(&toasts, cx));
      }
      // `px-3 py-1.5 shadow-md`
      Content::Tooltip(content) => Some(
        div()
          .px_3()
          .py_1p5()
          .shadow_md()
          .child(self.render_tooltip(&content, cx)),
      ),
      // `p-1 shadow-md`, `shadow-lg` for a submenu
      Content::Menu {
        entries,
        open_submenu,
      } => Some(
        div()
          .p(zpx(MENU_PADDING))
          .when(self.slot == Slot::Menu, |this| this.shadow_md())
          .when(self.slot == Slot::Submenu, |this| this.shadow_lg())
          .child(self.render_menu(&entries, open_submenu, cx)),
      ),
    };
    let composited = composited();
    let body = body.map(|body| if composited { body } else { body.shadow_none() });
    ui::text(div(), TextSize::Sm)
      .size_full()
      .p(zpx(shadow_margin()))
      .font_family(theme::SANS_FONT)
      .when_some(body, |this, body| {
        // `rounded-md border bg-popover text-popover-foreground`
        this.child(
          body
            .size_full()
            .when(composited, |this| this.rounded_md())
            .border_1()
            .border_color(palette.border)
            .bg(palette.popover)
            .text_color(palette.popover_foreground),
        )
      })
  }
}

/// Whether a compositor blends the popups' transparent corners and shadow
/// margin. Without one (a bare X11 window manager) they would show black, so
/// popups are drawn square, flat and without the margin.
fn composited() -> bool {
  thread_local! {
    static COMPOSITED: Cell<Option<bool>> = const { Cell::new(None) };
  }
  COMPOSITED.with(|cached| {
    if let Some(composited) = cached.get() {
      return composited;
    }
    #[cfg(target_os = "linux")]
    let composited = { gtk::gdk::Screen::default().is_some_and(|screen| screen.is_composited()) };
    #[cfg(not(target_os = "linux"))]
    let composited = true;
    cached.set(Some(composited));
    composited
  })
}

/// Transparent room around a popup for its shadow, when composited.
fn shadow_margin() -> f32 {
  if composited() {
    SHADOW_MARGIN
  } else {
    0.
  }
}

/// A built popup window and its view.
struct PopupWindow {
  window: tauri::Window<Wry>,
  view: WeakEntity<PopupView>,
}

thread_local! {
  /// Popup windows by label. Only touched on the main thread.
  static WINDOWS: RefCell<HashMap<String, PopupWindow>> = RefCell::default();
  /// A menu is open, so tooltips stay hidden, as under a React context menu.
  static MENU_OPEN: Cell<bool> = const { Cell::new(false) };
}

/// A dropdown's box below `anchor` with their right edges lined up, or their
/// left edges when `start` (screen coordinates), `sideOffset={4}` away,
/// flipped above where it would leave `screen`.
pub fn place_dropdown(
  anchor: Bounds<Pixels>,
  size: (f32, f32),
  screen: Bounds<Pixels>,
  start: bool,
) -> Point<Pixels> {
  let (width, height) = size;
  let x = if start {
    f32::from(anchor.left()).min(f32::from(screen.right()) - width)
  } else {
    f32::from(anchor.right()) - width
  }
  .max(f32::from(screen.left()));
  let mut y = f32::from(anchor.bottom()) + DROPDOWN_OFFSET;
  if y + height > f32::from(screen.bottom()) {
    y = f32::from(anchor.top()) - DROPDOWN_OFFSET - height;
  }
  gpui::point(px(x), px(y))
}

/// The popup window for `slot` of window `owner`, built on first use. Main
/// thread only.
fn popup_window(
  app: &AppHandle,
  slot: Slot,
  owner: &str,
) -> Option<(tauri::Window<Wry>, WeakEntity<PopupView>)> {
  let label = slot.label(owner);
  if let Some(found) = WINDOWS.with(|windows| {
    windows
      .borrow()
      .get(&label)
      .map(|popup| (popup.window.clone(), popup.view.clone()))
  }) {
    return Some(found);
  }
  let builder = tauri::WindowBuilder::new(app, &label)
    // Unnamed, it would be listed as Tauri's default "Tauri App".
    .title(&app.package_info().name)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .visible(false)
    .inner_size(1., 1.);
  // Toasts stay up while their window is in the background, so they ride
  // above it rather than above every app. Tooltips and menus close when it
  // loses focus.
  let builder = if slot == Slot::Toasts {
    let parent = tauri::Manager::get_window(app, owner)?;
    builder
      .parent(&parent)
      .map_err(|err| log::warn!("failed to parent the toast window: {err}"))
      .ok()?
  } else {
    builder.always_on_top(true)
  };
  let window = builder
    .build()
    .map_err(|err| log::warn!("failed to build a GPUI popup window: {err}"))
    .ok()?;
  // On Linux every window gets the app's menu bar.
  let _ = window.remove_menu();
  // Mark it a tooltip or menu so X11 window managers neither place nor
  // decorate it.
  #[cfg(target_os = "linux")]
  if let Ok(gtk_window) = window.gtk_window() {
    use gtk::prelude::GtkWindowExt;
    gtk_window.set_type_hint(match slot {
      Slot::Tooltip => gtk::gdk::WindowTypeHint::Tooltip,
      Slot::Menu | Slot::Submenu => gtk::gdk::WindowTypeHint::PopupMenu,
      Slot::Toasts => gtk::gdk::WindowTypeHint::Notification,
    });
  }
  let view: Rc<RefCell<Option<WeakEntity<PopupView>>>> = Rc::default();
  let built = view.clone();
  let popup_app = app.clone();
  let attached = window.attach_gpui_with(
    GpuiOptions::default().focus(false).transparent(true),
    move |cx| {
      let entity = cx.new(|_| PopupView {
        app: popup_app,
        slot,
        content: Content::Empty,
        palette: theme::palette(false),
        owner: String::new(),
        frame: Bounds::default(),
      });
      *built.borrow_mut() = Some(entity.downgrade());
      entity
    },
  );
  if let Err(err) = attached {
    log::warn!("failed to attach GPUI to a popup window: {err}");
    let _ = window.destroy();
    return None;
  }
  let view = view.borrow_mut().take()?;
  // An X11 compositor (xcompmgr, picom) decides at a window's first map
  // whether it is translucent, before GPUI has drawn into it, and shadows it
  // as opaque: a dark box around the first menu shown. Mapping it once while
  // it is still 1x1 settles that, so the first real show draws like the rest.
  #[cfg(target_os = "linux")]
  {
    let _ = window.show();
    let _ = window.hide();
  }
  WINDOWS.with(|windows| {
    windows.borrow_mut().insert(
      label,
      PopupWindow {
        window: window.clone(),
        view: view.clone(),
      },
    )
  });
  Some((window, view))
}

/// A popup's box on screen, and the zoom it was measured at.
type Placed = (Bounds<Pixels>, f32);

/// Shows `content` in the popup for `slot`, sized by `measure` and placed
/// by `place` (screen coordinates, logical pixels). Main thread only.
fn show(
  app: &AppHandle,
  slot: Slot,
  owner: &str,
  palette: Palette,
  content: Content,
  measure: impl FnOnce(&App, &Content) -> (f32, f32) + 'static,
  place: impl FnOnce((f32, f32)) -> Option<Point<Pixels>> + 'static,
) {
  let Some((window, view)) = popup_window(app, slot, owner) else {
    return;
  };
  let frame: Rc<RefCell<Option<Placed>>> = Rc::default();
  let placed = frame.clone();
  let owner = owner.to_string();
  let updated = tauri_plugin_gpui::with_app(move |cx| {
    // Measured at 100%; the view draws at the app's zoom.
    let zoom = super::ui::zoom_factor(cx);
    let size = measure(cx, &content);
    let size = (size.0 * zoom, size.1 * zoom);
    let Some(origin) = place(size) else {
      return;
    };
    let bounds = Bounds::new(origin, gpui::size(px(size.0), px(size.1)));
    *placed.borrow_mut() = Some((bounds, zoom));
    let _ = view.update(cx, |view, cx| {
      view.content = content;
      view.palette = palette;
      view.owner = owner;
      view.frame = bounds;
      cx.notify();
    });
  });
  let Some((bounds, zoom)) = frame.borrow_mut().take().filter(|_| updated.is_ok()) else {
    return;
  };
  let margin = px(shadow_margin() * zoom);
  let size = LogicalSize::new(
    f32::from(bounds.size.width + margin * 2.),
    f32::from(bounds.size.height + margin * 2.),
  );
  let _ = window.set_size(size);
  // GTK keeps a hidden, fixed-size window at its first size otherwise.
  #[cfg(target_os = "linux")]
  if let Ok(gtk_window) = window.gtk_window() {
    use gtk::prelude::{GtkWindowExt, WidgetExt};
    let (width, height) = (size.width.ceil() as i32, size.height.ceil() as i32);
    gtk_window.set_size_request(width, height);
    gtk_window.resize(width, height);
  }
  let _ = window.set_position(LogicalPosition::new(
    f32::from(bounds.origin.x - margin),
    f32::from(bounds.origin.y - margin),
  ));
  let _ = window.show();
}

/// Hides the popup for `slot` (a tooltip or menu). Main thread only.
fn hide(slot: Slot) {
  if slot == Slot::Menu {
    MENU_OPEN.with(|open| open.set(false));
  }
  hide_label(&slot.label(""));
}

fn hide_label(label: &str) {
  let window = WINDOWS.with(|windows| windows.borrow().get(label).map(|p| p.window.clone()));
  if let Some(window) = window {
    let _ = window.hide();
  }
}

/// Hides every tooltip and menu. Main thread only.
pub fn hide_all() {
  hide(Slot::Tooltip);
  hide(Slot::Submenu);
  hide(Slot::Menu);
}

/// Hides the tooltip. Main thread only.
pub fn hide_tooltip() {
  hide(Slot::Tooltip);
}

/// Hides the menus, as a click elsewhere does. Main thread only.
pub fn hide_menus() {
  hide(Slot::Submenu);
  hide(Slot::Menu);
}

/// The client area of `window` on screen, in logical pixels.
pub(super) fn client_bounds(window: &tauri::Window<Wry>) -> Option<Bounds<Pixels>> {
  let scale = window.scale_factor().ok()? as f32;
  let position = window.inner_position().ok()?;
  let size = window.inner_size().ok()?;
  Some(Bounds::new(
    gpui::point(px(position.x as f32 / scale), px(position.y as f32 / scale)),
    gpui::size(
      px(size.width as f32 / scale),
      px(size.height as f32 / scale),
    ),
  ))
}

/// The work area of the monitor `window` is on, in logical pixels.
fn screen_bounds(window: &tauri::Window<Wry>) -> Option<Bounds<Pixels>> {
  let monitor = window.current_monitor().ok().flatten()?;
  let scale = monitor.scale_factor() as f32;
  let area = monitor.work_area();
  Some(Bounds::new(
    gpui::point(
      px(area.position.x as f32 / scale),
      px(area.position.y as f32 / scale),
    ),
    gpui::size(
      px(area.size.width as f32 / scale),
      px(area.size.height as f32 / scale),
    ),
  ))
}

/// Carries out a sidebar's popup request for window `owner`. Main thread
/// only, outside a GPUI update.
pub fn handle(app: &AppHandle, owner: &str, request: PopupRequest) {
  match request {
    PopupRequest::HideTooltip => hide(Slot::Tooltip),
    PopupRequest::ShowPopover { popover, dark } => {
      hide(Slot::Tooltip);
      super::modal::show_popover(app, owner, popover, dark);
    }
    PopupRequest::HidePopover => super::modal::hide_popover(app, owner),
    PopupRequest::ShowMenu { entries, at, dark } => show_menu(app, owner, entries, at, dark),
    PopupRequest::ShowDropdown {
      entries,
      anchor,
      dark,
    } => show_dropdown(app, owner, entries, anchor, false, dark),
    PopupRequest::ShowSelect {
      entries,
      anchor,
      dark,
    } => show_dropdown(app, owner, entries, anchor, true, dark),
    PopupRequest::ShowTooltip {
      anchor,
      side,
      content,
      dark,
    } => {
      if MENU_OPEN.with(Cell::get) {
        return;
      }
      let Some(viewport) = tauri::Manager::get_window(app, owner)
        .as_ref()
        .and_then(client_bounds)
      else {
        return;
      };
      let anchor = Bounds::new(anchor.origin + viewport.origin, anchor.size);
      show(
        app,
        Slot::Tooltip,
        owner,
        theme::palette(dark),
        Content::Tooltip(content),
        |cx, content| match content {
          Content::Tooltip(content) => tooltip_size(cx, content),
          _ => (0., 0.),
        },
        move |size| Some(place_tooltip(anchor, side, size, viewport)),
      );
    }
  }
}

/// Shows a menu of `entries` in `slot`, placed by `place` given its size and
/// the screen's work area. Main thread only.
fn show_menu_at(
  app: &AppHandle,
  slot: Slot,
  owner: &str,
  entries: Vec<MenuEntry>,
  palette: Palette,
  place: impl FnOnce((f32, f32), Bounds<Pixels>) -> Point<Pixels> + 'static,
) {
  let Some(screen) = tauri::Manager::get_window(app, owner)
    .as_ref()
    .and_then(screen_bounds)
  else {
    return;
  };
  show(
    app,
    slot,
    owner,
    palette,
    Content::Menu {
      entries,
      open_submenu: None,
    },
    |cx, content| match content {
      Content::Menu { entries, .. } => menu_size(cx, entries),
      _ => (0., 0.),
    },
    move |size| Some(place(size, screen)),
  );
}

/// Opens a menu of `entries` for window `owner` with its corner at `at`
/// (the window's coordinates). Main thread only.
pub fn show_menu(
  app: &AppHandle,
  owner: &str,
  entries: Vec<MenuEntry>,
  at: Point<Pixels>,
  dark: bool,
) {
  let Some(client) = tauri::Manager::get_window(app, owner)
    .as_ref()
    .and_then(client_bounds)
  else {
    return;
  };
  MENU_OPEN.with(|open| open.set(true));
  hide(Slot::Tooltip);
  hide(Slot::Submenu);
  // base-ui's context menu: 2px right of and 5px above the pointer.
  let at = at + client.origin + gpui::point(px(2.), px(-5.));
  show_menu_at(
    app,
    Slot::Menu,
    owner,
    entries,
    theme::palette(dark),
    move |size, screen| place_menu(at, size, screen),
  );
}

/// Opens a menu of `entries` for window `owner` below `anchor`, a button in
/// the window's coordinates. Main thread only.
pub fn show_dropdown(
  app: &AppHandle,
  owner: &str,
  entries: Vec<MenuEntry>,
  anchor: Bounds<Pixels>,
  start: bool,
  dark: bool,
) {
  let Some(client) = tauri::Manager::get_window(app, owner)
    .as_ref()
    .and_then(client_bounds)
  else {
    return;
  };
  MENU_OPEN.with(|open| open.set(true));
  hide(Slot::Tooltip);
  hide(Slot::Submenu);
  let anchor = Bounds::new(anchor.origin + client.origin, anchor.size);
  show_menu_at(
    app,
    Slot::Menu,
    owner,
    entries,
    theme::palette(dark),
    move |size, screen| place_dropdown(anchor, size, screen, start),
  );
}

/// Shows window `owner`'s `toasts` in its bottom-left corner, or hides them
/// when there are none. Main thread only.
pub fn show_toasts(app: &AppHandle, owner: &str, toasts: Vec<Toast>, dark: bool) {
  if toasts.is_empty() {
    hide_label(&Slot::Toasts.label(owner));
    return;
  }
  let Some(client) = tauri::Manager::get_window(app, owner)
    .as_ref()
    .and_then(client_bounds)
  else {
    return;
  };
  show(
    app,
    Slot::Toasts,
    owner,
    theme::palette(dark),
    Content::Toasts(toasts),
    |cx, content| match content {
      Content::Toasts(toasts) => toasts_size(cx, toasts),
      _ => (0., 0.),
    },
    move |size| Some(place_toasts(size, client)),
  );
}

/// Moves window `owner`'s toasts back to its corner after it moved or
/// resized. Main thread only.
pub fn place_owner_toasts(app: &AppHandle, owner: &str) {
  let label = Slot::Toasts.label(owner);
  let Some((window, view)) = WINDOWS.with(|windows| {
    windows
      .borrow()
      .get(&label)
      .map(|popup| (popup.window.clone(), popup.view.clone()))
  }) else {
    return;
  };
  if !window.is_visible().unwrap_or(false) {
    return;
  }
  let Some(client) = tauri::Manager::get_window(app, owner)
    .as_ref()
    .and_then(client_bounds)
  else {
    return;
  };
  let frame = tauri_plugin_gpui::with_app(move |cx| {
    let zoom = super::ui::zoom_factor(cx);
    view.read_with(cx, |view, _| (view.frame, zoom)).ok()
  });
  if let Ok(Some((frame, zoom))) = frame {
    let size = (f32::from(frame.size.width), f32::from(frame.size.height));
    let origin = place_toasts(size, client);
    let margin = px(shadow_margin() * zoom);
    let _ = window.set_position(LogicalPosition::new(
      f32::from(origin.x - margin),
      f32::from(origin.y - margin),
    ));
  }
}

/// Destroys window `owner`'s toast window once the window is gone. Main
/// thread only.
pub fn forget_owner(owner: &str) {
  let label = Slot::Toasts.label(owner);
  if let Some(popup) = WINDOWS.with(|windows| windows.borrow_mut().remove(&label)) {
    let _ = popup.window.destroy();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(gpui::point(px(x), px(y)), gpui::size(px(w), px(h)))
  }

  #[test]
  fn places_a_dropdown_under_its_button_by_either_edge() {
    let anchor = bounds(300., 100., 80., 28.);
    let screen = bounds(0., 0., 1400., 900.);
    let size = (160., 120.);
    assert_eq!(
      place_dropdown(anchor, size, screen, false),
      gpui::point(px(220.), px(132.))
    );
    assert_eq!(
      place_dropdown(anchor, size, screen, true),
      gpui::point(px(300.), px(132.))
    );
  }

  #[test]
  fn places_a_right_tooltip_beside_its_row_centered() {
    let at = place_tooltip(
      bounds(100., 200., 260., 32.),
      Side::Right,
      (120., 34.),
      bounds(0., 0., 1400., 900.),
    );
    assert_eq!(at, gpui::point(px(366.), px(199.)));
  }

  #[test]
  fn centers_a_bottom_tooltip_under_its_button() {
    let at = place_tooltip(
      bounds(200., 10., 20., 20.),
      Side::Bottom,
      (100., 34.),
      bounds(0., 0., 1400., 900.),
    );
    assert_eq!(at, gpui::point(px(160.), px(36.)));
  }

  #[test]
  fn shifts_a_tooltip_inside_the_window() {
    let at = place_tooltip(
      bounds(10., 10., 20., 20.),
      Side::Bottom,
      (100., 34.),
      bounds(0., 0., 1400., 900.),
    );
    assert_eq!(at.x, px(5.));
  }

  #[test]
  fn wraps_long_tooltip_text_at_spaces() {
    let width = |s: &str| s.chars().count() as f32;
    assert_eq!(wrap_lines("short tip", 20., width), (9., 1));
    assert_eq!(wrap_lines("aaaa bbbb cccc dddd", 10., width), (9., 2));
    // A word longer than the line keeps a line of its own.
    assert_eq!(wrap_lines("a bbbbbbbbbbbb c", 10., width), (12., 3));
  }

  #[test]
  fn flips_a_menu_that_would_leave_the_screen() {
    let screen = bounds(0., 0., 1000., 800.);
    assert_eq!(
      place_menu(gpui::point(px(100.), px(100.)), (200., 300.), screen),
      gpui::point(px(100.), px(100.))
    );
    assert_eq!(
      place_menu(gpui::point(px(900.), px(700.)), (200., 300.), screen),
      gpui::point(px(700.), px(400.))
    );
  }

  #[test]
  fn opens_a_submenu_on_the_left_near_the_screen_edge() {
    let screen = bounds(0., 0., 1000., 800.);
    let trigger = bounds(700., 100., 200., 32.);
    assert_eq!(
      place_submenu(trigger, (150., 100.), screen),
      gpui::point(px(550.), px(100.))
    );
    let trigger = bounds(100., 100., 200., 32.);
    assert_eq!(
      place_submenu(trigger, (150., 100.), screen),
      gpui::point(px(300.), px(100.))
    );
  }

  #[test]
  fn places_toasts_in_the_bottom_left_corner() {
    let client = bounds(100., 50., 1200., 800.);
    assert_eq!(
      place_toasts((320., 90.), client),
      gpui::point(px(116.), px(744.))
    );
  }

  #[test]
  fn reads_toasts_from_react() {
    let toasts: Vec<Toast> = serde_json::from_str(
      r#"[{"id":"a","title":"Saved","type":"success"},
          {"id":"b","title":"Push failed","description":"Rejected","type":"error","action":"Retry"}]"#,
    )
    .unwrap();
    assert_eq!(toasts[0].kind, ToastKind::Success);
    assert_eq!(toasts[0].description, None);
    assert_eq!(toasts[1].action.as_deref(), Some("Retry"));
  }

  #[gpui::test]
  fn sizes_a_titled_branch_tooltip(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
      theme::init(cx);
      let branch = |name: Option<&str>| TooltipContent::Branch {
        name: name.map(|n| n.to_string().into()),
        title: "zn/blissful-shannon".into(),
        conflicted: false,
        pr: None,
      };
      let (_, bare) = tooltip_size(cx, &branch(None));
      // The title sits above the branch, `gap-1` apart.
      let (_, titled) = tooltip_size(cx, &branch(Some("Fix spool")));
      assert_eq!(titled, bare + LINE_HEIGHT + 4.);
      // A long title wraps at `max-w-xs`, its lines flush.
      let (width, long) = tooltip_size(cx, &branch(Some(&"word ".repeat(40))));
      assert!(width <= TOOLTIP_MAX_WIDTH);
      assert!(long >= titled + 2. * LINE_HEIGHT);
    });
  }

  #[gpui::test]
  fn sizes_toasts_like_the_react_toast(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
      theme::init(cx);
      let short = Toast {
        id: "a".into(),
        title: "Saved".into(),
        description: None,
        kind: ToastKind::Success,
        action: None,
      };
      // One line with no close button hugs its text.
      let (width, height) = toasts_size(cx, std::slice::from_ref(&short));
      assert!(width < 320. && height < 58.);
      // `min-w-[320px]`, and the close button's height in `p-4`.
      let error = Toast {
        kind: ToastKind::Error,
        ..short.clone()
      };
      assert_eq!(toasts_size(cx, std::slice::from_ref(&error)), (320., 58.));
      let long = Toast {
        description: Some("word ".repeat(40)),
        action: Some("Retry".into()),
        ..short.clone()
      };
      let (width, height) = toasts_size(cx, &[short, long]);
      // `max-w-sm`; the description wraps onto several lines.
      assert_eq!(width, 384.);
      assert!(height > 58. + 8. + 34. + 14. + 4. + 2. * LINE_HEIGHT);
    });
  }

  #[test]
  fn reads_every_menu_icon() {
    // Every icon a menu item can name.
    let names = [
      "archive",
      "copy",
      "folder-open",
      "folder-plus",
      "folder-search",
      "git-branch",
      "link",
      "pencil",
      "settings",
      "trash-2",
    ];
    for name in names {
      let parsed: Result<MenuIcon, _> = serde_json::from_value(serde_json::json!(name));
      assert!(parsed.is_ok(), "{name} does not parse");
    }
  }

  #[test]
  fn reads_menu_entries_from_react() {
    let entries: Vec<MenuEntry> = serde_json::from_str(
      r#"[
        {"kind":"item","id":1,"text":"Copy branch name","icon":"git-branch"},
        {"kind":"separator"},
        {"kind":"submenu","text":"Open in...","icon":"folder-open","items":[
          {"kind":"item","id":2,"text":"Open in Zed"}
        ]},
        {"kind":"item","id":3,"text":"Archive Workspace","icon":"archive","destructive":true}
      ]"#,
    )
    .unwrap();
    assert_eq!(
      entries[2],
      MenuEntry::Submenu {
        text: "Open in...".into(),
        icon: Some(MenuIcon::FolderOpen),
        detail: None,
        items: vec![MenuEntry::Item {
          id: 2,
          text: "Open in Zed".into(),
          icon: None,
          destructive: false,
          radio: None,
          check: None,
        }],
      }
    );
    assert_eq!(entry_top(&entries, 2), 1. + 4. + 32. + 9.);
  }

  #[test]
  fn builds_labels_radio_items_and_submenu_details() {
    let entries = native_entries(vec![
      NativeMenuItem::Label("Filter by".into()),
      NativeMenuItem::Separator,
      NativeMenuItem::submenu(
        "Assignee",
        Some("Ada".into()),
        vec![
          NativeMenuItem::item("Any", |_| {}).radio(false),
          NativeMenuItem::item("Ada", |_| {}).radio(true),
        ],
      ),
    ]);
    assert_eq!(
      entries[0],
      MenuEntry::Label {
        text: "Filter by".into()
      }
    );
    let MenuEntry::Submenu { detail, items, .. } = &entries[2] else {
      panic!("a submenu");
    };
    assert_eq!(detail.as_deref(), Some("Ada"));
    let radios: Vec<_> = items
      .iter()
      .map(|item| match item {
        MenuEntry::Item { radio, .. } => *radio,
        _ => None,
      })
      .collect();
    assert_eq!(radios, [Some(false), Some(true)]);
    assert_eq!(entry_top(&entries, 2), 1. + 4. + 32. + 9.);
  }

  #[test]
  fn toggles_a_checkbox_item_in_a_submenu() {
    let mut entries = native_entries(vec![NativeMenuItem::submenu(
      "View",
      None,
      vec![
        NativeMenuItem::item("Generated files", |_| {}).check(true),
        NativeMenuItem::item("Committed", |_| {}),
      ],
    )]);
    let MenuEntry::Submenu { items, .. } = &entries[0] else {
      panic!("{entries:?}");
    };
    let id = match &items[0] {
      MenuEntry::Item { id, .. } => *id,
      other => panic!("{other:?}"),
    };
    toggle_check(&mut entries, id);
    let MenuEntry::Submenu { items, .. } = &entries[0] else {
      panic!("{entries:?}");
    };
    let checks: Vec<_> = items
      .iter()
      .map(|e| match e {
        MenuEntry::Item { check, .. } => *check,
        _ => None,
      })
      .collect();
    assert_eq!(checks, [Some(false), None]);
  }
}
