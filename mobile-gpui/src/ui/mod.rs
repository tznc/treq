//! The GPUI interface: one navigation stack of full-screen pages.
//!
//! [`Root`] owns the stack and the app-wide [`AppModel`] (sign-in and
//! connection state). Screens push and pop pages through [`nav`].

mod home;
mod load;
mod screens;
mod terminal_view;
pub mod theme;
mod widgets;

use futures::{channel::mpsc::UnboundedReceiver, StreamExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, IconName, Sizable, Theme,
  },
  *,
};

use crate::{
  auth,
  backend::{self, AppEvent, Connection, ConnectionKind},
  managed, store,
};

pub use load::Load;

/// The signed-out, unconnected home is a landing page. Once signed in or
/// connected (even to a saved SSH host) the regular home takes over.
fn is_landing(user: Option<&str>, connected: bool) -> bool {
  user.is_none() && !connected
}

/// App-wide state the screens observe.
#[derive(Default)]
pub struct AppModel {
  pub user: Option<String>,
  pub auth_busy: bool,
  pub auth_error: Option<String>,
  /// The connected endpoint, if any.
  pub connection: Option<Connection>,
  /// The step text while connecting.
  pub connecting: Option<&'static str>,
  pub connect_error: Option<String>,
  /// Why the connected endpoint is cut off, if it is.
  pub cutoff: Option<String>,
  /// Bumped when every screen should refetch (reconnect, resume).
  pub epoch: usize,
  /// Set while the launch snapshot is being restored.
  restoring: bool,
}

/// Where a page is, for the restore snapshot.
#[derive(Clone, Default, PartialEq)]
pub struct Location {
  pub repo: Option<String>,
  pub workspace: Option<store::SnapshotWorkspace>,
}

struct Page {
  title: SharedString,
  view: AnyView,
  location: Location,
}

pub struct Root {
  model: Entity<AppModel>,
  stack: Vec<Page>,
}

struct Nav {
  root: WeakEntity<Root>,
  window: AnyWindowHandle,
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

  /// Pushes the page `build` creates. `location` names the repository
  /// and workspace it shows, for the restore snapshot.
  pub fn push<V: Render>(
    title: impl Into<SharedString>,
    location: Location,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
  ) {
    let title = title.into();
    let Nav { root, window } = cx.global::<Nav>();
    let (root, window) = (root.clone(), *window);
    // Deferred: callers are usually inside another entity's update.
    cx.defer(move |cx| {
      window
        .update(cx, |_, window, cx| {
          let view = build(window, cx);
          root.update(cx, |root, cx| {
            root.stack.push(Page {
              title,
              view: view.into(),
              location,
            });
            root.stack_changed(cx);
          })
        })
        .ok();
    });
  }

  pub fn pop(cx: &mut App) {
    update(cx, |root, cx| {
      if root.stack.len() > 1 {
        root.stack.pop();
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
    cx.observe(&model, |_, _, cx| cx.notify()).detach();
    cx.observe_window_appearance(window, |_, window, cx| {
      Theme::sync_system_appearance(Some(window), cx);
    })
    .detach();

    let weak = cx.weak_entity();
    tauri_plugin_gpui::on_back(move |cx| {
      weak
        .update(cx, |root, cx| {
          if root.stack.len() > 1 {
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
        title: "Treq".into(),
        view: home.into(),
        location: Location::default(),
      }],
    };
    restore(cx);
    root
  }

  fn stack_changed(&mut self, cx: &mut Context<Self>) {
    tauri_plugin_gpui::set_back_enabled(self.stack.len() > 1);
    self.save_snapshot(cx);
    cx.notify();
  }

  fn handle(&mut self, event: AppEvent, _window: &mut Window, cx: &mut Context<Self>) {
    match event {
      AppEvent::Auth(user) => self.model.update(cx, |m, cx| {
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
      } => self.model.update(cx, |m, cx| {
        if m
          .connection
          .as_ref()
          .is_some_and(|c| c.endpoint.id == endpoint_id)
        {
          m.cutoff = reason;
          cx.notify();
        }
      }),
      AppEvent::Resumed => resume(cx),
    }
  }
}

impl Render for Root {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // Keep clear of the status bar, navigation bar and soft keyboard.
    let visible = window.fully_visible_bounds();
    let viewport = window.viewport_size();
    let page = self.stack.last().expect("the stack keeps its root page");
    let can_go_back = self.stack.len() > 1;
    let model = self.model.read(cx);
    // The landing page brings its own branding, so it drops the title bar.
    let landing = !can_go_back && is_landing(model.user.as_deref(), model.connection.is_some());
    let blocked = model.cutoff.clone();
    let managed = model
      .connection
      .as_ref()
      .is_some_and(|c| c.kind == ConnectionKind::Managed);
    let theme = cx.theme();

    v_flex()
      .size_full()
      .bg(theme.background)
      .text_color(theme.foreground)
      .pt(visible.origin.y)
      .pl(visible.origin.x)
      .pr(viewport.width - visible.right())
      .pb(viewport.height - visible.bottom())
      .when(!landing, |root| {
        root.child(
          h_flex()
            .h(px(56.))
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .when(can_go_back, |bar| {
              bar.child(
                Button::new("back")
                  .ghost()
                  .large()
                  .icon(IconName::ArrowLeft)
                  .on_click(|_, _, cx| nav::pop(cx)),
              )
            })
            .child(
              div()
                .flex_1()
                .min_w_0()
                .px_2()
                .truncate()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(page.title.clone()),
            ),
        )
      })
      .child(div().flex_1().min_h_0().child(page.view.clone()))
      .when_some(blocked, |root, reason| {
        root.child(cutoff_overlay(reason, managed, cx))
      })
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
                .on_click(|_, _, cx| reauthenticate(cx)),
            )
          })
          .child(
            Button::new("switch-host")
              .outline()
              .large()
              .label("Switch host")
              .on_click(|_, _, cx| disconnect(cx)),
          )
          .into_any_element(),
      ]),
    )
}

// Actions on the app model, shared by several screens.

pub fn sign_in(cx: &mut App) {
  use tauri_plugin_opener::OpenerExt;
  let result = backend::get()
    .app
    .opener()
    .open_url(auth::sign_in_url(), None::<&str>);
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
      .cut_off_managed(treq_lib::core::remote_ssh_transport::CutoffReason::SessionEnded)
      .await;
    for endpoint in ended {
      backend.ptys.close_all_for_endpoint(&endpoint).await;
    }
    backend.pool.set_relay_access_token(None);
    backend.auth.sign_out().await;
  });
  cx.spawn(async move |cx| {
    let _ = done.await;
    model.update(cx, |m, cx| {
      m.auth_busy = false;
      if m
        .connection
        .as_ref()
        .is_some_and(|c| c.kind == ConnectionKind::Managed)
      {
        m.connection = None;
        m.cutoff = None;
      }
      cx.notify();
    });
    cx.update(nav::home);
  })
  .detach();
}

/// Connects to the managed instance, then opens the repository picker.
pub fn connect_managed(cx: &mut App) {
  connect(cx, None)
}

pub fn connect_user_managed(endpoint: store::UserManagedEndpoint, cx: &mut App) {
  connect(cx, Some(endpoint))
}

fn connect(cx: &mut App, user_managed: Option<store::UserManagedEndpoint>) {
  let model = model(cx);
  model.update(cx, |m, cx| {
    m.connecting = Some(if user_managed.is_some() {
      "Connecting"
    } else {
      "Checking the managed instance"
    });
    m.connect_error = None;
    cx.notify();
  });
  let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<&'static str>();
  let result = backend::spawn(async move {
    match user_managed {
      Some(record) => {
        managed::stop();
        let connection = Connection {
          endpoint: record.to_endpoint(),
          kind: ConnectionKind::UserManaged {
            id: record.id.clone(),
          },
        };
        backend::get().set_connection(Some(connection.clone()));
        Ok(connection)
      }
      None => {
        managed::connect(move |step| {
          let _ = progress_tx.unbounded_send(step);
        })
        .await
      }
    }
  });
  let progress_model = model.clone();
  cx.spawn(async move |cx| {
    while let Some(step) = progress_rx.next().await {
      progress_model.update(cx, |m, cx| {
        m.connecting = Some(step);
        cx.notify();
      });
    }
  })
  .detach();
  cx.spawn(async move |cx| {
    let Ok(result) = result.await else { return };
    let connected = result.is_ok();
    model.update(cx, |m, cx| {
      m.connecting = None;
      match result {
        Ok(connection) => {
          m.connection = Some(connection);
          m.cutoff = None;
          m.epoch += 1;
        }
        Err(error) => m.connect_error = Some(error),
      }
      cx.notify();
    });
    if connected {
      cx.update(|cx| {
        let restoring = model.read(cx).restoring;
        if !restoring {
          open_repositories(cx);
        }
      });
    }
  })
  .detach();
}

fn open_repositories(cx: &mut App) {
  nav::home(cx);
  screens::repos::open(cx);
}

pub fn disconnect(cx: &mut App) {
  managed::stop();
  backend::get().set_connection(None);
  store::save_session_snapshot(None);
  model(cx).update(cx, |m, cx| {
    m.connection = None;
    m.cutoff = None;
    m.connect_error = None;
    cx.notify();
  });
  nav::home(cx);
}

pub fn reauthenticate(cx: &mut App) {
  let model = model(cx);
  model.update(cx, |m, cx| {
    m.connecting = Some("Reauthenticating");
    cx.notify();
  });
  let result = backend::spawn(managed::reauthenticate());
  cx.spawn(async move |cx| {
    let Ok(result) = result.await else { return };
    model.update(cx, |m, cx| {
      m.connecting = None;
      match result {
        Ok(()) => {
          m.cutoff = None;
          m.connection = backend::get().connection();
          m.epoch += 1;
        }
        Err(error) => m.connect_error = Some(error),
      }
      cx.notify();
    });
  })
  .detach();
}

/// On return to the foreground: rebuild a stale managed connection, then
/// have every screen refetch (PRD mobile acceptance criterion 7).
fn resume(cx: &mut App) {
  let model = model(cx);
  let managed = model
    .read(cx)
    .connection
    .as_ref()
    .is_some_and(|c| c.kind == ConnectionKind::Managed);
  let busy = model.read(cx).connecting.is_some() || model.read(cx).cutoff.is_some();
  if managed && !busy {
    let stale = backend::spawn(managed::is_stale());
    cx.spawn(async move |cx| {
      if let Ok(true) = stale.await {
        cx.update(connect_managed);
      } else {
        model.update(cx, |m, cx| {
          m.epoch += 1;
          cx.notify();
        });
      }
    })
    .detach();
  } else {
    model.update(cx, |m, cx| {
      m.epoch += 1;
      cx.notify();
    });
  }
}

/// Restores the Supabase session, then the last endpoint, repository and
/// workspace. Each screen refetches from the host.
fn restore(cx: &mut Context<Root>) {
  let model = model(cx);
  model.update(cx, |m, _| m.restoring = true);
  let restored = backend::spawn(async { backend::get().auth.restore().await });
  cx.spawn(async move |_, cx| {
    let _ = restored.await;
    let snapshot = store::session_snapshot();
    let Some(snapshot) = snapshot else {
      model.update(cx, |m, _| m.restoring = false);
      return;
    };
    let connect_now = cx.update(|cx| match &snapshot.endpoint {
      store::SessionEndpoint::Managed => {
        let signed_in = model.read(cx).user.is_some();
        if signed_in {
          connect_managed(cx);
        }
        signed_in
      }
      store::SessionEndpoint::UserManaged { id } => {
        match store::user_managed_endpoints()
          .into_iter()
          .find(|e| &e.id == id)
        {
          Some(record) => {
            connect_user_managed(record, cx);
            true
          }
          None => false,
        }
      }
    });
    if !connect_now {
      model.update(cx, |m, _| m.restoring = false);
      return;
    }
    // Wait for the connect to finish.
    loop {
      let state = cx.update(|cx| {
        let m = model.read(cx);
        (m.connecting.is_some(), m.connection.is_some())
      });
      match state {
        (true, _) => {
          cx.background_executor()
            .timer(std::time::Duration::from_millis(200))
            .await
        }
        (false, connected) => {
          if connected {
            cx.update(|cx| screens::restore_pages(&snapshot, cx));
          }
          break;
        }
      }
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
  use super::is_landing;

  #[test]
  fn landing_only_when_signed_out_and_unconnected() {
    assert!(is_landing(None, false));
    assert!(!is_landing(Some("a@b.c"), false));
    assert!(!is_landing(None, true));
    assert!(!is_landing(Some("a@b.c"), true));
  }
}
