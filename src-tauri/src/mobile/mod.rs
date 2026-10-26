//! Treq mobile, rebuilt with GPUI (via tauri-plugin-gpui and gpui-kit).
//!
//! The same remote-control and review surface as the React mobile shell
//! (see `SPEC.md`), but rendered natively. Repository work runs on the
//! connected host through treq's own SSH transport and remote command
//! protocol, linked directly from `treq_lib` instead of going through IPC.

mod assets;
mod backend;
mod config;
mod errors;
mod managed;
mod reads;
mod remote;
mod store;
mod terminal;
mod ui;

use gpui_kit::AppContext as _;
use tauri::Manager;
use tauri_plugin_gpui::{GpuiConfig, GpuiOptions, GpuiWindowExt};

fn init_logging() {
  #[cfg(target_os = "android")]
  android_logger::init_once(
    android_logger::Config::default()
      .with_max_level(log::LevelFilter::Info)
      .with_tag("treq-gpui"),
  );
  #[cfg(not(any(target_os = "android", target_os = "ios")))]
  {
    use crate::result_ext::ResultExt as _;
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
      .try_init()
      .best_effort();
  }
}

/// Forwards `treq://auth/callback` links (at launch and while running).
fn listen_for_deep_links(app: &tauri::App) {
  use tauri_plugin_deep_link::DeepLinkExt;

  let forward = |urls: Vec<url::Url>| {
    for url in urls {
      if let Some(token) = crate::core::auth::callback_token(url.as_str()) {
        backend::get().emit(backend::AppEvent::AuthToken(token));
      }
    }
  };
  let deep_link = app.deep_link();
  if let Ok(Some(urls)) = deep_link.get_current() {
    forward(urls);
  }
  deep_link.on_open_url(move |event| forward(event.urls()));
}

/// Runs the mobile app. The app crate passes its own `generate_context!()`,
/// so the mobile Tauri config, not this crate's, drives the build.
#[expect(
  clippy::expect_used,
  reason = "a Tauri build failure at startup is unrecoverable"
)]
pub fn run(context: tauri::Context<tauri::Wry>) {
  init_logging();
  let builder = tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_deep_link::init());
  #[cfg(mobile)]
  let builder = builder
    .plugin(tauri_plugin_keystore::init())
    .plugin(tauri_plugin_biometric::init());
  let app = builder
    .setup(|app| {
      let data_dir = app.path().app_data_dir()?;
      // The app shares `com.treq` with the desktop app, so a desktop run
      // keeps its own settings database instead of opening the real one.
      #[cfg(desktop)]
      let data_dir = data_dir.join("mobile-gpui");
      let events = backend::init(app.handle().clone(), data_dir)?;
      listen_for_deep_links(app);
      tauri_plugin_gpui::init_with(
        app,
        GpuiConfig::new().assets(assets::AppAssets).on_launch(|cx| {
          // Initializes gpui-kit too.
          ui::theme::init(cx);
          // tauri-plugin-gpui draws Android's own selection toolbar and handles.
          #[cfg(target_os = "android")]
          gpui_kit::component::input::set_native_touch_selection(true, cx);
        }),
      )?;
      let window = tauri::WindowBuilder::new(app, "main")
        .title("Treq")
        .inner_size(420., 860.)
        .build()?;
      window.attach_gpui_view(GpuiOptions::default(), move |window, cx| {
        let root = cx.new(|cx| ui::Root::new(events, window, cx));
        cx.new(|cx| gpui_kit::base::Root::new(root, window, cx))
      })?;
      log::info!("treq-gpui: attached");
      Ok(())
    })
    .build(context)
    .expect("error while building the Treq GPUI app");
  app.run(|_, event| match event {
    tauri::RunEvent::Resumed => {
      backend::set_foreground(true);
      if let Some(backend) = backend::try_get() {
        backend.emit(backend::AppEvent::Resumed);
      }
    }
    #[cfg(mobile)]
    tauri::RunEvent::WindowEvent {
      event: tauri::WindowEvent::Suspended,
      ..
    } => backend::set_foreground(false),
    #[cfg(mobile)]
    tauri::RunEvent::WindowEvent {
      event: tauri::WindowEvent::Resumed,
      ..
    } => backend::set_foreground(true),
    _ => {}
  });
}
