//! A coding agent in the workspace: start, follow its log, send input, stop.

use std::time::Duration;

use gpui_kit::component::Selectable as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    v_flex, ActiveTheme, Sizable,
  },
  *,
};
use treq_lib::core::{agent_supervisor::AgentStatusResult, remote::TreqCommandRequest};

use super::{mutation_message, on_epoch, workspace::Target, Confirm};
use crate::{
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, nav, widgets, Load, Location},
};

/// The agents the host's supervisor allows (`agent_supervisor.rs`).
const AGENTS: [&str; 4] = ["claude", "codex", "cursor-agent", "copilot"];
const POLL: Duration = Duration::from_secs(4);
/// Log lines kept on screen.
const LOG_TAIL: usize = 400;

pub fn open(target: Target, cx: &mut App) {
  let location = Location {
    repo: Some(target.repo.clone()),
    workspace: Some(crate::store::SnapshotWorkspace {
      id: target.id.clone(),
      name: target.name.clone(),
    }),
  };
  nav::push("Agent", location, cx, move |window, cx| {
    cx.new(|cx| AgentScreen::new(target, window, cx))
  });
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
  Start,
  Send,
  Stop,
}

pub struct AgentScreen {
  target: Target,
  status: Load<AgentStatusResult>,
  logs: String,
  agent: usize,
  prompt: Entity<TextareaState>,
  input: Entity<TextareaState>,
  confirm: Confirm<Action>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  scroll: ScrollHandle,
  _poll: Task<()>,
}

fn textarea(placeholder: &str, window: &mut Window, cx: &mut App) -> Entity<TextareaState> {
  let placeholder = placeholder.to_string();
  cx.new(|cx| {
    TextareaState::new(window, cx)
      .placeholder(placeholder)
      .rows(3)
  })
}

impl AgentScreen {
  fn new(target: Target, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let poll = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(POLL).await;
      if this.update(cx, |this, cx| this.reload(cx)).is_err() {
        break;
      }
    });
    let mut this = Self {
      target,
      status: Load::Idle,
      logs: String::new(),
      agent: 0,
      prompt: textarea("Prompt", window, cx),
      input: textarea("Send input to the agent", window, cx),
      confirm: Confirm::default(),
      message: None,
      keys: IdempotencyKeys::default(),
      scroll: ScrollHandle::new(),
      _poll: poll,
    };
    this.reload(cx);
    this
  }

  fn running(&self) -> bool {
    self.status.ready().is_some_and(|s| s.running)
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    if !matches!(self.status, Load::Ready(_)) {
      self.status = Load::Loading;
    }
    let (repo, workspace) = (self.target.repo.clone(), self.target.id.clone());
    load::run(
      cx,
      remote::read::<AgentStatusResult>(TreqCommandRequest::AgentStatus { repo, workspace }),
      |this, result, cx| {
        this.status = Load::from_result(result);
        if this.running() {
          this.reload_logs(cx);
        }
      },
    );
  }

  fn reload_logs(&mut self, cx: &mut Context<Self>) {
    let (repo, workspace) = (self.target.repo.clone(), self.target.id.clone());
    load::run(
      cx,
      remote::read::<String>(TreqCommandRequest::AgentLogs { repo, workspace }),
      |this, result, _| {
        if let Ok(logs) = result {
          let lines: Vec<&str> = logs.lines().collect();
          let start = lines.len().saturating_sub(LOG_TAIL);
          this.logs = lines[start..].join("\n");
        }
      },
    );
  }

  fn act(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
    let (repo, workspace) = (self.target.repo.clone(), self.target.id.clone());
    let prompt = self.prompt.read(cx).value().trim().to_string();
    let input = self.input.read(cx).value().to_string();
    let agent = AGENTS[self.agent].to_string();
    // Stop is destructive and asks for a second tap; start and input run at once.
    if action == Action::Stop {
      if !self.confirm.tap(action) {
        return;
      }
    } else {
      if self.confirm.running.is_some() {
        return;
      }
      self.confirm.running = Some(action);
    }
    let (prefix, inputs) = match action {
      Action::Start => (
        "agent-start",
        vec![
          repo.clone(),
          workspace.clone(),
          agent.clone(),
          prompt.clone(),
        ],
      ),
      Action::Send => (
        "agent-input",
        vec![repo.clone(), workspace.clone(), input.clone()],
      ),
      Action::Stop => ("agent-stop", vec![]),
    };
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let key = self.keys.key_for(prefix, &refs);
    let request = match action {
      Action::Start => TreqCommandRequest::AgentStart {
        repo,
        workspace,
        agent,
        prompt,
        idempotency_key: key,
      },
      Action::Send => TreqCommandRequest::AgentInput {
        repo,
        workspace,
        input,
        idempotency_key: key,
      },
      Action::Stop => TreqCommandRequest::AgentStop { repo, workspace },
    };
    let success = match action {
      Action::Start => "Agent started.",
      Action::Send => "Input sent.",
      Action::Stop => "Agent stopped.",
    };
    load::run_in(
      window,
      cx,
      remote::mutate(request),
      move |this, outcome, window, cx| {
        this.confirm.done();
        let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
        this.keys.settle(prefix, &refs, &outcome);
        this.message = Some(mutation_message(&outcome, success));
        if matches!(outcome, Ok(Mutation::Applied(_))) && action == Action::Send {
          this.input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        this.reload(cx);
      },
    );
  }

  fn button(&self, action: Action, label: &str, cx: &mut Context<Self>) -> Button {
    let id = match action {
      Action::Start => "start",
      Action::Send => "send",
      Action::Stop => "stop",
    };
    let label = if self.confirm.is_running(action) {
      "Working…".to_string()
    } else if self.confirm.is_armed(action) {
      "Confirm stop".to_string()
    } else {
      label.to_string()
    };
    Button::new(id)
      .large()
      .label(label)
      .loading(self.confirm.is_running(action))
      .on_click(cx.listener(move |this, _, window, cx| {
        this.act(action, window, cx);
        cx.notify();
      }))
  }
}

impl Render for AgentScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let controls = match &self.status {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::centered_message("Checking the agent…", cx))
      }
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(error.clone(), cx)),
      Load::Ready(status) if status.running => widgets::card(cx)
        .p_4()
        .gap_3()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(format!(
          "Running {} (pid {})",
          status.agent.clone().unwrap_or_default(),
          status.pid.map(|p| p.to_string()).unwrap_or_default()
        )))
        .children(
          status
            .started_at
            .clone()
            .map(|t| widgets::muted(format!("Started {t}"), cx)),
        )
        .child(Textarea::new(&self.input))
        .child(
          h_flex()
            .gap_2()
            .child(self.button(Action::Send, "Send input", cx).primary())
            .child(self.button(Action::Stop, "Stop agent", cx).danger()),
        ),
      Load::Ready(_) => {
        let selected = self.agent;
        widgets::card(cx)
          .p_4()
          .gap_3()
          .child(
            ButtonGroup::new("agents")
              .outline()
              .children(AGENTS.iter().enumerate().map(|(i, agent)| {
                Button::new(("agent", i))
                  .label(*agent)
                  .selected(i == selected)
              }))
              .on_click(cx.listener(|this, selected: &Vec<usize>, _, cx| {
                if let Some(&i) = selected.first() {
                  this.agent = i;
                  cx.notify();
                }
              })),
          )
          .child(Textarea::new(&self.prompt))
          .child(h_flex().child(self.button(Action::Start, "Start agent", cx).primary()))
      }
    };
    let logs = (!self.logs.is_empty()).then(|| {
      widgets::section("Logs", cx).child(
        widgets::card(cx).p_2().child(
          div()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(cx.theme().mono_font_size)
            .child(self.logs.clone()),
        ),
      )
    });
    div()
      .id("agent")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(controls)
          .children(self.message.clone().map(|(ok, text)| {
            if ok {
              widgets::muted(text, cx)
            } else {
              widgets::error_text(text, cx)
            }
          }))
          .children(logs)
          .child(v_flex().h(px(8.))),
      )
  }
}
