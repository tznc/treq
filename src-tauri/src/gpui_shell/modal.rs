//! The window an [`Overlay`] dialog opens in. GPUI cannot paint over the
//! webview (a native child window), so a dialog that covers the whole shell
//! window, backdrop included, gets a borderless transparent window of its
//! own, placed over the shell window's client area and kept above it. Unlike
//! the popups (`popup`), it takes focus, for its search field.
//!
//! The workspace header's popovers (`popover`) open here too, over a
//! transparent backdrop, so one can drop over the framed webview and a click
//! outside it closes it.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use tauri::{AppHandle, LogicalPosition, LogicalSize, Wry};
use tauri_plugin_gpui::{
  gpui::{div, prelude::*, Context, Entity, WeakEntity, Window},
  GpuiOptions, GpuiWindowExt,
};

use super::{
  overlay::{Overlay, OverlayHost, OverlaySnapshot},
  popover::Popover,
  popup,
  screen::ActionSink,
  theme,
};

/// The modal window's root: the open dialog or popover, if any.
struct ModalRoot {
  overlay: Option<Entity<Overlay>>,
  popover: Option<Entity<Popover>>,
}

impl Render for ModalRoot {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    div()
      .size_full()
      .children(self.overlay.clone())
      .children(self.popover.clone())
  }
}

struct ModalWindow {
  window: tauri::Window<Wry>,
  root: WeakEntity<ModalRoot>,
}

thread_local! {
  /// Modal windows by their shell window's label. Main thread only.
  static MODALS: RefCell<HashMap<String, ModalWindow>> = RefCell::default();
}

fn label(owner: &str) -> String {
  format!("gpui-popup-modal-{owner}")
}

/// Window `owner`'s modal window, built on first use. Main thread only.
fn modal_window(
  app: &AppHandle,
  owner: &str,
) -> Option<(tauri::Window<Wry>, WeakEntity<ModalRoot>)> {
  if let Some(found) = MODALS.with(|modals| {
    modals
      .borrow()
      .get(owner)
      .map(|m| (m.window.clone(), m.root.clone()))
  }) {
    return Some(found);
  }
  let parent = tauri::Manager::get_window(app, owner)?;
  let window = tauri::WindowBuilder::new(app, label(owner))
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .skip_taskbar(true)
    .visible(false)
    .inner_size(1., 1.)
    .parent(&parent)
    .map_err(|err| log::warn!("failed to parent the GPUI modal window: {err}"))
    .ok()?
    .build()
    .map_err(|err| log::warn!("failed to build the GPUI modal window: {err}"))
    .ok()?;
  let _ = window.remove_menu();
  #[cfg(target_os = "linux")]
  if let Ok(gtk_window) = window.gtk_window() {
    use gtk::prelude::GtkWindowExt;
    gtk_window.set_type_hint(gtk::gdk::WindowTypeHint::Dialog);
  }
  let built: Rc<RefCell<Option<WeakEntity<ModalRoot>>>> = Rc::default();
  let slot = built.clone();
  let attached = window.attach_gpui_with(
    GpuiOptions::default().focus(true).transparent(true),
    move |cx| {
      let root = cx.new(|_| ModalRoot {
        overlay: None,
        popover: None,
      });
      *slot.borrow_mut() = Some(root.downgrade());
      root
    },
  );
  if let Err(err) = attached {
    log::warn!("failed to attach GPUI to the modal window: {err}");
    let _ = window.destroy();
    return None;
  }
  let root = built.borrow_mut().take()?;
  // As for the popups: settle an X11 compositor's translucency while 1x1.
  #[cfg(target_os = "linux")]
  {
    let _ = window.show();
    let _ = window.hide();
  }
  MODALS.with(|modals| {
    modals.borrow_mut().insert(
      owner.to_string(),
      ModalWindow {
        window: window.clone(),
        root: root.clone(),
      },
    )
  });
  Some((window, root))
}

/// Covers window `owner`'s client area with its modal window.
fn cover(app: &AppHandle, owner: &str, window: &tauri::Window<Wry>) {
  let Some(client) = tauri::Manager::get_window(app, owner)
    .as_ref()
    .and_then(popup::client_bounds)
  else {
    return;
  };
  let size = LogicalSize::new(f32::from(client.size.width), f32::from(client.size.height));
  let _ = window.set_size(size);
  #[cfg(target_os = "linux")]
  if let Ok(gtk_window) = window.gtk_window() {
    use gtk::prelude::{GtkWindowExt, WidgetExt};
    let (width, height) = (size.width.ceil() as i32, size.height.ceil() as i32);
    gtk_window.set_size_request(width, height);
    gtk_window.resize(width, height);
  }
  let _ = window.set_position(LogicalPosition::new(
    f32::from(client.origin.x),
    f32::from(client.origin.y),
  ));
}

/// Shows `snapshot` over window `owner`, or closes its dialog for `None`.
/// Main thread only, outside a GPUI update.
pub fn update(
  app: &AppHandle,
  owner: &str,
  snapshot: Option<OverlaySnapshot>,
  host: Arc<dyn OverlayHost>,
  sink: ActionSink,
) {
  let Some(snapshot) = snapshot else {
    // React pushes `None` whenever no dialog is open, which must not close
    // a popover.
    if !popover_open(owner) {
      hide(app, owner);
    }
    return;
  };
  let Some((window, root)) = modal_window(app, owner) else {
    return;
  };
  let opened = tauri_plugin_gpui::with_app(move |cx| {
    // The search field's colors.
    theme::set_kit_dark(snapshot.dark, cx);
    root
      .update(cx, |root, cx| {
        let reuse = root
          .overlay
          .as_ref()
          .filter(|o| o.read(cx).view().same_dialog(&snapshot.view))
          .cloned();
        let opened = root.overlay.is_none();
        root.popover = None;
        match reuse {
          Some(overlay) => overlay.update(cx, |o, cx| o.set_snapshot(snapshot, cx)),
          None => {
            root.overlay = Some(cx.new(|cx| Overlay::new(snapshot, host, sink, cx)));
          }
        }
        cx.notify();
        opened
      })
      .unwrap_or(false)
  });
  if opened.unwrap_or(false) || !window.is_visible().unwrap_or(false) {
    popup::hide_all();
    cover(app, owner, &window);
    let _ = window.show();
    cover(app, owner, &window);
    let _ = window.set_focus();
  }
}

/// Shows `popover` over window `owner`, in place of what it showed.
/// Main thread only, outside a GPUI update.
pub fn show_popover(app: &AppHandle, owner: &str, popover: Entity<Popover>, dark: bool) {
  let Some((window, root)) = modal_window(app, owner) else {
    return;
  };
  let _ = tauri_plugin_gpui::with_app(move |cx| {
    // The search field's colors.
    theme::set_kit_dark(dark, cx);
    let _ = root.update(cx, |root, cx| {
      root.overlay = None;
      root.popover = Some(popover);
      cx.notify();
    });
  });
  popup::hide_all();
  cover(app, owner, &window);
  let _ = window.show();
  cover(app, owner, &window);
  let _ = window.set_focus();
}

/// Closes window `owner`'s popover, if it shows one.
pub fn hide_popover(app: &AppHandle, owner: &str) {
  if popover_open(owner) {
    hide(app, owner);
  }
}

/// Whether window `owner`'s modal window shows a popover.
fn popover_open(owner: &str) -> bool {
  let Some(root) = MODALS.with(|modals| modals.borrow().get(owner).map(|m| m.root.clone())) else {
    return false;
  };
  tauri_plugin_gpui::with_app(move |cx| {
    root
      .read_with(cx, |root, _| root.popover.is_some())
      .unwrap_or(false)
  })
  .unwrap_or(false)
}

/// Closes window `owner`'s dialog and gives the window its focus back.
fn hide(app: &AppHandle, owner: &str) {
  let Some((window, root)) = MODALS.with(|modals| {
    modals
      .borrow()
      .get(owner)
      .map(|m| (m.window.clone(), m.root.clone()))
  }) else {
    return;
  };
  if !window.is_visible().unwrap_or(false) {
    return;
  }
  let _ = tauri_plugin_gpui::with_app(move |cx| {
    let _ = root.update(cx, |root, cx| {
      root.overlay = None;
      root.popover = None;
      cx.notify();
    });
  });
  let _ = window.hide();
  if let Some(parent) = tauri::Manager::get_window(app, owner) {
    let _ = parent.set_focus();
  }
}

/// Keeps window `owner`'s open dialog over it as it moves or resizes.
pub fn place(app: &AppHandle, owner: &str) {
  let window = MODALS.with(|modals| modals.borrow().get(owner).map(|m| m.window.clone()));
  if let Some(window) = window.filter(|w| w.is_visible().unwrap_or(false)) {
    cover(app, owner, &window);
  }
}

/// Destroys window `owner`'s modal window once the window is gone.
pub fn forget(owner: &str) {
  if let Some(modal) = MODALS.with(|modals| modals.borrow_mut().remove(owner)) {
    let _ = modal.window.destroy();
  }
}
