//! Treq's own persistent-terminal daemon: the default `pty-remote` backend
//! (see `core::pty_remote_supervisor`), which replaces tmux on remote hosts.
//!
//! One daemon per user per host (`treq pty-remote daemon`) holds every
//! session's PTY and a `vt100` model of its screen plus the last
//! [`HISTORY_LINES`] lines. The CLI calls that need it start it on demand
//! ([`connect_or_spawn`]); it exits on its own once it has had no sessions
//! for [`IDLE_EXIT`].
//!
//! The daemon listens on a unix socket under `~/.treq/pty/` (directory
//! 0700). It opens no network port, so only the user who owns the
//! directory can reach it, the same boundary as `core::agent_supervisor`.
//!
//! ## Wire protocol
//!
//! Each connection starts with one JSON [`Request`] line and gets one JSON
//! [`Reply`] line back. An [`Request::Attach`] connection then becomes a
//! stream: the daemon writes the replay ([`replay`]) followed by the
//! session's live output, and the client sends [`Frame`]s (input bytes or a
//! resize). The daemon closes the stream when the session's process exits.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::num::NonZeroU16;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Bumped when [`Request`], [`Reply`] or [`Frame`] change shape. A client
/// that finds a daemon speaking another version restarts it when it holds no
/// sessions, and otherwise asks the user to reset terminals.
pub const PROTOCOL_VERSION: u32 = 2;

/// Lines of history a reattach replays above the current screen.
pub const HISTORY_LINES: usize = 250;

/// How long the daemon stays up with no sessions before it exits.
pub const IDLE_EXIT: Duration = Duration::from_secs(60);

/// Delay between a paste and the Enter that submits it: a TUI reads an
/// Enter straight after a paste as part of the paste.
const SUBMIT_DELAY: Duration = Duration::from_millis(300);

/// A slow or stuck client is dropped rather than stalling the session.
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// The files the daemon keeps.
#[derive(Debug, Clone)]
pub struct DaemonPaths {
  dir: PathBuf,
}

impl DaemonPaths {
  pub fn new(dir: impl Into<PathBuf>) -> Self {
    Self { dir: dir.into() }
  }

  /// `~/.treq/pty`, or `$TREQ_PTY_DIR` when set (a spawned daemon is told
  /// its client's directory this way).
  pub fn default_paths() -> Result<Self, String> {
    if let Some(dir) = std::env::var_os("TREQ_PTY_DIR").filter(|dir| !dir.is_empty()) {
      return Ok(Self::new(dir));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
      "dependency_error: HOME is not set, so the terminal daemon has no home".to_string()
    })?;
    Ok(Self::new(Path::new(&home).join(".treq").join("pty")))
  }

  pub fn dir(&self) -> &Path {
    &self.dir
  }

  pub fn socket(&self) -> PathBuf {
    self.dir.join("daemon.sock")
  }

  fn lock(&self) -> PathBuf {
    self.dir.join("daemon.lock")
  }

  pub fn log(&self) -> PathBuf {
    self.dir.join("daemon.log")
  }
}

/// What a new session runs: `/bin/sh -c "cd <dir> && exec <program>"`.
/// `program` is always built by `remote_pty::build_launch_program` from a
/// typed spec, never taken from the wire as free text by a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartSpec {
  pub dir: String,
  pub program: String,
  pub cols: u16,
  pub rows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
  Hello,
  /// Starts `name` unless it is already running.
  Start {
    name: String,
    spec: StartSpec,
  },
  List,
  Stop {
    name: String,
  },
  Paste {
    name: String,
    text: String,
    submit: bool,
  },
  Resize {
    name: String,
    cols: u16,
    rows: u16,
  },
  /// Names a session; `None` clears the name.
  Rename {
    name: String,
    title: Option<String>,
  },
  /// Attaches to `name`, starting it from `start` first when it is not
  /// running. With no `start`, a missing session is an error.
  Attach {
    name: String,
    cols: u16,
    rows: u16,
    start: Option<StartSpec>,
  },
  /// Kills every session and stops the daemon.
  Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
  Ok(serde_json::Value),
  Error(String),
}

/// One session as [`Request::List`] answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
  pub name: String,
  /// Unix seconds of the session's last output.
  pub last_output: Option<u64>,
  pub title: Option<String>,
}

/// Seconds since the Unix epoch, by the host's clock.
pub fn unix_now() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |d| d.as_secs())
}

/// What [`Request::Hello`] answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
  pub protocol: u32,
  pub version: String,
  pub sessions: usize,
  /// The daemon's process id, for `treq pty-remote status`.
  #[serde(default)]
  pub pid: u32,
}

/// A client-to-daemon message on an attached stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
  Input(Vec<u8>),
  Resize { cols: u16, rows: u16 },
}

const FRAME_INPUT: u8 = 0;
const FRAME_RESIZE: u8 = 1;

impl Frame {
  /// One tag byte, a big-endian u32 length, then the payload.
  pub fn encode(&self) -> Vec<u8> {
    let (tag, payload) = match self {
      Self::Input(bytes) => (FRAME_INPUT, bytes.clone()),
      Self::Resize { cols, rows } => {
        let mut payload = cols.to_be_bytes().to_vec();
        payload.extend(rows.to_be_bytes());
        (FRAME_RESIZE, payload)
      }
    };
    let mut out = Vec::with_capacity(payload.len() + 5);
    out.push(tag);
    out.extend((payload.len() as u32).to_be_bytes());
    out.extend(payload);
    out
  }

  /// Reads one frame. `Ok(None)` at a clean end of stream.
  pub fn read(reader: &mut impl Read) -> std::io::Result<Option<Self>> {
    let mut header = [0u8; 5];
    match reader.read_exact(&mut header) {
      Ok(()) => {}
      Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
      Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    // Input arrives a keypress or a paste at a time; anything this large is
    // not a client of ours.
    if len > 16 * 1024 * 1024 {
      return Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "frame too large",
      ));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    match header[0] {
      FRAME_INPUT => Ok(Some(Self::Input(payload))),
      FRAME_RESIZE if payload.len() == 4 => Ok(Some(Self::Resize {
        cols: u16::from_be_bytes([payload[0], payload[1]]),
        rows: u16::from_be_bytes([payload[2], payload[3]]),
      })),
      _ => Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "unknown frame",
      )),
    }
  }
}

/// A terminal model sized `cols` x `rows` that keeps [`HISTORY_LINES`].
pub fn new_screen(cols: u16, rows: u16) -> vt100::Parser {
  vt100::Parser::new(nonzero(rows), nonzero(cols), HISTORY_LINES)
}

fn nonzero(value: u16) -> NonZeroU16 {
  NonZeroU16::new(value).unwrap_or(NonZeroU16::MIN)
}

/// The bytes that bring a freshly attached terminal to the session's state:
/// its history lines (scrolled up into the client's own scrollback), then
/// the current screen, cursor and input modes. A full-screen program on the
/// alternate screen has no history of its own, so only its screen is sent.
pub fn replay(parser: &mut vt100::Parser) -> Vec<u8> {
  let mut out = Vec::new();
  let screen = parser.screen_mut();
  if !screen.alternate_screen() {
    let (rows, cols) = screen.size();
    let rows = usize::from(rows.get());
    let total = screen.scrollback_len();
    let mut offset = total;
    while offset > 0 {
      // At scrollback offset `k` the view starts at history line
      // `total - k`, so its first `min(k, rows)` rows are history.
      screen.set_scrollback(offset);
      let take = offset.min(rows);
      for line in screen.rows_formatted(0, cols.get()).take(take) {
        out.extend(line.as_bytes());
        out.extend(b"\x1b[m\r\n");
      }
      offset -= take;
    }
    screen.set_scrollback(0);
    if total > 0 {
      // Push the history above the visible area before redrawing it.
      out.extend(b"\r\n".repeat(rows));
    }
  }
  out.extend(parser.screen().state_formatted().as_bytes());
  out
}

// -- Server -------------------------------------------------------------------

struct Session {
  master: Box<dyn portable_pty::MasterPty + Send>,
  writer: Box<dyn Write + Send>,
  child: Box<dyn portable_pty::Child + Send + Sync>,
  parser: vt100::Parser,
  clients: Vec<(u64, UnixStream)>,
  title: Option<String>,
  /// Unix seconds of the last output.
  last_output: Option<u64>,
}

impl Session {
  fn resize(&mut self, cols: u16, rows: u16) {
    if cols == 0 || rows == 0 {
      return;
    }
    let (cur_rows, cur_cols) = self.parser.screen().size();
    if cur_rows.get() == rows && cur_cols.get() == cols {
      return;
    }
    self
      .master
      .resize(portable_pty::PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
      })
      .ok();
    self.parser.set_size(nonzero(rows), nonzero(cols));
  }

  /// Ends the session's process group: SIGHUP as a closing terminal would,
  /// then SIGKILL for anything that ignores it.
  fn kill(&mut self) {
    if let Some(pid) = self.child.process_id() {
      let pgid = pid as libc::pid_t;
      // SAFETY: `killpg` takes plain integers and has no memory effects.
      // portable-pty starts each child in its own session, so its pid is
      // the process group id and this cannot reach the daemon itself.
      unsafe { libc::killpg(pgid, libc::SIGHUP) };
      std::thread::sleep(Duration::from_millis(50));
      // SAFETY: as above.
      unsafe { libc::killpg(pgid, libc::SIGKILL) };
    }
    self.child.kill().ok();
    for (_, client) in self.clients.drain(..) {
      client.shutdown(std::net::Shutdown::Both).ok();
    }
  }
}

#[derive(Default)]
struct State {
  sessions: HashMap<String, Session>,
  /// When the last session ended, for [`IDLE_EXIT`].
  idle_since: Option<Instant>,
}

struct Daemon {
  state: Mutex<State>,
  shutdown: AtomicBool,
  next_client: AtomicU64,
}

impl Daemon {
  fn lock(&self) -> MutexGuard<'_, State> {
    self.state.lock().unwrap_or_else(|e| e.into_inner())
  }
}

/// Creates the daemon's directory, readable by its owner only.
fn prepare_dir(paths: &DaemonPaths) -> Result<(), String> {
  use std::os::unix::fs::PermissionsExt;
  std::fs::create_dir_all(paths.dir())
    .map_err(|e| format!("Failed to create {}: {e}", paths.dir().display()))?;
  std::fs::set_permissions(paths.dir(), std::fs::Permissions::from_mode(0o700))
    .map_err(|e| format!("Failed to secure {}: {e}", paths.dir().display()))
}

/// Runs the daemon until a [`Request::Shutdown`] or [`IDLE_EXIT`] with no
/// sessions. Returns `Ok(false)` without serving when another daemon
/// already holds the lock.
pub fn serve(paths: &DaemonPaths) -> Result<bool, String> {
  serve_with_idle(paths, IDLE_EXIT)
}

pub fn serve_with_idle(paths: &DaemonPaths, idle_exit: Duration) -> Result<bool, String> {
  use std::os::unix::io::AsRawFd;
  prepare_dir(paths)?;
  let lock = std::fs::OpenOptions::new()
    .create(true)
    .truncate(false)
    .write(true)
    .open(paths.lock())
    .map_err(|e| format!("Failed to open the daemon lock: {e}"))?;
  // SAFETY: the fd belongs to `lock`, which outlives this call; flock only
  // reads it.
  if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
    return Ok(false);
  }
  let socket = paths.socket();
  // Holding the lock means any socket file left here is from a daemon that
  // has gone.
  std::fs::remove_file(&socket).ok();
  let listener =
    UnixListener::bind(&socket).map_err(|e| format!("Failed to bind {}: {e}", socket.display()))?;
  listener
    .set_nonblocking(true)
    .map_err(|e| format!("Failed to configure the daemon socket: {e}"))?;

  let daemon = Arc::new(Daemon {
    state: Mutex::new(State {
      idle_since: Some(Instant::now()),
      ..State::default()
    }),
    shutdown: AtomicBool::new(false),
    next_client: AtomicU64::new(0),
  });

  while !daemon.shutdown.load(Ordering::SeqCst) {
    match listener.accept() {
      Ok((stream, _)) => {
        stream.set_nonblocking(false).ok();
        let daemon = daemon.clone();
        std::thread::spawn(move || handle_connection(&daemon, stream));
      }
      Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
        std::thread::sleep(Duration::from_millis(25));
        let state = daemon.lock();
        if state
          .idle_since
          .is_some_and(|since| since.elapsed() >= idle_exit)
        {
          break;
        }
      }
      Err(e) => return Err(format!("Daemon socket failed: {e}")),
    }
  }

  let mut state = daemon.lock();
  for (_, mut session) in state.sessions.drain() {
    session.kill();
  }
  drop(state);
  std::fs::remove_file(&socket).ok();
  drop(lock);
  Ok(true)
}

fn write_reply(stream: &mut UnixStream, reply: &Reply) -> std::io::Result<()> {
  let mut line = serde_json::to_vec(reply).unwrap_or_default();
  line.push(b'\n');
  stream.write_all(&line)
}

fn ok(value: impl Serialize) -> Reply {
  Reply::Ok(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))
}

fn handle_connection(daemon: &Arc<Daemon>, stream: UnixStream) {
  let Ok(mut writer) = stream.try_clone() else {
    return;
  };
  let mut reader = BufReader::new(stream);
  let mut line = String::new();
  if reader.read_line(&mut line).unwrap_or(0) == 0 {
    return;
  }
  let request = match serde_json::from_str::<Request>(&line) {
    Ok(request) => request,
    Err(e) => {
      write_reply(&mut writer, &Reply::Error(format!("invalid_request: {e}"))).ok();
      return;
    }
  };
  let reply = match request {
    Request::Hello => ok(Hello {
      protocol: PROTOCOL_VERSION,
      version: env!("CARGO_PKG_VERSION").to_string(),
      sessions: daemon.lock().sessions.len(),
      pid: std::process::id(),
    }),
    Request::Start { name, spec } => match ensure_session(daemon, &name, &spec) {
      Ok(()) => ok(()),
      Err(e) => Reply::Error(e),
    },
    Request::List => {
      let mut listed: Vec<Listed> = daemon
        .lock()
        .sessions
        .iter()
        .map(|(name, session)| Listed {
          name: name.clone(),
          last_output: session.last_output,
          title: session.title.clone(),
        })
        .collect();
      listed.sort_by(|a, b| a.name.cmp(&b.name));
      ok(listed)
    }
    Request::Rename { name, title } => match daemon.lock().sessions.get_mut(&name) {
      Some(session) => {
        session.title = title;
        ok(())
      }
      None => Reply::Error(not_found(&name)),
    },
    Request::Stop { name } => {
      let removed = daemon.lock().sessions.remove(&name);
      if let Some(mut session) = removed {
        session.kill();
        mark_idle_if_empty(daemon);
      }
      ok(())
    }
    Request::Paste { name, text, submit } => match paste(daemon, &name, &text, submit) {
      Ok(()) => ok(()),
      Err(e) => Reply::Error(e),
    },
    Request::Resize { name, cols, rows } => {
      let mut state = daemon.lock();
      match state.sessions.get_mut(&name) {
        Some(session) => {
          session.resize(cols, rows);
          ok(())
        }
        None => Reply::Error(not_found(&name)),
      }
    }
    Request::Attach {
      name,
      cols,
      rows,
      start,
    } => {
      attach(daemon, &name, cols, rows, start.as_ref(), writer, reader);
      return;
    }
    Request::Shutdown => {
      let mut state = daemon.lock();
      for (_, mut session) in state.sessions.drain() {
        session.kill();
      }
      daemon.shutdown.store(true, Ordering::SeqCst);
      ok(())
    }
  };
  write_reply(&mut writer, &reply).ok();
}

fn not_found(name: &str) -> String {
  format!("session_not_found: {name} is not running")
}

fn mark_idle_if_empty(daemon: &Daemon) {
  let mut state = daemon.lock();
  if state.sessions.is_empty() {
    state.idle_since = Some(Instant::now());
  }
}

fn ensure_session(daemon: &Arc<Daemon>, name: &str, spec: &StartSpec) -> Result<(), String> {
  let mut state = daemon.lock();
  if state.sessions.contains_key(name) {
    return Ok(());
  }
  let cols = spec.cols.max(1);
  let rows = spec.rows.max(1);
  let pair = portable_pty::native_pty_system()
    .openpty(portable_pty::PtySize {
      rows,
      cols,
      pixel_width: 0,
      pixel_height: 0,
    })
    .map_err(|e| format!("Failed to open a terminal: {e}"))?;
  let quoted_dir = crate::core::remote::shell_quote(&spec.dir);
  let mut command = portable_pty::CommandBuilder::new("/bin/sh");
  command.args(["-c", &format!("cd {quoted_dir} && exec {}", spec.program)]);
  command.env("TERM", "xterm-256color");
  // `treq agent msg` on the host finds this session's mailbox by its name.
  command.env("TREQ_PTY_SESSION_ID", name);
  let child = pair
    .slave
    .spawn_command(command)
    .map_err(|e| format!("Failed to start the session: {e}"))?;
  drop(pair.slave);
  let mut reader = pair
    .master
    .try_clone_reader()
    .map_err(|e| format!("Failed to read the session: {e}"))?;
  let writer = pair
    .master
    .take_writer()
    .map_err(|e| format!("Failed to write to the session: {e}"))?;
  state.sessions.insert(
    name.to_string(),
    Session {
      master: pair.master,
      writer,
      child,
      parser: new_screen(cols, rows),
      clients: Vec::new(),
      title: None,
      last_output: None,
    },
  );
  state.idle_since = None;
  drop(state);

  let daemon = daemon.clone();
  let name = name.to_string();
  std::thread::spawn(move || {
    let mut buf = [0u8; 16 * 1024];
    loop {
      let n = match reader.read(&mut buf) {
        Ok(0) | Err(_) => break,
        Ok(n) => n,
      };
      let mut state = daemon.lock();
      let Some(session) = state.sessions.get_mut(&name) else {
        break;
      };
      session.parser.process(&buf[..n]);
      session.last_output = Some(unix_now());
      session
        .clients
        .retain_mut(|(_, client)| client.write_all(&buf[..n]).is_ok());
    }
    // The process exited (or the session was stopped): end its clients'
    // streams, which is how an attached terminal learns it is over.
    let ended = daemon.lock().sessions.remove(&name);
    if let Some(mut session) = ended {
      session.child.try_wait().ok();
      for (_, client) in session.clients.drain(..) {
        client.shutdown(std::net::Shutdown::Both).ok();
      }
    }
    mark_idle_if_empty(&daemon);
  });
  Ok(())
}

fn paste(daemon: &Daemon, name: &str, text: &str, submit: bool) -> Result<(), String> {
  {
    let mut state = daemon.lock();
    let session = state
      .sessions
      .get_mut(name)
      .ok_or_else(|| not_found(name))?;
    let mut bytes = Vec::with_capacity(text.len() + 12);
    // Bracket the paste only for a program that asked for it, as tmux's
    // `paste-buffer -p` does.
    let bracketed = session.parser.screen().bracketed_paste();
    if bracketed {
      bytes.extend(b"\x1b[200~");
    }
    bytes.extend(text.as_bytes());
    if bracketed {
      bytes.extend(b"\x1b[201~");
    }
    session
      .writer
      .write_all(&bytes)
      .map_err(|e| format!("Failed to send text: {e}"))?;
  }
  if submit {
    std::thread::sleep(SUBMIT_DELAY);
    let mut state = daemon.lock();
    let session = state
      .sessions
      .get_mut(name)
      .ok_or_else(|| not_found(name))?;
    session
      .writer
      .write_all(b"\r")
      .map_err(|e| format!("Failed to send Enter: {e}"))?;
  }
  Ok(())
}

fn attach(
  daemon: &Arc<Daemon>,
  name: &str,
  cols: u16,
  rows: u16,
  start: Option<&StartSpec>,
  mut writer: UnixStream,
  mut reader: BufReader<UnixStream>,
) {
  if let Some(spec) = start {
    if let Err(e) = ensure_session(daemon, name, spec) {
      write_reply(&mut writer, &Reply::Error(e)).ok();
      return;
    }
  }
  let id = daemon.next_client.fetch_add(1, Ordering::SeqCst);
  {
    let mut state = daemon.lock();
    let Some(session) = state.sessions.get_mut(name) else {
      drop(state);
      write_reply(&mut writer, &Reply::Error(not_found(name))).ok();
      return;
    };
    session.resize(cols, rows);
    // Reply and replay are written under the lock, so no output can land
    // between the replay and the live stream.
    let replayed = replay(&mut session.parser);
    if write_reply(&mut writer, &ok(())).is_err() || writer.write_all(&replayed).is_err() {
      return;
    }
    let Ok(client) = writer.try_clone() else {
      return;
    };
    client.set_write_timeout(Some(CLIENT_WRITE_TIMEOUT)).ok();
    session.clients.push((id, client));
  }

  loop {
    match Frame::read(&mut reader) {
      Ok(Some(Frame::Input(bytes))) => {
        let mut state = daemon.lock();
        let Some(session) = state.sessions.get_mut(name) else {
          break;
        };
        if session.writer.write_all(&bytes).is_err() {
          break;
        }
      }
      Ok(Some(Frame::Resize { cols, rows })) => {
        if let Some(session) = daemon.lock().sessions.get_mut(name) {
          session.resize(cols, rows);
        }
      }
      Ok(None) | Err(_) => break,
    }
  }
  if let Some(session) = daemon.lock().sessions.get_mut(name) {
    session.clients.retain(|(client, _)| *client != id);
  }
}

// -- Client -------------------------------------------------------------------

/// Connects to a running daemon. `Ok(None)` when none is running; this
/// never starts one, so listing or stopping on a host that has never used
/// the daemon leaves no process behind.
pub fn connect(paths: &DaemonPaths) -> Result<Option<UnixStream>, String> {
  match UnixStream::connect(paths.socket()) {
    Ok(stream) => Ok(Some(stream)),
    Err(e)
      if matches!(
        e.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
      ) =>
    {
      Ok(None)
    }
    Err(e) => Err(format!("Failed to reach the terminal daemon: {e}")),
  }
}

/// Sends one request on a fresh connection and returns its reply value.
pub fn call(stream: UnixStream, request: &Request) -> Result<serde_json::Value, String> {
  let (value, _) = call_keep(stream, request)?;
  Ok(value)
}

/// [`call`], also handing back the connection (positioned after the reply
/// line) for an attach stream.
pub fn call_keep(
  mut stream: UnixStream,
  request: &Request,
) -> Result<(serde_json::Value, BufReader<UnixStream>), String> {
  let mut line = serde_json::to_vec(request).map_err(|e| e.to_string())?;
  line.push(b'\n');
  stream
    .write_all(&line)
    .map_err(|e| format!("Failed to talk to the terminal daemon: {e}"))?;
  let mut reader = BufReader::new(stream);
  let mut reply = String::new();
  reader
    .read_line(&mut reply)
    .map_err(|e| format!("Failed to hear from the terminal daemon: {e}"))?;
  match serde_json::from_str::<Reply>(&reply) {
    Ok(Reply::Ok(value)) => Ok((value, reader)),
    Ok(Reply::Error(message)) => Err(message),
    Err(_) => Err("daemon_error: the terminal daemon closed the connection".to_string()),
  }
}

/// Sends `request` to a running daemon. `Ok(None)` when none is running.
pub fn request_if_running(
  paths: &DaemonPaths,
  request: &Request,
) -> Result<Option<serde_json::Value>, String> {
  match connect(paths)? {
    Some(stream) => call(stream, request).map(Some),
    None => Ok(None),
  }
}

/// Connects to the daemon, starting it (as `treq pty-remote daemon`) when
/// none is running, and checks it speaks [`PROTOCOL_VERSION`]. A daemon
/// from another version is restarted when it holds no sessions; with
/// sessions it would lose them, so the caller is told to reset instead.
pub fn connect_or_spawn(paths: &DaemonPaths) -> Result<UnixStream, String> {
  for _ in 0..2 {
    let stream = match connect(paths)? {
      Some(stream) => stream,
      None => {
        spawn_daemon(paths)?;
        wait_for_daemon(paths)?
      }
    };
    let hello: Hello = serde_json::from_value(call(stream, &Request::Hello)?)
      .map_err(|e| format!("daemon_error: unexpected hello from the terminal daemon: {e}"))?;
    if hello.protocol == PROTOCOL_VERSION {
      return connect(paths)?
        .ok_or_else(|| "daemon_error: the terminal daemon stopped".to_string());
    }
    if hello.sessions > 0 {
      return Err(format!(
        "daemon_version_mismatch: terminals on this host run under Treq {}; reset the machine's terminals to use this version",
        hello.version
      ));
    }
    request_if_running(paths, &Request::Shutdown)?;
    wait_for_exit(paths);
  }
  Err("daemon_error: could not start the terminal daemon".to_string())
}

fn spawn_daemon(paths: &DaemonPaths) -> Result<(), String> {
  use std::os::unix::process::CommandExt;
  prepare_dir(paths)?;
  let exe = std::env::current_exe().map_err(|e| format!("Failed to find treq: {e}"))?;
  let log = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(paths.log())
    .map_err(|e| format!("Failed to open {}: {e}", paths.log().display()))?;
  let log_err = log.try_clone().map_err(|e| e.to_string())?;
  let mut command = std::process::Command::new(exe);
  command
    .args(["pty-remote", "daemon", "--repo", "/"])
    .env("TREQ_PTY_DIR", paths.dir())
    .current_dir("/")
    .stdin(std::process::Stdio::null())
    .stdout(log)
    .stderr(log_err);
  // SAFETY: `setsid` is async-signal-safe and touches no memory; it moves
  // the daemon out of the SSH session, so it outlives the connection that
  // started it.
  unsafe {
    command.pre_exec(|| {
      libc::setsid();
      Ok(())
    });
  }
  command
    .spawn()
    .map(drop)
    .map_err(|e| format!("Failed to start the terminal daemon: {e}"))
}

fn wait_for_daemon(paths: &DaemonPaths) -> Result<UnixStream, String> {
  let deadline = Instant::now() + Duration::from_secs(5);
  while Instant::now() < deadline {
    if let Some(stream) = connect(paths)? {
      return Ok(stream);
    }
    std::thread::sleep(Duration::from_millis(25));
  }
  Err(format!(
    "daemon_error: the terminal daemon did not start; see {}",
    paths.log().display()
  ))
}

fn wait_for_exit(paths: &DaemonPaths) {
  let deadline = Instant::now() + Duration::from_secs(5);
  while Instant::now() < deadline && paths.socket().exists() {
    std::thread::sleep(Duration::from_millis(25));
  }
}

/// Stops the daemon and every session it holds, then clears its directory.
/// Succeeds when no daemon is running.
pub fn reset(paths: &DaemonPaths) -> Result<(), String> {
  if request_if_running(paths, &Request::Shutdown)?.is_some() {
    wait_for_exit(paths);
  }
  match std::fs::remove_dir_all(paths.dir()) {
    Ok(()) => Ok(()),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(e) => Err(format!("Failed to clear {}: {e}", paths.dir().display())),
  }
}

/// Bridges this process's terminal to an attached session until it ends:
/// what the SSH PTY channel runs (`treq pty-remote attach`). Stdin is put in
/// raw mode so keys pass through untouched, and size changes are forwarded
/// as [`Frame::Resize`].
pub fn run_attach(paths: &DaemonPaths, name: &str, start: StartSpec) -> Result<(), String> {
  let (cols, rows) = terminal_size().unwrap_or((start.cols, start.rows));
  let stream = connect_or_spawn(paths)?;
  let (_, mut reader) = call_keep(
    stream,
    &Request::Attach {
      name: name.to_string(),
      cols,
      rows,
      start: Some(start),
    },
  )?;
  let mut input = reader
    .get_ref()
    .try_clone()
    .map_err(|e| format!("Failed to attach: {e}"))?;
  let _raw = RawMode::enable();

  std::thread::spawn(move || {
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 4096];
    let mut size = (cols, rows);
    loop {
      // Wake every 200ms to notice a resize even while no key is pressed.
      if !stdin_ready(Duration::from_millis(200)) {
        if let Some(now) = terminal_size().filter(|now| *now != size) {
          size = now;
          let frame = Frame::Resize {
            cols: now.0,
            rows: now.1,
          };
          if input.write_all(&frame.encode()).is_err() {
            break;
          }
        }
        continue;
      }
      let n = match stdin.read(&mut buf) {
        Ok(0) | Err(_) => break,
        Ok(n) => n,
      };
      if input
        .write_all(&Frame::Input(buf[..n].to_vec()).encode())
        .is_err()
      {
        break;
      }
    }
    input.shutdown(std::net::Shutdown::Write).ok();
  });

  let mut stdout = std::io::stdout().lock();
  let mut buf = [0u8; 16 * 1024];
  loop {
    let n = match reader.read(&mut buf) {
      Ok(0) | Err(_) => break,
      Ok(n) => n,
    };
    if stdout
      .write_all(&buf[..n])
      .and_then(|()| stdout.flush())
      .is_err()
    {
      break;
    }
  }
  Ok(())
}

/// `true` when stdin has data or has closed within `timeout`.
fn stdin_ready(timeout: Duration) -> bool {
  let mut poll_fd = libc::pollfd {
    fd: libc::STDIN_FILENO,
    events: libc::POLLIN,
    revents: 0,
  };
  // SAFETY: `poll_fd` is a live `pollfd` and the count is 1.
  let ready = unsafe { libc::poll(&mut poll_fd, 1, timeout.as_millis() as i32) };
  ready > 0
}

fn terminal_size() -> Option<(u16, u16)> {
  // SAFETY: `winsize` is plain data; TIOCGWINSZ fills it or fails.
  let mut size: libc::winsize = unsafe { std::mem::zeroed() };
  // SAFETY: `size` is a live `winsize`, the struct TIOCGWINSZ writes.
  let ok = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, &mut size) } == 0;
  (ok && size.ws_col > 0 && size.ws_row > 0).then_some((size.ws_col, size.ws_row))
}

/// Raw mode on stdin for as long as it lives; restores the old settings.
struct RawMode(Option<libc::termios>);

impl RawMode {
  fn enable() -> Self {
    // SAFETY: `termios` is plain data that `tcgetattr` fills.
    let mut old: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `old` is a live `termios`.
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut old) } != 0 {
      return Self(None);
    }
    let mut raw = old;
    // SAFETY: `raw` is a live `termios` copied from the current settings.
    unsafe { libc::cfmakeraw(&mut raw) };
    // SAFETY: `raw` is a valid `termios`.
    unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) };
    Self(Some(old))
  }
}

impl Drop for RawMode {
  fn drop(&mut self) {
    if let Some(old) = &self.0 {
      // SAFETY: `old` holds the settings `tcgetattr` returned.
      unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, old) };
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A daemon serving from a fresh temporary directory on its own thread.
  struct TestDaemon {
    paths: DaemonPaths,
    handle: Option<std::thread::JoinHandle<Result<bool, String>>>,
    _dir: tempfile::TempDir,
  }

  impl TestDaemon {
    fn start() -> Self {
      // Unix socket paths are short; keep the directory near the root.
      let dir = tempfile::Builder::new()
        .prefix("tpty")
        .tempdir_in("/tmp")
        .unwrap();
      let paths = DaemonPaths::new(dir.path().join("pty"));
      let serve_paths = paths.clone();
      let handle =
        std::thread::spawn(move || serve_with_idle(&serve_paths, Duration::from_secs(600)));
      let deadline = Instant::now() + Duration::from_secs(5);
      while connect(&paths).unwrap().is_none() {
        assert!(Instant::now() < deadline, "daemon did not start");
        std::thread::sleep(Duration::from_millis(10));
      }
      Self {
        paths,
        handle: Some(handle),
        _dir: dir,
      }
    }

    fn call(&self, request: &Request) -> Result<serde_json::Value, String> {
      call(connect(&self.paths).unwrap().unwrap(), request)
    }

    fn start_session(&self, name: &str, program: &str) {
      self
        .call(&Request::Start {
          name: name.into(),
          spec: spec(program),
        })
        .unwrap();
    }

    fn attach(&self, name: &str) -> BufReader<UnixStream> {
      let (_, reader) = call_keep(
        connect(&self.paths).unwrap().unwrap(),
        &Request::Attach {
          name: name.into(),
          cols: 80,
          rows: 24,
          start: None,
        },
      )
      .unwrap();
      reader
    }

    fn names(&self) -> Vec<String> {
      let listed: Vec<Listed> = serde_json::from_value(self.call(&Request::List).unwrap()).unwrap();
      listed.into_iter().map(|l| l.name).collect()
    }
  }

  impl Drop for TestDaemon {
    fn drop(&mut self) {
      if let Some(handle) = self.handle.take() {
        request_if_running(&self.paths, &Request::Shutdown).ok();
        handle.join().ok();
      }
    }
  }

  fn spec(program: &str) -> StartSpec {
    StartSpec {
      dir: "/".into(),
      program: program.into(),
      cols: 80,
      rows: 24,
    }
  }

  /// Reads from `reader` until the text seen so far contains `needle`.
  fn read_until(reader: &mut impl Read, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    while !String::from_utf8_lossy(&seen).contains(needle) {
      assert!(
        Instant::now() < deadline,
        "never saw {needle:?}; got {:?}",
        String::from_utf8_lossy(&seen)
      );
      match reader.read(&mut buf) {
        Ok(0) => panic!(
          "stream ended before {needle:?}; got {:?}",
          String::from_utf8_lossy(&seen)
        ),
        Ok(n) => seen.extend(&buf[..n]),
        Err(e) => panic!("read failed: {e}"),
      }
    }
    String::from_utf8_lossy(&seen).into_owned()
  }

  fn send(reader: &BufReader<UnixStream>, frame: &Frame) {
    reader
      .get_ref()
      .try_clone()
      .unwrap()
      .write_all(&frame.encode())
      .unwrap();
  }

  #[test]
  fn frames_round_trip() {
    for frame in [
      Frame::Input(b"ls\r".to_vec()),
      Frame::Resize {
        cols: 132,
        rows: 43,
      },
    ] {
      let bytes = frame.encode();
      assert_eq!(Frame::read(&mut bytes.as_slice()).unwrap(), Some(frame));
    }
    assert_eq!(Frame::read(&mut [].as_slice()).unwrap(), None);
  }

  #[test]
  fn replay_carries_at_most_the_history_limit_and_the_current_screen() {
    let mut parser = new_screen(40, 5);
    for i in 0..400 {
      parser.process(format!("line {i}\r\n").as_bytes());
    }
    parser.process(b"prompt$ ");
    let replayed = replay(&mut parser);

    let mut client = vt100::Parser::new(nonzero(5), nonzero(40), 1000);
    client.process(&replayed);
    assert_eq!(client.screen().contents(), parser.screen().contents());
    assert_eq!(
      client.screen().cursor_position(),
      parser.screen().cursor_position()
    );

    // Lines 396..=399 and the prompt are on screen, so the 250 history
    // lines are 146..=395.
    let text = String::from_utf8_lossy(&replayed).into_owned();
    assert!(text.contains("line 146\x1b"), "oldest history line missing");
    assert!(text.contains("line 395\x1b"), "newest history line missing");
    assert!(
      !text.contains("line 145\x1b"),
      "replayed more than {HISTORY_LINES} lines"
    );
    assert_eq!(
      parser.screen().scrollback(),
      0,
      "left the session scrolled back"
    );
  }

  #[test]
  fn replay_of_a_full_screen_program_sends_only_its_screen() {
    let mut parser = new_screen(40, 5);
    parser.process(b"shell history\r\n".repeat(20).as_slice());
    parser.process(b"\x1b[?1049hTUI");
    let text = String::from_utf8_lossy(&replay(&mut parser)).into_owned();
    assert!(text.contains("TUI"));
    assert!(!text.contains("shell history"));
  }

  #[test]
  fn hello_reports_the_protocol_version() {
    let daemon = TestDaemon::start();
    let hello: Hello = serde_json::from_value(daemon.call(&Request::Hello).unwrap()).unwrap();
    assert_eq!(hello.protocol, PROTOCOL_VERSION);
    assert_eq!(hello.sessions, 0);
  }

  #[test]
  fn a_session_survives_its_client_and_replays_on_reattach() {
    let daemon = TestDaemon::start();
    daemon.start_session("s", "sh -c 'echo first-output; exec cat'");
    let mut first = daemon.attach("s");
    read_until(&mut first, "first-output");
    drop(first);

    let mut second = daemon.attach("s");
    read_until(&mut second, "first-output");
    send(&second, &Frame::Input(b"typed-after\r".to_vec()));
    read_until(&mut second, "typed-after");
    assert_eq!(daemon.names(), vec!["s".to_string()]);
  }

  #[test]
  fn a_session_knows_its_name_from_the_environment() {
    let daemon = TestDaemon::start();
    daemon.start_session(
      "treq-app-1-agent",
      "sh -c 'echo \"name=$TREQ_PTY_SESSION_ID.\"; exec cat'",
    );
    let mut reader = daemon.attach("treq-app-1-agent");
    read_until(&mut reader, "name=treq-app-1-agent.");
  }

  fn listed(daemon: &TestDaemon) -> Vec<Listed> {
    serde_json::from_value(daemon.call(&Request::List).unwrap()).unwrap()
  }

  #[test]
  fn list_reports_each_sessions_title_and_last_output() {
    let daemon = TestDaemon::start();
    let before = unix_now();
    daemon.start_session("s", "sh -c 'echo hi; exec cat'");
    let mut client = daemon.attach("s");
    read_until(&mut client, "hi");
    let sessions = listed(&daemon);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "s");
    assert_eq!(sessions[0].title, None);
    assert!(sessions[0].last_output.is_some_and(|at| at >= before));

    daemon
      .call(&Request::Rename {
        name: "s".into(),
        title: Some("Fix login".into()),
      })
      .unwrap();
    assert_eq!(listed(&daemon)[0].title.as_deref(), Some("Fix login"));
    daemon
      .call(&Request::Rename {
        name: "s".into(),
        title: None,
      })
      .unwrap();
    assert_eq!(listed(&daemon)[0].title, None);
  }

  #[test]
  fn renaming_a_missing_session_errors() {
    let daemon = TestDaemon::start();
    let error = daemon
      .call(&Request::Rename {
        name: "missing".into(),
        title: Some("x".into()),
      })
      .unwrap_err();
    assert!(error.starts_with("session_not_found"), "{error}");
  }

  #[test]
  fn start_is_idempotent() {
    let daemon = TestDaemon::start();
    daemon.start_session("s", "cat");
    daemon.start_session("s", "cat");
    assert_eq!(daemon.names(), vec!["s".to_string()]);
  }

  #[test]
  fn attach_without_start_to_a_missing_session_errors() {
    let daemon = TestDaemon::start();
    let error = call_keep(
      connect(&daemon.paths).unwrap().unwrap(),
      &Request::Attach {
        name: "missing".into(),
        cols: 80,
        rows: 24,
        start: None,
      },
    )
    .err()
    .unwrap();
    assert!(error.starts_with("session_not_found"), "{error}");
  }

  #[test]
  fn paste_reaches_the_session_and_submit_presses_enter() {
    let daemon = TestDaemon::start();
    daemon.start_session("s", "cat");
    let mut client = daemon.attach("s");
    daemon
      .call(&Request::Paste {
        name: "s".into(),
        text: "pasted text".into(),
        submit: true,
      })
      .unwrap();
    // `cat` echoes the line back once Enter sends it.
    let seen = read_until(&mut client, "pasted text\r\npasted text");
    assert!(
      !seen.contains("\x1b[200~"),
      "bracketed a paste nobody asked for"
    );
  }

  #[test]
  fn resize_reaches_the_program() {
    let daemon = TestDaemon::start();
    daemon.start_session(
      "s",
      "sh -c 'trap \"stty size\" WINCH; while :; do sleep 0.05; done'",
    );
    let mut client = daemon.attach("s");
    send(
      &client,
      &Frame::Resize {
        cols: 100,
        rows: 40,
      },
    );
    read_until(&mut client, "40 100");
  }

  #[test]
  fn the_stream_ends_when_the_program_exits() {
    let daemon = TestDaemon::start();
    daemon.start_session("s", "sh -c 'read line; echo bye'");
    let mut client = daemon.attach("s");
    send(&client, &Frame::Input(b"\r".to_vec()));
    read_until(&mut client, "bye");
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).ok();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !daemon.names().is_empty() {
      assert!(Instant::now() < deadline, "session outlived its program");
      std::thread::sleep(Duration::from_millis(20));
    }
  }

  #[test]
  fn stop_kills_the_whole_process_group() {
    let daemon = TestDaemon::start();
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pid");
    let pidfile_arg = crate::core::remote::shell_quote(&pidfile.to_string_lossy());
    daemon.start_session(
      "s",
      &format!("sh -c 'sleep 300 & echo $! > '{pidfile_arg}'; echo started; wait'"),
    );
    let mut client = daemon.attach("s");
    read_until(&mut client, "started");
    let pid: libc::pid_t = std::fs::read_to_string(&pidfile)
      .unwrap()
      .trim()
      .parse()
      .unwrap();

    daemon.call(&Request::Stop { name: "s".into() }).unwrap();
    assert!(daemon.names().is_empty());
    let deadline = Instant::now() + Duration::from_secs(5);
    // SAFETY: signal 0 only checks whether `pid` exists.
    while unsafe { libc::kill(pid, 0) } == 0 {
      assert!(
        Instant::now() < deadline,
        "the background child outlived stop"
      );
      std::thread::sleep(Duration::from_millis(20));
    }
  }

  #[test]
  fn reset_stops_every_session_and_the_daemon_and_clears_its_directory() {
    let mut daemon = TestDaemon::start();
    daemon.start_session("a", "cat");
    daemon.start_session("b", "cat");
    reset(&daemon.paths).unwrap();
    let handle = daemon.handle.take().unwrap();
    assert_eq!(handle.join().unwrap(), Ok(true));
    assert!(connect(&daemon.paths).unwrap().is_none());
    assert!(!daemon.paths.dir().exists());
  }

  #[test]
  fn reset_without_a_daemon_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    reset(&DaemonPaths::new(dir.path().join("pty"))).unwrap();
  }

  #[test]
  fn a_second_daemon_on_the_same_directory_stands_down() {
    let daemon = TestDaemon::start();
    assert_eq!(
      serve_with_idle(&daemon.paths, Duration::from_secs(1)),
      Ok(false)
    );
    // The first still answers.
    assert!(daemon.call(&Request::Hello).is_ok());
  }

  #[test]
  fn the_daemon_exits_after_idling_without_sessions() {
    let dir = tempfile::Builder::new()
      .prefix("tpty")
      .tempdir_in("/tmp")
      .unwrap();
    let paths = DaemonPaths::new(dir.path().join("pty"));
    assert_eq!(
      serve_with_idle(&paths, Duration::from_millis(100)),
      Ok(true)
    );
    assert!(!paths.socket().exists());
  }

  #[test]
  fn the_daemon_directory_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let daemon = TestDaemon::start();
    let mode = std::fs::metadata(daemon.paths.dir())
      .unwrap()
      .permissions()
      .mode();
    assert_eq!(mode & 0o777, 0o700);
  }
}
