//! Full-window screens drawn by GPUI in place of the webview.
//!
//! Each screen reads and changes the app's state through the backend itself
//! (a [`Host`], faked in tests). React only routes: it pushes a
//! [`ScreenSnapshot`] with the `gpui_screen_update` command saying which
//! screen shows and for what (a tab, a repository, a workspace), and GPUI
//! draws it where the webview would be, leaving the webview out of the frame
//! (hidden, with React still running in it). What a screen leaves to React
//! (closing it, flows not ported yet) goes back to the window's webview as
//! `gpui-screen-action` events; what it changed itself goes as native events
//! (`native::NativeEvent`), so React's caches follow.
//!
//! React keeps drawing its UI too, so pushing `null` (for a screen or dialog
//! GPUI does not draw yet) brings the webview back with React's version.

use std::{rc::Rc, sync::Arc};

use serde::{Deserialize, Serialize};
use tauri::{Webview, Wry};
use tauri_plugin_gpui::gpui::{self, div, prelude::*, App, Context, Entity, Window};

use super::{popup::PopupSink, theme};

pub(super) mod checklist;
mod merge;
mod onboarding;
mod settings;
mod settings_data;
mod workspace;
mod workspace_data;

#[cfg(test)]
mod tests;

/// Event React listens to on its own webview window.
pub const ACTION_EVENT: &str = "gpui-screen-action";

/// Set to show React's screens instead of GPUI's, to compare the two.
pub const WEB_UI_ENV: &str = "TREQ_WEB_UI";

/// What the screen shows, and the theme to draw it in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSnapshot {
  /// React's resolved theme, which can differ from the OS appearance.
  pub dark: bool,
  #[serde(flatten)]
  pub view: View,
}

/// One screen's data, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum View {
  /// No repository open: open one, or check the required tools.
  Onboarding(onboarding::Route),
  Settings(settings::Route),
  /// The workspace view's header and tab row, with the webview below.
  Workspace(Box<workspace_data::Route>),
  /// The commits and files a workspace merges, and the merge.
  MergePreview(merge::Route),
}

/// What a click on a screen asks React to do, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScreenAction {
  OpenRepo,
  OpenRemote,
  RecheckPrerequisites,
  OpenUrl {
    url: String,
  },
  /// A control's new value; `field` names it, as in `"app.theme"`.
  Field {
    field: String,
    value: serde_json::Value,
  },
  /// A slider released at `value`.
  Commit {
    field: String,
    value: serde_json::Value,
  },
  /// A button; `id` names it, as in `"settings.save"`.
  Press {
    id: String,
  },
}

/// Where a screen sends its actions.
pub type ActionSink = Rc<dyn Fn(ScreenAction, &mut App)>;

/// Everything the screens drawn from the backend ask of it; faked in tests.
pub trait Host:
  onboarding::OnboardingHost + settings_data::SettingsHost + merge::MergeHost
{
}

impl<T> Host for T where
  T: onboarding::OnboardingHost + settings_data::SettingsHost + merge::MergeHost
{
}

/// The GPUI view for whichever screen React pushed last.
pub struct Screen {
  snapshot: ScreenSnapshot,
  sink: ActionSink,
  popups: PopupSink,
  /// The window's webview, which a screen may frame part of itself around.
  webview: Option<Webview<Wry>>,
  host: Arc<dyn Host>,
  /// Screens with state of their own (text being edited, open dropdowns),
  /// kept while React keeps sending them.
  onboarding: Option<Entity<onboarding::OnboardingView>>,
  settings: Option<Entity<settings::SettingsView>>,
  workspace: Option<Entity<workspace::WorkspaceView>>,
  merge: Option<Entity<merge::MergeView>>,
}

impl Screen {
  pub fn new(
    snapshot: ScreenSnapshot,
    sink: ActionSink,
    popups: PopupSink,
    webview: Option<Webview<Wry>>,
    host: Arc<dyn Host>,
  ) -> Self {
    Self {
      snapshot,
      sink,
      popups,
      webview,
      host,
      onboarding: None,
      settings: None,
      workspace: None,
      merge: None,
    }
  }

  pub fn snapshot(&self) -> &ScreenSnapshot {
    &self.snapshot
  }

  pub fn set_snapshot(&mut self, snapshot: ScreenSnapshot, cx: &mut Context<Self>) {
    if self.snapshot != snapshot {
      self.snapshot = snapshot;
      cx.notify();
    }
  }
}

/// Context every screen draws with.
pub struct Ctx<'a> {
  pub palette: theme::Palette,
  pub sink: &'a ActionSink,
}

impl Ctx<'_> {
  /// A click handler that sends `action`.
  pub fn send(&self, action: ScreenAction) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) {
    let sink = self.sink.clone();
    move |_, _, cx| sink(action.clone(), cx)
  }
}

impl Render for Screen {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let dark = self.snapshot.dark;
    if !matches!(self.snapshot.view, View::Onboarding(_)) {
      self.onboarding = None;
    }
    if !matches!(self.snapshot.view, View::Settings(_)) {
      self.settings = None;
    }
    if !matches!(self.snapshot.view, View::Workspace(_)) {
      self.workspace = None;
    }
    if !matches!(self.snapshot.view, View::MergePreview(_)) {
      self.merge = None;
    }
    let ctx = Ctx {
      palette: theme::palette(dark),
      sink: &self.sink,
    };
    let body = match &self.snapshot.view {
      View::Onboarding(_) => match &self.onboarding {
        Some(view) => {
          view.update(cx, |view, cx| view.set_dark(dark, cx));
          view.clone().into_any_element()
        }
        None => {
          let (host, sink) = (self.host.clone(), self.sink.clone());
          let view = cx.new(|cx| onboarding::OnboardingView::new(host, sink, dark, cx));
          self.onboarding = Some(view.clone());
          view.into_any_element()
        }
      },
      View::Settings(route) => {
        let route = route.clone();
        match &self.settings {
          Some(view) => {
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let (host, sink) = (self.host.clone(), self.sink.clone());
            let view = cx.new(|cx| settings::SettingsView::new(route, host, sink, dark, cx));
            self.settings = Some(view.clone());
            view.into_any_element()
          }
        }
      }
      View::Workspace(route) => {
        let route = (**route).clone();
        match &self.workspace {
          Some(view) => {
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let (host, sink, popups) = (self.host.clone(), self.sink.clone(), self.popups.clone());
            let webview = self.webview.clone();
            let view = cx.new(|cx| {
              workspace::WorkspaceView::new(route, host, sink, dark, popups, webview, cx)
            });
            self.workspace = Some(view.clone());
            view.into_any_element()
          }
        }
      }
      View::MergePreview(route) => {
        let route = route.clone();
        match &self.merge {
          Some(view) => {
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let (host, sink) = (self.host.clone(), self.sink.clone());
            let view = cx.new(|cx| merge::MergeView::new(route, host, sink, dark, cx));
            self.merge = Some(view.clone());
            view.into_any_element()
          }
        }
      }
    };
    div()
      .size_full()
      .bg(ctx.palette.background)
      .text_color(ctx.palette.foreground)
      .font_family(theme::SANS_FONT)
      .child(body)
  }
}
