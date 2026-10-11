//! Adding a self-managed SSH host, with explicit host-key trust.
//!
//! Treq pins the host key by its SHA256 fingerprint and rejects the host if
//! the key ever changes, so the user confirms the fingerprint once here.

use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
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

pub struct AddHostScreen {
  name: Entity<InputState>,
  hostname: Entity<InputState>,
  port: Entity<InputState>,
  username: Entity<InputState>,
  fingerprint: Entity<InputState>,
  identity: Entity<InputState>,
  /// The endpoint awaiting trust confirmation.
  pending: Option<UserManagedEndpoint>,
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
    Self {
      name: input("Name (e.g. Workstation)", "", window, cx),
      hostname: input("Hostname or IP", "", window, cx),
      port: input("Port", "22", window, cx),
      username: input("Username", "", window, cx),
      fingerprint: input("Host key fingerprint (SHA256:...)", "", window, cx),
      identity: input("Identity", DEFAULT_IDENTITY, window, cx),
      pending: None,
      error: None,
      public_key: load::Load::Idle,
      scroll: ScrollHandle::new(),
    }
  }

  fn value(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).value().trim().to_string()
  }

  fn validate(&self, cx: &App) -> Result<UserManagedEndpoint, String> {
    let hostname = Self::value(&self.hostname, cx);
    let username = Self::value(&self.username, cx);
    let fingerprint = Self::value(&self.fingerprint, cx);
    let identity = Self::value(&self.identity, cx);
    let port = Self::value(&self.port, cx);
    if hostname.is_empty() {
      return Err("Enter the hostname.".into());
    }
    let port = port
      .parse::<u16>()
      .ok()
      .filter(|p| *p > 0)
      .ok_or("The port must be a number from 1 to 65535.")?;
    if username.is_empty() {
      return Err("Enter the username.".into());
    }
    if !fingerprint.starts_with("SHA256:") || fingerprint.len() <= "SHA256:".len() {
      return Err(
        "The fingerprint must look like SHA256:… (ssh-keyscan host | ssh-keygen -lf -).".into(),
      );
    }
    if identity.is_empty() {
      return Err("Enter the identity to authenticate with.".into());
    }
    let name = Self::value(&self.name, cx);
    let now = chrono::Utc::now();
    Ok(UserManagedEndpoint {
      id: format!("user-managed-{}", now.timestamp_millis()),
      display_name: if name.is_empty() {
        hostname.clone()
      } else {
        name
      },
      hostname,
      port,
      username,
      host_key_fingerprint: fingerprint,
      auth_identity_reference: identity,
      alias: None,
      created_at: now.to_rfc3339(),
    })
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
    let field = |label: &str, input: &Entity<InputState>| {
      v_flex()
        .gap_1()
        .child(div().text_sm().child(label.to_string()))
        .child(Input::new(input).large())
    };
    let form = widgets::card(cx)
      .p_4()
      .gap_3()
      .child(field("Name", &self.name))
      .child(field("Hostname", &self.hostname))
      .child(field("Port", &self.port))
      .child(field("Username", &self.username))
      .child(field("Host key fingerprint", &self.fingerprint))
      .child(field("Identity", &self.identity))
      .children(self.error.clone().map(|e| widgets::error_text(e, cx)));

    let actions = match &self.pending {
            None => v_flex().child(
                Button::new("continue")
                    .primary()
                    .large()
                    .w_full()
                    .label("Continue")
                    .on_click(cx.listener(|this, _, _, cx| {
                        match this.validate(cx) {
                            Ok(endpoint) => {
                                this.error = None;
                                this.pending = Some(endpoint);
                            }
                            Err(error) => this.error = Some(error),
                        }
                        cx.notify();
                    })),
            ),
            Some(endpoint) => widgets::card(cx)
                .p_4()
                .gap_3()
                .border_color(cx.theme().warning)
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Confirm host trust"))
                .child(widgets::muted(
                    format!(
                        "You are about to trust {}@{}:{} with host key fingerprint {}. Treq will reject this host if the key ever changes, unless you confirm a new fingerprint yourself.",
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
