//! Remote terminal in a workspace: persistent sessions on the host that can
//! be detached and reattached (tmux/screen through `pty-remote`).

use futures::channel::mpsc;
use gpui_kit::component::Disableable as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::{
  core::{
    pty_remote_supervisor::PtySessionInfo,
    remote::TreqCommandRequest,
    remote_pty::{PtyLaunchSpec, RemotePtyBinding},
  },
  local_db::Workspace,
};

use super::{on_epoch, workspace::Target};
use crate::{
  backend, remote,
  ui::{
    load, nav,
    terminal_view::{PtyEvent, TerminalView},
    widgets, Load, Location,
  },
};

/// Size used to start a session, until the view measures itself.
const INITIAL_SIZE: (u16, u16) = (80, 24);

/// Touch-toolbar keys and the bytes they send.
const KEYS: [(&str, &[u8]); 9] = [
  ("Esc", b"\x1b"),
  ("Tab", b"\t"),
  ("^C", b"\x03"),
  ("^D", b"\x04"),
  ("^Z", b"\x1a"),
  ("←", b"\x1b[D"),
  ("↑", b"\x1b[A"),
  ("↓", b"\x1b[B"),
  ("→", b"\x1b[C"),
];

pub fn open(target: Target, cx: &mut App) {
  let location = Location {
    repo: Some(target.repo.clone()),
    workspace: Some(crate::store::SnapshotWorkspace {
      id: target.id.clone(),
      name: target.name.clone(),
    }),
  };
  nav::push("Terminal", location, cx, move |_, cx| {
    cx.new(|cx| TerminalScreen::new(target, cx))
  });
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

/// A label for a new shell session, unique per launch.
pub fn new_session_label(now_ms: i64) -> String {
  let mut n = now_ms.max(0) as u64;
  let mut digits = Vec::new();
  loop {
    digits.push(std::char::from_digit((n % 36) as u32, 36).unwrap());
    n /= 36;
    if n == 0 {
      break;
    }
  }
  format!("shell-{}", digits.into_iter().rev().collect::<String>())
}

struct Attached {
  label: String,
  view: Entity<TerminalView>,
}

pub struct TerminalScreen {
  target: Target,
  workspace: Load<Workspace>,
  sessions: Load<Vec<PtySessionInfo>>,
  attached: Option<Attached>,
  attaching: Option<String>,
  notice: Option<String>,
  error: Option<String>,
}

impl TerminalScreen {
  fn new(target: Target, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| {
      if this.attached.is_none() {
        this.load_sessions(cx)
      }
    });
    let mut this = Self {
      target,
      workspace: Load::Loading,
      sessions: Load::Idle,
      attached: None,
      attaching: None,
      notice: None,
      error: None,
    };
    this.load_workspace(cx);
    this
  }

  fn load_workspace(&mut self, cx: &mut Context<Self>) {
    let repo = self.target.repo.clone();
    let id = self.target.id.clone();
    load::run(
      cx,
      async move {
        let list =
          remote::read::<Vec<Workspace>>(TreqCommandRequest::ListWorkspaces { repo }).await?;
        list
          .into_iter()
          .find(|w| w.id.to_string() == id)
          .ok_or_else(|| "This workspace no longer exists on the host.".to_string())
      },
      |this, result, cx| {
        this.workspace = Load::from_result(result);
        this.load_sessions(cx);
      },
    );
  }

  fn load_sessions(&mut self, cx: &mut Context<Self>) {
    let Some(workspace) = self.workspace.ready() else {
      return;
    };
    self.sessions = Load::Loading;
    let request = TreqCommandRequest::PtyList {
      repo: self.target.repo.clone(),
      workspace: Some(workspace.workspace_name.clone()),
    };
    load::run(
      cx,
      remote::read::<Vec<PtySessionInfo>>(request),
      |this, result, _| this.sessions = Load::from_result(result),
    );
  }

  fn attach(&mut self, label: String, window: &mut Window, cx: &mut Context<Self>) {
    let Some(workspace) = self.workspace.ready().cloned() else {
      return;
    };
    let endpoint = match backend::get().endpoint() {
      Ok(endpoint) => endpoint,
      Err(error) => {
        self.error = Some(error);
        return;
      }
    };
    self.attaching = Some(label.clone());
    self.error = None;
    self.notice = None;
    let repo = self.target.repo.clone();
    let remote_dir = workspace_dir(&repo, &workspace);
    let session_id = format!("gpui-{label}-{}", uuid::Uuid::new_v4());
    let (tx, rx) = mpsc::unbounded::<PtyEvent>();
    let request = TreqCommandRequest::PtyAttachCommand {
      repo: repo.clone(),
      workspace: workspace.workspace_name.clone(),
      label: label.clone(),
      remote_dir: remote_dir.clone(),
      launch: PtyLaunchSpec::Shell,
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
      let command = remote::read::<String>(request).await?;
      let output = tx.clone();
      backend::get()
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
        .map_err(|e| e.to_string())
    };
    let screen = cx.weak_entity();
    load::run_in(window, cx, attach, move |this, result, window, cx| {
      this.attaching = None;
      match result {
        Ok(()) => {
          let view = cx.new(|cx| {
            TerminalView::new(
              session_id,
              INITIAL_SIZE,
              rx,
              move |status, _, cx| {
                screen.update(cx, |this, cx| this.ended(status, cx)).ok();
              },
              window,
              cx,
            )
          });
          this.attached = Some(Attached { label, view });
        }
        Err(error) => this.error = Some(error),
      }
    });
  }

  fn ended(&mut self, status: Option<u32>, cx: &mut Context<Self>) {
    self.attached = None;
    self.notice = Some(match status {
      Some(code) => format!("The session ended (exit {code})."),
      None => "Connection to the session was lost. It may still be running on the host.".into(),
    });
    self.load_sessions(cx);
    cx.notify();
  }

  fn detach(&mut self, cx: &mut Context<Self>) {
    // Dropping the view closes the local channel only.
    self.attached = None;
    self.load_sessions(cx);
    cx.notify();
  }

  fn stop(&mut self, cx: &mut Context<Self>) {
    let (Some(attached), Some(workspace)) = (&self.attached, self.workspace.ready()) else {
      return;
    };
    let request = TreqCommandRequest::PtyStop {
      repo: self.target.repo.clone(),
      workspace: workspace.workspace_name.clone(),
      label: attached.label.clone(),
    };
    load::run(
      cx,
      remote::read::<serde_json::Value>(request),
      |this, result, cx| {
        if let Err(error) = result {
          this.error = Some(error);
        }
        this.detach(cx);
      },
    );
  }

  fn session_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let sessions = match &self.sessions {
      Load::Idle | Load::Loading => widgets::card(cx).child(widgets::centered_message(
        "Checking for running sessions…",
        cx,
      )),
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(error.clone(), cx)),
      Load::Ready(list) if list.is_empty() => widgets::card(cx).child(widgets::centered_message(
        "No sessions in this workspace.",
        cx,
      )),
      Load::Ready(list) => {
        widgets::card(cx).children(list.iter().enumerate().map(|(i, session)| {
          let label = session.label.clone();
          let detail = if session.running {
            "running"
          } else {
            "stopped"
          };
          let row = widgets::row(
            ("session", i),
            session.label.clone(),
            Some(detail.into()),
            cx,
          );
          if session.running {
            row.on_click(
              cx.listener(move |this, _, window, cx| this.attach(label.clone(), window, cx)),
            )
          } else {
            row.opacity(0.5)
          }
        }))
      }
    };
    let attaching = self.attaching.is_some();
    widgets::page()
      .children(
        self
          .workspace
          .error()
          .map(|e| widgets::error_text(e.to_string(), cx)),
      )
      .children(self.notice.clone().map(|n| widgets::muted(n, cx)))
      .child(widgets::section("Sessions", cx).child(sessions))
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("new-session")
              .primary()
              .large()
              .icon(IconName::SquareTerminal)
              .label(if attaching {
                "Starting…"
              } else {
                "New shell session"
              })
              .loading(attaching)
              .disabled(attaching || self.workspace.ready().is_none())
              .on_click(cx.listener(|this, _, window, cx| {
                let label = new_session_label(chrono::Utc::now().timestamp_millis());
                this.attach(label, window, cx);
                cx.notify();
              })),
          )
          .child(
            Button::new("refresh")
              .ghost()
              .icon(IconName::RefreshCw)
              .on_click(cx.listener(|this, _, _, cx| this.load_sessions(cx))),
          ),
      )
      .children(self.error.clone().map(|e| widgets::error_text(e, cx)))
  }

  fn terminal(&self, attached: &Attached, cx: &mut Context<Self>) -> impl IntoElement {
    let view = attached.view.clone();
    let toolbar = h_flex()
      .id("keys")
      .gap_1()
      .p_1()
      .overflow_x_scroll()
      .bg(cx.theme().secondary)
      .children(KEYS.iter().enumerate().map(|(i, (label, bytes))| {
        let view = view.clone();
        Button::new(("key", i))
          .outline()
          .small()
          .label(*label)
          .on_click(move |_, window, cx| {
            view.update(cx, |view, cx| {
              view.write(bytes.to_vec(), cx);
              view.focus(window, cx);
            });
          })
      }));
    v_flex()
      .size_full()
      .child(
        h_flex()
          .px_3()
          .py_1()
          .gap_2()
          .items_center()
          .border_b_1()
          .border_color(cx.theme().border)
          .child(widgets::mono(attached.label.clone(), cx).flex_1())
          .child(
            Button::new("detach")
              .outline()
              .small()
              .label("Detach")
              .on_click(cx.listener(|this, _, _, cx| this.detach(cx))),
          )
          .child(
            Button::new("stop")
              .danger()
              .small()
              .label("Stop")
              .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
          ),
      )
      .child(div().flex_1().min_h_0().child(attached.view.clone()))
      .child(toolbar)
  }
}

impl Render for TerminalScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    match &self.attached {
      Some(attached) => {
        let attached = Attached {
          label: attached.label.clone(),
          view: attached.view.clone(),
        };
        self.terminal(&attached, cx).into_any_element()
      }
      None => div()
        .id("terminal-sessions")
        .size_full()
        .overflow_y_scroll()
        .child(self.session_list(cx))
        .into_any_element(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{new_session_label, workspace_dir};

  #[test]
  fn labels_new_sessions_in_base36() {
    assert_eq!(new_session_label(0), "shell-0");
    assert_eq!(new_session_label(36 * 36), "shell-100");
  }

  #[test]
  fn workspace_dirs_resolve_under_the_repository() {
    let workspace: treq_lib::local_db::Workspace = serde_json::from_value(serde_json::json!({
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
