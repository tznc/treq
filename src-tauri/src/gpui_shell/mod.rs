//! GPUI window shell for desktop repo windows.
//!
//! A repo window is a plain Tauri window with GPUI attached as its renderer.
//! Its window model (`window`) owns the route, the selection, the dialogs
//! and the data, and builds what the shell draws: the workspace sidebar
//! (`sidebar`), a screen (`screen`) and the dialogs over them. Tooltips,
//! menus and toasts open in popup windows of their own (see `popup`).

use crate::result_ext::ResultExt;
use std::{
  collections::{HashMap, VecDeque},
  sync::Mutex,
};

use tauri::{AppHandle, Wry};
use tauri_plugin_gpui::{
  gpui::{self, prelude::*, Global, SharedString, WeakEntity},
  GpuiConfig, GpuiWindowExt,
};

mod checks_run;
mod confirm;
#[cfg(feature = "e2e")]
pub mod e2e;
mod form;
#[cfg(test)]
mod fuzz;
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
#[doc(hidden)]
pub use screen::bench;
mod shortcuts;
mod sidebar;
mod sidebar_actions;
use treq_ui::theme;
mod time;
mod title_bar;
mod toasts;
mod tooltips;
use treq_ui as ui;
mod window;

pub use overlay::OverlaySnapshot;
pub use popup::{MenuEntry, Toast};
use screen::Screen;
pub use screen::{Modal, ScreenSnapshot};
pub use sidebar::SidebarSnapshot;
use sidebar::{Sidebar, SidebarAction};
pub(crate) use window::note_just_initialized;

/// Tooltips, menus, toasts and the modal window are windows of their own,
/// placed at screen positions (`popup`, `modal`), which a Wayland client
/// cannot choose, so a Wayland session runs GTK through XWayland. Call
/// before GTK starts, while the process is still single-threaded.
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
          if ui::is_escape(&event.keystroke) {
            defer_main(&handle, |_| popup::hide_menus());
          }
        })
        .detach();
        #[cfg(feature = "e2e")]
        e2e::start(cx);
      }),
  )?;
  window::listen(app.handle());
  Ok(())
}

/// Root view of a shell window: the sidebar next to a screen, as the
/// window model presents them.
struct Shell {
  app: AppHandle,
  label: SharedString,
  sidebar: Option<gpui::Entity<Sidebar>>,
  screen: Option<gpui::Entity<Screen>>,
  /// The dialog drawn over the screen once there is one.
  modal: Option<Modal>,
  /// The window model, which draws the window (see `window`).
  window: Option<gpui::Entity<window::WindowController>>,
  /// What the model last presented.
  presented: Option<window::Presentation>,
  /// The OS appearance last rendered under.
  os_dark: Option<bool>,
  /// Hands the window's shortcuts to its model.
  shortcuts: Option<gpui::Subscription>,
  /// Focused while the model's confirmation shows, so it hears Enter and
  /// Escape.
  confirm_focus: Option<gpui::FocusHandle>,
}

impl Shell {
  /// The app's shortcuts pressed in this window go to the window model, as
  /// `shortcuts` and `window::window_shortcut` say. An interceptor sees
  /// every key, focused element or not, before a text field takes it.
  fn watch_shortcuts(&mut self, window: &gpui::Window, cx: &mut gpui::Context<Self>) {
    if self.shortcuts.is_some() {
      return;
    }
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
      // With the terminal pane's and the zoom keys.
      let shortcut = found
        .map(window::from_app)
        .or_else(|| window::window_shortcut(&keystroke.key, m, within(screen::TERMINAL_CONTEXT)));
      if shortcut == Some(window::Shortcut::Escape)
        && (modal::escape_popover(&label, cx) || ui::escape_menus(cx))
      {
        cx.stop_propagation();
        return;
      }
      if let Some(shortcut) = shortcut {
        // An open dialog still takes its Escape.
        if shortcut != window::Shortcut::Escape {
          cx.stop_propagation();
        }
        window::shortcut(&label, shortcut, cx);
      }
    }));
  }
}

impl Render for Shell {
  fn render(
    &mut self,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Self>,
  ) -> impl IntoElement {
    #[cfg(feature = "e2e")]
    {
      e2e::begin_frame(window);
      e2e::shell_window(window.window_handle());
    }
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| {
      crate::startup::mark("first render");
      // The syntax set takes a while to load; load it before the first
      // diff, but after the first frame, which it would compete with for a
      // core.
      window.on_next_frame(|_, _| {
        std::thread::Builder::new()
          .name("warm-highlighting".into())
          .spawn(screen::warm_highlighting)
          .warn_on_err("spawn highlighting warm-up thread");
      });
    });
    let os_dark = matches!(
      window.appearance(),
      gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
    );
    window::follow_appearance(self, os_dark, cx);
    // Until the model presents its theme, follow the OS.
    let dark = self
      .sidebar
      .as_ref()
      .map(|sidebar| sidebar.read(cx).snapshot().dark)
      .or_else(|| self.screen.as_ref().map(|s| s.read(cx).snapshot().dark))
      .unwrap_or(os_dark);
    let app = self.app.clone();
    ui::apply_zoom(window, cx);
    self.watch_shortcuts(window, cx);
    let confirm = self.confirm(window, dark, cx);
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
      .bg(theme::palette(dark).background)
      .when_some(self.sidebar.clone(), |this, sidebar| {
        let sidebar = Sidebar::element(&sidebar, cx);
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
          .flex()
          .flex_col()
          // The screen runs to the window's top edge beside the sidebar,
          // which holds the title bar; without it, the screen moves down.
          .when(title_bar::SHOWN && self.sidebar.is_none(), |this| {
            this.child(title_bar::drag_area("screen-title-bar"))
          })
          .when_some(self.screen.clone(), |this, screen| {
            this.child(
              gpui::div()
                .flex_1()
                .min_h_0()
                .child(Screen::element(&screen)),
            )
          }),
      )
      .children(confirm)
  }
}

impl Shell {
  /// The model's confirmation, over everything; it takes the focus as it
  /// opens.
  fn confirm(
    &mut self,
    window: &mut gpui::Window,
    dark: bool,
    cx: &mut gpui::Context<Self>,
  ) -> Option<ui::ConfirmDialog> {
    let Some(question) = self.presented.as_ref().and_then(|p| p.confirm.clone()) else {
      self.confirm_focus = None;
      return None;
    };
    let focus = self
      .confirm_focus
      .get_or_insert_with(|| {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        focus
      })
      .clone();
    let label = self.label.to_string();
    Some(
      question.confirm_dialog(&focus, window, &theme::palette(dark), move |yes, _, cx| {
        window::answer(&label, yes, cx)
      }),
    )
  }
}

/// Shell views by window label.
#[derive(Default)]
struct Shells(HashMap<String, WeakEntity<Shell>>);

impl Global for Shells {}

/// The smallest a repo window gets: room for the default sidebar beside a
/// screen whose tabs fit on one row at 100% zoom (the header wraps).
const MIN_WIDTH: f64 = 900.;
const MIN_HEIGHT: f64 = 500.;

/// Opens a repo window in the GPUI shell. Must run on the main thread.
/// `repo` is the repository it opens on (`None`: the start screen).
pub fn open_window(
  app: &AppHandle,
  label: &str,
  repo: Option<String>,
  title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  let window = tauri::WindowBuilder::new(app, label)
    .title(title)
    .inner_size(1400.0, 900.0)
    .min_inner_size(MIN_WIDTH, MIN_HEIGHT);
  // The default style already extends the content under an opaque bar,
  // hiding GPUI's top rows. An overlay leaves only the traffic lights, over
  // the title bar GPUI draws (`title_bar`).
  #[cfg(target_os = "macos")]
  let window = window
    .title_bar_style(tauri::TitleBarStyle::Overlay)
    .hidden_title(true);
  let window = window.build()?;
  crate::startup::mark("window built");
  #[cfg(target_os = "linux")]
  hide_menu_bar(&window)?;

  let shell_label = label.to_string();
  let shell_app = app.clone();
  // The first frame follows the OS appearance, before GPUI reports it.
  let os_dark = window.theme().ok().map(|theme| theme == tauri::Theme::Dark);
  window.attach_gpui(move |cx| {
    let shell = cx.new(|_| Shell {
      app: shell_app,
      label: shell_label.clone().into(),
      sidebar: None,
      screen: None,
      modal: None,
      window: None,
      presented: None,
      os_dark,
      shortcuts: None,
      confirm_focus: None,
    });
    cx.default_global::<Shells>()
      .0
      .insert(shell_label, shell.downgrade());
    shell
  })?;

  crate::startup::mark("gpui attached");
  window::start(app, label, repo)?;
  crate::startup::mark("window model started");
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

impl Shell {
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
    // Every GPUI window draws at the app's zoom.
    if cx.try_global::<ui::UiZoom>() != Some(&ui::UiZoom(next.zoom)) {
      cx.set_global(ui::UiZoom(next.zoom));
      theme::sync_zoom(cx);
      cx.refresh_windows();
    }
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
          defer_main(&self.app, |_| popup::hide_tooltip());
        }
      }
      Some(snapshot) => {
        theme::set_dark(snapshot.dark, cx);
        match &self.screen {
          Some(screen) => screen.update(cx, |screen, cx| screen.set_snapshot(snapshot, cx)),
          None => {
            let sink = screen_sink(self.label.to_string());
            let popups = popup_sink(self.app.clone(), self.label.to_string());
            let host = std::sync::Arc::new(native::Backend::new(
              self.app.clone(),
              self.label.to_string(),
            ));
            let modal = self.modal.clone();
            let screen = cx.new(|cx| Screen::new(snapshot, sink, popups, host, cx));
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
      let sink = overlay_sink(label.clone());
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

/// Hands screen actions to window `label`'s model.
fn screen_sink(label: String) -> screen::ActionSink {
  window::routed(label, window::screen_action)
}

/// Hands overlay actions to window `label`'s model, as [`screen_sink`]
/// does for screens.
fn overlay_sink(label: String) -> screen::ActionSink {
  window::routed(label, window::overlay_action)
}

/// Hands sidebar actions to window `label`'s model, and runs those it
/// leaves to GPUI ([`sidebar_actions`]).
fn action_sink(app: AppHandle, label: String) -> sidebar::ActionSink {
  std::rc::Rc::new(move |action: SidebarAction, cx| {
    let (app, label) = (app.clone(), label.clone());
    // The sidebar is mid-update while it sends; read it once it is done.
    cx.defer(move |cx| {
      if let SidebarAction::PrefetchWorkspace { id } = action {
        return prefetch_workspace(&label, id, cx);
      }
      if window::sidebar_action(&label, &action, cx) || !sidebar_actions::handles(&action) {
        return;
      }
      let snapshot = cx
        .try_global::<Shells>()
        .and_then(|shells| shells.0.get(&label))
        .and_then(WeakEntity::upgrade)
        .and_then(|shell| shell.read(cx).sidebar.clone())
        .map(|sidebar| sidebar.read(cx).snapshot().clone());
      if let Some(snapshot) = snapshot {
        sidebar_actions::handle(&app, &label, &snapshot, &action, cx);
      }
    });
  })
}

/// Reads ahead in window `label` what selecting workspace `id` of its
/// repository would show.
fn prefetch_workspace(label: &str, id: i64, cx: &mut gpui::App) {
  let Some(route) = window::workspace_route(label, id, cx) else {
    return;
  };
  let screen = cx
    .try_global::<Shells>()
    .and_then(|shells| shells.0.get(label))
    .and_then(WeakEntity::upgrade)
    .and_then(|shell| shell.read(cx).screen.clone());
  if let Some(screen) = screen {
    screen.update(cx, |screen, cx| screen.prefetch_workspace(route, cx));
  }
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
    update_shell(&label, move |shell, cx| {
      if let Some(screen) = &shell.screen {
        screen.update(cx, |screen, cx| screen.terminal_command(kind, dir, cx));
      }
    })
    .ignore_closed();
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

/// Runs `future` to completion on the calling thread, for host methods
/// that call async backend code. Hosts run on GPUI's background executor;
/// on the main thread this would freeze the window until the future (often a
/// network call) finished, so debug builds refuse it. Use this, not
/// `tauri::async_runtime::block_on`, in `gpui_shell`.
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
  debug_assert!(
    std::thread::current().name() != Some("main"),
    "gpui_shell::block_on called on the main thread; call the host from a background task"
  );
  tauri::async_runtime::block_on(future)
}

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
    app
      .run_on_main_thread(move || {
        while let Some(next) = DEFERRED.lock().ok().and_then(|mut d| d.pop_front()) {
          next(&handle);
        }
      })
      .ignore_closed();
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

/// Closes every popup, as when a shell window loses focus.
pub fn hide_popups(app: &AppHandle) {
  defer_main(app, |_| popup::hide_all());
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

/// A menu bar item for window `label`, named by `event`, with `payload`.
/// True when the window model handles it.
pub fn menu_event(app: &AppHandle, label: &str, event: &str, payload: Option<String>) -> bool {
  window::menu_event(app, label, event, payload)
}

/// A `treq://skill/install` link for window `label`: it shows the install
/// confirmation for catalog skill `skill_id`. Any thread.
pub fn skill_install_link(app: &AppHandle, label: &str, skill_id: String) {
  window::skill_install_link(app, label, skill_id);
}

/// Types `keystroke` (GPUI's notation, e.g. "cmd-v") into the focused GPUI
/// window, as if pressed there. On macOS a menu item's key equivalent takes
/// the key press before the window sees it, so the Edit menu hands its keys
/// back this way: text fields and the terminal handle them as typed.
pub fn type_keystroke(app: &AppHandle, keystroke: &'static str) {
  defer_main(app, move |_| {
    tauri_plugin_gpui::with_app(move |cx| {
      let Ok(keystroke) = gpui::Keystroke::parse(keystroke) else {
        return;
      };
      // Opening the menu bar with the mouse can leave no window active.
      let Some(window) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
      else {
        return;
      };
      window
        .update(cx, |_, window, cx| {
          window.dispatch_keystroke(keystroke, cx);
        })
        .ignore_closed();
    })
    .ignore_closed();
  });
}

/// Window `label` gained (`focused`) or lost focus.
pub fn window_focused(app: &AppHandle, label: &str, focused: bool) {
  window::window_focused(app, label, focused);
}

/// The cloud workspace connected (for window `label`, which asked) or its
/// certificate renewed.
pub fn managed_endpoint(
  app: &AppHandle,
  label: Option<&str>,
  event: crate::core::cloud::ManagedEndpointEvent,
) {
  window::managed_endpoint(app, label, event);
}

/// An endpoint was reauthenticated: its cutoff is lifted everywhere.
pub fn cutoff_cleared(app: &AppHandle, endpoint_id: &str) {
  window::cutoff_cleared(app, endpoint_id);
}

/// Forgets a destroyed window's shell.
/// The last repo window closed, and every terminal with it.
pub fn forget_terminals() {
  screen::forget_terminals();
}

pub fn forget_window(label: &str) {
  popup::forget_owner(label);
  toasts::forget(label);
  remote_repo::set_window(label, None);
  modal::forget(label);
  window::forget(label);
  let label = label.to_string();
  tauri_plugin_gpui::with_app(move |cx| {
    if let Some(shells) = cx.try_global::<Shells>() {
      if shells.0.contains_key(&label) {
        cx.global_mut::<Shells>().0.remove(&label);
      }
    }
  })
  .ignore_closed();
}

#[cfg(test)]
mod block_on_tests {
  #[test]
  fn block_on_runs_off_the_main_thread() {
    assert_eq!(super::block_on(async { 7 }), 7);
  }

  #[cfg(debug_assertions)]
  #[test]
  fn block_on_refuses_the_main_thread() {
    let on_main = std::thread::Builder::new()
      .name("main".into())
      .spawn(|| super::block_on(async { 7 }))
      .expect("spawn")
      .join();
    assert!(on_main.is_err());
  }
}
