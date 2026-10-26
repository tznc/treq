//! The Checks popover: `ChecksPopover`'s Treq workflow checks with their
//! trust and run controls, and the GitHub checks of the branch's pull
//! request (`w-80 p-2`).
//!
//! Running a workflow stays React's (`useWorkspaceChecks` tracks what runs
//! from this window for the header, the PR panel and the Logs tab): the
//! popover reads the workflows and their latest runs from the route, and
//! asks React to trust, run and re-run.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, AnyElement, Context, Div, FontWeight, Hsla, SharedString,
};

use super::{data::ChecksData, Popover};
use crate::{
  github::{PrCheckEntry, PrCiStatus},
  gpui_shell::{
    theme::{self, Palette},
    ui::{self, Button, Size, TextSize, Tone, Variant},
  },
};

/// `w-80`.
pub const WIDTH: f32 = 320.;

/// What the popover draws, which the workspace view keeps current.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
  /// The workflows, their latest runs, trust and setup script.
  pub data: ChecksData,
  /// The branch's pull request checks, when it has any.
  pub ci: Option<PrCiStatus>,
  /// Workflow files running from this window, a job of them included.
  pub running: Vec<String>,
  /// Workflow files running as a whole.
  pub running_workflows: Vec<String>,
  /// The setup script is being re-run from this window.
  pub setup_rerunning: bool,
  /// "View logs" is offered (the Logs preview is on).
  pub logs: bool,
}

pub struct State {
  input: Input,
}

impl State {
  pub fn new(input: Input) -> Self {
    Self { input }
  }

  pub fn set_input(&mut self, input: Input) {
    self.input = input;
  }
}

/// `ci-status.ts`' `ciStateForBucket`.
pub fn ci_state(bucket: &str) -> &'static str {
  match bucket {
    "pass" | "skipping" => "success",
    "fail" | "cancel" => "failure",
    crate::github::INACTIVE => "inactive",
    _ => "pending",
  }
}

/// `compareCiChecksBySeverity`: failed first, then pending, then passed.
fn severity(check: &PrCheckEntry) -> u8 {
  match ci_state(&check.bucket) {
    "failure" => 0,
    "pending" => 1,
    "inactive" => 3,
    _ => 2,
  }
}

/// `formatCheckDuration`.
pub fn check_duration(secs: u64) -> String {
  let (hours, minutes, seconds) = (secs / 3600, (secs % 3600) / 60, secs % 60);
  if hours > 0 {
    format!("{hours}h {minutes}m {seconds}s")
  } else if minutes > 0 {
    format!("{minutes}m {seconds}s")
  } else {
    format!("{seconds}s")
  }
}

/// `ciStatusStyle(state).textClassName`: `text-x-700 dark:text-x-400`.
fn ci_color(state: &str, p: &Palette) -> Hsla {
  match state {
    "success" => Tone::Success,
    "failure" => Tone::Danger,
    _ => Tone::Warning,
  }
  .text(p)
}

/// `ChecksStatusIcon`: one icon per run or summary status.
fn status_icon(
  id: impl Into<gpui::ElementId>,
  status: Option<&str>,
  size: f32,
  p: &Palette,
) -> AnyElement {
  match status {
    Some("passed") => {
      ui::icon(theme::ICON_CIRCLE_CHECK, size, Tone::Success.color(p)).into_any_element()
    }
    Some("failed") => {
      ui::icon(theme::ICON_CIRCLE_X, size, Tone::Danger.color(p)).into_any_element()
    }
    Some("running" | "pending") => ui::spinner(id, size, p.muted_foreground).into_any_element(),
    _ => ui::icon(theme::ICON_CIRCLE_DOT, size, p.muted_foreground).into_any_element(),
  }
}

impl Popover {
  pub(super) fn load_checks(&mut self, _: &mut Context<Self>) {}

  fn trust_repo(&mut self, cx: &mut Context<Self>) {
    self.press("workspace.checksTrust", cx);
  }

  fn run_all(&mut self, cx: &mut Context<Self>) {
    self.press("workspace.checksRunAll", cx);
  }

  fn rerun_setup(&mut self, cx: &mut Context<Self>) {
    self.press("workspace.checksRerunSetup", cx);
  }

  fn run_workflow(&mut self, filename: String, cx: &mut Context<Self>) {
    self.field("workspace.checksRun", filename.into(), cx);
  }

  /// "View logs": the popover closes and React opens the Logs tab.
  fn view_logs(&mut self, cx: &mut Context<Self>) {
    self.close(cx);
    self.press("workspace.checksLogs", cx);
  }

  pub(super) fn render_checks(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
    let super::Content::Checks(state) = &self.content else {
      return div().into_any_element();
    };
    let p = *p;
    let input = state.input.clone();
    let data = &input.data;
    let (summary, ..) = data.rollup(&input.running);
    let has_treq = summary != "none";
    let can_run = data.can_run();
    let setup_running = data.setup_running();

    let title = |text: String| {
      ui::text(div(), TextSize::Xs)
        .px_2()
        .pb_1()
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.muted_foreground)
        .child(text)
    };

    let mut panel = self.panel("checks-popover", WIDTH, 8., &p);
    if has_treq {
      let logs = input.logs.then(|| {
        ui::text(div(), TextSize::Xs)
          .id("checks-view-logs")
          .debug_selector(|| "checks-view-logs".into())
          .px_2()
          .pb_1()
          .cursor_pointer()
          .text_color(p.primary)
          .hover(|s| s.underline())
          .child("View logs")
          .on_click(cx.listener(|this, _, _, cx| this.view_logs(cx)))
      });
      let mut section = div()
        .child(
          div()
            .flex()
            .items_center()
            .justify_between()
            .child(title("Treq checks".into()))
            .children(logs),
        )
        .children(data.workflows.iter().enumerate().map(|(i, wf)| {
          let running = input.running_workflows.contains(&wf.filename);
          let status = if running {
            Some("running")
          } else {
            wf.latest.as_ref().map(|(_, s)| s.as_str())
          };
          let detail = if running {
            "running".to_string()
          } else if let Some((id, status)) = &wf.latest {
            format!("run #{id} {status}")
          } else {
            "not run".to_string()
          };
          let rerun = wf.latest.is_some();
          let disabled = !can_run || running;
          let filename = wf.filename.clone();
          let id = SharedString::from(format!("checks-run-{i}"));
          ui::text(div(), TextSize::Sm)
            .id(SharedString::from(format!("checks-workflow-{i}")))
            .debug_selector(move || format!("checks-workflow-{i}"))
            .flex()
            .items_center()
            .gap_2()
            .py_1()
            .px_2()
            .child(status_icon(
              SharedString::from(format!("checks-spin-{i}")),
              status,
              16.,
              &p,
            ))
            .child(div().flex_1().min_w_0().truncate().child(wf.name.clone()))
            .child(
              div()
                .flex_none()
                .text_color(p.muted_foreground)
                .child(detail),
            )
            .child(
              Button::new(id, &p)
                .aria_label(format!(
                  "{} {}",
                  if rerun { "Re-run" } else { "Run" },
                  wf.name
                ))
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .size_6()
                .p_0()
                .disabled(disabled)
                .icon(
                  if rerun {
                    theme::ICON_ROTATE_CW
                  } else {
                    theme::ICON_PLAY
                  },
                  12.,
                )
                .on_click(
                  cx.listener(move |this, _, _, cx| this.run_workflow(filename.clone(), cx)),
                ),
            )
        }));

      let amber = Tone::Warning.text(&p);
      let amber_border = Tone::Warning.border(&p);
      let any_running = summary == "running";
      let mut actions = div().flex().items_center().gap_2().px_2().pt_2();
      if !data.trusted {
        actions = actions.child(
          Button::new("checks-trust", &p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .gap_1()
            .border_color(amber_border)
            .text_color(amber)
            .child(ui::icon(theme::ICON_SHIELD_CHECK, 12., amber))
            .label("Trust Repository")
            .on_click(cx.listener(|this, _, _, cx| this.trust_repo(cx))),
        );
      }
      actions = actions.child(
        Button::new("checks-run-all", &p)
          .aria_label("Run all checks")
          .variant(Variant::Outline)
          .size(Size::Sm)
          .gap_1()
          .disabled(!can_run || any_running)
          .child(if any_running {
            ui::spinner("checks-run-all-spin", 12., p.foreground).into_any_element()
          } else {
            ui::icon(theme::ICON_PLAY, 12., p.foreground).into_any_element()
          })
          .child("Run all")
          .on_click(cx.listener(|this, _, _, cx| this.run_all(cx))),
      );
      if let Some(setup) = data.setup.as_ref().filter(|s| s.configured) {
        let busy = input.setup_rerunning || setup_running;
        actions = actions.child(
          Button::new("checks-setup", &p)
            .aria_label("Re-run setup script")
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .gap_1()
            .ml_auto()
            .disabled(busy)
            .child(if busy {
              ui::spinner("checks-setup-spin", 12., p.foreground).into_any_element()
            } else {
              status_icon("checks-setup-status", Some(setup.status.as_str()), 12., &p)
            })
            .child("Setup")
            .on_click(cx.listener(|this, _, _, cx| this.rerun_setup(cx))),
        );
      }
      section = section.child(actions);
      panel = panel.child(section);
    }
    if let Some(ci) = &input.ci {
      panel = panel.child(self.github_section(ci, has_treq, &p, cx));
    }
    panel.into_any_element()
  }

  /// The pull request's checks, failed first, each linking to its run.
  fn github_section(
    &self,
    ci: &PrCiStatus,
    after_treq: bool,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let p = *p;
    let mut checks: Vec<&PrCheckEntry> = ci.checks.iter().collect();
    checks.sort_by_key(|c| severity(c));
    let rows = checks.into_iter().enumerate().map(|(i, check)| {
      let state = ci_state(&check.bucket);
      let color = ci_color(state, &p);
      let inactive = state == "inactive";
      let icon = if inactive {
        ui::icon(theme::ICON_MINUS, 16., p.muted_foreground).into_any_element()
      } else if state == "pending" {
        ui::spinner(SharedString::from(format!("ci-spin-{i}")), 16., color).into_any_element()
      } else {
        ui::icon(
          if state == "success" {
            theme::ICON_CIRCLE_CHECK
          } else {
            theme::ICON_CIRCLE_X
          },
          16.,
          color,
        )
        .into_any_element()
      };
      let link = (!check.link.is_empty()).then(|| check.link.clone());
      let id = SharedString::from(format!("ci-check-{i}"));
      let selector = id.to_string();
      ui::text(div(), TextSize::Sm)
        .id(id)
        .debug_selector(move || selector)
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .py_1p5()
        .px_2()
        .rounded(radius::SM)
        .child(icon)
        .child(
          div()
            .flex_1()
            .min_w_0()
            .truncate()
            .when(inactive, |d| d.text_color(p.muted_foreground))
            .child(check.name.clone()),
        )
        .when(inactive, |row| {
          let selector = format!("ci-inactive-{i}");
          row.child(
            div()
              .debug_selector(move || selector)
              .flex_none()
              .text_color(p.muted_foreground)
              .child("inactive"),
          )
        })
        .children(check.duration_secs.filter(|_| !inactive).map(|secs| {
          div()
            .flex_none()
            .text_color(p.muted_foreground)
            .child(check_duration(secs))
        }))
        .when_some(link, |this, link| {
          this
            .cursor_pointer()
            .hover(|s| s.bg(p.muted.opacity(0.5)))
            .on_click(cx.listener(move |this, _, _, _| this.host.open_url(&link)))
        })
    });
    div()
      .debug_selector(|| "checks-github".into())
      .when(after_treq, |this| {
        this.mt_2().border_t_1().border_color(p.border).pt_2()
      })
      .child(
        ui::text(div(), TextSize::Xs)
          .px_2()
          .pb_1()
          .font_weight(FontWeight::MEDIUM)
          .text_color(p.muted_foreground)
          .child(format!("GitHub checks · {}/{}", ci.passed, ci.total)),
      )
      .child(
        div()
          .id("checks-github-list")
          .max_h(zpx(320.))
          .overflow_y_scroll()
          .children(rows),
      )
  }
}
