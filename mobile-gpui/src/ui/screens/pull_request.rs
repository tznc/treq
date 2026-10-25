//! The workspace branch's GitHub pull request: a tag in the workspace
//! header, the checks rows [`super::pr_detail`] shows, and the sheet that creates one. GitHub
//! is reached with the host's `gh`, so the host's sign-in is the one used.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    tag::Tag,
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};
use treq_lib::{
  core::remote::{RemotePullRequestStatus, TreqCommandRequest},
  github::{PrCheckEntry, PrCiStatus, PrInfo},
};

use super::workspace::Target;
use crate::{
  backend,
  ui::{sheet, widgets},
};

pub fn status_request(target: &Target) -> TreqCommandRequest {
  TreqCommandRequest::WorkspacePullRequest {
    repo: target.repo.clone(),
    workspace: target.id.clone(),
  }
}

/// Opens `url` in the phone's browser.
pub fn open_url(url: &str) -> Result<(), String> {
  use tauri_plugin_opener::OpenerExt;
  backend::get()
    .app
    .opener()
    .open_url(url, None::<&str>)
    .map_err(|error| error.to_string())
}

/// "Open", "Draft", "Merged" or "Closed".
pub fn state_label(pr: &PrInfo) -> &'static str {
  match pr.state.as_str() {
    "MERGED" => "Merged",
    "CLOSED" => "Closed",
    _ if pr.is_draft => "Draft",
    _ => "Open",
  }
}

/// What the checks add up to.
pub fn checks_summary(checks: &PrCiStatus) -> String {
  match checks.state.as_str() {
    "failure" => format!("{} of {} failing", checks.failed, checks.total),
    "pending" => format!("{} of {} running", checks.pending, checks.total),
    _ => format!("{} checks passed", checks.passed),
  }
}

/// The header's tag for the branch's pull request.
pub fn tag(status: &RemotePullRequestStatus) -> Option<Tag> {
  let pr = status.pull_request.as_ref()?;
  let open = pr.state == "OPEN";
  let mut label = format!("#{} {}", pr.number, state_label(pr));
  if let (true, Some(checks)) = (open, &status.checks) {
    label = format!("{label} · {}", checks_summary(checks));
  }
  let tag = match (
    pr.state.as_str(),
    status.checks.as_ref().map(|c| c.state.as_str()),
  ) {
    ("MERGED", _) => Tag::info(),
    ("CLOSED", _) => Tag::secondary(),
    (_, Some("failure")) => Tag::danger(),
    (_, Some("pending")) => Tag::warning(),
    (_, Some(_)) => Tag::success(),
    _ => Tag::secondary(),
  };
  Some(tag.small().child(label))
}

pub fn check_icon(check: &PrCheckEntry, cx: &App) -> AnyElement {
  let theme = cx.theme();
  let (icon, color) = match check.bucket.as_str() {
    "pass" => (IconName::CircleCheck, theme.success),
    "fail" | "cancel" => (IconName::CircleX, theme.danger),
    "pending" => (IconName::LoaderCircle, theme.warning),
    _ => (IconName::Minus, theme.muted_foreground),
  };
  div().text_color(color).child(icon).into_any_element()
}

pub fn duration(secs: u64) -> String {
  match secs {
    0..60 => format!("{secs}s"),
    _ => format!("{}m {}s", secs / 60, secs % 60),
  }
}

/// What [`CreateSheet`] hands back: title, body and whether it's a draft.
pub type OnCreate = Box<dyn Fn(String, String, bool, &mut App)>;

/// Title and description for a new pull request.
pub struct CreateSheet {
  title: Entity<InputState>,
  body: Entity<TextareaState>,
  into: String,
  error: Option<String>,
  on_create: OnCreate,
}

impl CreateSheet {
  pub fn new(
    title: String,
    body: String,
    into: String,
    on_create: OnCreate,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    Self {
      title: cx.new(|cx| {
        InputState::new(window, cx)
          .placeholder("Title")
          .default_value(title)
      }),
      body: cx.new(|cx| {
        TextareaState::new(window, cx)
          .placeholder("Description (optional)")
          .rows(4)
          .default_value(body)
      }),
      into,
      error: None,
      on_create,
    }
  }

  fn create(&mut self, draft: bool, cx: &mut Context<Self>) {
    let title = self.title.read(cx).value().trim().to_string();
    if title.is_empty() {
      self.error = Some("Enter a title.".into());
      return cx.notify();
    }
    let body = self.body.read(cx).value().to_string();
    sheet::close(cx);
    (self.on_create)(title, body, draft, cx);
  }
}

impl Render for CreateSheet {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    v_flex()
      .px_4()
      .pb_4()
      .gap_3()
      .child(widgets::muted(
        format!(
          "Pushes this branch, then opens a pull request into {}.",
          self.into
        ),
        cx,
      ))
      .child(Input::new(&self.title).large())
      .child(Textarea::new(&self.body))
      .when_some(self.error.clone(), |sheet, error| {
        sheet.child(widgets::error_text(error, cx))
      })
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("pr-create-draft")
              .outline()
              .large()
              .label("Create draft")
              .on_click(cx.listener(|this, _, _, cx| this.create(true, cx))),
          )
          .child(
            Button::new("pr-create")
              .primary()
              .large()
              .flex_1()
              .label("Create pull request")
              .on_click(cx.listener(|this, _, _, cx| this.create(false, cx))),
          ),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn pr(state: &str, draft: bool) -> PrInfo {
    PrInfo {
      number: 7,
      title: "Fix".into(),
      state: state.into(),
      url: "https://github.com/o/r/pull/7".into(),
      head_ref_name: "fix".into(),
      base_ref_name: "main".into(),
      merge_state_status: None,
      is_draft: draft,
      body: String::new(),
    }
  }

  fn checks(state: &str, passed: u32, failed: u32, pending: u32) -> PrCiStatus {
    PrCiStatus {
      state: state.into(),
      total: passed + failed + pending,
      passed,
      failed,
      pending,
      checks: Vec::new(),
    }
  }

  #[core::prelude::v1::test]
  fn states_read_as_words() {
    assert_eq!(state_label(&pr("OPEN", false)), "Open");
    assert_eq!(state_label(&pr("OPEN", true)), "Draft");
    assert_eq!(state_label(&pr("MERGED", false)), "Merged");
    assert_eq!(state_label(&pr("CLOSED", true)), "Closed");
  }

  #[core::prelude::v1::test]
  fn checks_sum_up_by_their_worst_state() {
    assert_eq!(
      checks_summary(&checks("failure", 3, 1, 1)),
      "1 of 5 failing"
    );
    assert_eq!(
      checks_summary(&checks("pending", 3, 0, 2)),
      "2 of 5 running"
    );
    assert_eq!(
      checks_summary(&checks("success", 4, 0, 0)),
      "4 checks passed"
    );
  }
}
