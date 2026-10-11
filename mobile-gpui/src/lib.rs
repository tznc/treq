//! Treq mobile, rebuilt with GPUI (via tauri-plugin-gpui and gpui-kit).
//!
//! The same remote-control and review surface as the React mobile shell
//! (see `SPEC.md`), but rendered natively. Repository work runs on the
//! connected host through treq's own SSH transport and remote command
//! protocol, linked directly from `treq_lib` instead of going through IPC.

mod auth;
mod backend;
mod config;
mod control_plane;
mod managed;
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
  let _ =
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
}

/// Forwards `treq://auth/callback` links (at launch and while running).
fn listen_for_deep_links(app: &tauri::App) {
  use tauri_plugin_deep_link::DeepLinkExt;

  let forward = |urls: Vec<url::Url>| {
    for url in urls {
      if let Some(token) = auth::callback_token(url.as_str()) {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
      let events = backend::init(app.handle().clone(), data_dir)?;
      listen_for_deep_links(app);
      tauri_plugin_gpui::init_with(
        app,
        GpuiConfig::new()
          .assets(gpui_kit::assets::Assets)
          .on_launch(|cx| {
            gpui_kit::init(cx);
            ui::theme::init(cx);
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
    .build(tauri::generate_context!())
    .expect("error while building the Treq GPUI app");
  app.run(|_, event| {
    if let (tauri::RunEvent::Resumed, Some(backend)) = (event, backend::try_get()) {
      backend.emit(backend::AppEvent::Resumed);
    }
  });
}
