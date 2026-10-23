//! The workspace terminal pane (`WorkspaceTerminalPane`): a row of agent
//! and shell terminals below the tab, each a column with its own header.
//!
//! The terminals run on PTYs GPUI starts itself ([`terminal_data`]) and
//! draw through a terminal emulator ([`terminal_emu`]). React still owns
//! which agent session is active and what a new one starts with: it says
//! so in the route, and hears back when the pane activates, closes or
//! starts a session (`terminal.*` fields).
//!
//! [`terminal_data`]: super::terminal_data
//! [`terminal_emu`]: super::terminal_emu

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::{zpx, DismissExt as _};
use crate::lock_ext::LockExt as _;
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use futures::StreamExt as _;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
  input::{Escape, Input, InputEvent, InputState},
  Sizable,
};
use serde::{Deserialize, Serialize};
use tauri_plugin_gpui::gpui::{
  self, anchored, deferred, div, img, linear_color_stop, linear_gradient, point, prelude::*, px,
  relative, App, Bounds, ClipboardItem, Context, Div, Entity, ExternalPaths, FocusHandle,
  KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, ScrollHandle,
  ScrollWheelEvent, SharedString, Stateful, Subscription, Task, Window,
};

use super::{
  code_data::{self, Environment, ENVIRONMENT_SETTING},
  environment,
  terminal_data::{self, AgentStart, Host, Listening, Live, RemoteTarget, Spawn},
  terminal_emu::{self, encode_key, paste_bytes, Key, Mouse, MouseAction, SelectKind},
  terminal_lightbox::{Lightbox, LightboxEvent},
  terminal_send::{assets_for, shell_quote, SendAsset},
  ActionSink, ScreenAction,
};
use crate::gpui_shell::{
  native::{NativeEvent, ToastTone},
  popup::{PopupSink, Side, TooltipContent},
  theme::{self, Palette},
  tooltips::{HasTooltips, Tooltips},
  ui::{self, TextSize},
};

/// `useTerminalPaneHeightResize`: the pane's share of the workspace view.
const HEIGHT_DEFAULT: f32 = 33.;
const HEIGHT_MIN: f32 = 15.;
const HEIGHT_MAX: f32 = 60.;
/// `h-1`: the divider above the pane, outside its share of the view.
const DIVIDER_HEIGHT: f32 = 4.;
/// `h-7`: a column's header.
const HEADER_HEIGHT: f32 = 28.;
/// A column never gets narrower than this share of the row (or 300px).
const MIN_COLUMN_SHARE: f32 = 0.4;
/// `onTerminalIdle` fires this long after the process last wrote.
const IDLE_AFTER: Duration = Duration::from_millis(2000);
/// The chat log snapshots the screen at most this often while it changes.
const CHAT_THROTTLE: Duration = Duration::from_secs(1);
/// xterm's cursor blink.
const BLINK: Duration = Duration::from_millis(600);
/// The scrollbar's width (`w-2`).
const SCROLLBAR: f32 = 8.;
/// `pt-1` above the grid.
const TOP_PAD: f32 = 4.;
/// Maximize/restore needs both Cmd and Ctrl: Super and Ctrl off macOS.
const MAXIMIZE_KEYS: &str = if cfg!(target_os = "macos") {
  "⌘ + Ctrl + J"
} else {
  "Super + Ctrl + J"
};
/// `RemoteTerminalPanel`'s xterm font size.
const REMOTE_FONT_SIZE: f32 = 13.;
/// `h-8`: a remote session's header.
const REMOTE_HEADER_HEIGHT: f32 = 32.;

/// A new session's start that React keeps (`pendingSessionData`), for the
/// sessions React still creates itself. Sessions the backend starts keep
/// theirs in `core::agent_launch` instead.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pending {
  /// The pane's id for the session.
  pub session_id: i64,
  pub pending_prompt: Option<String>,
  pub permission_mode: Option<String>,
  pub agent: Option<String>,
  pub workspace_path: Option<String>,
}

impl Pending {
  fn start(&self) -> super::launch_data::PendingSession {
    super::launch_data::PendingSession {
      pending_prompt: self.pending_prompt.clone(),
      permission_mode: self.permission_mode.clone(),
      agent: self.agent.clone(),
      workspace_path: self.workspace_path.clone(),
    }
  }
}

/// Something Dashboard asks of the pane (its imperative handle).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Command {
  /// Bumped per command, so each runs once.
  pub nonce: u64,
  /// `toggleCollapse`, `toggleMaximize`, `newShell`, `newAgent`,
  /// `closeWorkspace`, `send`, `openRemote`.
  pub kind: String,
  pub dir: Option<String>,
  pub session_id: Option<i64>,
  pub text: Option<String>,
  /// `openRemote`'s persistent session.
  pub remote: Option<RemoteTarget>,
}

/// What the route says about the pane while GPUI draws it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Route {
  /// The active agent session, while the workspace view shows.
  pub active_session_id: Option<i64>,
  pub pending: Vec<Pending>,
  /// Bumped when React's session list changes.
  pub sessions_revision: u64,
  /// Dashboard's latest commands; those newer than the last run go once.
  pub commands: Vec<Command>,
  /// A remote repository is active: a new shell runs on the host, in the
  /// session the window resolves (`resolveRemoteShell`).
  pub remote_shell: bool,
  /// The window's main repository, which pane session ids are relative to,
  /// while a supporting one is on screen. Empty: the one on screen.
  pub main_repo_path: String,
}

/// `TerminalSessionSummary`, which the sidebar spins on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
  pub id: String,
  pub kind: &'static str,
  pub name: String,
  pub branch_name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub repo_path: Option<String>,
  pub is_main_repo: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub agent: Option<String>,
  pub is_streaming: bool,
}

/// What the pane needs from the workspace around it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Place {
  pub repo_path: String,
  /// The selected workspace's directory, else the repository's.
  pub working_directory: String,
  pub current_branch: String,
  /// Workspace directories and their branches, for column labels.
  pub branches: Vec<(String, String)>,
  /// Workspace ids by directory, so a new agent opens in its column's.
  pub workspace_ids: Vec<(String, i64)>,
}

/// `scrollTerminalFullyIntoView`: how far the row scrolls right (negative:
/// left) for the column to be fully visible; a column wider than the row
/// lines its left edge up.
pub fn scroll_delta(row: (f32, f32), column: (f32, f32)) -> f32 {
  if column.1 - column.0 > row.1 - row.0 || column.0 < row.0 {
    column.0 - row.0
  } else if column.1 > row.1 {
    column.1 - row.1
  } else {
    0.
  }
}

/// `resizeTerminalPair`: moves the divider between two columns, neither
/// going below `min`.
pub fn resize_pair(left: f32, right: f32, delta: f32, min: f32) -> (f32, f32) {
  let next_left = (left + delta).max(min);
  let next_right = (right - (next_left - left)).max(min);
  (next_left, next_right)
}

/// The pane height for a drag to `y` in a view spanning `top..bottom`.
pub fn drag_height(top: f32, bottom: f32, y: f32) -> f32 {
  let height = bottom - top;
  if height <= 0. {
    return HEIGHT_DEFAULT;
  }
  ((bottom - y) / height * 100.).clamp(HEIGHT_MIN, HEIGHT_MAX)
}

/// The row's scrollbar thumb, as its left edge and width, for a row
/// `view` wide that scrolls `max` and is scrolled to `offset`.
pub fn scrollbar_thumb(view: f32, max: f32, offset: f32) -> (f32, f32) {
  let width = (view * view / (view + max)).clamp(32_f32.min(view), view);
  let left = if max > 0. {
    offset.clamp(0., max) / max * (view - width)
  } else {
    0.
  };
  (left, width)
}

/// The column to focus once `closed` goes: the one that takes its place,
/// else the one before.
pub fn neighbour(order: &[String], closed: &str) -> Option<String> {
  let index = order.iter().position(|id| id == closed)?;
  let rest: Vec<&String> = order.iter().filter(|id| *id != closed).collect();
  rest.get(index).or(rest.last()).map(|id| (*id).clone())
}

pub fn agent_column_id(pane_id: i64) -> String {
  format!("agent-{pane_id}")
}

/// xterm's cell: the font's advance and height, each rounded down to whole
/// device pixels.
pub fn cell_size(advance: f32, height: f32, scale: f32) -> (f32, f32) {
  let down = |v: f32| (v * scale).floor() / scale;
  (down(advance), down(height))
}

/// Where a glyph the font placed at `x` draws once each cell is `cell`
/// wide: at the start of its cell, as xterm's renderer puts it.
pub fn cell_x(x: f32, advance: f32, cell: f32) -> f32 {
  if advance <= 0. {
    return x;
  }
  (x / advance + 1e-3).floor() * cell
}

/// The key context of a focused terminal, which the shell's shortcuts read.
pub const TERMINAL_CONTEXT: &str = "Terminal";

/// A key press the pane (or the app) handles before a terminal sees it.
/// `Cmd` is the platform key or Ctrl, as `useTerminalPaneKeyboardShortcuts`
/// reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shortcut {
  ToggleCollapse,
  ToggleMaximize,
  NewAgent,
  NewShell,
  Close,
  /// Cmd+K, N, Shift+N or P: the app's, which the shell window forwards
  /// to React (`gpui_shell::shortcuts`), so the terminal leaves it.
  App(char),
}

pub fn shortcut(key: &str, ctrl: bool, platform: bool, shift: bool, alt: bool) -> Option<Shortcut> {
  if !(ctrl || platform) || alt {
    return None;
  }
  Some(match key {
    "j" if ctrl && platform => Shortcut::ToggleMaximize,
    "j" if !shift => Shortcut::ToggleCollapse,
    "]" if !shift => Shortcut::NewAgent,
    "\\" if !shift => Shortcut::NewShell,
    "w" if !shift => Shortcut::Close,
    "k" | "n" | "p" if !shift => Shortcut::App(key.chars().next()?),
    "n" => Shortcut::App('N'),
    _ => return None,
  })
}

/// What a terminal starts.
#[derive(Debug, Clone, PartialEq)]
pub enum Start {
  Shell {
    dir: String,
  },
  Agent {
    start: AgentStart,
    name: String,
    workspace_id: Option<i64>,
    working_dir: Option<String>,
  },
  /// A persistent session on a remote host (`RemoteTerminalPanel`).
  Remote {
    target: RemoteTarget,
  },
}

/// How a remote session's channel stands (`RemoteTerminalPanel`'s state).
#[derive(Debug, Clone, Default)]
pub struct RemoteState {
  /// Attaches after the first (`attachCount`).
  attaches: u32,
  /// The session is gone on the host, with its exit status.
  pub ended: Option<Option<u32>>,
  /// The channel closed but the session may still run.
  detached: bool,
  /// The channel closed: typing goes nowhere.
  closed: bool,
  pub stopping: bool,
  /// The automatic reattach for this drop was used; output from the
  /// session gives it back, so every drop gets one before Reattach is
  /// offered.
  auto_spent: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Status {
  Preparing,
  Ready,
  Failed(String),
  /// `prepareError`: the agent's command could not be built.
  AgentFailed(String),
}

/// What a terminal tells its pane.
#[derive(Debug, Clone, PartialEq)]
pub enum TermEvent {
  Focused,
  Output { from_process: bool },
  Idle,
  Close,
  Toast { title: String, description: String },
}

/// `TerminalSearchOverlay`.
#[derive(Default)]
struct Search {
  open: bool,
  query: String,
  input: Option<Entity<InputState>>,
  focus: bool,
  _subscription: Option<Subscription>,
}

/// How a terminal tells its pane what happened.
type OnEvent = Rc<dyn Fn(TermEvent, &mut App)>;

/// One terminal: the grid of a PTY's output, typed into while focused.
pub struct TerminalView {
  pub pty: String,
  host: Host,
  start: Start,
  live: Option<Arc<Live>>,
  status: Status,
  /// The size last given to the PTY, which follows the grid once it runs.
  pty_size: Option<(usize, usize)>,
  focus: FocusHandle,
  font_size: f32,
  /// The cell size, once measured.
  cell: Option<(f32, f32)>,
  /// The font's own advance, which the cell width rounds down.
  advance: f32,
  pub dark: bool,
  bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
  /// Wheel movement not yet a whole line.
  scroll_rest: f32,
  selecting: bool,
  /// The cell of the last mouse report, so a move within it sends none.
  reported: Option<(usize, usize)>,
  cursor_on: bool,
  idle: u64,
  chat_due: bool,
  search: Search,
  tooltips: Tooltips,
  on_event: OnEvent,
  pub remote: RemoteState,
  _exit: Option<Listening>,
  _tasks: Vec<Task<()>>,
}

fn now_millis() -> u128 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis())
    .unwrap_or_default()
}

impl HasTooltips for TerminalView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tooltips
  }
}

impl TerminalView {
  pub fn new(
    pty: String,
    start: Start,
    host: Host,
    (font_size, dark): (f32, bool),
    popups: PopupSink,
    on_event: OnEvent,
    cx: &mut Context<Self>,
  ) -> Self {
    Self {
      pty,
      host,
      start,
      live: None,
      status: Status::Preparing,
      pty_size: None,
      focus: cx.focus_handle(),
      font_size,
      cell: None,
      advance: 0.,
      dark,
      bounds: Rc::default(),
      scroll_rest: 0.,
      selecting: false,
      reported: None,
      cursor_on: true,
      idle: 0,
      chat_due: false,
      search: Search::default(),
      tooltips: Tooltips::new(popups, dark),
      on_event,
      remote: RemoteState::default(),
      _exit: None,
      _tasks: Vec::new(),
    }
  }

  pub fn is_agent(&self) -> bool {
    matches!(self.start, Start::Agent { .. })
  }

  fn remote_target(&self) -> Option<&RemoteTarget> {
    match &self.start {
      Start::Remote { target } => Some(target),
      _ => None,
    }
  }

  /// Listens for the remote channel's end before attaching, as
  /// `RemoteTerminalPanel` subscribes first.
  fn watch_exit(&mut self, cx: &mut Context<Self>) {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    self._exit = Some(self.host.listen_exit(
      &self.pty,
      Box::new(move |status| {
        let _ = tx.unbounded_send(status);
      }),
    ));
    self._tasks.push(cx.spawn(async move |this, cx| {
      if let Some(status) = rx.next().await {
        let _ = this.update(cx, |view, cx| view.on_remote_exit(status, cx));
      }
    }));
  }

  /// `handleChannelExit`: the session outlives its channel, so the host is
  /// asked whether the session still runs.
  fn on_remote_exit(&mut self, status: Option<u32>, cx: &mut Context<Self>) {
    self.remote.closed = true;
    let Some(target) = self.remote_target().cloned() else {
      return;
    };
    if self.remote.stopping {
      return;
    }
    let host = self.host.clone();
    let task = cx
      .background_executor()
      .spawn(async move { host.remote_running(&target) });
    self._tasks.push(cx.spawn(async move |this, cx| {
      let running = task.await;
      let _ = this.update(cx, |view, cx| {
        if view.remote.stopping {
          return;
        }
        match running {
          Some(false) => {
            view.remote.ended = Some(status);
            cx.notify();
          }
          _ => view.lost(None, cx),
        }
      });
    }));
  }

  /// The channel dropped or failed: reattaches once on its own, then
  /// shows the error (or the lost channel) with Reattach and Close.
  fn lost(&mut self, error: Option<String>, cx: &mut Context<Self>) {
    if !self.remote.auto_spent {
      self.remote.auto_spent = true;
      self.reattach(cx);
      return;
    }
    match error {
      Some(error) => self.status = Status::Failed(error),
      None => self.remote.detached = true,
    }
    cx.notify();
  }

  /// "Reattach": a fresh local id on the same persistent session, the old
  /// channel closed.
  pub fn reattach(&mut self, cx: &mut Context<Self>) {
    let Some(target) = self.remote_target() else {
      return;
    };
    let next = terminal_data::remote_pty_id(target, now_millis());
    let previous = std::mem::replace(&mut self.pty, next);
    let _ = self.host.close_pty(&previous);
    terminal_data::detach(&previous);
    rekey_kept(&previous, &self.pty);
    self.live = None;
    self._exit = None;
    self._tasks.clear();
    self.status = Status::Preparing;
    self.remote.attaches += 1;
    self.remote.ended = None;
    self.remote.detached = false;
    self.remote.closed = false;
    cx.notify();
  }

  /// "Stop": ends the session on the host, then closes the column.
  pub fn stop(&mut self, cx: &mut Context<Self>) {
    let Some(target) = self.remote_target().cloned() else {
      return;
    };
    self.remote.stopping = true;
    cx.notify();
    let host = self.host.clone();
    let task = cx
      .background_executor()
      .spawn(async move { host.remote_stop(&target) });
    self._tasks.push(cx.spawn(async move |this, cx| {
      let result = task.await;
      let _ = this.update(cx, |view, cx| {
        view.remote.stopping = false;
        match result {
          Ok(()) => (view.on_event)(TermEvent::Close, cx),
          Err(error) => view.status = Status::Failed(error),
        }
        cx.notify();
      });
    }));
  }

  /// The grid's size for the measured bounds.
  fn grid_size(&self) -> Option<(usize, usize)> {
    let bounds = self.bounds.get()?;
    let (w, h) = self.cell?;
    let cols = ((f32::from(bounds.size.width) - SCROLLBAR) / w)
      .floor()
      .max(2.) as usize;
    let rows = ((f32::from(bounds.size.height) - TOP_PAD) / h)
      .floor()
      .max(1.) as usize;
    Some((cols, rows))
  }

  fn measure(&mut self, window: &Window, cx: &App) {
    if self.cell.is_some() {
      return;
    }
    let system = cx.text_system();
    let font = system.resolve_font(&gpui::font(theme::MONO_FONT));
    let size = px(self.font_size);
    let advance = system
      .advance(font, size, 'm')
      .map(|a| f32::from(a.width))
      .unwrap_or(self.font_size * 0.6);
    let height = f32::from(system.ascent(font, size)) + f32::from(system.descent(font, size)).abs();
    self.advance = advance;
    self.cell = Some(cell_size(advance, height, window.scale_factor()));
  }

  /// Attaches to the PTY once the grid's size is known, starting it if no
  /// PTY runs with the id; afterwards keeps the PTY's size in step.
  fn sync(&mut self, cx: &mut Context<Self>) {
    let Some((cols, rows)) = self.grid_size() else {
      return;
    };
    match self.live.clone() {
      Some(live) => {
        if let Ok(mut emulator) = live.emulator.lock() {
          emulator.resize(cols, rows);
        }
        // A resize while the PTY starts reaches it once it runs; otherwise
        // the program draws for a size the grid isn't.
        if self.status == Status::Ready && self.pty_size != Some((cols, rows)) {
          let _ = self.host.resize_pty(&self.pty, rows as u16, cols as u16);
          self.pty_size = Some((cols, rows));
        }
      }
      None => {
        let (live, _) = terminal_data::attach(&self.host, &self.pty, cols, rows);
        if let Ok(mut emulator) = live.emulator.lock() {
          emulator.resize(cols, rows);
        }
        self.watch(&live, cx);
        self.live = Some(live);
        self.launch(cols, rows, cx);
      }
    }
  }

  fn watch(&mut self, live: &Arc<Live>, cx: &mut Context<Self>) {
    let mut rx = live.watch();
    self._tasks.push(cx.spawn(async move |this, cx| {
      while let Some(mut from_process) = rx.next().await {
        // A burst of output draws once.
        while let Ok(more) = rx.try_recv() {
          from_process |= more;
        }
        if this
          .update(cx, |view, cx| view.on_output(from_process, cx))
          .is_err()
        {
          break;
        }
      }
    }));
    if self._tasks.len() == 1 {
      self._tasks.push(cx.spawn(async move |this, cx| loop {
        cx.background_executor().timer(BLINK).await;
        if this
          .update(cx, |view, cx| {
            view.cursor_on = !view.cursor_on;
            cx.notify();
          })
          .is_err()
        {
          break;
        }
      }));
    }
  }

  fn on_output(&mut self, from_process: bool, cx: &mut Context<Self>) {
    (self.on_event)(TermEvent::Output { from_process }, cx);
    if from_process {
      // The channel works again, so its next drop gets its own attempt.
      self.remote.auto_spent = false;
      self.idle += 1;
      let idle = self.idle;
      cx.spawn(async move |this, cx| {
        cx.background_executor().timer(IDLE_AFTER).await;
        let _ = this.update(cx, |view, cx| {
          if view.idle == idle {
            view.chat_due = false;
            view.record_chat();
            (view.on_event)(TermEvent::Idle, cx);
          }
        });
      })
      .detach();
      if !self.chat_due && self.is_agent() {
        self.chat_due = true;
        cx.spawn(async move |this, cx| {
          cx.background_executor().timer(CHAT_THROTTLE).await;
          let _ = this.update(cx, |view, _| {
            if std::mem::take(&mut view.chat_due) {
              view.record_chat();
            }
          });
        })
        .detach();
      }
    }
    cx.notify();
  }

  /// `agentChatRecorder`: snapshots the agent's screen into its chat log.
  fn record_chat(&self) {
    let (Start::Agent { start, .. }, Some(live)) = (&self.start, &self.live) else {
      return;
    };
    let (host, repo, id, screen) = (
      self.host.clone(),
      start.repo_path.clone(),
      start.session_id,
      live.screen_text(),
    );
    std::thread::spawn(move || {
      if let Err(error) = host.record_chat_screen(&repo, id, &screen) {
        log::warn!("record agent chat failed: {error}");
      }
    });
  }

  /// Starts the PTY (preparing an agent's command first) unless it runs.
  fn launch(&mut self, cols: usize, rows: usize, cx: &mut Context<Self>) {
    let (host, pty, start) = (self.host.clone(), self.pty.clone(), self.start.clone());
    self.status = Status::Preparing;
    if matches!(start, Start::Remote { .. }) {
      self.watch_exit(cx);
    }
    let task = cx.background_executor().spawn(async move {
      if host.pty_exists(&pty) {
        let _ = host.resize_pty(&pty, rows as u16, cols as u16);
        return Ok(None);
      }
      let (spawn, warning) = match &start {
        Start::Remote { target } => {
          host
            .remote_attach(&pty, target, cols as u16, rows as u16)
            .map_err(|e| (false, e))?;
          return Ok(None);
        }
        Start::Shell { dir } => (
          Spawn {
            id: pty.clone(),
            working_dir: Some(dir.clone()),
            auto_command: host.shell_command(),
            ..Spawn::default()
          },
          None,
        ),
        Start::Agent {
          start,
          name,
          workspace_id,
          working_dir,
        } => {
          if let Err(error) = host.register_chat(start, &pty, name) {
            log::warn!("register agent chat failed: {error}");
          }
          let prepared = host.prepare_agent(start).map_err(|e| (true, e))?;
          (
            Spawn {
              id: pty.clone(),
              repo_path: Some(start.repo_path.clone()),
              workspace_id: *workspace_id,
              working_dir: working_dir.clone(),
              auto_command: Some(prepared.command),
            },
            prepared.skill_write_warning,
          )
        }
      };
      host.spawn_pty(&spawn).map_err(|e| (false, e))?;
      let _ = host.resize_pty(&pty, rows as u16, cols as u16);
      Ok(warning)
    });
    self._tasks.push(cx.spawn(async move |this, cx| {
      let result: Result<Option<String>, (bool, String)> = task.await;
      let _ = this.update(cx, |view, cx| {
        view.pty_size = Some((cols, rows));
        if let (Err((false, error)), Some(_)) = (&result, view.remote_target()) {
          view.lost(Some(error.clone()), cx);
          return;
        }
        view.status = match result {
          Ok(warning) => {
            if let Some(description) = warning {
              (view.on_event)(
                TermEvent::Toast {
                  title: "Could not write Treq skills".into(),
                  description,
                },
                cx,
              );
            }
            Status::Ready
          }
          Err((true, error)) => Status::AgentFailed(error),
          Err((false, error)) if error.contains("Session not found") => Status::Failed(
            "Terminal session is still initializing. Please wait a moment and try again.".into(),
          ),
          Err((false, error)) => Status::Failed(error),
        };
        cx.notify();
      });
    }));
  }

  /// "Try again": closes the PTY and starts over.
  fn retry(&mut self, cx: &mut Context<Self>) {
    let _ = self.host.close_pty(&self.pty);
    terminal_data::detach(&self.pty);
    self.live = None;
    self._tasks.clear();
    self.status = Status::Preparing;
    cx.notify();
  }

  /// `writeAgentMessage`: types `text` into the agent, noting it in the
  /// session's chat log.
  pub fn send_message(&self, text: &str) {
    if let (Start::Agent { start, .. }, Some(live)) = (&self.start, &self.live) {
      let (host, repo, id, screen, message) = (
        self.host.clone(),
        start.repo_path.clone(),
        start.session_id,
        live.screen_text(),
        text.to_string(),
      );
      std::thread::spawn(move || {
        if let Err(error) = host.record_chat_message(&repo, id, &screen, &message) {
          log::warn!("record agent message failed: {error}");
        }
      });
    }
    if let Err(error) = self.host.write_pty(&self.pty, &format!("{text}\r")) {
      log::warn!("pty write failed: {error}");
    }
  }

  /// Text dropped on the terminal (`handleTerminalDrop`), once it runs.
  fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
    // React's remote panel takes no drops.
    if self.status == Status::Ready && self.remote_target().is_none() {
      // Pasted, as a native terminal does with a drop.
      let bracketed = self.with_emulator(|e| e.bracketed_paste()).unwrap_or(false);
      let bytes = paste_bytes(text, bracketed);
      self.write(&String::from_utf8_lossy(&bytes), cx);
    }
  }

  pub fn write(&mut self, data: &str, cx: &mut Context<Self>) {
    // A remote channel takes nothing until it is attached (`isReadyRef`).
    if self.remote_target().is_some() && (self.status != Status::Ready || self.remote.closed) {
      return;
    }
    if let Some(live) = &self.live {
      live.typed(data);
      if let Ok(mut emulator) = live.emulator.lock() {
        emulator.scroll_to_bottom();
      }
    }
    if let Err(error) = self.host.write_pty(&self.pty, data) {
      log::warn!("pty write failed: {error}");
      // A remote panel shows the failure (`remotePtyWrite(...).catch(handleError)`).
      if self.remote_target().is_some() {
        self.lost(Some(error), cx);
      }
    }
  }

  fn with_emulator<T>(&self, f: impl FnOnce(&mut terminal_emu::Emulator) -> T) -> Option<T> {
    let live = self.live.as_ref()?;
    let mut emulator = live.emulator.lock().ok()?;
    Some(f(&mut emulator))
  }

  fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
    let k = &event.keystroke;
    let m = crate::gpui_shell::keys::held(k.modifiers);
    if shortcut(&k.key, m.control, m.platform, m.shift, m.alt).is_some() {
      // The pane's, or the app's: let it go on.
      cx.propagate();
      return;
    }
    // Keys typed into the search input are the input's.
    if self.status != Status::Ready || !self.focus.is_focused(window) {
      return;
    }
    if k.key == "f" && (m.platform || m.control) && !m.shift && self.is_agent() {
      self.open_search(cx);
      return;
    }
    let copy = (m.platform && k.key == "c") || (m.control && m.shift && k.key == "c");
    if copy {
      if let Some(text) = self.with_emulator(|e| e.selection_text()).flatten() {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        return;
      }
      if m.platform {
        return;
      }
    }
    let paste = (m.platform && k.key == "v") || (m.control && m.shift && k.key == "v");
    if paste {
      let item = cx.read_from_clipboard();
      // A copied image goes in as the path of a file holding it: Claude Code
      // and Codex attach a pasted image path, and nothing else reaches them
      // through a PTY. A remote host can't read a local file.
      let text = item.as_ref().and_then(|item| item.text()).or_else(|| {
        let image = item?.into_entries().find_map(|entry| match entry {
          gpui::ClipboardEntry::Image(image) => Some(image),
          _ => None,
        })?;
        if self.remote_target().is_some() {
          return None;
        }
        self
          .host
          .write_pasted_image(&image.bytes)
          .map(|path| shell_quote(&path))
          .inspect_err(|e| tracing::warn!("failed to save pasted image: {e}"))
          .ok()
      });
      if let Some(text) = text {
        let bracketed = self.with_emulator(|e| e.bracketed_paste()).unwrap_or(false);
        let bytes = paste_bytes(&text, bracketed);
        self.write(&String::from_utf8_lossy(&bytes), cx);
      }
      return;
    }
    if m.platform && k.key == "a" {
      self.with_emulator(|e| e.select_all());
      cx.notify();
      return;
    }
    // Shift+Enter continues the line, as React's terminal did.
    if k.key == "enter" && m.shift && !m.control && !m.alt && !m.platform {
      self.write("\\", cx);
      return;
    }
    let app_cursor = self.with_emulator(|e| e.app_cursor()).unwrap_or(false);
    let bytes = encode_key(
      Key {
        key: &k.key,
        key_char: k.key_char.as_deref(),
        ctrl: m.control,
        alt: m.alt,
        shift: m.shift,
        platform: m.platform,
      },
      app_cursor,
    );
    if let Some(bytes) = bytes {
      self.with_emulator(|e| e.clear_selection());
      self.write(&String::from_utf8_lossy(&bytes), cx);
      self.cursor_on = true;
      cx.stop_propagation();
    }
  }

  /// The cell under a window position, and whether it is the cell's right half.
  fn cell_at(&self, position: gpui::Point<Pixels>) -> Option<(usize, usize, bool)> {
    let bounds = self.bounds.get()?;
    let (w, h) = self.cell?;
    let x = (f32::from(position.x) - f32::from(bounds.origin.x)).max(0.);
    let y = (f32::from(position.y) - f32::from(bounds.origin.y) - TOP_PAD).max(0.);
    let (cols, rows) = self.grid_size()?;
    let col = ((x / w) as usize).min(cols - 1);
    let row = ((y / h) as usize).min(rows - 1);
    Some((row, col, x - col as f32 * w > w / 2.))
  }

  /// Sends a mouse event to the program, as xterm does once the program
  /// turns mouse reporting on; returns whether it did. Shift (Linux and
  /// Windows) leaves the mouse to selection, as in xterm.
  fn report(
    &mut self,
    button: terminal_emu::Button,
    action: MouseAction,
    position: gpui::Point<Pixels>,
    modifiers: gpui::Modifiers,
  ) -> bool {
    if self.status != Status::Ready
      || !self.with_emulator(|e| e.mouse_reporting()).unwrap_or(false)
      || (!cfg!(target_os = "macos") && modifiers.shift)
    {
      return false;
    }
    let Some((row, col, _)) = self.cell_at(position) else {
      return true;
    };
    if action == MouseAction::Move && self.reported == Some((row, col)) {
      return true;
    }
    self.reported = Some((row, col));
    let mouse = Mouse {
      button,
      action,
      row,
      col,
      shift: modifiers.shift,
      alt: modifiers.alt,
      ctrl: modifiers.control,
    };
    if let Some(bytes) = self.with_emulator(|e| e.encode_mouse(mouse)).flatten() {
      let _ = self
        .host
        .write_pty(&self.pty, &String::from_utf8_lossy(&bytes));
    }
    true
  }

  fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
    window.focus(&self.focus, cx);
    (self.on_event)(TermEvent::Focused, cx);
    if let Some(button) = mouse_button(event.button) {
      if self.report(button, MouseAction::Press, event.position, event.modifiers) {
        return;
      }
    }
    if event.button != MouseButton::Left {
      return;
    }
    let Some((row, col, right)) = self.cell_at(event.position) else {
      return;
    };
    let kind = match event.click_count {
      2 => SelectKind::Word,
      n if n >= 3 => SelectKind::Line,
      _ => SelectKind::Simple,
    };
    self.with_emulator(|e| {
      e.clear_find();
      if kind == SelectKind::Simple {
        e.clear_selection();
      }
      e.select_start(row, col, right, kind);
    });
    self.selecting = true;
    cx.notify();
  }

  fn on_mouse_up(&mut self, event: &gpui::MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
    self.selecting = false;
    if let Some(button) = mouse_button(event.button) {
      self.report(
        button,
        MouseAction::Release,
        event.position,
        event.modifiers,
      );
    }
  }

  fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
    let button = match event.pressed_button {
      Some(pressed) => mouse_button(pressed),
      None => Some(terminal_emu::Button::None),
    };
    if !self.selecting {
      if let Some(button) = button {
        if self.report(button, MouseAction::Move, event.position, event.modifiers) {
          return;
        }
      }
    }
    if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
      return;
    }
    if let Some((row, col, right)) = self.cell_at(event.position) {
      self.with_emulator(|e| e.select_extend(row, col, right));
      cx.notify();
    }
  }

  fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
    let Some((_, h)) = self.cell else {
      return;
    };
    let delta = event.delta.pixel_delta(px(h));
    // Horizontal swipes scroll the row of columns instead.
    if f32::from(delta.x).abs() > f32::from(delta.y).abs() {
      return;
    }
    self.scroll_rest += f32::from(delta.y) / h;
    let lines = self.scroll_rest.trunc() as i32;
    if lines == 0 {
      cx.stop_propagation();
      return;
    }
    self.scroll_rest -= lines as f32;
    let wheel = if lines > 0 {
      terminal_emu::Button::WheelUp
    } else {
      terminal_emu::Button::WheelDown
    };
    // xterm reports one wheel event, however far it went.
    self.reported = None;
    if self.report(wheel, MouseAction::Press, event.position, event.modifiers) {
      cx.stop_propagation();
      return;
    }
    let alt = self.with_emulator(|e| e.alt_screen()).unwrap_or(false);
    if alt {
      // Full-screen programs scroll by arrow keys, as xterm sends them.
      let app_cursor = self.with_emulator(|e| e.app_cursor()).unwrap_or(false);
      let key = if lines > 0 { "up" } else { "down" };
      let bytes = encode_key(
        Key {
          key,
          ..Key::default()
        },
        app_cursor,
      )
      .unwrap_or_default();
      let data = String::from_utf8_lossy(&bytes).repeat(lines.unsigned_abs() as usize);
      let _ = self.host.write_pty(&self.pty, &data);
    } else {
      self.with_emulator(|e| e.scroll(lines));
    }
    cx.stop_propagation();
    cx.notify();
  }

  pub fn open_search(&mut self, cx: &mut Context<Self>) {
    self.search.open = true;
    self.search.focus = true;
    cx.notify();
  }

  fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.search.open = false;
    self.search.query.clear();
    if let Some(input) = &self.search.input {
      input.update(cx, |input, cx| input.set_value("", window, cx));
    }
    self.with_emulator(|e| e.clear_find());
    window.focus(&self.focus, cx);
    cx.notify();
  }

  fn find(&mut self, forward: bool, cx: &mut Context<Self>) {
    let query = self.search.query.clone();
    if query.trim().is_empty() {
      return;
    }
    self.with_emulator(|e| e.find(&query, forward));
    cx.notify();
  }

  fn ensure_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if !self.search.open {
      return;
    }
    let input = match &self.search.input {
      Some(input) => input.clone(),
      None => {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        self.search._subscription = Some(cx.subscribe(
          &input,
          |view, input, event: &InputEvent, cx| match event {
            InputEvent::Change => {
              view.search.query = input.read(cx).value().to_string();
              cx.notify();
            }
            InputEvent::PressEnter { shift, .. } => view.find(!*shift, cx),
            _ => {}
          },
        ));
        self.search.input = Some(input.clone());
        input
      }
    };
    if std::mem::take(&mut self.search.focus) {
      input.update(cx, |input, cx| input.focus(window, cx));
    }
  }

  fn search_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let input = self.search.input.clone().filter(|_| self.search.open)?;
    let blank = self.search.query.trim().is_empty();
    let (muted, fg, accent) = (p.muted_foreground, p.foreground, p.accent);
    let bg = p.background;
    let button = |id: &'static str, icon: &'static str, disabled: bool| {
      div()
        .id(id)
        .debug_selector(move || id.into())
        .size_5()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .bg(bg)
        .text_color(muted)
        .when(disabled, |b| b.opacity(0.4))
        .when(!disabled, |b| {
          b.cursor_pointer()
            .hover(move |s| s.bg(accent).text_color(fg))
        })
        .child(ui::icon(icon, 16., muted))
    };
    let tip = |text: &'static str| Some(TooltipContent::Text(text.into()));
    let previous = button("terminal-find-previous", theme::ICON_CHEVRON_UP, blank)
      .when(!blank, |b| {
        b.on_click(cx.listener(|view, _, _, cx| view.find(false, cx)))
      });
    let next = button("terminal-find-next", theme::ICON_CHEVRON_DOWN, blank).when(!blank, |b| {
      b.on_click(cx.listener(|view, _, _, cx| view.find(true, cx)))
    });
    let close = button("terminal-close-search", theme::ICON_X, false)
      .on_click(cx.listener(|view, _, window, cx| view.close_search(window, cx)));
    let pty = self.pty.clone();
    let previous = self.tooltips.wrap(
      format!("terminal-find-previous-tip-{pty}"),
      Side::Top,
      tip("Previous (Shift+Enter)"),
      previous,
      cx,
    );
    let next = self.tooltips.wrap(
      format!("terminal-find-next-tip-{pty}"),
      Side::Top,
      tip("Next (Enter)"),
      next,
      cx,
    );
    let close = self.tooltips.wrap(
      format!("terminal-close-search-tip-{pty}"),
      Side::Top,
      tip("Close (Esc)"),
      close,
      cx,
    );
    Some(
      div()
        .debug_selector(|| "terminal-search".into())
        .absolute()
        .top_2()
        .right_2()
        .flex()
        .items_center()
        .gap_0p5()
        .p_0p5()
        .rounded_md()
        .border_1()
        .border_color(p.border)
        .bg(p.background)
        .text_color(p.foreground)
        .shadow_lg()
        .capture_action(cx.listener(|view, _: &Escape, window, cx| {
          view.close_search(window, cx);
          cx.stop_propagation();
        }))
        .child(
          ui::text(div(), TextSize::Sm)
            .h_6()
            .w_48()
            .px_3()
            .flex()
            .items_center()
            .rounded_sm()
            .border_1()
            .border_color(p.input)
            .bg(p.background)
            .child(Input::new(&input).appearance(false).bordered(false).small()),
        )
        .child(previous)
        .child(next)
        .child(close),
    )
  }

  fn grid(&self, focused: bool) -> Div {
    let scheme = terminal_emu::scheme(self.dark);
    let Some(((w, h), frame)) = self.cell.zip(self.with_emulator(|e| e.frame(scheme))) else {
      return div();
    };
    let (font_size, advance) = (px(self.font_size), self.advance);
    let cursor = frame.cursor.map(|(row, col)| {
      let base = div()
        .absolute()
        .top(zpx(TOP_PAD + row as f32 * h))
        .left(zpx(col as f32 * w))
        .h(zpx(h));
      if focused {
        base.w(zpx(1.)).when(self.cursor_on, |d| {
          d.bg(theme::data_color(scheme.foreground))
        })
      } else {
        base
          .w(zpx(w))
          .border_1()
          .border_color(theme::data_color(scheme.foreground))
      }
    });
    let lines = frame.lines;
    div()
      .absolute()
      .inset_0()
      .child(
        gpui::canvas(
          |_, _, _| {},
          move |bounds, _, window, cx| {
            for (row, runs) in lines.into_iter().enumerate() {
              let y = bounds.origin.y + px(TOP_PAD + row as f32 * h);
              for run in runs {
                let x = bounds.origin.x + px(run.col as f32 * w);
                if let Some(bg) = run.bg {
                  window.paint_quad(gpui::fill(
                    Bounds::new(point(x, y), gpui::size(px(run.cells as f32 * w), px(h))),
                    theme::data_color(bg),
                  ));
                }
                if run.text.trim().is_empty() && !run.underline && !run.strike {
                  continue;
                }
                paint_run(&run, point(x, y), (w, h, advance), font_size, window, cx);
              }
            }
          },
        )
        .size_full(),
      )
      .children(cursor)
  }

  fn scrollbar(&self, p: &Palette) -> Option<Div> {
    let (history, offset, rows) =
      self.with_emulator(|e| (e.history(), e.display_offset(), e.size().rows))?;
    let height = f32::from(self.bounds.get()?.size.height);
    if history == 0 {
      return None;
    }
    let total = (history + rows) as f32;
    let thumb = (rows as f32 / total * height).max(20.);
    let top = (history - offset) as f32 / total * height;
    Some(
      div()
        .absolute()
        .right_0()
        .top(zpx(top.min(height - thumb)))
        .w(zpx(SCROLLBAR))
        .h(zpx(thumb))
        .rounded(radius::SM)
        .bg(p.border),
    )
  }

  /// `RemoteTerminalPanel`'s overlays: attaching, and a lost channel or
  /// an error with Reattach and Close.
  fn remote_overlay(
    &self,
    target: &RemoteTarget,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<Div> {
    let error = match &self.status {
      Status::Failed(error) | Status::AgentFailed(error) => Some(error.clone()),
      _ => None,
    };
    if error.is_some() || self.remote.detached {
      let message = error.clone().unwrap_or_else(|| {
        "Connection to the remote session was lost. The session may still be running on the host."
          .into()
      });
      return Some(
        ui::text(div(), TextSize::Sm)
          .debug_selector(|| "remote-terminal-lost".into())
          .absolute()
          .inset_0()
          .flex()
          .flex_col()
          .items_center()
          .justify_center()
          .gap_2()
          .px_4()
          .text_center()
          .bg(p.background.opacity(0.9))
          .child(
            div()
              .text_color(if error.is_some() {
                ui::Tone::Danger.text(p)
              } else {
                p.foreground
              })
              .child(message),
          )
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .child(
                ui::Button::new("remote-terminal-reattach", p)
                  .size(ui::Size::Sm)
                  .label("Reattach")
                  .on_click(cx.listener(|view, _, _, cx| view.reattach(cx))),
              )
              .child(
                ui::Button::new("remote-terminal-close", p)
                  .size(ui::Size::Sm)
                  .variant(ui::Variant::Outline)
                  .label("Close")
                  .on_click(cx.listener(|view, _, _, cx| (view.on_event)(TermEvent::Close, cx))),
              ),
          ),
      );
    }
    if self.status != Status::Preparing || self.remote.ended.is_some() {
      return None;
    }
    let reattaching = target.reattach || self.remote.attaches > 0;
    Some(
      ui::text(div(), TextSize::Sm)
        .debug_selector(|| "remote-terminal-attaching".into())
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .bg(p.background.opacity(0.8))
        .text_color(p.muted_foreground)
        .child(div().mb_2().child(ui::spinner(
          "remote-terminal-spinner",
          20.,
          p.muted_foreground,
        )))
        .child(if reattaching {
          "Reattaching…"
        } else {
          "Starting remote session…"
        }),
    )
  }

  fn overlay(&self, p: &Palette, cx: &mut Context<Self>) -> Option<Div> {
    if let Some(target) = self.remote_target() {
      return self.remote_overlay(&target.clone(), p, cx);
    }
    match &self.status {
      Status::Ready => None,
      Status::Preparing => Some(
        div()
          .debug_selector(|| "terminal-preparing".into())
          .absolute()
          .inset_0()
          .flex()
          .items_center()
          .justify_center()
          .gap_2()
          .bg(p.background.opacity(0.8))
          .text_color(p.muted_foreground)
          .child(ui::spinner(
            "terminal-preparing-spinner",
            16.,
            p.muted_foreground,
          ))
          .child(ui::text(div(), TextSize::Sm).child("Preparing terminal...")),
      ),
      Status::AgentFailed(error) => Some(
        div()
          .absolute()
          .inset_0()
          .p_6()
          .flex()
          .items_center()
          .justify_center()
          .bg(theme::data_color(
            terminal_emu::scheme(self.dark).background,
          ))
          .child(
            ui::text(div(), TextSize::Sm)
              .max_w_full()
              .text_center()
              .text_color(p.muted_foreground)
              .child(format!("Failed to start agent: {error}")),
          ),
      ),
      Status::Failed(error) => {
        let error = error.clone();
        Some(
          div()
            .debug_selector(|| "terminal-error".into())
            .absolute()
            .inset_0()
            .p_6()
            .flex()
            .items_center()
            .justify_center()
            .bg(p.background.opacity(0.9))
            .child(
              ui::card(p)
                .max_w(zpx(384.))
                .p_4()
                .shadow_lg()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                  ui::text(div(), TextSize::Sm)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Unable to start terminal"),
                )
                .child(
                  ui::text(div(), TextSize::Sm)
                    .text_color(p.muted_foreground)
                    .child(error),
                )
                .child(
                  div()
                    .flex()
                    .gap_2()
                    .child(
                      ui::Button::new("terminal-retry", p)
                        .size(ui::Size::Sm)
                        .label("Try again")
                        .on_click(cx.listener(|view, _, _, cx| view.retry(cx))),
                    )
                    .child(
                      ui::Button::new("terminal-close-session", p)
                        .size(ui::Size::Sm)
                        .variant(ui::Variant::Outline)
                        .label("Close session")
                        .on_click(
                          cx.listener(|view, _, _, cx| (view.on_event)(TermEvent::Close, cx)),
                        ),
                    ),
                ),
            ),
        )
      }
    }
  }
}

impl Render for TerminalView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.measure(window, cx);
    self.sync(cx);
    self.ensure_search_input(window, cx);
    self.tooltips.sync(cx);
    let p = theme::palette(self.dark);
    let focused = self.focus.is_focused(window);
    let bounds = self.bounds.clone();
    let entity = cx.entity().downgrade();
    let search = self.search_bar(&p, cx);
    div()
      .id(SharedString::from(format!("terminal-{}", self.pty)))
      .debug_selector(|| "terminal-body".into())
      .key_context(TERMINAL_CONTEXT)
      .track_focus(&self.focus)
      .relative()
      .size_full()
      .overflow_hidden()
      .bg(theme::data_color(
        terminal_emu::scheme(self.dark).background,
      ))
      .cursor_text()
      .on_key_down(cx.listener(Self::on_key))
      .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
      // A click anywhere else blurs the terminal, as it does xterm's
      // textarea; its cursor turns to an outline.
      .on_mouse_down_out(cx.listener(|view, _, window, cx| {
        if view.focus.is_focused(window) {
          window.blur(cx);
        }
      }))
      .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
      .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
      .on_mouse_move(cx.listener(Self::on_mouse_move))
      .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
      .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
      .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_scroll_wheel(cx.listener(Self::on_scroll))
      .child(
        gpui::canvas(
          move |b, _, cx| {
            if bounds.get() != Some(b) {
              bounds.set(Some(b));
              if let Some(view) = entity.upgrade() {
                cx.defer(move |cx| view.update(cx, |_, cx| cx.notify()));
              }
            }
          },
          |_, _, _, _| {},
        )
        .absolute()
        .inset_0(),
      )
      .child(self.grid(focused))
      .children(self.scrollbar(&p))
      .children(search)
      .children(self.overlay(&p, cx))
  }
}

/// A column of the pane.
struct Column {
  id: String,
  view: Entity<TerminalView>,
  kind: Kind,
  /// Fixed after a divider drag; else the column shares what's left.
  width: Option<f32>,
  streaming: bool,
  bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
  Agent {
    pane_id: i64,
    name: String,
    repo_path: String,
    workspace_path: Option<String>,
    workspace_name: Option<String>,
    agent: String,
  },
  Shell {
    dir: String,
  },
  /// A persistent remote session, in a shell slot.
  Remote {
    target: Box<RemoteTarget>,
  },
}

/// A divider or the pane's edge being dragged.
#[derive(Clone, Copy)]
enum Drag {
  Height,
  Divider {
    left: usize,
    x: f32,
  },
  /// The row's scrollbar thumb, grabbed at `x` with the row scrolled to `from`.
  Scrollbar {
    x: f32,
    from: f32,
  },
}

/// A column the app keeps while its PTY runs, so a pane that opens on its
/// repository after its own pane is gone (its window closed) shows it again.
#[derive(Clone)]
struct Kept {
  id: String,
  kind: Kind,
  start: Start,
  /// The repository the column opened in.
  repo: String,
  /// The pane showing it; `None` once that pane is gone.
  owner: Option<u64>,
  /// When it opened, in [`NEXT_KEPT`] order.
  opened: u64,
}

type KeptColumns = std::sync::Mutex<std::collections::HashMap<String, Kept>>;

/// Kept columns by PTY id.
#[cfg(not(test))]
fn kept() -> &'static KeptColumns {
  static KEPT: std::sync::OnceLock<KeptColumns> = std::sync::OnceLock::new();
  KEPT.get_or_init(Default::default)
}

/// Each test thread keeps its own columns.
#[cfg(test)]
fn kept() -> &'static KeptColumns {
  thread_local! {
    static KEPT: &'static KeptColumns = Box::leak(Box::default());
  }
  KEPT.with(|kept| *kept)
}

/// A kept column's PTY now has the id `next` (a remote reattach).
fn rekey_kept(previous: &str, next: &str) {
  let mut kept = kept().lock_or_recover();
  if let Some(column) = kept.remove(previous) {
    kept.insert(next.to_string(), column);
  }
}

/// The last window closed: its terminals with it.
pub fn forget_kept() {
  kept().lock_or_recover().clear();
}

/// Takes the columns of `repo` no pane shows for pane `owner`.
fn adopt_kept(repo: &str, owner: u64) -> Vec<(String, Kept)> {
  let mut kept = kept().lock_or_recover();
  let mut adopted: Vec<(String, Kept)> = kept
    .iter_mut()
    .filter(|(_, c)| c.owner.is_none() && c.repo == repo)
    .map(|(pty, c)| {
      c.owner = Some(owner);
      (pty.clone(), c.clone())
    })
    .collect();
  adopted.sort_by_key(|(_, c)| c.opened);
  adopted
}

static NEXT_PANE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static NEXT_KEPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Counts the panes that went away leaving columns behind.
static RELEASES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub struct TerminalPane {
  /// Names the pane in [`Kept`] columns.
  serial: u64,
  host: Host,
  react: ActionSink,
  popups: PopupSink,
  /// The app's theme, which the pane's chrome follows.
  dark: bool,
  place: Place,
  route: Route,
  incoming: Option<Route>,
  /// A column to focus when the pane next draws.
  focus_next: Option<String>,
  /// A column to scroll into view when the pane next draws.
  reveal: Option<String>,
  /// Commands GPUI itself gave the pane, run at the next draw.
  later: Vec<Command>,
  /// What the terminals said since the pane last drew.
  events: Vec<(String, TermEvent)>,
  font_size: f32,
  pub collapsed: bool,
  pub maximized: bool,
  height: f32,
  columns: Vec<Column>,
  active: Option<String>,
  /// The workspace view's bounds, for height drags.
  pub container: Rc<Cell<Option<Bounds<Pixels>>>>,
  row: Rc<Cell<Option<Bounds<Pixels>>>>,
  row_scroll: ScrollHandle,
  controls_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
  drag: Option<Drag>,
  last_command: u64,
  summaries: Vec<Summary>,
  tooltips: Tooltips,
  /// The attachment under the mouse, which Cmd+C copies.
  hovered_asset: Option<SendAsset>,
  /// `SendAssetLightbox`, for the column it opened from.
  lightbox: Option<(String, Entity<Lightbox>, Subscription)>,
  /// `useRepoEnvironment`: where new terminals run, while the
  /// devcontainers preview is on; and the repository it was read for.
  environment: Option<Environment>,
  environment_repo: Option<String>,
  environment_menu: bool,
  starting_devcontainer: bool,
  _environment_task: Option<Task<()>>,
  _keys: Option<Subscription>,
  /// The repository's kept columns join the row when the pane next draws.
  adopt_due: bool,
  /// [`RELEASES`] when the pane last looked for kept columns.
  releases_seen: u64,
}

impl Drop for TerminalPane {
  /// The pane's terminals keep running for the next pane on their
  /// repository.
  fn drop(&mut self) {
    let mut released = false;
    for column in kept().lock_or_recover().values_mut() {
      if column.owner == Some(self.serial) {
        column.owner = None;
        released = true;
      }
    }
    if released {
      RELEASES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
  }
}

impl HasTooltips for TerminalPane {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tooltips
  }
}

impl TerminalPane {
  pub fn new(
    host: Host,
    react: ActionSink,
    popups: PopupSink,
    place: Place,
    _: &mut Context<Self>,
  ) -> Self {
    let settings = host
      .settings(&["terminal_font_size".to_string(), "ui_zoom".to_string()])
      .unwrap_or_default();
    let get = |key: &str| {
      settings
        .get(key)
        .cloned()
        .flatten()
        .and_then(|v| v.parse::<f32>().ok())
    };
    let font_size =
      (get("terminal_font_size").unwrap_or(14.) * get("ui_zoom").unwrap_or(100.) / 100.).round();
    Self {
      serial: NEXT_PANE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
      adopt_due: true,
      releases_seen: 0,
      host,
      react,
      popups: popups.clone(),
      dark: false,
      place,
      route: Route::default(),
      incoming: None,
      focus_next: None,
      reveal: None,
      later: Vec::new(),
      events: Vec::new(),
      font_size,
      collapsed: true,
      maximized: false,
      height: HEIGHT_DEFAULT,
      columns: Vec::new(),
      active: None,
      container: Rc::default(),
      row: Rc::default(),
      row_scroll: ScrollHandle::new(),
      controls_bounds: Rc::default(),
      drag: None,
      last_command: 0,
      summaries: Vec::new(),
      tooltips: Tooltips::new(popups, false),
      hovered_asset: None,
      lightbox: None,
      environment: None,
      environment_repo: None,
      environment_menu: false,
      starting_devcontainer: false,
      _environment_task: None,
      _keys: None,
    }
  }

  /// Hidden while another screen replaces the workspace view: the pane
  /// keeps its terminals but takes no shortcuts until it draws again.
  pub fn park(&mut self) {
    self._keys = None;
  }

  /// Where the pane sends its actions: the workspace view drawing it.
  pub fn set_react(&mut self, react: ActionSink) {
    self.react = react;
  }

  /// Follows the app's theme, terminals and all.
  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark == dark {
      return;
    }
    self.dark = dark;
    self.tooltips.dark = dark;
    for column in &self.columns {
      column.view.update(cx, |view, cx| {
        view.dark = dark;
        view.tooltips.dark = dark;
        cx.notify();
      });
    }
    cx.notify();
  }

  /// Reads the repository's environment once the pane opens on it.
  fn load_environment(&mut self, cx: &mut Context<Self>) {
    if self.collapsed {
      // Read again on the next open: it may have changed meanwhile.
      self.environment_repo = None;
      return;
    }
    let repo = self.place.repo_path.clone();
    if repo.is_empty() || self.environment_repo.as_deref() == Some(&repo) {
      return;
    }
    self.environment_repo = Some(repo.clone());
    let host = self.host.clone();
    let task = cx
      .background_executor()
      .spawn(async move { code_data::task_settings(&*host, &repo).environment });
    self._environment_task = Some(cx.spawn(async move |this, cx| {
      let environment = task.await;
      let _ = this.update(cx, |pane, cx| {
        pane.environment = environment;
        cx.notify();
      });
    }));
  }

  /// `setEnvironment`: saves the choice, starting a stopped devcontainer.
  fn set_environment(&mut self, devcontainer: bool, cx: &mut Context<Self>) {
    self.environment_menu = false;
    let repo = self.place.repo_path.clone();
    let value = if devcontainer {
      "devcontainer"
    } else {
      "local"
    };
    let saved = self
      .host
      .set_repo_settings(&repo, &[(ENVIRONMENT_SETTING.into(), value.into())]);
    if let Err(error) = saved {
      self.toast("Failed to save setting", error, ToastTone::Error);
      cx.notify();
      return;
    }
    super::workspace_data::WorkspaceHost::notify(
      &*self.host,
      NativeEvent::RepoSettingsChanged { repo_path: repo },
    );
    let Some(environment) = self.environment.as_mut() else {
      return;
    };
    environment.devcontainer = devcontainer;
    let stopped = environment.status.as_ref().is_some_and(|s| !s.running);
    if devcontainer && stopped && !self.starting_devcontainer {
      self.start_devcontainer(cx);
    }
    cx.notify();
  }

  fn start_devcontainer(&mut self, cx: &mut Context<Self>) {
    self.environment_menu = false;
    self.starting_devcontainer = true;
    let (host, repo) = (self.host.clone(), self.place.repo_path.clone());
    let task = cx
      .background_executor()
      .spawn(async move { host.devcontainer_up(&repo) });
    self._environment_task = Some(cx.spawn(async move |this, cx| {
      let result = task.await;
      let _ = this.update(cx, |pane, cx| {
        pane.starting_devcontainer = false;
        match result {
          Ok(status) => {
            if let Some(environment) = pane.environment.as_mut() {
              environment.status = Some(status);
            }
          }
          Err(error) => pane.toast("Devcontainer did not start", error, ToastTone::Error),
        }
        cx.notify();
      });
    }));
    cx.notify();
  }

  fn toast(&self, title: &str, description: String, tone: ToastTone) {
    super::workspace_data::WorkspaceHost::notify(
      &*self.host,
      NativeEvent::Toast {
        title: title.into(),
        description,
        tone,
        action: None,
      },
    );
  }

  /// Whether the pane covers the tab (maximized and open).
  pub fn covers(&self) -> bool {
    self.maximized && !self.collapsed && !self.columns.is_empty()
  }

  pub fn set_place(&mut self, place: Place, cx: &mut Context<Self>) {
    if self.place == place {
      return;
    }
    let moved = self.place.working_directory != place.working_directory;
    self.adopt_due |= self.place.repo_path != place.repo_path;
    self.place = place;
    // A newly selected workspace's first terminal becomes the active one.
    if moved && !self.collapsed {
      let dir = &self.place.working_directory;
      let id = self
        .columns
        .iter()
        .find(|c| self.column_key(c) == *dir)
        .map(|c| c.id.clone());
      if id.is_some() {
        self.reveal = id.clone();
        self.active = id;
      }
    }
    cx.notify();
  }

  fn send(&self, field: &str, value: serde_json::Value, cx: &mut App) {
    (self.react)(
      ScreenAction::Field {
        field: field.into(),
        value,
      },
      cx,
    );
  }

  /// Takes React's latest word on sessions and commands, acted on when
  /// the pane next draws.
  pub fn set_route(&mut self, route: Route, cx: &mut Context<Self>) {
    if route != self.route && self.incoming.as_ref() != Some(&route) {
      self.incoming = Some(route);
      cx.notify();
    }
  }

  fn apply_route(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
    if route == self.route {
      return;
    }
    let previous = std::mem::replace(&mut self.route, route.clone());
    for command in &route.commands {
      if command.nonce > self.last_command {
        self.last_command = command.nonce;
        self.run(command, window, cx);
      }
    }
    if route.active_session_id != previous.active_session_id
      || route.sessions_revision != previous.sessions_revision
      || route.pending != previous.pending
    {
      if let Some(id) = route.active_session_id {
        // `auto-mount`: the active session opens and takes focus.
        self.open_agent(id, window, cx);
      } else if previous.active_session_id.is_some() {
        self.active = None;
      }
    }
    cx.notify();
  }

  /// Closes every terminal of the workspace at `key` (`closeTerminalsForWorkspace`).
  fn close_workspace(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
    let ids: Vec<String> = self
      .columns
      .iter()
      .filter(|c| self.column_key(c) == key)
      .map(|c| c.id.clone())
      .collect();
    for id in ids {
      self.close(&id, window, cx);
    }
  }

  /// Runs a command of kind `kind` (as React's `GpuiTerminalCommand`s)
  /// when the pane next draws: closing a removed workspace's terminals, or
  /// a palette command.
  pub fn run_later(&mut self, kind: &str, dir: Option<String>, cx: &mut Context<Self>) {
    self.later.push(Command {
      nonce: 0,
      kind: kind.into(),
      dir,
      session_id: None,
      text: None,
      remote: None,
    });
    cx.notify();
  }

  fn run(&mut self, command: &Command, window: &mut Window, cx: &mut Context<Self>) {
    match command.kind.as_str() {
      "toggleCollapse" => self.toggle_collapse(cx),
      "toggleMaximize" => self.toggle_maximize(cx),
      "newShell" => self.add_shell(command.dir.clone(), window, cx),
      "newAgent" => self.new_agent(cx),
      "openRemote" => {
        if let Some(target) = command.remote.clone() {
          self.open_remote(target, window, cx);
        }
      }
      "closeWorkspace" => {
        let key = command.dir.clone().unwrap_or_default();
        self.close_workspace(&key, window, cx);
      }
      "send" => {
        if let (Some(id), Some(text)) = (command.session_id, &command.text) {
          let column = agent_column_id(id);
          if let Some(view) = self.view(&column) {
            view.update(cx, |view, cx| view.write(&format!("{text}\n"), cx));
          }
          self.focus_column(&column, window, cx);
        }
      }
      _ => {}
    }
  }

  /// `sendToIdleAgent`: sends `text` to an agent of `repo` that runs in
  /// the workspace on `branch` and is not writing, and focuses it. False
  /// when there is none.
  pub fn send_to_idle_agent(
    &mut self,
    repo: &str,
    branch: &str,
    text: &str,
    cx: &mut Context<Self>,
  ) -> bool {
    let idle = self.columns.iter().find(|c| {
      !c.streaming
        && matches!(&c.kind, Kind::Agent { repo_path, workspace_name: Some(name), .. }
          if repo_path == repo && name == branch)
    });
    let Some(column) = idle.map(|c| c.id.clone()) else {
      return false;
    };
    if let Some(view) = self.view(&column) {
      view.update(cx, |view, cx| view.write(&format!("{text}\n"), cx));
    }
    self.focus_next = Some(column);
    cx.notify();
    true
  }

  fn view(&self, id: &str) -> Option<Entity<TerminalView>> {
    self
      .columns
      .iter()
      .find(|c| c.id == id)
      .map(|c| c.view.clone())
  }

  fn column_key(&self, column: &Column) -> String {
    match &column.kind {
      Kind::Agent {
        workspace_path,
        repo_path,
        ..
      } => workspace_path.clone().unwrap_or_else(|| repo_path.clone()),
      Kind::Shell { dir } => dir.clone(),
      Kind::Remote { target } => target.remote_working_directory.clone(),
    }
  }

  fn toggle_collapse(&mut self, cx: &mut Context<Self>) {
    self.collapsed = !self.collapsed;
    if self.collapsed {
      self.maximized = false;
    }
    cx.notify();
  }

  fn toggle_maximize(&mut self, cx: &mut Context<Self>) {
    if self.maximized {
      self.maximized = false;
    } else {
      self.collapsed = false;
      self.maximized = true;
    }
    cx.notify();
  }

  fn on_event(&self, id: String, cx: &mut Context<Self>) -> OnEvent {
    let pane = cx.entity().downgrade();
    Rc::new(move |event, cx| {
      let id = id.clone();
      if let Some(pane) = pane.upgrade() {
        cx.defer(move |cx| {
          pane.update(cx, |pane, cx| {
            pane.events.push((id, event));
            cx.notify();
          });
        });
      }
    })
  }

  fn on_term_event(
    &mut self,
    id: &str,
    event: TermEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    match event {
      TermEvent::Focused => self.activate(id, cx),
      TermEvent::Output { from_process: true } => self.set_streaming(id, true, cx),
      TermEvent::Output { .. } => {}
      TermEvent::Idle => self.set_streaming(id, false, cx),
      TermEvent::Close => self.close(id, window, cx),
      TermEvent::Toast { title, description } => {
        super::workspace_data::WorkspaceHost::notify(
          &*self.host,
          crate::gpui_shell::native::NativeEvent::Toast {
            title,
            description,
            tone: crate::gpui_shell::native::ToastTone::Warning,
            action: None,
          },
        );
      }
    }
  }

  fn set_streaming(&mut self, id: &str, streaming: bool, cx: &mut Context<Self>) {
    if let Some(column) = self.columns.iter_mut().find(|c| c.id == id) {
      if column.streaming != streaming {
        column.streaming = streaming;
        self.publish(cx);
      }
    }
  }

  /// `onTerminalsChange`: only when the summaries changed.
  fn publish(&mut self, cx: &mut Context<Self>) {
    let summaries = self.build_summaries();
    if summaries != self.summaries {
      self.summaries = summaries.clone();
      self.send(
        "terminal.summaries",
        serde_json::to_value(summaries).unwrap_or_default(),
        cx,
      );
    }
  }

  fn build_summaries(&self) -> Vec<Summary> {
    self
      .columns
      .iter()
      .map(|column| match &column.kind {
        Kind::Agent {
          name,
          repo_path,
          workspace_name,
          agent,
          ..
        } => Summary {
          id: column.id.clone(),
          kind: "agent",
          name: name.clone(),
          branch_name: workspace_name.clone(),
          repo_path: Some(repo_path.clone()),
          is_main_repo: workspace_name.is_none(),
          agent: Some(agent.clone()),
          is_streaming: column.streaming,
        },
        Kind::Shell { .. } | Kind::Remote { .. } => {
          let dir = self.column_key(column);
          let (name, main) = self.label(&dir);
          Summary {
            id: column.id.clone(),
            kind: "shell",
            name: "Shell".into(),
            branch_name: Some(name),
            repo_path: None,
            is_main_repo: main,
            agent: None,
            is_streaming: column.streaming,
          }
        }
      })
      .collect()
  }

  /// `resolveTerminalWorkspace` for a shell: the workspace's branch for
  /// its directory, else the home branch.
  fn label(&self, dir: &str) -> (String, bool) {
    let found = self.place.branches.iter().find(|(path, _)| path == dir);
    match found {
      Some((_, branch)) => (branch.clone(), false),
      None => {
        let agent = self.columns.iter().find_map(|c| match &c.kind {
          Kind::Agent {
            workspace_path: Some(path),
            workspace_name: Some(name),
            ..
          } if path == dir => Some(name.clone()),
          _ => None,
        });
        match agent {
          Some(name) => (name, false),
          None => (self.home_branch(), true),
        }
      }
    }
  }

  fn home_branch(&self) -> String {
    if self.place.current_branch.is_empty() {
      "main".into()
    } else {
      self.place.current_branch.clone()
    }
  }

  /// `handleTerminalDoubleClick`.
  fn scroll_fully_into_view(&mut self, id: &str, cx: &mut Context<Self>) {
    let column = self.columns.iter().find(|c| c.id == id);
    let (Some(row), Some(column)) = (self.row.get(), column.and_then(|c| c.bounds.get())) else {
      return;
    };
    let span = |b: Bounds<Pixels>| (f32::from(b.left()), f32::from(b.right()));
    let delta = scroll_delta(span(row), span(column));
    if delta != 0. {
      let mut offset = self.row_scroll.offset();
      let max = self.row_scroll.max_offset().x;
      offset.x = (offset.x - px(delta)).clamp(-max, px(0.));
      self.row_scroll.set_offset(offset);
      cx.notify();
    }
  }

  fn activate(&mut self, id: &str, cx: &mut Context<Self>) {
    if self.active.as_deref() == Some(id) {
      return;
    }
    self.active = Some(id.to_string());
    if let Some(Kind::Agent { pane_id, .. }) =
      self.columns.iter().find(|c| c.id == id).map(|c| &c.kind)
    {
      if self.route.active_session_id != Some(*pane_id) {
        self.send("terminal.activeSession", (*pane_id).into(), cx);
      }
    }
    cx.notify();
  }

  fn focus_column(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
    self.collapsed = false;
    self.reveal = Some(id.to_string());
    self.activate(id, cx);
    if let Some(view) = self.view(id) {
      let focus = view.read(cx).focus.clone();
      window.focus(&focus, cx);
    }
    cx.notify();
  }

  /// Opens (or focuses) the agent session with the pane id `pane_id`.
  fn open_agent(&mut self, pane_id: i64, window: &mut Window, cx: &mut Context<Self>) {
    let id = agent_column_id(pane_id);
    if self.columns.iter().any(|c| c.id == id) {
      // Clicking a column makes it the active session, which React echoes
      // back: focus stays where the click put it (the search input, say).
      if self.active.as_deref() != Some(id.as_str()) {
        self.focus_column(&id, window, cx);
      }
      return;
    }
    let Some(kind_and_start) = self.agent_session(pane_id) else {
      return;
    };
    let (kind, pty, start) = kind_and_start;
    self.push_column(id.clone(), pty, kind, start, cx);
    self.publish(cx);
    self.focus_column(&id, window, cx);
  }

  /// The session's column and what its terminal starts, read from the
  /// session tables of the repository and its supporting ones.
  fn agent_session(&self, pane_id: i64) -> Option<(Kind, String, Start)> {
    let main = match &self.route.main_repo_path {
      main if !main.is_empty() => main.clone(),
      _ => self.place.repo_path.clone(),
    };
    let mut repos = vec![main.clone()];
    repos.extend(self.host.supporting_repos(&main));
    for repo in repos {
      let sessions = self.host.sessions(&repo).unwrap_or_default();
      let Some(session) = sessions
        .into_iter()
        .find(|s| terminal_data::pane_session_id(&repo, &main, s.id) == pane_id)
      else {
        continue;
      };
      // What the session starts with: the backend's, for a session it
      // started, else React's.
      let pending = self.host.pending_session(pane_id).or_else(|| {
        self
          .route
          .pending
          .iter()
          .find(|p| p.session_id == pane_id)
          .map(Pending::start)
      });
      let pending = pending.as_ref();
      let workspace = session.workspace_id.and_then(|id| {
        super::code_data::CodeHost::workspaces(&*self.host, &repo)
          .ok()?
          .into_iter()
          .find(|w| w.id == id)
      });
      let workspace_path = pending
        .and_then(|p| p.workspace_path.clone())
        .or_else(|| workspace.as_ref().map(super::merge::full_path));
      let agent = pending
        .and_then(|p| p.agent.clone())
        .unwrap_or_else(|| self.default_agent(&repo));
      let pty = terminal_data::agent_pty_id(&repo, session.id);
      let start = AgentStart {
        agent: agent.clone(),
        repo_path: repo.clone(),
        workspace_path: workspace_path.clone(),
        session_id: session.id,
        pending_prompt: pending.and_then(|p| p.pending_prompt.clone()),
        permission_mode: pending.and_then(|p| p.permission_mode.clone()),
      };
      let kind = Kind::Agent {
        pane_id,
        name: session.name.clone(),
        repo_path: repo,
        workspace_path,
        workspace_name: workspace.map(|w| w.branch_name),
        agent,
      };
      return Some((
        kind,
        pty,
        Start::Agent {
          start,
          name: session.name,
          workspace_id: session.workspace_id,
          working_dir: pending.and_then(|p| p.workspace_path.clone()),
        },
      ));
    }
    None
  }

  fn default_agent(&self, repo: &str) -> String {
    let key = vec!["default_agent".to_string()];
    let repo_agent = self
      .host
      .repo_settings(repo, &key)
      .ok()
      .and_then(|v| v.get("default_agent").cloned().flatten())
      .filter(|v| !v.is_empty());
    repo_agent
      .or_else(|| {
        self
          .host
          .settings(&key)
          .ok()
          .and_then(|v| v.get("default_agent").cloned().flatten())
          .filter(|v| !v.is_empty())
      })
      .unwrap_or_else(|| "claude".into())
  }

  /// The active terminal's directory, else the workspace's.
  fn active_dir(&self) -> String {
    self
      .active
      .as_ref()
      .and_then(|id| self.columns.iter().find(|c| &c.id == id))
      .map(|c| self.column_key(c))
      .unwrap_or_else(|| self.place.working_directory.clone())
  }

  fn new_agent(&mut self, cx: &mut Context<Self>) {
    let dir = self.active_dir();
    self.send("terminal.newAgent", dir.into(), cx);
  }

  fn add_shell(&mut self, dir: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
    let dir = dir.unwrap_or_else(|| self.active_dir());
    if self.route.remote_shell {
      // A remote repository's paths exist only on the host: the window resolves
      // the session and opens it with `openRemote`.
      self.send("terminal.newRemoteShell", dir.into(), cx);
      return;
    }
    // Two shells opened in the same millisecond still get their own ids.
    let mut millis = now_millis();
    while self
      .columns
      .iter()
      .any(|c| c.id == terminal_data::shell_pty_id(&dir, millis))
    {
      millis += 1;
    }
    let id = terminal_data::shell_pty_id(&dir, millis);
    let start = Start::Shell { dir: dir.clone() };
    self.push_column(id.clone(), id.clone(), Kind::Shell { dir }, start, cx);
    self.publish(cx);
    // The new shell takes focus at the end of the row, scrolled into view.
    self.focus_column(&id, window, cx);
    cx.notify();
  }

  /// `handleOpenRemoteSession`: the session joins the row in a shell slot
  /// and becomes the active terminal.
  fn open_remote(&mut self, target: RemoteTarget, window: &mut Window, cx: &mut Context<Self>) {
    let millis = now_millis();
    let id = terminal_data::remote_column_id(&target.label, millis);
    let pty = terminal_data::remote_pty_id(&target, millis);
    let start = Start::Remote {
      target: target.clone(),
    };
    let kind = Kind::Remote {
      target: Box::new(target),
    };
    self.push_column(id.clone(), pty, kind, start, cx);
    self.publish(cx);
    self.focus_column(&id, window, cx);
  }

  /// A column for `pty`, kept while the PTY runs.
  fn push_column(
    &mut self,
    id: String,
    pty: String,
    kind: Kind,
    start: Start,
    cx: &mut Context<Self>,
  ) {
    kept().lock_or_recover().insert(
      pty.clone(),
      Kept {
        id: id.clone(),
        kind: kind.clone(),
        start: start.clone(),
        repo: self.place.repo_path.clone(),
        owner: Some(self.serial),
        opened: NEXT_KEPT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
      },
    );
    let on_event = self.on_event(id.clone(), cx);
    let font_size = match kind {
      Kind::Remote { .. } => REMOTE_FONT_SIZE,
      _ => self.font_size,
    };
    let (host, look, popups) = (
      self.host.clone(),
      (font_size, self.dark),
      self.popups.clone(),
    );
    let view = cx.new(|cx| TerminalView::new(pty, start, host, look, popups, on_event, cx));
    self.columns.push(Column {
      id,
      view,
      kind,
      width: None,
      streaming: false,
      bounds: Rc::default(),
    });
  }

  /// Shows the repository's terminals that a closed window left running,
  /// once the pane opens on it or draws after such a window closed.
  /// A local one that ended meanwhile is dropped; a remote one reattaches
  /// to its session on the host.
  fn adopt(&mut self, cx: &mut Context<Self>) {
    let releases = RELEASES.load(std::sync::atomic::Ordering::Relaxed);
    let due = std::mem::take(&mut self.adopt_due) || releases != self.releases_seen;
    self.releases_seen = releases;
    if !due || self.place.repo_path.is_empty() {
      return;
    }
    let mut shown = None;
    for (pty, column) in adopt_kept(&self.place.repo_path, self.serial) {
      let remote = matches!(column.kind, Kind::Remote { .. });
      if !remote && !self.host.pty_exists(&pty) {
        kept().lock_or_recover().remove(&pty);
        terminal_data::detach(&pty);
        continue;
      }
      if self.columns.iter().any(|c| c.id == column.id) {
        continue;
      }
      shown = Some(column.id.clone());
      self.push_column(column.id, pty, column.kind, column.start, cx);
    }
    if let Some(id) = shown {
      self.collapsed = false;
      self.reveal = Some(id.clone());
      self.active = Some(id);
      self.publish(cx);
    }
  }

  fn close(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
    let order: Vec<String> = self.columns.iter().map(|c| c.id.clone()).collect();
    let Some(index) = order.iter().position(|c| c == id) else {
      return;
    };
    let column = self.columns.remove(index);
    let pty = column.view.read(cx).pty.clone();
    if let Err(error) = self.host.close_pty(&pty) {
      log::warn!("pty close failed: {error}");
    }
    terminal_data::detach(&pty);
    kept().lock_or_recover().remove(&pty);
    if let Kind::Agent { pane_id, .. } = column.kind {
      self.send("terminal.closeSession", pane_id.into(), cx);
    }
    if self.active.as_deref() == Some(id) {
      self.active = None;
      if let Some(next) = neighbour(&order, id) {
        self.focus_column(&next, window, cx);
      }
    }
    if self.columns.is_empty() {
      self.collapsed = true;
      self.maximized = false;
    }
    self.publish(cx);
    cx.notify();
  }

  fn shortcut(&mut self, shortcut: Shortcut, window: &mut Window, cx: &mut Context<Self>) {
    match shortcut {
      Shortcut::ToggleCollapse => self.toggle_collapse(cx),
      Shortcut::ToggleMaximize => self.toggle_maximize(cx),
      Shortcut::NewAgent => self.new_agent(cx),
      Shortcut::NewShell => self.add_shell(None, window, cx),
      Shortcut::Close => {
        if let Some(id) = self.active.clone() {
          self.close(&id, window, cx);
        }
      }
      Shortcut::App(_) => {}
    }
  }

  fn watch_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self._keys.is_some() {
      return;
    }
    let handle = window.window_handle();
    self._keys = Some(cx.observe_keystrokes(move |pane, event, window, cx| {
      if window.window_handle() != handle {
        return;
      }
      let k = &event.keystroke;
      let m = crate::gpui_shell::keys::held(k.modifiers);
      if let Some(found) = shortcut(&k.key, m.control, m.platform, m.shift, m.alt) {
        pane.shortcut(found, window, cx);
      } else if k.key == "c" && (m.platform || m.control) && pane.lightbox.is_none() {
        // `copySendAssetToClipboard`: the hovered attachment's quoted path.
        if let Some(asset) = &pane.hovered_asset {
          cx.write_to_clipboard(ClipboardItem::new_string(shell_quote(&asset.path)));
        }
      }
    }));
  }

  /// The attachments shown over a column's terminal.
  fn column_assets(&self, column: &Column, cx: &App) -> Vec<SendAsset> {
    let pty = column.view.read(cx).pty.clone();
    let active = self.active.as_deref() == Some(column.id.as_str());
    assets_for(&self.host.send_assets(), &pty, active)
  }

  fn open_lightbox(&mut self, column_id: &str, asset_id: &str, cx: &mut Context<Self>) {
    let Some(column) = self.columns.iter().find(|c| c.id == column_id) else {
      return;
    };
    let assets = self.column_assets(column, cx);
    let Some(index) = assets.iter().position(|a| a.id == asset_id) else {
      return;
    };
    let review = matches!(column.kind, Kind::Agent { .. });
    let host = self.host.clone();
    let lightbox = cx.new(|cx| Lightbox::new(host, assets, index, review, cx));
    let id = column_id.to_string();
    let subscription = cx.subscribe(&lightbox, move |pane, _, event: &LightboxEvent, cx| {
      pane.on_lightbox(&id, event.clone(), cx)
    });
    self.hovered_asset = None;
    self.lightbox = Some((column_id.to_string(), lightbox, subscription));
    cx.notify();
  }

  fn on_lightbox(&mut self, column_id: &str, event: LightboxEvent, cx: &mut Context<Self>) {
    match event {
      LightboxEvent::Close => {
        self.lightbox = None;
        cx.notify();
      }
      LightboxEvent::SendReview(prompt) => {
        if let Some(view) = self.view(column_id) {
          view.read(cx).send_message(&prompt);
        }
      }
      LightboxEvent::Toast {
        title,
        description,
        tone,
      } => super::workspace_data::WorkspaceHost::notify(
        &*self.host,
        crate::gpui_shell::native::NativeEvent::Toast {
          title,
          description,
          tone,
          action: None,
        },
      ),
    }
  }

  /// Keeps the lightbox on its column's attachments; closes it once none
  /// are left, or its column is gone.
  fn sync_lightbox(&mut self, cx: &mut Context<Self>) {
    let Some((column_id, lightbox, _)) = &self.lightbox else {
      return;
    };
    let assets = self
      .columns
      .iter()
      .find(|c| &c.id == column_id)
      .map(|c| self.column_assets(c, cx))
      .unwrap_or_default();
    if assets.is_empty() {
      self.lightbox = None;
      return;
    }
    lightbox.update(cx, |l, cx| l.set_assets(assets, cx));
  }

  /// `TerminalSendPreviews`: the column's attachments over the top of its
  /// terminal, under a fade, each a thumbnail to preview, drag or dismiss.
  fn previews(&mut self, index: usize, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let column = &self.columns[index];
    let assets = self.column_assets(column, cx);
    if assets.is_empty() {
      return None;
    }
    let column_id = column.id.clone();
    let hovered = self.hovered_asset.as_ref().map(|a| a.id.clone());
    let bg = theme::data_color(terminal_emu::scheme(self.dark).background);
    let dark = theme::palette(true);
    let thumbs = assets.into_iter().map(|asset| {
      let id = asset.id.clone();
      let shown = hovered.as_deref() == Some(id.as_str());
      let (open_column, open_id) = (column_id.clone(), id.clone());
      let (hover_asset, dismiss_id) = (asset.clone(), id.clone());
      let media = if asset.is_image() {
        div().size_full().child(
          img(std::path::PathBuf::from(&asset.path))
            .object_fit(ObjectFit::Contain)
            .size_full()
            .rounded(radius::MD),
        )
      } else {
        ui::text(div(), TextSize::Xs)
          .size_full()
          .px_1()
          .flex()
          .flex_col()
          .items_center()
          .justify_center()
          .gap_1()
          .child(ui::icon(theme::ICON_FILE_TEXT, 16., dark.foreground))
          .child(
            div()
              .w_full()
              .truncate()
              .flex()
              .justify_center()
              .child(asset.title.clone()),
          )
      };
      let drag = SendAssetDrag {
        path: asset.path.clone(),
        title: asset.title,
      };
      div()
        .id(SharedString::from(format!("terminal-send-preview-{id}")))
        .debug_selector({
          let id = id.clone();
          move || format!("terminal-send-preview-{id}")
        })
        .relative()
        .flex_none()
        .h_20()
        .w(zpx(144.))
        .min_w(zpx(144.))
        .rounded(radius::MD)
        .border_1()
        .border_color(dark.foreground.opacity(if shown { 0.4 } else { 0.15 }))
        .bg(dark.popover)
        .text_color(dark.foreground)
        .shadow_md()
        .cursor_grab()
        .on_hover(cx.listener(move |pane, hovered: &bool, _, cx| {
          let id = &hover_asset.id;
          if *hovered {
            pane.hovered_asset = Some(hover_asset.clone());
          } else if pane.hovered_asset.as_ref().is_some_and(|a| &a.id == id) {
            pane.hovered_asset = None;
          }
          cx.notify();
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |pane, _, _, cx| pane.open_lightbox(&open_column, &open_id, cx)))
        .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
        .child(media)
        .when(shown, |d| {
          d.child(
            div()
              .id(SharedString::from(format!("terminal-send-dismiss-{id}")))
              .debug_selector({
                let id = id.clone();
                move || format!("terminal-send-dismiss-{id}")
              })
              .absolute()
              .top(zpx(-8.))
              .right(zpx(-8.))
              .size_5()
              .flex()
              .items_center()
              .justify_center()
              .rounded_full()
              .border_1()
              .border_color(dark.muted_foreground)
              .bg(dark.foreground.opacity(0.8))
              .hover(|s| s.bg(dark.foreground))
              .shadow_md()
              .cursor_pointer()
              .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
              .on_click(cx.listener(move |pane, _, _, cx| {
                cx.stop_propagation();
                pane.hovered_asset = None;
                pane.host.dismiss_send_asset(&dismiss_id);
                // React drops its own copy, which its views draw.
                pane.send("terminal.dismissAsset", dismiss_id.clone().into(), cx);
                cx.notify();
              }))
              .child(ui::icon(theme::ICON_X, 12., dark.background)),
          )
        })
    });
    Some(
      div()
        .debug_selector(|| "terminal-send-previews".into())
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .child(
          div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .child(div().flex_1().bg(linear_gradient(
              180.,
              linear_color_stop(bg, 0.),
              linear_color_stop(bg.opacity(0.85), 1.),
            )))
            .child(div().flex_1().bg(linear_gradient(
              180.,
              linear_color_stop(bg.opacity(0.85), 0.),
              linear_color_stop(bg.opacity(0.), 1.),
            ))),
        )
        .child(
          div()
            .id(SharedString::from(format!(
              "terminal-send-previews-drop-{column_id}"
            )))
            .debug_selector(|| "terminal-send-previews-drop".into())
            .relative()
            .px_3()
            .pb_8()
            .pt_3()
            .occlude()
            .child(
              div()
                .id(SharedString::from(format!(
                  "terminal-send-group-{column_id}"
                )))
                .flex()
                .gap_2()
                .overflow_x_scroll()
                .pt_2()
                .pr_2()
                .children(thumbs),
            ),
        ),
    )
  }

  /// The row's width and how far it scrolls, while its columns overflow it.
  fn row_overflow(&self) -> Option<(f32, f32)> {
    let view = f32::from(self.row.get()?.size.width);
    let max = f32::from(self.row_scroll.max_offset().x);
    (view > 0. && max >= 1.).then_some((view, max))
  }

  /// A scrollbar under the row while its columns overflow it, so the ones
  /// past the edge show and can be reached.
  fn row_scrollbar(&self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let (view, max) = self.row_overflow()?;
    let offset = -f32::from(self.row_scroll.offset().x);
    let (left, width) = scrollbar_thumb(view, max, offset);
    let dragging = matches!(self.drag, Some(Drag::Scrollbar { .. }));
    let hover = p.muted_foreground;
    Some(
      div()
        .id("terminal-row-scrollbar")
        .debug_selector(|| "terminal-row-scrollbar".into())
        .absolute()
        .bottom_0()
        .left(px(left))
        .w(px(width))
        .h(zpx(SCROLLBAR))
        .rounded(radius::SM)
        .bg(if dragging { hover } else { p.border })
        .hover(move |s| s.bg(hover))
        .occlude()
        .on_mouse_down(
          MouseButton::Left,
          cx.listener(move |pane, e: &MouseDownEvent, _, cx| {
            pane.drag = Some(Drag::Scrollbar {
              x: f32::from(e.position.x),
              from: offset,
            });
            cx.stop_propagation();
            cx.notify();
          }),
        ),
    )
  }

  fn min_width(&self) -> f32 {
    self
      .row
      .get()
      .map(|b| f32::from(b.size.width) * MIN_COLUMN_SHARE)
      .filter(|w| *w > 0.)
      .unwrap_or(300.)
  }

  fn on_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
    let Some(drag) = self.drag else {
      return;
    };
    if event.pressed_button != Some(MouseButton::Left) {
      self.drag = None;
      return;
    }
    let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
    match drag {
      Drag::Height => {
        if self.maximized {
          return;
        }
        if let Some(b) = self.container.get() {
          let top = f32::from(b.origin.y);
          self.height = drag_height(top, top + f32::from(b.size.height), y);
        }
      }
      Drag::Divider { left, x: from } => {
        let min = self.min_width();
        let width = |c: &Column| {
          c.width
            .or_else(|| c.bounds.get().map(|b| f32::from(b.size.width)))
        };
        let (Some(l), Some(r)) = (
          self.columns.get(left).and_then(width),
          self.columns.get(left + 1).and_then(width),
        ) else {
          return;
        };
        let (l, r) = resize_pair(l, r, x - from, min);
        self.columns[left].width = Some(l);
        self.columns[left + 1].width = Some(r);
        self.drag = Some(Drag::Divider { left, x });
      }
      Drag::Scrollbar { x: grabbed, from } => {
        let Some((view, max)) = self.row_overflow() else {
          return;
        };
        let (_, thumb) = scrollbar_thumb(view, max, from);
        let travel = view - thumb;
        let offset = (from + (x - grabbed) * max / travel.max(1.)).clamp(0., max);
        self.row_scroll.set_offset(point(px(-offset), px(0.)));
      }
    }
    cx.notify();
  }

  fn controls(&mut self, p: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
    let fg = p.foreground;
    let accent = p.accent;
    let button = |id: &'static str, icon: &'static str, name: &'static str| {
      div()
        .id(id)
        .debug_selector(move || id.into())
        .role(gpui::Role::Button)
        .aria_label(name)
        .size_5()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(accent))
        .child(ui::icon(icon, 12., fg))
    };
    let tip = |text: &'static str, keys: &'static str| {
      Some(TooltipContent::Shortcut {
        text: text.into(),
        keys: crate::gpui_shell::keys::hint(keys).into(),
      })
    };
    let mut buttons: Vec<gpui::AnyElement> = Vec::new();
    if self.collapsed {
      let b = button("terminal-expand", theme::ICON_CHEVRON_UP, "Expand").on_click(cx.listener(
        |pane, _, _, cx| {
          pane.collapsed = false;
          cx.notify();
        },
      ));
      buttons.push(
        self
          .tooltips
          .wrap(
            "terminal-expand",
            Side::Top,
            tip("Expand", "⌘ + J"),
            b.test_support(),
            cx,
          )
          .into_any_element(),
      );
    } else {
      if let Some(environment) = &self.environment {
        let trigger = environment::trigger(
          "terminal-environment",
          environment,
          self.starting_devcontainer,
          environment::PANE_TRIGGER,
          p,
        )
        .on_click(cx.listener(|pane, _, _, cx| {
          pane.environment_menu = !pane.environment_menu;
          cx.notify();
        }));
        let menu = self.environment_menu.then(|| {
          let panel = environment::menu(
            "terminal-environment-menu",
            environment,
            self.starting_devcontainer,
            p,
            Self::set_environment,
            Self::start_devcontainer,
            cx,
          )
          .top(zpx(24.))
          .right_0()
          .dismiss_with("terminal-environment-menu", cx, |pane, cx| {
            let open = std::mem::take(&mut pane.environment_menu);
            cx.notify();
            open
          });
          deferred(panel).with_priority(1)
        });
        buttons.push(
          div()
            .relative()
            .child(trigger)
            .children(menu)
            .into_any_element(),
        );
      }
      if self.maximized {
        let b = button("terminal-restore", theme::ICON_MINIMIZE, "Restore")
          .on_click(cx.listener(|pane, _, _, cx| pane.toggle_maximize(cx)));
        buttons.push(
          self
            .tooltips
            .wrap(
              "terminal-restore",
              Side::Top,
              tip("Restore", MAXIMIZE_KEYS),
              b.test_support(),
              cx,
            )
            .into_any_element(),
        );
      } else if !self.columns.is_empty() {
        let b = button("terminal-maximize", theme::ICON_MAXIMIZE, "Maximize")
          .on_click(cx.listener(|pane, _, _, cx| pane.toggle_maximize(cx)));
        buttons.push(
          self
            .tooltips
            .wrap(
              "terminal-maximize",
              Side::Top,
              tip("Maximize", MAXIMIZE_KEYS),
              b.test_support(),
              cx,
            )
            .into_any_element(),
        );
      }
      let b = button("terminal-collapse", theme::ICON_CHEVRON_DOWN, "Collapse").on_click(
        cx.listener(|pane, _, _, cx| {
          pane.collapsed = true;
          pane.maximized = false;
          cx.notify();
        }),
      );
      buttons.push(
        self
          .tooltips
          .wrap(
            "terminal-collapse",
            Side::Top,
            tip("Collapse", "⌘ + J"),
            b.test_support(),
            cx,
          )
          .into_any_element(),
      );
    }
    div()
      .id("terminal-pane-controls")
      .debug_selector(|| "terminal-pane-controls".into())
      .flex()
      .items_center()
      .gap_px()
      .rounded_md()
      .border_1()
      .border_color(p.border.opacity(0.8))
      .bg(p.background.opacity(0.9))
      .p_0p5()
      .shadow_sm()
      .child({
        let bounds = self.controls_bounds.clone();
        gpui::canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
          .absolute()
          .size_full()
      })
      .children(buttons)
  }

  fn header(&mut self, index: usize, p: &Palette, cx: &mut Context<Self>) -> Div {
    let column = &self.columns[index];
    let active = self.active.as_deref() == Some(column.id.as_str());
    let id = column.id.clone();
    // Agent headers, and in the dark theme a shell's while selected, sit on
    // a dark fill, so their text and badge go light to stay readable.
    let on_dark = !matches!(column.kind, Kind::Shell { .. }) || (active && self.dark);
    let d = theme::palette(true);
    let (title_fg, icon_fg): (gpui::Hsla, gpui::Hsla) = if on_dark {
      (d.foreground, d.foreground)
    } else {
      (p.muted_foreground, p.foreground)
    };
    let label = |name: String, main: bool, key: String, react: ActionSink| {
      ui::text(div(), TextSize::Xs)
        .id(SharedString::from(format!("terminal-workspace-label-{id}")))
        .debug_selector(|| "terminal-workspace-label".into())
        .max_w(zpx(200.))
        .truncate()
        .rounded_sm()
        .bg(if on_dark {
          d.foreground.opacity(0.15)
        } else {
          p.muted.opacity(0.6)
        })
        .px_1()
        .py_0p5()
        .font_family(theme::MONO_FONT)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(title_fg)
        .cursor_pointer()
        .hover(|s| {
          s.bg(if on_dark {
            d.foreground.opacity(0.25)
          } else {
            p.muted
          })
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
          react(
            ScreenAction::Field {
              field: "terminal.navigate".into(),
              value: serde_json::json!({ "key": key, "isMainRepo": main }),
            },
            cx,
          )
        })
        .child(name)
    };
    let close_id = column.id.clone();
    let close = div()
      .id(SharedString::from(format!("terminal-close-{}", column.id)))
      .debug_selector(|| "terminal-close".into())
      .flex()
      .items_center()
      .justify_center()
      .rounded_sm()
      .cursor_pointer()
      .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
      .on_click(cx.listener(move |pane, _, window, cx| pane.close(&close_id, window, cx)));
    let base = ui::text(div(), TextSize::Sm)
      .h(zpx(HEADER_HEIGHT))
      .min_h(zpx(HEADER_HEIGHT))
      .px_2()
      .flex()
      .items_center()
      .justify_between()
      .gap_2()
      .border_b_1()
      .border_r_1()
      .border_color(p.border);
    match &column.kind {
      Kind::Agent {
        name,
        workspace_path,
        workspace_name,
        repo_path,
        agent,
        ..
      } => {
        let key = workspace_path.clone().unwrap_or_else(|| repo_path.clone());
        let label_name = workspace_name.clone().unwrap_or_else(|| self.home_branch());
        let view = column.view.clone();
        let search = div()
          .id(SharedString::from(format!("terminal-search-{}", column.id)))
          .debug_selector(|| "terminal-search-button".into())
          .h_6()
          .px_2()
          .flex()
          .items_center()
          .rounded_md()
          .cursor_pointer()
          .hover(|s| s.bg(p.muted.opacity(0.2)))
          .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
          .on_click(cx.listener(move |_, _, _, cx| view.update(cx, |v, cx| v.open_search(cx))))
          .child(ui::icon(theme::ICON_SEARCH, 16., d.foreground));
        let search = self.tooltips.wrap(
          format!("terminal-search-tip-{}", column.id),
          Side::Top,
          Some(TooltipContent::Text(
            crate::gpui_shell::keys::hint("Search (⌘ + F)").into(),
          )),
          search,
          cx,
        );
        let close = self.tooltips.wrap(
          format!("terminal-close-tip-{}", column.id),
          Side::Top,
          Some(TooltipContent::Shortcut {
            text: "Close".into(),
            keys: crate::gpui_shell::keys::hint("⌘ + W").into(),
          }),
          close
            .h_6()
            .px_2()
            .rounded_md()
            .hover(|s| s.bg(p.muted.opacity(0.2)))
            .child(ui::icon(theme::ICON_X, 16., d.foreground)),
          cx,
        );
        base
          .bg(if active {
            theme::data_color(terminal_emu::DARK.background).blend(p.primary.opacity(0.4))
          } else {
            d.accent
          })
          .child(
            div()
              .flex()
              .items_center()
              .gap_1p5()
              .min_w_0()
              .child(ui::icon(agent_icon(agent), 16., d.foreground))
              .child(
                div()
                  .truncate()
                  .font_weight(gpui::FontWeight::MEDIUM)
                  .text_color(d.foreground)
                  .child(name.clone()),
              )
              .child(label(
                label_name,
                workspace_name.is_none(),
                key,
                self.react.clone(),
              )),
          )
          .child(
            div()
              .flex()
              .items_center()
              .gap_1()
              .child(search)
              .child(close),
          )
      }
      Kind::Remote { target } => {
        let view = column.view.clone();
        let (ended, stopping) = {
          let state = &view.read(cx).remote;
          (state.ended, state.stopping)
        };
        let muted = p.muted_foreground;
        let stop = ui::text(
          ui::Button::new("remote-terminal-stop", p)
            .variant(ui::Variant::Ghost)
            .size(ui::Size::Xs)
            .disabled(stopping)
            .h_6()
            .px_2(),
          TextSize::Xs,
        )
        .label("Stop")
        .on_click({
          let view = view;
          move |_, _, cx| view.update(cx, |v, cx| v.stop(cx))
        });
        let detach_id = column.id.clone();
        let detach = ui::Button::new("remote-terminal-detach", p)
          .variant(ui::Variant::Ghost)
          .size(ui::Size::IconXs)
          .icon(theme::ICON_X, 14.)
          .on_click(cx.listener(move |pane, _, window, cx| pane.close(&detach_id, window, cx)));
        let detach = self.tooltips.wrap(
          format!("remote-terminal-detach-tip-{}", column.id),
          Side::Bottom,
          Some(TooltipContent::Text(
            "Detach (the session keeps running)".into(),
          )),
          div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(detach),
          cx,
        );
        // Wrapped so the active column's selector does not replace this one.
        div().flex_shrink_0().child(
          ui::text(div(), TextSize::Sm)
            .debug_selector(|| "remote-terminal-header".into())
            .h(zpx(REMOTE_HEADER_HEIGHT))
            .min_h(zpx(REMOTE_HEADER_HEIGHT))
            .flex_shrink_0()
            .px_2()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_r_1()
            .border_color(p.border)
            .bg(p.background)
            .child(
              div()
                .flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(muted)
                .child(ui::icon(theme::ICON_TERMINAL_SQUARE, 14., muted).flex_shrink_0())
                .child(
                  div()
                    .truncate()
                    .child(format!("{} · {}", target.endpoint.hostname, target.label)),
                )
                .children(ended.map(|status| {
                  ui::text(div(), TextSize::Xs)
                    .debug_selector(|| "remote-terminal-ended".into())
                    .text_color(ui::Tone::Warning.text(p))
                    .child(match status {
                      Some(code) => format!("(session ended, exit {code})"),
                      None => "(session ended)".into(),
                    })
                })),
            )
            .child(
              div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                  div()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(stop),
                )
                .child(detach),
            ),
        )
      }
      Kind::Shell { dir } => {
        let (name, main) = self.label(dir);
        let close = self.tooltips.wrap(
          format!("terminal-close-tip-{}", column.id),
          Side::Top,
          Some(TooltipContent::Shortcut {
            text: "Close".into(),
            keys: crate::gpui_shell::keys::hint("⌘ + W").into(),
          }),
          close
            .size_4()
            .opacity(if on_dark { 0.85 } else { 0.6 })
            .hover(|s| s.opacity(1.))
            .child(ui::icon(theme::ICON_X, 12., icon_fg)),
          cx,
        );
        base
          .bg(if active {
            p.primary.opacity(0.2)
          } else {
            p.background
          })
          .child(
            div()
              .flex()
              .items_center()
              .gap_1p5()
              .min_w_0()
              .child(ui::icon(theme::ICON_TERMINAL, 12., title_fg))
              .child(
                div()
                  .font_weight(gpui::FontWeight::MEDIUM)
                  .text_color(title_fg)
                  .child("Shell"),
              )
              .child(label(name, main, dir.clone(), self.react.clone())),
          )
          .child(close)
      }
    }
  }

  fn column(&mut self, index: usize, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let header = self.header(index, p, cx);
    let remote = matches!(self.columns[index].kind, Kind::Remote { .. });
    let previews = if remote {
      None
    } else {
      self.previews(index, cx)
    };
    let min = self.min_width();
    let column = &self.columns[index];
    let (id, view, bounds) = (
      column.id.clone(),
      column.view.clone(),
      column.bounds.clone(),
    );
    let active = self.active.as_deref() == Some(id.as_str());
    let focus_id = id.clone();
    let double_id = id.clone();
    let name = match &column.kind {
      Kind::Agent { name, .. } => format!("{name} terminal"),
      Kind::Shell { .. } => "Shell terminal".into(),
      Kind::Remote { .. } => "Remote terminal".into(),
    };
    div()
      .id(SharedString::from(format!("terminal-column-{id}")))
      .role(gpui::Role::Group)
      .aria_label(name)
      .test_support()
      .debug_selector(move || {
        format!(
          "terminal-column-{}",
          if id.starts_with("agent-") {
            "agent"
          } else {
            "shell"
          }
        )
      })
      .flex()
      .flex_col()
      .min_h_0()
      .overflow_hidden()
      .flex_shrink_0()
      .when_some(column.width, |d, w| d.w(zpx(w)))
      .when(column.width.is_none(), |d| d.flex_1())
      .min_w(zpx(min))
      .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |pane, e: &MouseDownEvent, _, cx| {
          pane.activate(&focus_id, cx);
          if e.click_count == 2 {
            pane.scroll_fully_into_view(&double_id, cx);
          }
        }),
      )
      .child(
        gpui::canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
          .absolute()
          .size_full(),
      )
      .child(header.when(active, |d| {
        d.debug_selector(|| "terminal-column-active".into())
      }))
      .child({
        let (on_asset, on_files) = (view.clone(), view.clone());
        div()
          .relative()
          .flex_1()
          .min_h_0()
          .border_r_1()
          .border_color(p.border)
          .bg(theme::data_color(
            terminal_emu::scheme(self.dark).background,
          ))
          .on_drop(move |drag: &SendAssetDrag, _, cx| {
            on_asset.update(cx, |view, cx| view.insert(&shell_quote(&drag.path), cx));
          })
          .on_drop(move |paths: &ExternalPaths, _, cx| {
            let text: Vec<String> = paths
              .paths()
              .iter()
              .map(|p| shell_quote(&p.to_string_lossy()))
              .collect();
            if !text.is_empty() {
              on_files.update(cx, |view, cx| view.insert(&text.join(" "), cx));
            }
          })
          .child(view)
          .children(previews)
      })
      // `ring-1 ring-inset ring-primary/40` on the active remote session.
      .when(remote && active, |d| {
        d.relative().child(
          div()
            .absolute()
            .inset_0()
            .border_1()
            .border_color(p.primary.opacity(0.4)),
        )
      })
  }
}

/// A `treq send` attachment being dragged onto a terminal
/// (`setSendAssetDragData`).
#[derive(Clone)]
struct SendAssetDrag {
  path: String,
  title: String,
}

impl Render for SendAssetDrag {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let d = theme::palette(true);
    ui::text(div(), TextSize::Xs)
      .px_2()
      .py_1()
      .rounded(radius::MD)
      .border_1()
      .border_color(d.foreground.opacity(0.4))
      .bg(d.popover)
      .text_color(d.foreground)
      .shadow_md()
      .child(self.title.clone())
  }
}

/// Part of a cell: x, y, width and height as fractions of the cell.
pub type CellPart = (f32, f32, f32, f32);

/// A block element (U+2580 to U+259F) as xterm draws it, filling its cell
/// edge to edge instead of drawing the font's glyph: the parts of the cell
/// it fills (x, y, width, height, as fractions of the cell) and how opaque
/// they are (the shades).
pub fn block(c: char) -> Option<(Vec<CellPart>, f32)> {
  const UL: CellPart = (0., 0., 0.5, 0.5);
  const UR: CellPart = (0.5, 0., 0.5, 0.5);
  const LL: CellPart = (0., 0.5, 0.5, 0.5);
  const LR: CellPart = (0.5, 0.5, 0.5, 0.5);
  let full = (0., 0., 1., 1.);
  let code = c as u32;
  let rects = match code {
    0x2580 => vec![(0., 0., 1., 0.5)],
    0x2581..=0x2587 => {
      let part = (code - 0x2580) as f32 / 8.;
      vec![(0., 1. - part, 1., part)]
    }
    0x2588 => vec![full],
    0x2589..=0x258f => vec![(0., 0., (0x2590 - code) as f32 / 8., 1.)],
    0x2590 => vec![(0.5, 0., 0.5, 1.)],
    0x2591..=0x2593 => return Some((vec![full], (code - 0x2590) as f32 / 4.)),
    0x2594 => vec![(0., 0., 1., 0.125)],
    0x2595 => vec![(0.875, 0., 0.125, 1.)],
    0x2596 => vec![LL],
    0x2597 => vec![LR],
    0x2598 => vec![UL],
    0x2599 => vec![UL, LL, LR],
    0x259a => vec![UL, LR],
    0x259b => vec![UL, UR, LL],
    0x259c => vec![UL, UR, LR],
    0x259d => vec![UR],
    0x259e => vec![UR, LL],
    0x259f => vec![UR, LL, LR],
    _ => return None,
  };
  Some((rects, 1.))
}

/// A shade (`░▒▓`) as xterm's custom glyphs draw it: a 2x2 pattern of
/// device pixels repeated over the cell, a quarter, half or three quarters
/// of them lit.
fn paint_shade(
  origin: gpui::Point<Pixels>,
  (cell, height): (f32, f32),
  alpha: f32,
  color: gpui::Hsla,
  window: &mut Window,
) {
  let pattern: [[bool; 2]; 2] = if alpha <= 0.25 {
    [[true, false], [false, false]]
  } else if alpha <= 0.5 {
    [[true, false], [false, true]]
  } else {
    [[false, true], [true, true]]
  };
  let dot = 1. / window.scale_factor();
  let (cols, rows) = ((cell / dot) as usize, (height / dot) as usize);
  for row in 0..rows {
    for col in 0..cols {
      if pattern[row % 2][col % 2] {
        window.paint_quad(gpui::fill(
          Bounds::new(
            point(
              origin.x + px(col as f32 * dot),
              origin.y + px(row as f32 * dot),
            ),
            gpui::size(px(dot), px(dot)),
          ),
          color,
        ));
      }
    }
  }
}

/// Paints one run of cells, each glyph at the start of its cell, as
/// xterm's renderer does when the font's advance is not a whole pixel.
fn paint_run(
  run: &terminal_emu::Run,
  origin: gpui::Point<Pixels>,
  (cell, height, advance): (f32, f32, f32),
  font_size: Pixels,
  window: &mut Window,
  cx: &mut App,
) {
  let color = theme::data_color(run.fg);
  // Block elements fill their cells; the font draws the rest.
  let mut text = String::with_capacity(run.text.len());
  for (index, c) in run.text.chars().enumerate() {
    match block(c) {
      Some((_, alpha)) if alpha < 1. => {
        let cell_origin = point(origin.x + px(index as f32 * cell), origin.y);
        paint_shade(cell_origin, (cell, height), alpha, color, window);
        text.push(' ');
      }
      Some((rects, _)) => {
        for (x, y, w, h) in rects {
          let origin = point(
            origin.x + px((index as f32 + x) * cell),
            origin.y + px(y * height),
          );
          window.paint_quad(gpui::fill(
            Bounds::new(origin, gpui::size(px(w * cell), px(h * height))),
            color,
          ));
        }
        text.push(' ');
      }
      None => text.push(c),
    }
  }
  if text.trim().is_empty() && !run.underline && !run.strike {
    return;
  }
  let mut font = gpui::font(theme::MONO_FONT);
  if run.bold {
    font.weight = gpui::FontWeight::BOLD;
  }
  if run.italic {
    font.style = gpui::FontStyle::Italic;
  }
  let text_run = gpui::TextRun {
    len: text.len(),
    font,
    color,
    background_color: None,
    underline: run.underline.then_some(gpui::UnderlineStyle {
      thickness: px(1.),
      color: Some(color),
      wavy: false,
    }),
    strikethrough: run.strike.then_some(gpui::StrikethroughStyle {
      thickness: px(1.),
      color: Some(color),
    }),
  };
  let mut line = window
    .text_system()
    .shape_line(text.into(), font_size, &[text_run], None);
  if (advance - cell).abs() > f32::EPSILON {
    let layout: &gpui::LineLayout = &line;
    let snapped = gpui::LineLayout {
      font_size: layout.font_size,
      width: px(run.cells as f32 * cell),
      ascent: layout.ascent,
      descent: layout.descent,
      runs: layout
        .runs
        .iter()
        .map(|shaped| gpui::ShapedRun {
          font_id: shaped.font_id,
          glyphs: shaped
            .glyphs
            .iter()
            .map(|glyph| gpui::ShapedGlyph {
              position: point(
                px(cell_x(f32::from(glyph.position.x), advance, cell)),
                glyph.position.y,
              ),
              ..glyph.clone()
            })
            .collect(),
        })
        .collect(),
      len: layout.len,
    };
    *line = Arc::new(snapped);
  }
  let _ = line.paint(origin, px(height), gpui::TextAlign::Left, None, window, cx);
}

/// The emulator's name for a mouse button xterm reports.
fn mouse_button(button: MouseButton) -> Option<terminal_emu::Button> {
  match button {
    MouseButton::Left => Some(terminal_emu::Button::Left),
    MouseButton::Middle => Some(terminal_emu::Button::Middle),
    MouseButton::Right => Some(terminal_emu::Button::Right),
    _ => None,
  }
}

fn agent_icon(agent: &str) -> &'static str {
  match agent {
    "codex" => theme::ICON_AGENT_CODEX,
    "cursor" => theme::ICON_AGENT_CURSOR,
    "copilot" => theme::ICON_AGENT_COPILOT,
    _ => theme::ICON_AGENT_CLAUDE,
  }
}

impl Render for TerminalPane {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.watch_keys(window, cx);
    self.adopt(cx);
    if let Some(route) = self.incoming.take() {
      self.apply_route(route, window, cx);
    }
    if let Some(column) = self.focus_next.take() {
      self.focus_column(&column, window, cx);
    }
    for command in std::mem::take(&mut self.later) {
      self.run(&command, window, cx);
    }
    for (id, event) in std::mem::take(&mut self.events) {
      self.on_term_event(&id, event, window, cx);
    }
    self.tooltips.sync(cx);
    self.sync_lightbox(cx);
    self.load_environment(cx);
    let p = theme::palette(self.dark);
    // Without terminals the pane, chip included, takes no room.
    if self.columns.is_empty() {
      return div().into_any_element();
    }
    if self.collapsed {
      return div()
        .debug_selector(|| "workspace-terminal-pane".into())
        .flex()
        .justify_end()
        .m_1p5()
        .child(self.controls(&p, cx))
        .into_any_element();
    }
    if let Some(id) = self.reveal.take() {
      // Columns and dividers alternate in the row.
      if let Some(index) = self.columns.iter().position(|c| c.id == id) {
        self.row_scroll.scroll_to_item(index * 2);
      }
    }
    let mut row: Vec<gpui::AnyElement> = Vec::new();
    for index in 0..self.columns.len() {
      if index > 0 {
        let left = index - 1;
        row.push(
          div()
            .id(SharedString::from(format!("terminal-divider-{index}")))
            .relative()
            .w_1()
            .h_full()
            .flex_shrink_0()
            .bg(
              if matches!(self.drag, Some(Drag::Divider { left: l, .. }) if l == left) {
                p.primary
              } else {
                p.border
              },
            )
            .hover(|s| s.bg(p.primary.opacity(0.5)))
            .child(
              div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(zpx(-4.))
                .w_3()
                .cursor_ew_resize()
                .on_mouse_down(
                  MouseButton::Left,
                  cx.listener(move |pane, e: &MouseDownEvent, _, cx| {
                    pane.drag = Some(Drag::Divider {
                      left,
                      x: f32::from(e.position.x),
                    });
                    cx.stop_propagation();
                    cx.notify();
                  }),
                ),
            )
            .into_any_element(),
        );
      }
      row.push(self.column(index, &p, cx).into_any_element());
    }
    let row_bounds = self.row.clone();
    let scrollbar = self.row_scrollbar(&p, cx);
    let controls = self.controls(&p, cx);
    let maximized = self.maximized;
    // `height%` of the workspace view, as React sizes the pane, with the
    // divider above it.
    let view_height = self.container.get().map(|b| f32::from(b.size.height));
    div()
      .flex()
      .flex_col()
      .flex_shrink_0()
      .when(maximized, |d| d.flex_1().min_h_0())
      .when(!maximized, |d| match view_height {
        Some(view) => d.h(zpx(
          view * self.height.min(HEIGHT_MAX) / 100. + DIVIDER_HEIGHT,
        )),
        None => d.h(relative(self.height / 100.)),
      })
      // `activePtySessionId` clears on a click outside the pane.
      .on_mouse_down_out(cx.listener(|pane, e: &MouseDownEvent, _, cx| {
        // The controls sit above the pane's edge, but are part of it.
        let on_controls = pane
          .controls_bounds
          .get()
          .is_some_and(|b| b.contains(&e.position));
        if !on_controls && pane.active.take().is_some() {
          cx.notify();
        }
      }))
      // A drag follows the mouse anywhere in the window, as React's
      // document listeners do, not only over the pane.
      .when(self.drag.is_some(), |d| {
        let pane = cx.entity().downgrade();
        d.child(
          gpui::canvas(
            |_, _, _| {},
            move |_, _, window, _| {
              let (moved, released) = (pane.clone(), pane);
              window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == gpui::DispatchPhase::Bubble {
                  let _ = moved.update(cx, |pane, cx| pane.on_drag(e, cx));
                }
              });
              window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
                if phase == gpui::DispatchPhase::Bubble {
                  let _ = released.update(cx, |pane, cx| {
                    pane.drag = None;
                    cx.notify();
                  });
                }
              });
            },
          )
          .absolute()
          .size_0(),
        )
      })
      .when(!maximized, |d| {
        d.child(
          div()
            .relative()
            .h_1()
            .flex_shrink_0()
            .bg(if matches!(self.drag, Some(Drag::Height)) {
              p.primary
            } else {
              p.border
            })
            .hover(|s| s.bg(p.primary.opacity(0.5)))
            .child(
              div()
                .id("terminal-height-divider")
                .absolute()
                .left_0()
                .right_0()
                .top(zpx(-4.))
                .h_3()
                .cursor_ns_resize()
                .on_mouse_down(
                  MouseButton::Left,
                  cx.listener(|pane, _, _, cx| {
                    pane.drag = Some(Drag::Height);
                    cx.stop_propagation();
                    cx.notify();
                  }),
                ),
            ),
        )
      })
      .child(
        div()
          .debug_selector(|| "workspace-terminal-pane".into())
          .relative()
          .flex_1()
          .min_h_0()
          .flex()
          .flex_col()
          .border_t_1()
          .border_color(p.border)
          .bg(p.background)
          .child(
            div()
              .id("terminal-scroll-container")
              .debug_selector(|| "terminal-scroll-container".into())
              .flex_1()
              .min_h_0()
              .flex()
              .overflow_x_scroll()
              .track_scroll(&self.row_scroll)
              .bg(theme::data_color(
                terminal_emu::scheme(self.dark).background,
              ))
              .children(row)
              // After the columns, so a column's index in the row stays
              // what `scroll_to_item` counts.
              .child(
                gpui::canvas(move |b, _, _| row_bounds.set(Some(b)), |_, _, _, _| {})
                  .absolute()
                  .size_full(),
              ),
          )
          .children(scrollbar)
          .child(
            div()
              .absolute()
              .right(zpx(6.))
              .when(maximized, |d| d.bottom(zpx(6.)))
              .when(!maximized, |d| d.top(zpx(-28.)))
              .child(controls),
          ),
      )
      .children(self.lightbox.as_ref().map(|(_, lightbox, _)| {
        // Over the whole window, as React's portal to the body.
        deferred(
          anchored()
            .position(point(px(0.), px(0.)))
            .child(lightbox.clone()),
        )
        .with_priority(2)
      }))
      .into_any_element()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn scrolls_a_column_the_least_to_show_it() {
    let row = (0., 1000.);
    assert_eq!(scroll_delta(row, (100., 500.)), 0.);
    assert_eq!(scroll_delta(row, (800., 1200.)), 200.);
    assert_eq!(scroll_delta(row, (-150., 250.)), -150.);
    // Wider than the row: left edges line up.
    assert_eq!(scroll_delta(row, (300., 1500.)), 300.);
  }

  #[test]
  fn rounds_cells_down_to_whole_device_pixels() {
    assert_eq!(cell_size(8.4, 18.48, 1.), (8., 18.));
    assert_eq!(cell_size(8.4, 18.48, 2.), (8., 18.));
    assert_eq!(cell_size(7.2, 15.84, 2.), (7., 15.5));
  }

  #[test]
  fn draws_block_elements_as_cell_fractions() {
    assert_eq!(block('█'), Some((vec![(0., 0., 1., 1.)], 1.)));
    assert_eq!(block('▀'), Some((vec![(0., 0., 1., 0.5)], 1.)));
    assert_eq!(block('▂'), Some((vec![(0., 0.75, 1., 0.25)], 1.)));
    assert_eq!(block('▊'), Some((vec![(0., 0., 0.75, 1.)], 1.)));
    assert_eq!(block('▐'), Some((vec![(0.5, 0., 0.5, 1.)], 1.)));
    assert_eq!(block('▒'), Some((vec![(0., 0., 1., 1.)], 0.5)));
    assert_eq!(
      block('▚'),
      Some((vec![(0., 0., 0.5, 0.5), (0.5, 0.5, 0.5, 0.5)], 1.))
    );
    assert_eq!(block('a'), None);
  }

  #[test]
  fn places_glyphs_on_their_cells() {
    assert_eq!(cell_x(0., 8.4, 8.), 0.);
    assert_eq!(cell_x(8.4 * 3., 8.4, 8.), 24.);
    // A combining mark shaped just past its letter stays in its cell.
    assert_eq!(cell_x(8.4 * 3. + 0.5, 8.4, 8.), 24.);
  }

  #[test]
  fn sizes_the_row_scrollbar_to_what_shows() {
    // A row showing half of what it holds has a thumb half its width.
    assert_eq!(scrollbar_thumb(600., 600., 0.), (0., 300.));
    assert_eq!(scrollbar_thumb(600., 600., 600.), (300., 300.));
    assert_eq!(scrollbar_thumb(600., 600., 300.), (150., 300.));
    // The thumb stays big enough to grab.
    assert_eq!(scrollbar_thumb(100., 10_000., 0.).1, 32.);
  }

  #[test]
  fn resizes_column_pairs() {
    assert_eq!(resize_pair(400., 400., 50., 300.), (450., 350.));
    assert_eq!(resize_pair(400., 400., 150., 300.), (550., 300.));
    assert_eq!(resize_pair(400., 400., -200., 300.), (300., 500.));
  }

  #[test]
  fn clamps_the_pane_height() {
    assert!((drag_height(0., 1000., 700.) - 30.).abs() < 0.01);
    assert_eq!(drag_height(0., 1000., 990.), HEIGHT_MIN);
    assert_eq!(drag_height(0., 1000., 0.), HEIGHT_MAX);
  }

  #[test]
  fn focuses_the_neighbour_of_a_closed_column() {
    let order: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    assert_eq!(neighbour(&order, "a").as_deref(), Some("b"));
    assert_eq!(neighbour(&order, "b").as_deref(), Some("c"));
    assert_eq!(neighbour(&order, "c").as_deref(), Some("b"));
    assert_eq!(neighbour(&["a".to_string()], "a"), None);
  }

  #[test]
  fn reads_pane_shortcuts() {
    assert_eq!(
      shortcut("j", false, true, false, false),
      Some(Shortcut::ToggleCollapse)
    );
    assert_eq!(
      shortcut("j", true, false, false, false),
      Some(Shortcut::ToggleCollapse)
    );
    assert_eq!(
      shortcut("j", true, true, false, false),
      Some(Shortcut::ToggleMaximize)
    );
    assert_eq!(
      shortcut("]", false, true, false, false),
      Some(Shortcut::NewAgent)
    );
    assert_eq!(
      shortcut("\\", true, false, false, false),
      Some(Shortcut::NewShell)
    );
    assert_eq!(
      shortcut("w", false, true, false, false),
      Some(Shortcut::Close)
    );
    assert_eq!(
      shortcut("k", true, false, false, false),
      Some(Shortcut::App('k'))
    );
    assert_eq!(
      shortcut("n", true, false, true, false),
      Some(Shortcut::App('N'))
    );
    assert_eq!(shortcut("c", true, false, false, false), None);
    assert_eq!(shortcut("j", false, false, false, false), None);
  }
}
