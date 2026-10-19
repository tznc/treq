//! `Onboarding`: the window with no repository open.
//!
//! Drawn from the backend directly: GPUI runs the prerequisite check and
//! opens the repository itself, then tells React (see `native`). Only the
//! remote button still goes to React, whose dialog GPUI does not draw yet.

use std::{rc::Rc, sync::Arc};

use serde::Deserialize;
use tauri_plugin_gpui::gpui::{div, prelude::*, px, Context, FontWeight, Window};

use super::{
  checklist::{self, Checklist, ToolsHost},
  ActionSink, Ctx, ScreenAction,
};
use crate::{
  core::{
    self,
    feature_preview::{self, PreviewFeature},
    PrerequisiteStatus,
  },
  gpui_shell::{
    native::{Backend, NativeEvent, ToastTone},
    theme,
    ui::{self, Button, TextSize, Variant},
  },
};

/// What React sends for this screen: nothing, as GPUI reads its own data.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Route {}

/// What the screen draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Onboarding {
  /// The second button's label ("Open via SSH" or "Open cloud workspace"),
  /// or `None` without it.
  pub remote_label: Option<String>,
  pub checklist: Checklist,
}

/// What the onboarding screen asks of the backend; faked in tests.
pub trait OnboardingHost: ToolsHost {
  fn remote_ssh_enabled(&self) -> bool;
  /// Asks for a folder, then opens it in this window.
  fn pick_repo(&self);
}

impl OnboardingHost for Backend {
  fn remote_ssh_enabled(&self) -> bool {
    self.with_db(|db| feature_preview::is_enabled(db, PreviewFeature::RemoteSsh))
  }

  fn pick_repo(&self) {
    pick_repo(self.clone());
  }
}

/// The onboarding screen and the state it loads from the backend.
pub struct OnboardingView {
  host: Arc<dyn OnboardingHost>,
  /// React's handlers, for the remote dialog.
  react: ActionSink,
  /// This view's own handler, which the drawn buttons call.
  sink: ActionSink,
  pub(super) remote_label: String,
  /// `None` until the first check returns.
  statuses: Option<Vec<PrerequisiteStatus>>,
  validating: bool,
  dark: bool,
}

impl OnboardingView {
  pub fn new(
    host: Arc<dyn OnboardingHost>,
    react: ActionSink,
    dark: bool,
    cx: &mut Context<Self>,
  ) -> Self {
    let view = cx.weak_entity();
    let sink: ActionSink = Rc::new(move |action, cx| {
      let _ = view.update(cx, |view, cx| view.handle(action, cx));
    });
    let mut this = Self {
      react,
      sink,
      remote_label: if host.remote_ssh_enabled() {
        "Open via SSH"
      } else {
        "Open cloud workspace"
      }
      .to_string(),
      statuses: None,
      validating: false,
      dark,
      host,
    };
    this.check(cx);
    this
  }

  pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.dark != dark {
      self.dark = dark;
      cx.notify();
    }
  }

  /// Looks up the tools again, off the main thread.
  fn check(&mut self, cx: &mut Context<Self>) {
    self.validating = true;
    cx.notify();
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let statuses = cx
        .background_spawn(async move { host.check_prerequisites() })
        .await;
      let _ = this.update(cx, |view, cx| {
        view.statuses = Some(statuses);
        view.validating = false;
        cx.notify();
      });
    })
    .detach();
  }

  fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    match action {
      ScreenAction::OpenRepo => self.host.pick_repo(),
      ScreenAction::RecheckPrerequisites => self.check(cx),
      ScreenAction::OpenUrl { url } => self.host.open_url(&url),
      other => (self.react)(other, cx),
    }
  }

  fn screen(&self) -> Onboarding {
    Onboarding {
      remote_label: Some(self.remote_label.clone()),
      checklist: checklist::from_statuses(
        "Before you start",
        self.statuses.as_deref(),
        self.validating,
      ),
    }
  }
}

impl Render for OnboardingView {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    let ctx = Ctx {
      palette: theme::palette(self.dark),
      sink: &self.sink,
    };
    render(&self.screen(), &ctx)
  }
}

/// Asks for a folder, then opens it in this window.
fn pick_repo(backend: Backend) {
  // Tauri calls must not run inside the GPUI click that asked for this.
  tauri::async_runtime::spawn(async move {
    let app = backend.app().clone();
    let _ = app.run_on_main_thread(move || {
      use tauri_plugin_dialog::DialogExt;
      let dialog = backend.app().dialog().file().set_title("Select Folder");
      dialog.pick_folder(move |folder| {
        let Some(path) = folder.and_then(|folder| folder.into_path().ok()) else {
          return;
        };
        let path = path.to_string_lossy().into_owned();
        tauri::async_runtime::spawn_blocking(move || open_repo_at(&backend, &path));
      });
    });
  });
}

/// Opens the repository at `path` in this window, as React's
/// `openRepositoryAtPath` does, and tells React it is open. Blocks.
pub fn open_repo_at(backend: &Backend, path: &str) {
  if let Err(err) = core::init(path) {
    backend.toast(
      ToastTone::Error,
      "Failed to open repository",
      err.to_string(),
    );
    return;
  }
  if let Err(err) = backend.with_db(|db| db.set_setting("last_opened_repo_path", path)) {
    log::warn!("failed to remember the last opened repository: {err}");
  }
  backend.set_window_repo_path(path);
  backend.notify(NativeEvent::RepoOpened {
    path: path.to_string(),
  });
  let name = path
    .rsplit('/')
    .find(|part| !part.is_empty())
    .unwrap_or(path);
  backend.toast(
    ToastTone::Success,
    "Repository Opened",
    format!("Now viewing {name}"),
  );
}

pub fn render(screen: &Onboarding, ctx: &Ctx) -> impl IntoElement {
  let p = ctx.palette;
  let header = div()
    .flex()
    .flex_col()
    .items_center()
    .gap(px(6.))
    .p_6()
    .child(
      div()
        .mb_2()
        .child(ui::icon(theme::ICON_FOLDER_OPEN, 40., p.muted_foreground)),
    )
    .child(
      ui::text(div(), TextSize::Lg)
        .font_weight(FontWeight::SEMIBOLD)
        .child("Open a Repository"),
    )
    .child(
      ui::text(div(), TextSize::Sm)
        .text_center()
        .text_color(p.muted_foreground)
        .child("Select a folder containing a Git repository to get started."),
    );

  let actions = div()
    .flex()
    .flex_col()
    .items_center()
    .gap_3()
    .px_6()
    .pb_6()
    .child(
      Button::new("onboarding-open-repo", &p)
        .w_full()
        .label("Open Repository")
        .on_click(ctx.send(ScreenAction::OpenRepo)),
    )
    .when_some(screen.remote_label.clone(), |this, label| {
      this.child(
        Button::new("onboarding-open-remote", &p)
          .variant(Variant::Outline)
          .w_full()
          // `mr-2` on the icon, on top of the button's own gap.
          .child(
            div()
              .flex()
              .mr_2()
              .child(ui::icon(theme::ICON_SERVER, 16., p.foreground)),
          )
          .label(label)
          .on_click(ctx.send(ScreenAction::OpenRemote)),
      )
    });

  div()
    .size_full()
    .flex()
    .items_center()
    .justify_center()
    .child(
      ui::card(&p).w(px(384.)).child(header).child(actions).child(
        div()
          .border_t_1()
          .border_color(p.border.opacity(0.5))
          .px_6()
          .pt_4()
          .pb_6()
          .child(checklist::render(&screen.checklist, ctx)),
      ),
    )
}
