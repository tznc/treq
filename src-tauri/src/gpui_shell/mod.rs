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

mod checks_run;
mod form;
mod keys;
pub mod launch;
mod menus;
mod modal;
pub mod native;
mod overlay;
mod popover;
mod popup;
pub mod remote_repo;
mod screen;
mod shortcuts;
mod sidebar;
mod sidebar_actions;
mod theme;
mod time;
mod toasts;
mod tooltips;
mod ui;
mod window;

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
  window::listen(app.handle());
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
  /// The window model, which draws the window unless `TREQ_WEB_UI` is set
  /// (see `window`).
  window: Option<gpui::Entity<window::WindowController>>,
  /// What the model last presented.
  presented: Option<window::Presentation>,
  /// `ui_zoom`, in percent.
  zoom: u32,
  /// The OS appearance last rendered under.
  os_dark: Option<bool>,
  /// Sends Dashboard's shortcuts pressed in this window to React.
  shortcuts: Option<gpui::Subscription>,
}

impl Shell {
  /// Dashboard's shortcuts pressed in GPUI (the webview's keys reach React
  /// directly) go back to it, as `shortcuts` says. An interceptor sees every
  /// key, focused element or not, before a text field takes it.
  fn watch_shortcuts(&mut self, window: &gpui::Window, cx: &mut gpui::Context<Self>) {
    if self.shortcuts.is_some() {
      return;
    }
    let sink = overlay_sink(self.app.clone(), self.label.to_string());
    let label = self.label.to_string();
    let handle = window.window_handle();
    self.shortcuts = Some(cx.intercept_keystrokes(move |event, window, cx| {
      if window.window_handle() != handle {
        return;
      }
      let keystroke = &event.keystroke;
      let m = keys::held(keystroke.modifiers);
      let within = |name: &str| event.context_stack.iter().any(|c| c.contains(name));
      let found = shortcuts::app_shortcut(
        &keystroke.key,
        keystroke.key_char.as_deref(),
        m.secondary(),
        m.shift,
        m.alt,
        within("Input"),
        within(screen::TERMINAL_CONTEXT),
      );
      // The window model answers them itself, with the terminal pane's
      // and the zoom keys.
      if window::drives(&label) {
        let shortcut = found
          .map(window::from_app)
          .or_else(|| window::window_shortcut(&keystroke.key, m, within(screen::TERMINAL_CONTEXT)));
        if let Some(shortcut) = shortcut {
          // An open dialog still takes its Escape.
          if shortcut != window::Shortcut::Escape {
            cx.stop_propagation();
          }
          window::shortcut(&label, shortcut, cx);
        }
        return;
      }
      let action = match found {
        Some(shortcuts::AppShortcut::Palette) => ScreenAction::Press {
          id: "overlay.commandPalette".into(),
        },
        Some(shortcuts::AppShortcut::Key(key)) => ScreenAction::Field {
          field: "overlay.shortcut".into(),
          value: key.into(),
        },
        None => return,
      };
      // React's handlers take the key, so a text field does not type it.
      cx.stop_propagation();
      sink(action, cx);
    }));
  }
}

impl Render for Shell {
  fn render(
    &mut self,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Self>,
  ) -> impl IntoElement {
    let os_dark = matches!(
      window.appearance(),
      gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
    );
    window::follow_appearance(self, os_dark, cx);
    // Until React sends its theme, follow the OS.
    let dark = self
      .sidebar
      .as_ref()
      .map(|sidebar| sidebar.read(cx).snapshot().dark)
      .or_else(|| self.screen.as_ref().map(|s| s.read(cx).snapshot().dark))
      .unwrap_or(os_dark);
    let app = self.app.clone();
    window.set_rem_size(gpui::px(16. * self.zoom as f32 / 100.));
    self.watch_shortcuts(window, cx);
    gpui::div()
      .size_full()
      .flex()
      .font_family(theme::SANS_FONT)
      // A click anywhere in the window closes an open menu, as one outside
      // a native menu does.
      .capture_any_mouse_down(move |_, window, cx| {
        // Blurs the focused input too, as a browser does.
        window.blur(cx);
        defer_main(&app, |_| popup::hide_menus())
      })
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
/// `repo` is the repository it opens on (`None`: the start screen).
pub fn open_window(
  app: &AppHandle,
  label: &str,
  repo: Option<String>,
  title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  let url = crate::open_new_window::window_url(repo.as_deref());
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
      window: None,
      presented: None,
      zoom: window::DEFAULT_ZOOM,
      os_dark: None,
      shortcuts: None,
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
  if std::env::var_os(screen::WEB_UI_ENV).is_none() {
    window::start(app, label, repo)?;
  }
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
/// sidebar for `None`. A window the model drives ignores React's.
pub fn update_sidebar(app: &AppHandle, label: &str, snapshot: Option<SidebarSnapshot>) {
  let label = label.to_string();
  let _ = app.run_on_main_thread(move || {
    let result = update_shell(&label, move |shell, cx| {
      if shell.react_draws() {
        shell.show_sidebar(snapshot, cx);
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
  let label = label.to_string();
  let _ = app.run_on_main_thread(move || {
    let result = update_shell(&label, move |shell, cx| {
      if shell.react_draws() {
        shell.show_screen(snapshot, cx);
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
      if shell.react_draws() {
        shell.show_modal(modal, cx);
      }
    });
    if let Err(err) = result {
      log::warn!("failed to update the GPUI modal: {err}");
    }
  });
}

impl Shell {
  /// Whether React's pushes are drawn: only without the window model
  /// (`TREQ_WEB_UI`).
  fn react_draws(&self) -> bool {
    self.window.is_none()
  }

  /// Draws what the window model presents.
  fn present(&mut self, next: window::Presentation, cx: &mut gpui::Context<Self>) {
    let before = self.presented.take();
    if before.as_ref().is_none_or(|b| b.sidebar != next.sidebar) {
      self.show_sidebar(next.sidebar.clone(), cx);
    }
    if before.as_ref().is_none_or(|b| b.screen != next.screen) {
      self.show_screen(next.screen.clone(), cx);
    }
    if before.as_ref().is_none_or(|b| b.modal != next.modal) {
      self.show_modal(next.modal.clone(), cx);
    }
    if before.as_ref().is_none_or(|b| b.overlay != next.overlay) {
      self.show_overlay(next.overlay.clone());
    }
    self.show_file_picker(next.file_picker.clone(), cx);
    self.zoom = next.zoom;
    self.presented = Some(next);
    cx.notify();
  }

  fn show_sidebar(&mut self, snapshot: Option<SidebarSnapshot>, cx: &mut gpui::Context<Self>) {
    let Some(snapshot) = snapshot else {
      if self.sidebar.take().is_some() {
        defer_main(&self.app, |_| popup::hide_all());
      }
      return;
    };
    match &self.sidebar {
      Some(sidebar) => sidebar.update(cx, |sidebar, cx| sidebar.set_snapshot(snapshot, cx)),
      None => {
        let label = self.label.to_string();
        let sink = action_sink(self.app.clone(), label.clone());
        let popups = popup_sink(self.app.clone(), label);
        self.sidebar = Some(cx.new(|_| Sidebar::new(snapshot, sink, popups)));
      }
    }
  }

  fn show_screen(&mut self, snapshot: Option<ScreenSnapshot>, cx: &mut gpui::Context<Self>) {
    if let Some(snapshot) = &snapshot {
      remote_repo::set_window(&self.label, snapshot.repository.clone());
      // Agent links queued for this window's repository start once it shows.
      if let screen::View::Workspace(route) = &snapshot.view {
        launch::window_shows_repo(&self.app, &self.label, &route.repo_path);
      }
    }
    match snapshot {
      None => {
        // A tooltip the screen showed would outlive it.
        if self.screen.take().is_some() {
          cx.set_global(screen::ModalOpen::default());
          defer_main(&self.app, |_| popup::hide_tooltip());
        }
      }
      Some(snapshot) => {
        theme::set_kit_dark(snapshot.dark, cx);
        match &self.screen {
          Some(screen) => screen.update(cx, |screen, cx| screen.set_snapshot(snapshot, cx)),
          None => {
            let sink = screen_sink(self.app.clone(), self.label.to_string());
            let popups = popup_sink(self.app.clone(), self.label.to_string());
            let webview = self.webview.clone();
            let host = std::sync::Arc::new(native::Backend::new(
              self.app.clone(),
              self.label.to_string(),
            ));
            let modal = self.modal.clone();
            let screen = cx.new(|cx| Screen::new(snapshot, sink, popups, webview, host, cx));
            screen.update(cx, |screen, cx| screen.set_modal(modal, cx));
            self.screen = Some(screen);
          }
        }
      }
    }
  }

  fn show_modal(&mut self, modal: Option<Modal>, cx: &mut gpui::Context<Self>) {
    self.modal = modal.clone();
    if let Some(screen) = &self.screen {
      screen.update(cx, |screen, cx| screen.set_modal(modal, cx));
    }
  }

  fn show_overlay(&self, snapshot: Option<OverlaySnapshot>) {
    let label = self.label.to_string();
    defer_main(&self.app, move |app| {
      let host = std::sync::Arc::new(native::Backend::new(app.clone(), label.clone()));
      let sink = overlay_sink(app.clone(), label.clone());
      modal::update(app, &label, snapshot, host, sink);
    });
  }

  fn show_file_picker(
    &mut self,
    scope: Option<screen::file_picker::Scope>,
    cx: &mut gpui::Context<Self>,
  ) {
    if let Some(screen) = &self.screen {
      screen.update(cx, |screen, cx| screen.set_file_picker(scope, cx));
    }
  }
}

/// Sends screen actions to the webview of window `label`, as
/// [`action_sink`] does for the sidebar.
fn screen_sink(app: AppHandle, label: String) -> screen::ActionSink {
  window::routed(app, label, screen::ACTION_EVENT, window::screen_action)
}

/// Shows `snapshot`'s dialog over the whole of window `label` (see
/// `overlay`), or closes it for `None`.
pub fn update_overlay(app: &AppHandle, label: &str, snapshot: Option<OverlaySnapshot>) {
  let label = label.to_string();
  defer_main(app, move |app| {
    if !window::react_overlay(&label, snapshot.clone()) {
      return;
    }
    let host = std::sync::Arc::new(native::Backend::new(app.clone(), label.clone()));
    let sink = overlay_sink(app.clone(), label.clone());
    modal::update(app, &label, snapshot, host, sink);
  });
}

/// Sends overlay actions to the webview of window `label`, as
/// [`screen_sink`] does for screens.
fn overlay_sink(app: AppHandle, label: String) -> screen::ActionSink {
  window::routed(app, label, overlay::ACTION_EVENT, window::overlay_action)
}

/// Sends `payload` as event `event` to the webview of window `label`.
/// Callable inside a GPUI update.
fn emit_to_webview<S: serde::Serialize + Clone + Send + 'static>(
  app: &AppHandle,
  label: &str,
  event: &'static str,
  payload: S,
) {
  let (app, label) = (app.clone(), label.to_string());
  tauri::async_runtime::spawn(async move {
    let target = tauri::EventTarget::webview_window(&label);
    if let Err(err) = tauri::Emitter::emit_to(&app, target, event, payload) {
      log::warn!("failed to send {event}: {err}");
    }
  });
}

/// Runs the sidebar actions GPUI handles itself ([`sidebar_actions`]) and
/// sends the rest to the webview of window `label`. Tauri calls must not
/// run inside a GPUI update, so each send runs in a task.
fn action_sink(app: AppHandle, label: String) -> sidebar::ActionSink {
  fn send(app: AppHandle, label: String, action: SidebarAction) {
    tauri::async_runtime::spawn(async move {
      let target = tauri::EventTarget::webview_window(&label);
      if let Err(err) = tauri::Emitter::emit_to(&app, target, sidebar::ACTION_EVENT, action) {
        log::warn!("failed to send GPUI sidebar action: {err}");
      }
    });
  }
  std::rc::Rc::new(move |action: SidebarAction, cx| {
    let (app, label) = (app.clone(), label.clone());
    let driven = window::drives(&label);
    if !driven && !sidebar_actions::handles(&action) {
      return send(app, label, action);
    }
    // The sidebar is mid-update while it sends; read it once it is done.
    cx.defer(move |cx| {
      if driven && window::sidebar_action(&label, &action, cx) {
        return;
      }
      if !sidebar_actions::handles(&action) {
        return send(app, label, action);
      }
      let snapshot = cx
        .try_global::<Shells>()
        .and_then(|shells| shells.0.get(&label))
        .and_then(WeakEntity::upgrade)
        .and_then(|shell| shell.read(cx).sidebar.clone())
        .map(|sidebar| sidebar.read(cx).snapshot().clone());
      let handled = snapshot
        .is_some_and(|snapshot| sidebar_actions::handle(&app, &label, &snapshot, &action, cx));
      if !handled && !driven {
        send(app, label, action);
      }
    });
  })
}

/// Closes the terminals of a workspace GPUI removed, in window `label`.
/// Callable from any thread.
fn close_workspace_terminals(app: &AppHandle, label: &str, path: String) {
  terminal_command(app, label, "closeWorkspace", Some(path));
}

/// Gives window `label`'s terminal pane a command of kind `kind`.
/// Callable from any thread.
fn terminal_command(app: &AppHandle, label: &str, kind: &'static str, dir: Option<String>) {
  let label = label.to_string();
  defer_main(app, move |_| {
    let _ = update_shell(&label, move |shell, cx| {
      if let Some(screen) = &shell.screen {
        screen.update(cx, |screen, cx| screen.terminal_command(kind, dir, cx));
      }
    });
  });
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
/// window's screens draw from it, and the window models follow the cloud
/// workspace's state.
pub fn account_changed(app: &AppHandle, snapshot: crate::core::cloud::AccountSnapshot) {
  let state = snapshot
    .cloud
    .instance_status
    .as_ref()
    .and_then(|s| s.instance.as_ref())
    .map(|i| i.status.clone());
  window::instance_state(app, state);
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

/// Shows React's toasts in window `label`, with the app's own
/// ([`toasts`]).
pub fn update_toasts(app: &AppHandle, label: &str, toasts: Vec<Toast>) {
  toasts::set_react(app, label, toasts);
}

/// Draws window `label`'s toasts, in its sidebar's theme.
fn draw_toasts(app: &AppHandle, label: &str, toasts: Vec<Toast>) {
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

/// A menu bar item for window `label`, sent to React as `event` with
/// `payload`. True when the window model handles it instead.
pub fn menu_event(app: &AppHandle, label: &str, event: &str, payload: Option<String>) -> bool {
  window::menu_event(app, label, event, payload)
}

/// Window `label` gained focus.
pub fn window_focused(app: &AppHandle, label: &str) {
  window::window_focused(app, label);
}

/// The cloud workspace connected (for window `label`, which asked) or its
/// certificate renewed. True when the window models take it.
pub fn managed_endpoint(
  app: &AppHandle,
  label: Option<&str>,
  event: crate::core::cloud::ManagedEndpointEvent,
) -> bool {
  if std::env::var_os(screen::WEB_UI_ENV).is_some() {
    return false;
  }
  window::managed_endpoint(app, label, event);
  true
}

/// An endpoint was reauthenticated: its cutoff is lifted everywhere.
pub fn cutoff_cleared(app: &AppHandle, endpoint_id: &str) {
  window::cutoff_cleared(app, endpoint_id);
}

/// Forgets a destroyed window's shell.
pub fn forget_window(label: &str) {
  popup::forget_owner(label);
  toasts::forget(label);
  remote_repo::set_window(label, None);
  modal::forget(label);
  window::forget(label);
  let label = label.to_string();
  let _ = tauri_plugin_gpui::with_app(move |cx| {
    if let Some(shells) = cx.try_global::<Shells>() {
      if shells.0.contains_key(&label) {
        cx.global_mut::<Shells>().0.remove(&label);
      }
    }
  });
}
