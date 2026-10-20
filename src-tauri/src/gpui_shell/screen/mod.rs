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

pub use modal::Modal;
pub(super) use modal::ModalOpen;

use super::{
  popup::{PopupRequest, PopupSink},
  theme,
};

mod change_move;
mod changes;
mod changes_data;
mod changes_review;
mod changes_threads;
pub(super) mod checklist;
mod code;
mod code_data;
mod commits;
mod commits_data;
mod file_picker;
mod highlight;
mod logs;
mod logs_data;
mod merge;
mod modal;
mod onboarding;
mod onboarding_remote;
mod remote;
pub(super) mod remote_chrome;
mod remote_data;
mod rename_dialog;
mod settings;
mod settings_data;
mod terminal;
mod terminal_data;
mod terminal_emu;
mod terminal_lightbox;
mod terminal_send;
mod workspace;
mod workspace_data;
mod workspace_deletion;
mod workspace_dialog;
mod workspace_dialog_data;
mod workspace_modal;

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
  /// The remote dialogs open over the screen.
  #[serde(default)]
  pub remote: remote::Route,
  #[serde(flatten)]
  pub view: View,
  /// The Dashboard dialog open over the screen.
  #[serde(default)]
  pub modal: Option<workspace_modal::ModalRoute>,
  /// The remote repository the window shows; absent for a local one. The
  /// backend sends what the screens read and change in it over SSH.
  #[serde(default)]
  pub repository: Option<super::remote_repo::RemoteRepository>,
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
  onboarding::OnboardingHost
  + settings_data::SettingsHost
  + merge::MergeHost
  + terminal_data::TerminalHost
  + modal::ModalsHost
  + remote_data::RemoteHost
  + workspace_dialog_data::WorkspaceDialogHost
{
}

impl<T> Host for T where
  T: onboarding::OnboardingHost
    + settings_data::SettingsHost
    + merge::MergeHost
    + terminal_data::TerminalHost
    + modal::ModalsHost
    + remote_data::RemoteHost
    + workspace_dialog_data::WorkspaceDialogHost
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
  /// The dialog drawn over the screen, whichever it is, and its views.
  modal: Option<Modal>,
  agent_prompt: Option<Entity<modal::agent_prompt::AgentPromptView>>,
  prompt_history: Option<Entity<modal::prompt_history::PromptHistoryView>>,
  stash: Option<Entity<modal::stash::StashView>>,
  /// The remote dialogs, while one is open.
  remote: Option<Entity<remote::RemoteView>>,
  /// The dialog drawn over the screen, while React keeps it open.
  workspace_modal: Option<workspace_modal::OpenModal>,
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
      modal: None,
      agent_prompt: None,
      prompt_history: None,
      stash: None,
      remote: None,
      workspace_modal: None,
    }
  }

  /// Draws `modal` over the screen, or closes the open one for `None`. The
  /// dialog's own state (typed text, the selection) lasts while React keeps
  /// sending the same kind of dialog.
  pub fn set_modal(&mut self, modal: Option<Modal>, cx: &mut Context<Self>) {
    if self.modal == modal {
      return;
    }
    cx.set_global(ModalOpen(modal.is_some()));
    self.modal = modal;
    cx.notify();
  }

  pub fn snapshot(&self) -> &ScreenSnapshot {
    &self.snapshot
  }

  /// The remote repository's access is cut off: the window dims behind
  /// `remote-cutoff-overlay`.
  pub fn cut_off(&self) -> bool {
    match &self.snapshot.view {
      View::Workspace(route) => route.remote_chrome.as_ref().is_some_and(|r| r.cut_off),
      _ => false,
    }
  }

  pub fn palette(&self) -> super::theme::Palette {
    super::theme::palette(self.snapshot.dark)
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
    match &self.snapshot.modal {
      None => self.workspace_modal = None,
      Some(route) => match &self.workspace_modal {
        Some(open) if open.is_for(route) => open.update(route, dark, cx),
        _ => {
          let host: Arc<dyn workspace_dialog_data::WorkspaceDialogHost> = self.host.clone();
          self.workspace_modal = Some(workspace_modal::OpenModal::open(
            route,
            host,
            self.sink.clone(),
            dark,
            cx,
          ));
        }
      },
    }
    let workspace_modal = self.workspace_modal.as_ref().filter(|open| open.shown(cx));
    // The webview is a window of its own; a dialog over it must hide it.
    let covered = workspace_modal.is_some();
    let workspace_modal = workspace_modal.map(workspace_modal::OpenModal::element);
    let modal = self.modal_view(dark, cx);
    let ctx = Ctx {
      palette: theme::palette(dark),
      sink: &self.sink,
    };
    let body = match &self.snapshot.view {
      View::Onboarding(route) => {
        let route = route.clone();
        match &self.onboarding {
          Some(view) => {
            view.update(cx, |view, cx| {
              view.set_dark(dark, cx);
              view.set_route(route, cx);
            });
            view.clone().into_any_element()
          }
          None => {
            let (host, sink) = (self.host.clone(), self.sink.clone());
            let view = cx.new(|cx| {
              let mut view = onboarding::OnboardingView::new(host, sink, dark, cx);
              view.set_route(route, cx);
              view
            });
            self.onboarding = Some(view.clone());
            view.into_any_element()
          }
        }
      }
      View::Settings(route) => {
        let route = route.clone();
        match &self.settings {
          Some(view) => {
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let (host, sink, popups) = (self.host.clone(), self.sink.clone(), self.popups.clone());
            let view =
              cx.new(|cx| settings::SettingsView::new(route, host, sink, dark, popups, cx));
            self.settings = Some(view.clone());
            view.into_any_element()
          }
        }
      }
      View::Workspace(route) => {
        let route = (**route).clone();
        match &self.workspace {
          Some(view) => {
            view.update(cx, |view, cx| {
              view.set_covered(covered, cx);
              view.set_route(route, dark, cx)
            });
            view.clone().into_any_element()
          }
          None => {
            let (host, sink, popups) = (self.host.clone(), self.sink.clone(), self.popups.clone());
            let webview = self.webview.clone();
            let view = cx.new(|cx| {
              workspace::WorkspaceView::new(route, host, sink, dark, popups, webview, cx)
            });
            view.update(cx, |view, cx| view.set_covered(covered, cx));
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
    // A dialog over the whole window would leave the webview on top, so the
    // workspace stops framing it.
    let remote_open = self.snapshot.remote.is_open();
    if let Some(workspace) = &self.workspace {
      workspace.update(cx, |workspace, cx| {
        workspace.set_covered(remote_open || covered, cx)
      });
    }
    let remote = if remote_open {
      let route = self.snapshot.remote.clone();
      Some(match &self.remote {
        Some(view) => {
          view.update(cx, |view, cx| view.set_route(route, dark, cx));
          view.clone()
        }
        None => {
          let (host, sink, popups) = (self.host.clone(), self.sink.clone(), self.popups.clone());
          let view = cx.new(|cx| {
            let mut view = remote::RemoteView::new(host, sink, popups, dark);
            view.set_route(route, dark, cx);
            view
          });
          self.remote = Some(view.clone());
          view
        }
      })
    } else {
      self.remote = None;
      None
    };
    div()
      .size_full()
      .bg(ctx.palette.background)
      .text_color(ctx.palette.foreground)
      .font_family(theme::SANS_FONT)
      .child(body)
      .children(remote)
      .children(workspace_modal)
      .children(modal)
  }
}

impl Screen {
  /// The open dialog's view, kept while the same kind stays open.
  fn modal_view(&mut self, dark: bool, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
    let env = || modal::Env {
      host: self.host.clone(),
      react: self.sink.clone(),
      popups: self.popups.clone(),
    };
    let route = self.modal.clone();
    if !matches!(route, Some(Modal::AgentPrompt(_))) && self.agent_prompt.take().is_some() {
      // A tooltip the dialog showed would outlive it.
      (self.popups)(PopupRequest::HideTooltip, cx);
    }
    if !matches!(route, Some(Modal::PromptHistory(_))) {
      self.prompt_history = None;
    }
    if !matches!(route, Some(Modal::Stash(_))) {
      self.stash = None;
    }
    match route? {
      Modal::AgentPrompt(route) => Some(match &self.agent_prompt {
        Some(view) => {
          view.update(cx, |view, cx| view.set_route(route, dark, cx));
          view.clone().into_any_element()
        }
        None => {
          let env = env();
          let view = cx.new(|cx| modal::agent_prompt::AgentPromptView::new(env, route, dark, cx));
          self.agent_prompt = Some(view.clone());
          view.into_any_element()
        }
      }),
      Modal::PromptHistory(route) => Some(match &self.prompt_history {
        Some(view) => {
          view.update(cx, |view, cx| view.set_route(route, dark, cx));
          view.clone().into_any_element()
        }
        None => {
          let env = env();
          let view =
            cx.new(|cx| modal::prompt_history::PromptHistoryView::new(env, route, dark, cx));
          self.prompt_history = Some(view.clone());
          view.into_any_element()
        }
      }),
      Modal::Stash(route) => Some(match &self.stash {
        Some(view) => {
          view.update(cx, |view, cx| view.set_route(route, dark, cx));
          view.clone().into_any_element()
        }
        None => {
          let env = env();
          let view = cx.new(|cx| modal::stash::StashView::new(env, route, dark, cx));
          self.stash = Some(view.clone());
          view.into_any_element()
        }
      }),
    }
  }
}
