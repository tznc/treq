//! An SSH host: adding one, with explicit host-key trust, and editing a
//! saved one (rename, check its key again, remove).
//!
//! Treq pins the host key by its SHA256 fingerprint and rejects the host if
//! the key ever changes. The user never types the fingerprint: Continue reads
//! the key the host presents and asks the user to confirm it once. When a
//! host's key changes, "Check host key" shows the old and new fingerprints
//! and trusting the new one keeps the host's repositories.

use gpui_kit::component::Disableable as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, IconName, Sizable,
  },
  prelude::FluentBuilder as _,
  *,
};

use super::Confirm;
use crate::{
  managed,
  store::{self, UserManagedEndpoint},
  ui::{self, load, nav, widgets, Location, Machine},
};

/// On a phone the only SSH identity is the device key in the OS keystore,
/// so the form has no identity field there.
#[cfg(mobile)]
const DEFAULT_IDENTITY: Option<&str> = None;
#[cfg(not(mobile))]
const DEFAULT_IDENTITY: Option<&str> = Some("~/.ssh/id_ed25519");

#[cfg(mobile)]
const DEVICE_IDENTITY: &str = treq_lib::core::remote_ssh_transport::DEVICE_KEYSTORE_KEY_REFERENCE;
#[cfg(not(mobile))]
const DEVICE_IDENTITY: &str = "";

pub fn open(cx: &mut App) {
  nav::push("Add SSH host", Location::default(), cx, |window, cx| {
    cx.new(|cx| AddHostScreen::new(None, window, cx))
  });
}

/// A saved host's settings.
pub fn edit(record: UserManagedEndpoint, cx: &mut App) {
  let title = record.display_name.clone();
  nav::push(title, Location::default(), cx, |window, cx| {
    cx.new(|cx| AddHostScreen::new(Some(record), window, cx))
  });
}

/// A saved host's settings, reading the key it presents now.
pub fn review_key(record: UserManagedEndpoint, cx: &mut App) {
  let title = record.display_name.clone();
  nav::push(title, Location::default(), cx, |window, cx| {
    cx.new(|cx| {
      let mut screen = AddHostScreen::new(Some(record), window, cx);
      screen.probe(cx);
      screen
    })
  });
}

/// `user@host:port` → its parts. The port defaults to 22; IPv6 addresses
/// go in brackets (`dev@[fd00::1]:2222`).
fn parse_address(address: &str) -> Result<(String, String, u16), String> {
  let address = address.trim();
  let (username, rest) = address
    .rsplit_once('@')
    .filter(|(user, _)| !user.is_empty())
    .ok_or("Enter the address as user@host, such as dev@10.0.4.12 or dev@build.lan:2222.")?;
  let (hostname, port) = if let Some(bracketed) = rest.strip_prefix('[') {
    let (host, after) = bracketed
      .split_once(']')
      .ok_or("Close the IPv6 address with ].")?;
    (host, after.strip_prefix(':'))
  } else {
    match rest.split_once(':') {
      Some((host, port)) => (host, Some(port)),
      None => (rest, None),
    }
  };
  if hostname.is_empty() {
    return Err("Enter the host after the @.".into());
  }
  let port = match port {
    None => 22,
    Some(port) => port
      .parse::<u16>()
      .ok()
      .filter(|p| *p > 0)
      .ok_or("The port must be a number from 1 to 65535.")?,
  };
  Ok((username.to_string(), hostname.to_string(), port))
}

/// The add-host form's fields, kept apart from the view so the rules are testable.
#[derive(Default)]
struct HostForm {
  name: String,
  address: String,
  /// The key path, on desktop builds only.
  identity: Option<String>,
}

impl HostForm {
  /// Whether Continue is enabled: an address (and a key, where it's asked).
  fn complete(&self) -> bool {
    !self.address.is_empty() && self.identity.as_ref().is_none_or(|i| !i.is_empty())
  }

  /// The endpoint the form describes. The host key fingerprint stays empty
  /// until the probe reads it and the user confirms it.
  fn endpoint(&self, now: chrono::DateTime<chrono::Utc>) -> Result<UserManagedEndpoint, String> {
    let (username, hostname, port) = parse_address(&self.address)?;
    let identity = match &self.identity {
      Some(identity) if identity.is_empty() => {
        return Err("Enter the identity to authenticate with.".into())
      }
      Some(identity) => identity.clone(),
      None => DEVICE_IDENTITY.to_string(),
    };
    Ok(UserManagedEndpoint {
      id: format!("user-managed-{}", now.timestamp_millis()),
      display_name: if self.name.is_empty() {
        hostname.clone()
      } else {
        self.name.clone()
      },
      hostname,
      port,
      username,
      host_key_fingerprint: String::new(),
      auth_identity_reference: identity,
      alias: None,
      created_at: now.to_rfc3339(),
      terminal_backend: Default::default(),
    })
  }
}

/// Reads the host key on the backend runtime.
fn probe_on_backend(cx: &mut Context<AddHostScreen>, endpoint: UserManagedEndpoint) {
  let (hostname, port) = (endpoint.hostname.clone(), endpoint.port);
  load::run(
    cx,
    async move {
      treq_lib::core::remote_ssh_host_key_probe::probe_host_key_fingerprint(&hostname, port).await
    },
    move |this, result, _| this.probe_finished(endpoint, result),
  );
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
  Remove,
}

pub struct AddHostScreen {
  /// The saved host being edited; `None` when adding one.
  saved: Option<UserManagedEndpoint>,
  name: Entity<InputState>,
  address: Entity<InputState>,
  identity: Option<Entity<InputState>>,
  /// The endpoint awaiting trust confirmation.
  pending: Option<UserManagedEndpoint>,
  /// Reading the host key the host presents.
  probing: bool,
  /// Set when a saved host's key was checked and hasn't changed.
  key_unchanged: bool,
  /// Starts reading the host key; tests swap in one that needs no backend.
  start_probe: fn(&mut Context<Self>, UserManagedEndpoint),
  confirm: Confirm<Action>,
  error: Option<String>,
  public_key: load::Load<String>,
  scroll: ScrollHandle,
}

impl AddHostScreen {
  fn new(saved: Option<UserManagedEndpoint>, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let input = |placeholder: &str, value: &str, window: &mut Window, cx: &mut Context<Self>| {
      let placeholder = placeholder.to_string();
      let value = value.to_string();
      cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        if !value.is_empty() {
          state.set_value(value, window, cx);
        }
        state
      })
    };
    let address_value = saved
      .as_ref()
      .map(|s| format!("{}@{}:{}", s.username, s.hostname, s.port))
      .unwrap_or_default();
    let name_value = saved
      .as_ref()
      .map(|s| s.display_name.clone())
      .unwrap_or_default();
    let address = input("dev@build.lan:2222", &address_value, window, cx);
    let identity = match (&saved, DEFAULT_IDENTITY) {
      (None, Some(default)) => Some(input("Identity", default, window, cx)),
      _ => None,
    };
    // Re-render on edits so Continue enables as soon as the form is complete.
    for field in std::iter::once(&address).chain(identity.as_ref()) {
      cx.subscribe(field, |_, _, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          cx.notify();
        }
      })
      .detach();
    }
    Self {
      saved,
      name: input("Name (e.g. Workstation)", &name_value, window, cx),
      address,
      identity,
      pending: None,
      probing: false,
      key_unchanged: false,
      start_probe: probe_on_backend,
      confirm: Confirm::default(),
      error: None,
      public_key: load::Load::Idle,
      scroll: ScrollHandle::new(),
    }
  }

  fn value(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).value().trim().to_string()
  }

  /// The form's current text, trimmed.
  fn form(&self, cx: &App) -> HostForm {
    HostForm {
      name: Self::value(&self.name, cx),
      address: Self::value(&self.address, cx),
      identity: self.identity.as_ref().map(|i| Self::value(i, cx)),
    }
  }

  /// Reads the host's key and asks the user to confirm it.
  fn probe(&mut self, cx: &mut Context<Self>) {
    let endpoint = match &self.saved {
      Some(saved) => saved.clone(),
      None => match self.form(cx).endpoint(chrono::Utc::now()) {
        Ok(endpoint) => endpoint,
        Err(error) => {
          self.error = Some(error);
          return cx.notify();
        }
      },
    };
    self.error = None;
    self.key_unchanged = false;
    self.probing = true;
    (self.start_probe)(cx, endpoint);
    cx.notify();
  }

  /// Shows the confirmation for the probed key, or why the host was not reached.
  fn probe_finished(&mut self, endpoint: UserManagedEndpoint, result: Result<String, String>) {
    self.probing = false;
    match result {
      Ok(fingerprint) if fingerprint == endpoint.host_key_fingerprint => self.key_unchanged = true,
      Ok(fingerprint) => {
        self.pending = Some(UserManagedEndpoint {
          host_key_fingerprint: fingerprint,
          ..endpoint
        })
      }
      Err(error) => self.error = Some(crate::errors::describe(&error).message),
    }
  }

  /// Saves the confirmed host and connects to it from the machine list.
  fn trust(&mut self, cx: &mut Context<Self>) {
    let Some(endpoint) = self.pending.take() else {
      return;
    };
    store::save_user_managed_endpoint(endpoint.clone());
    // A changed key needs a fresh connection; the old one never verified.
    if let Some(connection) = crate::backend::get().remove_connection(&endpoint.id) {
      crate::backend::spawn_detached(async move {
        crate::backend::get()
          .pool
          .mark_dead(&connection.endpoint)
          .await;
      });
    }
    nav::home(cx);
    ui::connect(Machine::Host(endpoint), true, cx);
  }

  fn save_name(&mut self, cx: &mut Context<Self>) {
    let Some(saved) = &mut self.saved else { return };
    let name = Self::value(&self.name, cx);
    saved.display_name = if name.is_empty() {
      saved.hostname.clone()
    } else {
      name
    };
    store::save_user_managed_endpoint(saved.clone());
    nav::pop(cx);
  }

  fn remove(&mut self, cx: &mut Context<Self>) {
    if !self.confirm.tap(Action::Remove) {
      return cx.notify();
    }
    if let Some(saved) = &self.saved {
      ui::forget_host(&saved.id, cx);
    }
    nav::home(cx);
  }

  fn device_key(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let key = match &self.public_key {
      load::Load::Ready(key) => Some(key.clone()),
      _ => None,
    };
    widgets::section("This device's key", cx).child(
      widgets::card(cx)
        .p_4()
        .gap_3()
        .child(widgets::muted(
          "Add this public key to ~/.ssh/authorized_keys on the host. The private key stays in this device's secure storage.",
          cx,
        ))
        .children(key.clone().map(|key| {
          div()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .child(widgets::mono(key, cx).whitespace_normal())
        }))
        .children(
          self
            .public_key
            .error()
            .map(|e| widgets::error_text(e.to_string(), cx)),
        )
        .child(match key {
          Some(key) => Button::new("copy-key")
            .outline()
            .icon(IconName::Copy)
            .label("Copy public key")
            .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(key.clone()))),
          None => Button::new("show-key")
            .outline()
            .label("Show public key")
            .loading(self.public_key.is_loading())
            .on_click(cx.listener(|this, _, _, cx| {
              this.public_key = load::Load::Loading;
              load::run(cx, managed::device_public_key_line(), |this, result, _| {
                this.public_key = load::Load::from_result(result);
              });
            })),
        }),
    )
  }

  /// The presented key to trust: a new host's, or a saved host's new key
  /// beside the one it replaces.
  fn trust_card(&self, endpoint: &UserManagedEndpoint, cx: &mut Context<Self>) -> Div {
    let replacing = self
      .saved
      .as_ref()
      .map(|s| s.host_key_fingerprint.clone())
      .filter(|old| !old.is_empty());
    let (title, message) = match &replacing {
      None => (
        "Trust this host?",
        format!(
          "{} presented this key. Compare it with ssh-keygen -lf on the host. Treq rejects the host if the key ever changes.",
          endpoint.hostname
        ),
      ),
      Some(_) => (
        "The host key changed",
        format!(
          "{} now presents a different key. Trust it only if you know why it changed (a reinstall, a new machine). Compare it with ssh-keygen -lf on the host.",
          endpoint.hostname
        ),
      ),
    };
    let fingerprint = |label: &str, value: String| {
      v_flex()
        .gap_1()
        .child(widgets::muted(label.to_string(), cx))
        .child(
          div()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .child(widgets::mono(value, cx).whitespace_normal()),
        )
    };
    widgets::card(cx)
      .p_4()
      .gap_3()
      .border_color(cx.theme().warning)
      .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
      .child(widgets::muted(message, cx))
      .when_some(replacing, |card, old| {
        card.child(fingerprint("Trusted until now", old))
      })
      .child(fingerprint(
        if self.saved.is_some() {
          "New key"
        } else {
          "Key"
        },
        endpoint.host_key_fingerprint.clone(),
      ))
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("trust")
              .primary()
              .label(if self.saved.is_some() {
                "Trust new key"
              } else {
                "Trust and connect"
              })
              .on_click(cx.listener(|this, _, _, cx| this.trust(cx))),
          )
          .child(
            Button::new("cancel")
              .outline()
              .label("Cancel")
              .on_click(cx.listener(|this, _, _, cx| {
                this.pending = None;
                cx.notify();
              })),
          ),
      )
  }
}

impl Render for AddHostScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let field = |id: &'static str, label: &str, input: &Entity<InputState>, disabled: bool| {
      v_flex()
        .gap_1()
        .child(div().text_sm().child(label.to_string()))
        .child(Input::new(input).id(id).large().disabled(disabled))
    };
    let editing = self.saved.is_some();
    let form = widgets::card(cx)
      .p_4()
      .gap_3()
      .child(field("address", "Address", &self.address, editing))
      .child(field("name", "Name", &self.name, false))
      .when_some(self.identity.clone(), |form, identity| {
        form.child(field("identity", "Identity", &identity, false))
      })
      .when(!editing, |form| {
        form.children(self.error.clone().map(|e| widgets::error_text(e, cx)))
      });

    let actions = match (&self.pending, editing) {
      (Some(endpoint), _) => self.trust_card(endpoint, cx),
      (None, false) => v_flex().child(
        Button::new("continue")
          .primary()
          .large()
          .w_full()
          .label("Continue")
          .loading(self.probing)
          .disabled(self.probing || !self.form(cx).complete())
          .on_click(cx.listener(|this, _, _, cx| this.probe(cx))),
      ),
      (None, true) => v_flex()
        .gap_3()
        .child(
          Button::new("save")
            .primary()
            .large()
            .w_full()
            .label("Save")
            .on_click(cx.listener(|this, _, _, cx| this.save_name(cx))),
        )
        .child(
          widgets::section("Host key", cx).child(
            widgets::card(cx)
              .p_4()
              .gap_3()
              .child(
                widgets::mono(
                  self
                    .saved
                    .as_ref()
                    .map(|s| s.host_key_fingerprint.clone())
                    .unwrap_or_default(),
                  cx,
                )
                .whitespace_normal(),
              )
              .when(self.key_unchanged, |card| {
                card.child(widgets::muted("The host still presents this key.", cx))
              })
              .children(self.error.clone().map(|e| widgets::error_text(e, cx)))
              .child(
                h_flex().child(
                  Button::new("check-key")
                    .outline()
                    .label("Check host key")
                    .loading(self.probing)
                    .disabled(self.probing)
                    .on_click(cx.listener(|this, _, _, cx| this.probe(cx))),
                ),
              ),
          ),
        )
        .child(
          h_flex().child(
            Button::new("remove")
              .danger()
              .label(if self.confirm.is_armed(Action::Remove) {
                "Tap again to remove"
              } else {
                "Remove host"
              })
              .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
          ),
        ),
    };

    let device_key = cfg!(mobile).then(|| self.device_key(cx));
    div()
      .id("add-host")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(widgets::section("Host", cx).child(form))
          .child(actions)
          .children(device_key),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn filled() -> HostForm {
    HostForm {
      name: String::new(),
      address: "git@github.com".into(),
      identity: Some("~/.ssh/id_ed25519".into()),
    }
  }

  #[core::prelude::v1::test]
  fn parses_one_address_field() {
    assert_eq!(
      parse_address("dev@build.lan:2222").unwrap(),
      ("dev".into(), "build.lan".into(), 2222)
    );
    assert_eq!(
      parse_address(" dev@10.0.4.12 ").unwrap(),
      ("dev".into(), "10.0.4.12".into(), 22)
    );
    assert_eq!(
      parse_address("dev@[fd00::1]:2200").unwrap(),
      ("dev".into(), "fd00::1".into(), 2200)
    );
    for bad in [
      "build.lan",
      "@build.lan",
      "dev@",
      "dev@host:0",
      "dev@host:ssh",
      "dev@[::1",
    ] {
      assert!(parse_address(bad).is_err(), "{bad:?}");
    }
  }

  #[core::prelude::v1::test]
  fn continue_needs_an_address_and_any_asked_for_key() {
    assert!(!HostForm::default().complete());
    assert!(filled().complete());
    let no_key = HostForm {
      identity: Some(String::new()),
      ..filled()
    };
    assert!(!no_key.complete());
    // On a phone there is no identity field.
    let phone = HostForm {
      identity: None,
      ..filled()
    };
    assert!(phone.complete());
  }

  #[core::prelude::v1::test]
  fn endpoint_has_no_fingerprint_until_the_probe_confirms_one() {
    let endpoint = filled().endpoint(chrono::Utc::now()).unwrap();
    assert_eq!(endpoint.hostname, "github.com");
    assert_eq!(endpoint.port, 22);
    assert_eq!(endpoint.username, "git");
    assert_eq!(endpoint.auth_identity_reference, "~/.ssh/id_ed25519");
    assert_eq!(endpoint.host_key_fingerprint, "");
  }

  #[core::prelude::v1::test]
  fn display_name_defaults_to_the_hostname() {
    let now = chrono::Utc::now();
    assert_eq!(filled().endpoint(now).unwrap().display_name, "github.com");
    let named = HostForm {
      name: "Workstation".into(),
      ..filled()
    };
    assert_eq!(named.endpoint(now).unwrap().display_name, "Workstation");
  }

  /// UI tests through gpui-kit's harness: real clicks and typing, no backend.
  mod ui {
    // Explicit imports only: a `gpui_kit::*` glob would shadow the `#[test]`
    // that `#[gpui_kit::test]` expands to.
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
      base::Root, px, size, AppContext, Bounds, Context, Entity, Point, TestAppContext,
      WindowBounds, WindowHandle, WindowOptions,
    };

    use super::super::AddHostScreen;
    use crate::store::UserManagedEndpoint;

    const PROBED: &str = "SHA256:probed";

    /// Stands in for the network probe: the host presents `PROBED`.
    fn probe_ok(cx: &mut Context<AddHostScreen>, endpoint: UserManagedEndpoint) {
      cx.spawn(async move |this, cx| {
        this
          .update(cx, |view, cx| {
            view.probe_finished(endpoint, Ok(PROBED.into()));
            cx.notify();
          })
          .ok();
      })
      .detach();
    }

    fn saved(fingerprint: &str) -> UserManagedEndpoint {
      UserManagedEndpoint {
        id: "user-managed-1".into(),
        display_name: "Workstation".into(),
        hostname: "build.lan".into(),
        port: 2222,
        username: "dev".into(),
        host_key_fingerprint: fingerprint.into(),
        auth_identity_reference: "~/.ssh/id_ed25519".into(),
        alias: None,
        created_at: String::new(),
        terminal_backend: Default::default(),
      }
    }

    fn open(
      cx: &mut TestAppContext,
      record: Option<UserManagedEndpoint>,
    ) -> (WindowHandle<Root>, Entity<AddHostScreen>) {
      cx.update(gpui_kit::init);
      cx.update(|cx| {
        let bounds = Bounds {
          origin: Point::default(),
          size: size(px(400.), px(900.)),
        };
        let (window, view) = gpui_kit::open_window(
          WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
          },
          cx,
          |window, cx| {
            cx.new(|cx| {
              let mut screen = AddHostScreen::new(record, window, cx);
              screen.start_probe = probe_ok;
              screen
            })
          },
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Root"), view)
      })
    }

    #[gpui_kit::test]
    fn continue_waits_for_an_address(cx: &mut TestAppContext) {
      let (handle, view) = open(cx, None);
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        for id in ["name", "address", "identity"] {
          assert!(window.try_find(id).is_some(), "missing {id}");
        }
        for id in ["hostname", "port", "username", "fingerprint"] {
          assert!(window.try_find(id).is_none(), "unexpected {id}");
        }
        window.click("continue", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let screen = view.read(cx);
        assert!(!screen.probing);
        assert!(screen.pending.is_none());
      });
    }

    #[gpui_kit::test]
    fn continue_shows_the_presented_host_key_for_confirmation(cx: &mut TestAppContext) {
      let (handle, view) = open(cx, None);
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("address", cx);
        window.input("git@github.com", cx);
        window.click("continue", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let pending = view.read(cx).pending.as_ref().expect("confirmation shown");
        assert_eq!(pending.host_key_fingerprint, PROBED);
        assert_eq!(pending.hostname, "github.com");
        assert_eq!(pending.username, "git");
        assert_eq!(pending.port, 22);
      });
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("trust").visible());
        window.click("cancel", cx);
      })
      .unwrap();
      cx.update(|cx| assert!(view.read(cx).pending.is_none()));
    }

    #[gpui_kit::test]
    fn an_invalid_address_shows_an_error_instead_of_probing(cx: &mut TestAppContext) {
      let (handle, view) = open(cx, None);
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("address", cx);
        window.input("github.com:0", cx);
        window.click("continue", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let screen = view.read(cx);
        assert!(screen.pending.is_none());
        assert!(screen
          .error
          .as_deref()
          .unwrap_or_default()
          .contains("user@host"));
      });
    }

    #[gpui_kit::test]
    fn a_changed_key_asks_to_trust_the_new_one(cx: &mut TestAppContext) {
      let (handle, view) = open(cx, Some(saved("SHA256:old")));
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("check-key", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let screen = view.read(cx);
        let pending = screen.pending.as_ref().expect("new key shown");
        assert_eq!(pending.id, "user-managed-1");
        assert_eq!(pending.host_key_fingerprint, PROBED);
      });
    }

    #[gpui_kit::test]
    fn an_unchanged_key_says_so(cx: &mut TestAppContext) {
      let (handle, view) = open(cx, Some(saved(PROBED)));
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("check-key", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let screen = view.read(cx);
        assert!(screen.pending.is_none());
        assert!(screen.key_unchanged);
      });
    }
  }
}
