//! Dashboard's dialogs, drawn by GPUI over whichever screen shows.
//!
//! React keeps the state that says which dialog is open (Dashboard's
//! `unifiedDialogDefaults`, `renameTarget`, `showWorkspaceDeletion` and
//! `pendingChangeMove`) and sends it as the snapshot's `modal`. GPUI draws
//! the dialog over the whole window, sidebar included, and does the work
//! through the backend; it hands back only what React still owns: closing
//! the dialog, opening the workspace it made, and deleting a workspace.

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, Context, Div, Entity, FontWeight, Pixels, SharedString, Stateful,
};

use super::{
  change_move::{ChangeMoveDialog, ChangeMoveRoute},
  open_workspaces::OpenWorkspacesDialog,
  open_workspaces_data,
  rename_dialog::{RenameDialog, RenameRoute},
  workspace_deletion::{DeletionRoute, WorkspaceDeletion},
  workspace_dialog::WorkspaceDialog,
  workspace_dialog_data::{self, WorkspaceDialogHost},
  ActionSink, Host, ScreenAction,
};
use crate::gpui_shell::{
  theme::{self, Palette},
  ui::{self, TextSize},
};

/// The dialog React has open, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ModalRoute {
  /// `UnifiedWorkspaceDialog`.
  WorkspaceDialog(Box<workspace_dialog_data::Route>),
  /// `RenameWorkspaceDialog`.
  RenameWorkspace(RenameRoute),
  /// `WorkspaceDeletion`, the command palette's "Delete Workspace".
  WorkspaceDeletion(DeletionRoute),
  /// The `AlertDialog` that confirms moving dropped files.
  ConfirmChangeMove(ChangeMoveRoute),
  /// The sidebar's Open Workspaces dialog.
  OpenWorkspaces(open_workspaces_data::Route),
}

impl ModalRoute {
  /// Routes with the same identity update one dialog; another one opens a
  /// fresh dialog.
  fn identity(&self) -> (u8, String) {
    match self {
      ModalRoute::WorkspaceDialog(route) => (0, route.key.to_string()),
      ModalRoute::RenameWorkspace(route) => (1, route.workspace_id.to_string()),
      ModalRoute::WorkspaceDeletion(_) => (2, String::new()),
      ModalRoute::ConfirmChangeMove(route) => (3, route.files.join("\0")),
      ModalRoute::OpenWorkspaces(route) => (4, route.repo_path.clone()),
    }
  }
}

/// The open dialog's view.
pub enum Modal {
  WorkspaceDialog(Entity<WorkspaceDialog>),
  RenameWorkspace(Entity<RenameDialog>),
  WorkspaceDeletion(Entity<WorkspaceDeletion>),
  ConfirmChangeMove(Entity<ChangeMoveDialog>),
  OpenWorkspaces(Entity<OpenWorkspacesDialog>),
}

/// A dialog and the route it was opened for.
pub struct OpenModal {
  identity: (u8, String),
  pub modal: Modal,
}

impl OpenModal {
  pub fn open<V: 'static>(
    route: &ModalRoute,
    host: Arc<dyn Host>,
    sink: ActionSink,
    dark: bool,
    cx: &mut Context<V>,
  ) -> Self {
    let dialogs: Arc<dyn WorkspaceDialogHost> = host.clone();
    let modal = match route.clone() {
      ModalRoute::OpenWorkspaces(route) => {
        Modal::OpenWorkspaces(cx.new(|cx| OpenWorkspacesDialog::new(route, host, sink, dark, cx)))
      }
      ModalRoute::WorkspaceDialog(route) => {
        Modal::WorkspaceDialog(cx.new(|cx| WorkspaceDialog::new(*route, dialogs, sink, dark, cx)))
      }
      ModalRoute::RenameWorkspace(route) => {
        Modal::RenameWorkspace(cx.new(|cx| RenameDialog::new(route, dialogs, sink, dark, cx)))
      }
      ModalRoute::WorkspaceDeletion(route) => {
        Modal::WorkspaceDeletion(cx.new(|cx| WorkspaceDeletion::new(route, sink, dark, cx)))
      }
      ModalRoute::ConfirmChangeMove(route) => {
        Modal::ConfirmChangeMove(cx.new(|cx| ChangeMoveDialog::new(route, dialogs, sink, dark, cx)))
      }
    };
    Self {
      identity: route.identity(),
      modal,
    }
  }

  /// Whether `route` belongs to this dialog.
  pub fn is_for(&self, route: &ModalRoute) -> bool {
    self.identity == route.identity()
  }

  /// Passes React's newer route and theme to the open dialog.
  pub fn update<V: 'static>(&self, route: &ModalRoute, dark: bool, cx: &mut Context<V>) {
    match (&self.modal, route.clone()) {
      (Modal::WorkspaceDialog(view), ModalRoute::WorkspaceDialog(_)) => {
        view.update(cx, |view, cx| view.set_dark(dark, cx))
      }
      (Modal::RenameWorkspace(view), ModalRoute::RenameWorkspace(_)) => {
        view.update(cx, |view, cx| view.set_dark(dark, cx))
      }
      (Modal::WorkspaceDeletion(view), ModalRoute::WorkspaceDeletion(route)) => {
        view.update(cx, |view, cx| view.set_route(route, dark, cx))
      }
      (Modal::ConfirmChangeMove(view), ModalRoute::ConfirmChangeMove(route)) => {
        view.update(cx, |view, cx| view.set_route(route, dark, cx))
      }
      (Modal::OpenWorkspaces(view), ModalRoute::OpenWorkspaces(_)) => {
        view.update(cx, |view, cx| view.set_dark(dark, cx))
      }
      _ => {}
    }
  }

  /// Whether the dialog still shows; one that asked React to close it
  /// hides at once.
  pub fn shown(&self, cx: &gpui::App) -> bool {
    match &self.modal {
      Modal::WorkspaceDialog(view) => !view.read(cx).dismissed,
      Modal::RenameWorkspace(view) => !view.read(cx).dismissed,
      Modal::WorkspaceDeletion(view) => !view.read(cx).dismissed,
      Modal::ConfirmChangeMove(view) => !view.read(cx).dismissed,
      Modal::OpenWorkspaces(view) => !view.read(cx).dismissed,
    }
  }

  pub fn element(&self) -> gpui::AnyElement {
    match &self.modal {
      Modal::WorkspaceDialog(view) => view.clone().into_any_element(),
      Modal::RenameWorkspace(view) => view.clone().into_any_element(),
      Modal::WorkspaceDeletion(view) => view.clone().into_any_element(),
      Modal::ConfirmChangeMove(view) => view.clone().into_any_element(),
      Modal::OpenWorkspaces(view) => view.clone().into_any_element(),
    }
  }
}

/// Sends `Press { id }`.
pub fn press(sink: &ActionSink, id: &str, cx: &mut gpui::App) {
  sink(ScreenAction::Press { id: id.into() }, cx);
}

/// `ui/dialog.tsx`'s backdrop (`bg-black/40`, `p-4`) over the whole window,
/// and its `DialogContent` panel, `max_width` wide at most.
pub fn dialog_panel(
  id: &'static str,
  max_width: f32,
  viewport: gpui::Size<Pixels>,
  p: &Palette,
) -> (Stateful<Div>, Div) {
  let backdrop = div()
    .id(SharedString::from(format!("{id}-backdrop")))
    .w(viewport.width)
    .h(viewport.height)
    .p_4()
    .flex()
    .items_center()
    .justify_center()
    .bg(p.scrim())
    .occlude();
  let panel = div()
    .debug_selector(move || id.into())
    .w_full()
    .max_w(zpx(max_width))
    .p_6()
    .rounded(radius::LG)
    .border_1()
    .border_color(p.border.opacity(0.5))
    .bg(p.popover)
    .text_color(p.popover_foreground)
    .font_family(theme::SANS_FONT)
    .shadow_2xl();
  (backdrop, panel)
}

/// `DialogTitle`: `text-lg font-semibold leading-none`.
pub fn dialog_title(title: impl Into<SharedString>) -> Div {
  ui::text(div(), TextSize::Lg)
    .font_weight(FontWeight::SEMIBOLD)
    .child(title.into())
}

/// `DialogDescription`: `text-sm text-muted-foreground`.
pub fn dialog_description(text: impl Into<SharedString>, p: &Palette) -> Div {
  ui::text(div(), TextSize::Sm)
    .text_color(p.muted_foreground)
    .child(text.into())
}

/// `DialogHeader`: `flex flex-col space-y-1.5`.
pub fn dialog_header(title: impl Into<SharedString>, description: Div) -> Div {
  let title: SharedString = title.into();
  div()
    .flex()
    .flex_col()
    .gap_1p5()
    // `DialogTitle` is the dialog's heading.
    .child(
      div()
        .id("dialog-title")
        .role(gpui::Role::Heading)
        .aria_label(title.clone())
        .test_support()
        .child(dialog_title(title)),
    )
    .child(description)
}

/// `<Label className="text-xs">`: `font-medium leading-none`.
pub fn label_xs(text: impl Into<SharedString>) -> Div {
  ui::text(div(), TextSize::Xs)
    .font_weight(FontWeight::MEDIUM)
    .child(text.into())
}

/// A checkbox's state; `Some` is the indeterminate dash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
  On,
  Off,
  Some,
}

impl From<Check> for ui::CheckState {
  fn from(check: Check) -> Self {
    match check {
      Check::On => ui::CheckState::On,
      Check::Off => ui::CheckState::Off,
      Check::Some => ui::CheckState::Mixed,
    }
  }
}
