//! `Onboarding`: the window with no repository open, "Open a Repository".
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
use std::{collections::HashMap, rc::Rc, sync::Arc, time::Instant};

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, deferred, div, prelude::*, AnyElement, Context, Div, FontWeight, SharedString, Window,
};

use super::onboarding_remote;

use super::{
  checklist::{self, Checklist},
  machines::{Listing, Machine, MachinesHost, Place, Recent},
  settings_data::SettingsHost,
  workspace_ops::WorkspaceActionsHost,
  ActionSink, Ctx, ScreenAction,
};
use crate::{
  core::{self, PrerequisiteStatus},
  gpui_shell::{
    native::{Backend, NativeEvent, ToastTone},
    theme::{self, Palette},
    ui::{self, Button, DismissExt as _, Size, TextSize, Variant},
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
pub trait OnboardingHost: MachinesHost {
  /// Asks for a folder, then opens it in this window.
  fn pick_repo(&self);
}

impl OnboardingHost for Backend {
  fn pick_repo(&self) {
    pick_repo(self.clone());
  }
}

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
}

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
      let _ = view.update(cx, |view, cx| view.handle(action, cx));
    });
    let mut this = Self {
      react,
      sink,
      listing: Listing::default(),
      container: None,
      tools: HashMap::new(),
      popover: None,
      popover_closed: None,
      dark,
      host,
      route: Route::default(),
      inputs: onboarding_remote::Inputs::default(),
      cloud_seen: cloud_seen(cx),
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
  fn load(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let listing = cx.background_spawn(async move { host.listing() }).await;
      let _ = this.update(cx, |view, cx| {
        view.listing = listing;
        cx.notify();
      });
    })
    .detach();
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
      let _ = this.update(cx, |view, cx| {
        view.tools.insert(
          id,
          Tools {
            result: Some(result),
            checking: false,
          },
        );
        cx.notify();
      });
    })
    .detach();
  }

  fn machine(&self, id: &str) -> Option<&Machine> {
    self.listing.machines.iter().find(|m| m.id == id)
  }

  fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    let press = match action {
      ScreenAction::OpenRepo => return self.host.pick_repo(),
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
        if self.host.remove_machine(&machine.place) {
          self.tools.remove(&machine.id);
          self.load(cx);
        }
        cx.notify();
      }
      _ => (self.react)(ScreenAction::Press { id: press.clone() }, cx),
    }
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
      let _ = this.update(cx, |view, cx| {
        view.container = Some(Ok(path.clone()));
        cx.notify();
      });
      let result = cx
        .background_spawn(async move { host.start_dev_container(&path) })
        .await;
      let _ = this.update(cx, |view, cx| {
        view.container = result.err().map(Err);
        view.load(cx);
        cx.notify();
      });
    })
    .detach();
  }

  /// A machine's row: open a repository on it.
  fn open_on(&mut self, machine: &Machine, cx: &mut Context<Self>) {
    let press = |id: String| ScreenAction::Press { id };
    match &machine.place {
      Place::Local => self.host.pick_repo(),
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
          self.check(&machine.id, machine.place, cx);
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

/// A clickable list row that the e2e suite finds by its name.
fn row(
  id: impl Into<SharedString>,
  label: impl Into<SharedString>,
  p: &Palette,
) -> gpui::Stateful<Div> {
  let id = id.into();
  div()
    .id(id.clone())
    .debug_selector(move || id.to_string())
    .role(gpui::Role::Button)
    .aria_label(label)
    .flex()
    .items_center()
    .gap_2()
    .px_2()
    .py(zpx(6.))
    .rounded(zpx(6.))
    .hover(|s| s.bg(p.accent))
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
    let (status_icon, status_color, status_label) = match (error.is_some(), checklist::ready(&list))
    {
      (true, _) => (theme::ICON_CIRCLE_ALERT, p.warning, "Could not check tools"),
      (false, Some(true)) => (theme::ICON_CIRCLE_CHECK, p.success, "Tools installed"),
      (false, Some(false)) => (theme::ICON_CIRCLE_X, p.destructive, "Tools missing"),
      (false, None) => (theme::ICON_WRENCH, p.muted_foreground, "Check tools"),
    };
    let tools_id = format!("machine-tools-{}", machine.id);
    let tools_button = div()
      .id(SharedString::from(tools_id.clone()))
      .debug_selector(move || tools_id.clone())
      .role(gpui::Role::Button)
      .aria_label(format!("{status_label} on {}", machine.name))
      .flex_none()
      .p_1()
      .rounded(zpx(4.))
      .hover(|s| s.bg(p.muted))
      .child(ui::icon(status_icon, 16., status_color))
      .on_click({
        // Not the row's press, which opens a repository on the machine.
        let send = ctx.send(press(format!("machines.tools:{}", machine.id)));
        move |event, window, cx| {
          cx.stop_propagation();
          send(event, window, cx);
        }
      })
      .test_support();
    let open = self.popover.as_deref() == Some(machine.id.as_str());
    let popover = open.then(|| self.tools_popover(machine, list, error, ctx, cx));
    div()
      .relative()
      .child(
        row(format!("machine-{}", machine.id), machine.name.clone(), &p)
          .when(open, |d| d.bg(p.accent))
          .on_click(ctx.send(press(format!("machines.open:{}", machine.id))))
          .child(ui::icon(
            place_icon(&machine.place),
            16.,
            p.muted_foreground,
          ))
          .child(
            div()
              .min_w_0()
              .flex_1()
              .child(
                ui::text(div(), TextSize::Sm)
                  .truncate()
                  .child(machine.name.clone()),
              )
              .when(!machine.detail.is_empty(), |d| {
                d.child(
                  ui::text(div(), TextSize::Xs)
                    .truncate()
                    .text_color(p.muted_foreground)
                    .child(machine.detail.clone()),
                )
              }),
          )
          .child(tools_button)
          .test_support(),
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
    deferred(
      div()
        .id("machine-tools-popover")
        .debug_selector(|| "machine-tools-popover".into())
        .absolute()
        .top_0()
        .left(zpx(MACHINES_WIDTH - 16.))
        .w(zpx(POPOVER_WIDTH))
        .p_3()
        .rounded(zpx(8.))
        .border_1()
        .border_color(p.border)
        .bg(p.popover)
        .text_color(p.popover_foreground)
        .shadow_md()
        .occlude()
        .dismiss_with("machine-tools", cx, |view, cx| view.close_popover(cx))
        .child(checklist::render(&list, ctx))
        .when_some(error, |d, error| {
          d.child(
            ui::text(div(), TextSize::Xs)
              .debug_selector(|| "machine-tools-error".into())
              .mt_2()
              .text_color(p.destructive)
              .child(error),
          )
        })
        .when(removable, |d| {
          d.child(
            div()
              .mt_3()
              .pt_2()
              .border_t_1()
              .border_color(p.border.opacity(0.5))
              .flex()
              .justify_end()
              .child(
                Button::new("machine-remove", &p)
                  .variant(Variant::Ghost)
                  .size(Size::Sm)
                  .label("Remove Host")
                  .on_click(ctx.send(press(format!("machines.remove:{}", machine.id)))),
              ),
          )
        }),
    )
    .with_priority(1)
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
    let mut container_status = self.container.clone().map(|status| {
      let (text, color) = match status {
        Ok(path) => (
          format!("Starting {}…", path.rsplit('/').next().unwrap_or(&path)),
          p.muted_foreground,
        ),
        Err(error) => (error, p.destructive),
      };
      ui::text(div(), TextSize::Xs)
        .debug_selector(|| "machines-container-status".into())
        .px_2()
        .py_1()
        .text_color(color)
        .child(text)
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
                  .flex()
                  .items_center()
                  .justify_between()
                  .px_2()
                  .pb_1()
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .text_color(p.muted_foreground)
                      .child(section),
                  )
                  .children(add(section).map(|(id, label, action)| {
                    div()
                      .id(id)
                      .debug_selector(move || id.into())
                      .role(gpui::Role::Button)
                      .aria_label(label)
                      .p_0p5()
                      .rounded(zpx(4.))
                      .cursor_pointer()
                      .hover(|s| s.bg(p.accent))
                      .on_click(ctx.send(ScreenAction::Press { id: action.into() }))
                      .child(ui::icon(theme::ICON_PLUS, 14., p.muted_foreground))
                      .test_support()
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
                    ui::text(div(), TextSize::Xs)
                      .px_2()
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
              .w_full()
              .child(div().flex().mr_2().child(ui::icon(
                theme::ICON_CLOUD,
                16.,
                p.primary_foreground,
              )))
              .label("Sign in to Treq Cloud")
              .on_click(ctx.send(ScreenAction::Press {
                id: "machines.signIn".into(),
              })),
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
    row(
      format!("recent-{}-{i}", recent.machine_id),
      recent.name.clone(),
      &p,
    )
    .on_click(ctx.send(action))
    .child(ui::icon(theme::ICON_FOLDER_GIT, 16., p.muted_foreground))
    .child(
      ui::text(div(), TextSize::Sm)
        .flex_none()
        .font_weight(FontWeight::MEDIUM)
        .child(recent.name.clone()),
    )
    .child(
      ui::text(div(), TextSize::Xs)
        .min_w_0()
        .flex_1()
        .truncate()
        .text_color(p.muted_foreground)
        .child(recent.path.clone()),
    )
    .test_support()
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
            .rounded(zpx(8.))
            .border_1()
            .border_color(p.border)
            .child(
              ui::text(div(), TextSize::Sm)
                .pb_2()
                .text_color(p.muted_foreground)
                .child("Enter the repository's path on the host."),
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
                .child(
                  ui::text(div(), TextSize::Xs)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.muted_foreground)
                    .child(machine.name.clone()),
                )
                .when(!machine.detail.is_empty(), |d| {
                  d.child(
                    ui::text(div(), TextSize::Xs)
                      .truncate()
                      .text_color(p.muted_foreground.opacity(0.7))
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
      ui::text(div(), TextSize::Sm)
        .debug_selector(|| "onboarding-no-recents".into())
        .px_2()
        .py_6()
        .text_color(p.muted_foreground)
        .child("Repositories you open show up here, grouped by machine.")
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
              .gap(zpx(6.))
              .child(
                ui::text(div(), TextSize::Lg)
                  .font_weight(FontWeight::SEMIBOLD)
                  .child("Open a Repository"),
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
              .child(div().flex().mr_2().child(ui::icon(
                theme::ICON_FOLDER_OPEN,
                16.,
                p.primary_foreground,
              )))
              .label("Open Repository")
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
              .child("Recent Repositories"),
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
    div()
      .size_full()
      .flex()
      .bg(p.background)
      .text_color(p.foreground)
      .child(machines)
      .child(recents)
  }
}

/// Asks for a folder, then opens it in this window.
fn pick_repo(backend: Backend) {
  // Tauri calls must not run inside the GPUI click that asked for this.
  tauri::async_runtime::spawn(async move {
    let app = backend.app().clone();
    let _ = app.run_on_main_thread(move || {
      use tauri_plugin_dialog::DialogExt;
      let dialog = backend.app().dialog().file().set_title("Select Folder");
      dialog.pick_folder(move |folder| {
        let Some(path) = folder.and_then(|folder| folder.into_path().ok()) else {
          return;
        };
        let path = path.to_string_lossy().into_owned();
        tauri::async_runtime::spawn_blocking(move || open_repo_at(&backend, &path));
      });
    });
  });
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
/// remembered as the last one.
pub(super) fn open_repo<H>(host: &H, path: &str, shown: impl FnOnce(&str))
where
  H: SettingsHost + WorkspaceActionsHost + ?Sized,
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
    let question = format!("{path} is not a Git repository. Initialize one here?");
    if !host.ask("Not a Git Repository", &question) {
      toast(
        ToastTone::Error,
        "Not a Git Repository",
        "Please select a folder that contains a git repository.".into(),
      );
      return;
    }
    if let Err(err) = core::init_git_repository(path) {
      toast(ToastTone::Error, "Failed to open repository", err);
      return;
    }
  }
  if let Err(err) = core::init(path) {
    toast(
      ToastTone::Error,
      "Failed to open repository",
      err.to_string(),
    );
    return;
  }
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
  toast(
    ToastTone::Success,
    "Repository Opened",
    format!("Now viewing {name}"),
  );
}
