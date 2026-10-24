//! New workspaces, several at once: one title or branch per line, an
//! indented line stacked on the line above, and the host's plan for each
//! line shown before anything is made.

use std::time::Duration;

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Indent, InputEvent, Outdent, Textarea, TextareaState},
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::core::{
  new_workspaces::{plan_unchecked, NewWorkspaceRow, NewWorkspaceStep},
  remote::TreqCommandRequest,
};

use super::{mutation_message, workspaces::OnDone};
use crate::{
  errors,
  remote::{self, IdempotencyKeys, Mutation},
  ui::{load, sheet, widgets, Load},
};

/// How long typing pauses before the host is asked for a plan.
const PLAN_DELAY: Duration = Duration::from_millis(400);

/// Indent per stacking level in the plan.
const INDENT: f32 = 16.;

/// The plan's height before it scrolls, so a long plan leaves the button in
/// view.
const PLAN_MAX_H: f32 = 240.;

/// Opens the sheet. New workspaces stack on `stack_on`, or the default
/// branch; `done` runs once any were made.
pub fn open(repo: String, stack_on: Option<String>, done: OnDone, cx: &mut App) {
  let title = match &stack_on {
    Some(branch) => format!("New workspaces on {branch}"),
    None => "New workspaces".into(),
  };
  sheet::open(Some(title.into()), cx, move |window, cx| {
    cx.new(|cx| NewWorkspacesSheet::new(repo, stack_on, done, window, cx))
  });
}

/// The button's label for a plan: what pressing it does.
fn action_label(rows: &[NewWorkspaceRow]) -> String {
  let count = |step| rows.iter().filter(|r| r.step == Some(step)).count();
  let (create, open) = (
    count(NewWorkspaceStep::Create),
    count(NewWorkspaceStep::Open) + count(NewWorkspaceStep::AlreadyOpen),
  );
  let workspaces = |n: usize| {
    if n == 1 {
      "1 workspace".to_string()
    } else {
      format!("{n} workspaces")
    }
  };
  match (create, open) {
    (0, 0) => "Create workspaces".into(),
    (c, 0) => format!("Create {}", workspaces(c)),
    (0, o) => format!("Open {}", workspaces(o)),
    (c, o) => format!("Create {c}, open {o}"),
  }
}

/// A row's plan in words.
fn step_text(row: &NewWorkspaceRow) -> String {
  match (row.step, &row.error) {
    (_, Some(error)) => error.clone(),
    (Some(NewWorkspaceStep::Create), _) => match &row.base {
      Some(base) if row.depth > 0 => format!("New branch on {base}"),
      _ => "New branch".into(),
    },
    (Some(NewWorkspaceStep::Open), _) => "Opens the existing branch".into(),
    (Some(NewWorkspaceStep::AlreadyOpen), _) => "Already open".into(),
    (None, None) => String::new(),
  }
}

struct NewWorkspacesSheet {
  repo: String,
  stack_on: Option<String>,
  lines: Entity<TextareaState>,
  plan: Load<Vec<NewWorkspaceRow>>,
  /// The host's treq predates planning: lines are planned here and made
  /// one at a time.
  legacy: bool,
  /// What a run did, row by row.
  results: Option<Vec<NewWorkspaceRow>>,
  running: bool,
  error: Option<String>,
  keys: IdempotencyKeys,
  done: OnDone,
  _plan: Task<()>,
}

impl NewWorkspacesSheet {
  fn new(
    repo: String,
    stack_on: Option<String>,
    done: OnDone,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let lines = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(5)
        .placeholder("Fix login redirect\nfeat/billing-api\n  Billing page")
    });
    cx.subscribe(&lines, |this, _, event: &InputEvent, cx| {
      if let InputEvent::Change = event {
        this.text_changed(cx);
      }
    })
    .detach();
    lines.update(cx, |state, cx| state.focus(window, cx));
    Self {
      repo,
      stack_on,
      lines,
      plan: Load::Idle,
      legacy: false,
      results: None,
      running: false,
      error: None,
      keys: IdempotencyKeys::default(),
      done,
      _plan: Task::ready(()),
    }
  }

  fn text(&self, cx: &App) -> String {
    self.lines.read(cx).value().to_string()
  }

  /// Replans once typing pauses.
  fn text_changed(&mut self, cx: &mut Context<Self>) {
    self.results = None;
    self.error = None;
    if self.text(cx).trim().is_empty() {
      self.plan = Load::Idle;
      self._plan = Task::ready(());
      return cx.notify();
    }
    if self.legacy {
      self.plan = Load::Ready(self.unchecked(cx));
      return cx.notify();
    }
    self._plan = cx.spawn(async move |this, cx| {
      cx.background_executor().timer(PLAN_DELAY).await;
      this.update(cx, |this, cx| this.replan(cx)).ok();
    });
  }

  fn unchecked(&self, cx: &App) -> Vec<NewWorkspaceRow> {
    plan_unchecked(&self.text(cx), self.stack_on.as_deref().unwrap_or_default())
  }

  fn replan(&mut self, cx: &mut Context<Self>) {
    let text = self.text(cx);
    if self.plan.ready().is_none() {
      self.plan = Load::Loading;
    }
    let request = TreqCommandRequest::PlanNewWorkspaces {
      repo: self.repo.clone(),
      text: text.clone(),
      stack_on: self.stack_on.clone(),
    };
    load::run(
      cx,
      remote::read::<Vec<NewWorkspaceRow>>(request),
      move |this, result, cx| {
        // A newer edit is on its way; its plan replaces this one.
        if this.text(cx) != text {
          return;
        }
        this.plan = match result {
          Err(error) if error.contains("unknown workspace action") => {
            this.legacy = true;
            Load::Ready(this.unchecked(cx))
          }
          other => Load::from_result(other.map_err(|e| errors::describe(&e).message)),
        };
        cx.notify();
      },
    );
  }

  fn indent(&mut self, outdent: bool, window: &mut Window, cx: &mut Context<Self>) {
    self.lines.update(cx, |state, cx| state.focus(window, cx));
    if outdent {
      window.dispatch_action(Box::new(Outdent), cx);
    } else {
      window.dispatch_action(Box::new(Indent), cx);
    }
  }

  fn run(&mut self, cx: &mut Context<Self>) {
    let Some(plan) = self.plan.ready().cloned() else {
      return;
    };
    if self.running || plan.iter().all(|r| r.step.is_none()) {
      return;
    }
    self.running = true;
    self.error = None;
    let text = self.text(cx);
    if self.legacy {
      let repo = self.repo.clone();
      load::run(cx, create_one_by_one(repo, plan), |this, rows, cx| {
        this.finish(Ok(rows), cx)
      });
    } else {
      let inputs = [self.repo.as_str(), text.as_str()];
      let key = self.keys.key_for("new-workspaces", &inputs);
      let inputs: Vec<String> = inputs.iter().map(|s| s.to_string()).collect();
      let request = TreqCommandRequest::NewWorkspaces {
        repo: self.repo.clone(),
        text,
        stack_on: self.stack_on.clone(),
        idempotency_key: key,
      };
      load::run(cx, remote::mutate(request), move |this, outcome, cx| {
        let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
        this.keys.settle("new-workspaces", &refs, &outcome);
        let rows = match outcome {
          Ok(Mutation::Applied(value)) => {
            serde_json::from_value::<Vec<NewWorkspaceRow>>(value).map_err(|e| e.to_string())
          }
          other => Err(errors::describe(&mutation_message(&other, "").1).message),
        };
        this.finish(rows, cx);
      });
    }
    cx.notify();
  }

  /// Closes on full success; otherwise shows what each line did.
  fn finish(&mut self, rows: Result<Vec<NewWorkspaceRow>, String>, cx: &mut Context<Self>) {
    self.running = false;
    match rows {
      Ok(rows) => {
        let made = rows.iter().any(|r| r.workspace_id.is_some());
        let failed = rows.iter().any(|r| r.step.is_some() && r.error.is_some());
        if made {
          (self.done)(cx);
        }
        if failed {
          self.results = Some(rows);
        } else {
          sheet::close(cx);
        }
      }
      Err(error) => self.error = Some(error),
    }
    cx.notify();
  }

  fn plan_rows(&self, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let rows = match (&self.results, &self.plan) {
      (Some(results), _) => results,
      (None, Load::Ready(rows)) => rows,
      (None, Load::Loading) => return widgets::muted("Checking…", cx).px_4().into_any_element(),
      (None, Load::Failed(error)) => {
        return widgets::error_text(error.clone(), cx)
          .px_4()
          .into_any_element()
      }
      (None, Load::Idle) => {
        return widgets::muted(
          "One per line: a title or a branch name. Indent a line to stack it on the line above.",
          cx,
        )
        .px_4()
        .into_any_element()
      }
    };
    let ran = self.results.is_some();
    v_flex()
      .children(rows.iter().enumerate().map(|(i, row)| {
        let failed = row.error.is_some();
        let icon = if failed {
          IconName::TriangleAlert
        } else if ran {
          IconName::Check
        } else if row.step == Some(NewWorkspaceStep::Create) {
          IconName::Plus
        } else {
          IconName::FolderOpen
        };
        h_flex()
          .id(("plan", i))
          .px_4()
          .py_1p5()
          .gap_2()
          .items_start()
          .pl(px(16. + INDENT * row.depth as f32))
          .child(
            div()
              .pt_0p5()
              .text_color(if failed {
                theme.danger
              } else {
                theme.muted_foreground
              })
              .child(icon),
          )
          .child(
            v_flex()
              .flex_1()
              .min_w_0()
              .child(
                div()
                  .truncate()
                  .child(row.title.clone().unwrap_or_else(|| row.line.clone())),
              )
              .children(
                row
                  .branch
                  .clone()
                  .filter(|_| row.title.is_some())
                  .map(|branch| widgets::mono(branch, cx).text_xs().truncate()),
              )
              .child(
                div()
                  .text_xs()
                  .text_color(if failed {
                    theme.danger
                  } else {
                    theme.muted_foreground
                  })
                  .child(step_text(row)),
              ),
          )
      }))
      .into_any_element()
  }
}

impl Render for NewWorkspacesSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let rows = self.plan.ready().cloned().unwrap_or_default();
    let runnable = rows.iter().any(|r| r.step.is_some()) && self.results.is_none();
    v_flex()
      .gap_3()
      .child(
        v_flex()
          .px_4()
          .gap_2()
          .child(Textarea::new(&self.lines))
          .child(
            h_flex()
              .gap_2()
              .child(
                Button::new("outdent")
                  .outline()
                  .small()
                  .label("Outdent")
                  .on_click(cx.listener(|this, _, window, cx| this.indent(true, window, cx))),
              )
              .child(
                Button::new("indent")
                  .outline()
                  .small()
                  .label("Indent")
                  .on_click(cx.listener(|this, _, window, cx| this.indent(false, window, cx))),
              )
              .when(self.legacy, |bar| {
                bar.child(widgets::muted(
                  "This host's treq can't check lines first.",
                  cx,
                ))
              }),
          ),
      )
      .child(
        div()
          .id("plan-rows")
          .max_h(px(PLAN_MAX_H))
          .overflow_y_scroll()
          .child(self.plan_rows(cx)),
      )
      .children(
        self
          .error
          .clone()
          .map(|e| widgets::error_text(e, cx).px_4()),
      )
      .child(
        div().px_4().child(
          Button::new("create-workspaces")
            .primary()
            .large()
            .w_full()
            .h(px(widgets::CTA_H - 8.))
            .label(if self.results.is_some() {
              "Done".to_string()
            } else {
              action_label(&rows)
            })
            .loading(self.running)
            .disabled(!(runnable || self.results.is_some()) || self.running)
            .on_click(cx.listener(|this, _, _, cx| {
              if this.results.is_some() {
                sheet::close(cx);
              } else {
                this.run(cx);
              }
            })),
        ),
      )
  }
}

/// Makes each planned line with `CreateWorkspace`, in order, for hosts that
/// can't run a plan. A line stacked on a failed one fails too.
async fn create_one_by_one(repo: String, mut rows: Vec<NewWorkspaceRow>) -> Vec<NewWorkspaceRow> {
  let mut failed: Vec<String> = Vec::new();
  for row in &mut rows {
    let (Some(branch), Some(base)) = (row.branch.clone(), row.base.clone()) else {
      continue;
    };
    if row.step.is_none() {
      continue;
    }
    if failed.contains(&base) {
      row.error = Some(format!("Not made: {base} failed"));
      failed.push(branch);
      continue;
    }
    let metadata = row
      .title
      .as_ref()
      .map(|title| serde_json::json!({ "title": title }).to_string());
    let outcome = remote::mutate(TreqCommandRequest::CreateWorkspace {
      repo: repo.clone(),
      branch_name: branch.clone(),
      source_branch: Some(base).filter(|b| !b.is_empty()),
      metadata,
      idempotency_key: uuid::Uuid::new_v4().to_string(),
    })
    .await;
    match outcome {
      Ok(Mutation::Applied(value)) => {
        row.workspace_id = value.get("id").and_then(|id| id.as_i64());
      }
      other => {
        row.error = Some(errors::describe(&mutation_message(&other, "").1).message);
        failed.push(branch);
      }
    }
  }
  rows
}

#[cfg(test)]
mod tests {
  use super::*;

  fn row(step: Option<NewWorkspaceStep>) -> NewWorkspaceRow {
    NewWorkspaceRow {
      line: "x".into(),
      depth: 0,
      branch: Some("x".into()),
      title: None,
      base: Some("main".into()),
      step,
      error: step.is_none().then(|| "bad".into()),
      workspace_id: None,
    }
  }

  #[core::prelude::v1::test]
  fn the_button_says_what_it_does() {
    use NewWorkspaceStep::*;
    assert_eq!(action_label(&[row(Some(Create))]), "Create 1 workspace");
    assert_eq!(
      action_label(&[row(Some(Create)), row(Some(Create)), row(None)]),
      "Create 2 workspaces"
    );
    assert_eq!(action_label(&[row(Some(AlreadyOpen))]), "Open 1 workspace");
    assert_eq!(
      action_label(&[row(Some(Create)), row(Some(Open))]),
      "Create 1, open 1"
    );
    assert_eq!(action_label(&[]), "Create workspaces");
  }
}
