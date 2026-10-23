//! The remote repository dialogs Dashboard opens over any screen:
//! `RemoteSetupDialog` (the Treq-managed cloud workspace and "Your own VM"),
//! `RemoteTerminalDialog` and `RemoteAmbiguousMutationDialog`.
//!
//! GPUI reads what it can from the backend itself: the `~/.ssh` keys, the
//! host key a machine presents, the `pty-remote` sessions running on an
//! endpoint. What lives in React stays there and crosses as route fields
//! and actions: the Supabase-backed cloud workspace (`cloud`), registering
//! an endpoint, opening a terminal session, and the mutation feedback store.
//!
//! The dialogs cover the whole window, sidebar included, as `ui/dialog.tsx`
//! did.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use gpui_kit::component::{
  input::{InputEvent, InputState},
  select::{Select, SelectEvent, SelectState},
};
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, anchored, canvas, deferred, div, point, prelude::*, px, AnyElement, Bounds, Context,
  Entity, Focusable as _, FontWeight, MouseButton, Pixels, SharedString, Subscription, Window,
};

use super::{
  account_data::{self, CloudAction},
  commits::{dialog_description, dialog_panel, dialog_title},
  remote_data::{default_key, RemoteHost},
  settings::account::{cloud_card, cloud_card_element, cloud_delete_dialog_element, ClickFn},
  ActionSink, ScreenAction,
};
use crate::{
  core::{
    pty_remote_supervisor::PtySessionInfo, remote_control_plane::SshEndpoint,
    remote_local_keys::LocalSshIdentity,
  },
  gpui_shell::{
    form::SelectOption,
    native,
    popup::PopupSink,
    theme::{self, Palette},
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, IconButton, Size, TextSize, Variant},
  },
};

/// What React sends of the dialogs: whichever are open.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Route {
  pub setup: Option<SetupRoute>,
  pub terminal: Option<TerminalRoute>,
  /// Why the last remote mutation could not be verified, while it is open.
  pub ambiguous: Option<String>,
}

impl Route {
  /// Whether any dialog shows.
  pub fn is_open(&self) -> bool {
    self.setup.is_some() || self.terminal.is_some() || self.reason().is_some()
  }

  fn reason(&self) -> Option<&str> {
    self
      .ambiguous
      .as_deref()
      .filter(|reason| !reason.is_empty())
  }
}

/// `RemoteSetupDialogProps`, less the handlers.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SetupRoute {
  /// Whether the "Your own VM" path is offered (the `remoteSsh` preview).
  pub allow_user_managed: bool,
  /// The screen to open on instead of the choice: `managed` (the machines
  /// page's cloud row) or `ownVm` (its SSH Hosts +).
  pub screen: Option<String>,
  /// The window is registering the endpoint the form confirmed.
  pub registering: bool,
}

/// `RemoteTerminalDialogProps`, less the handlers.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRoute {
  pub endpoint: SshEndpoint,
  pub repository_id: String,
  pub workspace_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
  Choice,
  Managed,
  UserManaged,
}

/// The "Your own VM" form (`RemoteUserManagedSetupPanel`), which React
/// mounts only while that screen shows.
struct Form {
  display_name: Entity<InputState>,
  hostname: Entity<InputState>,
  port: Entity<InputState>,
  username: Entity<InputState>,
  alias: Entity<InputState>,
  key: Entity<SelectState<Vec<SelectOption>>>,
  identities: Vec<LocalSshIdentity>,
  /// The `~/.ssh/config` aliases the hostname and alias fields suggest.
  aliases: Vec<String>,
  key_reference: String,
  alias_mode: bool,
  /// The fingerprint being confirmed, while the trust box shows.
  trust: Option<String>,
  probing: bool,
  probe_error: String,
  resolving: bool,
  resolve_error: String,
  /// The confirmed values went to the window, which is registering them.
  submitting: bool,
  _subscriptions: Vec<Subscription>,
}

struct Setup {
  mode: Mode,
  form: Option<Form>,
  /// The cloud workspace's delete confirmation.
  confirming_delete: bool,
}

/// The sessions the terminal dialog found.
#[derive(Default)]
struct Sessions {
  /// Bumped per request, so a stale answer is dropped.
  request: u64,
  /// `None` while loading.
  list: Option<Vec<PtySessionInfo>>,
  error: Option<String>,
}

pub struct RemoteView {
  host: Arc<dyn RemoteHost>,
  /// React's handlers.
  react: ActionSink,
  route: Route,
  dark: bool,
  setup: Option<Setup>,
  sessions: Sessions,
  tips: Tooltips,
  /// Where the suggestion fields were last drawn, in window coordinates.
  bounds: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
}

impl HasTooltips for RemoteView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

/// A dialog's place in the window.
struct Frame {
  id: &'static str,
  /// `max-w-*`, in pixels.
  max_width: f32,
  /// Later dialogs paint over earlier ones.
  priority: usize,
  /// The press its backdrop sends React.
  dismiss: &'static str,
}

/// The text of a `<label>`'s caption: `text-sm font-medium`.
fn caption(text: &'static str) -> Div {
  ui::text(div(), TextSize::Sm)
    .font_weight(FontWeight::MEDIUM)
    .child(text)
}

type Div = gpui::Div;

/// `text-xs text-muted-foreground`.
fn hint(text: impl Into<SharedString>, p: &Palette) -> Div {
  ui::text(div(), TextSize::Xs)
    .text_color(p.muted_foreground)
    .child(text.into())
}

/// `DialogHeader`: a title over its description.
fn header(title: impl IntoElement, description: impl Into<SharedString>, p: &Palette) -> Div {
  div()
    .flex()
    .flex_col()
    .gap_1p5()
    .child(dialog_title(title))
    .child(dialog_description(description, p))
}

/// A plain `<input>`: `rounded-md border border-border/60 bg-background
/// px-2 py-1.5`.
fn text_field(id: &'static str, state: &Entity<InputState>, p: &Palette) -> impl IntoElement {
  div()
    .id(id)
    .debug_selector(|| id.into())
    .w_full()
    .h(zpx(34.))
    .px_2()
    .flex()
    .items_center()
    .rounded(radius::MD)
    .border_1()
    .border_color(p.border.opacity(0.6))
    .bg(p.background)
    .child(ui::input(state).appearance(false).bordered(false).flex_1())
}

/// A `<label className="flex flex-col gap-1">`: a caption over a control.
fn labelled(text: &'static str, control: impl IntoElement) -> Div {
  div()
    .flex()
    .flex_col()
    .gap_1()
    .child(caption(text))
    .child(control)
}

/// Lays `cells` out as `grid-cols-1 sm:grid-cols-2 gap-3`; a cell that is
/// `true` spans both columns.
fn grid(cells: Vec<(AnyElement, bool)>, two_columns: bool) -> Div {
  let mut rows: Vec<Vec<AnyElement>> = Vec::new();
  let mut open_row: Vec<AnyElement> = Vec::new();
  for (cell, span) in cells {
    if span || !two_columns {
      if !open_row.is_empty() {
        rows.push(std::mem::take(&mut open_row));
      }
      rows.push(vec![cell]);
    } else {
      open_row.push(cell);
      if open_row.len() == 2 {
        rows.push(std::mem::take(&mut open_row));
      }
    }
  }
  if !open_row.is_empty() {
    rows.push(open_row);
  }
  div()
    .flex()
    .flex_col()
    .gap_3()
    .children(rows.into_iter().map(|row| {
      let alone = row.len() == 1;
      div()
        .flex()
        .gap_3()
        .children(row.into_iter().map(move |cell| {
          div()
            .min_w_0()
            .when(alone, |this| this.w_full())
            .when(!alone, |this| this.flex_1().w_0())
            .child(cell)
        }))
    }))
}

/// `mt-1 text-muted-foreground`'s words, each in `font-mono` when marked, so
/// the paragraph wraps as text does.
fn words(parts: &[(&str, bool)], p: &Palette) -> Div {
  let mut paragraph = div()
    .flex()
    .flex_wrap()
    .gap_x_1()
    .text_color(p.muted_foreground);
  for (text, mono) in parts {
    for word in text.split_whitespace() {
      paragraph = paragraph.child(
        div()
          .when(*mono, |this| this.font_family(theme::MONO_FONT))
          .child(word.to_string()),
      );
    }
  }
  paragraph
}

/// `Number(port) || 22`.
fn port_number(port: &str) -> u16 {
  port
    .trim()
    .parse::<u16>()
    .ok()
    .filter(|p| *p != 0)
    .unwrap_or(22)
}

impl RemoteView {
  pub fn new(host: Arc<dyn RemoteHost>, react: ActionSink, popups: PopupSink, dark: bool) -> Self {
    Self {
      host,
      react,
      route: Route::default(),
      dark,
      setup: None,
      sessions: Sessions::default(),
      tips: Tooltips::new(popups, dark),
      bounds: Rc::default(),
    }
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route == route && self.dark == dark {
      return;
    }
    self.dark = dark;
    self.tips.dark = dark;
    let before = std::mem::replace(&mut self.route, route);
    let now = self.route.clone();

    match (&now.setup, self.setup.is_some()) {
      (None, _) => self.setup = None,
      (Some(setup), false) => {
        // `handleOpenRemoteSetup` reads the cloud workspace afresh.
        self.host.cloud_action(CloudAction::Refresh);
        self.setup = Some(Setup {
          mode: Self::first_mode(setup),
          form: None,
          confirming_delete: false,
        });
      }
      (Some(setup), true) => {
        let allowed = before.setup.as_ref().map(|s| s.allow_user_managed);
        if allowed.is_some_and(|allowed| allowed != setup.allow_user_managed) {
          // The dialog reopens on its first screen when that changes.
          let mode = Self::first_mode(setup);
          if let Some(state) = self.setup.as_mut() {
            state.mode = mode;
            state.form = None;
          }
        }
        // React finished registering: the form is usable again.
        let was = before.setup.as_ref().is_some_and(|s| s.registering);
        if was && !setup.registering {
          if let Some(form) = self.setup.as_mut().and_then(|s| s.form.as_mut()) {
            form.submitting = false;
          }
        }
      }
    }

    if before.terminal != now.terminal {
      self.load_sessions(cx);
    }
    cx.notify();
  }

  fn first_mode(setup: &SetupRoute) -> Mode {
    match setup.screen.as_deref() {
      Some("managed") => return Mode::Managed,
      Some("ownVm") if setup.allow_user_managed => return Mode::UserManaged,
      _ => {}
    }
    if setup.allow_user_managed {
      Mode::Choice
    } else {
      Mode::Managed
    }
  }

  fn send(&self, action: ScreenAction, cx: &mut gpui::App) {
    (self.react)(action, cx);
  }

  fn press_react(&self, id: &str, cx: &mut gpui::App) {
    self.send(ScreenAction::Press { id: id.to_string() }, cx);
  }

  fn field_react(&self, field: &str, value: serde_json::Value, cx: &mut gpui::App) {
    self.send(
      ScreenAction::Field {
        field: field.to_string(),
        value,
      },
      cx,
    );
  }

  /// Lists the persistent sessions of the endpoint the terminal dialog
  /// opened for (`remotePtyListPersistentSessions`).
  fn load_sessions(&mut self, cx: &mut Context<Self>) {
    self.sessions.request += 1;
    self.sessions.list = None;
    self.sessions.error = None;
    let Some(terminal) = self.route.terminal.clone() else {
      return;
    };
    let (request, host) = (self.sessions.request, self.host.clone());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move {
          host.persistent_sessions(
            &terminal.endpoint,
            &terminal.repository_id,
            &terminal.workspace_id,
          )
        })
        .await;
      this
        .update(cx, |view, cx| {
          if view.sessions.request != request {
            return;
          }
          match result {
            Ok(list) => view.sessions.list = Some(list),
            Err(error) => {
              view.sessions.error = Some(error);
              view.sessions.list = Some(Vec::new());
            }
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
    let Some(setup) = self.setup.as_mut() else {
      return;
    };
    setup.mode = mode;
    setup.form = None;
    if mode == Mode::UserManaged {
      let form = self.new_form(window, cx);
      if let Some(setup) = self.setup.as_mut() {
        setup.form = Some(form);
      }
      self.load_identities(cx);
    }
    cx.notify();
  }

  fn new_form(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Form {
    let mut subscriptions = Vec::new();
    let mut input = |placeholder: &str, value: &str, subscriptions: &mut Vec<Subscription>| {
      let state = cx.new(|cx| {
        InputState::new(window, cx)
          .placeholder(placeholder.to_string())
          .default_value(value.to_string())
      });
      // The Continue button follows what is typed, and the suggestions
      // follow the focus.
      subscriptions.push(cx.subscribe(&state, |_, _, event: &InputEvent, cx| {
        if matches!(
          event,
          InputEvent::Change | InputEvent::Focus | InputEvent::Blur
        ) {
          cx.notify();
        }
      }));
      state
    };
    let display_name = input("", "", &mut subscriptions);
    let hostname = input("e.g. 203.0.113.4", "", &mut subscriptions);
    let port = input("", "22", &mut subscriptions);
    let username = input("", "", &mut subscriptions);
    let alias = input("", "", &mut subscriptions);
    let key = cx.new(|cx| SelectState::new(Vec::<SelectOption>::new(), None, window, cx));
    subscriptions.push(cx.subscribe(
      &key,
      |view, _, event: &SelectEvent<Vec<SelectOption>>, cx| {
        let SelectEvent::Confirm(Some(value)) = event else {
          return;
        };
        if let Some(form) = view.setup.as_mut().and_then(|s| s.form.as_mut()) {
          form.key_reference = value.clone();
        }
        cx.notify();
      },
    ));
    Form {
      display_name,
      hostname,
      port,
      username,
      alias,
      key,
      identities: Vec::new(),
      aliases: Vec::new(),
      key_reference: String::new(),
      alias_mode: false,
      trust: None,
      probing: false,
      probe_error: String::new(),
      resolving: false,
      resolve_error: String::new(),
      submitting: false,
      _subscriptions: subscriptions,
    }
  }

  /// Reads the `~/.ssh` keys, and picks the default one.
  fn load_identities(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let (identities, aliases) = cx
        .background_spawn(async move { (host.ssh_identities(), host.ssh_aliases()) })
        .await;
      this
        .update_in(cx, |view, window, cx| {
          let Some(form) = view.setup.as_mut().and_then(|s| s.form.as_mut()) else {
            return;
          };
          if form.key_reference.is_empty() {
            form.key_reference = default_key(&identities).unwrap_or_default();
          }
          let options: Vec<SelectOption> = identities
            .iter()
            .map(|identity| SelectOption {
              value: identity.reference.clone(),
              label: format!("{} ({})", identity.label, identity.algorithm),
            })
            .collect();
          let selected = form.key_reference.clone();
          form.identities = identities;
          form.aliases = aliases;
          form.key.update(cx, |key, cx| {
            key.set_items(options, window, cx);
            if !selected.is_empty() {
              key.set_selected_value(&selected, window, cx);
            }
          });
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn form(&mut self) -> Option<&mut Form> {
    self.setup.as_mut().and_then(|s| s.form.as_mut())
  }

  /// `probeHostKey`: reads the fingerprint the host presents, then asks to
  /// trust it.
  fn probe_host_key(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    let Some(form) = self.form() else {
      return;
    };
    form.probing = true;
    form.probe_error.clear();
    let hostname = form.hostname.read(cx).value().trim().to_string();
    let port = port_number(&form.port.read(cx).value());
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move { host.probe_host_key(&hostname, port) })
        .await;
      this
        .update(cx, |view, cx| {
          if let Some(form) = view.form() {
            form.probing = false;
            match result {
              Ok(fingerprint) => form.trust = Some(fingerprint),
              Err(error) => form.probe_error = error,
            }
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
    cx.notify();
  }

  /// `resolveAlias`: fills the hostname, port and username from
  /// `~/.ssh/config`, never trusting anything.
  fn resolve_alias(&mut self, cx: &mut Context<Self>) {
    let host = self.host.clone();
    let Some(form) = self.form() else {
      return;
    };
    let alias = form.alias.read(cx).value().trim().to_string();
    if alias.is_empty() {
      return;
    }
    form.resolving = true;
    form.resolve_error.clear();
    cx.spawn(async move |this, cx| {
      let result = cx
        .background_spawn(async move { host.resolve_alias(&alias) })
        .await;
      this
        .update_in(cx, |view, window, cx| {
          if let Some(form) = view.form() {
            form.resolving = false;
            match result {
              Ok(resolved) => {
                let (hostname, port, username) = (
                  form.hostname.clone(),
                  form.port.clone(),
                  form.username.clone(),
                );
                hostname.update(cx, |input, cx| {
                  input.set_value(resolved.hostname, window, cx)
                });
                port.update(cx, |input, cx| {
                  input.set_value(resolved.port.to_string(), window, cx)
                });
                if let Some(name) = resolved.username.filter(|name| !name.is_empty()) {
                  username.update(cx, |input, cx| input.set_value(name, window, cx));
                }
              }
              Err(error) => form.resolve_error = error,
            }
          }
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
    cx.notify();
  }

  /// `submitUserManaged`: hands the confirmed values to React.
  fn register(&mut self, cx: &mut Context<Self>) {
    let Some(form) = self.form() else {
      return;
    };
    let Some(fingerprint) = form.trust.take() else {
      return;
    };
    form.submitting = true;
    let hostname = form.hostname.read(cx).value().trim().to_string();
    let display = form.display_name.read(cx).value().trim().to_string();
    let alias = form.alias.read(cx).value().trim().to_string();
    #[allow(
      clippy::redundant_clone,
      reason = "`hostname` is read again below; json! hides the use"
    )]
    let values = serde_json::json!({
      "display_name": if display.is_empty() { hostname.clone() } else { display },
      "hostname": hostname,
      "port": port_number(&form.port.read(cx).value()),
      "username": form.username.read(cx).value().trim(),
      "host_key_fingerprint": fingerprint.trim(),
      "auth_identity_reference": form.key_reference.trim(),
      "alias": if form.alias_mode && !alias.is_empty() { Some(alias) } else { None },
    });
    self.field_react("remote.registerUserManaged", values, cx);
    cx.notify();
  }

  /// A button of the cloud workspace card or its delete confirmation.
  fn pressed(&mut self, id: &str, cx: &mut Context<Self>) {
    match id {
      "cloud.delete.ask" | "cloud.delete.cancel" | "cloud.delete.confirm" => {
        if let Some(setup) = self.setup.as_mut() {
          setup.confirming_delete = id == "cloud.delete.ask";
        }
        if id == "cloud.delete.confirm" {
          self.host.cloud_action(CloudAction::Delete);
        }
      }
      "cloud.upgrade" => self
        .host
        .open_url(&format!("{}/dashboard", native::web_url())),
      "account.signIn" => self
        .host
        .open_url(&format!("{}/sign-in?source=desktop", native::web_url())),
      "cloud.provision" => self.host.cloud_action(CloudAction::Provision),
      "cloud.wake" => self.host.cloud_action(CloudAction::Wake),
      "cloud.repair" => self.host.cloud_action(CloudAction::Repair),
      // The repository list opens once the endpoint connects.
      "cloud.openRepositories" => self.host.cloud_action(CloudAction::OpenRepositories),
      other => self.press_react(&format!("remote.{other}"), cx),
    }
    cx.notify();
  }

  /// A press handler for the cloud card's buttons.
  fn cloud_press(cx: &Context<Self>) -> impl Fn(&str) -> ClickFn {
    let view = cx.weak_entity();
    move |id| {
      let (view, id) = (view.clone(), id.to_string());
      Box::new(move |_, _, cx| {
        view
          .update(cx, |view, cx| view.pressed(&id, cx))
          .ignore_closed();
      })
    }
  }

  /// `ui/dialog.tsx`: the backdrop over the whole window, which closes the
  /// dialog when clicked, and the panel.
  fn dialog(
    &self,
    frame: Frame,
    window: &Window,
    content: impl IntoElement,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let Frame {
      id,
      max_width,
      priority,
      dismiss,
    } = frame;
    let (backdrop, panel) = dialog_panel(id, max_width, window.viewport_size(), p);
    deferred(
      anchored().position(point(px(0.), px(0.))).child(
        backdrop
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _, _, cx| view.press_react(dismiss, cx)),
          )
          .child(
            ui::text(panel, TextSize::Sm)
              .id(id)
              .font_family(theme::SANS_FONT)
              .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
              .child(content),
          ),
      ),
    )
    .with_priority(priority)
    .into_any_element()
  }

  fn setup_dialog(
    &mut self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let route = self.route.setup.clone()?;
    // Opened straight on "Your own VM": its form, as choosing it makes.
    if self
      .setup
      .as_ref()
      .is_some_and(|s| s.mode == Mode::UserManaged && s.form.is_none())
    {
      self.set_mode(Mode::UserManaged, window, cx);
    }
    let setup = self.setup.as_ref()?;
    let (mode, confirming) = (setup.mode, setup.confirming_delete);
    let content = match mode {
      Mode::Choice => self.choice(window, p, cx).into_any_element(),
      Mode::Managed => self.managed(&route, p, cx).into_any_element(),
      Mode::UserManaged => self.user_managed(window, p, cx).into_any_element(),
    };
    // The own-VM form ends in an action pair, so it has no X.
    let content = div()
      .relative()
      .child(content)
      .when(mode != Mode::UserManaged, |d| {
        d.child(
          div().absolute().top(zpx(-8.)).right(zpx(-8.)).child(
            IconButton::new("remote-setup-close", theme::ICON_X, "Close", p)
              .on_click(cx.listener(|view, _, _, cx| view.press_react("remote.setup.close", cx))),
          ),
        )
      });
    let frame = Frame {
      id: "remote-setup-dialog",
      max_width: 576.,
      priority: 3,
      dismiss: "remote.setup.close",
    };
    let dialog = self.dialog(frame, window, content, p, cx);
    let confirm = (mode == Mode::Managed && confirming).then(|| {
      let press = Self::cloud_press(cx);
      let layer = div()
        .w(window.viewport_size().width)
        .h(window.viewport_size().height)
        .child(cloud_delete_dialog_element(p, &press));
      deferred(anchored().position(point(px(0.), px(0.))).child(layer))
        .with_priority(6)
        .into_any_element()
    });
    Some(
      div()
        .children(Some(dialog))
        .children(confirm)
        .into_any_element(),
    )
  }

  fn choice(&mut self, window: &Window, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    let card = |id: &'static str,
                title: &'static str,
                description: &'static str,
                mode: Mode,
                cx: &mut Context<Self>| {
      let hover_border = p.primary.opacity(0.6);
      let hover_bg = p.muted.opacity(0.4);
      div()
        .id(id)
        .debug_selector(|| id.into())
        .flex_1()
        .min_w_0()
        .cursor_pointer()
        .rounded(radius::LG)
        .border_1()
        .border_color(p.border.opacity(0.6))
        .p_4()
        .hover(move |s| s.border_color(hover_border).bg(hover_bg))
        .child(div().font_weight(FontWeight::MEDIUM).child(title))
        .child(
          div()
            .mt_1()
            .text_color(p.muted_foreground)
            .child(description),
        )
        .on_click(cx.listener(move |view, _, window, cx| view.set_mode(mode, window, cx)))
    };
    let wide = window.viewport_size().width >= crate::gpui_shell::ui::zoomed(640., window);
    div()
      .child(header(
        "Connect a remote repository",
        "Choose how the remote machine is managed.",
        p,
      ))
      .child(
        div()
          .mt_4()
          .flex()
          .gap_3()
          .when(!wide, |this| this.flex_col())
          .child(card(
            "remote-setup-managed",
            "Treq-managed cloud workspace",
            "Treq provisions and maintains one persistent cloud workspace for your account.",
            Mode::Managed,
            cx,
          ))
          .child(card(
            "remote-setup-own-vm",
            "Your own VM",
            "Connect to a machine you already run. You install Treq, JJ, Git, and agents there yourself.",
            Mode::UserManaged,
            cx,
          )),
      )
  }

  fn managed(
    &mut self,
    route: &SetupRoute,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let confirming = self.setup.as_ref().is_some_and(|s| s.confirming_delete);
    let account = account_data::account(cx);
    let press = Self::cloud_press(cx);
    let card = if account.user.is_some() {
      let cloud = account.cloud.unwrap_or_default();
      let card = cloud_card(&cloud, confirming);
      cloud_card_element(&card, p, &self.tips, &press, cx).into_any_element()
    } else {
      Self::sign_in(&account, &press, p).into_any_element()
    };
    div()
      .child(header(
        "Treq-managed cloud workspace",
        "Treq manages the machine lifecycle. Commands run over managed SSH with short-lived certificates.",
        p,
      ))
      .child(div().mt_4().child(card))
      // Opened on this screen from the Overview, there is no choice to go
      // back to.
      .when(route.allow_user_managed && route.screen.is_none(), |this| {
        this.child(
          div().mt_6().child(
            Button::new("remote-setup-back", p)
              .variant(Variant::Ghost)
              .label("Back")
              .on_click(cx.listener(|view, _, window, cx| view.set_mode(Mode::Choice, window, cx))),
          ),
        )
      })
  }

  /// Before the account is signed in: the cloud workspace belongs to an
  /// account, so sign in first.
  fn sign_in(
    account: &super::settings::AccountRoute,
    press: &impl Fn(&str) -> ClickFn,
    p: &Palette,
  ) -> Div {
    let busy = account.loading || account.signing_in;
    div()
      .debug_selector(|| "remote-setup-signed-out".into())
      .flex()
      .flex_col()
      .items_start()
      .gap_3()
      .child(hint(
        "Sign in to Treq to create your cloud workspace. The browser opens the sign-in page and brings you back here.",
        p,
      ))
      .child(
        Button::new("remote-setup-sign-in", p)
          .child(ui::icon(theme::ICON_EXTERNAL_LINK, 16., p.primary_foreground))
          .label(if account.signing_in {
            "Signing in…"
          } else {
            "Sign in with Browser"
          })
          .disabled(busy)
          .on_click(press("account.signIn")),
      )
      .children(account.sign_in_error.clone().map(|error| {
        ui::text(div(), TextSize::Sm)
          .text_color(p.destructive)
          .child(format!("Sign-in failed: {error}"))
      }))
  }

  /// A text field whose `<datalist>` suggests the `~/.ssh/config` aliases
  /// containing what is typed, while it has focus. Picking one only fills
  /// the field.
  fn suggest_field(
    &self,
    id: &'static str,
    state: &Entity<InputState>,
    aliases: &[String],
    window: &Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Div {
    let typed = state.read(cx).value().to_string();
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let needle = typed.to_lowercase();
    let matches: Vec<String> = if focused && !needle.is_empty() {
      aliases
        .iter()
        .filter(|alias| alias.to_lowercase().contains(&needle) && **alias != typed)
        .cloned()
        .collect()
    } else {
      Vec::new()
    };
    let anchor = self.bounds.borrow().get(id).copied();
    let bounds = self.bounds.clone();
    let list = anchor.filter(|_| !matches.is_empty()).map(|anchor| {
      let menu_accent = p.menu_accent;
      let rows = matches.into_iter().enumerate().map(|(i, alias)| {
        let (state, value) = (state.clone(), alias.clone());
        div()
          .id(SharedString::from(format!("{id}-suggestion-{i}")))
          .debug_selector(move || format!("{id}-suggestion-{i}"))
          .px_2()
          .py_1()
          .rounded(radius::SM)
          .cursor_pointer()
          .hover(move |s| s.bg(menu_accent))
          .child(alias)
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, window, cx| {
              state.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
              cx.stop_propagation();
            }),
          )
      });
      deferred(
        anchored()
          .position(point(anchor.left(), anchor.bottom() + px(2.)))
          .child(
            div()
              .w(anchor.size.width)
              .p_1()
              .rounded(radius::MD)
              .border_1()
              .border_color(p.border)
              .bg(p.popover)
              .text_color(p.popover_foreground)
              .shadow_md()
              .occlude()
              .children(rows),
          ),
      )
      .with_priority(7)
    });
    div()
      .relative()
      .w_full()
      .child(text_field(id, state, p))
      .child(
        canvas(
          move |at, _, _| {
            bounds.borrow_mut().insert(id, at);
          },
          |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
      )
      .children(list)
  }

  fn user_managed(
    &mut self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let two_columns = window.viewport_size().width >= crate::gpui_shell::ui::zoomed(640., window);
    // Opened on this form from the Overview: Cancel closes it.
    let direct = self
      .route
      .setup
      .as_ref()
      .is_some_and(|r| r.screen.is_some());
    let Some(form) = self.setup.as_ref().and_then(|s| s.form.as_ref()) else {
      return div();
    };
    let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
    let (hostname, username, port) = (
      value(&form.hostname),
      value(&form.username),
      value(&form.port),
    );
    let key_selected = form
      .identities
      .iter()
      .any(|identity| identity.reference == form.key_reference);
    let valid = !hostname.trim().is_empty() && !username.trim().is_empty() && key_selected;
    let busy = form.submitting;

    let key = div()
      .w_full()
      .child(
        Select::new(&form.key)
          .id("remote-ssh-key")
          .placeholder("Select a key")
          .h(zpx(34.)),
      )
      .into_any_element();
    let key_cell = div()
      .flex()
      .flex_col()
      .gap_1()
      .child(caption("SSH key"))
      .child(key)
      .when(form.identities.is_empty(), |this| {
        this.child(hint(
          "No SSH keys found in ~/.ssh. Create one with ssh-keygen -t ed25519.",
          p,
        ))
      });

    let alias_toggle = div()
      .id("remote-alias-toggle")
      .debug_selector(|| "remote-alias-toggle".into())
      .flex()
      .items_center()
      .gap_2()
      .cursor_pointer()
      .child(ui::Checkbox::new("remote-alias-checkbox", p).checked(form.alias_mode))
      .child(div().child("Use an explicit SSH alias for this endpoint (optional)"))
      .on_click(cx.listener(|view, _, _, cx| {
        if let Some(form) = view.form() {
          form.alias_mode = !form.alias_mode;
        }
        cx.notify();
      }));

    let alias_empty = form.alias.read(cx).value().trim().is_empty();
    let alias_cell = form.alias_mode.then(|| {
      labelled(
        "Alias",
        div()
          .flex()
          .flex_col()
          .gap_1()
          .child(
            div()
              .flex()
              .gap_2()
              .child(div().flex_1().child(self.suggest_field(
                "remote-alias",
                &form.alias,
                &form.aliases,
                window,
                p,
                cx,
              )))
              .child(
                Button::new("remote-alias-autofill", p)
                  .variant(Variant::Outline)
                  .size(Size::Sm)
                  .disabled(alias_empty || form.resolving)
                  .label(if form.resolving {
                    "Resolving..."
                  } else {
                    "Autofill from alias"
                  })
                  .on_click(cx.listener(|view, _, _, cx| view.resolve_alias(cx))),
              ),
          )
          .child(hint(
            "Reads hostname/port/username from ~/.ssh/config for this alias only. This never grants trust - you still confirm the host key the server presents.",
            p,
          ))
          .when(!form.resolve_error.is_empty(), |this| {
            this.child(
              ui::text(div(), TextSize::Xs)
                .text_color(p.destructive)
                .child(form.resolve_error.clone()),
            )
          }),
      )
    });

    let mut cells: Vec<(AnyElement, bool)> = vec![
      (
        labelled(
          "Display name",
          text_field("remote-display-name", &form.display_name, p),
        )
        .into_any_element(),
        false,
      ),
      (
        labelled(
          "Hostname or IP address",
          self.suggest_field(
            "remote-hostname",
            &form.hostname,
            &form.aliases,
            window,
            p,
            cx,
          ),
        )
        .into_any_element(),
        false,
      ),
      (
        labelled("SSH port", text_field("remote-port", &form.port, p)).into_any_element(),
        false,
      ),
      (
        labelled("Username", text_field("remote-username", &form.username, p)).into_any_element(),
        false,
      ),
      (key_cell.into_any_element(), true),
      (alias_toggle.into_any_element(), true),
    ];
    if let Some(alias) = alias_cell {
      cells.push((alias.into_any_element(), true));
    }

    let trust = form.trust.clone().map(|fingerprint| {
      let endpoint = format!("{username}@{hostname}:{port}");
      div()
        .id("remote-trust-confirmation")
        .debug_selector(|| "remote-trust-confirmation".into())
        .mt_4()
        .rounded(radius::MD)
        .border_1()
        .border_color(ui::Tone::Warning.border(p))
        .bg(ui::Tone::Warning.tint(p))
        .p_3()
        .child(div().font_weight(FontWeight::MEDIUM).child("Confirm host trust"))
        .child(div().mt_1().child(words(
          &[
            ("You are about to trust", false),
            (endpoint.as_str(), true),
            ("with host key fingerprint", false),
            (fingerprint.as_str(), true),
            (
              ". Treq will reject this endpoint if the presented key ever changes. Compare it with the output of ssh-keygen -lf on the host before trusting it.",
              false,
            ),
          ],
          p,
        )))
        .child(
          div()
            .mt_3()
            .flex()
            .gap_2()
            .child(
              Button::new("remote-trust-connect", p)
                .size(Size::Sm)
                .disabled(busy)
                .label("Trust and connect")
                .on_click(cx.listener(|view, _, _, cx| view.register(cx))),
            )
            .child(
              Button::new("remote-trust-cancel", p)
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .label("Cancel")
                .on_click(cx.listener(|view, _, _, cx| {
                  if let Some(form) = view.form() {
                    form.trust = None;
                  }
                  cx.notify();
                })),
            ),
        )
    });

    let show_continue = form.trust.is_none();
    let probing = form.probing;
    let probe_error = form.probe_error.clone();
    div()
      .child(header(
        "Your own VM",
        "Enter the connection details exactly - Treq never infers trust from your local SSH configuration.",
        p,
      ))
      .child(div().mt_4().child(grid(cells, two_columns)))
      .children(trust)
      .when(!probe_error.is_empty(), |this| {
        this.child(
          div().mt_4().child(
            ui::Alert::new("remote-probe-error", "Couldn't reach the host", p)
              .severity(ui::Severity::Danger)
              .message(probe_error),
          ),
        )
      })
      .child(
        div()
          .mt_6()
          .flex()
          .justify_between()
          .child(if direct {
            Button::new("remote-setup-cancel", p)
              .variant(Variant::Ghost)
              .label("Cancel")
              .on_click(cx.listener(|view, _, _, cx| view.press_react("remote.setup.close", cx)))
          } else {
            Button::new("remote-setup-back", p)
              .variant(Variant::Ghost)
              .disabled(busy)
              .label("Back")
              .on_click(cx.listener(|view, _, window, cx| view.set_mode(Mode::Choice, window, cx)))
          })
          .when(show_continue, |this| {
            this.child(
              Button::new("remote-continue", p)
                .disabled(!valid || busy || probing)
                .label(if probing {
                  "Checking host key…"
                } else {
                  "Continue"
                })
                .on_click(cx.listener(|view, _, _, cx| view.probe_host_key(cx))),
            )
          }),
      )
  }

  fn terminal_dialog(
    &mut self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let terminal = self.route.terminal.clone()?;
    let red = ui::Tone::Danger.text(p);
    let list = self.sessions.list.clone();
    let loading = list.is_none() && self.sessions.error.is_none();
    let sessions = list.clone().unwrap_or_default();

    let rows = (!sessions.is_empty()).then(|| {
      div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
          ui::text(div(), TextSize::Xs)
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.muted_foreground)
            .child("RUNNING SESSIONS"),
        )
        .children(sessions.iter().enumerate().map(|(i, session)| {
          let label = session.label.clone();
          div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .rounded(radius::MD)
            .border_1()
            .border_color(p.border)
            .px_3()
            .py_2()
            .child(
              div()
                .min_w_0()
                .child(
                  div()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(session.label.clone()),
                )
                .child(
                  ui::text(div(), TextSize::Xs)
                    .text_color(p.muted_foreground)
                    .truncate()
                    .child(if session.running {
                      "running"
                    } else {
                      "stopped"
                    }),
                ),
            )
            .child(
              Button::new(format!("remote-reattach-{i}"), p)
                .size(Size::Sm)
                .disabled(!session.running)
                .label("Reattach")
                .on_click(cx.listener(move |view, _, _, cx| {
                  view.open_session(Some(label.clone()), true, cx)
                })),
            )
        }))
    });

    let content = div()
      .child(
        div()
          .flex()
          .flex_col()
          .gap_1p5()
          .child(dialog_title(
            div()
              .flex()
              .items_center()
              .gap_2()
              .child(ui::icon(
                theme::ICON_TERMINAL_SQUARE,
                16.,
                p.popover_foreground,
              ))
              .child("Remote terminal"),
          ))
          .child(dialog_description(
            format!(
              "Open an interactive shell on {}.",
              terminal.endpoint.hostname
            ),
            p,
          )),
      )
      .child(
        div()
          .mt_2()
          .flex()
          .flex_col()
          .gap_3()
          .when(loading, |this| {
            this.child(
              div()
                .py_4()
                .flex()
                .items_center()
                .gap_2()
                .text_color(p.muted_foreground)
                .child(ui::spinner(
                  "remote-terminal-loading",
                  16.,
                  p.muted_foreground,
                ))
                .child("Checking for running sessions…"),
            )
          })
          .when_some(self.sessions.error.clone(), |this, error| {
            this.child(
              div()
                .debug_selector(|| "remote-terminal-error".into())
                .text_color(red)
                .child(error),
            )
          })
          .children(rows)
          .when(list.is_some(), |this| {
            this.child(
              Button::new("remote-start-new", p)
                .variant(if sessions.is_empty() {
                  Variant::Default
                } else {
                  Variant::Outline
                })
                .label("Start new session")
                .on_click(cx.listener(|view, _, _, cx| view.open_session(None, false, cx))),
            )
          }),
      );
    let frame = Frame {
      id: "remote-terminal-dialog",
      max_width: 512.,
      priority: 5,
      dismiss: "remote.terminal.close",
    };
    Some(self.dialog(frame, window, content, p, cx))
  }

  /// Hands React the session to open in the terminal pane. A new session
  /// has no label: React makes a fresh one.
  fn open_session(&mut self, label: Option<String>, reattach: bool, cx: &mut Context<Self>) {
    self.field_react(
      "remote.terminal.open",
      serde_json::json!({ "label": label, "reattach": reattach }),
      cx,
    );
  }

  fn ambiguous_dialog(
    &mut self,
    window: &mut Window,
    p: &Palette,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let reason = self.route.reason()?.to_string();
    // `DialogContent` is a plain block: the reason and the buttons follow
    // the header with no gap.
    let content = div()
      .child(header(
        "Remote change could not be verified",
        "A network interruption happened while a mutation was in flight. Treq did not retry automatically because the remote state is ambiguous. Refresh to check whether the change landed before trying again.",
        p,
      ))
      .child(
        div()
          .debug_selector(|| "remote-ambiguous-reason".into())
          .child(reason),
      )
      .child(
        div()
          .flex()
          .justify_end()
          .gap_2()
          .child(
            Button::new("remote-ambiguous-dismiss", p)
              .variant(Variant::Outline)
              .label("Dismiss")
              .on_click(cx.listener(|view, _, _, cx| {
                view.press_react("remote.ambiguous.dismiss", cx)
              })),
          )
          .child(
            Button::new("remote-ambiguous-refresh", p)
              .label("Refresh")
              .on_click(cx.listener(|view, _, _, cx| {
                view.press_react("remote.ambiguous.refresh", cx)
              })),
          ),
      );
    let frame = Frame {
      id: "remote-ambiguous-dialog",
      max_width: 512.,
      priority: 4,
      dismiss: "remote.ambiguous.dismiss",
    };
    Some(self.dialog(frame, window, content, p, cx))
  }
}

impl Render for RemoteView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    let p = theme::palette(self.dark);
    let layers = [
      self.setup_dialog(window, &p, cx),
      self.ambiguous_dialog(window, &p, cx),
      self.terminal_dialog(window, &p, cx),
    ];
    // The dialogs are drawn over the window; this view takes no room.
    div()
      .absolute()
      .size_0()
      .children(layers.into_iter().flatten())
  }
}
