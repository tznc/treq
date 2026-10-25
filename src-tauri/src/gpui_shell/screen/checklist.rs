//! `PrerequisiteChecklist`: which of Git, the agent CLIs and the GitHub CLI
//! are installed, with an install link for each missing one.

use crate::gpui_shell::ui::zpx;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{div, prelude::*, FontWeight, SharedString};

use super::{Ctx, ScreenAction};
use crate::core::{self, PrerequisiteStatus};
use crate::gpui_shell::{
  native::Backend,
  theme,
  ui::{self, Button, Size, TextSize, Variant},
};

/// What a screen with the checklist asks of the backend; faked in tests.
pub trait ToolsHost: Send + Sync {
  /// Blocks: called off the main thread.
  fn check_prerequisites(&self) -> Vec<PrerequisiteStatus>;
  fn open_url(&self, url: &str);
}

impl ToolsHost for Backend {
  fn check_prerequisites(&self) -> Vec<PrerequisiteStatus> {
    core::check_prerequisites()
  }

  fn open_url(&self, url: &str) {
    Backend::open_url(self, url);
  }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checklist {
  pub title: String,
  /// A check is running: "Check again" is disabled and its icon spins.
  pub validating: bool,
  pub groups: Vec<Group>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
  pub label: String,
  /// Drawn in the destructive color: no agent CLI is installed.
  #[serde(default)]
  pub error: bool,
  pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
  pub binary: String,
  pub label: String,
  #[serde(default)]
  pub note: Option<String>,
  /// Whether this tool is required; a missing required tool gets a red X.
  #[serde(default)]
  pub required: bool,
  /// `None` while the check is still running.
  pub installed: Option<bool>,
  pub install_url: String,
}

pub fn render(list: &Checklist, ctx: &Ctx) -> impl IntoElement {
  let p = ctx.palette;
  let header = div()
    .mb_1()
    .flex()
    .items_center()
    .justify_between()
    .child(ui::text(div().font_weight(FontWeight::MEDIUM), TextSize::Sm).child(list.title.clone()))
    .child({
      let refresh = ui::icon(theme::ICON_REFRESH, 12., p.foreground);
      ui::text(
        Button::new("prerequisites-recheck", &p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .disabled(list.validating)
          .mr_neg_2()
          .h(zpx(24.))
          .gap_1()
          .px_2(),
        TextSize::Xs,
      )
      .child(if list.validating {
        ui::spin("prerequisites-spin", refresh).into_any_element()
      } else {
        refresh.into_any_element()
      })
      .label("Check again")
      .on_click(ctx.send(ScreenAction::RecheckPrerequisites))
    });

  div()
    .w_full()
    .child(header)
    .children(list.groups.iter().map(|group| {
      div()
        .mt_2()
        .child(
          ui::text(div(), TextSize::Xs)
            .text_color(if group.error {
              p.destructive
            } else {
              p.muted_foreground
            })
            .child(group.label.clone()),
        )
        .children(group.tools.iter().map(|tool| tool_row(tool, ctx)))
    }))
}

fn tool_row(tool: &Tool, ctx: &Ctx) -> impl IntoElement {
  let p = ctx.palette;
  let status = match tool.installed {
    Some(true) => ui::icon(theme::ICON_CIRCLE_CHECK, 16., p.success),
    Some(false) if tool.required => ui::icon(theme::ICON_CIRCLE_X, 16., p.destructive),
    _ => ui::icon(theme::ICON_CIRCLE, 16., p.muted_foreground),
  };
  let muted = |label: &'static str| {
    ui::text(div(), TextSize::Xs)
      .text_color(p.muted_foreground)
      .child(label)
      .into_any_element()
  };
  let trailing = match tool.installed {
    None => muted("Checking..."),
    Some(true) => muted("Installed"),
    Some(false) => {
      let id = SharedString::from(format!("install-{}", tool.binary));
      ui::text(
        Button::new(id, &p)
          .variant(Variant::Link)
          .size(Size::Sm)
          .h_auto()
          .gap_1()
          .p_0(),
        TextSize::Xs,
      )
      .label("Install")
      .icon(theme::ICON_EXTERNAL_LINK, 12.)
      .on_click(ctx.send(ScreenAction::OpenUrl {
        url: tool.install_url.clone(),
      }))
      .into_any_element()
    }
  };
  div()
    .flex()
    .items_center()
    .gap_2()
    .py_1()
    .child(status)
    .child(
      div()
        .min_w_0()
        .flex_1()
        .child(ui::text(div(), TextSize::Sm).child(tool.label.clone()))
        .when_some(tool.note.clone(), |this, note| {
          this.child(
            ui::text(div(), TextSize::Xs)
              .text_color(p.muted_foreground)
              .child(note),
          )
        }),
    )
    .child(trailing)
}

/// Which group a tool is listed under.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
  Required,
  Agent,
  Optional,
}

/// A tool the checklist reports on (`PREREQUISITE_TOOLS` in React).
struct Known {
  binary: &'static str,
  /// Only a machine Treq reaches over SSH or in a container needs it: the
  /// app runs jj and its terminals itself.
  host_only: bool,
  label: &'static str,
  kind: Kind,
  install_url: &'static str,
  note: Option<&'static str>,
}

/// The tools, in display order. Their binaries are
/// `core::PREREQUISITE_BINARIES`, and `core::HOST_BINARIES` on a host.
const TOOLS: [Known; 8] = [
  Known {
    binary: "git",
    host_only: false,
    label: "Git",
    kind: Kind::Required,
    install_url: "https://git-scm.com/downloads",
    note: None,
  },
  Known {
    binary: "jj",
    host_only: true,
    label: "Jujutsu",
    kind: Kind::Required,
    install_url: "https://jj-vcs.github.io/jj/latest/install-and-setup/",
    note: None,
  },
  Known {
    binary: "tmux",
    host_only: true,
    label: "tmux",
    kind: Kind::Optional,
    install_url: "https://github.com/tmux/tmux/wiki/Installing",
    note: Some("Only for hosts set to keep terminals in tmux"),
  },
  Known {
    binary: "claude",
    host_only: false,
    label: "Claude Code",
    kind: Kind::Agent,
    install_url: "https://code.claude.com/docs/en/setup",
    note: None,
  },
  Known {
    binary: "codex",
    host_only: false,
    label: "Codex",
    kind: Kind::Agent,
    install_url: "https://developers.openai.com/codex/cli",
    note: None,
  },
  Known {
    binary: "cursor-agent",
    host_only: false,
    label: "Cursor Agent",
    kind: Kind::Agent,
    install_url: "https://cursor.com/docs/cli/installation",
    note: None,
  },
  Known {
    binary: "copilot",
    host_only: false,
    label: "GitHub Copilot CLI",
    kind: Kind::Agent,
    install_url: "https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli",
    note: None,
  },
  Known {
    binary: "gh",
    host_only: false,
    label: "GitHub CLI",
    kind: Kind::Optional,
    install_url: "https://cli.github.com/",
    note: Some("Needed for pull requests and issues"),
  },
];

const GROUPS: [(Kind, &str); 3] = [
  (Kind::Required, "Required"),
  (Kind::Agent, "Agent CLI (install at least one)"),
  (Kind::Optional, "Optional"),
];

/// The checklist for `statuses` from `core::check_prerequisites`, or every
/// tool still "Checking..." before the first check returns.
pub fn from_statuses(
  title: &str,
  statuses: Option<&[PrerequisiteStatus]>,
  validating: bool,
) -> Checklist {
  checklist(title, statuses, validating, false)
}

/// The checklist of a machine Treq reaches over SSH or in a container:
/// jj is required there too, and tmux is offered.
pub fn from_host_statuses(
  title: &str,
  statuses: Option<&[PrerequisiteStatus]>,
  validating: bool,
) -> Checklist {
  checklist(title, statuses, validating, true)
}

/// Whether every required tool is installed, or `None` before a check.
pub fn ready(list: &Checklist) -> Option<bool> {
  let required = || {
    list
      .groups
      .iter()
      .flat_map(|g| &g.tools)
      .filter(|t| t.required)
  };
  if required().any(|t| t.installed.is_none()) {
    return None;
  }
  Some(required().all(|t| t.installed == Some(true)) && !list.groups.iter().any(|g| g.error))
}

fn checklist(
  title: &str,
  statuses: Option<&[PrerequisiteStatus]>,
  validating: bool,
  host: bool,
) -> Checklist {
  let installed = |binary: &str| {
    statuses.map(|statuses| {
      statuses
        .iter()
        .any(|status| status.binary == binary && status.installed)
    })
  };
  let no_agent = statuses.is_some()
    && !TOOLS
      .iter()
      .any(|tool| tool.kind == Kind::Agent && installed(tool.binary) == Some(true));
  Checklist {
    title: title.to_string(),
    validating,
    groups: GROUPS
      .iter()
      .map(|(kind, label)| Group {
        label: label.to_string(),
        error: *kind == Kind::Agent && no_agent,
        tools: TOOLS
          .iter()
          .filter(|tool| tool.kind == *kind && (host || !tool.host_only))
          .map(|tool| Tool {
            binary: tool.binary.to_string(),
            label: tool.label.to_string(),
            note: tool.note.map(str::to_string),
            required: tool.kind == Kind::Required,
            installed: installed(tool.binary),
            install_url: tool.install_url.to_string(),
          })
          .collect(),
      })
      .collect(),
  }
}
