//! Process-wide services shared by every screen: the tokio runtime that
//! treq's SSH transport needs, the settings database, the SSH connection
//! pool and remote PTY manager, and the HTTP client for Supabase.
//!
//! GPUI's executor is not tokio, so async work is started here with
//! [`spawn`] and its result awaited from GPUI through a oneshot channel.

use std::{
  future::Future,
  path::PathBuf,
  sync::{Arc, Mutex, OnceLock},
};

use futures::channel::{mpsc, oneshot};
use treq_lib::{
  core::{
    remote_control_plane::SshEndpoint, remote_pty::RemotePtyManager,
    remote_ssh_transport::SshConnectionPool,
  },
  db::Database,
};

use crate::auth::AuthState;

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

#[derive(Debug, Clone)]
pub struct Connection {
  pub endpoint: SshEndpoint,
  pub kind: ConnectionKind,
}

pub struct Backend {
  runtime: tokio::runtime::Runtime,
  pub app: tauri::AppHandle,
  pub db: Mutex<Database>,
  pub pool: Arc<SshConnectionPool>,
  pub ptys: RemotePtyManager,
  pub http: reqwest::Client,
  pub auth: AuthState,
  connection: Mutex<Option<Connection>>,
  events: mpsc::UnboundedSender<AppEvent>,
}

static BACKEND: OnceLock<Arc<Backend>> = OnceLock::new();

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
  let backend = Arc::new(Backend {
    ptys: RemotePtyManager::new(pool.clone()),
    runtime,
    app,
    db: Mutex::new(db),
    pool,
    http: reqwest::Client::builder()
      .user_agent(concat!("treq-gpui/", env!("CARGO_PKG_VERSION")))
      .build()?,
    auth: AuthState::default(),
    connection: Mutex::new(None),
    events,
  });
  BACKEND
    .set(backend)
    .map_err(|_| anyhow::anyhow!("backend initialized twice"))?;
  Ok(receiver)
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

  pub fn connection(&self) -> Option<Connection> {
    self.connection.lock().unwrap().clone()
  }

  pub fn endpoint(&self) -> Result<SshEndpoint, String> {
    self
      .connection()
      .map(|c| c.endpoint)
      .ok_or_else(|| "Not connected to a host.".to_string())
  }

  pub fn set_connection(&self, connection: Option<Connection>) {
    *self.connection.lock().unwrap() = connection;
  }

  /// Swaps in a renewed certificate, keeping the endpoint id (the pool
  /// keeps its open connections).
  pub fn update_endpoint(&self, endpoint: SshEndpoint) {
    if let Some(connection) = self.connection.lock().unwrap().as_mut() {
      if connection.endpoint.id == endpoint.id {
        connection.endpoint = endpoint;
      }
    }
  }
}
