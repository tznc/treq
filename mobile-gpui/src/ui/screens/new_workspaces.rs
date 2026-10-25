//! New workspaces, several at once: one title or branch per line, an
//! indented line stacked on the line above. A Preview step shows the
//! workspaces the host will make, and nothing is made until it is confirmed
//! there. Lines whose workspace is already open are left out: they do
//! nothing.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Indent, InputEvent, Outdent, Textarea, TextareaState},
    popover::Popover,
    tag::Tag,
    v_flex, ActiveTheme, Icon, IconName, Sizable,
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

/// How long typing pauses before the host is asked how many lines it would
/// make, for the Preview button.
const PLAN_DELAY: Duration = Duration::from_millis(400);

/// Indent per stacking level in the plan.
const INDENT: f32 = 16.;

/// The plan's height before it scrolls, so a long plan leaves the buttons in
/// view.
const PLAN_MAX_H: f32 = 440.;

const PREVIEW_TITLE: &str = "Preview";

const LINES_HELP: &str =
  "One per line: a title or a branch name. Indent a line to stack it on the line above.";

/// How long the plan is on screen before Create takes a tap. Create sits
/// where Preview was, so a double tap on Preview would otherwise skip the
/// confirmation.
const CONFIRM_GUARD: Duration = Duration::from_millis(350);

/// Opens the sheet. New workspaces stack on `stack_on`, or the default
/// branch; `done` runs once any were made.
pub fn open(repo: String, stack_on: Option<String>, done: OnDone, cx: &mut App) {
  let title = match &stack_on {
    Some(branch) => format!("New workspaces on {branch}"),
    None => "New workspaces".into(),
  };
  let title = SharedString::from(title);
  // The sheet draws its own title row, which changes with the step and
  // holds the help button.
  sheet::open(None, cx, move |window, cx| {
    cx.new(|cx| NewWorkspacesSheet::new(repo, stack_on, title, done, window, cx))
  });
}

fn workspaces(n: usize) -> String {
  if n == 1 {
    "1 workspace".to_string()
  } else {
    format!("{n} workspaces")
  }
}

/// The rows that make a workspace. An already open one does nothing.
fn to_make(rows: &[NewWorkspaceRow]) -> impl Iterator<Item = &NewWorkspaceRow> {
  rows
    .iter()
    .filter(|r| r.step != Some(NewWorkspaceStep::AlreadyOpen))
}

fn make_count(rows: &[NewWorkspaceRow]) -> usize {
  to_make(rows).filter(|r| r.step.is_some()).count()
}

/// The Create button's label for a plan.
fn action_label(rows: &[NewWorkspaceRow]) -> String {
  match make_count(rows) {
    0 => "Create workspaces".into(),
    n => format!("Create {}", workspaces(n)),
  }
}

/// The Preview button's label: how many the lines will make, once known.
fn preview_label(count: Option<usize>) -> String {
  match count {
    Some(n) if n > 0 => format!("{PREVIEW_TITLE} {}", workspaces(n)),
    _ => PREVIEW_TITLE.into(),
  }
}

struct NewWorkspacesSheet {
  repo: String,
  stack_on: Option<String>,
  /// The sheet's title on the lines step.
  title: SharedString,
  lines: Entity<TextareaState>,
  /// On the Preview step: the plan is shown and the lines can't change.
  previewing: bool,
  plan: Load<Vec<NewWorkspaceRow>>,
  /// The lines `plan` is for.
  planned: Option<String>,
  /// When the plan was shown, for [`CONFIRM_GUARD`].
  shown_at: Option<Instant>,
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
    title: SharedString,
    done: OnDone,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let lines = cx.new(|cx| {
      TextareaState::new(window, cx)
        .rows(5)
        .placeholder("Fix login redirect\nfeat/billing-api\n  Billing page")
    });
    // Preview is enabled once there is a line.
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
      title,
      lines,
      previewing: false,
      plan: Load::Idle,
      planned: None,
      shown_at: None,
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

  /// Plans the lines once typing pauses, for the Preview button's count.
  fn text_changed(&mut self, cx: &mut Context<Self>) {
    if self.text(cx).trim().is_empty() {
      self.plan = Load::Idle;
      self.planned = None;
      self._plan = Task::ready(());
      return cx.notify();
    }
    if self.legacy {
      self.plan = Load::Ready(self.unchecked(cx));
      self.planned = Some(self.text(cx));
      return cx.notify();
    }
    self._plan = cx.spawn(async move |this, cx| {
      cx.background_executor().timer(PLAN_DELAY).await;
      this.update(cx, |this, cx| this.replan(cx)).ok();
    });
    cx.notify();
  }

  /// Whether `plan` is the host's answer for the lines as they are.
  fn plan_current(&self, cx: &App) -> bool {
    self.planned.as_deref() == Some(self.text(cx).as_str())
      && matches!(self.plan, Load::Ready(_) | Load::Failed(_))
  }

  /// Moves to the Preview step, asking the host for the plan unless it
  /// already has one for these lines.
  fn preview(&mut self, cx: &mut Context<Self>) {
    if self.previewing || self.text(cx).trim().is_empty() {
      return;
    }
    self.previewing = true;
    self.results = None;
    self.error = None;
    if self.plan_current(cx) {
      self.shown_at = Some(Instant::now());
    } else {
      self.shown_at = None;
      self.plan = Load::Loading;
      self._plan = Task::ready(());
      self.replan(cx);
    }
    cx.notify();
  }

  /// Back to the lines, as they were.
  fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.running {
      return;
    }
    self.previewing = false;
    self.shown_at = None;
    self.error = None;
    self.lines.update(cx, |state, cx| state.focus(window, cx));
    cx.notify();
  }

  fn unchecked(&self, cx: &App) -> Vec<NewWorkspaceRow> {
    plan_unchecked(&self.text(cx), self.stack_on.as_deref().unwrap_or_default())
  }

  fn replan(&mut self, cx: &mut Context<Self>) {
    let text = self.text(cx);
    let request = TreqCommandRequest::PlanNewWorkspaces {
      repo: self.repo.clone(),
      text: text.clone(),
      stack_on: self.stack_on.clone(),
    };
    load::run(
      cx,
      remote::read::<Vec<NewWorkspaceRow>>(request),
      move |this, result, cx| {
        // The lines changed since; a newer plan replaces this one.
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
        this.planned = Some(text);
        if this.previewing {
          this.shown_at = Some(Instant::now());
        }
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
    let settled = self
      .shown_at
      .is_some_and(|at| at.elapsed() >= CONFIRM_GUARD);
    if !self.previewing
      || !settled
      || self.running
      || self.results.is_some()
      || plan.iter().all(|r| r.step.is_none())
    {
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
      (None, Load::Idle) => return div().into_any_element(),
    };
    let ran = self.results.is_some();
    if to_make(rows).next().is_none() {
      return widgets::muted("These workspaces are all open already.", cx)
        .px_4()
        .into_any_element();
    }
    v_flex()
      .children(to_make(rows).enumerate().map(|(i, row)| {
        let error = row.error.clone().filter(|_| row.step.is_some() || !ran);
        // A stacked row hangs off the row above; a top-level one needs no
        // icon unless it failed.
        let icon = if row.error.is_some() {
          Some(IconName::TriangleAlert.into())
        } else if row.depth > 0 {
          Some(Icon::from(LucideIcon::CornerDownRight))
        } else {
          None
        };
        // "New" goes in front of the branch name, wherever the row shows it.
        let new = row.step == Some(NewWorkspaceStep::Create) && row.error.is_none();
        let branch_line = |text: String, mono: bool| {
          h_flex()
            .gap_1p5()
            .min_w_0()
            .when(new, |line| line.child(Tag::info().xsmall().child("New")))
            .child(if mono {
              widgets::mono(text, cx).text_xs().truncate()
            } else {
              div().truncate().child(text)
            })
        };
        h_flex()
          .id(("plan", i))
          .px_4()
          .py_1p5()
          .gap_2()
          .items_start()
          .pl(px(16. + INDENT * row.depth.saturating_sub(1) as f32))
          .children(icon.map(|icon| {
            div()
              .pt_0p5()
              .text_color(if row.error.is_some() {
                theme.danger
              } else {
                theme.muted_foreground
              })
              .child(icon)
          }))
          .child(
            v_flex()
              .flex_1()
              .min_w_0()
              .gap_0p5()
              .map(|rows| match (&row.title, &row.branch) {
                (Some(title), Some(branch)) => rows
                  .child(div().truncate().child(title.clone()))
                  .child(branch_line(branch.clone(), true)),
                _ => rows.child(branch_line(row.line.trim().to_string(), false)),
              })
              .children(error.map(|error| div().text_xs().text_color(theme.danger).child(error))),
          )
      }))
      .into_any_element()
  }
}

impl NewWorkspacesSheet {
  /// The lines step: what to make, before anything is checked.
  fn render_lines(&self, cx: &mut Context<Self>) -> Div {
    let empty = self.text(cx).trim().is_empty();
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
                  .icon(LucideIcon::ListIndentDecrease)
                  .tooltip("Outdent")
                  .on_click(cx.listener(|this, _, window, cx| this.indent(true, window, cx))),
              )
              .child(
                Button::new("indent")
                  .outline()
                  .icon(LucideIcon::ListIndentIncrease)
                  .tooltip("Indent")
                  .on_click(cx.listener(|this, _, window, cx| this.indent(false, window, cx))),
              ),
          )
          .when(self.legacy, |lines| {
            lines.child(widgets::muted(
              "This host's treq can't check lines first.",
              cx,
            ))
          }),
      )
      .child(
        div().px_4().child(
          Button::new("preview-workspaces")
            .primary()
            .large()
            .w_full()
            .h(px(widgets::CTA_H - 8.))
            .label(preview_label(
              self.plan.ready().map(|rows| make_count(rows)),
            ))
            .disabled(empty)
            .on_click(cx.listener(|this, _, _, cx| this.preview(cx))),
        ),
      )
  }

  /// The Preview step: what each line will do, confirmed with the button.
  fn render_preview(&self, cx: &mut Context<Self>) -> Div {
    let rows = self.plan.ready().cloned().unwrap_or_default();
    let ran = self.results.is_some();
    let runnable = make_count(&rows) > 0 && !ran;
    v_flex()
      .gap_3()
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
        h_flex()
          .px_4()
          .gap_2()
          .when(!ran, |bar| {
            bar.child(
              Button::new("back-to-lines")
                .outline()
                .large()
                .size(px(widgets::CTA_H - 8.))
                .icon(IconName::ChevronLeft)
                .tooltip("Back")
                .disabled(self.running)
                .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
            )
          })
          .child(
            Button::new("create-workspaces")
              .primary()
              .large()
              .flex_1()
              .h(px(widgets::CTA_H - 8.))
              .label(if ran {
                "Done".to_string()
              } else {
                action_label(&rows)
              })
              .loading(self.running)
              .disabled(!(runnable || ran) || self.running)
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

impl Render for NewWorkspacesSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let title = if self.previewing {
      SharedString::from(PREVIEW_TITLE)
    } else {
      self.title.clone()
    };
    v_flex()
      .child(
        // Matches the title row `sheet` draws for other sheets.
        h_flex()
          .h(px(36.))
          .px_4()
          .gap_2()
          .items_center()
          .child(
            div()
              .flex_1()
              .min_w_0()
              .text_sm()
              .text_color(cx.theme().muted_foreground)
              .truncate()
              .child(title),
          )
          .when(!self.previewing, |row| {
            row.child(
              Popover::new("lines-help")
                .anchor(Anchor::BottomRight)
                .trigger(
                  Button::new("lines-help-button")
                    .ghost()
                    .small()
                    .icon(LucideIcon::CircleQuestionMark),
                )
                .max_w(px(280.))
                .text_sm()
                .child(LINES_HELP),
            )
          }),
      )
      .child(if self.previewing {
        self.render_preview(cx)
      } else {
        self.render_lines(cx)
      })
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
  fn the_buttons_count_only_what_is_made() {
    use NewWorkspaceStep::*;
    assert_eq!(action_label(&[row(Some(Create))]), "Create 1 workspace");
    assert_eq!(
      action_label(&[row(Some(Create)), row(Some(Open)), row(None)]),
      "Create 2 workspaces"
    );
    assert_eq!(
      action_label(&[row(Some(Create)), row(Some(AlreadyOpen))]),
      "Create 1 workspace"
    );
    assert_eq!(action_label(&[row(Some(AlreadyOpen))]), "Create workspaces");
    assert_eq!(preview_label(Some(3)), "Preview 3 workspaces");
    assert_eq!(preview_label(Some(0)), "Preview");
    assert_eq!(preview_label(None), "Preview");
  }
}
