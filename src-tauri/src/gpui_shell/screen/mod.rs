//! Full-window screens drawn by GPUI.
//!
//! Each screen reads and changes the app's state through the backend itself
//! (a [`Host`], faked in tests). The window model routes: it presents a
//! [`ScreenSnapshot`] saying which screen shows and for what (a tab, a
//! repository, a workspace). What a screen leaves to the model (closing it,
//! navigating) goes back as [`ScreenAction`]s; what it changed itself goes
//! as native events (`native::NativeEvent`), so the model's data follows.

use crate::result_ext::ResultExt;
use std::{rc::Rc, sync::Arc};

use serde::{Deserialize, Serialize};
use tauri_plugin_gpui::gpui::{self, div, prelude::*, App, Context, Entity, Window};

pub(super) use changes_data::ChangesHost;
pub use modal::Modal;
pub use terminal::{forget_kept as forget_terminals, TERMINAL_CONTEXT};
pub(super) use workspace_data::WorkspaceHost;

use super::{
  confirm,
  popup::{PopupRequest, PopupSink},
  theme,
};

pub mod account_data;
mod agent_card;
#[doc(hidden)]
pub mod bench;
mod change_move;
mod changes;
mod changes_data;
mod changes_review;
mod changes_threads;
pub(super) mod checklist;
mod code;
pub(crate) mod code_data;
pub(super) use code_data::display_title;
mod commits;
mod commits_data;
mod environment;
mod file_browser;
mod file_browser_data;
pub(super) mod file_picker;
mod github;
mod github_data;
mod highlight;
mod integrations_cache;
pub(crate) use highlight::warm as warm_highlighting;
mod intraline;
pub(super) mod launch_data;
mod linear;
mod linear_data;
mod logs;
mod logs_data;
mod machines;
mod merge;
mod modal;
mod onboarding;
mod open_workspaces;
mod open_workspaces_data;
mod reactions;
pub(in crate::gpui_shell) mod rows;
pub(super) use onboarding::open_repo_at;
mod onboarding_remote;
mod pr_review;
mod pr_review_data;
pub(super) mod prefetch;
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
pub(super) mod workspace_data;
mod workspace_deletion;
mod workspace_dialog;
mod workspace_dialog_data;
mod workspace_modal;
pub(super) mod workspace_ops;

#[cfg(test)]
mod tests;

/// What the screen shows, and the theme to draw it in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSnapshot {
  /// The resolved theme, which can differ from the OS appearance.
  pub dark: bool,
  /// The remote dialogs open over the screen.
  #[serde(default)]
  pub remote: remote::Route,
  #[serde(flatten)]
  pub view: View,
  /// The workspace dialog open over the screen.
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
  /// The workspace view: its header, tabs and terminal pane.
  Workspace(Box<workspace_data::Route>),
  /// The commits and files a workspace merges, and the merge.
  MergePreview(merge::Route),
  /// The repository's Linear issues and projects.
  Linear(linear_data::Route),
  /// `GitHubPanel`'s issues and pull requests.
  Github(github_data::Route),
  /// A GitHub pull request's diff, reviewed and submitted to GitHub.
  PrReview(pr_review::Route),
}

/// What a click on a screen asks the window model to do, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScreenAction {
  OpenRepo,
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
  + pr_review_data::PrReviewHost
  + terminal_data::TerminalHost
  + modal::ModalsHost
  + remote_data::RemoteHost
  + workspace_dialog_data::WorkspaceDialogHost
  + linear_data::LinearHost
  + github_data::GithubHost
  + open_workspaces_data::OpenWorkspacesHost
{
}

impl<T> Host for T where
  T: onboarding::OnboardingHost
    + settings_data::SettingsHost
    + merge::MergeHost
    + pr_review_data::PrReviewHost
    + terminal_data::TerminalHost
    + modal::ModalsHost
    + remote_data::RemoteHost
    + workspace_dialog_data::WorkspaceDialogHost
    + linear_data::LinearHost
    + github_data::GithubHost
    + open_workspaces_data::OpenWorkspacesHost
{
}

/// The GPUI view for whichever screen the model presented last.
pub struct Screen {
  snapshot: ScreenSnapshot,
  sink: ActionSink,
  /// The window model, for what the screen does not run itself.
  react: ActionSink,
  popups: PopupSink,
  host: Arc<dyn Host>,
  /// Screens with state of their own (text being edited, open dropdowns),
  /// kept while the model keeps presenting them.
  onboarding: Option<Entity<onboarding::OnboardingView>>,
  settings: Option<Entity<settings::SettingsView>>,
  workspace: Option<Entity<workspace::WorkspaceView>>,
  /// The terminal pane, while another screen replaces the workspace view:
  /// its terminals keep running and show again when the workspace does.
  terminal: Option<Entity<terminal::TerminalPane>>,
  merge: Option<Entity<merge::MergeView>>,
  linear: Option<Entity<linear::LinearView>>,
  github: Option<Entity<github::GithubView>>,
  pr_review: Option<Entity<pr_review::PrReviewView>>,
  /// The dialog drawn over the screen, whichever it is, and its views.
  modal: Option<Modal>,
  agent_prompt: Option<Entity<modal::agent_prompt::AgentPromptView>>,
  prompt_history: Option<Entity<modal::prompt_history::PromptHistoryView>>,
  stash: Option<Entity<modal::stash::StashView>>,
  /// The remote dialogs, while one is open.
  remote: Option<Entity<remote::RemoteView>>,
  /// The dialog drawn over the screen, while the model keeps it open.
  workspace_modal: Option<workspace_modal::OpenModal>,
  /// Cmd+P's "Jump to File", while the window model keeps it open.
  file_picker: Option<(file_picker::Scope, Entity<file_picker::FilePicker>)>,
  _file_picker: Option<gpui::Subscription>,
  /// The window-wide dialog (palette, shortcuts sheet, workspace picker,
  /// branch switcher), drawn over everything else.
  overlay: Option<Entity<crate::gpui_shell::overlay::Overlay>>,
  /// What had focus before the overlay opened, given it back on close.
  focus_before_overlay: Option<gpui::FocusHandle>,
  /// Whether [`Self::focus_before_overlay`] was taken for the open overlay.
  overlay_focus_taken: bool,
  /// The open confirmation (the palette's Delete Workspace).
  confirm: Option<confirm::Pending<Self>>,
  /// Draws the screen again each minute, for its relative times ("3
  /// minutes ago", "Today"), which a cached screen would otherwise keep.
  _clock: gpui::Task<()>,
}

/// How often the screen's relative times are drawn again.
const CLOCK: std::time::Duration = std::time::Duration::from_secs(60);

impl Screen {
  pub fn new(
    snapshot: ScreenSnapshot,
    react: ActionSink,
    popups: PopupSink,
    host: Arc<dyn Host>,
    cx: &mut Context<Self>,
  ) -> Self {
    // What the screen runs itself, the rest going on to the model.
    let react_for_self = react.clone();
    let screen = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      if !Screen::runs(&action) {
        return react(action, cx);
      }
      let (screen, react) = (screen.clone(), react.clone());
      cx.defer(move |cx| {
        let ran = screen
          .update(cx, |screen, cx| screen.run(&action, cx))
          .unwrap_or(false);
        if !ran {
          react(action, cx);
        }
      });
    });
    let clock = cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(CLOCK).await;
      if this.update(cx, |_, cx| cx.notify()).is_err() {
        break;
      }
    });
    Self {
      snapshot,
      sink,
      react: react_for_self,
      popups,
      host,
      onboarding: None,
      settings: None,
      workspace: None,
      terminal: None,
      merge: None,
      linear: None,
      github: None,
      pr_review: None,
      modal: None,
      agent_prompt: None,
      prompt_history: None,
      stash: None,
      remote: None,
      workspace_modal: None,
      file_picker: None,
      _file_picker: None,
      overlay: None,
      focus_before_overlay: None,
      overlay_focus_taken: false,
      confirm: None,
      _clock: clock,
    }
  }

  /// Shows `snapshot`'s dialog over the screen, keeping what was typed when
  /// it is the same dialog, or closes it for `None`. `host` and `sink` are
  /// what a new dialog reads through and reports to.
  pub fn set_overlay(
    &mut self,
    snapshot: Option<crate::gpui_shell::OverlaySnapshot>,
    host: Arc<dyn crate::gpui_shell::overlay::OverlayHost>,
    sink: ActionSink,
    cx: &mut Context<Self>,
  ) {
    use crate::gpui_shell::overlay::Overlay;
    let Some(snapshot) = snapshot else {
      if self.overlay.take().is_some() {
        cx.notify();
      }
      return;
    };
    let same = self
      .overlay
      .as_ref()
      .filter(|o| o.read(cx).view().same_dialog(&snapshot.view))
      .cloned();
    match same {
      Some(overlay) => overlay.update(cx, |o, cx| o.set_snapshot(snapshot, cx)),
      None => self.overlay = Some(cx.new(|cx| Overlay::new(snapshot, host, sink, cx))),
    }
    cx.notify();
  }

  /// Notes what had focus when an overlay opens, and gives it back when
  /// the last one closes.
  fn track_overlay_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    match (&self.overlay, self.overlay_focus_taken) {
      (Some(_), false) => {
        self.focus_before_overlay = window.focused(cx);
        self.overlay_focus_taken = true;
      }
      (None, true) => {
        self.overlay_focus_taken = false;
        if let Some(focus) = self.focus_before_overlay.take() {
          focus.focus(window, cx);
        }
      }
      _ => {}
    }
  }

  /// Opens "Jump to File" on `scope`, or closes it for `None`. A pick goes
  /// out as `filePicker.select` with the checkout-relative path, a dismissal
  /// as `filePicker.close`.
  pub fn set_file_picker(&mut self, scope: Option<file_picker::Scope>, cx: &mut Context<Self>) {
    if self.file_picker.as_ref().map(|(s, _)| s) == scope.as_ref() {
      return;
    }
    self._file_picker = None;
    self.file_picker = scope.map(|scope| {
      let host: Arc<dyn code_data::CodeHost> = self.host.clone();
      let dark = self.snapshot.dark;
      let picker = cx.new(|cx| file_picker::FilePicker::new(host, scope.clone(), dark, cx));
      let sink = self.sink.clone();
      self._file_picker = Some(cx.subscribe(
        &picker,
        move |_, _, event: &file_picker::FilePickerEvent, cx| {
          let action = match event {
            file_picker::FilePickerEvent::Selected(path) => ScreenAction::Field {
              field: "filePicker.select".into(),
              value: path.clone().into(),
            },
            file_picker::FilePickerEvent::Dismissed => ScreenAction::Press {
              id: "filePicker.close".into(),
            },
          };
          sink(action, cx);
        },
      ));
      (scope, picker)
    });
    cx.notify();
  }

  /// Draws `modal` over the screen, or closes the open one for `None`. The
  /// dialog's own state (typed text, the selection) lasts while the model
  /// keeps presenting the same kind of dialog.
  pub fn set_modal(&mut self, modal: Option<Modal>, cx: &mut Context<Self>) {
    if self.modal == modal {
      return;
    }
    self.modal = modal;
    cx.notify();
  }

  pub fn snapshot(&self) -> &ScreenSnapshot {
    &self.snapshot
  }

  /// The screen as the shell lays it out, cached: a frame another view
  /// asks for (the sidebar's hover or spinning status dot) reuses the last
  /// drawing instead of rendering the whole screen again. The screen and
  /// the views inside it notify themselves for everything they draw.
  pub fn element(this: &Entity<Self>) -> impl IntoElement {
    this
      .clone()
      .cached(gpui::StyleRefinement::default().size_full())
  }

  /// Whether the screen may run `action` itself ([`Screen::run`]).
  fn runs(action: &ScreenAction) -> bool {
    matches!(action, ScreenAction::Field { field, .. } if field == "workspaceDeletion.delete")
  }

  /// Runs what Dashboard's handlers ran for the screen's dialogs; false
  /// leaves `action` to React.
  fn run(&mut self, action: &ScreenAction, cx: &mut Context<Self>) -> bool {
    let ScreenAction::Field { field, value } = action else {
      return false;
    };
    match (field.as_str(), value.as_i64(), &self.snapshot.modal) {
      (
        "workspaceDeletion.delete",
        Some(id),
        Some(workspace_modal::ModalRoute::WorkspaceDeletion(route)),
      ) if self.host.acts_locally(&route.repo_path) => {
        // The dialog closes first, as `handleDelete` runs after it.
        (self.react_sink())(
          ScreenAction::Press {
            id: "workspaceDeletion.close".into(),
          },
          cx,
        );
        self.delete_workspace(route.repo_path.clone(), id, cx);
        true
      }
      _ => false,
    }
  }

  fn confirm_slot(screen: &mut Self) -> &mut Option<confirm::Pending<Self>> {
    &mut screen.confirm
  }

  /// `handleDelete`: asks, deletes and closes the workspace's terminals.
  fn delete_workspace(&mut self, repo_path: String, id: i64, cx: &mut Context<Self>) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let found = cx
        .background_spawn(async move {
          code_data::CodeHost::workspaces(&*host, &repo_path)
            .ok()?
            .into_iter()
            .find(|w| w.id == id)
        })
        .await;
      let Some(workspace) = found else {
        return;
      };
      let workspace = workspace_ops::Removed::of(&workspace);
      this
        .update(cx, |screen, cx| {
          let question = workspace_ops::delete_question(&workspace);
          confirm::ask(
            &mut screen.confirm,
            question,
            move |screen, cx| screen.delete_confirmed(workspace, cx),
            cx,
          );
        })
        .ignore_closed();
    })
    .detach();
  }

  fn delete_confirmed(&mut self, workspace: workspace_ops::Removed, cx: &mut Context<Self>) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let path = workspace.path.clone();
      let gone = cx
        .background_spawn(async move { workspace_ops::delete(&*host, &workspace) })
        .await;
      if gone {
        this
          .update(cx, |screen, cx| screen.close_terminals(path, cx))
          .ignore_closed();
      }
    })
    .detach();
  }

  /// Reads ahead what showing workspace `route` would read, as the pointer
  /// rests on its row in the sidebar. Nothing on screen changes.
  pub fn prefetch_workspace(&mut self, route: workspace_data::Route, cx: &mut Context<Self>) {
    if let Some(workspace) = &self.workspace {
      workspace.update(cx, |workspace, cx| workspace.prefetch(route, cx));
    }
  }

  /// Closes the terminals of a workspace GPUI removed.
  pub fn close_terminals(&mut self, path: String, cx: &mut Context<Self>) {
    self.terminal_command("closeWorkspace", Some(path), cx);
  }

  /// Gives the workspace's terminal pane a command of kind `kind`, as
  /// React's `terminalPane()` handle does.
  pub fn terminal_command(&mut self, kind: &str, dir: Option<String>, cx: &mut Context<Self>) {
    if let Some(workspace) = &self.workspace {
      workspace.update(cx, |workspace, cx| {
        workspace.terminal_command(kind, dir, cx)
      });
    } else if let Some(pane) = &self.terminal {
      pane.update(cx, |pane, cx| pane.run_later(kind, dir, cx));
    }
  }

  /// Where the screen's own actions go: through [`Screen::run`] to React.
  fn react_sink(&self) -> ActionSink {
    self.react.clone()
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
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let dark = self.snapshot.dark;
    self.track_overlay_focus(window, cx);
    let confirm = confirm::render(Self::confirm_slot, self, window, &theme::palette(dark), cx);
    if !matches!(self.snapshot.view, View::Onboarding(_)) {
      self.onboarding = None;
    }
    if !matches!(self.snapshot.view, View::Settings(_)) {
      self.settings = None;
    }
    if !matches!(self.snapshot.view, View::Workspace(_)) {
      if let Some(workspace) = self.workspace.take() {
        if let Some(pane) = workspace.update(cx, |view, cx| view.take_terminal(cx)) {
          self.terminal = Some(pane);
        }
      }
    }
    if !matches!(self.snapshot.view, View::MergePreview(_)) {
      self.merge = None;
    }
    if !matches!(self.snapshot.view, View::Linear(_)) {
      self.linear = None;
    }
    if !matches!(self.snapshot.view, View::Github(_)) && self.github.take().is_some() {
      // A tooltip the view showed would outlive it.
      (self.popups)(PopupRequest::HideTooltip, cx);
    }
    if !matches!(self.snapshot.view, View::PrReview(_)) {
      self.pr_review = None;
    }
    match &self.snapshot.modal {
      None => self.workspace_modal = None,
      Some(route) => match &self.workspace_modal {
        Some(open) if open.is_for(route) => open.update(route, dark, cx),
        _ => {
          self.workspace_modal = Some(workspace_modal::OpenModal::open(
            route,
            self.host.clone(),
            self.sink.clone(),
            dark,
            cx,
          ));
        }
      },
    }
    let workspace_modal = self.workspace_modal.as_ref().filter(|open| open.shown(cx));
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
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let (host, sink, popups) = (self.host.clone(), self.sink.clone(), self.popups.clone());
            let view =
              cx.new(|cx| workspace::WorkspaceView::new(route, host, sink, dark, popups, cx));
            if let Some(pane) = self.terminal.take() {
              view.update(cx, |view, cx| view.adopt_terminal(pane, cx));
            }
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
      View::Linear(route) => {
        let route = route.clone();
        match &self.linear {
          Some(view) => {
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let (host, sink, popups) = (self.host.clone(), self.sink.clone(), self.popups.clone());
            let view = cx.new(|cx| linear::LinearView::new(route, host, sink, dark, popups, cx));
            self.linear = Some(view.clone());
            view.into_any_element()
          }
        }
      }
      View::Github(route) => {
        let route = route.clone();
        match &self.github {
          Some(view) => {
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let host: Arc<dyn github_data::GithubHost> = self.host.clone();
            let (sink, popups) = (self.sink.clone(), self.popups.clone());
            let view = cx.new(|cx| github::GithubView::new(route, host, sink, dark, popups, cx));
            self.github = Some(view.clone());
            view.into_any_element()
          }
        }
      }
      View::PrReview(route) => {
        let route = route.clone();
        match &self.pr_review {
          Some(view) => {
            view.update(cx, |view, cx| view.set_route(route, dark, cx));
            view.clone().into_any_element()
          }
          None => {
            let (host, sink, popups) = (self.host.clone(), self.sink.clone(), self.popups.clone());
            let view =
              cx.new(|cx| pr_review::PrReviewView::new(route, host, sink, dark, popups, cx));
            self.pr_review = Some(view.clone());
            view.into_any_element()
          }
        }
      }
    };
    let remote_open = self.snapshot.remote.is_open();
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
            // The cloud card draws Rust's account.
            cx.observe_global::<account_data::Account>(|_, cx| cx.notify())
              .detach();
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
      // A mouse-down blurs the focused input, as in a browser; an input
      // under the pointer takes focus again in the bubble phase.
      .capture_any_mouse_down(|_, window, cx| window.blur(cx))
      .child(body)
      .children(remote)
      .children(workspace_modal)
      .children(modal)
      .children(self.file_picker.as_ref().map(|(_, picker)| picker.clone()))
      .children(self.overlay.clone())
      .children(confirm)
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

/// A scoped thread's result, its panic carried on to the caller.
pub(super) fn joined<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
  handle
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}
