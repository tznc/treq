// The app side of dispatch (listener, responses) is unused by the headless CLI.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
mod agent_dispatch;
#[cfg(feature = "gui")]
mod agent_runtime;
pub mod auto_rebase;
pub mod auto_review;
pub mod binary_paths;
// The `treq` command-line entry point is desktop-only: it is backed by
// `tauri-plugin-cli`, which has no Android/iOS implementation.
#[cfg(desktop)]
mod cli;
#[cfg(feature = "gui")]
mod commands;
mod commit_timestamps;
pub mod conflict_markers;
pub mod core;
pub mod db;
pub mod file_indexer;
pub mod github;
#[cfg(feature = "gpui-shell")]
#[cfg(desktop)]
mod gpui_shell;
pub mod jj;
#[cfg(feature = "gui")]
pub mod linear;
pub mod local_db;
pub mod lock_ext;
pub mod notify_dispatch;
#[cfg(feature = "gui")]
pub(crate) mod open_new_window;
#[cfg(feature = "gui")]
pub mod pr_status;
pub mod pty;
pub mod repo_config;
pub mod review_aggregate;
pub mod send_dispatch;
pub mod telemetry;
pub mod tracker;
pub mod unified_patch;

#[cfg(feature = "gui")]
use agent_runtime::{
  parse_agent_request_from_url, route_agent_deep_link, route_agent_dispatch_request,
  start_agent_ipc_listener, start_instance_registry_heartbeat,
};
#[cfg(feature = "gui")]
use commands::file_watcher::WatcherManager;
#[cfg(feature = "gui")]
use db::Database;
#[cfg(feature = "gui")]
use pty::PtyManager;
#[cfg(feature = "gui")]
use std::collections::HashMap;
#[cfg(feature = "gui")]
use std::sync::Mutex;
#[cfg(feature = "gui")]
#[cfg(desktop)]
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
#[cfg(feature = "gui")]
use tauri::{AppHandle, Emitter, EventTarget, Manager};
#[cfg(feature = "gui")]
use tauri_plugin_log::{Target, TargetKind};

#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
extern crate self as treq_lib;

#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
mod e2e_test_helpers;

#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
mod tauri_test_bridge;

#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
pub(crate) mod test_pty_events;

#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use crate::commands::CloudAction;
#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use crate::commands::CutoffReasonDto;
#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use crate::core::remote::TreqCommandRequest;
#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use crate::core::remote_control_plane::SshEndpoint;
#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use crate::core::remote_pty::PtyLaunchSpec;
#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use crate::core::skills::SkillInstallScope;
#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use commands::RemoteExecState;
#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
use commands::RemotePtyState;

#[cfg(feature = "gui")]
#[cfg(feature = "tauri-test")]
#[tauri_test::setup(init = tauri_test_bridge::init_test_state)]
pub struct TauriTestApp;

#[cfg(feature = "gui")]
pub(crate) struct AppState {
  db: Mutex<Database>,
  pty_manager: PtyManager,
  watcher_manager: WatcherManager,
  window_repo_paths: Mutex<HashMap<String, String>>,
  window_last_focused_at: Mutex<HashMap<String, u64>>,
  /// Supporting repository paths shown in each window, by window label.
  window_supporting_repo_paths: Mutex<HashMap<String, Vec<String>>>,
  dispatch_instance_id: String,
  dispatch_started_at: u64,
  dispatch_endpoint: String,
  // Held for its Drop guards (file writer + provider shutdown).
  telemetry: telemetry::TelemetryGuards,
}

#[cfg(feature = "gui")]
impl AppState {
  pub(crate) fn new(
    db: Database,
    pty_manager: PtyManager,
    watcher_manager: WatcherManager,
    dispatch_instance_id: String,
    dispatch_started_at: u64,
    dispatch_endpoint: String,
    telemetry: telemetry::TelemetryGuards,
  ) -> Self {
    Self {
      db: Mutex::new(db),
      pty_manager,
      watcher_manager,
      window_repo_paths: Mutex::new(HashMap::new()),
      window_last_focused_at: Mutex::new(HashMap::new()),
      window_supporting_repo_paths: Mutex::new(HashMap::new()),
      dispatch_instance_id,
      dispatch_started_at,
      dispatch_endpoint,
      telemetry,
    }
  }
}

/// Picks which webview a menu event targets, following the same rule as
/// `emit_to_focused`: the currently focused window, falling back to the
/// most recently focused one that still exists.
#[cfg(feature = "gui")]
fn resolve_menu_target_label(app: &AppHandle) -> Option<String> {
  let windows = app.webview_windows();
  let focused: Vec<String> = windows
    .iter()
    .filter_map(|(label, window)| {
      if window.is_focused().unwrap_or(false) {
        Some(label.clone())
      } else {
        None
      }
    })
    .collect();
  let existing: Vec<String> = windows.keys().cloned().collect();
  let last_focused = app
    .try_state::<AppState>()
    .and_then(|state| state.window_last_focused_at.lock().ok().map(|g| g.clone()))
    .unwrap_or_default();
  open_new_window::resolve_menu_event_window_label(&focused, &last_focused, &existing)
}

/// Hands a menu event for window `label` to its GPUI window model; false
/// when React handles it.
#[cfg(feature = "gui")]
fn gpui_menu_event(app: &AppHandle, label: &str, event: &str, payload: Option<String>) -> bool {
  #[cfg(feature = "gpui-shell")]
  #[cfg(desktop)]
  if gpui_shell::is_enabled(app) {
    return gpui_shell::menu_event(app, label, event, payload);
  }
  let _ = (app, label, event, payload);
  false
}

/// A menu event for the focused window: its GPUI window model's, else
/// React's (`emit_to_focused`).
#[cfg(feature = "gui")]
fn menu_to_focused(app: &AppHandle, event: &str) {
  let Some(label) = resolve_menu_target_label(app) else {
    return;
  };
  if !gpui_menu_event(app, &label, event, None) {
    let _ = app.emit_to(EventTarget::webview_window(&label), event, ());
  }
}

/// Emits an event only to the focused webview window.
///
/// On macOS the menu bar unfocuses every window, so this never broadcasts
/// globally — a broadcast would run folder pickers in every open webview.
#[cfg(feature = "gui")]
pub fn emit_to_focused<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
  let Some(label) = resolve_menu_target_label(app) else {
    return;
  };
  let _ = app.emit_to(EventTarget::webview_window(&label), event, payload);
}

/// What a `treq://` deep link asks the app to do.
#[cfg(feature = "gui")]
#[cfg(desktop)]
#[derive(Debug, PartialEq, Eq)]
enum DeepLinkKind {
  /// `treq://agent/start?...`: start an agent in the window open on a repo.
  Agent,
  /// `treq://auth/callback?token=...`: finish browser sign-in.
  AuthCallback,
  Unknown,
}

/// Sorts a deep link by what it asks for. An auth callback is matched by the
/// same rule as `authCallbackToken` in `src/lib/auth-deep-link.ts`.
#[cfg(feature = "gui")]
#[cfg(desktop)]
fn classify_deep_link(url: &str) -> DeepLinkKind {
  if url.starts_with("treq://agent/start?") {
    return DeepLinkKind::Agent;
  }
  match url::Url::parse(url) {
    Ok(parsed)
      if parsed.scheme() == "treq"
        && parsed.host_str() == Some("auth")
        && parsed.path() == "/callback" =>
    {
      DeepLinkKind::AuthCallback
    }
    _ => DeepLinkKind::Unknown,
  }
}

/// Brings the window a menu event would target to the front.
#[cfg(feature = "gui")]
#[cfg(desktop)]
fn focus_target_window(app: &AppHandle) {
  let Some(window) = resolve_menu_target_label(app).and_then(|l| app.get_webview_window(&l)) else {
    return;
  };
  let _ = window.unminimize();
  let _ = window.show();
  let _ = window.set_focus();
}

/// Handles the regular "Open..." menu action (CmdOrCtrl+O).
///
/// Guarded the same way as "Open in New Window": a macOS File-menu
/// redelivery must not be able to stack a second directory picker on top of
/// one already showing. The target webview is resolved and retained before
/// the picker steals window focus, and the picker itself is shown after
/// leaving the AppKit menu-tracking loop.
#[cfg(feature = "gui")]
#[cfg(desktop)]
fn schedule_open_repo(app: AppHandle) {
  if !open_new_window::OPEN_REPO_GATE.try_begin() {
    return;
  }
  let Some(label) = resolve_menu_target_label(&app) else {
    open_new_window::OPEN_REPO_GATE.end();
    return;
  };
  std::thread::spawn(move || {
    let app_for_main = app.clone();
    if app
      .run_on_main_thread(move || {
        pick_folder_and_notify(app_for_main, label);
      })
      .is_err()
    {
      open_new_window::OPEN_REPO_GATE.end();
    }
  });
}

#[cfg(feature = "gui")]
#[cfg(desktop)]
fn pick_folder_and_notify(app: AppHandle, label: String) {
  use tauri_plugin_dialog::DialogExt;

  app
    .dialog()
    .file()
    .set_title("Select Folder")
    .pick_folder(move |folder| {
      let Some(file_path) = folder else {
        open_new_window::OPEN_REPO_GATE.end();
        return;
      };
      let path = match file_path.into_path() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => {
          open_new_window::OPEN_REPO_GATE.end();
          return;
        }
      };
      if !gpui_menu_event(
        &app,
        &label,
        "menu-repository-path-selected",
        Some(path.clone()),
      ) {
        let _ = app.emit_to(
          EventTarget::webview_window(&label),
          "menu-repository-path-selected",
          path,
        );
      }
      open_new_window::OPEN_REPO_GATE.end();
    });
}

#[cfg(feature = "gui")]
#[cfg(desktop)]
fn schedule_open_repo_in_new_window(app: AppHandle) {
  if !open_new_window::OPEN_NEW_WINDOW_GATE.try_begin() {
    return;
  }
  // Leave the AppKit menu-tracking run loop before showing NSOpenPanel.
  // Showing the panel nested in the menu action redelivers the same item
  // when the new window becomes key, which retriggers the folder picker.
  std::thread::spawn(move || {
    let app_for_main = app.clone();
    if app
      .run_on_main_thread(move || {
        pick_folder_and_open_new_window(app_for_main);
      })
      .is_err()
    {
      open_new_window::OPEN_NEW_WINDOW_GATE.end();
    }
  });
}

#[cfg(feature = "gui")]
#[cfg(desktop)]
fn pick_folder_and_open_new_window(app: AppHandle) {
  use tauri_plugin_dialog::DialogExt;

  app
    .dialog()
    .file()
    .set_title("Select Folder")
    .pick_folder(move |folder| {
      let Some(file_path) = folder else {
        open_new_window::OPEN_NEW_WINDOW_GATE.end();
        return;
      };
      let path = match file_path.into_path() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => {
          open_new_window::OPEN_NEW_WINDOW_GATE.end();
          return;
        }
      };
      let app_for_init = app.clone();
      tauri::async_runtime::spawn(async move {
        let path_for_init = path.clone();
        let init_ok = tauri::async_runtime::spawn_blocking(move || core::init(&path_for_init))
          .await
          .ok()
          .and_then(Result::ok)
          .unwrap_or(false);
        if !init_ok {
          menu_to_focused(&app_for_init, "menu-open-in-new-window-invalid");
          open_new_window::OPEN_NEW_WINDOW_GATE.end();
          return;
        }
        let label = open_new_window::new_repo_window_label();
        let title = open_new_window::new_repo_window_title(&path);
        let app_for_window = app_for_init.clone();
        // The GPUI shell builds windows on the main thread.
        let _ = app_for_init.run_on_main_thread(move || {
          if let Err(err) = open_repo_window(&app_for_window, &label, Some(path), &title) {
            log::error!("failed to open repo window: {err}");
          }
        });
        open_new_window::OPEN_NEW_WINDOW_GATE.end();
      });
    });
}

/// Opens a window on repository `repo` (`None`: the start screen): inside
/// the GPUI shell when it is on (see `gpui_shell`), else as a plain
/// webview window showing the React app.
#[cfg(feature = "gui")]
fn open_repo_window(
  app: &AppHandle,
  label: &str,
  repo: Option<String>,
  title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(feature = "gpui-shell")]
  #[cfg(desktop)]
  if gpui_shell::is_enabled(app) {
    return gpui_shell::open_window(app, label, repo, title);
  }
  let url = open_new_window::window_url(repo.as_deref());
  tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App(url.into()))
    .title(title)
    .inner_size(1400.0, 900.0)
    .build()?;
  Ok(())
}

/// Web page a menu item opens in the browser, on every desktop platform.
/// Menu items handled inside the app return `None`.
#[cfg(feature = "gui")]
#[cfg(desktop)]
fn menu_item_url(menu_id: &str) -> Option<&'static str> {
  match menu_id {
    "learn_more" => Some("https://treq.dev/docs"),
    "discussions" => Some("https://github.com/tznc/treq/discussions"),
    _ => None,
  }
}

#[cfg(feature = "gui")]
#[cfg(desktop)]
fn is_cli_process<I, S>(args: I) -> bool
where
  I: IntoIterator<Item = S>,
  S: AsRef<std::ffi::OsStr>,
{
  let Some(first_arg) = args.into_iter().nth(1) else {
    return false;
  };
  let first_arg = first_arg.as_ref().to_string_lossy();

  !first_arg.starts_with("treq://") && !first_arg.starts_with("-psn_")
}

#[cfg(feature = "gui")]
#[cfg_attr(all(mobile, not(feature = "embed")), tauri::mobile_entry_point)]
pub fn run() {
  telemetry::install_panic_hook();
  // CLI invocations never start Tauri: see `cli::args` for why.
  #[cfg(desktop)]
  if is_cli_process(std::env::args_os()) {
    std::process::exit(cli::args::run(std::env::args_os()));
  }
  #[cfg(feature = "gpui-shell")]
  #[cfg(desktop)]
  gpui_shell::force_x11_on_wayland();
  let builder = tauri::Builder::default();
  // Must be the first plugin. On Windows and Linux a `treq://` link starts a
  // second process; this plugin makes it exit and hands its arguments to the
  // running app. The `deep-link` feature passes the link on to `on_open_url`
  // below, so the callback only brings the app to the front.
  #[cfg(desktop)]
  let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
    focus_target_window(app);
  }));
  let builder = {
    let builder = builder
      .plugin(
        tauri_plugin_log::Builder::new()
          .level(tauri_plugin_log::log::LevelFilter::Info)
          .target(Target::new(TargetKind::Dispatch(
            tauri_plugin_log::fern::Dispatch::new().chain(tauri_plugin_log::fern::Output::call(
              telemetry::forward_log_record,
            )),
          )))
          .build(),
      )
      .plugin(tauri_plugin_opener::init())
      .plugin(tauri_plugin_dialog::init())
      .plugin(tauri_plugin_deep_link::init())
      .plugin(tauri_plugin_notification::init());
    // Device-key storage for the mobile connectivity flow (mobile PRD,
    // "Security and key custody") - both plugins are `#[cfg(mobile)]`-gated
    // upstream and have no desktop implementation worth shipping, see
    // `core::remote_device_key`.
    #[cfg(mobile)]
    let builder = builder
      .plugin(tauri_plugin_keystore::init())
      .plugin(tauri_plugin_biometric::init());
    builder
  };
  #[cfg(desktop)]
  let builder = builder.plugin(tauri_plugin_cli::init());
  builder
    .on_window_event(|window, event| {
      // A shell window losing focus closes its tooltips and menus, as
      // native ones would; its toasts follow it as it moves or resizes.
      #[cfg(feature = "gpui-shell")]
      #[cfg(desktop)]
      if !window.label().starts_with("gpui-popup-") && gpui_shell::is_enabled(window.app_handle()) {
        match event {
          tauri::WindowEvent::Focused(false) => gpui_shell::hide_popups(window.app_handle()),
          tauri::WindowEvent::Focused(true) => {
            commands::cloud::window_focused(window.app_handle());
            gpui_shell::window_focused(window.app_handle(), window.label());
          }
          tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
            gpui_shell::place_toasts(window.app_handle(), window.label())
          }
          _ => {}
        }
      }
      // Fires once, after the window is actually gone (not on a
      // cancellable close request), for every window this app opens —
      // "main" and any later "Open in New Window" targets alike — so
      // this is the one place that reliably closes a window's own
      // terminals without touching another window's, regardless of
      // which UI path (dashboard unmount, native "Close Window", the
      // OS window-close button) tore it down.
      if let tauri::WindowEvent::Destroyed = event {
        let label = window.label().to_string();
        #[cfg(feature = "gpui-shell")]
        #[cfg(desktop)]
        gpui_shell::forget_window(&label);
        let app_handle = window.app_handle().clone();
        if let Some(state) = app_handle.try_state::<AppState>() {
          state.pty_manager.close_all_for_window(&label);
        }
        if let Some(state) = app_handle.try_state::<commands::remote_pty_commands::RemotePtyState>()
        {
          tauri::async_runtime::block_on(state.0.close_all_for_window(&label));
        }
      }
    })
    .setup(move |app| {
      let log_dir = app.path().app_log_dir().expect("Failed to get app log dir");
      let telemetry = telemetry::init(&log_dir).expect("Failed to initialize telemetry");

      let app_dir = app
        .path()
        .app_data_dir()
        .expect("Failed to get app data dir");
      std::fs::create_dir_all(&app_dir).expect("Failed to create app data directory");
      std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_string_lossy().to_string());
      let db_path = app_dir.join(crate::core::APP_DB_FILE_NAME);

      let db = Database::new(db_path).expect("Failed to open database");
      db.init().expect("Failed to initialize database");

      // The saved repo opens with the window (avoids Onboarding flash).
      let saved_repo_path = db.get_setting("last_opened_repo_path").ok().flatten();

      // Load cached binary paths and initialize in-memory cache
      let binary_paths = commands::load_cached_binary_paths(&db);
      binary_paths::init_binary_paths_cache(binary_paths);

      // Load cached editor apps and initialize in-memory cache
      let editor_apps = commands::load_cached_editor_apps(&db);
      binary_paths::init_editor_apps_cache(editor_apps);

      let pty_manager = PtyManager::new();

      // Initialize file watcher
      let watcher_manager = WatcherManager::new();
      watcher_manager.set_app_handle(app.handle().clone());

      // Background PR-status poller (sidebar reads from its cache)
      crate::pr_status::set_app_handle(app.handle().clone());

      // Automatic reviews: jj operations signal, the frontend launches
      // the review terminal.
      {
        use tauri::Emitter;
        let review_app = app.handle().clone();
        crate::auto_review::set_emitter(Box::new(move |event| {
          // Shell windows start the review from Rust.
          #[cfg(feature = "gpui-shell")]
          #[cfg(desktop)]
          if gpui_shell::is_enabled(&review_app) {
            gpui_shell::launch::auto_review(&review_app, event);
            return;
          }
          let _ = review_app.emit("auto-review-triggered", event);
        }));
      }

      let (dispatch_listener, dispatch_endpoint) = agent_dispatch::bind_ephemeral_listener()?;
      let dispatch_instance_id = uuid::Uuid::new_v4().to_string();
      let dispatch_started_at = agent_dispatch::now_millis();
      let app_state = AppState::new(
        db,
        pty_manager,
        watcher_manager,
        dispatch_instance_id,
        dispatch_started_at,
        dispatch_endpoint,
        telemetry,
      );

      app.manage(app_state);
      #[cfg(not(mobile))]
      let remote_exec_state = commands::remote_control::RemoteExecState::default();
      // Mobile authenticates with the device key held in the OS
      // keystore rather than a file under `~/.ssh`, so its pool gets a
      // loader for that reserved key reference.
      #[cfg(mobile)]
      let remote_exec_state = commands::remote_control::RemoteExecState(std::sync::Arc::new(
        crate::core::remote_ssh_transport::SshConnectionPool::new().with_device_key_provider(
          crate::core::remote_device_key::device_key_provider(app.handle().clone()),
        ),
      ));
      app.manage(commands::remote_pty_commands::RemotePtyState::new(
        &remote_exec_state,
      ));
      app.manage(remote_exec_state);
      // Only now: a shell window's model reads the database and the binary
      // caches as soon as the window is made.
      #[cfg(feature = "gpui-shell")]
      #[cfg(desktop)]
      if let Err(err) = gpui_shell::init(app) {
        log::error!("GPUI shell unavailable, using the webview window: {err}");
      }
      open_repo_window(
        app.handle(),
        "main",
        saved_repo_path.clone(),
        "Treq - Stacking ADE",
      )?;
      // GPUI shell windows draw the account from Rust, which owns the
      // Supabase session; without the shell the webview keeps it.
      #[cfg(feature = "gpui-shell")]
      #[cfg(desktop)]
      if gpui_shell::is_enabled(app.handle()) {
        commands::cloud::start(app.handle());
      }
      start_agent_ipc_listener(app.handle().clone(), dispatch_listener);
      start_instance_registry_heartbeat(app.handle().clone());
      crate::core::app_setup_script::spawn_run_if_needed(
        crate::core::app_setup_script::Trigger::Startup,
      );

      // Listen for deep-link events and forward to frontend
      #[cfg(desktop)]
      {
        use tauri_plugin_deep_link::DeepLinkExt;
        // Installers register `treq://` with the OS. An AppImage or a dev
        // build is not installed, so it registers itself at startup.
        #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
        if let Err(err) = app.deep_link().register_all() {
          log::warn!("failed to register deep link schemes: {err}");
        }
        let handle = app.handle().clone();
        app.deep_link().on_open_url(move |event| {
          let urls: Vec<String> = event.urls().into_iter().map(|u| u.to_string()).collect();
          for url in urls {
            match classify_deep_link(&url) {
              // One window only: the sign-in token can be exchanged once.
              DeepLinkKind::AuthCallback => {
                if !commands::cloud::handle_auth_callback(&handle, &url) {
                  emit_to_focused(&handle, "deep-link-received", vec![url]);
                }
              }
              DeepLinkKind::Agent => {
                if let Some(request) = parse_agent_request_from_url(&url) {
                  let response = route_agent_dispatch_request(&handle, &request);
                  if response.status != "handled"
                    && !agent_runtime::open_window_for_agent_link(&handle, &url)
                  {
                    log::info!(
                      "agent deep link unmatched repo={} request_id={}",
                      request.repo,
                      request.request_id
                    );
                  }
                } else if !route_agent_deep_link(&handle, url) {
                  log::info!("agent deep link ignored (no matching window/repo)");
                }
              }
              DeepLinkKind::Unknown => log::info!("deep link ignored (unrecognized url)"),
            }
          }
        });
      }

      // Create menu
      //
      // Native menus (tauri::menu) have no mobile implementation; gate the
      // whole block so mobile targets (Android/iOS) build.
      #[cfg(desktop)]
      {
        #[cfg(target_os = "macos")]
        {
          use tauri::menu::PredefinedMenuItem;

          // App menu (automatically gets app name on macOS)
          let app_menu = SubmenuBuilder::new(app, "App")
            .item(&PredefinedMenuItem::hide(app, None)?)
            .item(&PredefinedMenuItem::hide_others(app, None)?)
            .item(&PredefinedMenuItem::show_all(app, None)?)
            .separator()
            .item(&PredefinedMenuItem::quit(app, None)?)
            .build()?;

          // File menu items
          let open_item = MenuItemBuilder::with_id("open", "Open...")
            .accelerator("CmdOrCtrl+O")
            .build(app)?;

          let open_new_window_item =
            MenuItemBuilder::with_id("open_new_window", "Open in New Window...")
              .accelerator("CmdOrCtrl+Shift+O")
              .build(app)?;

          let open_ssh_item = MenuItemBuilder::with_id("open_ssh", "Open via SSH...")
            .accelerator("CmdOrCtrl+Alt+O")
            .build(app)?;

          let add_repository_item =
            MenuItemBuilder::with_id("add_repository", "Add Repository...").build(app)?;

          let file_menu = SubmenuBuilder::new(app, "File")
            .item(&open_item)
            .item(&open_new_window_item)
            .item(&open_ssh_item)
            .separator()
            .item(&add_repository_item)
            .build()?;

          // Edit menu with native shortcuts
          let edit_menu = SubmenuBuilder::new(app, "Edit")
            .item(&PredefinedMenuItem::undo(app, None)?)
            .item(&PredefinedMenuItem::redo(app, None)?)
            .separator()
            .item(&PredefinedMenuItem::cut(app, None)?)
            .item(&PredefinedMenuItem::copy(app, None)?)
            .item(&PredefinedMenuItem::paste(app, None)?)
            .item(&PredefinedMenuItem::select_all(app, None)?)
            .build()?;

          // View menu
          let view_menu = SubmenuBuilder::new(app, "View")
            .item(&PredefinedMenuItem::fullscreen(app, None)?)
            .build()?;

          // Go menu items
          let dashboard_item = MenuItemBuilder::with_id("dashboard", "Dashboard")
            .accelerator("CmdOrCtrl+D")
            .build(app)?;

          let settings_item = MenuItemBuilder::with_id("settings", "Settings")
            .accelerator("CmdOrCtrl+,")
            .build(app)?;

          let go_menu = SubmenuBuilder::new(app, "Go")
            .item(&dashboard_item)
            .item(&settings_item)
            .build()?;

          // Developer menu (only in debug mode)
          #[cfg(debug_assertions)]
          let developer_menu = {
            let open_web_inspector =
              MenuItemBuilder::with_id("open_web_inspector", "Open Web Inspector")
                .accelerator("CmdOrCtrl+Shift+I")
                .build(app)?;

            let force_rebase_item =
              MenuItemBuilder::with_id("force_rebase_workspace", "Force Rebase Workspace")
                .accelerator("CmdOrCtrl+Shift+R")
                .build(app)?;

            let factory_reset_item =
              MenuItemBuilder::with_id("factory_reset", "Factory Reset").build(app)?;

            SubmenuBuilder::new(app, "Developer")
              .item(&open_web_inspector)
              .separator()
              .item(&force_rebase_item)
              .separator()
              .item(&factory_reset_item)
              .build()?
          };

          // Window menu
          //
          // Close Window intentionally has no keyboard accelerator: the OS-default
          // Cmd+W is repurposed in the frontend to close the selected terminal (or
          // do nothing) instead of closing the whole treq window. The menu item is
          // still clickable and closes the focused window via the handler below.
          let close_window_item =
            MenuItemBuilder::with_id("close_window", "Close Window").build(app)?;

          let window_menu = SubmenuBuilder::new(app, "Window")
            .item(&PredefinedMenuItem::minimize(app, None)?)
            .item(&PredefinedMenuItem::maximize(app, None)?)
            .separator()
            .item(&close_window_item)
            .build()?;

          // Help menu
          let view_logs_item = MenuItemBuilder::with_id("view_logs", "View Logs").build(app)?;

          let learn_more_item = MenuItemBuilder::with_id("learn_more", "Learn More").build(app)?;
          let discussions_item =
            MenuItemBuilder::with_id("discussions", "Community Discussions").build(app)?;

          let help_menu = SubmenuBuilder::new(app, "Help")
            .item(&view_logs_item)
            .separator()
            .item(&learn_more_item)
            .item(&discussions_item)
            .build()?;

          let menu_builder = MenuBuilder::new(app)
            .item(&app_menu)
            .item(&file_menu)
            .item(&edit_menu)
            .item(&view_menu)
            .item(&go_menu);

          // Add Developer menu in debug mode
          #[cfg(debug_assertions)]
          let menu_builder = menu_builder.item(&developer_menu);

          let menu = menu_builder.item(&window_menu).item(&help_menu).build()?;

          app.set_menu(menu)?;
        }

        #[cfg(not(target_os = "macos"))]
        {
          // File menu items
          let open_item = MenuItemBuilder::with_id("open", "Open...")
            .accelerator("CmdOrCtrl+O")
            .build(app)?;

          let open_new_window_item =
            MenuItemBuilder::with_id("open_new_window", "Open in New Window...")
              .accelerator("CmdOrCtrl+Shift+O")
              .build(app)?;

          let open_ssh_item = MenuItemBuilder::with_id("open_ssh", "Open via SSH...")
            .accelerator("CmdOrCtrl+Alt+O")
            .build(app)?;

          let add_repository_item =
            MenuItemBuilder::with_id("add_repository", "Add Repository...").build(app)?;

          let file_menu = SubmenuBuilder::new(app, "File")
            .item(&open_item)
            .item(&open_new_window_item)
            .item(&open_ssh_item)
            .separator()
            .item(&add_repository_item)
            .build()?;

          // Go menu items
          let dashboard_item = MenuItemBuilder::with_id("dashboard", "Dashboard")
            .accelerator("CmdOrCtrl+D")
            .build(app)?;

          let settings_item = MenuItemBuilder::with_id("settings", "Settings")
            .accelerator("CmdOrCtrl+,")
            .build(app)?;

          let go_menu = SubmenuBuilder::new(app, "Go")
            .item(&dashboard_item)
            .item(&settings_item)
            .build()?;

          // Developer menu (only in debug mode)
          #[cfg(debug_assertions)]
          let developer_menu = {
            let open_web_inspector =
              MenuItemBuilder::with_id("open_web_inspector", "Open Web Inspector")
                .accelerator("CmdOrCtrl+Shift+I")
                .build(app)?;

            let force_rebase_item =
              MenuItemBuilder::with_id("force_rebase_workspace", "Force Rebase Workspace")
                .accelerator("CmdOrCtrl+Shift+R")
                .build(app)?;

            let factory_reset_item =
              MenuItemBuilder::with_id("factory_reset", "Factory Reset").build(app)?;

            SubmenuBuilder::new(app, "Developer")
              .item(&open_web_inspector)
              .separator()
              .item(&force_rebase_item)
              .separator()
              .item(&factory_reset_item)
              .build()?
          };

          // Help menu
          let view_logs_item = MenuItemBuilder::with_id("view_logs", "View Logs").build(app)?;
          let learn_more_item = MenuItemBuilder::with_id("learn_more", "Learn More").build(app)?;
          let discussions_item =
            MenuItemBuilder::with_id("discussions", "Community Discussions").build(app)?;
          let help_menu = SubmenuBuilder::new(app, "Help")
            .item(&view_logs_item)
            .separator()
            .item(&learn_more_item)
            .item(&discussions_item)
            .build()?;

          let mut menu_builder = MenuBuilder::new(app).item(&file_menu).item(&go_menu);

          // Add Developer menu in debug mode
          #[cfg(debug_assertions)]
          {
            menu_builder = menu_builder.item(&developer_menu);
          }

          let menu = menu_builder.item(&help_menu).build()?;

          app.set_menu(menu)?;
        }

        // Handle menu events - emit only to focused window
        app.on_menu_event(move |app, event| match event.id().as_ref() {
          "dashboard" => menu_to_focused(app, "navigate-to-dashboard"),
          "settings" => menu_to_focused(app, "navigate-to-settings"),
          "open" => schedule_open_repo(app.clone()),
          "open_new_window" => schedule_open_repo_in_new_window(app.clone()),
          "open_ssh" => menu_to_focused(app, "menu-open-ssh"),
          "add_repository" => menu_to_focused(app, "menu-add-repository"),
          "open_web_inspector" =>
          {
            #[cfg(debug_assertions)]
            if let Some(w) = app.get_webview_window("main") {
              w.open_devtools();
            }
          }
          "close_window" => {
            for (_, window) in app.webview_windows() {
              if window.is_focused().unwrap_or(false) {
                let _ = window.close();
                break;
              }
            }
          }
          "force_rebase_workspace" => menu_to_focused(app, "menu-force-rebase-workspace"),
          "factory_reset" => menu_to_focused(app, "menu-factory-reset"),
          "view_logs" => menu_to_focused(app, "menu-view-logs"),
          id => {
            if let Some(url) = menu_item_url(id) {
              use tauri_plugin_opener::OpenerExt;
              let _ = app.opener().open_url(url, None::<&str>);
            }
          }
        });
      } // #[cfg(desktop)]

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      commands::gpui_sidebar_update,
      commands::gpui_screen_update,
      commands::gpui_modal_update,
      commands::gpui_overlay_update,
      commands::gpui_menu_show,
      commands::gpui_menu_hide,
      commands::gpui_toasts_update,
      commands::gpui_toast_action,
      commands::acknowledge_agent_dispatch,
      commands::detect_editor_apps,
      commands::get_treq_bin_dir,
      commands::check_prerequisites,
      commands::get_workspaces,
      commands::create_workspace,
      commands::open_or_create_workspace_from_pr,
      commands::delete_workspace,
      commands::archive_workspace,
      commands::get_repo_default_branch,
      commands::push_workspace_to_remote,
      commands::pull_workspace_from_remote,
      commands::merge_workspace,
      commands::get_merge_landing_plan,
      commands::get_merge_preview_diff,
      commands::move_workspace_changes,
      commands::abandon_commit,
      commands::undo_commit,
      commands::revert_commit,
      commands::rename_workspace,
      commands::list_workspace_statuses,
      commands::get_workspace_status,
      commands::update_workspace,
      commands::set_workspace_target_branch,
      commands::check_and_rebase_workspaces,
      commands::ensure_workspace_indexed,
      commands::get_setting,
      commands::get_settings_batch,
      commands::set_setting,
      commands::get_app_setup_script_status,
      commands::save_app_setup_script,
      commands::run_app_setup_script,
      commands::get_app_setup_logs,
      commands::get_system_logs,
      commands::open_system_log_dir,
      commands::get_repo_setting,
      commands::set_repo_setting,
      commands::list_skill_catalog,
      commands::list_installed_skills,
      commands::install_skill,
      commands::uninstall_skill,
      commands::set_skill_install_scope,
      commands::get_workspace_file_hunks,
      commands::get_workspace_file_hunks_batch,
      commands::get_workspace_file_lines,
      commands::jj_restore_file,
      commands::jj_restore_all,
      commands::jj_snapshot_working_copy,
      commands::jj_restore_snapshot,
      commands::undo_repo_operation,
      commands::create_commit,
      commands::list_commits,
      commands::jj_split,
      commands::get_repo_current_branch,
      commands::get_workspace_changed_files,
      commands::set_git_submodule_synced,
      commands::init_repo,
      commands::jj_git_fetch_background,
      commands::jj_get_commits_ahead,
      commands::get_workspace_diff,
      commands::get_commit_diff,
      commands::get_commit_file_diff,
      commands::jj_check_branch_exists,
      commands::list_repo_branches,
      commands::switch_repo_branch,
      commands::get_commit_description,
      commands::describe_commit,
      commands::shift_commit_timestamp,
      commands::shift_mutable_commits_to_now,
      commands::start_resolve_conflicts,
      commands::build_resolve_agent_prompt,
      commands::resolve_commit,
      commands::pty_create_session,
      commands::devcontainer_status,
      commands::devcontainer_up,
      commands::pty_session_exists,
      commands::pty_write,
      commands::pty_write_suppress_echo,
      commands::pty_resize,
      commands::pty_close,
      commands::remote_pty_create,
      commands::remote_pty_write,
      commands::remote_pty_resize,
      commands::remote_pty_close,
      commands::remote_pty_session_exists,
      commands::remote_pty_list_persistent_sessions,
      commands::remote_pty_reattach,
      commands::read_file,
      commands::write_send_review_image,
      commands::write_agent_cli_files,
      commands::get_file_modified_at,
      commands::list_directory,
      commands::list_directories_batch,
      commands::ls_workspace_with_status,
      commands::list_gitignored_path_suggestions,
      commands::get_workspace_readme,
      commands::search_workspace_files,
      commands::create_session,
      commands::get_sessions,
      commands::update_session_access,
      commands::get_session_model,
      commands::add_prompt_history,
      commands::get_prompt_history,
      commands::get_workspace_starting_prompt,
      commands::stash_workspace_changes,
      commands::stash_commit,
      commands::list_stashes,
      commands::delete_stash,
      commands::apply_stash,
      commands::get_stash_diff,
      commands::export_stash_git_patch,
      commands::mark_file_viewed,
      commands::unmark_file_viewed,
      commands::start_file_watcher,
      commands::stop_file_watcher,
      commands::load_pending_review,
      commands::save_pending_review,
      commands::clear_pending_review,
      commands::list_agent_review_comments,
      commands::create_review_draft,
      commands::update_agent_review_comment,
      commands::resolve_agent_review_comment,
      commands::delete_agent_review_comment,
      commands::apply_agent_review_suggestion,
      commands::load_file_browser_review,
      commands::save_file_browser_review,
      commands::clear_file_browser_review,
      commands::open_browser_webview,
      commands::navigate_browser_webview,
      commands::close_browser_webview,
      commands::sync_browser_webview_bounds,
      commands::list_ssh_hosts,
      commands::list_local_ssh_identities,
      commands::read_local_ssh_public_key,
      commands::resolve_ssh_config_alias,
      commands::probe_ssh_host_key,
      commands::remote_dispatch_local,
      commands::remote_dispatch_over_ssh,
      commands::remote_dispatch_mutation_over_ssh,
      commands::remote_force_cutoff,
      commands::remote_clear_cutoff,
      commands::remote_cut_off_managed,
      commands::cloud_account,
      commands::cloud_sign_out,
      commands::cloud_exchange_token,
      commands::cloud_refresh_subscription,
      commands::cloud_workspace_action,
      commands::remote_set_relay_access_token,
      commands::set_window_repo_path,
      commands::set_window_supporting_repo_paths,
      commands::list_supporting_repos,
      commands::add_supporting_repo,
      commands::remove_supporting_repo,
      commands::rebase_home_repo_branch,
      commands::dry_run_home_repo_rebase,
      commands::get_git_remote_url,
      commands::get_pr_info_via_gh,
      commands::start_pr_status_polling,
      commands::stop_pr_status_polling,
      commands::list_cached_pr_statuses,
      commands::get_cached_pr_info,
      commands::list_cached_pr_ci_statuses,
      commands::get_cached_pr_ci_status,
      commands::refresh_pr_statuses,
      commands::refresh_pr_branch_status,
      commands::get_pr_checks_via_gh,
      commands::get_pr_checks_for_pr,
      commands::gh_list_issues,
      commands::gh_view_issue,
      commands::github_open_or_create_workspace_from_issue,
      commands::gh_create_issue,
      commands::gh_create_issue_comment,
      commands::gh_close_issue,
      commands::gh_reopen_issue,
      commands::gh_edit_issue,
      commands::gh_delete_issue,
      commands::gh_list_prs,
      commands::gh_view_pr,
      commands::gh_create_pr_comment,
      commands::gh_close_pr,
      commands::gh_repo_merge_settings,
      commands::merge_workspace_on_github,
      commands::sync_workspace_after_remote_merge,
      commands::gh_reopen_pr,
      commands::gh_set_pr_draft,
      commands::gh_current_user,
      commands::gh_pr_diff,
      commands::gh_reply_pr_review_thread,
      commands::gh_submit_pr_review,
      commands::gh_create_pr,
      commands::gh_list_pr_review_threads,
      commands::linear_list_teams,
      commands::linear_list_issues,
      commands::linear_open_or_create_workspace_from_issue,
      commands::linear_get_viewer,
      commands::linear_set_proxy_session,
      commands::linear_list_projects,
      commands::linear_list_project_documents,
      commands::linear_list_issue_comments,
      commands::linear_list_project_comments,
      commands::linear_list_document_comments,
      commands::linear_prepare_review,
      commands::linear_apply_review_suggestion,
      commands::load_repo_yaml_config,
      commands::list_workflows,
      commands::run_workflow_job,
      commands::run_workflow,
      commands::is_repo_trusted,
      commands::trust_repo,
      commands::list_workflow_runs,
      commands::get_workspace_setup_status,
      commands::rerun_workspace_setup_script,
      commands::get_run_logs,
      commands::get_repo_logs,
      commands::run_logs_sql,
      commands::get_log_timeseries,
      commands::register_agent_chat,
      commands::record_agent_chat_user_message,
      commands::record_agent_chat_screen,
      commands::list_agent_chats,
      commands::get_agent_chat,
    ])
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|app_handle, event| {
      // Local and remote PTY children (and their reader threads) are
      // otherwise only closed by an explicit `pty_close`/`remote_pty_close`
      // call from the frontend. Neither happens when a window is closed
      // via dashboard teardown or the native "Close Window" menu, so
      // without this the PTY processes and reader threads outlive the
      // window (and, on platforms that keep the app running with no
      // windows open, outlive it indefinitely). `Exit` fires once, after
      // every window has closed or `app.exit()` was called, so this is
      // the one place that's safe to tear every session down at once.
      if let tauri::RunEvent::Exit = event {
        if let Some(state) = app_handle.try_state::<AppState>() {
          state.pty_manager.close_all();
        }
        if let Some(state) = app_handle.try_state::<commands::remote_pty_commands::RemotePtyState>()
        {
          tauri::async_runtime::block_on(state.0.close_all());
        }
      }
    });
}

/// Without the desktop app, `treq` is only the command-line interface.
#[cfg(not(feature = "gui"))]
pub fn run() {
  telemetry::install_panic_hook();
  std::process::exit(cli::args::run(std::env::args_os()));
}

#[cfg(all(test, feature = "gui"))]
mod tests {
  use super::agent_runtime::{
    build_agent_deep_link_url, extract_repo_from_agent_deep_link, parse_agent_request_from_url,
  };
  use super::is_cli_process;
  use super::menu_item_url;

  #[test]
  fn maps_learn_more_menu_item_to_docs_url() {
    assert_eq!(menu_item_url("learn_more"), Some("https://treq.dev/docs"));
  }

  #[test]
  fn maps_discussions_menu_item_to_github_discussions() {
    assert_eq!(
      menu_item_url("discussions"),
      Some("https://github.com/tznc/treq/discussions")
    );
  }

  #[test]
  fn returns_no_url_for_menu_items_handled_in_app() {
    assert_eq!(menu_item_url("view_logs"), None);
    assert_eq!(menu_item_url("settings"), None);
  }

  #[test]
  fn identifies_cli_process_before_tauri_plugins_initialize() {
    assert!(is_cli_process(["treq", "st"]));
    assert!(is_cli_process(["treq", "--help"]));
    assert!(is_cli_process(["treq", "unknown-command"]));
    assert!(!is_cli_process(["treq"]));
    assert!(!is_cli_process([
      "treq",
      "treq://agent/start?repo=%2Ftmp%2Frepo"
    ]));
    assert!(!is_cli_process(["treq", "-psn_0_12345"]));
  }

  #[test]
  fn extracts_repo_from_agent_deep_link() {
    let url =
            "treq://agent/start?repo=%2Ftmp%2Frepo&branch=feat%2Fx&prompt=hello&mode=plan&agent=codex&request_id=req-1";
    let repo = extract_repo_from_agent_deep_link(url);
    assert_eq!(repo.as_deref(), Some("/tmp/repo"));
  }

  #[test]
  fn ignores_non_agent_deep_link() {
    let url = "treq://auth/callback?token=abc";
    assert_eq!(extract_repo_from_agent_deep_link(url), None);
  }

  #[test]
  fn classifies_agent_start_link_as_agent() {
    let url = "treq://agent/start?repo=%2Ftmp%2Frepo&request_id=req-1";
    assert_eq!(super::classify_deep_link(url), super::DeepLinkKind::Agent);
  }

  #[test]
  fn classifies_auth_callback_link_as_auth_callback() {
    let url = "treq://auth/callback?token=abc";
    assert_eq!(
      super::classify_deep_link(url),
      super::DeepLinkKind::AuthCallback
    );
  }

  #[test]
  fn classifies_other_links_as_unknown() {
    for url in [
      "treq://auth/other?token=abc",
      "treq://auth/callback/extra?token=abc",
      "https://treq.dev/auth/callback?token=abc",
      "treq://settings",
      "not a url",
    ] {
      assert_eq!(
        super::classify_deep_link(url),
        super::DeepLinkKind::Unknown,
        "{url}"
      );
    }
  }

  #[test]
  fn parse_agent_request_from_url_round_trips() {
    let request = crate::agent_dispatch::AgentDispatchRequest {
      request_id: "req-1".to_string(),
      repo: "/tmp/repo".to_string(),
      branch: "feat/x".to_string(),
      prompt: "hello".to_string(),
      mode: "plan".to_string(),
      agent: "codex".to_string(),
    };
    let url = build_agent_deep_link_url(&request);
    let parsed = parse_agent_request_from_url(&url).expect("request should parse");
    assert_eq!(parsed, request);
  }
}
