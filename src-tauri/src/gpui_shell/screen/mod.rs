//! Full-window screens drawn by GPUI in place of the webview.
//!
//! As with the sidebar, React still owns each screen's data and handlers. It
//! pushes a [`ScreenSnapshot`] with the `gpui_screen_update` command while a
//! ported screen is on screen, and GPUI draws it where the webview would be,
//! leaving the webview out of the frame (hidden, with React still running in
//! it). Clicks go back to the window's webview as `gpui-screen-action`
//! events, which React carries out with the handlers its own UI uses.
//!
//! React keeps drawing its UI too, so pushing `null` (for a screen or dialog
//! GPUI does not draw yet) brings the webview back with React's version.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tauri_plugin_gpui::gpui::{self, div, prelude::*, App, Context, Entity, Window};

use super::theme;

pub(super) mod checklist;
mod onboarding;
mod settings;

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
  Onboarding(onboarding::Onboarding),
  Settings(Box<settings::Settings>),
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

/// The GPUI view for whichever screen React pushed last.
pub struct Screen {
  snapshot: ScreenSnapshot,
  sink: ActionSink,
  /// Screens with state of their own (text being edited, open dropdowns),
  /// kept while React keeps sending them.
  settings: Option<Entity<settings::SettingsView>>,
}

impl Screen {
  pub fn new(snapshot: ScreenSnapshot, sink: ActionSink) -> Self {
    Self {
      snapshot,
      sink,
      settings: None,
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
    if !matches!(self.snapshot.view, View::Settings(_)) {
      self.settings = None;
    }
    let ctx = Ctx {
      palette: theme::palette(dark),
      sink: &self.sink,
    };
    let body = match &self.snapshot.view {
      View::Onboarding(screen) => onboarding::render(screen, &ctx).into_any_element(),
      View::Settings(screen) => {
        let screen = (**screen).clone();
        match &self.settings {
          Some(view) => {
            view.update(cx, |view, cx| view.set_snapshot(screen, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let sink = self.sink.clone();
            let view = cx.new(|_| settings::SettingsView::new(screen, dark, sink));
            self.settings = Some(view.clone());
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
