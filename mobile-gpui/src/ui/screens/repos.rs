//! Repository picker for the connected host.

use gpui_kit::{
  component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    Sizable,
  },
  *,
};
use treq_lib::core::remote::{RemoteRepoProbe, TreqCommandRequest};

use super::workspaces;
use crate::{
  backend, remote, store,
  ui::{load, nav, widgets, Location},
};

pub fn open(cx: &mut App) {
  nav::push("Repositories", Location::default(), cx, |window, cx| {
    cx.new(|cx| ReposScreen::new(window, cx))
  });
}

pub struct ReposScreen {
  path: Entity<InputState>,
  probing: bool,
  error: Option<String>,
  scroll: ScrollHandle,
}

impl ReposScreen {
  fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
    let path = cx.new(|cx| InputState::new(window, cx).placeholder("~/src/my-repo"));
    cx.subscribe(&path, |this, _, event: &InputEvent, cx| {
      if let InputEvent::PressEnter { .. } = event {
        this.inspect(cx);
      }
    })
    .detach();
    Self {
      path,
      probing: false,
      error: None,
      scroll: ScrollHandle::new(),
    }
  }

  fn inspect(&mut self, cx: &mut Context<Self>) {
    let path = self.path.read(cx).value().trim().to_string();
    if path.is_empty() || self.probing {
      return;
    }
    self.open_repository(store::canonicalize_remote_path(&path), cx);
  }

  fn open_repository(&mut self, path: String, cx: &mut Context<Self>) {
    self.probing = true;
    self.error = None;
    let repo = path.clone();
    load::run(
      cx,
      async move { remote::read::<RemoteRepoProbe>(TreqCommandRequest::ProbeRepo { repo }).await },
      move |this, result, cx| {
        this.probing = false;
        match result {
          Ok(probe) if probe.exists && probe.is_repo => {
            if let Ok(endpoint) = backend::get().endpoint() {
              store::upsert_saved_repository(
                &endpoint.id,
                store::endpoint_generation(&endpoint),
                &path,
              );
            }
            workspaces::open(path, cx);
          }
          Ok(_) => this.error = Some(format!("No repository found at {path} on this host.")),
          Err(error) => this.error = Some(error),
        }
      },
    );
  }
}

impl Render for ReposScreen {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let saved = backend::get()
      .endpoint()
      .map(|e| store::saved_repositories(&e.id, store::endpoint_generation(&e)))
      .unwrap_or_default();
    let saved_list = if saved.is_empty() {
      widgets::card(cx).child(widgets::centered_message("No saved repositories yet.", cx))
    } else {
      widgets::card(cx).children(saved.into_iter().enumerate().map(|(i, repo)| {
        let path = repo.canonical_remote_path.clone();
        widgets::row(("repo", i), repo.display_name.clone(), None, cx)
          .on_click(cx.listener(move |this, _, _, cx| this.open_repository(path.clone(), cx)))
      }))
    };
    div()
      .id("repos")
      .size_full()
      .overflow_y_scroll()
      .track_scroll(&self.scroll)
      .child(
        widgets::page()
          .child(widgets::section("Repositories on this host", cx).child(saved_list))
          .child(
            widgets::section("Open by path", cx).child(
              widgets::card(cx)
                .p_4()
                .gap_3()
                .child(Input::new(&self.path).large())
                .child(
                  h_flex().child(
                    Button::new("inspect")
                      .primary()
                      .large()
                      .label(if self.probing {
                        "Inspecting…"
                      } else {
                        "Inspect repository"
                      })
                      .loading(self.probing)
                      .on_click(cx.listener(|this, _, _, cx| this.inspect(cx))),
                  ),
                )
                .children(self.error.clone().map(|e| widgets::error_text(e, cx))),
            ),
          ),
      )
  }
}
