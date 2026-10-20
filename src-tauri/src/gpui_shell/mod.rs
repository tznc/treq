//! GPUI window shell for desktop repo windows.
//!
//! A repo window is a plain Tauri window with GPUI
//! attached as its renderer, and the React app runs in a child webview that
//! GPUI places with `webview_frame`. This is the first step of moving the
//! desktop UI to GPUI one piece at a time: GPUI owns the window, and every
//! screen that is still web UI lives in the webview.
//!
//! The workspace sidebar is the first screen drawn by GPUI (see `sidebar`).
//! Its tooltips and menus, and the app's toasts, open in popup windows of
//! their own (see `popup`), since GPUI cannot paint over the webview.
//!
//! The child webview has the same label as its window. Tauri treats a window
//! whose only webview shares its label as a webview window, so
//! `get_webview_window`, `emit_to` and the `WebviewWindow` command extractor
//! keep working as they do for a window built with `WebviewWindowBuilder`.

use std::{
  collections::{HashMap, VecDeque},
  sync::Mutex,
};

use tauri::{
  AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, Webview, WebviewBuilder, WebviewUrl,
  Wry,
};
use tauri_plugin_gpui::{
  gpui::{self, prelude::*, Global, SharedString, WeakEntity},
  webview_frame, GpuiConfig, GpuiWindowExt,
};

mod form;
mod modal;
pub mod native;
mod overlay;
mod popover;
mod popup;
pub mod remote_repo;
mod screen;
mod shortcuts;
mod sidebar;
mod theme;
mod time;
mod tooltips;
mod ui;

pub use overlay::OverlaySnapshot;
pub use popup::{MenuEntry, Toast};
pub use screen::{Modal, ScreenSnapshot};
use screen::{Screen, ScreenAction};
pub use sidebar::SidebarSnapshot;
use sidebar::{Sidebar, SidebarAction};

/// Set on `window` in the webview so the React app knows GPUI hosts it.
const HOSTED_FLAG_SCRIPT: &str = "window.__TREQ_GPUI_SHELL__ = true;";

/// Tells React that `TREQ_WEB_UI` keeps its screens, so it draws them whole.
const WEB_UI_FLAG_SCRIPT: &str = "window.__TREQ_WEB_UI__ = true;";

/// tauri-plugin-gpui stacks the webview above GPUI with an X11 child window
/// on Linux, so a Wayland session runs GTK through XWayland. Call before GTK
/// starts, while the process is still single-threaded.
pub fn force_x11_on_wayland() {
  if cfg!(target_os = "linux")
    && std::env::var_os("WAYLAND_DISPLAY").is_some()
    && std::env::var_os("GDK_BACKEND").is_none()
  {
    std::env::set_var("GDK_BACKEND", "x11");
  }
}

/// Starts GPUI. Call once from `setup`, before the first shell window.
pub fn init(app: &mut tauri::App<Wry>) -> Result<(), tauri_plugin_gpui::GpuiError> {
  let handle = app.handle().clone();
  tauri_plugin_gpui::init_with(
    app,
    GpuiConfig::new()
      .assets(theme::Assets)
      .on_launch(move |cx| {
        theme::init(cx);
        // Escape in a shell window closes an open menu, as in a native one.
        cx.observe_keystrokes(move |event, _, _| {
          if event.keystroke.key == "escape" {
            defer_main(&handle, |_| popup::hide_menus());
          }
        })
        .detach();
      }),
  )?;
  app.manage(ShellEnabled);
  Ok(())
}

/// Managed once `init` succeeds.
struct ShellEnabled;

/// Whether `init` ran, so new windows should open in the shell.
pub fn is_enabled<R: Runtime>(app: &AppHandle<R>) -> bool {
  app.try_state::<ShellEnabled>().is_some()
}

/// Root view of a shell window: the GPUI sidebar, once React sends it,
/// next to the webview, or next to a GPUI screen that takes the webview's
/// place.
struct Shell {
  app: AppHandle,
  label: SharedString,
  webview: Option<Webview<Wry>>,
  sidebar: Option<gpui::Entity<Sidebar>>,
  /// The screen GPUI draws instead of the webview, while React sends one.
  screen: Option<gpui::Entity<Screen>>,
  /// The dialog React asked for, drawn over the screen once there is one.
  modal: Option<Modal>,
}

impl Render for Shell {
  fn render(
    &mut self,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Self>,
  ) -> impl IntoElement {
    // Until React sends its theme, follow the OS.
    let dark = self
      .sidebar
      .as_ref()
      .map(|sidebar| sidebar.read(cx).snapshot().dark)
      .or_else(|| self.screen.as_ref().map(|s| s.read(cx).snapshot().dark))
      .unwrap_or(matches!(
        window.appearance(),
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
      ));
    let app = self.app.clone();
    let palette_sink = overlay_sink(self.app.clone(), self.label.to_string());
    gpui::div()
      .size_full()
      .flex()
      .font_family(theme::SANS_FONT)
      // Dashboard's shortcuts pressed in GPUI (the webview's keys reach
      // React directly) go back to it, as `shortcuts` says.
      .capture_key_down(move |event, window, cx| {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let contexts = window.context_stack();
        let within = |name: &str| contexts.iter().any(|c| c.contains(name));
        let found = shortcuts::app_shortcut(
          &keystroke.key,
          keystroke.key_char.as_deref(),
          m.secondary(),
          m.shift,
          m.alt,
          within("Input"),
          within(screen::TERMINAL_CONTEXT),
        );
        match found {
          Some(shortcuts::AppShortcut::Palette) => {
            let id = "overlay.commandPalette".to_string();
            palette_sink(ScreenAction::Press { id }, cx);
          }
          Some(shortcuts::AppShortcut::Key(key)) => palette_sink(
            ScreenAction::Field {
              field: "overlay.shortcut".into(),
              value: key.into(),
            },
            cx,
          ),
          None => {}
        }
      })
      // A click anywhere in the window closes an open menu, as one outside
      // a native menu does.
      .capture_any_mouse_down(move |_, _, _| defer_main(&app, |_| popup::hide_menus()))
      // Shown only until the webview's first frame, so opening a window
      // does not flash.
      .bg(theme::palette(dark).background)
      .when_some(self.sidebar.clone(), |this, sidebar| {
        // The cut-off overlay spans the sidebar too; the workspace view
        // dims its own part below the remote strip.
        let cut_off = self
          .screen
          .as_ref()
          .map(|s| s.read(cx))
          .filter(|s| s.cut_off());
        match cut_off {
          Some(screen) => {
            let p = screen.palette();
            this.child(
              gpui::div()
                .relative()
                .flex_none()
                .h_full()
                .flex()
                .child(sidebar)
                .child(screen::remote_chrome::cutoff_overlay(&p)),
            )
          }
          None => this.child(sidebar),
        }
      })
      .child(
        gpui::div()
          .flex_1()
          .min_w_0()
          .h_full()
          // Leaving the frame out hides the webview; React keeps running.
          .when_some(self.screen.clone(), |this, screen| this.child(screen))
          .when(self.screen.is_none(), |this| {
            this.when_some(self.webview.as_ref(), |this, webview| {
              this.child(webview_frame(webview))
            })
          }),
      )
  }
}

/// Shell views by window label, so the webview can be handed to its shell
/// once it exists.
#[derive(Default)]
struct Shells(HashMap<String, WeakEntity<Shell>>);

impl Global for Shells {}

/// Opens a repo window in the GPUI shell. Must run on the main thread.
pub fn open_window(
  app: &AppHandle,
  label: &str,
  url: String,
  title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  let window = tauri::WindowBuilder::new(app, label)
    .title(title)
    .inner_size(1400.0, 900.0)
    .build()?;
  #[cfg(target_os = "linux")]
  hide_menu_bar(&window)?;

  let shell_label = label.to_string();
  let shell_app = app.clone();
  window.attach_gpui(move |cx| {
    let shell = cx.new(|_| Shell {
      app: shell_app,
      label: shell_label.clone().into(),
      webview: None,
      sidebar: None,
      screen: None,
      modal: None,
    });
    cx.default_global::<Shells>()
      .0
      .insert(shell_label, shell.downgrade());
    shell
  })?;

  // Same label as the window; see the module docs.
  let webview = window.add_child(
    WebviewBuilder::new(label, WebviewUrl::App(url.into()))
      .initialization_script(HOSTED_FLAG_SCRIPT)
      .initialization_script(if std::env::var_os(screen::WEB_UI_ENV).is_some() {
        WEB_UI_FLAG_SCRIPT
      } else {
        ""
      }),
    LogicalPosition::new(0., 0.),
    LogicalSize::new(1., 1.),
  )?;
  // `webview_frame` shows it from its first frame.
  webview.hide()?;

  update_shell(label, move |shell, _| shell.webview = Some(webview))?;
  Ok(())
}

/// GPUI paints over GTK's menu bar, which still took the clicks on the top
/// ~25px (the header's Stack and branch buttons, the sidebar's settings).
/// Hidden for good, as `show_all` would show it again; its accelerators
/// (Ctrl+O...) stay with the window. Tauri adds it after `build` returns, so
/// this looks for it until it is there.
#[cfg(target_os = "linux")]
fn hide_menu_bar(window: &tauri::Window) -> tauri::Result<()> {
  use gtk::{
    glib::{timeout_add_local, ControlFlow},
    prelude::{Cast, ContainerExt, WidgetExt},
  };
  fn hide(widget: &gtk::Widget) -> bool {
    if let Some(bar) = widget.downcast_ref::<gtk::MenuBar>() {
      bar.set_no_show_all(true);
      bar.hide();
      return true;
    }
    widget
      .downcast_ref::<gtk::Container>()
      .is_some_and(|c| c.children().iter().any(hide))
  }
  let gtk_window = window.gtk_window()?;
  let mut tries = 0;
  timeout_add_local(std::time::Duration::from_millis(50), move || {
    tries += 1;
    if hide(gtk_window.upcast_ref()) || tries >= 100 {
      ControlFlow::Break
    } else {
      ControlFlow::Continue
    }
  });
  Ok(())
}

/// Runs `f` on the shell of window `label`, if it has one, and redraws it.
/// Must run on the main thread, outside a GPUI update.
fn update_shell(
  label: &str,
  f: impl FnOnce(&mut Shell, &mut gpui::Context<Shell>) + 'static,
) -> Result<(), tauri_plugin_gpui::GpuiError> {
  let label = label.to_string();
  tauri_plugin_gpui::with_app(move |cx| {
    let Some(shell) = cx
      .try_global::<Shells>()
      .and_then(|shells| shells.0.get(&label))
      .and_then(WeakEntity::upgrade)
    else {
      return;
    };
    shell.update(cx, |shell, cx| {
      f(shell, cx);
      cx.notify();
    });
  })
}

/// Shows `snapshot` in the GPUI sidebar of window `label`, or removes the
/// sidebar for `None`.
pub fn update_sidebar(app: &AppHandle, label: &str, snapshot: Option<SidebarSnapshot>) {
  let label = label.to_string();
  let _ = app.run_on_main_thread(move || {
    let result = update_shell(&label, move |shell, cx| {
      let Some(snapshot) = snapshot else {
        shell.sidebar = None;
        defer_main(&shell.app, |_| popup::hide_all());
        return;
      };
      match &shell.sidebar {
        Some(sidebar) => sidebar.update(cx, |sidebar, cx| sidebar.set_snapshot(snapshot, cx)),
        None => {
          let label = shell.label.to_string();
          let sink = action_sink(shell.app.clone(), label.clone());
          let popups = popup_sink(shell.app.clone(), label);
          shell.sidebar = Some(cx.new(|_| Sidebar::new(snapshot, sink, popups)));
        }
      }
    });
    if let Err(err) = result {
      log::warn!("failed to update the GPUI sidebar: {err}");
    }
  });
}

/// Draws `snapshot` in place of window `label`'s webview, or brings the
/// webview back for `None`. `TREQ_WEB_UI` keeps the webview, so React's
/// screens can be compared with GPUI's.
pub fn update_screen(app: &AppHandle, label: &str, snapshot: Option<ScreenSnapshot>) {
  let snapshot = snapshot.filter(|_| std::env::var_os(screen::WEB_UI_ENV).is_none());
  if let Some(snapshot) = &snapshot {
    remote_repo::set_window(label, snapshot.repository.clone());
  }
  let label = label.to_string();
  let _ = app.run_on_main_thread(move || {
    let result = update_shell(&label, move |shell, cx| match snapshot {
      None => {
        // A tooltip the screen showed would outlive it.
        if shell.screen.take().is_some() {
          cx.set_global(screen::ModalOpen::default());
          defer_main(&shell.app, |_| popup::hide_tooltip());
        }
      }
      Some(snapshot) => {
        theme::set_kit_dark(snapshot.dark, cx);
        match &shell.screen {
          Some(screen) => screen.update(cx, |screen, cx| screen.set_snapshot(snapshot, cx)),
          None => {
            let sink = screen_sink(shell.app.clone(), shell.label.to_string());
            let popups = popup_sink(shell.app.clone(), shell.label.to_string());
            let webview = shell.webview.clone();
            let host = std::sync::Arc::new(native::Backend::new(
              shell.app.clone(),
              shell.label.to_string(),
            ));
            let modal = shell.modal.clone();
            let screen = cx.new(|_| Screen::new(snapshot, sink, popups, webview, host));
            screen.update(cx, |screen, cx| screen.set_modal(modal, cx));
            shell.screen = Some(screen);
          }
        }
      }
    });
    if let Err(err) = result {
      log::warn!("failed to update the GPUI screen: {err}");
    }
  });
}

/// Draws `modal` over window `label`'s screen, or closes the open dialog for
/// `None`. A dialog React asks for while no screen is drawn waits for one.
pub fn update_modal(app: &AppHandle, label: &str, modal: Option<Modal>) {
  let modal = modal.filter(|_| std::env::var_os(screen::WEB_UI_ENV).is_none());
  let label = label.to_string();
  let _ = app.run_on_main_thread(move || {
    let result = update_shell(&label, move |shell, cx| {
      shell.modal = modal.clone();
      if let Some(screen) = &shell.screen {
        screen.update(cx, |screen, cx| screen.set_modal(modal, cx));
      }
    });
    if let Err(err) = result {
      log::warn!("failed to update the GPUI modal: {err}");
    }
  });
}

/// Sends screen actions to the webview of window `label`, as
/// [`action_sink`] does for the sidebar.
fn screen_sink(app: AppHandle, label: String) -> screen::ActionSink {
  std::rc::Rc::new(move |action: ScreenAction, _| {
    let (app, label) = (app.clone(), label.clone());
    tauri::async_runtime::spawn(async move {
      let target = tauri::EventTarget::webview_window(&label);
      if let Err(err) = tauri::Emitter::emit_to(&app, target, screen::ACTION_EVENT, action) {
        log::warn!("failed to send GPUI screen action: {err}");
      }
    });
  })
}

/// Shows `snapshot`'s dialog over the whole of window `label` (see
/// `overlay`), or closes it for `None`.
pub fn update_overlay(app: &AppHandle, label: &str, snapshot: Option<OverlaySnapshot>) {
  let label = label.to_string();
  defer_main(app, move |app| {
    let host = std::sync::Arc::new(native::Backend::new(app.clone(), label.clone()));
    let sink = overlay_sink(app.clone(), label.clone());
    modal::update(app, &label, snapshot, host, sink);
  });
}

/// Sends overlay actions to the webview of window `label`, as
/// [`screen_sink`] does for screens.
fn overlay_sink(app: AppHandle, label: String) -> screen::ActionSink {
  std::rc::Rc::new(move |action: ScreenAction, _| {
    let (app, label) = (app.clone(), label.clone());
    tauri::async_runtime::spawn(async move {
      let target = tauri::EventTarget::webview_window(&label);
      if let Err(err) = tauri::Emitter::emit_to(&app, target, overlay::ACTION_EVENT, action) {
        log::warn!("failed to send GPUI overlay action: {err}");
      }
    });
  })
}

/// Sends sidebar actions to the webview of window `label`. Tauri calls must
/// not run inside a GPUI update, so each send runs in a task.
fn action_sink(app: AppHandle, label: String) -> sidebar::ActionSink {
  std::rc::Rc::new(move |action: SidebarAction, _| {
    let (app, label) = (app.clone(), label.clone());
    tauri::async_runtime::spawn(async move {
      let target = tauri::EventTarget::webview_window(&label);
      if let Err(err) = tauri::Emitter::emit_to(&app, target, sidebar::ACTION_EVENT, action) {
        log::warn!("failed to send GPUI sidebar action: {err}");
      }
    });
  })
}

/// Opens the sidebar's tooltip popups for window `label`.
fn popup_sink(app: AppHandle, label: String) -> popup::PopupSink {
  std::rc::Rc::new(move |request, _| {
    let label = label.clone();
    defer_main(&app, move |app| popup::handle(app, &label, request));
  })
}

type Deferred = Box<dyn FnOnce(&AppHandle) + Send>;

/// Calls waiting for the main thread, in the order they were deferred.
static DEFERRED: Mutex<VecDeque<Deferred>> = Mutex::new(VecDeque::new());

/// Runs `f` on the main thread once the current GPUI update and Tauri call
/// have returned, after every call deferred before it (so a click's "close
/// the menu" never lands after the menu it opens).
fn defer_main(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
  if let Ok(mut deferred) = DEFERRED.lock() {
    deferred.push_back(Box::new(f));
  }
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
      while let Some(next) = DEFERRED.lock().ok().and_then(|mut d| d.pop_front()) {
        next(&handle);
      }
    });
  });
}

/// Hands GPUI the account Rust owns ([`crate::core::cloud`]); every shell
/// window's screens draw from it.
pub fn account_changed(app: &AppHandle, snapshot: crate::core::cloud::AccountSnapshot) {
  let account = screen::account_data::Account(Some(screen::account_data::route_from(&snapshot)));
  defer_main(app, move |_| {
    let result = tauri_plugin_gpui::with_app(move |cx| {
      if cx.try_global::<screen::account_data::Account>() != Some(&account) {
        cx.set_global(account);
        cx.refresh_windows();
      }
    });
    if let Err(err) = result {
      log::warn!("failed to update the GPUI account: {err}");
    }
  });
}

/// Opens a menu of `entries` at the last right-click in window `label`'s
/// sidebar.
pub fn show_menu(app: &AppHandle, label: &str, entries: Vec<MenuEntry>) {
  let label = label.to_string();
  defer_main(app, move |app| {
    // A screen's dropdown button asked React for this menu.
    if let Some(anchor) = popup::take_dropdown_anchor() {
      let dark = tauri_plugin_gpui::with_app({
        let label = label.clone();
        move |cx| {
          let screen = cx
            .try_global::<Shells>()
            .and_then(|shells| shells.0.get(&label))
            .and_then(WeakEntity::upgrade)
            .and_then(|shell| shell.read(cx).screen.clone())?;
          Some(screen.read(cx).snapshot().dark)
        }
      });
      if let Ok(Some(dark)) = dark {
        popup::show_dropdown(app, &label, entries, anchor, dark);
        return;
      }
    }
    let anchor = tauri_plugin_gpui::with_app({
      let label = label.clone();
      move |cx| {
        let sidebar = cx
          .try_global::<Shells>()
          .and_then(|shells| shells.0.get(&label))
          .and_then(WeakEntity::upgrade)
          .and_then(|shell| shell.read(cx).sidebar.clone())?;
        let sidebar = sidebar.read(cx);
        Some((sidebar.menu_anchor(), sidebar.snapshot().dark))
      }
    });
    if let Ok(Some((at, dark))) = anchor {
      popup::show_menu(app, &label, entries, at, dark);
    }
  });
}

/// Closes an open menu.
pub fn hide_menus(app: &AppHandle) {
  defer_main(app, |_| popup::hide_menus());
}

/// Closes every popup, as when a shell window loses focus.
pub fn hide_popups(app: &AppHandle) {
  defer_main(app, |_| popup::hide_all());
}

/// Shows window `label`'s toasts, in its sidebar's theme.
pub fn update_toasts(app: &AppHandle, label: &str, toasts: Vec<Toast>) {
  let label = label.to_string();
  defer_main(app, move |app| {
    let dark = tauri_plugin_gpui::with_app({
      let label = label.clone();
      move |cx| {
        let sidebar = cx
          .try_global::<Shells>()
          .and_then(|shells| shells.0.get(&label))
          .and_then(WeakEntity::upgrade)
          .and_then(|shell| shell.read(cx).sidebar.clone())?;
        Some(sidebar.read(cx).snapshot().dark)
      }
    });
    let dark = dark.ok().flatten().unwrap_or(false);
    popup::show_toasts(app, &label, toasts, dark);
  });
}

/// Keeps window `label`'s toasts in its corner, and its open dialog over
/// it, as it moves or resizes.
pub fn place_toasts(app: &AppHandle, label: &str) {
  let label = label.to_string();
  defer_main(app, move |app| {
    popup::place_owner_toasts(app, &label);
    modal::place(app, &label);
  });
}

/// Forgets a destroyed window's shell.
pub fn forget_window(label: &str) {
  popup::forget_owner(label);
  remote_repo::set_window(label, None);
  modal::forget(label);
  let label = label.to_string();
  let _ = tauri_plugin_gpui::with_app(move |cx| {
    if let Some(shells) = cx.try_global::<Shells>() {
      if shells.0.contains_key(&label) {
        cx.global_mut::<Shells>().0.remove(&label);
      }
    }
  });
}
