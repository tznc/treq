#![cfg_attr(
  test,
  allow(
    clippy::string_slice,
    reason = "tests slice fixed literals at known offsets"
  )
)]

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
/// The GPUI screens' pure helpers, for `benches/diff_view.rs`.
#[cfg(feature = "gpui-shell")]
#[cfg(desktop)]
#[doc(hidden)]
pub use gpui_shell::bench as gpui_bench;
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
pub mod pty_stream;
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
use tauri::{AppHandle, Manager};
#[cfg(feature = "gui")]
use tauri_plugin_log::{Target, TargetKind};

// `tests/common` names the crate as integration tests see it.
#[cfg(test)]
extern crate self as treq_lib;

/// The real-repository fixture the integration tests use (`tests/common`).
#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test fixture")]
mod test_repo;

#[cfg(feature = "gui")]
pub(crate) struct AppState {
  db: Mutex<Database>,
  pty_manager: PtyManager,
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
    dispatch_instance_id: String,
    dispatch_started_at: u64,
    dispatch_endpoint: String,
    telemetry: telemetry::TelemetryGuards,
  ) -> Self {
    Self {
      db: Mutex::new(db),
      pty_manager,
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

/// The app's repo windows by label, leaving out the popup windows the GPUI
/// shell opens over them (tooltips, menus, toasts and dialogs).
#[cfg(feature = "gui")]
fn repo_windows(app: &AppHandle) -> HashMap<String, tauri::Window> {
  app
    .windows()
    .into_iter()
    .filter(|(label, _)| !label.starts_with("gpui-popup-"))
    .collect()
}

/// Whether closing window `closed` leaves no repo window open, given the
/// repo windows still listed (which may include `closed` itself).
#[cfg(feature = "gui")]
fn is_last_repo_window<'a>(closed: &str, open: impl IntoIterator<Item = &'a String>) -> bool {
  !closed.starts_with("gpui-popup-") && open.into_iter().all(|label| label == closed)
}

/// Picks which window a menu event targets: the currently focused window,
/// falling back to the most recently focused one that still exists.
#[cfg(feature = "gui")]
fn resolve_menu_target_label(app: &AppHandle) -> Option<String> {
  let windows = repo_windows(app);
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

/// Hands a menu event for window `label` to its GPUI window model.
#[cfg(feature = "gui")]
fn gpui_menu_event(app: &AppHandle, label: &str, event: &str, payload: Option<String>) {
  #[cfg(feature = "gpui-shell")]
  #[cfg(desktop)]
  if !gpui_shell::menu_event(app, label, event, payload) {
    log::debug!("window {label} has no handler for {event}");
  }
  #[cfg(not(all(feature = "gpui-shell", desktop)))]
  let _ = (app, label, event, payload);
}

/// A menu event for the focused window's model.
///
/// On macOS the menu bar unfocuses every window, so this falls back to the
/// most recently focused window and never broadcasts: a broadcast would run
/// folder pickers in every open window.
#[cfg(feature = "gui")]
fn menu_to_focused(app: &AppHandle, event: &str) {
  if let Some(label) = resolve_menu_target_label(app) {
    gpui_menu_event(app, &label, event, None);
  }
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

/// Sorts a deep link by what it asks for.
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
  let Some(window) = resolve_menu_target_label(app).and_then(|l| app.get_window(&l)) else {
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
/// one already showing. The target window is resolved and retained before
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
      gpui_menu_event(&app, &label, "menu-repository-path-selected", Some(path));
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
      let app_for_init = app;
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

/// Go > Overview: always a new window on the start screen, the machines
/// and recent repositories.
#[cfg(feature = "gui")]
#[cfg(desktop)]
fn open_overview_window(app: &AppHandle) {
  let label = open_new_window::new_repo_window_label();
  let handle = app.clone();
  let _ = app.run_on_main_thread(move || {
    if let Err(err) = open_repo_window(&handle, &label, None, "Treq - Overview") {
      log::error!("failed to open overview window: {err}");
    }
  });
}

/// Opens a window on repository `repo` (`None`: the start screen) in the
/// GPUI shell (see `gpui_shell`).
#[cfg(feature = "gui")]
fn open_repo_window(
  app: &AppHandle,
  label: &str,
  repo: Option<String>,
  title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(feature = "gpui-shell")]
  #[cfg(desktop)]
  return gpui_shell::open_window(app, label, repo, title);
  #[cfg(not(all(feature = "gpui-shell", desktop)))]
  {
    let _ = (app, label, repo, title);
    Err("built without the GPUI shell".into())
  }
}

/// GPUI could not start, so no window can open: says why in a native
/// dialog, then quits once it is closed.
#[cfg(feature = "gpui-shell")]
#[cfg(desktop)]
fn quit_without_gpui(app: &AppHandle, error: &dyn std::fmt::Display) {
  use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
  log::error!("GPUI failed to start: {error}");
  let handle = app.clone();
  app
    .dialog()
    .message(format!(
      "Treq could not start its renderer, so it has to quit.\n\n{error}"
    ))
    .title("Treq could not start")
    .kind(MessageDialogKind::Error)
    .show(move |_| handle.exit(1));
}

/// The macOS Edit menu: each item's id, title, accelerator and the GPUI
/// keystroke it types into the focused window.
#[cfg(feature = "gui")]
#[cfg(desktop)]
const EDIT_MENU: &[(&str, &str, &str, &str)] = &[
  ("edit_undo", "Undo", "Cmd+Z", "cmd-z"),
  ("edit_redo", "Redo", "Cmd+Shift+Z", "cmd-shift-z"),
  ("edit_cut", "Cut", "Cmd+X", "cmd-x"),
  ("edit_copy", "Copy", "Cmd+C", "cmd-c"),
  ("edit_paste", "Paste", "Cmd+V", "cmd-v"),
  ("edit_select_all", "Select All", "Cmd+A", "cmd-a"),
];

/// The keystroke an Edit menu item types, by its id.
#[cfg(feature = "gui")]
#[cfg(desktop)]
fn edit_menu_keystroke(menu_id: &str) -> Option<&'static str> {
  EDIT_MENU
    .iter()
    .find(|(id, ..)| *id == menu_id)
    .map(|&(.., keystroke)| keystroke)
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
#[allow(
  clippy::exit,
  reason = "a CLI invocation hands its exit code to the shell, and `tauri::generate_context!` expands to an exit"
)]
#[allow(
  clippy::expect_used,
  reason = "a Tauri build failure at startup is unrecoverable"
)]
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
      if !window.label().starts_with("gpui-popup-") {
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
      // cancellable close request), whichever UI path closed it. A
      // window's terminals outlive it while another repo window is open,
      // so another window can show them; the last repo window closes
      // every terminal.
      if let tauri::WindowEvent::Destroyed = event {
        let label = window.label().to_string();
        #[cfg(feature = "gpui-shell")]
        #[cfg(desktop)]
        gpui_shell::forget_window(&label);
        let app_handle = window.app_handle().clone();
        if is_last_repo_window(&label, repo_windows(&app_handle).keys()) {
          #[cfg(feature = "gpui-shell")]
          #[cfg(desktop)]
          gpui_shell::forget_terminals();
          if let Some(state) = app_handle.try_state::<AppState>() {
            state.pty_manager.close_all();
          }
          if let Some(state) =
            app_handle.try_state::<commands::remote_pty_commands::RemotePtyState>()
          {
            tauri::async_runtime::block_on(state.0.close_all());
          }
        }
      }
    })
    .setup(move |app| {
      let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Failed to get app log dir: {e}"))?;
      let telemetry =
        telemetry::init(&log_dir).map_err(|e| format!("Failed to initialize telemetry: {e}"))?;

      let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;
      std::fs::create_dir_all(&app_dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;
      std::env::set_var("TREQ_APP_DATA_DIR", app_dir.to_string_lossy().to_string());
      let db_path = app_dir.join(crate::core::APP_DB_FILE_NAME);

      let db = Database::new(db_path).map_err(|e| format!("Failed to open database: {e}"))?;
      db.init()
        .map_err(|e| format!("Failed to initialize database: {e}"))?;

      // The saved repo opens with the window (avoids Onboarding flash).
      let saved_repo_path = db.get_setting("last_opened_repo_path").ok().flatten();
      // The e2e suite opens its fixture repository instead.
      #[cfg(feature = "e2e")]
      let saved_repo_path = gpui_shell::e2e::repo().or(saved_repo_path);

      // Resolve the login-shell PATH off the main thread, before anything
      // else can wait on it, so the first terminal doesn't wait on shell
      // startup.
      std::thread::spawn(binary_paths::get_extended_path);

      // Cached paths and editors are read from the database here; what was
      // never looked for is detected in the background once the state is
      // managed (`detect_binaries`, and the window's `detect_editors`).
      let (binary_paths, undetected_binaries) = commands::load_cached_binary_paths(&db);
      binary_paths::init_binary_paths_cache(binary_paths);
      if let Some(editor_apps) = commands::load_cached_editor_apps(&db) {
        binary_paths::init_editor_apps_cache(editor_apps);
      }

      let pty_manager = PtyManager::new();

      // Background PR-status poller (sidebar reads from its cache)
      crate::pr_status::set_app_handle(app.handle().clone());

      // Automatic reviews: jj operations signal, the window launches the
      // review terminal.
      #[cfg(feature = "gpui-shell")]
      #[cfg(desktop)]
      {
        let review_app = app.handle().clone();
        crate::auto_review::set_emitter(Box::new(move |event| {
          gpui_shell::launch::auto_review(&review_app, event);
        }));
      }

      let (dispatch_listener, dispatch_endpoint) = agent_dispatch::bind_ephemeral_listener()?;
      let dispatch_instance_id = uuid::Uuid::new_v4().to_string();
      let dispatch_started_at = agent_dispatch::now_millis();
      let app_state = AppState::new(
        db,
        pty_manager,
        dispatch_instance_id,
        dispatch_started_at,
        dispatch_endpoint,
        telemetry,
      );

      app.manage(app_state);
      if !undetected_binaries.is_empty() {
        let handle = app.handle().clone();
        std::thread::spawn(move || {
          commands::binary::detect_binaries(&handle.state::<AppState>().db, &undetected_binaries);
        });
      }
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
      // Only now: a window's model reads the database and the binary
      // caches as soon as the window is made.
      #[cfg(feature = "gpui-shell")]
      #[cfg(desktop)]
      match gpui_shell::init(app) {
        Ok(()) => open_repo_window(app.handle(), "main", saved_repo_path, "Treq - Stacking ADE")?,
        Err(err) => quit_without_gpui(app.handle(), &err),
      }
      // Rust owns the Supabase session; the windows draw the account.
      commands::cloud::start(app.handle());
      start_agent_ipc_listener(app.handle().clone(), dispatch_listener);
      start_instance_registry_heartbeat(app.handle().clone());
      crate::core::app_setup_script::spawn_run_if_needed(
        crate::core::app_setup_script::Trigger::Startup,
      );

      // Deep links: sign-in callbacks and agent starts.
      #[cfg(desktop)]
      {
        use tauri_plugin_deep_link::DeepLinkExt;
        // Installers register `treq://` with the OS. An AppImage or a dev
        // build is not installed, so it registers itself at startup.
        // It rewrites a launcher entry and runs `xdg-mime`, so it stays off
        // the startup path.
        #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
        {
          let handle = app.handle().clone();
          std::thread::spawn(move || {
            if let Err(err) = handle.deep_link().register_all() {
              log::warn!("failed to register deep link schemes: {err}");
            }
          });
        }
        let handle = app.handle().clone();
        app.deep_link().on_open_url(move |event| {
          let urls: Vec<String> = event.urls().into_iter().map(|u| u.to_string()).collect();
          for url in urls {
            match classify_deep_link(&url) {
              // One window only: the sign-in token can be exchanged once.
              DeepLinkKind::AuthCallback => commands::cloud::handle_auth_callback(&handle, &url),
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

          // Edit menu. The predefined items send `paste:` and the like to
          // the first responder, which GPUI's view does not answer, and
          // their key equivalents keep Cmd+V... from ever reaching GPUI.
          // These type their keys into the window instead.
          let mut edit_menu = SubmenuBuilder::new(app, "Edit");
          for &(id, text, accelerator, _) in EDIT_MENU {
            if id == "edit_cut" {
              edit_menu = edit_menu.separator();
            }
            edit_menu = edit_menu.item(
              &MenuItemBuilder::with_id(id, text)
                .accelerator(accelerator)
                .build(app)?,
            );
          }
          let edit_menu = edit_menu.build()?;

          // View menu
          let view_menu = SubmenuBuilder::new(app, "View")
            .item(&PredefinedMenuItem::fullscreen(app, None)?)
            .build()?;

          // Go menu items
          let overview_item = MenuItemBuilder::with_id("overview", "Overview").build(app)?;

          let dashboard_item = MenuItemBuilder::with_id("dashboard", "Dashboard")
            .accelerator("CmdOrCtrl+D")
            .build(app)?;

          let settings_item = MenuItemBuilder::with_id("settings", "Settings")
            .accelerator("CmdOrCtrl+,")
            .build(app)?;

          let go_menu = SubmenuBuilder::new(app, "Go")
            .item(&overview_item)
            .separator()
            .item(&dashboard_item)
            .item(&settings_item)
            .build()?;

          // Developer menu (only in debug mode)
          #[cfg(debug_assertions)]
          let developer_menu = {
            let force_rebase_item =
              MenuItemBuilder::with_id("force_rebase_workspace", "Force Rebase Workspace")
                .accelerator("CmdOrCtrl+Shift+R")
                .build(app)?;

            let factory_reset_item =
              MenuItemBuilder::with_id("factory_reset", "Factory Reset").build(app)?;

            SubmenuBuilder::new(app, "Developer")
              .item(&force_rebase_item)
              .separator()
              .item(&factory_reset_item)
              .build()?
          };

          // Window menu
          //
          // Close Window intentionally has no keyboard accelerator: the OS-default
          // Cmd+W is repurposed in the window to close the selected terminal (or
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
          let overview_item = MenuItemBuilder::with_id("overview", "Overview").build(app)?;

          let dashboard_item = MenuItemBuilder::with_id("dashboard", "Dashboard")
            .accelerator("CmdOrCtrl+D")
            .build(app)?;

          let settings_item = MenuItemBuilder::with_id("settings", "Settings")
            .accelerator("CmdOrCtrl+,")
            .build(app)?;

          let go_menu = SubmenuBuilder::new(app, "Go")
            .item(&overview_item)
            .separator()
            .item(&dashboard_item)
            .item(&settings_item)
            .build()?;

          // Developer menu (only in debug mode)
          #[cfg(debug_assertions)]
          let developer_menu = {
            let force_rebase_item =
              MenuItemBuilder::with_id("force_rebase_workspace", "Force Rebase Workspace")
                .accelerator("CmdOrCtrl+Shift+R")
                .build(app)?;

            let factory_reset_item =
              MenuItemBuilder::with_id("factory_reset", "Factory Reset").build(app)?;

            SubmenuBuilder::new(app, "Developer")
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

        // Handle menu events: only the focused window's
        app.on_menu_event(move |app, event| match event.id().as_ref() {
          "overview" => open_overview_window(app),
          "dashboard" => menu_to_focused(app, "navigate-to-dashboard"),
          "settings" => menu_to_focused(app, "navigate-to-settings"),
          "open" => schedule_open_repo(app.clone()),
          "open_new_window" => schedule_open_repo_in_new_window(app.clone()),
          "open_ssh" => menu_to_focused(app, "menu-open-ssh"),
          "add_repository" => menu_to_focused(app, "menu-add-repository"),
          "close_window" => {
            for (_, window) in repo_windows(app) {
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
            if let Some(keystroke) = edit_menu_keystroke(id) {
              #[cfg(feature = "gpui-shell")]
              gpui_shell::type_keystroke(app, keystroke);
              #[cfg(not(feature = "gpui-shell"))]
              let _ = keystroke;
            } else if let Some(url) = menu_item_url(id) {
              use tauri_plugin_opener::OpenerExt;
              let _ = app.opener().open_url(url, None::<&str>);
            }
          }
        });
      } // #[cfg(desktop)]

      Ok(())
    })
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
#[allow(clippy::exit, reason = "the CLI hands its exit code to the shell")]
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
  use super::is_last_repo_window;
  use super::menu_item_url;
  use super::{edit_menu_keystroke, EDIT_MENU};

  #[test]
  fn edit_menu_items_type_their_own_shortcut() {
    assert_eq!(edit_menu_keystroke("edit_paste"), Some("cmd-v"));
    assert_eq!(edit_menu_keystroke("edit_select_all"), Some("cmd-a"));
    assert_eq!(edit_menu_keystroke("settings"), None);
    for &(id, _, accelerator, keystroke) in EDIT_MENU {
      let typed = keystroke.replace('-', "+").to_lowercase();
      assert_eq!(accelerator.to_lowercase(), typed, "{id}");
    }
  }

  #[test]
  fn terminals_close_only_with_the_last_repo_window() {
    let labels = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(!is_last_repo_window("main", &labels(&["treq-1"])));
    assert!(!is_last_repo_window("main", &labels(&["main", "treq-1"])));
    assert!(is_last_repo_window("main", &labels(&[])));
    assert!(is_last_repo_window("main", &labels(&["main"])));
    assert!(!is_last_repo_window("gpui-popup-modal-main", &labels(&[])));
  }

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
