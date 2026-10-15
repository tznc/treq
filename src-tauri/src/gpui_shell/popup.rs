//! Tooltips and menus that pop out of the GPUI sidebar, drawn like the React
//! app's `TooltipContent` and `ContextMenuContent`.
//!
//! GPUI cannot paint over the webview (a native child window), so each popup
//! is a small borderless, unfocusable Tauri window with GPUI attached. One
//! window per [`Slot`] is built on first use, then moved and resized.

use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  rc::Rc,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Wry};
use tauri_plugin_gpui::{
  gpui::{
    self, div, prelude::*, px, svg, App, Bounds, Context, Hsla, Pixels, Point, SharedString,
    WeakEntity, Window,
  },
  GpuiOptions, GpuiWindowExt,
};

use super::{
  defer_main,
  sidebar::PrState,
  theme::{self, Palette},
};

/// Room around a popup's box for its shadow.
const SHADOW_MARGIN: f32 = 8.;
/// Gap between a tooltip and its element: base-ui's `sideOffset` in
/// `ui/tooltip.tsx`.
const TOOLTIP_OFFSET: f32 = 6.;
/// How close a tooltip may come to the window's edge, as base-ui's
/// collision padding.
const EDGE_PADDING: f32 = 5.;
/// `text-sm`: 14px on a 20px line.
const TEXT_SIZE: f32 = 14.;
const LINE_HEIGHT: f32 = 20.;
/// A menu row: `px-2 py-1.5 text-sm`.
const ITEM_HEIGHT: f32 = 32.;
const ITEM_PADDING_X: f32 = 8.;
/// A menu icon: `w-4 h-4 mr-2`.
const ITEM_ICON: f32 = 16.;
const ITEM_ICON_GAP: f32 = 8.;
/// `my-1 h-px`.
const SEPARATOR_HEIGHT: f32 = 9.;
/// The menu box: `p-1 border min-w-[8rem]`.
const MENU_PADDING: f32 = 4.;
const MENU_MIN_WIDTH: f32 = 128.;
/// Sent to the menu's webview when an item is picked.
pub const MENU_SELECT_EVENT: &str = "gpui-menu-select";

/// What a tooltip shows.
#[derive(Debug, Clone, PartialEq)]
pub enum TooltipContent {
  Text(SharedString),
  /// A branch in mono with its icon, and its conflict and PR state below,
  /// like the React sidebar's row tooltips.
  Branch {
    title: SharedString,
    conflicted: bool,
    pr: Option<PrState>,
  },
}

/// Which side of its element a tooltip sits on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
  Right,
  Bottom,
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
}

/// Where a sidebar's popup requests go: popup windows in the app, a list in
/// tests.
pub type PopupSink = Rc<dyn Fn(PopupRequest, &mut App)>;

/// An icon beside a menu item, from the React menus' lucide icons.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuIcon {
  Archive,
  Copy,
  FolderOpen,
  FolderPlus,
  FolderSearch,
  GitBranch,
  Link,
  Pencil,
  Settings,
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
  },
  Separator,
  Submenu {
    text: String,
    #[serde(default)]
    icon: Option<MenuIcon>,
    items: Vec<MenuEntry>,
  },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
  Tooltip,
  Menu,
  Submenu,
}

impl Slot {
  fn label(self) -> &'static str {
    match self {
      Slot::Tooltip => "gpui-popup-tooltip",
      Slot::Menu => "gpui-popup-menu",
      Slot::Submenu => "gpui-popup-submenu",
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
}

/// The root view of a popup window.
struct PopupView {
  app: AppHandle,
  slot: Slot,
  content: Content,
  palette: Palette,
  /// The window the menu belongs to, for sending picks to its webview.
  owner: String,
  /// The popup's box on screen, for placing a submenu beside an item.
  frame: Bounds<Pixels>,
}

/// The width of `text` in `family` at `text-sm`, without kerning.
fn text_width(cx: &App, text: &str, family: &'static str) -> f32 {
  let system = cx.text_system();
  let font = system.resolve_font(&gpui::font(family));
  let size = px(TEXT_SIZE);
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

/// The tooltip box's size: `px-3 py-1.5` and a border around its lines.
fn tooltip_size(cx: &App, content: &TooltipContent) -> (f32, f32) {
  let (width, lines) = match content {
    TooltipContent::Text(text) => (text_width(cx, text, theme::SANS_FONT), 1),
    TooltipContent::Branch {
      title,
      conflicted,
      pr,
    } => {
      // `gap-1.5` after a `w-3` icon.
      let mut width = 12. + 6. + text_width(cx, title, theme::MONO_FONT);
      let mut lines = 1;
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
  };
  let lines = lines as f32;
  (
    (width + 24. + 2.).ceil(),
    LINE_HEIGHT * lines + 4. * (lines - 1.) + 12. + 2.,
  )
}

fn entry_width(cx: &App, entry: &MenuEntry) -> f32 {
  let (text, icon, submenu) = match entry {
    MenuEntry::Separator => return 0.,
    MenuEntry::Item { text, icon, .. } => (text, icon, false),
    MenuEntry::Submenu { text, icon, .. } => (text, icon, true),
  };
  let mut width = ITEM_PADDING_X * 2. + text_width(cx, text, theme::SANS_FONT);
  if icon.is_some() {
    width += ITEM_ICON + ITEM_ICON_GAP;
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
  fn render_tooltip(&self, content: &TooltipContent) -> gpui::AnyElement {
    let palette = &self.palette;
    match content {
      TooltipContent::Text(text) => div().child(text.clone()).into_any_element(),
      TooltipContent::Branch {
        title,
        conflicted,
        pr,
      } => div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
          div()
            .flex()
            .items_center()
            .gap(px(6.))
            .font_family(theme::MONO_FONT)
            .child(
              svg()
                .path(theme::ICON_BRANCH)
                .size(px(12.))
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
          .mx(px(-MENU_PADDING))
          .my_1()
          .h_px()
          .bg(palette.muted)
          .into_any_element(),
        MenuEntry::Item {
          id,
          text,
          icon,
          destructive,
        } => {
          let id = *id;
          let color = if *destructive {
            palette.destructive
          } else {
            palette.popover_foreground
          };
          menu_row(("item", index), text, *icon, color, &palette, false)
            .debug_selector(move || format!("menu-item-{id}"))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
              if *hovered {
                this.set_open_submenu(None, cx);
              }
            }))
            .on_click(cx.listener(move |this, _, _, _| this.pick(id)))
            .into_any_element()
        }
        MenuEntry::Submenu { text, icon, items } => {
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
          .child(div().ml_auto().child("›"))
          .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered && this.content_open_submenu() != Some(index) {
              this.set_open_submenu(Some(index), cx);
              this.open_submenu_window(index, items.clone());
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
  fn open_submenu_window(&self, index: usize, items: Vec<MenuEntry>) {
    let Content::Menu { entries, .. } = &self.content else {
      return;
    };
    // The trigger row, inside the box's border and padding.
    let inset = px(1. + MENU_PADDING);
    let top = self.frame.origin.y + px(entry_top(entries, index));
    let trigger = Bounds::new(
      gpui::point(self.frame.origin.x + inset, top),
      gpui::size(self.frame.size.width - inset * 2., px(ITEM_HEIGHT)),
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

  /// Sends a picked item to the menu's webview and closes the menus.
  fn pick(&self, id: u32) {
    let owner = self.owner.clone();
    defer_main(&self.app, move |app| {
      hide(Slot::Submenu);
      hide(Slot::Menu);
      let target = tauri::EventTarget::webview_window(&owner);
      if let Err(err) = app.emit_to(target, MENU_SELECT_EVENT, MenuSelect { id }) {
        log::warn!("failed to send a GPUI menu pick: {err}");
      }
    });
  }
}

#[derive(Clone, Serialize)]
struct MenuSelect {
  id: u32,
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
    .h(px(ITEM_HEIGHT))
    .px(px(ITEM_PADDING_X))
    .rounded_sm()
    .text_color(color)
    .when(highlighted, |this| this.bg(hover_bg))
    .hover(move |style| style.bg(hover_bg))
    .when_some(icon, |this, icon| {
      this.child(
        svg()
          .path(icon.path())
          .size(px(ITEM_ICON))
          .flex_none()
          .mr(px(ITEM_ICON_GAP))
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
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let palette = self.palette;
    let body = match self.content.clone() {
      Content::Empty => None,
      // `px-3 py-1.5 shadow-md`
      Content::Tooltip(content) => Some(
        div()
          .px_3()
          .py(px(6.))
          .shadow_md()
          .child(self.render_tooltip(&content)),
      ),
      // `p-1 shadow-md`, `shadow-lg` for a submenu
      Content::Menu {
        entries,
        open_submenu,
      } => Some(
        div()
          .p(px(MENU_PADDING))
          .when(self.slot == Slot::Menu, |this| this.shadow_md())
          .when(self.slot == Slot::Submenu, |this| this.shadow_lg())
          .child(self.render_menu(&entries, open_submenu, cx)),
      ),
    };
    let composited = composited();
    let body = body.map(|body| if composited { body } else { body.shadow_none() });
    div()
      .size_full()
      .p(px(shadow_margin()))
      .font_family(theme::SANS_FONT)
      .text_size(px(TEXT_SIZE))
      .line_height(px(LINE_HEIGHT))
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
  /// Popup windows by slot. Only touched on the main thread.
  static WINDOWS: RefCell<HashMap<Slot, PopupWindow>> = RefCell::default();
  /// A menu is open, so tooltips stay hidden, as under a React context menu.
  static MENU_OPEN: Cell<bool> = const { Cell::new(false) };
}

/// The popup window for `slot`, built on first use. Main thread only.
fn popup_window(
  app: &AppHandle,
  slot: Slot,
) -> Option<(tauri::Window<Wry>, WeakEntity<PopupView>)> {
  if let Some(found) = WINDOWS.with(|windows| {
    windows
      .borrow()
      .get(&slot)
      .map(|popup| (popup.window.clone(), popup.view.clone()))
  }) {
    return Some(found);
  }
  let window = tauri::WindowBuilder::new(app, slot.label())
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .visible(false)
    .inner_size(1., 1.)
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
  WINDOWS.with(|windows| {
    windows.borrow_mut().insert(
      slot,
      PopupWindow {
        window: window.clone(),
        view: view.clone(),
      },
    )
  });
  Some((window, view))
}

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
  let Some((window, view)) = popup_window(app, slot) else {
    return;
  };
  let frame: Rc<RefCell<Option<Bounds<Pixels>>>> = Rc::default();
  let placed = frame.clone();
  let owner = owner.to_string();
  let updated = tauri_plugin_gpui::with_app(move |cx| {
    let size = measure(cx, &content);
    let Some(origin) = place(size) else {
      return;
    };
    let bounds = Bounds::new(origin, gpui::size(px(size.0), px(size.1)));
    *placed.borrow_mut() = Some(bounds);
    let _ = view.update(cx, |view, cx| {
      view.content = content;
      view.palette = palette;
      view.owner = owner;
      view.frame = bounds;
      cx.notify();
    });
  });
  let Some(bounds) = frame.borrow_mut().take().filter(|_| updated.is_ok()) else {
    return;
  };
  let margin = px(shadow_margin());
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

/// Hides the popup for `slot`. Main thread only.
fn hide(slot: Slot) {
  if slot == Slot::Menu {
    MENU_OPEN.with(|open| open.set(false));
  }
  let window = WINDOWS.with(|windows| windows.borrow().get(&slot).map(|p| p.window.clone()));
  if let Some(window) = window {
    let _ = window.hide();
  }
}

/// Hides every popup. Main thread only.
pub fn hide_all() {
  hide(Slot::Tooltip);
  hide(Slot::Submenu);
  hide(Slot::Menu);
}

/// Hides the menus, as a click elsewhere does. Main thread only.
pub fn hide_menus() {
  hide(Slot::Submenu);
  hide(Slot::Menu);
}

/// The client area of `window` on screen, in logical pixels.
fn client_bounds(window: &tauri::Window<Wry>) -> Option<Bounds<Pixels>> {
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

#[cfg(test)]
mod tests {
  use super::*;

  fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(gpui::point(px(x), px(y)), gpui::size(px(w), px(h)))
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
        items: vec![MenuEntry::Item {
          id: 2,
          text: "Open in Zed".into(),
          icon: None,
          destructive: false,
        }],
      }
    );
    assert_eq!(entry_top(&entries, 2), 1. + 4. + 32. + 9.);
  }
}
