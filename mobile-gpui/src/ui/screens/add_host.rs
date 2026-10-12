//! Adding a self-managed SSH host, with explicit host-key trust.
//!
//! Treq pins the host key by its SHA256 fingerprint and rejects the host if
//! the key ever changes. The user never types the fingerprint: Continue reads
//! the key the host presents and asks the user to confirm it once.

use gpui_kit::component::Disableable as _;
use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};

use crate::{
  managed,
  store::{self, UserManagedEndpoint},
  ui::{load, nav, widgets, Location},
};

/// On a phone the only SSH identity is the device key in the OS keystore.
#[cfg(mobile)]
const DEFAULT_IDENTITY: &str = treq_lib::core::remote_ssh_transport::DEVICE_KEYSTORE_KEY_REFERENCE;
#[cfg(not(mobile))]
const DEFAULT_IDENTITY: &str = "~/.ssh/id_ed25519";

pub fn open(cx: &mut App) {
  nav::push("Add SSH host", Location::default(), cx, |window, cx| {
    cx.new(|cx| AddHostScreen::new(window, cx))
  });
}

/// The add-host form's fields, kept apart from the view so the rules are testable.
#[derive(Default)]
struct HostForm {
  name: String,
  hostname: String,
  port: String,
  username: String,
  identity: String,
}

impl HostForm {
  /// Whether Continue is enabled: a host, a user and a key are filled in.
  fn complete(&self) -> bool {
    [&self.hostname, &self.username, &self.identity]
      .iter()
      .all(|value| !value.is_empty())
  }

  /// The endpoint the form describes. The host key fingerprint stays empty
  /// until the probe reads it and the user confirms it.
  fn endpoint(&self, now: chrono::DateTime<chrono::Utc>) -> Result<UserManagedEndpoint, String> {
    if self.hostname.is_empty() {
      return Err("Enter the hostname.".into());
    }
    let port = self
      .port
      .parse::<u16>()
      .ok()
      .filter(|p| *p > 0)
      .ok_or("The port must be a number from 1 to 65535.")?;
    if self.username.is_empty() {
      return Err("Enter the username.".into());
    }
    if self.identity.is_empty() {
      return Err("Enter the identity to authenticate with.".into());
    }
    Ok(UserManagedEndpoint {
      id: format!("user-managed-{}", now.timestamp_millis()),
      display_name: if self.name.is_empty() {
        self.hostname.clone()
      } else {
        self.name.clone()
      },
      hostname: self.hostname.clone(),
      port,
      username: self.username.clone(),
      host_key_fingerprint: String::new(),
      auth_identity_reference: self.identity.clone(),
      alias: None,
      created_at: now.to_rfc3339(),
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

pub struct AddHostScreen {
  name: Entity<InputState>,
  hostname: Entity<InputState>,
  port: Entity<InputState>,
  username: Entity<InputState>,
  identity: Entity<InputState>,
  /// The endpoint awaiting trust confirmation.
  pending: Option<UserManagedEndpoint>,
  /// Reading the host key the host presents.
  probing: bool,
  /// Starts reading the host key; tests swap in one that needs no backend.
  start_probe: fn(&mut Context<Self>, UserManagedEndpoint),
  error: Option<String>,
  public_key: load::Load<String>,
  scroll: ScrollHandle,
}

impl AddHostScreen {
  fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
    let hostname = input("Hostname or IP", "", window, cx);
    let username = input("Username", "", window, cx);
    let identity = input("Identity", DEFAULT_IDENTITY, window, cx);
    // Re-render on edits so Continue enables as soon as the form is complete.
    for field in [&hostname, &username, &identity] {
      cx.subscribe(field, |_, _, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
          cx.notify();
        }
      })
      .detach();
    }
    Self {
      name: input("Name (e.g. Workstation)", "", window, cx),
      hostname,
      port: input("Port", "22", window, cx),
      username,
      identity,
      pending: None,
      probing: false,
      start_probe: probe_on_backend,
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
      hostname: Self::value(&self.hostname, cx),
      port: Self::value(&self.port, cx),
      username: Self::value(&self.username, cx),
      identity: Self::value(&self.identity, cx),
    }
  }

  /// Reads the host's key and asks the user to confirm it.
  fn probe(&mut self, cx: &mut Context<Self>) {
    let endpoint = match self.form(cx).endpoint(chrono::Utc::now()) {
      Ok(endpoint) => endpoint,
      Err(error) => {
        self.error = Some(error);
        return cx.notify();
      }
    };
    self.error = None;
    self.probing = true;
    (self.start_probe)(cx, endpoint);
    cx.notify();
  }

  /// Shows the confirmation for the probed key, or why the host was not reached.
  fn probe_finished(&mut self, endpoint: UserManagedEndpoint, result: Result<String, String>) {
    self.probing = false;
    match result {
      Ok(fingerprint) => {
        self.pending = Some(UserManagedEndpoint {
          host_key_fingerprint: fingerprint,
          ..endpoint
        })
      }
      Err(error) => self.error = Some(error),
    }
  }

  fn device_key(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let key = match &self.public_key {
      load::Load::Idle | load::Load::Loading => None,
      load::Load::Ready(key) => Some(key.clone()),
      load::Load::Failed(_) => None,
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
                .children(self.public_key.error().map(|e| widgets::error_text(e.to_string(), cx)))
                .child(match key {
                    Some(key) => Button::new("copy-key")
                        .outline()
                        .icon(IconName::Copy)
                        .label("Copy public key")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(key.clone()))
                        }),
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
}

impl Render for AddHostScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let field = |id: &'static str, label: &str, input: &Entity<InputState>| {
      v_flex()
        .gap_1()
        .child(div().text_sm().child(label.to_string()))
        .child(Input::new(input).id(id).large())
    };
    let form = widgets::card(cx)
      .p_4()
      .gap_3()
      .child(field("name", "Name", &self.name))
      .child(field("hostname", "Hostname", &self.hostname))
      .child(field("port", "Port", &self.port))
      .child(field("username", "Username", &self.username))
      .child(field("identity", "Identity", &self.identity))
      .children(self.error.clone().map(|e| widgets::error_text(e, cx)));

    let actions = match &self.pending {
            None => v_flex().child(
                Button::new("continue")
                    .primary()
                    .large()
                    .w_full()
                    .label("Continue")
                    .loading(self.probing)
                    .disabled(self.probing || !self.form(cx).complete())
                    .on_click(cx.listener(|this, _, _, cx| this.probe(cx))),
            ),
            Some(endpoint) => widgets::card(cx)
                .p_4()
                .gap_3()
                .border_color(cx.theme().warning)
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Confirm host trust"))
                .child(widgets::muted(
                    format!(
                        "You are about to trust {}@{}:{} with host key fingerprint {}. Treq will reject this host if the key ever changes. Compare it with ssh-keygen -lf on the host before trusting it.",
                        endpoint.username, endpoint.hostname, endpoint.port, endpoint.host_key_fingerprint
                    ),
                    cx,
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("trust")
                                .primary()
                                .label("Trust and save")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(endpoint) = this.pending.take() {
                                        store::save_user_managed_endpoint(endpoint);
                                        nav::pop(cx);
                                    }
                                })),
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
                ),
        };

    let device_key = self.device_key(cx);
    div()
      .id("add-host")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(widgets::section("Host", cx).child(form))
          .child(actions)
          .child(device_key),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn filled() -> HostForm {
    HostForm {
      name: String::new(),
      hostname: "github.com".into(),
      port: "22".into(),
      username: "git".into(),
      identity: "keystore:device".into(),
    }
  }

  #[core::prelude::v1::test]
  fn continue_needs_a_host_a_user_and_a_key() {
    assert!(!HostForm::default().complete());
    assert!(filled().complete());
    for clear in [
      |f: &mut HostForm| f.hostname.clear(),
      |f: &mut HostForm| f.username.clear(),
      |f: &mut HostForm| f.identity.clear(),
    ] {
      let mut form = filled();
      clear(&mut form);
      assert!(!form.complete());
    }
    // The name and port are not required to enable Continue.
    let form = HostForm {
      port: String::new(),
      ..filled()
    };
    assert!(form.complete());
  }

  #[core::prelude::v1::test]
  fn endpoint_has_no_fingerprint_until_the_probe_confirms_one() {
    let endpoint = filled().endpoint(chrono::Utc::now()).unwrap();
    assert_eq!(endpoint.hostname, "github.com");
    assert_eq!(endpoint.port, 22);
    assert_eq!(endpoint.username, "git");
    assert_eq!(endpoint.auth_identity_reference, "keystore:device");
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

  #[core::prelude::v1::test]
  fn rejects_an_invalid_port() {
    for port in ["", "0", "70000", "ssh"] {
      let form = HostForm {
        port: port.into(),
        ..filled()
      };
      assert!(form.endpoint(chrono::Utc::now()).is_err(), "port {port:?}");
    }
  }

  /// UI tests through gpui-kit's harness: real clicks and typing, no backend.
  mod ui {
    // Explicit imports only: a `gpui_kit::*` glob would shadow the `#[test]`
    // that `#[gpui_kit::test]` expands to.
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
      base::Root, px, size, App, AppContext, Bounds, Context, Entity, Point, TestAppContext,
      Window, WindowBounds, WindowHandle, WindowOptions,
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

    fn open(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<AddHostScreen>) {
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
              let mut screen = AddHostScreen::new(window, cx);
              screen.start_probe = probe_ok;
              screen
            })
          },
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Root"), view)
      })
    }

    fn fill_host_and_user(window: &mut Window, cx: &mut App) {
      window.click("hostname", cx);
      window.input("github.com", cx);
      window.click("username", cx);
      window.input("git", cx);
    }

    #[gpui_kit::test]
    fn has_no_fingerprint_field_and_continue_waits_for_a_host_and_user(cx: &mut TestAppContext) {
      let (handle, view) = open(cx);
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        for id in ["name", "hostname", "port", "username", "identity"] {
          assert!(window.try_find(id).is_some(), "missing {id}");
        }
        assert!(window.try_find("fingerprint").is_none());
        window.click("continue", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let screen = view.read(cx);
        assert!(!screen.probing);
        assert!(screen.pending.is_none());
        assert!(screen.error.is_none());
      });
    }

    #[gpui_kit::test]
    fn continue_shows_the_presented_host_key_for_confirmation(cx: &mut TestAppContext) {
      let (handle, view) = open(cx);
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        fill_host_and_user(window, cx);
        // The username field still has focus, as on a phone.
        window.click("continue", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let pending = view.read(cx).pending.as_ref().expect("confirmation shown");
        assert_eq!(pending.host_key_fingerprint, PROBED);
        assert_eq!(pending.hostname, "github.com");
        assert_eq!(pending.username, "git");
        assert_eq!(pending.display_name, "github.com");
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
    fn an_invalid_port_shows_an_error_instead_of_probing(cx: &mut TestAppContext) {
      let (handle, view) = open(cx);
      cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        fill_host_and_user(window, cx);
        window.click("port", cx);
        window.press("backspace", cx);
        window.press("backspace", cx);
        window.input("0", cx);
        window.click("continue", cx);
      })
      .unwrap();
      cx.run_until_parked();
      cx.update(|cx| {
        let screen = view.read(cx);
        assert!(screen.pending.is_none());
        assert!(screen.error.as_deref().unwrap_or_default().contains("port"));
      });
    }
  }
}
