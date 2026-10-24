//! Process-wide services shared by every screen: the tokio runtime that
//! treq's SSH transport needs, the settings database, the SSH connection
//! pool and remote PTY manager, and the Supabase client.
//!
//! GPUI's executor is not tokio, so async work is started here with
//! [`spawn`] and its result awaited from GPUI through a oneshot channel.

use std::{
  future::Future,
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
  },
};

use futures::channel::{mpsc, oneshot};
use treq_lib::{
  core::{
    auth::{Auth, SessionStore, SESSION_SETTING},
    remote_control_plane::SshEndpoint,
    remote_pty::RemotePtyManager,
    remote_ssh_transport::SshConnectionPool,
    supabase::Supabase,
  },
  db::Database,
};

use crate::config;

/// Something the UI should react to, sent from background work.
#[derive(Debug, Clone)]
pub enum AppEvent {
  /// Sign-in state changed (`Some(email)` when signed in).
  Auth(Option<String>),
  /// A one-time sign-in token arrived through `treq://auth/callback`.
  AuthToken(String),
  /// The pool cut off an endpoint's credentials (`None` reason: cleared).
  Cutoff {
    endpoint_id: String,
    reason: Option<String>,
  },
  /// The app came back to the foreground.
  Resumed,
}

/// How the current endpoint was reached.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionKind {
  Managed,
  UserManaged { id: String },
}

/// The machine id of the Treq Cloud machine. An SSH host's is its record id.
pub const MANAGED_MACHINE: &str = "managed";

#[derive(Debug, Clone)]
pub struct Connection {
  pub endpoint: SshEndpoint,
  pub kind: ConnectionKind,
}

impl Connection {
  /// Which machine this connects to: [`MANAGED_MACHINE`] or the SSH host's id.
  pub fn machine(&self) -> &str {
    match &self.kind {
      ConnectionKind::Managed => MANAGED_MACHINE,
      ConnectionKind::UserManaged { id } => id,
    }
  }
}

/// Every machine connected this session, and which one the screens act on.
/// The pool keeps each endpoint's SSH connection, so switching machines
/// leaves the others connected.
#[derive(Default)]
struct Connections {
  all: Vec<Connection>,
  active: Option<String>,
}

pub struct Backend {
  runtime: tokio::runtime::Runtime,
  pub app: tauri::AppHandle,
  pub db: Mutex<Database>,
  pub pool: Arc<SshConnectionPool>,
  pub ptys: RemotePtyManager,
  pub supabase: Arc<Supabase>,
  pub auth: Arc<Auth>,
  connections: Mutex<Connections>,
  events: mpsc::UnboundedSender<AppEvent>,
}

static BACKEND: OnceLock<Arc<Backend>> = OnceLock::new();

/// Cleared while the app is in the background, where polling the host
/// would only spend battery.
static FOREGROUND: AtomicBool = AtomicBool::new(true);

pub fn set_foreground(foreground: bool) {
  FOREGROUND.store(foreground, Ordering::Relaxed);
}

pub fn in_foreground() -> bool {
  FOREGROUND.load(Ordering::Relaxed)
}

/// The backend, once `init` has run (TAO reports `Resumed` before setup).
pub fn try_get() -> Option<&'static Arc<Backend>> {
  BACKEND.get()
}

pub fn get() -> &'static Arc<Backend> {
  BACKEND.get().expect("backend::init runs in setup")
}

/// Opens the settings database under `data_dir` and builds the services.
/// Returns the receiving end of the [`AppEvent`] channel for the UI.
pub fn init(
  app: tauri::AppHandle,
  data_dir: PathBuf,
) -> anyhow::Result<mpsc::UnboundedReceiver<AppEvent>> {
  std::fs::create_dir_all(&data_dir)?;
  // treq's core resolves its app database through this variable.
  std::env::set_var("TREQ_APP_DATA_DIR", &data_dir);
  let db = Database::new(data_dir.join(treq_lib::core::APP_DB_FILE_NAME))?;
  db.init()?;

  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(2)
    .thread_name("treq-backend")
    .enable_all()
    .build()?;
  let pool = Arc::new(new_pool(&app));
  let (events, receiver) = mpsc::unbounded();
  let supabase = Arc::new(Supabase::new(
    &config::env().supabase,
    Arc::new(SettingsStore),
  ));
  // Sign-in changes reach the UI as `Auth(email)`.
  supabase.auth.subscribe(|status| {
    let email = status
      .session
      .as_ref()
      .map(|s| s.user.email.clone().unwrap_or_else(|| s.user.id.clone()));
    if let Some(backend) = try_get() {
      backend.emit(AppEvent::Auth(email));
    }
  });
  let backend = Arc::new(Backend {
    ptys: RemotePtyManager::new(pool.clone()),
    runtime,
    app,
    db: Mutex::new(db),
    pool,
    auth: supabase.auth.clone(),
    supabase,
    connections: Mutex::new(Connections::default()),
    events,
  });
  BACKEND
    .set(backend)
    .map_err(|_| anyhow::anyhow!("backend initialized twice"))?;
  Ok(receiver)
}

/// Keeps the Supabase session in the settings database.
struct SettingsStore;

impl SessionStore for SettingsStore {
  fn load(&self) -> Option<String> {
    try_get()?.setting(SESSION_SETTING)
  }

  fn save(&self, value: &str) {
    if let Some(backend) = try_get() {
      backend.set_setting(SESSION_SETTING, value);
    }
  }
}

#[cfg(mobile)]
fn new_pool(app: &tauri::AppHandle) -> SshConnectionPool {
  SshConnectionPool::new().with_device_key_provider(
    treq_lib::core::remote_device_key::device_key_provider(app.clone()),
  )
}

#[cfg(not(mobile))]
fn new_pool(_app: &tauri::AppHandle) -> SshConnectionPool {
  SshConnectionPool::new()
}

/// Runs `future` on the backend runtime. Await the receiver from GPUI.
pub fn spawn<T: Send + 'static>(
  future: impl Future<Output = T> + Send + 'static,
) -> oneshot::Receiver<T> {
  let (tx, rx) = oneshot::channel();
  get().runtime.spawn(async move {
    let _ = tx.send(future.await);
  });
  rx
}

/// Runs `future` on the backend runtime without waiting for it.
pub fn spawn_detached(future: impl Future<Output = ()> + Send + 'static) {
  get().runtime.spawn(future);
}

impl Backend {
  pub fn emit(&self, event: AppEvent) {
    let _ = self.events.unbounded_send(event);
  }

  pub fn with_db<T>(&self, f: impl FnOnce(&Database) -> T) -> T {
    f(&self.db.lock().unwrap_or_else(|e| e.into_inner()))
  }

  pub fn setting(&self, key: &str) -> Option<String> {
    let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
    db.get_setting(key).ok().flatten().filter(|v| !v.is_empty())
  }

  pub fn set_setting(&self, key: &str, value: &str) {
    let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(error) = db.set_setting(key, value) {
      log::error!("saving setting {key}: {error}");
    }
  }

  /// The machine the screens act on.
  pub fn connection(&self) -> Option<Connection> {
    let connections = self.connections.lock().unwrap();
    let active = connections.active.as_deref()?;
    connections
      .all
      .iter()
      .find(|c| c.machine() == active)
      .cloned()
  }

  /// The connection to `machine`, active or not.
  pub fn connection_to(&self, machine: &str) -> Option<Connection> {
    let connections = self.connections.lock().unwrap();
    connections
      .all
      .iter()
      .find(|c| c.machine() == machine)
      .cloned()
  }

  pub fn endpoint(&self) -> Result<SshEndpoint, String> {
    self
      .connection()
      .map(|c| c.endpoint)
      .ok_or_else(|| "Not connected to a host.".to_string())
  }

  /// Adds or replaces the connection to its machine, without changing
  /// which machine is active.
  pub fn put_connection(&self, connection: Connection) {
    let mut connections = self.connections.lock().unwrap();
    connections
      .all
      .retain(|c| c.machine() != connection.machine());
    connections.all.push(connection);
  }

  /// Makes `machine` the one the screens act on (`None`: no machine).
  pub fn set_active(&self, machine: Option<&str>) {
    self.connections.lock().unwrap().active = machine.map(str::to_string);
  }

  /// Forgets the connection to `machine`, returning it.
  pub fn remove_connection(&self, machine: &str) -> Option<Connection> {
    let mut connections = self.connections.lock().unwrap();
    if connections.active.as_deref() == Some(machine) {
      connections.active = None;
    }
    let index = connections
      .all
      .iter()
      .position(|c| c.machine() == machine)?;
    Some(connections.all.remove(index))
  }

  /// Swaps in a renewed certificate, keeping the endpoint id (the pool
  /// keeps its open connections).
  pub fn update_endpoint(&self, endpoint: SshEndpoint) {
    for connection in &mut self.connections.lock().unwrap().all {
      if connection.endpoint.id == endpoint.id {
        connection.endpoint = endpoint.clone();
      }
    }
  }
}
