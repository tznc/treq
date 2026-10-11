mod agent_dispatch;
mod agent_runtime;
pub mod auto_rebase;
pub mod auto_review;
pub mod binary_paths;
// The `treq` command-line entry point is desktop-only: it is backed by
// `tauri-plugin-cli`, which has no Android/iOS implementation.
#[cfg(desktop)]
mod cli;
mod commands;
mod commit_timestamps;
pub mod conflict_markers;
pub mod core;
pub mod db;
pub mod file_indexer;
pub mod github;
pub mod jira;
pub mod jj;
pub mod linear;
pub mod local_db;
pub mod lock_ext;
pub mod notify_dispatch;
mod open_new_window;
pub mod pr_status;
pub mod pty;
pub mod repo_config;
pub mod review_aggregate;
pub mod send_dispatch;
pub mod telemetry;
pub mod tracker;
pub mod trello;
pub mod unified_patch;

use agent_runtime::{
  parse_agent_request_from_url, route_agent_deep_link, route_agent_dispatch_request,
  start_agent_ipc_listener, start_instance_registry_heartbeat,
};
use commands::file_watcher::WatcherManager;
use db::Database;
use pty::PtyManager;
use std::collections::HashMap;
use std::sync::Mutex;
#[cfg(desktop)]
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, EventTarget, Manager};
use tauri_plugin_log::{Target, TargetKind};

#[cfg(feature = "tauri-test")]
extern crate self as treq_lib;

#[cfg(feature = "tauri-test")]
mod e2e_test_helpers;

#[cfg(feature = "tauri-test")]
mod tauri_test_bridge;

#[cfg(feature = "tauri-test")]
pub(crate) mod test_pty_events;

#[cfg(feature = "tauri-test")]
use crate::commands::CutoffReasonDto;
#[cfg(feature = "tauri-test")]
use crate::core::remote::TreqCommandRequest;
#[cfg(feature = "tauri-test")]
use crate::core::remote_control_plane::SshEndpoint;
#[cfg(feature = "tauri-test")]
use crate::core::remote_pty::PtyLaunchSpec;
#[cfg(feature = "tauri-test")]
use crate::core::skills::SkillInstallScope;
#[cfg(feature = "tauri-test")]
use crate::tracker::TrackerProvider;
#[cfg(feature = "tauri-test")]
use commands::RemoteExecState;
#[cfg(feature = "tauri-test")]
use commands::RemotePtyState;

#[cfg(feature = "tauri-test")]
#[tauri_test::setup(init = tauri_test_bridge::init_test_state)]
pub struct TauriTestApp;

pub(crate) struct AppState {
  db: Mutex<Database>,
  pty_manager: PtyManager,
  watcher_manager: WatcherManager,
  window_repo_paths: Mutex<HashMap<String, String>>,
  window_last_focused_at: Mutex<HashMap<String, u64>>,
  dispatch_instance_id: String,
  dispatch_started_at: u64,
  dispatch_endpoint: String,
  // Held for its Drop guards (file writer + provider shutdown).
  _telemetry: telemetry::TelemetryGuards,
}

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
      dispatch_instance_id,
      dispatch_started_at,
      dispatch_endpoint,
      _telemetry: telemetry,
    }
  }
}

/// Picks which webview a menu event targets, following the same rule as
/// `emit_to_focused`: the currently focused window, falling back to the
/// most recently focused one that still exists.
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

/// Emits an event only to the focused webview window.
///
/// On macOS the menu bar unfocuses every window, so this never broadcasts
/// globally — a broadcast would run folder pickers in every open webview.
pub fn emit_to_focused<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
  let Some(label) = resolve_menu_target_label(app) else {
    return;
  };
  let _ = app.emit_to(EventTarget::webview_window(&label), event, payload);
}

/// What a `treq://` deep link asks the app to do.
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
      let _ = app.emit_to(
        EventTarget::webview_window(&label),
        "menu-repository-path-selected",
        path,
      );
      open_new_window::OPEN_REPO_GATE.end();
    });
}

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
          emit_to_focused(&app_for_init, "menu-open-in-new-window-invalid", ());
          open_new_window::OPEN_NEW_WINDOW_GATE.end();
          return;
        }
        let label = open_new_window::new_repo_window_label();
        let url = open_new_window::new_repo_window_url(&path);
        let title = open_new_window::new_repo_window_title(&path);
        let _ = tauri::WebviewWindowBuilder::new(
          &app_for_init,
          &label,
          tauri::WebviewUrl::App(url.into()),
        )
        .title(&title)
        .inner_size(1400.0, 900.0)
        .build();
        open_new_window::OPEN_NEW_WINDOW_GATE.end();
      });
    });
}

/// Web page a menu item opens in the browser, on every desktop platform.
/// Menu items handled inside the app return `None`.
#[cfg(desktop)]
fn menu_item_url(menu_id: &str) -> Option<&'static str> {
  match menu_id {
    "learn_more" => Some("https://treq.dev/docs"),
    _ => None,
  }
}

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

#[cfg_attr(all(mobile, not(feature = "embed")), tauri::mobile_entry_point)]
pub fn run() {
  telemetry::install_panic_hook();
  // CLI invocations never start Tauri: see `cli::args` for why.
  #[cfg(desktop)]
  if is_cli_process(std::env::args_os()) {
    std::process::exit(cli::args::run(std::env::args_os()));
  }
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
      // Fires once, after the window is actually gone (not on a
      // cancellable close request), for every window this app opens —
      // "main" and any later "Open in New Window" targets alike — so
      // this is the one place that reliably closes a window's own
      // terminals without touching another window's, regardless of
      // which UI path (dashboard unmount, native "Close Window", the
      // OS window-close button) tore it down.
      if let tauri::WindowEvent::Destroyed = event {
        let label = window.label().to_string();
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

      // Read saved repo path to embed in the window URL (avoids Onboarding flash)
      let saved_repo_path = db.get_setting("last_opened_repo_path").ok().flatten();
      let window_url = if let Some(ref path) = saved_repo_path {
        let encoded = urlencoding::encode(path).into_owned();
        format!("index.html?repo={}", encoded)
      } else {
        "index.html".to_string()
      };

      let _window =
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App(window_url.into()))
          .title("Treq - Stacking ADE")
          .inner_size(1400.0, 900.0)
          .build()?;

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
      start_agent_ipc_listener(app.handle().clone(), dispatch_listener);
      start_instance_registry_heartbeat(app.handle().clone());

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
                emit_to_focused(&handle, "deep-link-received", vec![url]);
              }
              DeepLinkKind::Agent => {
                if let Some(request) = parse_agent_request_from_url(&url) {
                  let response = route_agent_dispatch_request(&handle, &request);
                  if response.status != "handled" {
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

          let file_menu = SubmenuBuilder::new(app, "File")
            .item(&open_item)
            .item(&open_new_window_item)
            .item(&open_ssh_item)
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

          let check_for_updates_item =
            MenuItemBuilder::with_id("check_for_updates", "Check for Updates...").build(app)?;

          let learn_more_item = MenuItemBuilder::with_id("learn_more", "Learn More").build(app)?;

          let help_menu = SubmenuBuilder::new(app, "Help")
            .item(&check_for_updates_item)
            .item(&view_logs_item)
            .separator()
            .item(&learn_more_item)
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

          let file_menu = SubmenuBuilder::new(app, "File")
            .item(&open_item)
            .item(&open_new_window_item)
            .item(&open_ssh_item)
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
          let help_menu = SubmenuBuilder::new(app, "Help")
            .item(&view_logs_item)
            .separator()
            .item(&learn_more_item)
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
          "dashboard" => emit_to_focused(app, "navigate-to-dashboard", ()),
          "settings" => emit_to_focused(app, "navigate-to-settings", ()),
          "open" => schedule_open_repo(app.clone()),
          "open_new_window" => schedule_open_repo_in_new_window(app.clone()),
          "open_ssh" => emit_to_focused(app, "menu-open-ssh", ()),
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
          "force_rebase_workspace" => emit_to_focused(app, "menu-force-rebase-workspace", ()),
          "factory_reset" => emit_to_focused(app, "menu-factory-reset", ()),
          "view_logs" => {
            use tauri_plugin_opener::OpenerExt;
            if let Ok(dir) = app.path().app_log_dir() {
              let _ = app.opener().open_path(dir.to_string_lossy(), None::<&str>);
            }
          }
          "check_for_updates" => emit_to_focused(app, "menu-check-for-updates", ()),
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
      commands::move_workspace_changes,
      commands::move_commit_to_existing_workspace,
      commands::abandon_commit,
      commands::undo_commit,
      commands::revert_commit,
      commands::rename_workspace,
      commands::list_workspace_statuses,
      commands::get_workspace_status,
      commands::update_workspace,
      commands::schedule_workspaces,
      commands::set_workspace_target_branch,
      commands::check_and_rebase_workspaces,
      commands::resolve_workspace_bookmark_conflict,
      commands::ensure_workspace_indexed,
      commands::get_setting,
      commands::get_settings_batch,
      commands::set_setting,
      commands::get_repo_setting,
      commands::set_repo_setting,
      commands::list_skill_catalog,
      commands::list_installed_skills,
      commands::install_skill,
      commands::uninstall_skill,
      commands::set_skill_install_scope,
      commands::check_for_app_update,
      commands::install_app_update,
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
      commands::cleanup_agent_cli_files,
      commands::get_file_modified_at,
      commands::list_directory,
      commands::list_directories_batch,
      commands::list_send_artifacts,
      commands::ls_workspace_with_status,
      commands::list_gitignored_path_suggestions,
      commands::get_workspace_readme,
      commands::list_directory_cached,
      commands::search_workspace_files,
      commands::create_session,
      commands::get_sessions,
      commands::update_session_access,
      commands::get_session_model,
      commands::set_session_model,
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
      commands::resolve_agent_review_comment,
      commands::delete_agent_review_comment,
      commands::apply_agent_review_suggestion,
      commands::load_file_browser_review,
      commands::save_file_browser_review,
      commands::clear_file_browser_review,
      commands::load_pending_page_review,
      commands::save_pending_page_review,
      commands::clear_pending_page_review,
      commands::open_browser_webview,
      commands::navigate_browser_webview,
      commands::close_browser_webview,
      commands::set_browser_select_mode,
      commands::sync_browser_webview_bounds,
      commands::list_ssh_hosts,
      commands::list_local_ssh_identities,
      commands::read_local_ssh_public_key,
      commands::resolve_ssh_config_alias,
      commands::ensure_mobile_device_key,
      commands::remote_dispatch_local,
      commands::remote_dispatch_over_ssh,
      commands::remote_dispatch_mutation_over_ssh,
      commands::remote_transport_metrics,
      commands::remote_force_cutoff,
      commands::remote_clear_cutoff,
      commands::remote_cut_off_managed,
      commands::remote_set_relay_access_token,
      commands::remote_cutoff_reason,
      commands::set_window_repo_path,
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
      commands::gh_reopen_pr,
      commands::gh_set_pr_draft,
      commands::gh_create_pr,
      commands::gh_list_pr_review_threads,
      commands::linear_list_teams,
      commands::linear_list_issues,
      commands::linear_open_or_create_workspace_from_issue,
      commands::linear_start_auto_kickoff_polling,
      commands::linear_get_viewer,
      commands::linear_set_proxy_session,
      commands::linear_list_projects,
      commands::linear_list_project_documents,
      commands::linear_list_issue_comments,
      commands::linear_list_project_comments,
      commands::linear_list_document_comments,
      commands::tracker_list_containers,
      commands::tracker_list_items,
      commands::tracker_get_viewer,
      commands::tracker_open_or_create_workspace_from_item,
      commands::tracker_start_auto_kickoff_polling,
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

#[cfg(test)]
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
