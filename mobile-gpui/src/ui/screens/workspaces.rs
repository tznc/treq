//! Workspaces in a repository, and creating one.

use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    v_flex, Sizable,
  },
  *,
};
use treq_lib::{core::remote::TreqCommandRequest, local_db::Workspace};

use super::{mutation_message, on_epoch, workspace, Confirm};
use crate::{
  remote::{self, IdempotencyKeys},
  ui::{load, nav, widgets, Load, Location},
};

pub fn open(repo: String, cx: &mut App) {
  let title = repo.rsplit('/').next().unwrap_or(&repo).to_string();
  let location = Location {
    repo: Some(repo.clone()),
    workspace: None,
  };
  nav::push(title, location, cx, move |window, cx| {
    cx.new(|cx| WorkspacesScreen::new(repo, window, cx))
  });
}

#[derive(Clone, Copy, PartialEq)]
struct Create;

pub struct WorkspacesScreen {
  repo: String,
  list: Load<Vec<Workspace>>,
  branch: Entity<InputState>,
  source: Entity<InputState>,
  confirm: Confirm<Create>,
  message: Option<(bool, String)>,
  keys: IdempotencyKeys,
  scroll: ScrollHandle,
}

impl WorkspacesScreen {
  fn new(repo: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
    on_epoch(cx, |this, cx| this.reload(cx));
    let mut this = Self {
      repo,
      list: Load::Idle,
      branch: cx.new(|cx| InputState::new(window, cx).placeholder("Branch name")),
      source: cx.new(|cx| InputState::new(window, cx).placeholder("Source branch (optional)")),
      confirm: Confirm::default(),
      message: None,
      keys: IdempotencyKeys::default(),
      scroll: ScrollHandle::new(),
    };
    this.reload(cx);
    this
  }

  fn reload(&mut self, cx: &mut Context<Self>) {
    self.list = Load::Loading;
    let repo = self.repo.clone();
    load::run(
      cx,
      async move { remote::read::<Vec<Workspace>>(TreqCommandRequest::ListWorkspaces { repo }).await },
      |this, result, _| this.list = Load::from_result(result),
    );
  }

  fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let branch = self.branch.read(cx).value().trim().to_string();
    let source = self.source.read(cx).value().trim().to_string();
    if branch.is_empty() {
      self.message = Some((false, "Enter a branch name.".into()));
      return;
    }
    if !self.confirm.tap(Create) {
      return;
    }
    let inputs = [self.repo.as_str(), branch.as_str(), source.as_str()];
    let key = self.keys.key_for("create-workspace", &inputs);
    let inputs: Vec<String> = inputs.iter().map(|s| s.to_string()).collect();
    let request = TreqCommandRequest::CreateWorkspace {
      repo: self.repo.clone(),
      branch_name: branch,
      source_branch: (!source.is_empty()).then_some(source),
      metadata: None,
      idempotency_key: key,
    };
    load::run_in(
      window,
      cx,
      remote::mutate(request),
      move |this, outcome, window, cx| {
        this.confirm.done();
        let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
        this.keys.settle("create-workspace", &refs, &outcome);
        this.message = Some(mutation_message(&outcome, "Workspace created."));
        if matches!(outcome, Ok(remote::Mutation::Applied(_))) {
          this.branch.update(cx, |s, cx| s.set_value("", window, cx));
          this.source.update(cx, |s, cx| s.set_value("", window, cx));
          this.reload(cx);
        }
      },
    );
  }
}

impl Render for WorkspacesScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let list = match &self.list {
      Load::Idle | Load::Loading => {
        widgets::card(cx).child(widgets::centered_message("Loading workspaces…", cx))
      }
      Load::Failed(error) => widgets::card(cx)
        .p_4()
        .child(widgets::error_text(error.clone(), cx)),
      Load::Ready(list) if list.is_empty() => {
        widgets::card(cx).child(widgets::centered_message("No workspaces found.", cx))
      }
      Load::Ready(list) => widgets::card(cx).children(list.iter().enumerate().map(|(i, ws)| {
        let title = if ws.title.is_empty() {
          ws.workspace_name.clone()
        } else {
          ws.title.clone()
        };
        let (repo, id, name) = (self.repo.clone(), ws.id.to_string(), title.clone());
        widgets::row(("ws", i), title, Some(ws.branch_name.clone().into()), cx)
          .on_click(move |_, _, cx| workspace::open(repo.clone(), id.clone(), name.clone(), cx))
      })),
    };
    let create_label = if self.confirm.is_running(Create) {
      "Working…"
    } else if self.confirm.is_armed(Create) {
      "Confirm create"
    } else {
      "Create workspace"
    };
    div()
      .id("workspaces")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(
            widgets::section("Workspaces", cx).child(list).child(
              Button::new("refresh")
                .ghost()
                .label("Refresh")
                .loading(self.list.is_loading())
                .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            ),
          )
          .child(
            widgets::section("New workspace", cx).child(
              widgets::card(cx)
                .p_4()
                .gap_3()
                .child(Input::new(&self.branch).large())
                .child(Input::new(&self.source).large())
                .child(
                  v_flex().child(
                    Button::new("create")
                      .primary()
                      .large()
                      .label(create_label)
                      .loading(self.confirm.is_running(Create))
                      .on_click(cx.listener(|this, _, window, cx| {
                        this.create(window, cx);
                        cx.notify();
                      })),
                  ),
                )
                .children(self.message.clone().map(|(ok, text)| {
                  if ok {
                    widgets::muted(text, cx)
                  } else {
                    widgets::error_text(text, cx)
                  }
                })),
            ),
          ),
      )
  }
}
