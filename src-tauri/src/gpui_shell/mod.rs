//! Experimental GPUI window shell (the `gpuiSidebar` feature preview).
//!
//! With the preview on, a repo window is a plain Tauri window with GPUI
//! attached as its renderer, and the React app runs in a child webview that
//! GPUI places with `webview_frame`. This is the first step of moving the
//! desktop UI to GPUI one piece at a time: GPUI owns the window, and every
//! screen that is still web UI lives in the webview.
//!
//! The workspace sidebar is the first screen drawn by GPUI (see `sidebar`).
//!
//! The child webview has the same label as its window. Tauri treats a window
//! whose only webview shares its label as a webview window, so
//! `get_webview_window`, `emit_to` and the `WebviewWindow` command extractor
//! keep working as they do for a window built with `WebviewWindowBuilder`.

use std::collections::HashMap;

use tauri::{
  AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, Webview, WebviewBuilder, WebviewUrl,
  Wry,
};
use tauri_plugin_gpui::{
  gpui::{self, prelude::*, Global, SharedString, WeakEntity},
  webview_frame, GpuiConfig, GpuiWindowExt,
};

mod sidebar;
mod theme;

pub use sidebar::SidebarSnapshot;

use crate::core::feature_preview::{self, PreviewFeature};
use crate::db::Database;

/// Set on `window` in the webview so the React app knows GPUI hosts it.
const HOSTED_FLAG_SCRIPT: &str = "window.__TREQ_GPUI_SHELL__ = true;";

/// Whether this platform can host a webview inside a GPUI window.
///
/// tauri-plugin-gpui stacks the webview above GPUI with an X11 child window
/// on Linux, so a Wayland session (unless GTK is forced onto XWayland) falls
/// back to the plain webview window.
fn platform_supported() -> bool {
  if cfg!(target_os = "linux") {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let forced_x11 = std::env::var("GDK_BACKEND").is_ok_and(|backend| backend == "x11");
    return !wayland || forced_x11;
  }
  true
}

/// Whether repo windows open in the GPUI shell for this run of the app.
/// Read once at startup: switching the preview takes effect on restart.
pub fn should_enable(db: &Database) -> bool {
  feature_preview::is_enabled(db, PreviewFeature::GpuiSidebar) && platform_supported()
}

/// Starts GPUI. Call once from `setup`, before the first shell window.
pub fn init(app: &mut tauri::App<Wry>) -> Result<(), tauri_plugin_gpui::GpuiError> {
  tauri_plugin_gpui::init_with(
    app,
    GpuiConfig::new()
      .assets(theme::Assets)
      .on_launch(theme::init),
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
/// next to the webview.
struct Shell {
  app: AppHandle,
  label: SharedString,
  webview: Option<Webview<Wry>>,
  sidebar: Option<SidebarSnapshot>,
}

impl Render for Shell {
  fn render(&mut self, window: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
    // Until React sends its theme, follow the OS.
    let dark = self.sidebar.as_ref().map_or(
      matches!(
        window.appearance(),
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
      ),
      |snapshot| snapshot.dark,
    );
    gpui::div()
      .size_full()
      .flex()
      .font_family(theme::SANS_FONT)
      // Shown only until the webview's first frame, so opening a window
      // does not flash.
      .bg(theme::palette(dark).background)
      .when_some(self.sidebar.as_ref(), |this, snapshot| {
        this.child(sidebar::render(snapshot, &self.label, &self.app))
      })
      .child(
        gpui::div()
          .flex_1()
          .min_w_0()
          .h_full()
          .when_some(self.webview.as_ref(), |this, webview| {
            this.child(webview_frame(webview))
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

  let shell_label = label.to_string();
  let shell_app = app.clone();
  window.attach_gpui(move |cx| {
    let shell = cx.new(|_| Shell {
      app: shell_app,
      label: shell_label.clone().into(),
      webview: None,
      sidebar: None,
    });
    cx.default_global::<Shells>()
      .0
      .insert(shell_label, shell.downgrade());
    shell
  })?;

  // Same label as the window; see the module docs.
  let webview = window.add_child(
    WebviewBuilder::new(label, WebviewUrl::App(url.into()))
      .initialization_script(HOSTED_FLAG_SCRIPT),
    LogicalPosition::new(0., 0.),
    LogicalSize::new(1., 1.),
  )?;
  // `webview_frame` shows it from its first frame.
  webview.hide()?;

  update_shell(label, move |shell| shell.webview = Some(webview))?;
  Ok(())
}

/// Runs `f` on the shell of window `label`, if it has one, and redraws it.
/// Must run on the main thread, outside a GPUI update.
fn update_shell(
  label: &str,
  f: impl FnOnce(&mut Shell) + 'static,
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
      f(shell);
      cx.notify();
    });
  })
}

/// Shows `snapshot` in the GPUI sidebar of window `label`, or hides the
/// sidebar for `None` (React then draws its own).
pub fn update_sidebar(app: &AppHandle, label: &str, snapshot: Option<SidebarSnapshot>) {
  let label = label.to_string();
  let _ = app.run_on_main_thread(move || {
    let result = update_shell(&label, move |shell| shell.sidebar = snapshot);
    if let Err(err) = result {
      log::warn!("failed to update the GPUI sidebar: {err}");
    }
  });
}

/// Forgets a destroyed window's shell.
pub fn forget_window(label: &str) {
  let label = label.to_string();
  let _ = tauri_plugin_gpui::with_app(move |cx| {
    if let Some(shells) = cx.try_global::<Shells>() {
      if shells.0.contains_key(&label) {
        cx.global_mut::<Shells>().0.remove(&label);
      }
    }
  });
}
