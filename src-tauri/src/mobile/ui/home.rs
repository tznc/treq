//! Home: a landing page when signed out with no machines, otherwise the
//! machine list (Treq Cloud and SSH hosts), each with its own connection.

use crate::mobile::ui::widgets::TouchButton as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    alert::Alert,
    button::{Button, ButtonVariants},
    h_flex,
    spinner::Spinner,
    v_flex, ActiveTheme, IconName, Sizable,
  },
  div, px, AnyElement, App, Context, Div, InteractiveElement, IntoElement, ParentElement, Render,
  ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
};
use treq_ui::{Dot, Tone};

use super::{model, screens, widgets, Machine, MachineState, MachineStatus};
use crate::core::auth::OAuthProvider;
use crate::core::pty_remote_supervisor::PtyBackend;
use crate::mobile::{
  backend::MANAGED_MACHINE,
  errors::{self, Fix},
  managed, store,
};

const TAGLINE: &str = "Isolates each agent and rebases stacked PRs when the base moves";

pub struct HomeScreen {
  scroll: ScrollHandle,
}

/// The status line and dot for a machine row.
fn status(state: &MachineState) -> (Option<Tone>, Option<String>) {
  match &state.status {
    MachineStatus::Idle => (None, None),
    MachineStatus::Connecting(step) => (None, Some(format!("{step}…"))),
    MachineStatus::Ready => (Some(Tone::Success), Some("Connected".into())),
    MachineStatus::Failed(error) => (
      Some(Tone::Danger),
      Some(match errors::describe(error).fix {
        Some(Fix::ReviewHostKey) => "Host key changed".into(),
        _ => "Can't connect".into(),
      }),
    ),
    MachineStatus::CutOff(_) => (Some(Tone::Warning), Some("Access ended".into())),
  }
}

/// The state named in the row's detail line, beside its dot or spinner.
fn status_word(state: &MachineState) -> Option<String> {
  let shown = state.status.is_connecting() || state.status == MachineStatus::Ready;
  status(state).1.filter(|_| shown)
}

/// Whether machine `id` is the one open beside the list (`beside`), as
/// the active connection.
fn is_open(id: &str, beside: bool, active: Option<&str>) -> bool {
  beside && active == Some(id)
}

/// Repository folder name, for the row's detail line.
fn repo_name(path: &str) -> &str {
  path
    .trim_end_matches('/')
    .rsplit('/')
    .next()
    .unwrap_or(path)
}

/// What the host is missing, as one sentence, if anything.
fn missing_tools(state: &MachineState) -> Option<String> {
  let check = state.check.as_ref()?;
  // Hosts added here keep the default backend, the Treq daemon.
  let backend = PtyBackend::default();
  match (!check.holds_terminals(backend), check.agents.is_empty()) {
    (false, false) => None,
    (true, true) => Some(
      "This host can't keep terminals (Treq supports Linux and macOS hosts), and has no agent CLI (claude, codex, cursor-agent, copilot)."
        .into(),
    ),
    (true, false) => Some(
      "This host can't keep terminals (Treq supports Linux and macOS hosts): terminals and agents won't start."
        .into(),
    ),
    (false, true) => Some("No agent CLI found (claude, codex, cursor-agent, copilot).".into()),
  }
}

impl HomeScreen {
  pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
    cx.observe(&model(cx), |_, _, cx| cx.notify()).detach();
    Self {
      scroll: ScrollHandle::new(),
    }
  }

  /// One machine: dot, name, status or last repository, and a problem with
  /// its fix underneath when the last connect failed.
  fn machine_row(
    &self,
    machine: Machine,
    name: String,
    address: Option<String>,
    trailing: Option<AnyElement>,
    cx: &mut Context<Self>,
  ) -> Div {
    let state = model(cx).read(cx).machine(machine.id());
    let palette = treq_ui::theme::current(cx);
    let (tone, status_text) = status(&state);
    let last_repo = crate::mobile::backend::get()
      .connection_to(machine.id())
      .and_then(|c| super::last_repository_on(&c.endpoint))
      .or_else(|| match &machine {
        Machine::Host(record) => super::last_repository_on(&record.to_endpoint()),
        Machine::Managed => None,
      });
    let detail = [
      // Beside the dot, so the word is never cut off.
      status_word(&state),
      address,
      last_repo.as_deref().map(|r| repo_name(r).to_string()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let connecting = state.status.is_connecting();
    let id = machine.id().to_string();
    // A fixed slot, so names line up whether or not a machine has a status.
    let leading = div()
      .w(px(14.))
      .flex()
      .justify_center()
      .map(|slot| match (connecting, tone) {
        (true, _) => slot.child(Spinner::new().small()),
        (false, Some(tone)) => slot.child(Dot::new(
          tone,
          status_text.clone().unwrap_or_default(),
          &palette,
        )),
        (false, None) => slot,
      })
      .into_any_element();
    let active = model(cx)
      .read(cx)
      .connection
      .as_ref()
      .map(|c| c.machine().to_string());
    let open = is_open(
      &id,
      super::nav::beside(cx.entity_id(), cx).is_some(),
      active.as_deref(),
    );
    let row = widgets::row_full(
      SharedString::from(format!("machine-{id}")),
      Some(leading),
      name,
      (!detail.is_empty()).then(|| detail.into()),
      trailing,
      cx,
    )
    .when(open, |row| row.bg(palette.selected()))
    .when(!connecting, |row| {
      let machine = machine.clone();
      row.on_click(move |_, _, cx| super::open_machine(machine.clone(), cx))
    });

    let problem = match &state.status {
      MachineStatus::Failed(error) => Some(errors::describe(error)),
      MachineStatus::CutOff(reason) => Some(errors::Problem {
        message: format!("This device's access ended ({reason}). Reauthenticate to continue."),
        fix: Some(Fix::Reauthenticate),
      }),
      _ => None,
    };
    let warning = (state.status == MachineStatus::Ready)
      .then(|| missing_tools(&state))
      .flatten();
    v_flex()
      .child(row)
      .when_some(problem, |col, problem| {
        let action = problem.fix.map(|fix| {
          let machine = machine.clone();
          let (label, id) = match fix {
            Fix::Retry => ("Retry", "retry"),
            Fix::ReviewHostKey => ("Review host key", "review-key"),
            Fix::Reauthenticate => ("Reauthenticate", "reauthenticate"),
          };
          Button::new(SharedString::from(format!("{id}-{}", machine.id())))
            .outline()
            .label(label)
            .on_click(move |_, _, cx| match fix {
              Fix::Retry => super::connect(machine.clone(), true, cx),
              Fix::ReviewHostKey => {
                if let Machine::Host(record) = &machine {
                  screens::add_host::review_key(record.clone(), cx)
                }
              }
              Fix::Reauthenticate => super::reauthenticate(cx),
            })
            .touch()
        });
        col.child(
          v_flex()
            .px_4()
            .py_2()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(widgets::error_text(problem.message, cx))
            .children(action.map(|a| h_flex().child(a))),
        )
      })
      .when_some(warning, |col, warning| {
        col.child(
          div()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(widgets::muted(warning, cx)),
        )
      })
  }

  fn cloud(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let model = model(cx).read(cx);
    let busy = model.auth_busy;
    let error = model.auth_error.clone();
    let notice = model.auth_notice.clone();
    let signed_in = model.user.is_some();
    let section = widgets::section("Treq Cloud", cx);
    if !signed_in {
      return section.child(
        widgets::card(cx)
          .p_4()
          .gap_3()
          .children(notice.map(|n| div().text_sm().child(n)))
          .child(widgets::muted(
            "Sign in to reach your Treq Cloud machine.",
            cx,
          ))
          .child(h_flex().gap_2().children(provider_buttons(busy, false)))
          .children(error.map(|e| widgets::error_text(e, cx).px_4().pb_3())),
      );
    }
    let secure_storage = match &model.machine(MANAGED_MACHINE).status {
      MachineStatus::Failed(error) => error
        .strip_prefix(managed::SECURE_STORAGE_PREFIX)
        .map(|detail| detail.trim().to_string()),
      _ => None,
    };
    let row = self.machine_row(Machine::Managed, "Cloud machine".into(), None, None, cx);
    section
      .child(widgets::card(cx).child(row))
      .when_some(secure_storage, |section, detail| {
        section.child(
          Alert::warning("secure-storage", detail)
            .title("Secure storage isn't set up on this device"),
        )
      })
  }

  fn hosts(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let hosts = store::user_managed_endpoints();
    let palette = treq_ui::theme::current(cx);
    let count = hosts.len();
    let title = treq_ui::SectionHeader::new("section-ssh-hosts", "SSH hosts", &palette)
      .when(count > 0, |header| header.count(count));
    let list = if hosts.is_empty() {
      widgets::card(cx).child(widgets::empty(
        "hosts-empty",
        "No SSH hosts on this device yet.",
        cx,
      ))
    } else {
      let rows = hosts
        .into_iter()
        .map(|host| {
          let address = format!("{}@{}:{}", host.username, host.hostname, host.port);
          let edit_record = host.clone();
          let edit = Button::new(SharedString::from(format!("edit-{}", host.id)))
            .ghost()
            .small()
            .icon(IconName::Ellipsis)
            .accessibility_label("Edit host")
            .on_click(move |_, _, cx| {
              cx.stop_propagation();
              screens::add_host::edit(edit_record.clone(), cx);
            })
            .touch();
          let name = host.display_name.clone();
          self.machine_row(
            Machine::Host(host),
            name,
            Some(address),
            Some(edit.into_any_element()),
            cx,
          )
        })
        .collect::<Vec<_>>();
      widgets::card(cx).children(rows)
    };
    v_flex().gap_3().child(title).child(list).child(
      Button::new("add-host")
        .outline()
        .icon(IconName::Plus)
        .label("Add SSH host")
        .on_click(|_, _, cx| screens::add_host::open(cx))
        .touch(),
    )
  }

  fn account(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
    let model = model(cx).read(cx);
    let email = model.user.clone()?;
    let busy = model.auth_busy;
    Some(
      widgets::section("Account", cx).child(
        widgets::card(cx).p_4().child(
          h_flex()
            .gap_3()
            .items_center()
            .child(div().flex_1().min_w_0().truncate().child(email))
            .child(
              Button::new("sign-out")
                .outline()
                .label("Sign out")
                .loading(busy)
                .on_click(|_, _, cx| super::sign_out(cx))
                .touch(),
            ),
        ),
      ),
    )
  }

  /// The signed-out start screen: brand centered, sign-in within thumb reach.
  fn landing(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let model = model(cx).read(cx);
    let busy = model.auth_busy;
    let error = model.auth_error.clone();
    let notice = model.auth_notice.clone();
    v_flex()
      .min_h_full()
      .p_6()
      .gap_6()
      .child(
        v_flex()
          .flex_1()
          .items_center()
          .justify_center()
          .gap_3()
          .child(widgets::logo(96.))
          .child(widgets::wordmark(40., cx))
          .child(widgets::muted(TAGLINE, cx).max_w(px(300.)).text_center()),
      )
      .child(
        v_flex()
          .gap_3()
          .children(provider_buttons(busy, true))
          .children(error.map(|e| widgets::error_text(e, cx).text_center()))
          .children(notice.map(|n| widgets::muted(n, cx).text_center()))
          .child(or_divider(cx))
          .child(
            Button::new("add-host")
              .outline()
              .large()
              .w_full()
              .h(px(widgets::CTA_H))
              .label("Use your own SSH host")
              .on_click(|_, _, cx| screens::add_host::open(cx))
              .touch(),
          ),
      )
  }
}

impl Render for HomeScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let model = model(cx).read(cx);
    let landing = super::is_landing(model.user.as_deref(), super::has_machines(model));
    let content = if landing {
      self.landing(cx).into_any_element()
    } else {
      let cloud = self.cloud(cx);
      let hosts = self.hosts(cx);
      let account = self.account(cx);
      widgets::page()
        .child(hosts)
        .child(cloud)
        .children(account)
        .into_any_element()
    };
    div()
      .id("home")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .bg(cx.theme().background)
      .child(content)
  }
}

/// One sign-in button per OAuth provider; `cta` makes them full-width.
/// A rule with "or" in its middle, between sign-in and the SSH host option.
fn or_divider(cx: &App) -> impl IntoElement {
  let theme = cx.theme();
  let rule = || div().flex_1().h(px(1.)).bg(theme.border);
  h_flex()
    .gap_3()
    .items_center()
    .child(rule())
    .child(widgets::muted("or", cx))
    .child(rule())
}

fn provider_buttons(busy: bool, cta: bool) -> Vec<treq_ui::press::Press> {
  [
    (
      "sign-in-github",
      "Continue with GitHub",
      treq_ui::theme::ICON_GITHUB,
      OAuthProvider::Github,
    ),
    (
      "sign-in-google",
      "Continue with Google",
      treq_ui::theme::ICON_GOOGLE,
      OAuthProvider::Google,
    ),
  ]
  .into_iter()
  .map(|(id, label, icon, provider)| {
    let button = Button::new(id)
      .outline()
      .icon(gpui_kit::component::Icon::default().path(icon))
      .label(label)
      .loading(busy)
      .disabled(busy);
    let button = if cta {
      button.large().w_full().h(px(widgets::CTA_H))
    } else {
      button
    };
    button
      .on_click(move |_, _, cx| super::sign_in(provider, cx))
      .touch()
  })
  .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::{remote::RemoteHostCheck, remote_pty::RemoteAgentId};

  fn ready(terminal: Option<&str>, agents: Vec<RemoteAgentId>) -> MachineState {
    MachineState {
      status: MachineStatus::Ready,
      check: Some(RemoteHostCheck {
        treq_version: "0.3.0".into(),
        root: "/home/dev".into(),
        root_exists: true,
        terminal: terminal.map(str::to_string),
        tmux: false,
        agents,
      }),
    }
  }

  /// The row says its state in words beside the dot: progress, and that it
  /// is connected. A failure is explained under the row instead.
  #[core::prelude::v1::test]
  fn the_status_dot_has_a_word() {
    let mut state = MachineState::default();
    assert_eq!(status_word(&state), None);
    state.status = MachineStatus::Ready;
    assert_eq!(status_word(&state).as_deref(), Some("Connected"));
    state.status = MachineStatus::Connecting("Connecting");
    assert_eq!(status_word(&state).as_deref(), Some("Connecting…"));
  }

  /// In a split, the machine whose repositories show beside the list is
  /// marked.
  #[core::prelude::v1::test]
  fn the_open_machine_is_highlighted() {
    assert!(is_open("host-1", true, Some("host-1")));
    assert!(!is_open("host-2", true, Some("host-1")));
    assert!(!is_open("host-1", false, Some("host-1")));
    assert!(!is_open("host-1", true, None));
  }

  #[core::prelude::v1::test]
  fn names_what_the_host_is_missing() {
    assert_eq!(
      missing_tools(&ready(Some("daemon"), vec![RemoteAgentId::Claude])),
      None
    );
    // A host whose treq predates the daemon reports its multiplexer.
    assert_eq!(
      missing_tools(&ready(Some("tmux"), vec![RemoteAgentId::Claude])),
      None
    );
    assert!(missing_tools(&ready(None, vec![RemoteAgentId::Claude]))
      .unwrap()
      .contains("can't keep terminals"));
    assert!(missing_tools(&ready(Some("daemon"), vec![]))
      .unwrap()
      .contains("agent CLI"));
    // An older treq gives no check, so nothing is claimed missing.
    assert_eq!(missing_tools(&MachineState::default()), None);
  }

  #[core::prelude::v1::test]
  fn rows_show_the_repository_folder() {
    assert_eq!(repo_name("/home/dev/src/event-bus"), "event-bus");
    assert_eq!(repo_name("/home/dev/src/event-bus/"), "event-bus");
  }

  #[core::prelude::v1::test]
  fn status_lines() {
    assert_eq!(status(&MachineState::default()), (None, None));
    let failed = MachineState {
      status: MachineStatus::Failed("x".into()),
      check: None,
    };
    assert_eq!(status(&failed).0, Some(Tone::Danger));
  }
}
