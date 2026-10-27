//! `Onboarding`: the window with no repository open, "Overview".
//!
//! Two panels: the machines Treq opens repositories on (this computer,
//! saved SSH hosts, the Treq Cloud workspace and dev containers), each with
//! a popover of the tools installed on it, and the repositories opened
//! recently on each machine, grouped by machine. A connected SSH host's
//! repository form shows under its group.
//!
//! Drawn from the backend directly (see `machines`): GPUI checks the tools
//! and opens local repositories itself. Connecting to a machine, opening a
//! remote repository and the remote setup dialog go to the window.

use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{collections::HashMap, rc::Rc, sync::Arc, time::Instant};

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, AnyElement, Context, Div, FontWeight, Window,
};

use super::onboarding_remote;

use super::{
  checklist::{self, Checklist},
  machines::{Listing, Machine, MachinesHost, Place, Recent},
  settings_data::SettingsHost,
  ActionSink, Ctx, ScreenAction,
};
use crate::{
  core::{
    self,
    pty_remote_supervisor::{PtyBackend, TerminalStatus},
    PrerequisiteStatus,
  },
  gpui_shell::{
    confirm::{self, Question},
    native::{Backend, NativeEvent, ToastTone},
    theme::{self, Palette},
    ui::{
      self, radius, Alert, Button, DismissExt as _, Dot, EmptyState, IconButton, IconButtonSize,
      Loading, Popover, Row, RowDensity, Severity, Size, TextSize, Tone, Variant,
    },
  },
};

/// The machine panel's width.
const MACHINES_WIDTH: f32 = 288.;
/// The tools popover's width.
const POPOVER_WIDTH: f32 = 320.;

/// What the window sends for this screen: only the connected endpoint's
/// repository form, as GPUI reads the rest itself.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Route {
  /// An SSH endpoint is connected with no repository open: the form under
  /// its machine's group.
  pub remote_repo: Option<super::onboarding_remote::Form>,
}

/// What the onboarding screen asks of the backend; faked in tests.
pub trait OnboardingHost: MachinesHost {}

impl OnboardingHost for Backend {}

/// A machine's tools: the last check's result and whether one runs.
#[derive(Debug, Clone, Default)]
struct Tools {
  result: Option<Result<Vec<PrerequisiteStatus>, String>>,
  checking: bool,
}

/// The onboarding screen and the state it loads from the backend.
pub struct OnboardingView {
  host: Arc<dyn OnboardingHost>,
  /// The window's handlers, for the remote actions.
  react: ActionSink,
  /// This view's own handler, which the drawn buttons call.
  sink: ActionSink,
  listing: Listing,
  /// A dev container is starting, or why the last one did not.
  container: Option<Result<String, String>>,
  /// By machine id.
  tools: HashMap<String, Tools>,
  /// The machine whose tools popover is open.
  popover: Option<String>,
  /// The popover a press outside just closed, so the press on its own
  /// button does not open it again.
  popover_closed: Option<(String, Instant)>,
  dark: bool,
  route: Route,
  inputs: onboarding_remote::Inputs,
  /// Signed in, and the cloud workspace's status, as the cloud row last
  /// showed them.
  cloud_seen: (bool, Option<String>),
  /// Remove Host's or a terminals change's confirmation, while open.
  confirm: Option<confirm::Pending<Self>>,
  /// By machine id: a terminals change running (`None`), or how the last
  /// one went, shown in the machine's popover.
  terminals: HashMap<String, Option<Result<(), String>>>,
  /// By machine id: what the host last said holds its terminals, or
  /// `None` while it is asked.
  live: HashMap<String, Option<Result<TerminalStatus, String>>>,
}

/// The listing the last onboarding screen read, which the next one draws
/// until its own read returns: opening a window or coming back to the
/// start screen shows the machines and recent repositories at once.
struct LastListing(Listing);

impl gpui::Global for LastListing {}

/// What of the account the cloud row shows.
fn cloud_seen(cx: &gpui::App) -> (bool, Option<String>) {
  let account = super::account_data::account(cx);
  let status = account.cloud.and_then(|c| c.instance).map(|i| i.status);
  (account.user.is_some(), status)
}

impl OnboardingView {
  pub fn new(
    host: Arc<dyn OnboardingHost>,
    react: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let view = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      view
        .update(cx, |view, cx| view.handle(action, cx))
        .ignore_closed();
    });
    let mut this = Self {
      react,
      sink,
      listing: cx
        .try_global::<LastListing>()
        .map(|last| last.0.clone())
        .unwrap_or_default(),
      container: None,
      tools: HashMap::new(),
      popover: None,
      popover_closed: None,
      dark,
      host,
      route: Route::default(),
      inputs: onboarding_remote::Inputs::default(),
      cloud_seen: cloud_seen(cx),
      confirm: None,
      terminals: HashMap::new(),
      live: HashMap::new(),
    };
    // Signing in, or the cloud workspace changing state, changes its row.
    cx.observe_global::<super::account_data::Account>(|view, cx| {
      let seen = cloud_seen(cx);
      if seen != view.cloud_seen {
        view.cloud_seen = seen;
        view.load(cx);
      }
    })
    .detach();
    this.load(cx);
    this.check("local", Place::Local, cx);
    this
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  pub fn set_route(&mut self, route: Route, cx: &mut Context<Self>) {
    if self.route != route {
      let endpoint = |r: &Route| r.remote_repo.as_ref().map(|f| f.endpoint_id.clone());
      // A host just registered or connected shows in the list.
      let reload = endpoint(&self.route) != endpoint(&route);
      self.route = route;
      if reload {
        self.load(cx);
      }
      cx.notify();
    }
  }

  /// Lists the machines and recent repositories again, off the main thread.
  /// Docker answers last: the rows draw without it, and the dev containers
  /// join them (the last ones stay until then).
  fn load(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let listed = host.clone();
      let mut listing = cx.background_spawn(async move { listed.listing() }).await;
      let containers = listing.containers;
      this
        .update(cx, |view, cx| {
          if containers {
            let (machines, recents) = view.dev_containers();
            listing.machines.extend(machines);
            listing.recents.extend(recents);
          }
          view.show(listing, cx);
        })
        .ignore_closed();
      if !containers {
        return;
      }
      let (machines, recents) = cx.background_spawn(async move { host.containers() }).await;
      this
        .update(cx, |view, cx| {
          let mut listing = view.listing.clone();
          listing
            .machines
            .retain(|m| !matches!(m.place, Place::DevContainer { .. }));
          listing
            .recents
            .retain(|r| !r.machine_id.starts_with("devcontainer:"));
          listing.machines.extend(machines);
          listing.recents.extend(recents);
          view.show(listing, cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// The dev containers the last listing had.
  fn dev_containers(&self) -> (Vec<Machine>, Vec<Recent>) {
    (
      self
        .listing
        .machines
        .iter()
        .filter(|m| matches!(m.place, Place::DevContainer { .. }))
        .cloned()
        .collect(),
      self
        .listing
        .recents
        .iter()
        .filter(|r| r.machine_id.starts_with("devcontainer:"))
        .cloned()
        .collect(),
    )
  }

  /// Draws `listing` and keeps it for the next screen, unless it is the
  /// one drawn.
  fn show(&mut self, listing: Listing, cx: &mut Context<Self>) {
    crate::startup::mark("onboarding: machines listed");
    cx.set_global(LastListing(listing.clone()));
    if self.listing != listing {
      self.listing = listing;
      cx.notify();
    }
  }

  /// Looks up the machine's tools again, off the main thread.
  fn check(&mut self, id: &str, place: Place, cx: &mut Context<Self>) {
    self.tools.entry(id.to_string()).or_default().checking = true;
    cx.notify();
    let (host, id) = (self.host.clone(), id.to_string());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move { host.machine_tools(&place) })
        .await;
      this
        .update(cx, |view, cx| {
          view.tools.insert(
            id,
            Tools {
              result: Some(result),
              checking: false,
            },
          );
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn machine(&self, id: &str) -> Option<&Machine> {
    self.listing.machines.iter().find(|m| m.id == id)
  }

  fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    let press = match action {
      // The open popover's "Check again", or this computer's.
      ScreenAction::RecheckPrerequisites => {
        let id = self.popover.clone().unwrap_or_else(|| "local".into());
        if let Some(machine) = self.machine(&id).cloned() {
          self.check(&machine.id, machine.place, cx);
        }
        return;
      }
      ScreenAction::OpenUrl { url } => return self.host.open_url(&url),
      ScreenAction::Press { id } => id,
      other => return (self.react)(other, cx),
    };
    let (name, arg) = press.split_once(':').unwrap_or((press.as_str(), ""));
    match name {
      "machines.tools" => self.toggle_popover(arg, cx),
      "machines.addContainer" => self.add_container(cx),
      "machines.open" => {
        let Some(machine) = self.machine(arg).cloned() else {
          return;
        };
        self.popover = None;
        self.open_on(&machine, cx);
      }
      "machines.signIn" => self.host.open_url(&format!(
        "{}/sign-in?source=desktop",
        crate::gpui_shell::native::web_url()
      )),
      "machines.openLocal" => self.host.open_local(arg),
      "machines.remove" => {
        let Some(machine) = self.machine(arg).cloned() else {
          return;
        };
        self.popover = None;
        let question = Question::new(
          format!("Remove {}?", machine.name),
          "Treq forgets this SSH host. Nothing on the host is deleted.",
          "Remove host",
        )
        .destructive();
        confirm::ask(
          &mut self.confirm,
          question,
          move |view, cx| {
            if view.host.remove_machine(&machine.place) {
              view.tools.remove(&machine.id);
              view.load(cx);
            }
          },
          cx,
        );
        cx.notify();
      }
      "machines.terminalBackend" => {
        let Some(machine) = self.machine(arg).cloned() else {
          return;
        };
        let Some(current) = machine.terminals else {
          return;
        };
        let next = match current {
          PtyBackend::Daemon => PtyBackend::Tmux,
          PtyBackend::Tmux => PtyBackend::Daemon,
        };
        self.popover = None;
        let question = Question::new(
          format!(
            "Use {} for terminals on {}?",
            backend_name(next),
            machine.name
          ),
          "Terminals and agents running on the host now are closed.",
          match next {
            PtyBackend::Daemon => "Use Treq daemon",
            PtyBackend::Tmux => "Use tmux",
          },
        )
        .destructive();
        confirm::ask(
          &mut self.confirm,
          question,
          move |view, cx| {
            let place = machine.place.clone();
            view.change_terminals(&machine.id, cx, move |host| {
              host.set_terminal_backend(&place, next)
            });
          },
          cx,
        );
        cx.notify();
      }
      "machines.resetTerminals" => {
        let Some(machine) = self.machine(arg).cloned() else {
          return;
        };
        self.popover = None;
        let question = Question::new(
          format!("Reset terminals on {}?", machine.name),
          "Every terminal and agent Treq runs on the host is closed.",
          "Reset terminals",
        )
        .destructive();
        confirm::ask(
          &mut self.confirm,
          question,
          move |view, cx| {
            let place = machine.place.clone();
            view.change_terminals(&machine.id, cx, move |host| host.reset_terminals(&place));
          },
          cx,
        );
        cx.notify();
      }
      _ => (self.react)(ScreenAction::Press { id: press.clone() }, cx),
    }
  }

  /// Asks the machine what holds its terminals (`treq pty-remote status`).
  fn ask_terminals(&mut self, id: &str, place: Place, cx: &mut Context<Self>) {
    self.live.insert(id.to_string(), None);
    cx.notify();
    let (host, id) = (self.host.clone(), id.to_string());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move { host.terminal_status(&place) })
        .await;
      this
        .update(cx, |view, cx| {
          view.live.insert(id, Some(result));
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// Runs a change to a machine's terminals off the main thread, then lists
  /// the machines again so its popover shows the outcome.
  fn change_terminals(
    &mut self,
    id: &str,
    cx: &mut Context<Self>,
    change: impl FnOnce(&dyn OnboardingHost) -> Result<(), String> + Send + 'static,
  ) {
    self.terminals.insert(id.to_string(), None);
    cx.notify();
    let (host, id) = (self.host.clone(), id.to_string());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move { change(host.as_ref()) })
        .await;
      this
        .update(cx, |view, cx| {
          view.terminals.insert(id.clone(), Some(result));
          if let Some(machine) = view.machine(&id).cloned() {
            view.ask_terminals(&id, machine.place, cx);
          }
          view.load(cx);
        })
        .ignore_closed();
    })
    .detach();
  }

  /// The Dev Containers section's +: a repository's container, started.
  fn add_container(&mut self, cx: &mut Context<Self>) {
    if matches!(self.container, Some(Ok(_))) {
      return;
    }
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let picker = host.clone();
      let Some(path) = cx
        .background_spawn(async move { picker.pick_folder() })
        .await
      else {
        return;
      };
      this
        .update(cx, |view, cx| {
          view.container = Some(Ok(path.clone()));
          cx.notify();
        })
        .ignore_closed();
      let result = cx
        .background_spawn(async move { host.start_dev_container(&path) })
        .await;
      this
        .update(cx, |view, cx| {
          view.container = result.err().map(Err);
          view.load(cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  /// A machine's row: open a repository on it.
  fn open_on(&mut self, machine: &Machine, cx: &mut Context<Self>) {
    let press = |id: String| ScreenAction::Press { id };
    match &machine.place {
      Place::Local => (self.react)(ScreenAction::OpenRepo, cx),
      Place::Ssh { endpoint_id } => {
        (self.react)(press(format!("machines.connect:{endpoint_id}")), cx)
      }
      Place::Cloud {
        endpoint_id: Some(endpoint_id),
        status,
      } if status == "ready" => (self.react)(press(format!("machines.connect:{endpoint_id}")), cx),
      // The setup dialog signs in, creates, wakes or repairs the cloud
      // workspace.
      Place::Cloud { .. } => (self.react)(press("machines.cloud".into()), cx),
      Place::DevContainer { repo_path, .. } => self.host.open_local(repo_path),
    }
  }

  fn toggle_popover(&mut self, id: &str, cx: &mut Context<Self>) {
    let just_closed = self
      .popover_closed
      .take()
      .is_some_and(|(closed, at)| closed == id && at.elapsed().as_millis() < 500);
    if self.popover.as_deref() == Some(id) || just_closed {
      self.popover = None;
    } else {
      self.popover = Some(id.to_string());
      // Remote machines are asked only when their popover first opens.
      if let Some(machine) = self.machine(id).cloned() {
        if !self.tools.contains_key(id) {
          self.check(&machine.id, machine.place.clone(), cx);
        }
        // Terminals come and go, so their status is asked on every open.
        if machine.terminals.is_some() {
          self.ask_terminals(&machine.id, machine.place, cx);
        }
      }
    }
    cx.notify();
  }

  fn close_popover(&mut self, cx: &mut Context<Self>) -> bool {
    let Some(id) = self.popover.take() else {
      return false;
    };
    self.popover_closed = Some((id, Instant::now()));
    cx.notify();
    true
  }

  /// The machine's tools as a checklist, titled with its name.
  fn checklist(&self, machine: &Machine) -> Checklist {
    let tools = self.tools.get(&machine.id);
    let statuses = tools
      .and_then(|t| t.result.as_ref())
      .and_then(|r| r.as_ref().ok())
      .map(Vec::as_slice);
    let checking = tools.is_none_or(|t| t.checking);
    if machine.place.is_host() {
      checklist::from_host_statuses(&machine.name, statuses, checking)
    } else {
      checklist::from_statuses(&machine.name, statuses, checking)
    }
  }
}

fn plural(n: usize, one: &str) -> String {
  if n == 1 {
    format!("1 {one}")
  } else {
    format!("{n} {one}s")
  }
}

/// What the host said holds its terminals, as one line with a dot: green
/// when the machine's backend holds them, grey when idle, amber when the
/// host could not answer or lacks what the machine is set to.
fn live_status(
  status: Option<Result<TerminalStatus, String>>,
  backend: PtyBackend,
  p: &Palette,
) -> AnyElement {
  let (tone, line) = match status {
    None => (None, "Asking the host…".to_string()),
    Some(Err(error)) => (
      Some(Tone::Warning),
      format!("Couldn't ask the host: {error}"),
    ),
    Some(Ok(status)) => match backend {
      PtyBackend::Daemon => {
        let older = (status.tmux_sessions > 0).then(|| {
          format!(
            " {} from before still in tmux.",
            plural(status.tmux_sessions, "terminal")
          )
        });
        match status.daemon {
          Some(d) => (
            Some(Tone::Success),
            format!(
              "Treq daemon running (pid {}, treq {}): {}.{}",
              d.pid,
              d.version,
              plural(d.sessions, "terminal"),
              older.unwrap_or_default()
            ),
          ),
          None => (
            None,
            format!(
              "Treq daemon not running. It starts with the first terminal.{}",
              older.unwrap_or_default()
            ),
          ),
        }
      }
      PtyBackend::Tmux if !status.tmux_installed => (
        Some(Tone::Warning),
        "tmux is not installed on the host.".to_string(),
      ),
      PtyBackend::Tmux => (
        if status.tmux_sessions > 0 {
          Some(Tone::Success)
        } else {
          None
        },
        format!("tmux: {}.", plural(status.tmux_sessions, "terminal")),
      ),
    },
  };
  div()
    .id("machine-terminals-status")
    .debug_selector(|| "machine-terminals-status".into())
    .role(gpui::Role::Status)
    .aria_label(gpui::SharedString::from(line.clone()))
    .flex()
    .items_center()
    .gap_2()
    .children(tone.map(|tone| Dot::new(tone, p)))
    .child(ui::text(div(), TextSize::Sm).min_w_0().child(line))
    .test_support()
    .into_any_element()
}

fn backend_name(backend: PtyBackend) -> &'static str {
  match backend {
    PtyBackend::Daemon => "the Treq daemon",
    PtyBackend::Tmux => "tmux",
  }
}

fn press(id: impl Into<String>) -> ScreenAction {
  ScreenAction::Press { id: id.into() }
}

fn place_icon(place: &Place) -> &'static str {
  match place {
    Place::Local => theme::ICON_LAPTOP,
    Place::Ssh { .. } => theme::ICON_SERVER,
    Place::Cloud { .. } => theme::ICON_CLOUD,
    Place::DevContainer { .. } => theme::ICON_BOX,
  }
}

/// A group's label in the machines panel, as the sidebar's.
fn group_label(label: &str, p: &Palette) -> Div {
  ui::text(div(), TextSize::Xs)
    .font_weight(FontWeight::SEMIBOLD)
    .text_color(p.muted_foreground)
    .child(label.to_uppercase())
}

impl OnboardingView {
  fn machine_row(&self, machine: &Machine, ctx: &Ctx, cx: &mut Context<Self>) -> AnyElement {
    let p = ctx.palette;
    let list = self.checklist(machine);
    let error = self
      .tools
      .get(&machine.id)
      .and_then(|t| t.result.as_ref())
      .and_then(|r| r.as_ref().err())
      .cloned();
    // One dot for the machine's tools, once they are checked.
    let (status, status_label) = match (error.is_some(), checklist::ready(&list)) {
      (true, _) => (Some(Tone::Warning), "Could not check tools"),
      (false, Some(true)) => (Some(Tone::Success), "Tools installed"),
      (false, Some(false)) => (Some(Tone::Danger), "Tools missing"),
      (false, None) => (None, "Check tools"),
    };
    let open = self.popover.as_deref() == Some(machine.id.as_str());
    let tools_button = IconButton::new(
      format!("machine-tools-{}", machine.id),
      theme::ICON_WRENCH,
      format!("{status_label} on {}", machine.name),
      &p,
    )
    .size(IconButtonSize::Small)
    .selected(open)
    .on_click({
      // Not the row's press, which opens a repository on the machine.
      let send = ctx.send(press(format!("machines.tools:{}", machine.id)));
      move |event, window, cx| {
        cx.stop_propagation();
        send(event, window, cx);
      }
    });
    let popover = open.then(|| self.tools_popover(machine, list, error, ctx, cx));
    let row_id = format!("machine-{}", machine.id);
    div()
      .relative()
      .child(
        Row::new(row_id, &p)
          .density(if machine.detail.is_empty() {
            RowDensity::Default
          } else {
            RowDensity::TwoLine
          })
          .icon(place_icon(&machine.place))
          .title(machine.name.clone())
          .when(!machine.detail.is_empty(), |row| {
            row.meta(machine.detail.clone())
          })
          .when_some(status, |row, tone| row.trailing(Dot::new(tone, &p)))
          .actions(tools_button, 24.)
          .selected(open)
          .on_click(ctx.send(press(format!("machines.open:{}", machine.id)))),
      )
      .children(popover)
      .into_any_element()
  }

  fn tools_popover(
    &self,
    machine: &Machine,
    list: Checklist,
    error: Option<String>,
    ctx: &Ctx,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let p = ctx.palette;
    let removable = matches!(machine.place, Place::Ssh { .. });
    let terminals = machine
      .terminals
      .map(|backend| self.terminals_section(machine, backend, ctx));
    Popover::new("machine-tools-popover", &p)
      .w(zpx(POPOVER_WIDTH))
      .p_3()
      .gap_3()
      .dismiss_with("machine-tools", cx, |view, cx| view.close_popover(cx))
      .child(checklist::render(&list, ctx))
      .when_some(error, |d, error| {
        d.child(
          Alert::new("machine-tools-error", "Couldn't check the tools", &p)
            .severity(Severity::Warning)
            .message(error),
        )
      })
      .children(terminals)
      .when(removable, |d| {
        d.child(
          div()
            .pt_2()
            .border_t_1()
            .border_color(p.border)
            .flex()
            .justify_end()
            .child(
              Button::new("machine-remove", &p)
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .label("Remove host…")
                .on_click(ctx.send(press(format!("machines.remove:{}", machine.id)))),
            ),
        )
      })
      .into_any_element()
  }

  /// What holds the machine's terminals, switching it (SSH hosts, whose
  /// setting Treq keeps), and resetting them.
  fn terminals_section(&self, machine: &Machine, backend: PtyBackend, ctx: &Ctx) -> AnyElement {
    let p = ctx.palette;
    let state = self.terminals.get(&machine.id);
    let busy = matches!(state, Some(None));
    let switchable = matches!(machine.place, Place::Ssh { .. });
    div()
      .debug_selector(|| "machine-terminals".into())
      .pt_2()
      .border_t_1()
      .border_color(p.border)
      .flex()
      .flex_col()
      .gap_2()
      .child(
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child(format!("Terminals are held by {}.", backend_name(backend))),
      )
      .child(live_status(
        self.live.get(&machine.id).cloned().flatten(),
        backend,
        &p,
      ))
      .when_some(
        state.cloned().flatten().and_then(Result::err),
        |d, error| {
          d.child(
            Alert::new("machine-terminals-error", "Couldn't change terminals", &p)
              .severity(Severity::Warning)
              .message(error),
          )
        },
      )
      .child(
        div()
          .flex()
          .gap_2()
          .justify_end()
          .when(switchable, |d| {
            d.child(
              Button::new("machine-terminal-backend", &p)
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .disabled(busy)
                .label(match backend {
                  PtyBackend::Daemon => "Use tmux…",
                  PtyBackend::Tmux => "Use Treq daemon…",
                })
                .on_click(ctx.send(press(format!("machines.terminalBackend:{}", machine.id)))),
            )
          })
          .child(
            Button::new("machine-reset-terminals", &p)
              .variant(Variant::Ghost)
              .size(Size::Sm)
              .disabled(busy)
              .label("Reset terminals…")
              .on_click(ctx.send(press(format!("machines.resetTerminals:{}", machine.id)))),
          ),
      )
      .into_any_element()
  }

  /// The left panel: the machines by section, and adding one.
  fn machines_panel(&self, ctx: &Ctx, cx: &mut Context<Self>) -> Div {
    let p = ctx.palette;
    let mut sections: Vec<(&'static str, Vec<AnyElement>)> = Vec::new();
    for machine in &self.listing.machines {
      let row = self.machine_row(machine, ctx, cx);
      let section = machine.place.section();
      match sections.iter_mut().find(|(s, _)| *s == section) {
        Some((_, rows)) => rows.push(row),
        None => sections.push((section, vec![row])),
      }
    }
    // Sections with a + show even with nothing in them yet.
    let ssh = Place::Ssh {
      endpoint_id: String::new(),
    }
    .section();
    if self.listing.ssh && !sections.iter().any(|(s, _)| *s == ssh) {
      sections.insert(sections.len().min(1), (ssh, Vec::new()));
    }
    let containers = Place::DevContainer {
      container_id: String::new(),
      repo_path: String::new(),
      running: false,
    }
    .section();
    if self.listing.containers && !sections.iter().any(|(s, _)| *s == containers) {
      sections.push((containers, Vec::new()));
    }
    let add = |section: &str| match section {
      s if s == ssh => Some(("machines-add-ssh", "Add SSH host", "machines.addSsh")),
      s if s == containers => Some((
        "machines-add-container",
        "Add dev container",
        "machines.addContainer",
      )),
      _ => None,
    };
    let mut container_status = self.container.clone().map(|status| match status {
      Ok(path) => div()
        .px_2()
        .py_1()
        .child(
          Loading::new("machines-container-starting", &p).label(format!(
            "Starting {}…",
            path.rsplit('/').next().unwrap_or(&path)
          )),
        )
        .into_any_element(),
      Err(error) => div()
        .px_2()
        .py_1()
        .child(
          Alert::new(
            "machines-container-status",
            "Couldn't start the dev container",
            &p,
          )
          .severity(Severity::Danger)
          .message(error),
        )
        .into_any_element(),
    });
    div()
      .debug_selector(|| "onboarding-machines".into())
      .flex_none()
      .w(zpx(MACHINES_WIDTH))
      .h_full()
      .flex()
      .flex_col()
      .border_r_1()
      .border_color(p.border)
      .bg(p.sidebar)
      .text_color(p.sidebar_foreground)
      .child(
        ui::text(div(), TextSize::Sm)
          .px_4()
          .pt_4()
          .pb_2()
          .font_weight(FontWeight::SEMIBOLD)
          .child("Machines"),
      )
      .child(
        div()
          .id("onboarding-machine-list")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .px_2()
          .children(sections.into_iter().map(|(section, rows)| {
            let empty = rows.is_empty();
            div()
              .pb_3()
              .child(
                div()
                  .h(zpx(theme::CONTROL_SM))
                  .flex()
                  .items_center()
                  .justify_between()
                  .pl_2()
                  .child(group_label(section, &p))
                  .children(add(section).map(|(id, label, action)| {
                    IconButton::new(id, theme::ICON_PLUS, label, &p)
                      .size(IconButtonSize::Small)
                      .on_click(ctx.send(press(action)))
                  })),
              )
              .children(rows)
              .when(section == containers, |d| {
                d.children(container_status.take())
              })
              .when(
                empty && !(section == containers && self.container.is_some()),
                |d| {
                  d.child(
                    ui::text(div(), TextSize::Sm)
                      .px_2()
                      .py_1()
                      .text_color(p.muted_foreground)
                      .child(if section == ssh {
                        "No hosts yet."
                      } else {
                        "No containers yet."
                      }),
                  )
                },
              )
          })),
      )
      // Signed out there is no cloud row: signing in adds it.
      .when(self.listing.signed_out, |d| {
        d.child(
          div().p_3().border_t_1().border_color(p.border).child(
            Button::new("onboarding-sign-in", &p)
              .variant(Variant::Outline)
              .w_full()
              .icon(theme::ICON_CLOUD, 16.)
              .label("Sign in to Treq Cloud")
              .on_click(ctx.send(press("machines.signIn"))),
          ),
        )
      })
  }

  fn recent_row(&self, i: usize, recent: &Recent, ctx: &Ctx) -> AnyElement {
    let p = ctx.palette;
    let action = match &recent.saved_id {
      Some(id) => press(format!("machines.openSaved:{id}")),
      None => press(format!("machines.openLocal:{}", recent.path)),
    };
    Row::new(format!("recent-{}-{i}", recent.machine_id), &p)
      .density(RowDensity::Default)
      .icon(theme::ICON_FOLDER_GIT)
      .title(recent.name.clone())
      .meta(recent.path.clone())
      .on_click(ctx.send(action))
      .into_any_element()
  }

  /// The right panel: recent repositories grouped by machine, with the
  /// connected host's repository form under its group.
  fn recents_panel(&mut self, ctx: &Ctx, window: &mut Window, cx: &mut Context<Self>) -> Div {
    let p = ctx.palette;
    let form = self.route.remote_repo.clone();
    if let Some(form) = &form {
      self.inputs.sync(form, &self.react, window, cx);
    }
    let form_machine = form.as_ref().and_then(|form| {
      self
        .listing
        .machines
        .iter()
        .find(|m| match &m.place {
          Place::Ssh { endpoint_id } => *endpoint_id == form.endpoint_id,
          Place::Cloud { endpoint_id, .. } => endpoint_id.as_deref() == Some(&form.endpoint_id),
          _ => false,
        })
        .map(|m| m.id.clone())
    });
    let groups: Vec<AnyElement> = self
      .listing
      .machines
      .iter()
      .filter_map(|machine| {
        let recents: Vec<_> = self
          .listing
          .recents
          .iter()
          .filter(|r| r.machine_id == machine.id)
          .enumerate()
          .map(|(i, r)| self.recent_row(i, r, ctx))
          .collect();
        let with_form = form_machine.as_deref() == Some(machine.id.as_str());
        if recents.is_empty() && !with_form {
          return None;
        }
        let form = with_form.then(|| {
          div()
            .debug_selector(|| "onboarding-remote-form".into())
            .mx_2()
            .mt_2()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(radius::MD)
            .border_1()
            .border_color(p.border)
            .child(
              ui::text(div(), TextSize::Sm)
                .text_color(p.muted_foreground)
                .child("Pick the repository's folder on the host."),
            )
            .children(form.clone().map(|form| {
              onboarding_remote::render(&form, &self.inputs, &self.react, self.dark, &p, cx)
            }))
        });
        Some(
          div()
            .debug_selector(|| "onboarding-recent-group".into())
            .pb_5()
            .child(
              div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .pb_1()
                .child(ui::icon(
                  place_icon(&machine.place),
                  14.,
                  p.muted_foreground,
                ))
                // A name the user gave: kept as typed.
                .child(
                  ui::text(div(), TextSize::Xs)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.muted_foreground)
                    .child(machine.name.clone()),
                )
                .when(!machine.detail.is_empty(), |d| {
                  d.child(
                    ui::text(div(), TextSize::Xs)
                      .min_w_0()
                      .truncate()
                      .text_color(p.muted_foreground)
                      .child(machine.detail.clone()),
                  )
                }),
            )
            .children(recents)
            .children(form)
            .into_any_element(),
        )
      })
      .collect();
    let empty = groups.is_empty().then(|| {
      EmptyState::new("onboarding-no-recents", "No recent repositories", &p)
        .icon(theme::ICON_FOLDER_GIT)
        .message("Repositories you open show up here, grouped by machine.")
    });
    div()
      .flex_1()
      .min_w_0()
      .h_full()
      .flex()
      .flex_col()
      .child(
        div()
          .flex()
          .items_start()
          .justify_between()
          .gap_4()
          .px_6()
          .pt_6()
          .pb_4()
          .child(
            div()
              .flex()
              .flex_col()
              .gap_1p5()
              .child(
                ui::text(div(), TextSize::Lg)
                  .font_weight(FontWeight::SEMIBOLD)
                  .child("Overview"),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child("Pick a recent repository, or a machine to open one on."),
              ),
          )
          .child(
            Button::new("onboarding-open-repo", &p)
              .flex_none()
              .icon(theme::ICON_FOLDER_OPEN, 16.)
              .label("Open repository…")
              .on_click(ctx.send(ScreenAction::OpenRepo)),
          ),
      )
      .child(
        div()
          .id("onboarding-recents")
          .debug_selector(|| "onboarding-recents".into())
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .px_4()
          .pb_6()
          .child(
            ui::text(div(), TextSize::Sm)
              .px_2()
              .pb_3()
              .font_weight(FontWeight::SEMIBOLD)
              .child("Recent repositories"),
          )
          .children(groups)
          .children(empty),
      )
  }
}

impl Render for OnboardingView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let p = theme::palette(self.dark);
    let sink = self.sink.clone();
    let ctx = Ctx {
      palette: p,
      sink: &sink,
    };
    let machines = self.machines_panel(&ctx, cx);
    let recents = self.recents_panel(&ctx, window, cx);
    let confirm = confirm::render(Self::confirm_slot, self, window, &p, cx);
    div()
      .size_full()
      .flex()
      .bg(p.background)
      .text_color(p.foreground)
      .child(machines)
      .child(recents)
      .children(confirm)
  }
}

impl OnboardingView {
  fn confirm_slot(view: &mut Self) -> &mut Option<confirm::Pending<Self>> {
    &mut view.confirm
  }
}

/// Opens the repository at `path` in this window, as React's
/// `openRepositoryAtPath` does, and tells React it is open. Blocks.
pub fn open_repo_at(backend: &Backend, path: &str) {
  open_repo(backend, path, |path| {
    backend.set_window_repo_path(path);
    backend.with_db(|db| crate::core::remote_records::remember_local_opened(db, path));
  });
}

/// `openRepositoryAtPath` through `host`; `shown` records the path as the
/// window's (`setWindowRepoPath`). A folder that fails to open is not
/// remembered as the last one. The window model has already offered to
/// initialize a folder that is not a repository, so one still without a
/// repository was declined.
pub(super) fn open_repo<H>(host: &H, path: &str, shown: impl FnOnce(&str))
where
  H: SettingsHost + ?Sized,
{
  let toast = |tone, title: &str, description: String| {
    SettingsHost::notify(
      host,
      NativeEvent::Toast {
        title: title.into(),
        description,
        tone,
        action: None,
      },
    )
  };
  if !core::is_repository(path) {
    toast(
      ToastTone::Error,
      "Not a Git Repository",
      "Please select a folder that contains a git repository.".into(),
    );
    return;
  }
  // The rebase pass runs behind the window, which opens at once; its
  // `prepare_repo` must not set the repository up a second time.
  if let Err(err) = core::init_deferring_rebase(path) {
    toast(
      ToastTone::Error,
      "Failed to open repository",
      err.to_string(),
    );
    return;
  }
  crate::gpui_shell::note_just_initialized(path);
  let last = [("last_opened_repo_path".to_string(), path.to_string())];
  if let Err(err) = host.set_settings(&last) {
    log::warn!("failed to remember the last opened repository: {err}");
  }
  shown(path);
  SettingsHost::notify(
    host,
    NativeEvent::RepoOpened {
      path: path.to_string(),
    },
  );
  let name = path
    .rsplit('/')
    .find(|part| !part.is_empty())
    .unwrap_or(path);
  toast(ToastTone::Success, &format!("Opened {name}"), String::new());
}
