//! One workspace: status, changed files, mutations, and links to the
//! commit, conflict, agent and terminal views.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    tag::Tag,
    v_flex, ActiveTheme, IconName,
  },
  *,
};
use serde::Deserialize;
use treq_lib::{
  core::remote::TreqCommandRequest, core::workspaces::WorkspaceStatus, jj::JjFileChange,
};

use super::{agent, commits, conflicts, diff, mutation_message, on_epoch, terminal, Confirm};
use crate::{
  remote::{self, IdempotencyKeys, Mutation},
  store::SnapshotWorkspace,
  ui::{load, nav, widgets, Load, Location},
};

/// How often the change marker is polled to notice remote edits.
const MARKER_POLL: Duration = Duration::from_secs(15);

pub fn open(repo: String, workspace_id: String, name: String, cx: &mut App) {
  let location = Location {
    repo: Some(repo.clone()),
    workspace: Some(SnapshotWorkspace {
      id: workspace_id.clone(),
      name: name.clone(),
    }),
  };
  nav::push(name.clone(), location, cx, move |window, cx| {
    cx.new(|cx| WorkspaceScreen::new(repo, workspace_id, name, window, cx))
  });
}

/// The repository and workspace a view works in. `id` is the numeric
/// workspace id the remote CLI expects.
#[derive(Clone)]
pub struct Target {
  pub repo: String,
  pub id: String,
  pub name: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
  Rebase,
  Commit,
  Push,
}

#[derive(Deserialize)]
struct ChangeMarker {
  operation_id: Option<String>,
}

pub struct WorkspaceScreen {
  target: Target,
  status: Load<WorkspaceStatus>,
  changes: Load<Vec<JjFileChange>>,
  marker: Option<String>,
  rebase_onto: Entity<InputState>,
  commit_message: Entity<InputState>,
  confirm: Confirm<Action>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  scroll: ScrollHandle,
  _poll: Task<()>,
}

impl WorkspaceScreen {
  fn new(
    repo: String,
    id: String,
    name: String,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let poll = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(MARKER_POLL).await;
      if this.update(cx, |this, cx| this.poll_marker(cx)).is_err() {
        break;
      }
    });
    let mut this = Self {
      target: Target { repo, id, name },
      status: Load::Idle,
      changes: Load::Idle,
      marker: None,
      rebase_onto: cx.new(|cx| InputState::new(window, cx).placeholder("Rebase onto branch")),
      commit_message: cx.new(|cx| InputState::new(window, cx).placeholder("Commit message")),
      confirm: Confirm::default(),
      message: None,
      keys: IdempotencyKeys::default(),
      scroll: ScrollHandle::new(),
      _poll: poll,
    };
    this.reload(cx);
    this
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    self.status = Load::Loading;
    self.changes = Load::Loading;
    let Target { repo, id, .. } = self.target.clone();
    let (r, w) = (repo.clone(), id.clone());
    load::run(
      cx,
      async move {
        remote::read::<WorkspaceStatus>(TreqCommandRequest::InspectWorkspace {
          repo: r,
          workspace: w,
        })
        .await
      },
      |this, result, _| this.status = Load::from_result(result),
    );
    load::run(
      cx,
      async move {
        remote::read::<Vec<JjFileChange>>(TreqCommandRequest::ListChanges {
          repo,
          workspace: Some(id),
        })
        .await
      },
      |this, result, _| this.changes = Load::from_result(result),
    );
    self.poll_marker(cx);
  }

  /// Refetches when the workspace changed on the host since the last poll.
  fn poll_marker(&mut self, cx: &mut Context<Self>) {
    let Target { repo, id, .. } = self.target.clone();
    load::run(
      cx,
      async move {
        remote::read::<ChangeMarker>(TreqCommandRequest::WorkspaceChangeMarker {
          repo,
          workspace: Some(id),
        })
        .await
      },
      |this, result, cx| {
        if let Ok(marker) = result {
          let changed = this.marker.is_some() && this.marker != marker.operation_id;
          this.marker = marker.operation_id;
          if changed {
            this.reload(cx);
          }
        }
      },
    );
  }

  fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
    let Target { repo, id, .. } = self.target.clone();
    let target = self.rebase_onto.read(cx).value().trim().to_string();
    let message = self.commit_message.read(cx).value().trim().to_string();
    match action {
      Action::Rebase if target.is_empty() => {
        self.message = Some((false, "Enter the branch to rebase onto.".into()));
        return;
      }
      Action::Commit if message.is_empty() => {
        self.message = Some((false, "Enter a commit message.".into()));
        return;
      }
      _ => {}
    }
    if !self.confirm.tap(action) {
      return;
    }
    let (prefix, inputs, success): (&str, Vec<String>, &str) = match action {
      Action::Rebase => (
        "rebase",
        vec![repo.clone(), id.clone(), target.clone()],
        "Rebased.",
      ),
      Action::Commit => (
        "commit",
        vec![repo.clone(), id.clone(), message.clone()],
        "Committed.",
      ),
      Action::Push => ("push", vec![repo.clone(), id.clone()], "Pushed."),
    };
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let key = self.keys.key_for(prefix, &refs);
    let request = match action {
      Action::Rebase => TreqCommandRequest::RebaseWorkspace {
        repo,
        workspace: id,
        target_branch: target,
        idempotency_key: key,
      },
      Action::Commit => TreqCommandRequest::CreateCommit {
        repo,
        workspace: Some(id),
        message,
        base_change_id: None,
        idempotency_key: key,
      },
      Action::Push => TreqCommandRequest::GitPush {
        repo,
        workspace: Some(id),
        idempotency_key: key,
      },
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
        if matches!(outcome, Ok(Mutation::Applied(_))) {
          if action == Action::Commit {
            this
              .commit_message
              .update(cx, |s, cx| s.set_value("", window, cx));
          }
          this.reload(cx);
        }
      },
    );
  }

  fn action_button(
    &self,
    action: Action,
    label: &str,
    confirm: &str,
    cx: &mut Context<Self>,
  ) -> Button {
    let label = if self.confirm.is_running(action) {
      "Working…".to_string()
    } else if self.confirm.is_armed(action) {
      confirm.to_string()
    } else {
      label.to_string()
    };
    let id = match action {
      Action::Rebase => "rebase",
      Action::Commit => "commit",
      Action::Push => "push",
    };
    Button::new(id)
      .when(!self.confirm.is_armed(action), |b| b.outline())
      .when(self.confirm.is_armed(action), |b| b.warning())
      .label(label)
      .loading(self.confirm.is_running(action))
      .on_click(cx.listener(move |this, _, window, cx| {
        this.run(action, window, cx);
        cx.notify();
      }))
  }

  fn status(&self, cx: &App) -> impl IntoElement {
    let body = match &self.status {
      Load::Idle | Load::Loading => widgets::muted("Loading status…", cx),
      Load::Failed(error) => widgets::error_text(error.clone(), cx),
      Load::Ready(status) => {
        let partial = &status.partial;
        h_flex()
          .flex_wrap()
          .gap_2()
          .child(if partial.has_changes {
            Tag::warning().child("Uncommitted changes")
          } else {
            Tag::success().child("Clean")
          })
          .child(if partial.has_conflicts {
            Tag::danger().child(format!("{} conflicted", status.conflicted_files.len()))
          } else {
            Tag::secondary().child("No conflicts")
          })
          .child(Tag::secondary().child(format!("{} ahead", partial.commits_ahead)))
          .child(Tag::secondary().child(partial.current.branch_name.clone()))
      }
    };
    widgets::card(cx)
      .p_4()
      .gap_2()
      .child(body)
      .children(self.marker.as_ref().map(|op| {
        widgets::muted(
          format!("op {}", op.chars().take(12).collect::<String>()),
          cx,
        )
      }))
  }

  fn changes(&self, cx: &App) -> impl IntoElement {
    match &self.changes {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::centered_message("Loading changes…", cx))
      }
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(error.clone(), cx)),
      Load::Ready(changes) if changes.is_empty() => {
        widgets::card(cx).child(widgets::centered_message("No changed files.", cx))
      }
      Load::Ready(changes) => {
        widgets::card(cx).children(changes.iter().enumerate().map(|(i, change)| {
          let target = self.target.clone();
          let path = change.path.clone();
          let detail = match &change.previous_path {
            Some(previous) => format!("{} · from {previous}", change.status),
            None => format!("{} · {} lines", change.status, change.changed_line_count),
          };
          widgets::row(("change", i), change.path.clone(), Some(detail.into()), cx)
            .on_click(move |_, _, cx| diff::open(target.clone(), path.clone(), cx))
        }))
      }
    }
  }
}

impl Render for WorkspaceScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let target = self.target.clone();
    let nav_button = |id: &'static str, icon: IconName, label: &'static str| {
      Button::new(id).outline().icon(icon).label(label).flex_1()
    };
    let links = v_flex()
      .gap_2()
      .child(
        h_flex()
          .gap_2()
          .child(
            nav_button("commits", IconName::GalleryVerticalEnd, "Commits").on_click({
              let t = target.clone();
              move |_, _, cx| commits::open(t.clone(), cx)
            }),
          )
          .child(
            nav_button("conflicts", IconName::TriangleAlert, "Conflicts").on_click({
              let t = target.clone();
              move |_, _, cx| conflicts::open(t.clone(), cx)
            }),
          ),
      )
      .child(
        h_flex()
          .gap_2()
          .child(nav_button("agent", IconName::Bot, "Agent").on_click({
            let t = target.clone();
            move |_, _, cx| agent::open(t.clone(), cx)
          }))
          .child(
            nav_button("terminal", IconName::SquareTerminal, "Terminal").on_click({
              let t = target.clone();
              move |_, _, cx| terminal::open(t.clone(), cx)
            }),
          ),
      );
    let rebase = self.action_button(Action::Rebase, "Rebase", "Confirm rebase", cx);
    let commit = self.action_button(Action::Commit, "Commit", "Confirm commit", cx);
    let push = self.action_button(Action::Push, "Push bookmark", "Confirm push", cx);
    let mutations = widgets::card(cx)
      .p_4()
      .gap_3()
      .child(
        h_flex()
          .gap_2()
          .child(div().flex_1().child(Input::new(&self.rebase_onto)))
          .child(rebase),
      )
      .child(
        h_flex()
          .gap_2()
          .child(div().flex_1().child(Input::new(&self.commit_message)))
          .child(commit),
      )
      .child(h_flex().child(push))
      .children(self.message.clone().map(|(ok, text)| {
        if ok {
          widgets::muted(text, cx)
        } else {
          widgets::error_text(text, cx)
        }
      }));

    div()
      .id("workspace")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(
            widgets::section("Status", cx).child(self.status(cx)).child(
              Button::new("refresh")
                .ghost()
                .icon(IconName::RefreshCw)
                .label("Refresh")
                .loading(self.status.is_loading())
                .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            ),
          )
          .child(links)
          .child(widgets::section("Changed files", cx).child(self.changes(cx)))
          .child(widgets::section("Changes", cx).child(mutations))
          .child(div().h(px(8.)).bg(cx.theme().background)),
      )
  }
}
