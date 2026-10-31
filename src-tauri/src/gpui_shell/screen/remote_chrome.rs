//! What Dashboard draws around a remote repository's workspace view: the
//! strip above it (the repository's name, Terminal and Refresh), the
//! connection banner (`RemoteStatusBanner`), the capability notice
//! (`RemoteCapabilityNotice`) and, while access is cut off, the overlay
//! that dims the window (`remote-cutoff-overlay`).

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{self, div, prelude::*, App, ClickEvent, Div, Window};

use crate::gpui_shell::{
  theme::Palette,
  ui::{self, TextSize, Tone},
};

/// What React knows of the remote repository the workspace view shows.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteChrome {
  /// `activeRepository.displayName`.
  pub display_name: String,
  /// An SSH endpoint is connected, so "Terminal" opens its dialog.
  pub terminal: bool,
  /// `RemoteConnectionState`: `online`, `connecting`, `reconnecting`,
  /// `waking`, `stale`, `offline`, `degraded` or `cutoff`.
  pub state: String,
  /// The endpoint's credential is cut off until the user reauthenticates.
  pub cut_off: bool,
  /// `RemoteCapabilityNotice`'s lines: why unsupported actions are.
  pub notices: Vec<String>,
}

/// `RemoteStatusBanner`'s `COPY`: the banner's label and tone, or `None`
/// while connected.
fn copy(state: &str) -> Option<(&'static str, Tone)> {
  Some(match state {
    "online" => return None,
    "connecting" => ("Connecting…", Tone::Warning),
    "reconnecting" => ("Reconnecting…", Tone::Warning),
    "waking" => ("Waking managed VM…", Tone::Warning),
    "stale" => ("Showing cached data - refreshing", Tone::Warning),
    "offline" => ("Offline", Tone::Danger),
    "degraded" => ("Degraded - some checks are failing", Tone::Danger),
    "cutoff" => (
      "Credential cutoff - reauthenticate to continue",
      Tone::Danger,
    ),
    _ => return None,
  })
}

/// The links the banner offers in `state`: Wake now, Reconnect, Refresh
/// (Refresh not while cut off).
pub fn banner_links(state: &str, cut_off: bool) -> Vec<(&'static str, &'static str)> {
  let mut links = Vec::new();
  if state == "waking" {
    links.push(("remote.wake", "Wake now"));
  }
  if state == "offline" || state == "reconnecting" {
    links.push(("remote.reconnect", "Reconnect"));
  }
  if !cut_off {
    links.push(("remote.refresh", "Refresh"));
  }
  links
}

/// A press React answers (`remote.*`).
pub type Press =
  std::rc::Rc<dyn Fn(&'static str) -> Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>;

/// An `underline underline-offset-2` text button; a disabled one takes no
/// clicks and looks the same.
fn link(
  id: &'static str,
  action: &'static str,
  label: &'static str,
  enabled: bool,
  press: &Press,
) -> impl IntoElement {
  div()
    .id(id)
    .debug_selector(move || id.into())
    .role(gpui::Role::Button)
    .aria_label(label)
    .underline()
    .child(label)
    .when(enabled, |d| d.cursor_pointer().on_click(press(action)))
    .test_support()
}

/// The strip above the workspace view: the name and its links, the banner
/// and the capability notice.
pub fn strip(chrome: &RemoteChrome, _dark: bool, p: &Palette, press: &Press) -> Div {
  let enabled = !chrome.cut_off;
  let top = ui::text(div(), TextSize::Xs)
    .flex()
    .items_center()
    .justify_between()
    .px_4()
    .py_1()
    .text_color(p.muted_foreground)
    .child(div().child(chrome.display_name.clone()))
    .child(
      div()
        .flex()
        .items_center()
        .gap_3()
        .when(chrome.terminal, |d| {
          d.child(link(
            "remote-terminal-link",
            "remote.terminal",
            "Terminal",
            enabled,
            press,
          ))
        })
        .child(link(
          "remote-refresh-link",
          "remote.refresh",
          "Refresh",
          enabled,
          press,
        )),
    );
  let banner = copy(&chrome.state).map(|(label, t)| {
    let (bg, fg, border) = (t.tint(p), t.text(p), t.border(p));
    ui::text(div(), TextSize::Sm)
      .debug_selector(|| "remote-status-banner".into())
      .flex()
      .items_center()
      .justify_between()
      .gap_3()
      .rounded_md()
      .border_1()
      .border_color(border)
      .bg(bg)
      .text_color(fg)
      .px_3()
      .py_1p5()
      .child(
        div()
          .flex()
          .child(div().font_weight(gpui::FontWeight::MEDIUM).child(label))
          .when(chrome.cut_off, |d| {
            d.child(
              div()
                .text_color(p.muted_foreground)
                .child("\u{a0}- Access is blocked until you reauthenticate."),
            )
          }),
      )
      .child(
        div().flex().items_center().gap_2().children(
          banner_links(&chrome.state, chrome.cut_off)
            .into_iter()
            .map(|(action, label)| {
              let id = match action {
                "remote.wake" => "remote-banner-wake",
                "remote.reconnect" => "remote-banner-reconnect",
                _ => "remote-banner-refresh",
              };
              link(id, action, label, true, press)
            }),
        ),
      )
  });
  let notice = (!chrome.notices.is_empty()).then(|| {
    ui::text(div(), TextSize::Xs)
      .debug_selector(|| "remote-capability-notice".into())
      .border_b_1()
      .border_color(p.border)
      .px_4()
      .py_1p5()
      .text_color(p.muted_foreground)
      .children(chrome.notices.iter().map(|n| div().child(n.clone())))
  });
  div()
    .debug_selector(|| "remote-strip".into())
    .flex_none()
    .border_b_1()
    .border_color(p.border)
    .bg(p.background)
    .child(top)
    .child(div().px_4().pb_1p5().children(banner))
    .children(notice)
}

/// `remote-cutoff-overlay`: dims and blocks what it covers.
pub fn cutoff_overlay(p: &Palette) -> impl IntoElement {
  div()
    .id("remote-cutoff-overlay")
    .debug_selector(|| "remote-cutoff-overlay".into())
    .absolute()
    .inset_0()
    .bg(p.background.opacity(0.4))
    .occlude()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_banner_reads_like_react() {
    assert_eq!(copy("online"), None);
    assert_eq!(copy("offline"), Some(("Offline", Tone::Danger)));
    assert_eq!(copy("waking").map(|c| c.1), Some(Tone::Warning));
    assert_eq!(
      banner_links("waking", false),
      vec![("remote.wake", "Wake now"), ("remote.refresh", "Refresh")]
    );
    assert_eq!(
      banner_links("offline", false),
      vec![
        ("remote.reconnect", "Reconnect"),
        ("remote.refresh", "Refresh")
      ]
    );
    assert_eq!(banner_links("cutoff", true), vec![]);
  }
}
