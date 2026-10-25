//! Terminals in a workspace: persistent sessions on the host (tmux/screen
//! through `pty-remote`) running a shell or a coding agent. [`Sessions`]
//! lists them in the workspace's Terminals tab; [`TerminalScreen`] is one
//! session, attached, full screen. Leaving it detaches; the session keeps
//! running on the host.

use std::{collections::HashMap, rc::Rc, time::Duration};

use futures::channel::mpsc;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::{
  core::{
    pty_remote_supervisor::PtySessionInfo,
    remote::{RemoteHostCheck, TreqCommandRequest},
    remote_pty::{AttachCommandCache, PtyLaunchSpec, RemoteAgentId, RemotePtyBinding},
  },
  local_db::Workspace,
};

use super::workspace::Target;
use crate::{
  backend, errors, remote,
  ui::{
    self, load, nav, sheet,
    terminal_view::{PtyEvent, TerminalView},
    widgets, Load, Location,
  },
};

/// Attach commands the host answered, kept for re-attaching.
static ATTACH_COMMANDS: std::sync::LazyLock<AttachCommandCache> =
  std::sync::LazyLock::new(AttachCommandCache::default);

/// Size used to start a session, until the view measures itself.
const INITIAL_SIZE: (u16, u16) = (80, 24);

/// How long a reattached session must stay up before a later drop gets
/// its own automatic reattach.
const AUTO_REATTACH_RESET: Duration = Duration::from_secs(10);

/// The gap between a sent message and its Enter, so an agent's TUI reads
/// the text as typed rather than pasted (a pasted newline doesn't submit).
const SUBMIT_DELAY: Duration = Duration::from_millis(60);

/// A toolbar key: a named key (encoded for the program's cursor mode), a
/// control combination, or fixed bytes.
#[derive(Clone, Copy)]
enum Key {
  Named(&'static str),
  Ctrl(&'static str),
  Bytes(&'static [u8]),
}

impl Key {
  fn bytes(self, application_cursor: bool) -> Vec<u8> {
    match self {
      Self::Named(key) => crate::terminal::key_bytes(key, false, false, application_cursor),
      Self::Ctrl(key) => crate::terminal::key_bytes(key, true, false, application_cursor),
      Self::Bytes(bytes) => Some(bytes.to_vec()),
    }
    .unwrap_or_default()
  }
}

/// Shell keys a phone keyboard lacks.
const SHELL_KEYS: &[(&str, Key)] = &[
  ("Esc", Key::Named("escape")),
  ("⇥", Key::Named("tab")),
  ("^C", Key::Ctrl("c")),
  ("^D", Key::Ctrl("d")),
  ("^Z", Key::Ctrl("z")),
  ("←", Key::Named("left")),
  ("↑", Key::Named("up")),
  ("↓", Key::Named("down")),
  ("→", Key::Named("right")),
];

/// Keys an agent's TUI asks for: interrupt, mode switch and menu movement.
const AGENT_KEYS: &[(&str, Key)] = &[
  ("Esc", Key::Named("escape")),
  ("⇧⇥", Key::Bytes(b"\x1b[Z")),
  ("↑", Key::Named("up")),
  ("↓", Key::Named("down")),
  ("⏎", Key::Named("enter")),
  ("^C", Key::Ctrl("c")),
  ("/", Key::Bytes(b"/")),
];

/// What a session runs. Sessions carry it in their label's prefix, so the
/// list can tell agents from shells without asking the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
  Shell,
  Agent(RemoteAgentId),
}

impl Program {
  pub fn of_label(label: &str) -> Self {
    let prefix = label.split('-').next().unwrap_or_default();
    RemoteAgentId::ALL
      .into_iter()
      .find(|agent| label_prefix(*agent) == prefix)
      .map_or(Self::Shell, Self::Agent)
  }

  pub fn name(self) -> &'static str {
    match self {
      Self::Shell => "Shell",
      Self::Agent(agent) => agent_name(agent),
    }
  }

  fn icon(self) -> IconName {
    match self {
      Self::Shell => IconName::SquareTerminal,
      Self::Agent(_) => IconName::Bot,
    }
  }

  fn keys(self) -> &'static [(&'static str, Key)] {
    match self {
      Self::Shell => SHELL_KEYS,
      Self::Agent(_) => AGENT_KEYS,
    }
  }

  fn launch(self, args: Vec<String>) -> PtyLaunchSpec {
    match self {
      Self::Shell => PtyLaunchSpec::Shell,
      Self::Agent(agent) => PtyLaunchSpec::Agent { agent, args },
    }
  }

  fn prefix(self) -> &'static str {
    match self {
      Self::Shell => "shell",
      Self::Agent(agent) => label_prefix(agent),
    }
  }
}

pub fn agent_name(agent: RemoteAgentId) -> &'static str {
  match agent {
    RemoteAgentId::Claude => "Claude Code",
    RemoteAgentId::Codex => "Codex",
    RemoteAgentId::CursorAgent => "Cursor Agent",
  }
}

/// Label prefixes have no `-`, so the prefix is the label's first part.
fn label_prefix(agent: RemoteAgentId) -> &'static str {
  match agent {
    RemoteAgentId::Claude => "claude",
    RemoteAgentId::Codex => "codex",
    RemoteAgentId::CursorAgent => "cursor",
  }
}

/// The remote directory a workspace's terminal starts in.
pub fn workspace_dir(repo: &str, workspace: &Workspace) -> String {
  if workspace.workspace_path.starts_with('/') {
    workspace.workspace_path.clone()
  } else {
    format!(
      "{}/.treq/workspaces/{}",
      repo.trim_end_matches('/'),
      workspace.workspace_path
    )
  }
}

/// A label for a new session of `program`, unique per launch.
pub fn new_session_label(program: Program, now_ms: i64) -> String {
  let mut n = now_ms.max(0) as u64;
  let mut digits = Vec::new();
  loop {
    digits.push(std::char::from_digit((n % 36) as u32, 36).unwrap());
    n /= 36;
    if n == 0 {
      break;
    }
  }
  format!(
    "{}-{}",
    program.prefix(),
    digits.into_iter().rev().collect::<String>()
  )
}

/// What the active machine can run, from its last connect.
fn host_check(cx: &App) -> Option<RemoteHostCheck> {
  let model = ui::model(cx).read(cx);
  let machine = model.connection.as_ref()?.machine().to_string();
  model.machine(&machine).check
}

/// The agents a host offers: those it reported installed, or every one
/// when its treq predates host checks.
fn agents_on(check: Option<&RemoteHostCheck>) -> Vec<RemoteAgentId> {
  match check {
    Some(check) => check.agents.clone(),
    None => RemoteAgentId::ALL.to_vec(),
  }
}

/// The sessions in a workspace, for its Terminals tab.
pub struct Sessions {
  target: Target,
  workspace: Option<Workspace>,
  sessions: Load<Vec<PtySessionInfo>>,
  notice: Option<String>,
  /// Unsent messages by session label, kept while the session's page is
  /// closed (to read a diff, say) so the message is there on return.
  drafts: HashMap<String, String>,
}

fn list_request(repo: &str, workspace: &Workspace) -> TreqCommandRequest {
  TreqCommandRequest::PtyList {
    repo: repo.to_string(),
    workspace: Some(workspace.workspace_name.clone()),
  }
}

impl Sessions {
  /// Lists at once when `workspace`, the record, is already known.
  pub fn new(target: Target, workspace: Option<Workspace>, cx: &mut Context<Self>) -> Self {
    let sessions = workspace
      .as_ref()
      .and_then(|w| remote::last(&list_request(&target.repo, w)))
      .map_or(Load::Idle, Load::Ready);
    let mut this = Self {
      target,
      workspace,
      sessions,
      notice: None,
      drafts: HashMap::new(),
    };
    this.fetch(true, cx);
    this
  }

  /// The workspace record (from the workspace's status), which names its
  /// sessions and directory.
  pub fn set_workspace(&mut self, workspace: Workspace, cx: &mut Context<Self>) {
    let first = self.workspace.is_none();
    self.workspace = Some(workspace);
    if first {
      self.fetch(true, cx);
    }
  }

  pub fn running(&self) -> usize {
    self
      .sessions
      .ready()
      .map_or(0, |list| list.iter().filter(|s| s.running).count())
  }

  pub fn reload(&mut self, cx: &mut Context<Self>) {
    self.fetch(false, cx);
  }

  /// `join` as in [`remote::read_cached`].
  fn fetch(&mut self, join: bool, cx: &mut Context<Self>) {
    let Some(workspace) = &self.workspace else {
      return;
    };
    if self.sessions.ready().is_none() {
      self.sessions = Load::Loading;
    }
    let request = list_request(&self.target.repo, workspace);
    load::run(
      cx,
      remote::read_cached::<Vec<PtySessionInfo>>(request, join),
      |this, result, cx| {
        this.sessions = Load::from_result(result);
        cx.notify();
      },
    );
  }

  /// The running agent sessions, by label, that a review can go to.
  pub fn running_agents(&self) -> Vec<(String, RemoteAgentId)> {
    self.sessions.ready().map_or_else(Vec::new, |list| {
      list
        .iter()
        .filter(|s| s.running)
        .filter_map(|s| match Program::of_label(&s.label) {
          Program::Agent(agent) => Some((s.label.clone(), agent)),
          Program::Shell => None,
        })
        .collect()
    })
  }

  /// The agents a new session can run on this host.
  pub fn agents_offered(cx: &App) -> Vec<RemoteAgentId> {
    agents_on(host_check(cx).as_ref())
  }

  /// Pastes `text` into running session `label` and submits it, then opens
  /// the session. `done` hears whether the host took it.
  pub fn paste_into(
    &mut self,
    label: String,
    text: String,
    done: impl FnOnce(Result<(), String>, &mut App) + 'static,
    cx: &mut Context<Self>,
  ) {
    let Some(workspace) = &self.workspace else {
      return done(Err("This workspace is still loading.".into()), cx);
    };
    let request = TreqCommandRequest::PtyPaste {
      repo: self.target.repo.clone(),
      workspace: workspace.workspace_name.clone(),
      label: label.clone(),
      text,
      submit: true,
      idempotency_key: format!("pty.paste:{}", uuid::Uuid::new_v4()),
    };
    load::run(cx, remote::mutate(request), move |this, outcome, cx| {
      let (ok, message) = super::mutation_message(&outcome, "");
      if ok {
        let program = Program::of_label(&label);
        this.open(label, program.launch(Vec::new()), cx);
        done(Ok(()), cx);
      } else {
        done(Err(errors::describe(&message).message), cx);
      }
    });
  }

  /// Starts a new `agent` session with `args`, and opens it.
  pub fn start_agent(&mut self, agent: RemoteAgentId, args: Vec<String>, cx: &mut Context<Self>) {
    self.start(Program::Agent(agent), args, cx);
  }

  fn start(&mut self, program: Program, args: Vec<String>, cx: &mut Context<Self>) {
    let label = new_session_label(program, chrono::Utc::now().timestamp_millis());
    self.open(label, program.launch(args), cx);
  }

  /// Attaches to `label`, starting it with `launch` if it isn't running.
  fn open(&mut self, label: String, launch: PtyLaunchSpec, cx: &mut Context<Self>) {
    let Some(workspace) = self.workspace.clone() else {
      return;
    };
    self.notice = None;
    TerminalScreen::open(
      self.target.clone(),
      workspace,
      label,
      launch,
      cx.weak_entity(),
      cx,
    );
  }

  fn new_agent(&mut self, cx: &mut Context<Self>) {
    self.new_agent_with(String::new(), cx)
  }

  /// The New agent sheet with `message` already typed as its first message.
  pub fn new_agent_with(&mut self, message: String, cx: &mut Context<Self>) {
    let agents = agents_on(host_check(cx).as_ref());
    let sessions = cx.weak_entity();
    sheet::open(Some("New agent".into()), cx, move |window, cx| {
      cx.new(|cx| AgentSheet::new(agents, sessions, message, window, cx))
    });
  }

  fn session_rows(&self, cx: &mut Context<Self>) -> AnyElement {
    match &self.sessions {
      Load::Idle | Load::Loading => widgets::card(cx)
        .child(widgets::centered_message(
          "Checking for running sessions…",
          cx,
        ))
        .into_any_element(),
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(
          errors::describe(error).message,
          cx,
        ))
        .into_any_element(),
      Load::Ready(list) if list.is_empty() => widgets::card(cx)
        .child(widgets::centered_message(
          "No terminals in this workspace. Start an agent or a shell; it keeps running on the host when you leave.",
          cx,
        ))
        .into_any_element(),
      Load::Ready(list) => {
        let theme = cx.theme();
        widgets::card(cx)
          .children(list.iter().enumerate().map(|(i, session)| {
            let program = Program::of_label(&session.label);
            let label = session.label.clone();
            let leading = div()
              .text_color(if session.running {
                theme.foreground
              } else {
                theme.muted_foreground
              })
              .child(program.icon())
              .into_any_element();
            let detail = if session.running {
              session.label.clone()
            } else {
              format!("{} · exited", session.label)
            };
            widgets::row_full(
              ("session", i),
              Some(leading),
              program.name(),
              Some(detail.into()),
              None,
              cx,
            )
            .when(!session.running, |row| row.opacity(0.6))
            .on_click(cx.listener(move |this, _, _, cx| {
              // A running session ignores the launch; an exited one
              // starts its program again.
              this.open(label.clone(), program.launch(Vec::new()), cx)
            }))
          }))
          .into_any_element()
      }
    }
  }
}

impl Render for Sessions {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let check = host_check(cx);
    let no_multiplexer = check.as_ref().is_some_and(|c| c.terminal.is_none());
    let no_agents = agents_on(check.as_ref()).is_empty();
    let ready = self.workspace.is_some() && !no_multiplexer;
    v_flex()
      .gap_3()
      .children(self.notice.clone().map(|n| widgets::muted(n, cx)))
      .when(no_multiplexer, |list| {
        list.child(widgets::error_text(
          "Terminals need tmux or screen on the host. Install one there, then reconnect.",
          cx,
        ))
      })
      .child(self.session_rows(cx))
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("new-agent")
              .primary()
              .large()
              .flex_1()
              .icon(IconName::Bot)
              .label("New agent")
              .disabled(!ready || no_agents)
              .on_click(cx.listener(|this, _, _, cx| this.new_agent(cx))),
          )
          .child(
            Button::new("new-shell")
              .outline()
              .large()
              .flex_1()
              .icon(IconName::SquareTerminal)
              .label("New shell")
              .disabled(!ready)
              .on_click(cx.listener(|this, _, _, cx| {
                this.start(Program::Shell, Vec::new(), cx)
              })),
          ),
      )
      .when(no_agents && !no_multiplexer, |list| {
        list.child(widgets::muted(
          "No coding agents found on this host. Install Claude Code, Codex or Cursor Agent there, then reconnect.",
          cx,
        ))
      })
  }
}

/// Picking an agent and, optionally, what to ask it first.
struct AgentSheet {
  agents: Vec<RemoteAgentId>,
  selected: usize,
  message: Entity<InputState>,
  sessions: WeakEntity<Sessions>,
}

impl AgentSheet {
  fn new(
    agents: Vec<RemoteAgentId>,
    sessions: WeakEntity<Sessions>,
    message: String,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let message = cx.new(|cx| {
      InputState::new(window, cx)
        .placeholder("First message (optional)")
        .default_value(message)
    });
    Self {
      agents,
      selected: 0,
      message,
      sessions,
    }
  }

  fn start(&mut self, cx: &mut Context<Self>) {
    let Some(agent) = self.agents.get(self.selected).copied() else {
      return;
    };
    let message = self.message.read(cx).value().trim().to_string();
    let args = if message.is_empty() {
      Vec::new()
    } else {
      vec![message]
    };
    sheet::close(cx);
    self
      .sessions
      .update(cx, |sessions, cx| {
        sessions.start(Program::Agent(agent), args, cx)
      })
      .ok();
  }
}

impl Render for AgentSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let selected = self.selected;
    let start_label = self
      .agents
      .get(selected)
      .map(|a| format!("Start {}", agent_name(*a)))
      .unwrap_or_else(|| "Start".into());
    v_flex()
      .gap_3()
      .child(
        v_flex().children(self.agents.iter().enumerate().map(|(i, agent)| {
          widgets::choice(
            ("agent", i),
            agent_name(*agent),
            Some(agent.binary_name().into()),
            i == selected,
            cx,
          )
          .on_click(cx.listener(move |this, _, _, cx| {
            this.selected = i;
            cx.notify();
          }))
        })),
      )
      .child(
        v_flex()
          .px_4()
          .gap_3()
          .child(Input::new(&self.message).large())
          .child(
            Button::new("start-agent")
              .primary()
              .large()
              .w_full()
              .h(px(widgets::CTA_H - 8.))
              .label(start_label)
              .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
          ),
      )
  }
}

/// One session, attached, with the keys and message line a phone needs.
pub struct TerminalScreen {
  target: Target,
  workspace: Workspace,
  label: String,
  program: Program,
  view: Load<Entity<TerminalView>>,
  /// Set when the session ended, with its exit status (`None` when the
  /// connection dropped). Its last screen stays up so its final output
  /// (an error, a summary) can still be read.
  ended: Option<Option<u32>>,
  /// The automatic reattach after a dropped connection was used. A
  /// session that stays up again gives it back, so each drop gets one
  /// before Reattach is offered.
  auto_spent: bool,
  /// Counts attaches, so a timer can tell whether its attach is current.
  attaches: usize,
  message: Entity<InputState>,
  sessions: WeakEntity<Sessions>,
}

impl TerminalScreen {
  fn open(
    target: Target,
    workspace: Workspace,
    label: String,
    launch: PtyLaunchSpec,
    sessions: WeakEntity<Sessions>,
    cx: &mut App,
  ) {
    let program = Program::of_label(&label);
    let location = Location {
      repo: Some(target.repo.clone()),
      workspace: Some(crate::store::SnapshotWorkspace {
        id: target.id.clone(),
        name: target.name.clone(),
      }),
    };
    let screen: Rc<std::cell::RefCell<Option<WeakEntity<Self>>>> = Default::default();
    let menu_screen = screen.clone();
    let header = nav::Header {
      on_title: None,
      more: Some(Rc::new(move |_, cx| {
        let Some(screen) = menu_screen.borrow().clone() else {
          return;
        };
        sheet::menu(
          program.name(),
          vec![
            sheet::MenuEntry::new("stop", "Stop session…", move |_, cx| {
              screen.update(cx, |screen, cx| screen.confirm_stop(cx)).ok();
            })
            .danger(),
          ],
          cx,
        );
      })),
    };
    nav::push_with(program.name(), header, location, cx, move |window, cx| {
      let entity = cx.new(|cx| {
        Self::new(
          target, workspace, label, program, launch, sessions, window, cx,
        )
      });
      *screen.borrow_mut() = Some(entity.downgrade());
      entity
    });
  }

  #[expect(clippy::too_many_arguments, reason = "the screen needs its target, PTY spec and GPUI handles")]
  fn new(
    target: Target,
    workspace: Workspace,
    label: String,
    program: Program,
    launch: PtyLaunchSpec,
    sessions: WeakEntity<Sessions>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let draft = sessions
      .upgrade()
      .and_then(|list| list.update(cx, |list, _| list.drafts.remove(&label)))
      .unwrap_or_default();
    let message = cx.new(|cx| {
      InputState::new(window, cx)
        .placeholder(format!("Message {}", program.name()))
        .default_value(draft)
    });
    cx.subscribe_in(
      &message,
      window,
      |this, _, event: &InputEvent, window, cx| {
        if let InputEvent::PressEnter { .. } = event {
          this.send(window, cx);
        }
      },
    )
    .detach();
    // Leaving the page detaches; the list then shows what's still running
    // and keeps the unsent message for the next visit.
    let list = sessions.clone();
    cx.on_release(move |this: &mut Self, cx| {
      let draft = this.message.read(cx).value().to_string();
      let label = this.label.clone();
      let list = list.clone();
      cx.defer(move |cx| {
        list
          .update(cx, |list, cx| {
            if !draft.trim().is_empty() {
              list.drafts.insert(label, draft);
            }
            list.reload(cx)
          })
          .ok();
      });
    })
    .detach();
    let mut this = Self {
      target,
      workspace,
      label,
      program,
      view: Load::Loading,
      ended: None,
      auto_spent: false,
      attaches: 0,
      message,
      sessions,
    };
    this.attach(launch, window, cx);
    this
  }

  fn attach(&mut self, launch: PtyLaunchSpec, window: &mut Window, cx: &mut Context<Self>) {
    let endpoint = match backend::get().endpoint() {
      Ok(endpoint) => endpoint,
      Err(error) => {
        self.view = Load::Failed(error);
        return;
      }
    };
    self.view = Load::Loading;
    self.attaches += 1;
    let attach_id = self.attaches;
    let repo = self.target.repo.clone();
    let workspace = self.workspace.clone();
    let label = self.label.clone();
    let remote_dir = workspace_dir(&repo, &workspace);
    let session_id = format!("gpui-{label}-{}", uuid::Uuid::new_v4());
    let (tx, rx) = mpsc::unbounded::<PtyEvent>();
    let request = TreqCommandRequest::PtyAttachCommand {
      repo: repo.clone(),
      workspace: workspace.workspace_name.clone(),
      label,
      remote_dir: remote_dir.clone(),
      launch,
      cols: INITIAL_SIZE.0,
      rows: INITIAL_SIZE.1,
    };
    let binding = RemotePtyBinding {
      endpoint_id: endpoint.id.clone(),
      repository_id: repo,
      workspace_id: workspace.workspace_name.clone(),
      remote_working_directory: remote_dir,
      local_session_id: session_id.clone(),
      window_label: None,
    };
    let attach = async move {
      // Re-attaching the same session skips asking the host for the command.
      let cache_key = AttachCommandCache::key(&endpoint, &request);
      let command = match ATTACH_COMMANDS.get(&cache_key) {
        Some(command) => command,
        None => {
          let command = remote::read::<String>(request).await?;
          ATTACH_COMMANDS.insert(cache_key.clone(), command.clone());
          command
        }
      };
      let output = tx.clone();
      let attached = backend::get()
        .ptys
        .create_with_command(
          binding,
          &endpoint,
          &command,
          INITIAL_SIZE.0,
          INITIAL_SIZE.1,
          move |bytes| {
            let _ = output.unbounded_send(PtyEvent::Output(bytes));
          },
          move |status| {
            let _ = tx.unbounded_send(PtyEvent::Exited(status));
          },
        )
        .await
        .map_err(|e| e.to_string());
      if attached.is_err() {
        // The host may have changed under the cached command.
        ATTACH_COMMANDS.forget(&cache_key);
      }
      attached
    };
    let screen = cx.weak_entity();
    load::run_in(window, cx, attach, move |this, result, window, cx| {
      this.view = match result {
        Ok(()) => {
          let view = cx.new(|cx| {
            TerminalView::new(
              session_id,
              INITIAL_SIZE,
              rx,
              move |status, window, cx| {
                screen
                  .update(cx, |this, cx| this.ended(status, window, cx))
                  .ok();
              },
              window,
              cx,
            )
          });
          if this.program == Program::Shell {
            view.update(cx, |view, cx| view.focus(window, cx));
          }
          cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTO_REATTACH_RESET).await;
            this
              .update(cx, |this, _| {
                if this.attaches == attach_id && this.ended.is_none() {
                  this.auto_spent = false;
                }
              })
              .ok();
          })
          .detach();
          Load::Ready(view)
        }
        Err(error) => Load::Failed(errors::describe(&error).message),
      };
      cx.notify();
    });
  }

  /// The session's program exited, or the channel dropped. A drop
  /// reattaches once on its own; after that, or when the program exited,
  /// the last screen stays up with Reattach (or Start again).
  fn ended(&mut self, status: Option<u32>, window: &mut Window, cx: &mut Context<Self>) {
    if status.is_none() && !self.auto_spent {
      self.auto_spent = true;
      self.attach(self.program.launch(Vec::new()), window, cx);
      cx.notify();
      return;
    }
    self.ended = Some(status);
    cx.notify();
  }

  fn ended_reason(&self, status: Option<u32>) -> String {
    let name = self.program.name();
    match status {
      Some(0) => format!("{name} exited."),
      Some(code) => format!("{name} exited (code {code})."),
      None => "The connection dropped. The session may still be running on the host.".into(),
    }
  }

  /// Attaches again: a running session continues, an exited one starts
  /// its program again.
  fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.ended = None;
    self.attach(self.program.launch(Vec::new()), window, cx);
    cx.notify();
  }

  fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Load::Ready(view) = &self.view else {
      return;
    };
    let text = self.message.read(cx).value().to_string();
    if text.trim().is_empty() {
      return;
    }
    self
      .message
      .update(cx, |state, cx| state.set_value("", window, cx));
    let view = view.clone();
    view.update(cx, |view, cx| view.write(text.into_bytes(), cx));
    cx.spawn(async move |_, cx| {
      cx.background_executor().timer(SUBMIT_DELAY).await;
      view.update(cx, |view, cx| view.write(b"\r".to_vec(), cx));
    })
    .detach();
  }

  fn confirm_stop(&mut self, cx: &mut Context<Self>) {
    let this = cx.weak_entity();
    sheet::confirm(
      format!("Stop {}?", self.program.name()),
      "Ends the session on the host. Anything still running in it stops.",
      "Stop session",
      true,
      move |_, cx| {
        this.update(cx, |screen, cx| screen.stop(cx)).ok();
      },
      cx,
    );
  }

  fn stop(&mut self, cx: &mut Context<Self>) {
    let request = TreqCommandRequest::PtyStop {
      repo: self.target.repo.clone(),
      workspace: self.workspace.workspace_name.clone(),
      label: self.label.clone(),
    };
    let name = self.program.name();
    load::run(
      cx,
      remote::read::<serde_json::Value>(request),
      move |this, result, cx| {
        let notice = match result {
          Ok(_) => format!("{name} stopped."),
          Err(error) => errors::describe(&error).message,
        };
        this
          .sessions
          .update(cx, |list, cx| {
            list.notice = Some(notice);
            cx.notify();
          })
          .ok();
        nav::pop(cx);
      },
    );
  }

  fn key_row(&self, view: &Entity<TerminalView>, cx: &App) -> impl IntoElement {
    h_flex()
      .id("keys")
      .gap_1()
      .p_1()
      .overflow_x_scroll()
      .bg(cx.theme().secondary)
      .children(
        self
          .program
          .keys()
          .iter()
          .enumerate()
          .map(|(i, (label, key))| {
            let view = view.clone();
            Button::new(("key", i))
              .outline()
              .small()
              .flex_shrink_0()
              .min_w(px(36.))
              .label(*label)
              .on_click(move |_, window, cx| {
                view.update(cx, |view, cx| {
                  let bytes = key.bytes(view.application_cursor());
                  view.write(bytes, cx);
                  view.focus(window, cx);
                });
              })
          }),
      )
  }
}

impl Render for TerminalScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let theme = cx.theme();
    let view = match &self.view {
      Load::Ready(view) => view.clone(),
      Load::Idle | Load::Loading => {
        return widgets::centered_message(format!("Attaching to {}…", self.label), cx)
          .size_full()
          .into_any_element();
      }
      Load::Failed(error) => {
        return widgets::page()
          .child(widgets::error_text(error.clone(), cx))
          .child(
            Button::new("back")
              .outline()
              .large()
              .label("Back to terminals")
              .on_click(|_, _, cx| nav::pop(cx)),
          )
          .into_any_element();
      }
    };
    let agent = matches!(self.program, Program::Agent(_));
    v_flex()
      .size_full()
      .child(
        h_flex()
          .px_3()
          .py_1()
          .border_b_1()
          .border_color(theme.border)
          .child(widgets::mono(self.label.clone(), cx).text_xs()),
      )
      .child(div().flex_1().min_h_0().child(view.clone()))
      .when_some(self.ended, |screen, status| {
        screen.child(
          v_flex()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(theme.border)
            .child(widgets::muted(self.ended_reason(status), cx))
            .child(
              h_flex()
                .gap_2()
                .child(
                  Button::new("restart")
                    .primary()
                    .flex_1()
                    .label(if status.is_none() {
                      "Reattach"
                    } else {
                      "Start again"
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.restart(window, cx))),
                )
                .child(
                  Button::new("leave")
                    .outline()
                    .flex_1()
                    .label("Back to terminals")
                    .on_click(|_, _, cx| nav::pop(cx)),
                ),
            ),
        )
      })
      .when(self.ended.is_none(), |screen| {
        screen.child(self.key_row(&view, cx))
      })
      .when(agent && self.ended.is_none(), |screen| {
        screen.child(
          h_flex()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(theme.border)
            .child(div().flex_1().child(Input::new(&self.message)))
            .child(
              Button::new("send")
                .primary()
                .icon(IconName::ArrowUp)
                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
            ),
        )
      })
      .into_any_element()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[core::prelude::v1::test]
  fn labels_carry_their_program() {
    assert_eq!(new_session_label(Program::Shell, 0), "shell-0");
    assert_eq!(new_session_label(Program::Shell, 36 * 36), "shell-100");
    for agent in RemoteAgentId::ALL {
      let label = new_session_label(Program::Agent(agent), 1_700_000_000_000);
      assert_eq!(Program::of_label(&label), Program::Agent(agent));
    }
    assert_eq!(Program::of_label("shell-abc"), Program::Shell);
    // Sessions from other clients keep working as shells.
    assert_eq!(Program::of_label("main"), Program::Shell);
  }

  #[core::prelude::v1::test]
  fn arrow_keys_follow_the_cursor_mode() {
    let up = Key::Named("up");
    assert_eq!(up.bytes(false), b"\x1b[A");
    assert_eq!(up.bytes(true), b"\x1bOA");
    assert_eq!(Key::Ctrl("c").bytes(true), b"\x03");
    assert_eq!(Key::Bytes(b"\x1b[Z").bytes(true), b"\x1b[Z");
  }

  #[core::prelude::v1::test]
  fn hosts_without_a_check_offer_every_agent() {
    assert_eq!(agents_on(None), RemoteAgentId::ALL.to_vec());
    let check = RemoteHostCheck {
      treq_version: "0.4.0".into(),
      root: "/home/dev".into(),
      root_exists: true,
      terminal: Some("tmux".into()),
      agents: vec![RemoteAgentId::Codex],
    };
    assert_eq!(agents_on(Some(&check)), vec![RemoteAgentId::Codex]);
  }

  #[core::prelude::v1::test]
  fn workspace_dirs_resolve_under_the_repository() {
    let workspace: Workspace = serde_json::from_value(serde_json::json!({
      "id": 1, "repo_path": "/r", "workspace_name": "w", "workspace_path": "feature-x",
      "branch_name": "b", "created_at": "", "refreshed_at": null, "metadata": null,
      "target_branch": null, "title": "", "description": null, "moved_files": null,
      "not_on_remote": false, "sparse_patterns": null
    }))
    .unwrap();
    assert_eq!(
      workspace_dir("/r/", &workspace),
      "/r/.treq/workspaces/feature-x"
    );
  }
}
