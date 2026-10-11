//! Home: a landing page when signed out, otherwise account, managed
//! instance and saved SSH hosts.

use gpui_kit::{component::Disableable as _, prelude::FluentBuilder as _};
use gpui_kit::{
  component::{
    alert::Alert,
    button::{Button, ButtonVariants},
    h_flex,
    spinner::Spinner,
    v_flex, ActiveTheme, IconName, Sizable,
  },
  *,
};

use super::{model, screens, widgets};
use crate::{backend::ConnectionKind, managed, store};

const TAGLINE: &str = "Isolates each agent and rebases stacked PRs when the base moves";

pub struct HomeScreen {
  scroll: ScrollHandle,
}

impl HomeScreen {
  pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
    cx.observe(&model(cx), |_, _, cx| cx.notify()).detach();
    Self {
      scroll: ScrollHandle::new(),
    }
  }

  fn account(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let model = model(cx).read(cx);
    let busy = model.auth_busy;
    let error = model.auth_error.clone();
    let content = match model.user.clone() {
      Some(email) => h_flex()
        .gap_3()
        .items_center()
        .child(
          v_flex()
            .flex_1()
            .min_w_0()
            .child(widgets::muted("Signed in as", cx))
            .child(div().truncate().child(email)),
        )
        .child(
          Button::new("sign-out")
            .outline()
            .label("Sign out")
            .loading(busy)
            .on_click(|_, _, cx| super::sign_out(cx)),
        ),
      None => h_flex()
        .gap_3()
        .items_center()
        .child(
          widgets::muted("Sign in to reach your Treq-managed instance.", cx)
            .flex_1()
            .min_w_0(),
        )
        .child(
          Button::new("sign-in")
            .primary()
            .label("Sign in")
            .loading(busy)
            .on_click(|_, _, cx| super::sign_in(cx)),
        ),
    };
    widgets::section("Account", cx).child(
      widgets::card(cx)
        .p_4()
        .gap_2()
        .child(content)
        .children(error.map(|e| widgets::error_text(e, cx))),
    )
  }

  fn connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let model = model(cx).read(cx);
    let signed_in = model.user.is_some();
    let connecting = model.connecting;
    let error = model.connect_error.clone();
    let connection = model.connection.clone();

    let body = if let Some(connection) = connection {
      let endpoint = &connection.endpoint;
      let kind = match connection.kind {
        ConnectionKind::Managed => "Treq-managed instance",
        ConnectionKind::UserManaged { .. } => "SSH host",
      };
      v_flex()
        .gap_3()
        .child(v_flex().child(widgets::muted(kind, cx)).child(format!(
          "{}@{}:{}",
          endpoint.username, endpoint.hostname, endpoint.port
        )))
        .child(
          h_flex()
            .gap_2()
            .child(
              Button::new("open-repos")
                .primary()
                .label("Repositories")
                .on_click(|_, _, cx| screens::repos::open(cx)),
            )
            .child(
              Button::new("disconnect")
                .outline()
                .label("Switch host")
                .on_click(|_, _, cx| super::disconnect(cx)),
            ),
        )
    } else {
      v_flex()
        .gap_3()
        .child(
          Button::new("connect-managed")
            .primary()
            .large()
            .w_full()
            .label("Connect to managed instance")
            .disabled(!signed_in || connecting.is_some())
            .on_click(|_, _, cx| super::connect_managed(cx)),
        )
        .when(!signed_in, |c| {
          c.child(widgets::muted(
            "Sign in to connect to your Treq-managed instance.",
            cx,
          ))
        })
    };

    widgets::section("Remote", cx).child(
      widgets::card(cx)
        .p_4()
        .gap_3()
        .child(body)
        .when_some(connecting, |c, step| {
          c.child(
            h_flex()
              .gap_2()
              .items_center()
              .child(Spinner::new().small())
              .child(widgets::muted(format!("{step}…"), cx)),
          )
        })
        .when_some(error, |c, error| {
          c.child(match error.strip_prefix(managed::SECURE_STORAGE_PREFIX) {
            Some(detail) => Alert::warning("secure-storage", detail.trim().to_string())
              .title("Secure storage isn't set up on this device")
              .into_any_element(),
            None => widgets::error_text(error, cx).into_any_element(),
          })
        }),
    )
  }

  // The saved-host rows, or `None` when there are none.
  fn host_rows(&self, cx: &mut Context<Self>) -> Option<Div> {
    let connecting = model(cx).read(cx).connecting.is_some();
    let hosts = store::user_managed_endpoints();
    if hosts.is_empty() {
      return None;
    }
    Some(
      widgets::card(cx).children(hosts.into_iter().enumerate().map(|(i, host)| {
        let detail = format!("{}@{}:{}", host.username, host.hostname, host.port);
        let id = host.id.clone();
        let remove = Button::new(("remove-host", i))
          .ghost()
          .small()
          .icon(IconName::Delete)
          .on_click(move |_, _, cx| {
            cx.stop_propagation();
            store::remove_user_managed_endpoint(&id);
            cx.refresh_windows();
          });
        widgets::row_with(
          ("host", i),
          host.display_name.clone(),
          Some(detail.into()),
          Some(remove.into_any_element()),
          cx,
        )
        .when(!connecting, |row| {
          row.on_click(move |_, _, cx| super::connect_user_managed(host.clone(), cx))
        })
      })),
    )
  }

  fn hosts(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let list = self.host_rows(cx).unwrap_or_else(|| {
      widgets::card(cx).child(widgets::centered_message(
        "No SSH hosts saved on this device.",
        cx,
      ))
    });
    widgets::section("SSH hosts on this device", cx)
      .child(list)
      .child(
        Button::new("add-host")
          .outline()
          .icon(IconName::Plus)
          .label("Add SSH host")
          .on_click(|_, _, cx| screens::add_host::open(cx)),
      )
  }

  /// The signed-out start screen: brand centered, sign-in within thumb reach.
  fn landing(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let model = model(cx).read(cx);
    let busy = model.auth_busy;
    let error = model.auth_error.clone();
    let connecting = model.connecting;
    let connect_error = model.connect_error.clone();
    // Saved hosts stay reachable below the buttons.
    let hosts = self.host_rows(cx);
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
          .child(
            Button::new("sign-in")
              .primary()
              .large()
              .w_full()
              .h(px(widgets::CTA_H))
              .label("Sign in")
              .loading(busy)
              .on_click(|_, _, cx| super::sign_in(cx)),
          )
          .children(error.map(|e| widgets::error_text(e, cx).text_center()))
          .child(
            Button::new("add-host")
              .outline()
              .large()
              .w_full()
              .h(px(widgets::CTA_H))
              .label("Use your own SSH host")
              .on_click(|_, _, cx| screens::add_host::open(cx)),
          )
          .when_some(connecting, |c, step| {
            c.child(
              h_flex()
                .gap_2()
                .justify_center()
                .items_center()
                .child(Spinner::new().small())
                .child(widgets::muted(format!("{step}…"), cx)),
            )
          })
          .children(connect_error.map(|e| widgets::error_text(e, cx).text_center()))
          .children(hosts.map(|rows| widgets::section("SSH hosts on this device", cx).child(rows))),
      )
  }

  /// Compact brand header above the signed-in home.
  fn brand_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
    h_flex()
      .gap_2()
      .items_center()
      .child(widgets::logo(28.))
      .child(widgets::wordmark(20., cx))
  }
}

impl Render for HomeScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let model = model(cx).read(cx);
    let landing = super::is_landing(model.user.as_deref(), model.connection.is_some());
    let content = if landing {
      self.landing(cx).into_any_element()
    } else {
      let header = self.brand_header(cx);
      let account = self.account(cx);
      let connection = self.connection(cx);
      let hosts = self.hosts(cx);
      widgets::page()
        .child(header)
        .child(account)
        .child(connection)
        .child(hosts)
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
