//! The GPUI interface: one navigation stack of full-screen pages. On a
//! wide screen the page under the top one shows beside it; see [`panes`].
//!
//! [`Root`] owns the stack and the app-wide [`AppModel`] (sign-in and
//! connection state). Screens push and pop pages through [`nav`].

mod delete_account;
mod hold;
mod home;
mod load;
#[cfg(test)]
mod perf;
mod review;
mod screens;
pub mod sheet;
mod terminal_view;
pub mod theme;
mod widgets;

use crate::mobile::ui::widgets::TouchButton as _;
use crate::result_ext::ResultExt as _;
use futures::{channel::mpsc::UnboundedReceiver, StreamExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, IconName, Sizable,
  },
  div, px, AnyView, AnyWindowHandle, App, AppContext, BorrowAppContext, Context, Div, Entity,
  EntityId, FontWeight, Global, InteractiveElement, IntoElement, MouseDownEvent, ParentElement,
  Pixels, Render, SharedString, Stateful, StatefulInteractiveElement, StyleRefinement, Styled,
  WeakEntity, Window,
};

use std::collections::HashMap;

use crate::core::remote::RemoteHostCheck;

use crate::mobile::{
  backend::{self, AppEvent, Connection, ConnectionKind, MANAGED_MACHINE},
  managed, remote, store,
};

pub use load::Load;

/// The signed-out home with no machines is a landing page. Once signed in,
/// or with a saved SSH host, the machine list takes over.
fn is_landing(user: Option<&str>, has_machines: bool) -> bool {
  user.is_none() && !has_machines
}

/// Whether there is a machine to list: a connection or a saved SSH host.
fn has_machines(model: &AppModel) -> bool {
  model.connection.is_some() || !store::user_managed_endpoints().is_empty()
}

/// App-wide state the screens observe.
#[derive(Default)]
pub struct AppModel {
  pub user: Option<String>,
  pub auth_busy: bool,
  pub auth_error: Option<String>,
  /// What Home says after signing out on purpose, such as a deleted account.
  pub auth_notice: Option<String>,
  /// The machine the screens act on, if any.
  pub connection: Option<Connection>,
  /// Each machine's state this session, by machine id
  /// ([`MANAGED_MACHINE`] or an SSH host's record id).
  pub machines: HashMap<String, MachineState>,
  /// Bumped when every screen should refetch (reconnect, resume).
  pub epoch: usize,
  /// Set while the launch snapshot is being restored.
  restoring: bool,
}

impl AppModel {
  pub fn machine(&self, id: &str) -> MachineState {
    self.machines.get(id).cloned().unwrap_or_default()
  }

  /// Why the active machine is cut off, if it is.
  pub fn cutoff(&self) -> Option<String> {
    let id = self.connection.as_ref()?.machine();
    match &self.machines.get(id)?.status {
      MachineStatus::CutOff(reason) => Some(reason.clone()),
      _ => None,
    }
  }

  fn set_status(&mut self, id: &str, status: MachineStatus) {
    self.machines.entry(id.to_string()).or_default().status = status;
  }
}

/// One machine's connection state.
#[derive(Clone, Default)]
pub struct MachineState {
  pub status: MachineStatus,
  /// What the host can run, from the last connect. `None` before one, or
  /// when the host's treq predates host checks.
  pub check: Option<RemoteHostCheck>,
}

#[derive(Clone, Default, PartialEq)]
pub enum MachineStatus {
  /// Not connected this session.
  #[default]
  Idle,
  /// Connecting, with the step shown.
  Connecting(&'static str),
  Ready,
  /// The last connect failed, with the raw error.
  Failed(String),
  /// The device's access ended (Treq Cloud), with the reason.
  CutOff(String),
}

impl MachineStatus {
  pub fn is_connecting(&self) -> bool {
    matches!(self, Self::Connecting(_))
  }
}

/// A machine to connect to.
#[derive(Clone)]
pub enum Machine {
  Managed,
  Host(store::UserManagedEndpoint),
}

impl Machine {
  pub fn id(&self) -> &str {
    match self {
      Machine::Managed => MANAGED_MACHINE,
      Machine::Host(record) => &record.id,
    }
  }
}

/// Where a page is, for the restore snapshot.
#[derive(Clone, Default, PartialEq)]
pub struct Location {
  pub repo: Option<String>,
  pub workspace: Option<store::SnapshotWorkspace>,
}

struct Page {
  title: SharedString,
  header: nav::Header,
  view: AnyView,
  location: Location,
}

pub struct Root {
  model: Entity<AppModel>,
  stack: Vec<Page>,
  sheet: Option<sheet::Sheet>,
  /// Set briefly after a sheet closes; see [`sheet::dismiss`].
  tap_guard: bool,
  /// Whether the window is wide enough to show two pages; see [`panes`].
  split: bool,
  /// Whether the last tap in a split was on the left pane, so a page it
  /// opens replaces the one beside it instead of stacking on top.
  list_focus: bool,
}

pub(crate) struct Nav {
  root: WeakEntity<Root>,
  pub(crate) window: AnyWindowHandle,
}
impl Global for Nav {}

struct Model(Entity<AppModel>);
impl Global for Model {}

pub fn model(cx: &App) -> Entity<AppModel> {
  cx.global::<Model>().0.clone()
}

/// Navigation: push and pop full-screen pages.
pub mod nav {
  use super::*;

  /// What a page's title bar offers besides Back.
  #[derive(Clone, Default)]
  pub struct Header {
    /// Tapping the title opens this (a switcher), shown with a chevron.
    pub on_title: Option<sheet::Action>,
    /// The ⋯ button at the trailing edge opens this (the page's menu).
    pub more: Option<sheet::Action>,
  }

  /// Pushes the page `build` creates. `location` names the repository
  /// and workspace it shows, for the restore snapshot.
  pub fn push<V: Render>(
    title: impl Into<SharedString>,
    location: Location,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
  ) {
    push_with(title, Header::default(), location, cx, build)
  }

  /// [`push`] with a title bar that offers `header`'s actions.
  pub fn push_with<V: Render>(
    title: impl Into<SharedString>,
    header: Header,
    location: Location,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
  ) {
    place(false, title.into(), header, location, cx, build)
  }

  /// [`push_with`] in place of the top page, as a switcher does.
  pub fn replace_with<V: Render>(
    title: impl Into<SharedString>,
    header: Header,
    location: Location,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
  ) {
    place(true, title.into(), header, location, cx, build)
  }

  fn place<V: Render>(
    replace: bool,
    title: SharedString,
    header: Header,
    location: Location,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
  ) {
    let Nav { root, window } = cx.global::<Nav>();
    let (root, window) = (root.clone(), *window);
    // Deferred: callers are usually inside another entity's update.
    cx.defer(move |cx| {
      window
        .update(cx, |_, window, cx| {
          let view = build(window, cx);
          root.update(cx, |root, cx| {
            if (replace || (root.split && root.list_focus)) && root.stack.len() > 1 {
              root.stack.pop();
            }
            root.stack.push(Page {
              title,
              header,
              view: view.into(),
              location,
            });
            root.stack_changed(cx);
          })
        })
        .ok();
    });
  }

  /// Whether the page showing `view` is the one on screen.
  pub fn is_top(view: EntityId, cx: &App) -> bool {
    cx.global::<Nav>()
      .root
      .upgrade()
      .and_then(|root| root.read(cx).stack.last().map(|p| p.view.entity_id()))
      == Some(view)
  }

  /// Retitles the page showing `view` (a renamed terminal).
  pub fn set_title(view: EntityId, title: impl Into<SharedString>, cx: &mut App) {
    let title = title.into();
    update(cx, move |root, cx| {
      if let Some(page) = root.stack.iter_mut().find(|p| p.view.entity_id() == view) {
        page.title = title;
        cx.notify();
      }
    });
  }

  pub fn pop(cx: &mut App) {
    pop_n(1, cx)
  }

  /// Pops `count` pages, keeping the home page.
  pub fn pop_n(count: usize, cx: &mut App) {
    update(cx, move |root, cx| {
      let keep = root.stack.len().saturating_sub(count).max(1);
      if keep < root.stack.len() {
        root.stack.truncate(keep);
        root.stack_changed(cx);
      }
    });
  }

  /// Back to the home page.
  pub fn home(cx: &mut App) {
    update(cx, |root, cx| {
      root.stack.truncate(1);
      root.stack_changed(cx);
    });
  }

  fn update(cx: &mut App, f: impl FnOnce(&mut Root, &mut Context<Root>) + 'static) {
    let root = cx.global::<Nav>().root.clone();
    // Deferred: callers are usually inside another entity's update.
    cx.defer(move |cx| {
      root.update(cx, f).ok();
    });
  }
}

impl Root {
  pub fn new(
    mut events: UnboundedReceiver<AppEvent>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let model = cx.new(|_| AppModel::default());
    cx.set_global(Nav {
      root: cx.weak_entity(),
      window: window.window_handle(),
    });
    cx.set_global(Model(model.clone()));
    sheet::install(cx.weak_entity(), cx);
    cx.observe(&model, |_, _, cx| cx.notify()).detach();
    theme::sync_appearance(window, cx);
    cx.observe_window_appearance(window, |_, window, cx| {
      theme::sync_appearance(window, cx);
    })
    .detach();

    let weak = cx.weak_entity();
    tauri_plugin_gpui::on_back(move |cx| {
      weak
        .update(cx, |root, cx| {
          if root.sheet.is_some() {
            sheet::dismiss(root, cx);
          } else if root.stack.len() > 1 {
            root.stack.pop();
            root.stack_changed(cx);
          }
        })
        .ok();
    });

    cx.spawn_in(window, async move |this, cx| {
      while let Some(event) = events.next().await {
        if this
          .update_in(cx, |root, window, cx| root.handle(event, window, cx))
          .is_err()
        {
          break;
        }
      }
    })
    .detach();

    let home = cx.new(|cx| home::HomeScreen::new(window, cx));
    let root = Self {
      model,
      stack: vec![Page {
        title: "Machines".into(),
        header: nav::Header::default(),
        view: home.into(),
        location: Location::default(),
      }],
      sheet: None,
      tap_guard: false,
      split: false,
      list_focus: false,
    };
    restore(cx);
    root
  }

  fn stack_changed(&mut self, cx: &mut Context<Self>) {
    self.sheet = None;
    tauri_plugin_gpui::set_back_enabled(self.stack.len() > 1);
    self.save_snapshot(cx);
    // A page revealed by a pop was cached while it was covered; redraw it
    // once so state it reads outside the entity (the store) is current.
    if let Some(page) = self.stack.last() {
      App::notify(cx, page.view.entity_id());
    }
    cx.notify();
  }

  fn handle(&mut self, event: AppEvent, _window: &mut Window, cx: &mut Context<Self>) {
    match event {
      AppEvent::Auth(user) => self.model.update(cx, |m, cx| {
        if user.is_some() {
          m.auth_notice = None;
        }
        m.user = user;
        cx.notify();
      }),
      AppEvent::AuthToken(token) => {
        self.model.update(cx, |m, cx| {
          m.auth_busy = true;
          m.auth_error = None;
          cx.notify();
        });
        let model = self.model.clone();
        load::run(
          cx,
          async move { backend::get().auth.exchange_token(&token).await },
          move |_, result, cx| {
            model.update(cx, |m, cx| {
              m.auth_busy = false;
              m.auth_error = result.err();
              cx.notify();
            });
          },
        );
      }
      AppEvent::Cutoff {
        endpoint_id,
        reason,
      } => {
        let machine = [MANAGED_MACHINE.to_string()]
          .into_iter()
          .chain(store::user_managed_endpoints().into_iter().map(|e| e.id))
          .find(|id| {
            backend::get()
              .connection_to(id)
              .is_some_and(|c| c.endpoint.id == endpoint_id)
          });
        if let Some(machine) = machine {
          self.model.update(cx, |m, cx| {
            m.set_status(
              &machine,
              match reason {
                Some(reason) => MachineStatus::CutOff(reason),
                None => MachineStatus::Ready,
              },
            );
            cx.notify();
          });
        }
      }
      AppEvent::Resumed => resume(cx),
    }
  }
}

/// The narrowest window that shows a page beside the one under it.
const SPLIT_MIN_WIDTH: Pixels = px(768.);
/// The width of the left pane in a split.
const LIST_PANE_WIDTH: Pixels = px(360.);

/// How many pages of a `depth`-page stack show side by side in a window
/// `width` wide: on a tablet the page under the top one (usually the list it
/// was opened from) stays on the left, on a phone only the top page shows.
fn panes(width: Pixels, depth: usize) -> usize {
  if width >= SPLIT_MIN_WIDTH && depth >= 2 {
    2
  } else {
    1
  }
}

impl Root {
  /// A page with its title bar. `back` pops this many pages, `None` hides
  /// Back.
  fn pane(&self, ix: usize, back: Option<usize>, title_bar: bool, cx: &App) -> Stateful<Div> {
    let page = &self.stack[ix];
    let theme = cx.theme();
    v_flex()
      .id(("pane", ix))
      .size_full()
      .min_w_0()
      .when(title_bar, |pane| {
        pane.child(
          h_flex()
            .h(px(56.))
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .when_some(back, |bar, count| {
              bar.child(
                Button::new("back")
                  .ghost()
                  .large()
                  .icon(IconName::ArrowLeft)
                  .on_click(move |_, _, cx| nav::pop_n(count, cx))
                  .touch(),
              )
            })
            .child(
              h_flex()
                .id("page-title")
                .flex_1()
                .min_w_0()
                .px_2()
                .gap_1()
                .items_center()
                .child(
                  div()
                    .min_w_0()
                    .truncate()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(page.title.clone()),
                )
                .when_some(page.header.on_title.clone(), |title, action| {
                  title
                    .child(
                      div()
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(IconName::ChevronDown),
                    )
                    .on_click(move |_, window, cx| action(window, cx))
                }),
            )
            .when_some(page.header.more.clone(), |bar, action| {
              bar.child(
                Button::new("page-more")
                  .ghost()
                  .large()
                  .icon(IconName::Ellipsis)
                  .on_click(move |_, window, cx| action(window, cx))
                  .touch(),
              )
            }),
        )
      })
      .child(
        div()
          .flex_1()
          .min_h_0()
          // Cached: a sheet, spinner or caret blink elsewhere in the window
          // must not re-render the page. It re-renders when it notifies
          // itself or an entity it observes, or when its bounds change.
          .child(
            page
              .view
              .clone()
              .cached(StyleRefinement::default().size_full()),
          ),
      )
  }
}

impl Render for Root {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // Keep clear of the status bar, navigation bar and soft keyboard.
    let visible = window.fully_visible_bounds();
    let viewport = window.viewport_size();
    let depth = self.stack.len();
    self.split = panes(visible.size.width, depth) == 2;
    let model = self.model.read(cx);
    // The landing page brings its own branding, so it drops the title bar.
    let landing = depth == 1 && is_landing(model.user.as_deref(), has_machines(model));
    let blocked = model.cutoff();
    let managed = model
      .connection
      .as_ref()
      .is_some_and(|c| c.kind == ConnectionKind::Managed);
    let theme = cx.theme();
    let (background, foreground, border) = (theme.background, theme.foreground, theme.border);

    let content = if self.split {
      let weak = cx.weak_entity();
      let focus = move |list: bool| {
        let weak = weak.clone();
        move |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
          weak.update(cx, |root, _| root.list_focus = list).ok();
        }
      };
      // Back on the list closes it and the page beside it.
      let list_back = (depth > 2).then_some(2);
      h_flex()
        .flex_1()
        .min_h_0()
        .child(
          div()
            .w(LIST_PANE_WIDTH)
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(border)
            .on_any_mouse_down(focus(true))
            .child(self.pane(depth - 2, list_back, true, cx)),
        )
        .child(
          div()
            .flex_1()
            .min_w_0()
            .h_full()
            .on_any_mouse_down(focus(false))
            .child(self.pane(depth - 1, Some(1), true, cx)),
        )
    } else {
      let back = (depth > 1).then_some(1);
      h_flex()
        .flex_1()
        .min_h_0()
        .child(self.pane(depth - 1, back, !landing, cx))
    };

    v_flex()
      .size_full()
      .bg(background)
      .text_color(foreground)
      .pt(visible.origin.y)
      .pl(visible.origin.x)
      .pr(viewport.width - visible.right())
      .pb(viewport.height - visible.bottom())
      .child(content)
      .when_some(blocked, |root, reason| {
        root.child(cutoff_overlay(reason, managed, cx))
      })
      .when_some(self.sheet.as_ref(), |root, sheet| {
        root.child(sheet::render(sheet, viewport.height - visible.bottom(), cx))
      })
      .when(self.tap_guard, |root| root.child(sheet::tap_guard()))
  }
}

/// Blocks repository screens until the user reauthenticates (PRD mobile
/// acceptance criterion 8).
fn cutoff_overlay(reason: String, managed: bool, cx: &App) -> impl IntoElement {
  let theme = cx.theme();
  div()
    .absolute()
    .inset_0()
    .bg(theme.background.opacity(0.96))
    .flex()
    .items_center()
    .justify_center()
    .p_6()
    .child(
      widgets::card(cx).p_5().gap_3().max_w(px(420.)).children([
        div()
          .text_lg()
          .font_weight(FontWeight::SEMIBOLD)
          .child("Remote access is blocked")
          .into_any_element(),
        widgets::muted(
          format!(
            "This device's access ended ({reason}). Its certificate was revoked or \
                         expired. Reauthenticate to continue."
          ),
          cx,
        )
        .into_any_element(),
        h_flex()
          .gap_2()
          .when(managed, |row| {
            row.child(
              Button::new("reauthenticate")
                .primary()
                .large()
                .label("Reauthenticate")
                .on_click(|_, _, cx| reauthenticate(cx))
                .touch(),
            )
          })
          .child(
            Button::new("switch-machine")
              .outline()
              .large()
              .label("Other machines")
              .on_click(|_, _, cx| show_machines(cx))
              .touch(),
          )
          .into_any_element(),
      ]),
    )
}

// Actions on the app model, shared by several screens.

pub fn sign_in(cx: &mut App) {
  use tauri_plugin_opener::OpenerExt;
  let result = backend::get().app.opener().open_url(
    crate::core::auth::sign_in_url(&crate::mobile::config::env().web_url),
    None::<&str>,
  );
  if let Err(error) = result {
    model(cx).update(cx, |m, cx| {
      m.auth_error = Some(error.to_string());
      cx.notify();
    });
  }
}

pub fn sign_out(cx: &mut App) {
  let model = model(cx);
  model.update(cx, |m, cx| {
    m.auth_busy = true;
    cx.notify();
  });
  let done = backend::spawn(async {
    let backend = backend::get();
    managed::stop();
    let ended = backend
      .pool
      .cut_off_managed(crate::core::remote_ssh_transport::CutoffReason::SessionEnded)
      .await;
    for endpoint in ended {
      backend.ptys.close_all_for_endpoint(&endpoint).await;
    }
    backend.pool.set_relay_access_token(None);
    backend.auth.sign_out().await;
    remote::forget_reads();
  });
  cx.spawn(async move |cx| {
    done.await.ignore_closed();
    backend::get().remove_connection(MANAGED_MACHINE);
    let was_active = model.update(cx, |m, cx| {
      m.auth_busy = false;
      m.machines.remove(MANAGED_MACHINE);
      let was_active = m
        .connection
        .as_ref()
        .is_some_and(|c| c.kind == ConnectionKind::Managed);
      if was_active {
        m.connection = None;
      }
      cx.notify();
      was_active
    });
    if was_active {
      cx.update(nav::home);
    }
  })
  .detach();
}

/// Opens `machine`: straight back to its last repository when it's
/// connected, otherwise after connecting.
pub fn open_machine(machine: Machine, cx: &mut App) {
  let model = model(cx);
  let state = model.read(cx).machine(machine.id());
  match (&state.status, backend::get().connection_to(machine.id())) {
    (MachineStatus::Connecting(_), _) => {}
    (MachineStatus::Ready, Some(connection)) => {
      activate(connection, &model, cx);
      open_location(cx);
    }
    _ => connect(machine, true, cx),
  }
}

/// Connects to `machine` and checks what it can run. With `open`, then
/// opens its last repository (or the repository picker).
pub fn connect(machine: Machine, open: bool, cx: &mut App) {
  let model = model(cx);
  let id = machine.id().to_string();
  model.update(cx, |m, cx| {
    m.set_status(
      &id,
      MachineStatus::Connecting(match machine {
        Machine::Managed => "Checking the machine",
        Machine::Host(_) => "Connecting",
      }),
    );
    cx.notify();
  });
  let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<&'static str>();
  // The pages a restore reopens; otherwise `open` shows the last repository.
  let snapshot = (open && model.read(cx).restoring)
    .then(store::session_snapshot)
    .flatten();
  let result = backend::spawn(async move {
    let connection = match machine {
      Machine::Host(record) => {
        let connection = Connection {
          endpoint: record.to_endpoint(),
          kind: ConnectionKind::UserManaged { id: record.id },
        };
        backend::get().put_connection(connection.clone());
        connection
      }
      Machine::Managed => {
        let tx = progress_tx.clone();
        managed::connect(move |step| {
          tx.unbounded_send(step).ignore_closed();
        })
        .await?
      }
    };
    if open {
      // Read alongside the host check, so the pages open on content.
      screens::prefetch_location(&connection.endpoint, snapshot.as_ref());
    }
    progress_tx
      .unbounded_send("Checking the machine")
      .ignore_closed();
    let check = remote::host_check(&connection.endpoint).await?;
    Ok::<_, String>((connection, check))
  });
  let progress_model = model.clone();
  let progress_id = id.clone();
  cx.spawn(async move |cx| {
    while let Some(step) = progress_rx.next().await {
      progress_model.update(cx, |m, cx| {
        if m.machine(&progress_id).status.is_connecting() {
          m.set_status(&progress_id, MachineStatus::Connecting(step));
          cx.notify();
        }
      });
    }
  })
  .detach();
  cx.spawn(async move |cx| {
    let Ok(result) = result.await else { return };
    match result {
      Ok((connection, check)) => {
        model.update(cx, |m, cx| {
          let state = m.machines.entry(id.clone()).or_default();
          state.status = MachineStatus::Ready;
          state.check = check;
          cx.notify();
        });
        cx.update(|cx| {
          // A background reconnect leaves another active machine alone.
          let current = model
            .read(cx)
            .connection
            .as_ref()
            .map(|c| c.machine().to_string());
          if open || current.is_none() || current.as_deref() == Some(id.as_str()) {
            activate(connection, &model, cx);
          }
          if open && !model.read(cx).restoring {
            open_location(cx);
          }
        });
      }
      Err(error) => model.update(cx, |m, cx| {
        m.set_status(&id, MachineStatus::Failed(error));
        cx.notify();
      }),
    }
  })
  .detach();
}

/// Has every screen refetch, after a change one screen made that others
/// show (an archived workspace leaves the list).
pub fn refetch(cx: &mut App) {
  model(cx).update(cx, |m, cx| {
    m.epoch += 1;
    cx.notify();
  });
}

/// Makes `connection`'s machine the one the screens act on.
fn activate(connection: Connection, model: &Entity<AppModel>, cx: &mut App) {
  backend::get().set_active(Some(connection.machine()));
  model.update(cx, |m, cx| {
    m.connection = Some(connection);
    m.epoch += 1;
    cx.notify();
  });
}

/// The active machine's last repository, or its repository picker.
fn open_location(cx: &mut App) {
  nav::home(cx);
  screens::repos::open(cx);
  if let Some(repo) = last_repository(cx) {
    screens::workspaces::open(repo, cx);
  }
}

/// The repository last opened on the active machine.
pub fn last_repository(cx: &App) -> Option<String> {
  let connection = model(cx).read(cx).connection.clone()?;
  last_repository_on(&connection.endpoint)
}

pub fn last_repository_on(
  endpoint: &crate::core::remote_control_plane::SshEndpoint,
) -> Option<String> {
  store::saved_repositories(&endpoint.id, store::endpoint_generation(endpoint))
    .into_iter()
    .next()
    .map(|r| r.canonical_remote_path)
}

/// The active machine's name, for page titles.
pub fn machine_name(cx: &App) -> Option<String> {
  match &model(cx).read(cx).connection.as_ref()?.kind {
    ConnectionKind::Managed => Some("Cloud machine".into()),
    ConnectionKind::UserManaged { id } => store::user_managed_endpoints()
      .into_iter()
      .find(|e| &e.id == id)
      .map(|e| e.display_name),
  }
}

/// Back to the machine list. Every machine stays connected.
pub fn show_machines(cx: &mut App) {
  backend::get().set_active(None);
  store::save_session_snapshot(None);
  model(cx).update(cx, |m, cx| {
    m.connection = None;
    cx.notify();
  });
  nav::home(cx);
}

/// Removes a saved SSH host and drops its connection and terminals.
pub fn forget_host(id: &str, cx: &mut App) {
  store::remove_user_managed_endpoint(id);
  remote::forget_reads();
  if let Some(connection) = backend::get().remove_connection(id) {
    backend::spawn_detached(async move {
      let backend = backend::get();
      backend.pool.mark_dead(&connection.endpoint).await;
      backend
        .ptys
        .close_all_for_endpoint(&connection.endpoint.id)
        .await;
    });
  }
  let id = id.to_string();
  model(cx).update(cx, |m, cx| {
    m.machines.remove(&id);
    if m.connection.as_ref().is_some_and(|c| c.machine() == id) {
      m.connection = None;
    }
    cx.notify();
  });
}

pub fn reauthenticate(cx: &mut App) {
  let model = model(cx);
  model.update(cx, |m, cx| {
    m.set_status(
      MANAGED_MACHINE,
      MachineStatus::Connecting("Reauthenticating"),
    );
    cx.notify();
  });
  let result = backend::spawn(managed::reauthenticate());
  cx.spawn(async move |cx| {
    let Ok(result) = result.await else { return };
    model.update(cx, |m, cx| {
      match result {
        Ok(()) => {
          m.set_status(MANAGED_MACHINE, MachineStatus::Ready);
          if m
            .connection
            .as_ref()
            .is_some_and(|c| c.kind == ConnectionKind::Managed)
          {
            m.connection = backend::get().connection_to(MANAGED_MACHINE);
          }
          m.epoch += 1;
        }
        Err(error) => m.set_status(MANAGED_MACHINE, MachineStatus::Failed(error)),
      }
      cx.notify();
    });
  })
  .detach();
}

/// How long the pooled connection gets to answer on return to the
/// foreground before it counts as dead.
const RESUME_PROBE: std::time::Duration = std::time::Duration::from_secs(3);

/// On return to the foreground: check the pooled connection (a silent one is
/// dropped, so the refetch reconnects at once instead of waiting out a channel
/// open), have every screen refetch (PRD mobile acceptance criterion 7), and
/// rebuild a stale Treq Cloud connection. The staleness check runs alongside
/// the refetch; the rebuild refetches again.
fn resume(cx: &mut App) {
  let model = model(cx);
  let probe = backend::get().endpoint().ok().map(|endpoint| {
    backend::spawn(async move { backend::get().pool.probe(&endpoint, RESUME_PROBE).await })
  });
  // The Treq Cloud machine may have slept, or its certificate lapsed, while
  // the app was in the background. Only a ready one is rechecked.
  let managed = model.read(cx).machine(MANAGED_MACHINE).status == MachineStatus::Ready;
  let stale = managed.then(|| backend::spawn(managed::is_stale()));
  cx.spawn(async move |cx| {
    if let Some(probe) = probe {
      probe.await.ignore_closed();
    }
    model.update(cx, |m, cx| {
      m.epoch += 1;
      cx.notify();
    });
    if let Some(stale) = stale {
      if let Ok(true) = stale.await {
        cx.update(|cx| connect(Machine::Managed, false, cx));
      }
    }
  })
  .detach();
}

/// Restores the Supabase session, then the last endpoint, repository and
/// workspace. Each screen refetches from the host.
fn restore(cx: &mut Context<Root>) {
  let model = model(cx);
  model.update(cx, |m, _| m.restoring = true);
  let restored = backend::spawn(async { backend::get().auth.restore().await });
  cx.spawn(async move |_, cx| {
    let snapshot = store::session_snapshot();
    // Only Treq Cloud needs the session: an SSH host connects without
    // waiting for its refresh, a network round trip.
    if !matches!(
      snapshot.as_ref().map(|s| &s.endpoint),
      Some(store::SessionEndpoint::UserManaged { .. })
    ) {
      restored.await.ignore_closed();
    }
    let Some(snapshot) = snapshot else {
      model.update(cx, |m, _| m.restoring = false);
      return;
    };
    let machine = cx.update(|cx| match &snapshot.endpoint {
      store::SessionEndpoint::Managed => model.read(cx).user.is_some().then_some(Machine::Managed),
      store::SessionEndpoint::UserManaged { id } => store::user_managed_endpoints()
        .into_iter()
        .find(|e| &e.id == id)
        .map(Machine::Host),
    });
    let Some(machine) = machine else {
      model.update(cx, |m, _| m.restoring = false);
      return;
    };
    let id = machine.id().to_string();
    // Woken when the connect finishes, rather than polling for it.
    let (done_tx, done_rx) = futures::channel::oneshot::channel();
    let mut done_tx = Some(done_tx);
    let _finished = cx.update(|cx| {
      connect(machine, true, cx);
      cx.observe(&model, move |model, cx| {
        if !model.read(cx).machine(&id).status.is_connecting() {
          if let Some(done) = done_tx.take() {
            done.send(()).ignore_closed();
          }
        }
      })
    });
    done_rx.await.ignore_closed();
    if cx.update(|cx| model.read(cx).connection.is_some()) {
      cx.update(|cx| screens::restore_pages(&snapshot, cx));
    }
    model.update(cx, |m, _| m.restoring = false);
  })
  .detach();
}

impl Root {
  /// Saves where the user is, so a relaunch can return there.
  fn save_snapshot(&self, cx: &App) {
    let model = self.model.read(cx);
    if model.restoring {
      return;
    }
    let Some(connection) = &model.connection else {
      return;
    };
    let endpoint = match &connection.kind {
      ConnectionKind::Managed => store::SessionEndpoint::Managed,
      ConnectionKind::UserManaged { id } => store::SessionEndpoint::UserManaged { id: id.clone() },
    };
    let location = self
      .stack
      .last()
      .map(|p| p.location.clone())
      .unwrap_or_default();
    store::save_session_snapshot(Some(&store::SessionSnapshot {
      endpoint,
      repo_path: location.repo,
      workspace: location.workspace,
    }));
  }
}

#[cfg(test)]
mod tests {
  use super::{is_landing, panes};
  use gpui_kit::px;

  #[test]
  fn phones_show_one_page() {
    assert_eq!(panes(px(412.), 1), 1);
    assert_eq!(panes(px(412.), 3), 1);
    // A phone in landscape is still too narrow for a list beside a page.
    assert_eq!(panes(px(767.), 2), 1);
  }

  #[test]
  fn wide_screens_show_the_page_beside_the_one_under_it() {
    assert_eq!(panes(px(768.), 1), 1);
    assert_eq!(panes(px(768.), 2), 2);
    assert_eq!(panes(px(1366.), 4), 2);
  }

  #[test]
  fn landing_only_when_signed_out_and_unconnected() {
    assert!(is_landing(None, false));
    assert!(!is_landing(Some("a@b.c"), false));
    assert!(!is_landing(None, true));
    assert!(!is_landing(Some("a@b.c"), true));
  }
}
